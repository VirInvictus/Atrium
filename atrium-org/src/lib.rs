// SPDX-License-Identifier: MIT
//! Org-mode projection for the Atrium task manager (Phase 16,
//! extracted into its own crate at v0.9.0).
//!
//! Provides the hand-rolled parser + emitter, the one-shot importer,
//! the project → `.org` writer, and the auto-debounced `VaultWriter`
//! task that hooks into atrium-core's worker via the
//! [`atrium_core::VaultDirtyNotifier`] trait.
//!
//! The crate is split into:
//!
//! - [`org`] — parser, emitter, importer, writer. The pure
//!   functional surface; no tokio task lives here. Used directly
//!   by `atrium-cli`'s `import org` / `export org` subcommands.
//! - [`vault_writer`] — background task that turns
//!   `ProjectDirty(project_id)` notifications into debounced
//!   `.org` file writes.
//! - [`vault_watcher`] — `notify`-backed watcher that picks up
//!   external edits and applies them to the DB through the
//!   worker handle.
//! - [`self_write`] — the [`RecentWrites`] set shared between
//!   writer and watcher to break the self-write echo.
//!
//! For write-only callers (the CLI), [`spawn_org_vault`] is the
//! one-call entry point. For the full two-way GUI loop,
//! [`spawn_vault_loop`] returns a [`VaultConfig`] up front so the
//! worker can spawn with the vault hook installed; the
//! [`VaultLoopHandle`] holds the shared state and attaches the
//! watcher half once the [`WorkerHandle`] exists.
//!
//! [`VaultEvent`]s flow back from both halves so the GUI can
//! surface conflict backups and parse failures as toasts.
//!
//! No third-party Org crate. `orgize` and `starsector` were both
//! surveyed at Phase 16 and rejected as dormant — see CLAUDE.md's
//! dependency-discipline section.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use atrium_core::db::read_pool::ReadPool;
use atrium_core::{VaultConfig, WorkerHandle};
use tokio::sync::mpsc;

pub mod org;
pub mod rrule_cookie;
pub mod self_write;
pub mod sidecar;
pub mod vault_watcher;
pub mod vault_writer;

pub use rrule_cookie::{
    cookie_matches_rrule, org_repeater_to_rrule, rrule_to_org_cookie, rrule_to_org_repeater,
};
pub use self_write::RecentWrites;
pub use sidecar::{Sidecar, read_sidecar, sidecar_path, write_sidecar};
pub use vault_watcher::{VaultWatcher, spawn_vault_watcher, spawn_vault_watcher_with_events};
pub use vault_writer::{
    OrgVaultNotifier, VaultWriteRequest, VaultWriter, spawn_vault_writer,
    spawn_vault_writer_with_events, spawn_vault_writer_with_recent,
};

/// Operational events the writer + watcher surface back to the
/// caller. The GUI binds these to toasts (`ConflictBackup` /
/// `ParseFailed`); the CLI typically ignores them.
///
/// Doesn't carry happy-path notifications — successful writes /
/// applied diffs land in `tracing::trace` only.
#[derive(Debug, Clone)]
pub enum VaultEvent {
    /// The writer detected an external edit (file mtime not in
    /// `RecentWrites`) and snapshotted the file to `backup` before
    /// the atomic-overwrite proceeded. The user's edit is
    /// recoverable from the `.atrium.bak.*` sibling. Spec §7.3.3
    /// rule 5 — last-writer-wins; the loser is preserved.
    ConflictBackup { source: PathBuf, backup: PathBuf },

    /// The watcher hit a parse error on a vault file. The DB
    /// version is preserved and sync is paused for that file
    /// until it parses cleanly again — repeated bad saves don't
    /// re-toast on every event. The first failure after a pause-
    /// or-recovery transition is what surfaces.
    ParseFailed { source: PathBuf, error: String },

    /// A previously-paused file parsed cleanly and is back in
    /// sync. Surfaced once per pause→clean transition so the
    /// user knows the watcher is live again on that file.
    ParseRecovered { source: PathBuf },

    /// A vault file disappeared (the user `rm`ed it, or moved it
    /// out of the vault). Per spec §3.5 the DB is canonical, so
    /// Atrium retains the project's tasks; the GUI surfaces a
    /// toast so the user knows the projection is now stale on
    /// disk. The next project flush recreates the file.
    FileRemoved { source: PathBuf },

    /// A headline's SCHEDULED cookie disagrees with the canonical
    /// `:RRULE:` property in its drawer. Per spec §7.3.3 rule 3,
    /// `:RRULE:` wins — DB stays canonical, the watcher rewrites
    /// the file so the cookie matches. The user's cookie edit
    /// gets reverted; the toast tells them so.
    RruleDiverged {
        source: PathBuf,
        title: String,
        cookie: String,
        rrule: String,
    },

