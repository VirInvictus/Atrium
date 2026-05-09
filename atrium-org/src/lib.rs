// SPDX-License-Identifier: MIT
//! Org-mode projection for the Atrium task manager (Phase 16,
//! extracted into its own crate at v0.9.0).
//!
//! Provides the hand-rolled parser + emitter, the one-shot importer,
//! the project → `.org` writer, and the auto-debounced `VaultWriter`
//! task that hooks into atrium-core's worker via the
//! [`atrium_core::VaultDirtyNotifier`] trait.
//!
//! The crate is split into two modules:
//!
//! - [`org`] — parser, emitter, importer, writer. The pure
//!   functional surface; no tokio task lives here. Used directly
//!   by `atrium-cli`'s `import org` / `export org` subcommands.
//! - [`vault_writer`] — the background task that turns
//!   `ProjectDirty(project_id)` notifications into debounced
//!   `.org` file writes. Used by the GTK binary's boot path.
//!
//! Convenience builder [`spawn_org_vault`] gives the GUI / CLI a
//! single entry point: pass a vault root and a [`ReadPool`], get
//! back a [`VaultConfig`] ready to thread into
//! [`atrium_core::spawn_worker_with_vault`].
//!
//! No third-party Org crate. `orgize` and `starsector` were both
//! surveyed at Phase 16 and rejected as dormant — see CLAUDE.md's
//! dependency-discipline section. The hand-roll satisfies the
//! "preserve unknown constructs verbatim" rule (spec §7.3.3 rule 1)
//! by capturing every unrecognised line into the task's
//! `unknown_lines` field and re-emitting on write.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use atrium_core::db::read_pool::ReadPool;
use atrium_core::{VaultConfig, WorkerHandle};

pub mod org;
pub mod self_write;
pub mod vault_watcher;
pub mod vault_writer;

pub use self_write::RecentWrites;
pub use vault_watcher::{VaultWatcher, spawn_vault_watcher};
pub use vault_writer::{
    OrgVaultNotifier, VaultWriteRequest, VaultWriter, spawn_vault_writer,
    spawn_vault_writer_with_recent,
};

/// One-shot builder: spin up an Org `VaultWriter` against `root` +
/// `pool` and return a [`VaultConfig`] ready to pass into
/// [`atrium_core::spawn_worker_with_vault`].
///
/// The writer task lives on the current tokio runtime for the
/// lifetime of the worker. Drops cleanly when the worker drops.
///
/// **Write-only — no watcher.** Use [`spawn_org_vault_with_watcher`]
/// for the full Phase 17 two-way sync. This entry point stays
/// available for callers that want write-only behaviour (the v0.8.0
/// shape; tests).
pub fn spawn_org_vault(root: PathBuf, pool: ReadPool) -> VaultConfig {
    let notifier = spawn_vault_writer(root, pool);
    VaultConfig {
        notifier: Arc::new(notifier),
    }
}

/// Phase 17 entry point: spin up both the [`VaultWriter`] and the
/// [`VaultWatcher`] sharing one [`RecentWrites`] set so the writer's
/// own filesystem changes don't echo back through the watcher.
///
/// Returns the [`VaultConfig`] (pass to
/// [`atrium_core::spawn_worker_with_vault`]) and the watcher's
/// [`tokio::task::JoinHandle`]. Tests typically `await` the handle
/// for clean shutdown; the GUI just lets it run.
///
/// `worker_handle` must be the same handle the worker returns, so
/// the watcher can submit writes through it. Wire order:
///
/// ```ignore
/// // 1. Open DB + read pool.
/// let conn = atrium_core::db::open(&db_path)?;
/// let pool = ReadPool::new(&db_path, 4);
///
/// // 2. Build the writer + recent-writes set.
/// let recent = std::sync::Arc::new(std::sync::RwLock::new(RecentWrites::new()));
/// let notifier = atrium_org::spawn_vault_writer_with_recent(
///     vault_root.clone(), pool.clone(), recent.clone());
/// let vault_config = atrium_core::VaultConfig {
///     notifier: std::sync::Arc::new(notifier),
/// };
///
/// // 3. Spawn the worker with the vault hook.
/// let (handle, changes_rx, library_rx) =
///     atrium_core::spawn_worker_with_vault(conn, Some(vault_config));
///
/// // 4. Spawn the watcher with the same recent-writes set.
/// let watcher = atrium_org::spawn_vault_watcher(
///     vault_root, handle.clone(), pool, recent)?;
/// ```
///
/// `spawn_org_vault_with_watcher` collapses steps 2 + 4 into one
/// call when the caller doesn't need the intermediate handles.
pub fn spawn_org_vault_with_watcher(
    root: PathBuf,
    pool: ReadPool,
    worker_handle: WorkerHandle,
) -> Result<(VaultConfig, tokio::task::JoinHandle<()>), notify::Error> {
    let recent = Arc::new(RwLock::new(RecentWrites::new()));
    let notifier = spawn_vault_writer_with_recent(root.clone(), pool.clone(), recent.clone());
    let vault_config = VaultConfig {
        notifier: Arc::new(notifier),
    };
    let watcher_handle = spawn_vault_watcher(root, worker_handle, pool, recent)?;
    Ok((vault_config, watcher_handle))
}