    /// v0.16.0 — the watcher saw a TODO keyword on a headline
    /// that isn't in any of the vault's configured `[[todo_sequences]]`
    /// sets. The keyword is preserved verbatim via
    /// `task.orig_keyword` (graceful degradation; never destroy
    /// data) but the GUI may want to prompt the user to add it
    /// to the workflow set so future occurrences map cleanly.
    /// Surfaces once per (file, keyword) on the first sighting;
    /// subsequent same-keyword edits stay quiet.
    UnknownKeyword { source: PathBuf, keyword: String },
}

/// Write-only Org vault setup. Spins up a [`VaultWriter`] against
/// `root` + `pool` and returns a [`VaultConfig`] ready to pass into
/// [`atrium_core::spawn_worker_with_vault`]. No watcher; external
/// edits don't flow back into the DB. Used by the CLI and by tests
/// that don't care about two-way sync.
pub fn spawn_org_vault(root: PathBuf, pool: ReadPool) -> VaultConfig {
    let notifier = spawn_vault_writer(root, pool);
    VaultConfig {
        notifier: Arc::new(notifier),
    }
}

/// Two-way Org vault setup (Phase 17 / GUI). Builds the writer-side
/// wiring up front so the worker can spawn with the vault hook
/// installed; returns a [`VaultLoopHandle`] that the caller passes
/// the resulting [`WorkerHandle`] back into to attach the watcher
/// half. The third return is the [`VaultEvent`] receiver, which the
/// GUI binds to toasts.
///
/// Wire order:
///
/// ```ignore
/// let (vault_config, vault_loop, events_rx) =
///     atrium_org::spawn_vault_loop(vault_root, pool.clone());
///
/// let (handle, changes_rx, library_rx) =
///     atrium_core::spawn_worker_with_vault(conn, Some(vault_config));
///
/// let _watcher = vault_loop.attach_watcher(handle.clone())?;
/// // bind events_rx to the GUI's toast surface
/// ```
pub fn spawn_vault_loop(
    root: PathBuf,
    pool: ReadPool,
) -> (
    VaultConfig,
    VaultLoopHandle,
    mpsc::UnboundedReceiver<VaultEvent>,
) {
    let recent_writes = Arc::new(RwLock::new(RecentWrites::new()));
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let notifier = spawn_vault_writer_with_events(
        root.clone(),
        pool.clone(),
        recent_writes.clone(),
        Some(events_tx.clone()),
    );
    let vault_config = VaultConfig {
        notifier: Arc::new(notifier),
    };
    let loop_handle = VaultLoopHandle {
        root,
        pool,
        recent_writes,
        events_tx,
    };
    (vault_config, loop_handle, events_rx)
}

/// Half-finished vault loop returned by [`spawn_vault_loop`]. Holds
/// the shared state the writer task is already using; the caller
/// passes the [`WorkerHandle`] (returned by `spawn_worker_with_vault`)
/// into [`Self::attach_watcher`] to spawn the watcher half on the
/// same `RecentWrites` set + event channel.
pub struct VaultLoopHandle {
    root: PathBuf,
    pool: ReadPool,
    recent_writes: Arc<RwLock<RecentWrites>>,
    events_tx: mpsc::UnboundedSender<VaultEvent>,
}

impl VaultLoopHandle {
    /// Spawn the watcher half. Returns the watcher's
    /// [`tokio::task::JoinHandle`] — let it drop to keep the watcher
    /// running for the runtime's lifetime, or `await` it for clean
    /// shutdown in tests.
    pub fn attach_watcher(
        self,
        worker_handle: WorkerHandle,
    ) -> Result<tokio::task::JoinHandle<()>, notify::Error> {
        spawn_vault_watcher_with_events(
            self.root,
            worker_handle,
            self.pool,
            self.recent_writes,
            Some(self.events_tx),
        )
    }

    /// Clone of the shared `RecentWrites` set the writer +
    /// watcher both consult to suppress self-induced echoes.
    /// Surfaced (v0.13.x) so an out-of-band writer — the
    /// fresh-vault seed in the GUI binary's boot path — can
    /// register the files it writes via `write_project_to_vault`
    /// before the watcher's debounce sees them. Without this,
    /// the watcher treats every seed file as an external edit
    /// (it isn't in the set) and the writer's pre-flush conflict
    /// check then backs each file up to `<file>.atrium.bak.<UTC>`
    /// on the first project-dirty notification — surfacing as a
    /// flood of spurious "vault conflict" warnings on first boot.
    ///
    /// The Arc-RwLock-RecentWrites discipline is documented in
    /// `self_write.rs`. Holding the lock for the duration of a
    /// `record_with_mtime` call is fine; both the writer and
    /// watcher take it briefly per event.
    pub fn recent_writes(&self) -> std::sync::Arc<std::sync::RwLock<RecentWrites>> {
        self.recent_writes.clone()
    }
}
