// SPDX-License-Identifier: MIT
//! Auto-debounced background vault writer (Phase 16, v0.7.16;
//! moved into the `atrium-org` crate at v0.9.0).
//!
//! Pairs with atrium-core's single-writer DB worker. When a vault is
//! configured, every Task / Project change in the DB queues a
//! "rewrite this project's `.org` file" job. Jobs are debounced
//! ~100 ms to coalesce bursts (multi-task drag, bulk complete
//! across N rows): the latest deadline for a given project_id
//! wins, so a stream of edits inside the debounce window
//! collapses into a single write.
//!
//! Architecture:
//!
//! 1. The atrium-core worker holds an
//!    `Option<Arc<dyn VaultDirtyNotifier>>` it pings after every
//!    successful Task / Project / Tag mutation.
//! 2. atrium-org's [`OrgVaultNotifier`] is the impl. It wraps an
//!    `mpsc::Sender<VaultWriteRequest>` and `try_send`s a
//!    `ProjectDirty(project_id)` request from the worker thread.
//! 3. The matching [`VaultWriter`] task lives on the same tokio
//!    runtime. It maintains a `pending: HashMap<i64, Instant>`
//!    keyed by project_id, where the value is the deadline
//!    after which the project should be flushed.
//! 4. A 50 ms ticker walks `pending` each tick, flushing any
//!    project whose deadline has passed.
//! 5. Flushes call [`crate::org::write_project_to_vault`]
//!    against a connection borrowed from the supplied
//!    [`ReadPool`]. Failures emit `tracing::warn` events; the
//!    task continues so a single bad write doesn't break the
//!    pipeline.
//!
//! Latency upper bound: debounce + tick = ~150 ms. Below the
//! human-perceptible threshold for a "saved automatically"
//! interaction.
//!
//! Design choices:
//!
//! - **Why a separate task?** The DB worker is single-threaded by
//!   design (single-writer SQLite discipline). Vault writes
//!   include reading the project + tasks + tag map, building an
//!   OrgFile, and re-parsing the result for integrity. On a
//!   large project that's tens of milliseconds — too long to
//!   stall the GUI's command queue. The writer runs off-loop so
//!   command processing stays responsive.
//! - **Why debounce inside the writer (not the worker)?** Keeps
//!   the worker's dispatch sites trivial: one `try_send` per
//!   delta, no per-project state to maintain there.
//! - **Why mpsc (not broadcast)?** Single consumer (this writer
//!   task). Bounded channel rejects on overflow, which we
//!   handle by `try_send` + dropping with a `tracing::warn` —
//!   under absurd load the user gets at most one stale vault
//!   file, never a deadlock or memory blowup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

use atrium_core::DbError;
use atrium_core::VaultDirtyNotifier;
use atrium_core::db::read_pool::ReadPool;
use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use tracing::{trace, warn};

use crate::VaultEvent;
use crate::self_write::RecentWrites;

/// Request to the vault writer. The notifier `try_send`s these
/// after every successful mutation affecting a project. The
/// `Shutdown` variant drains pending writes and exits the task —
/// used during clean teardown (currently no caller emits it
/// outside tests; the task lives for the runtime's lifetime).
#[derive(Debug, Clone, Copy)]
pub enum VaultWriteRequest {
    ProjectDirty(i64),
    Shutdown,
}

/// atrium-org's [`VaultDirtyNotifier`] impl. Wraps the request
/// sender so the atrium-core worker can call
/// `notify_project_dirty(pid)` without knowing about Org / mpsc /
/// debouncing.
#[derive(Clone)]
pub struct OrgVaultNotifier {
    tx: mpsc::Sender<VaultWriteRequest>,
}

impl OrgVaultNotifier {
    /// Clone of the underlying request sender. Tests use this to
    /// inject `Shutdown` for clean teardown.
    pub fn sender(&self) -> mpsc::Sender<VaultWriteRequest> {
        self.tx.clone()
    }
}

impl VaultDirtyNotifier for OrgVaultNotifier {
    fn notify_project_dirty(&self, project_id: i64) {
        // Full channel → drop, not block. Under absurd load the
        // worst case is one stale vault file until the next
        // dirty notification clears the backlog. Worker
        // dispatch sites must never stall.
        let _ = self
            .tx
            .try_send(VaultWriteRequest::ProjectDirty(project_id));
    }
}

/// Background task state. Owns the read pool + vault root + the
/// pending-writes map. The `recent_writes` field is shared with
/// [`crate::vault_watcher::VaultWatcher`] so the watcher can
/// suppress inotify events the writer just generated. The
/// `events_tx` field, when set, ferries operational notices
/// ([`VaultEvent::ConflictBackup`]) up to the GUI for toast
/// surfacing. `last_sidecar` caches the most recently emitted
/// sidecar contents so we skip rewriting `<vault>/.atrium/config.toml`
/// on flushes that don't change tag colours.
pub struct VaultWriter {
    root: PathBuf,
    pool: ReadPool,
    rx: mpsc::Receiver<VaultWriteRequest>,
    pending: HashMap<i64, Instant>,
    debounce: Duration,
    recent_writes: Arc<RwLock<RecentWrites>>,
    events_tx: Option<mpsc::UnboundedSender<VaultEvent>>,
    last_sidecar: Option<crate::sidecar::Sidecar>,
    /// Durable content-hash ledger of what Atrium last wrote to each vault
    /// file (persisted under `<vault>/.atrium/`). The conflict-detection
    /// pre-write consults it so Atrium's own writes are never mistaken for
    /// external edits — unlike `recent_writes`, which is a 2 s in-memory
    /// inotify-echo window and so forgets Atrium's writes across the gap
    /// between two task edits or across a restart.
    write_ledger: WriteLedger,
}

impl VaultWriter {
    /// 50 ms tick — the upper bound on detection latency. Total
    /// latency from a DB write to a vault file landing is
    /// `tick + debounce` ≈ 150 ms.
    const TICK: Duration = Duration::from_millis(50);

    pub fn new(root: PathBuf, pool: ReadPool, rx: mpsc::Receiver<VaultWriteRequest>) -> Self {
        Self::with_recent_writes(root, pool, rx, Arc::new(RwLock::new(RecentWrites::new())))
    }

    /// Variant that wires an externally-owned `RecentWrites` so a
    /// matching [`crate::vault_watcher::VaultWatcher`] can read
    /// from the same set.
    pub fn with_recent_writes(
        root: PathBuf,
        pool: ReadPool,
        rx: mpsc::Receiver<VaultWriteRequest>,
        recent_writes: Arc<RwLock<RecentWrites>>,
    ) -> Self {
        Self::with_recent_writes_and_events(root, pool, rx, recent_writes, None)
    }

    /// Variant that also wires an event sender so the writer can
    /// surface [`VaultEvent::ConflictBackup`] back to the caller.
    /// `None` keeps the prior log-only behaviour.
    pub fn with_recent_writes_and_events(
        root: PathBuf,
        pool: ReadPool,
        rx: mpsc::Receiver<VaultWriteRequest>,
        recent_writes: Arc<RwLock<RecentWrites>>,
        events_tx: Option<mpsc::UnboundedSender<VaultEvent>>,
    ) -> Self {
        let write_ledger = WriteLedger::load(&root);
        let mut writer = Self {
            root,
            pool,
            rx,
            pending: HashMap::new(),
            debounce: Duration::from_millis(100),
            recent_writes,
            events_tx,
            last_sidecar: None,
            write_ledger,
        };
        writer.seed_ledger_from_synced_files();
        writer
    }

    /// Seed the content ledger at startup for every project whose `.org`
    /// file is already byte-identical to what Atrium would write right
    /// now (i.e. in sync with the DB). Without this, the *first* flush of
    /// each not-yet-tracked file after a launch mistakes Atrium's own
    /// prior output for an external edit and snapshots it once — harmless
    /// but noisy, and spread across sessions until every file has been
    /// touched. Seeding closes that window on launch.
    ///
    /// Safety: only in-sync files are seeded. The watcher does not
    /// re-scan on startup, so an edit made in Emacs while Atrium was
    /// closed lives only on disk; such a file will NOT match canonical,
    /// is left unseeded, and so still trips the conflict backup on its
    /// first flush. Purely an optimisation — any read error skips silently
    /// and the pre-existing per-file backup remains the fallback.
    fn seed_ledger_from_synced_files(&mut self) {
        let renders = self.pool.with(|conn| {
            let projects = atrium_core::db::read::list_projects(conn)
                .map_err(|e| DbError::Sync(e.to_string()))?;
            let mut out = Vec::with_capacity(projects.len());
            for project in projects {
                if let Ok(rendered) =
                    crate::org::render_project_to_string(conn, &self.root, project.id)
                {
                    out.push(rendered);
                }
            }
            Ok::<_, DbError>(out)
        });
        let Ok(renders) = renders else {
            return;
        };
        let mut seeded_any = false;
        for (path, text) in renders {
            let canonical = content_hash(text.as_bytes());
            if let Ok(bytes) = std::fs::read(&path)
                && content_hash(&bytes) == canonical
                && self.write_ledger.seed_if_absent(&path, canonical)
            {
                seeded_any = true;
            }
        }
        if seeded_any {
            self.write_ledger.flush();
        }
    }

    /// Run the writer to completion. Returns when the request
    /// channel closes or a `Shutdown` message arrives.
    pub async fn run(mut self) {
        let mut ticker = tokio::time::interval(Self::TICK);
        loop {
            tokio::select! {
                request = self.rx.recv() => {
                    match request {
                        Some(VaultWriteRequest::ProjectDirty(pid)) => {
                            // Last-deadline-wins; bursts collapse.
                            let deadline = Instant::now() + self.debounce;
                            self.pending.insert(pid, deadline);
                        }
                        Some(VaultWriteRequest::Shutdown) | None => {
                            self.flush_all();
                            break;
                        }
                    }
                }
                _ = ticker.tick() => {
                    self.flush_due();
                }
            }
        }
    }

    /// Flush every project whose deadline has passed, then refresh
    /// the sidecar if its DB-derived state has changed.
    fn flush_due(&mut self) {
        let now = Instant::now();
        let due: Vec<i64> = self
            .pending
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .map(|(pid, _)| *pid)
            .collect();
        if due.is_empty() {
            return;
        }
        for pid in due {
            self.pending.remove(&pid);
            self.write_project(pid);
        }
        self.refresh_sidecar_if_changed();
    }

    /// Flush every pending project regardless of deadline + write
    /// the sidecar. Used during clean shutdown.
    fn flush_all(&mut self) {
        let pids: Vec<i64> = self.pending.keys().copied().collect();
        let had_pending = !pids.is_empty();
        for pid in pids {
            self.pending.remove(&pid);
            self.write_project(pid);
        }
        if had_pending {
            self.refresh_sidecar_if_changed();
        }
    }

    /// Re-read the sidecar-shaped slice of the DB (tag colours +
    /// perspectives) and write `<vault>/.atrium/config.toml` when
    /// the result differs from the last write. Idempotent: a
    /// flush burst that doesn't touch sidecar state produces no
    /// sidecar IO. v0.16.0 — `todo_sequences` lives only on disk
    /// (not in SQL), so we read the existing on-disk sidecar and
    /// preserve its sequences before emitting; otherwise every
    /// flush would silently erase user-configured sequences.
    fn refresh_sidecar_if_changed(&mut self) {
        let mut next = match self.pool.with(crate::sidecar::build_from_db) {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "sidecar refresh: DB read failed; skipping");
                return;
            }
        };
        // Preserve disk-only fields (todo_sequences) by reading
        // the existing sidecar and folding them forward. NotFound
        // returns Sidecar::default() with an empty Vec, which is
        // the right behaviour for fresh vaults.
        if let Ok(on_disk) = crate::sidecar::read_sidecar(&self.root) {
            next.todo_sequences = on_disk.todo_sequences;
        }
        if self.last_sidecar.as_ref() == Some(&next) {
            return;
        }
        match crate::sidecar::write_sidecar(&self.root, &next) {
            Ok(()) => {
                trace!("sidecar refreshed");
                self.last_sidecar = Some(next);
            }
            Err(e) => {
                warn!(error = %e, "sidecar write failed; will retry next flush");
            }
        }
    }

    fn write_project(&mut self, project_id: i64) {
        // Conflict-detection pre-write: if the destination file
        // exists with contents we don't recognise as our own last
        // write, an external editor (Doom Emacs, vim-orgmode, etc.)
        // has changed the file since. Snapshot the current contents
        // to <file>.atrium.bak.<timestamp> so the user's edit
        // survives the overwrite. Spec §7.3.3 rule 5 — last-writer-
        // wins; the loser is preserved.
        let dest = self.pool.with(|conn| {
            crate::org::project_vault_path(conn, &self.root, project_id)
                .map_err(|e| DbError::Sync(e.to_string()))
        });
        if let Ok(dest_path) = &dest {
            let dest_path = dest_path.clone();
            self.maybe_back_up_external_edit(&dest_path);
        }

        let result = self.pool.with(|conn| {
            crate::org::write_project_to_vault(conn, &self.root, project_id)
                .map_err(|e| DbError::Sync(e.to_string()))
        });
        match result {
            Ok(summary) => {
                trace!(
                    project_id,
                    file = %summary.file_path.display(),
                    tasks = summary.task_count,
                    "vault write succeeded"
                );
                // Record the content we just wrote, durably, so a
                // later flush recognises it as ours and never backs
                // it up spuriously.
                if let Ok(bytes) = std::fs::read(&summary.file_path) {
                    self.write_ledger
                        .record(&summary.file_path, content_hash(&bytes));
                }
                // Tell the watcher this file is one of ours so the
                // resulting inotify event gets suppressed by exact
                // (path, mtime) match. If metadata fails, the
                // entry isn't recorded — the watcher will process
                // the event as if external (harmless: a no-op
                // diff against the just-written DB state).
                if let Ok(mut rw) = self.recent_writes.write() {
                    let _recorded = rw.record(summary.file_path);
                }
            }
            Err(e) => {
                warn!(project_id, error = %e, "vault write failed");
            }
        }
    }

    /// Inspect the current on-disk file (if any) before the writer
    /// overwrites it. Returns `Some(backup_path)` when an external
    /// edit was detected and snapshotted; `None` when the file
    /// doesn't exist, its contents match what Atrium last wrote, an
    /// immediate self-write echo matches by mtime, or the stat / read
    /// failed (logged at warn level — never panics).
    fn maybe_back_up_external_edit(&mut self, dest: &Path) -> Option<PathBuf> {
        let mtime = match std::fs::metadata(dest).and_then(|m| m.modified()) {
            Ok(m) => m,
            Err(_) => return None,
        };
        // Fast path: an immediate inotify-echo of our own just-completed
        // write (matches by exact mtime within the 2 s window).
        let is_self = self
            .recent_writes
            .read()
            .is_ok_and(|rw| rw.is_self_write(dest, mtime));
        if is_self {
            return None;
        }
        // Durable path: does the on-disk content match what Atrium last
        // wrote to this file? `recent_writes` forgets across the 2 s window
        // and across restarts, so it flags Atrium's own older writes as
        // "external"; the content ledger doesn't. Only a genuine external
        // edit changes the bytes away from our recorded hash.
        if let Ok(bytes) = std::fs::read(dest)
            && self.write_ledger.get(dest) == Some(content_hash(&bytes))
        {
            return None;
        }
        let bak = backup_path(dest, SystemTime::now());
        match std::fs::copy(dest, &bak) {
            Ok(_) => {
                warn!(
                    file = %dest.display(),
                    backup = %bak.display(),
                    "vault conflict: external edit backed up before overwrite"
                );
                if let Some(tx) = &self.events_tx {
                    let _ = tx.send(VaultEvent::ConflictBackup {
                        source: dest.to_path_buf(),
                        backup: bak.clone(),
                    });
                }
                Some(bak)
            }
            Err(e) => {
                warn!(
                    file = %dest.display(),
                    error = %e,
                    "vault conflict detected but backup copy failed; proceeding with overwrite"
                );
                None
            }
        }
    }
}

/// Build a `<file>.atrium.bak.<UTC-timestamp>` path adjacent to
/// `dest`. The timestamp format is filesystem-safe (no colons), UTC,
/// and sortable so multiple backups for the same file order
/// chronologically when listed.
fn backup_path(dest: &Path, now: SystemTime) -> PathBuf {
    let utc: DateTime<Utc> = now.into();
    let stamp = utc.format("%Y%m%dT%H%M%SZ");
    let file_name = match dest.file_name() {
        Some(f) => f.to_os_string(),
        None => std::ffi::OsString::from("vault"),
    };
    let mut bak_name = file_name;
    bak_name.push(format!(".atrium.bak.{stamp}"));
    let parent = dest.parent().unwrap_or_else(|| Path::new(""));
    parent.join(bak_name)
}

/// FNV-1a 64-bit hash of a file's bytes. Stable across processes (unlike
/// `std`'s `DefaultHasher`), so a hash written to the ledger last session
/// compares equal this session. No dependency, and collisions are
/// astronomically unlikely for the vault-file size range — and a collision
/// only ever risks a *missed* backup of a genuine external edit, never data
/// loss (the DB stays canonical).
fn content_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Durable per-file record of the content Atrium last wrote to each vault
/// file, persisted under `<vault>/.atrium/write-ledger`. Lets the conflict
/// detector tell Atrium's own writes from genuine external edits across the
/// `recent_writes` TTL and across restarts. Non-critical cache: a lost or
/// stale ledger only risks an occasional spurious backup, never data loss.
struct WriteLedger {
    file: PathBuf,
    root: PathBuf,
    hashes: HashMap<PathBuf, u64>,
}

impl WriteLedger {
    fn load(root: &Path) -> Self {
        let file = root.join(".atrium").join("write-ledger");
        let mut hashes = HashMap::new();
        if let Ok(text) = std::fs::read_to_string(&file) {
            for line in text.lines() {
                if let Some((hex, rel)) = line.split_once('\t')
                    && let Ok(h) = u64::from_str_radix(hex, 16)
                {
                    hashes.insert(root.join(rel), h);
                }
            }
        }
        Self {
            file,
            root: root.to_path_buf(),
            hashes,
        }
    }

    fn get(&self, path: &Path) -> Option<u64> {
        self.hashes.get(path).copied()
    }

    fn record(&mut self, path: &Path, hash: u64) {
        if self.hashes.get(path) == Some(&hash) {
            return;
        }
        self.hashes.insert(path.to_path_buf(), hash);
        self.persist();
    }

    /// Insert an entry only if the file is not already tracked, without
    /// persisting. The startup seed batches many of these and calls
    /// [`WriteLedger::flush`] once at the end. Returns whether it inserted.
    fn seed_if_absent(&mut self, path: &Path, hash: u64) -> bool {
        if self.hashes.contains_key(path) {
            return false;
        }
        self.hashes.insert(path.to_path_buf(), hash);
        true
    }

    /// Persist the in-memory map to disk. Used by the batch seed after
    /// folding in every in-sync file, so seeding writes the ledger once
    /// rather than once per file.
    fn flush(&self) {
        self.persist();
    }

    fn persist(&self) {
        let mut out = String::new();
        for (path, h) in &self.hashes {
            if let Ok(rel) = path.strip_prefix(&self.root) {
                out.push_str(&format!("{h:016x}\t{}\n", rel.display()));
            }
        }
        if let Some(parent) = self.file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Temp + rename so a crash mid-write can't leave a torn ledger.
        let tmp = self.file.with_extension("tmp");
        if std::fs::write(&tmp, out.as_bytes()).is_ok() {
            let _ = std::fs::rename(&tmp, &self.file);
        }
    }
}

/// Spawn a vault writer task on the current tokio runtime and
/// return the [`OrgVaultNotifier`] the atrium-core worker pings.
/// The task lives for the runtime's lifetime; its `JoinHandle` is
/// detached. Uses an internal [`RecentWrites`] — for the
/// vault-watcher-aware variant, see [`spawn_vault_writer_with_recent`].
pub fn spawn_vault_writer(root: PathBuf, pool: ReadPool) -> OrgVaultNotifier {
    spawn_vault_writer_with_recent(root, pool, Arc::new(RwLock::new(RecentWrites::new())))
}

/// Variant that wires an externally-owned [`RecentWrites`] so the
/// companion [`crate::vault_watcher::VaultWatcher`] can read from
/// the same set. The two halves of the Phase 17 sync loop share
/// this set to break the self-write echo.
pub fn spawn_vault_writer_with_recent(
    root: PathBuf,
    pool: ReadPool,
    recent_writes: Arc<RwLock<RecentWrites>>,
) -> OrgVaultNotifier {
    spawn_vault_writer_with_events(root, pool, recent_writes, None)
}

/// Variant that wires both an externally-owned [`RecentWrites`] set
/// and an optional [`VaultEvent`] sender. `None` for `events_tx`
/// keeps the writer's prior log-only behaviour; `Some(tx)` lets the
/// writer surface [`VaultEvent::ConflictBackup`] notices to the
/// caller for toast surfacing.
pub fn spawn_vault_writer_with_events(
    root: PathBuf,
    pool: ReadPool,
    recent_writes: Arc<RwLock<RecentWrites>>,
    events_tx: Option<mpsc::UnboundedSender<VaultEvent>>,
) -> OrgVaultNotifier {
    let (tx, rx) = mpsc::channel(256);
    let writer =
        VaultWriter::with_recent_writes_and_events(root, pool, rx, recent_writes, events_tx);
    tokio::spawn(writer.run());
    OrgVaultNotifier { tx }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atrium_core::db::open;
    use atrium_core::{NewProject, NewTask, spawn_worker};
    use rusqlite::Connection;

    /// Set up a fresh file-backed DB + read pool + writable
    /// connection for round-trip tests. Returns
    /// `(writer_conn, read_pool, scratch_dir)`. The scratch dir
    /// is the parent for vault writes too.
    fn fresh_setup(label: &str) -> (Connection, ReadPool, PathBuf) {
        let scratch =
            std::env::temp_dir().join(format!("atrium-vw-{}-{}", label, std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let db_path = scratch.join("atrium.db");
        let writer_conn = open(&db_path).unwrap();
        let pool = ReadPool::new(&db_path, 4);
        (writer_conn, pool, scratch)
    }

    #[tokio::test]
    async fn vault_writer_emits_project_file_on_dirty_request() {
        let (writer_conn, pool, scratch) = fresh_setup("emit");

        let (handle, _changes_rx, _library_rx) = spawn_worker(writer_conn);
        let project = handle
            .create_project(NewProject {
                title: "Sample".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = handle
            .create_task(NewTask {
                title: "first".to_string(),
                project_id: Some(project.id),
                ..Default::default()
            })
            .await
            .unwrap();

        let notifier = spawn_vault_writer(scratch.clone(), pool);
        notifier
            .sender()
            .send(VaultWriteRequest::ProjectDirty(project.id))
            .await
            .unwrap();

        // Wait long enough for debounce + tick to fire.
        tokio::time::sleep(Duration::from_millis(250)).await;

        let expected_path = scratch.join("Sample.org");
        assert!(
            expected_path.exists(),
            "expected vault file at {}",
            expected_path.display()
        );
        let contents = std::fs::read_to_string(&expected_path).unwrap();
        assert!(contents.contains("first"), "got: {contents}");

        // Clean shutdown.
        let _ = notifier.sender().send(VaultWriteRequest::Shutdown).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[tokio::test]
    async fn vault_writer_debounces_burst_into_one_write() {
        let (writer_conn, pool, scratch) = fresh_setup("debounce");

        let (handle, _changes_rx, _library_rx) = spawn_worker(writer_conn);
        let project = handle
            .create_project(NewProject {
                title: "Burst".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = handle
            .create_task(NewTask {
                title: "t".to_string(),
                project_id: Some(project.id),
                ..Default::default()
            })
            .await
            .unwrap();

        let notifier = spawn_vault_writer(scratch.clone(), pool);
        for _ in 0..5 {
            notifier
                .sender()
                .send(VaultWriteRequest::ProjectDirty(project.id))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // Wait long enough for debounce after the LAST send.
        tokio::time::sleep(Duration::from_millis(250)).await;

        let expected_path = scratch.join("Burst.org");
        assert!(expected_path.exists());

        let _ = notifier.sender().send(VaultWriteRequest::Shutdown).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = std::fs::remove_dir_all(&scratch);
    }

    // ── Conflict detection (spec §7.3.3 rule 5) ──────────────

    #[test]
    fn backup_path_format_is_filesystem_safe_and_sortable() {
        // 1_715_270_400 unix seconds = 2024-05-09 16:00:00 UTC.
        let dest = PathBuf::from("/tmp/vault/Errands.org");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_715_270_400);
        let bak = backup_path(&dest, now);
        let s = bak.to_string_lossy();
        assert!(s.starts_with("/tmp/vault/Errands.org.atrium.bak."));
        assert!(s.ends_with("Z"), "stamp must end with Z: {s}");
        assert!(!s.contains(':'), "no colons in path: {s}");
        assert!(s.contains("20240509T160000Z"), "stamp shape: {s}");
    }

    #[tokio::test]
    async fn writer_backs_up_external_edit_before_overwriting() {
        let (writer_conn, pool, scratch) = fresh_setup("conflict");
        let (handle, _changes_rx, _library_rx) = spawn_worker(writer_conn);
        let project = handle
            .create_project(NewProject {
                title: "Conflict".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = handle
            .create_task(NewTask {
                title: "Buy milk".to_string(),
                project_id: Some(project.id),
                ..Default::default()
            })
            .await
            .unwrap();

        // First flush — establishes the file + records the
        // mtime in RecentWrites.
        let notifier = spawn_vault_writer(scratch.clone(), pool);
        notifier
            .sender()
            .send(VaultWriteRequest::ProjectDirty(project.id))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        let dest = scratch.join("Conflict.org");
        assert!(dest.exists());

        // Simulate Doom Emacs saving the file — content changes,
        // mtime advances. The writer hasn't seen this mtime, so
        // the next flush should detect the conflict.
        let external_content = "* TODO Buy milk\nThis is what the user typed in Doom\n";
        std::fs::write(&dest, external_content).unwrap();

        // Mutate DB and trigger another flush. This test uses
        // spawn_worker (no vault hook), so the writer doesn't get
        // pinged automatically — we send the ProjectDirty by hand
        // to mirror what the worker would do.
        let _ = handle
            .create_task(NewTask {
                title: "Buy bread".to_string(),
                project_id: Some(project.id),
                ..Default::default()
            })
            .await
            .unwrap();
        notifier
            .sender()
            .send(VaultWriteRequest::ProjectDirty(project.id))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;

        // The user's edit must survive in a .atrium.bak.* sibling.
        let entries: Vec<_> = std::fs::read_dir(&scratch)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        let bak = entries
            .iter()
            .find(|n| n.starts_with("Conflict.org.atrium.bak."))
            .unwrap_or_else(|| panic!("no .atrium.bak.* sibling found in {entries:?}"));
        let bak_text = std::fs::read_to_string(scratch.join(bak)).unwrap();
        assert!(
            bak_text.contains("This is what the user typed in Doom"),
            "backup must hold the user's external edit; got: {bak_text}"
        );
        // The main file is now the DB's view (overwrite proceeded).
        let main_text = std::fs::read_to_string(&dest).unwrap();
        assert!(
            main_text.contains("Buy bread"),
            "main file should have DB state"
        );

        let _ = notifier.sender().send(VaultWriteRequest::Shutdown).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[tokio::test]
    async fn writer_does_not_back_up_self_writes() {
        let (writer_conn, pool, scratch) = fresh_setup("noselfbak");
        let (handle, _changes_rx, _library_rx) = spawn_worker(writer_conn);
        let project = handle
            .create_project(NewProject {
                title: "Clean".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = handle
            .create_task(NewTask {
                title: "task one".to_string(),
                project_id: Some(project.id),
                ..Default::default()
            })
            .await
            .unwrap();

        let notifier = spawn_vault_writer(scratch.clone(), pool);
        // Trigger several flushes back-to-back. None of them
        // should produce a backup — every overwrite is the
        // writer overwriting its own previous output. We drive
        // flushes by hand because spawn_worker has no vault hook.
        for i in 0..3 {
            let _ = handle
                .create_task(NewTask {
                    title: format!("task {i}"),
                    project_id: Some(project.id),
                    ..Default::default()
                })
                .await
                .unwrap();
            notifier
                .sender()
                .send(VaultWriteRequest::ProjectDirty(project.id))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        // Drain.
        tokio::time::sleep(Duration::from_millis(250)).await;

        let entries: Vec<_> = std::fs::read_dir(&scratch)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            !entries.iter().any(|n| n.contains(".atrium.bak.")),
            "self-writes must not produce backups; saw: {entries:?}"
        );

        let _ = notifier.sender().send(VaultWriteRequest::Shutdown).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The regression test for the over-reactivity bug: a *fresh* writer
    /// (empty `RecentWrites`, as after a relaunch or once the 2 s echo
    /// window has lapsed) must recognise Atrium's own prior on-disk write
    /// via the persisted content ledger and NOT back it up. Before the
    /// ledger, completing a task each session spuriously snapshotted the
    /// file every time.
    #[tokio::test]
    async fn fresh_writer_recognises_own_prior_write_via_ledger() {
        let (writer_conn, pool, scratch) = fresh_setup("ledger");
        let (handle, _changes_rx, _library_rx) = spawn_worker(writer_conn);
        let project = handle
            .create_project(NewProject {
                title: "Ledger".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let task = handle
            .create_task(NewTask {
                title: "task one".to_string(),
                project_id: Some(project.id),
                ..Default::default()
            })
            .await
            .unwrap();

        // Session 1: writer establishes the file and persists the ledger,
        // then shuts down.
        let notifier1 = spawn_vault_writer(scratch.clone(), pool.clone());
        notifier1
            .sender()
            .send(VaultWriteRequest::ProjectDirty(project.id))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        let _ = notifier1.sender().send(VaultWriteRequest::Shutdown).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let dest = scratch.join("Ledger.org");
        assert!(dest.exists());
        assert!(scratch.join(".atrium").join("write-ledger").exists());

        // A DB change in the "next session": complete the task.
        handle.toggle_complete(task.id).await.unwrap();

        // Session 2: a brand-new writer with an empty RecentWrites. Before
        // the ledger fix its first flush would mistake the on-disk file
        // (its own session-1 output) for an external edit and back it up.
        let notifier2 = spawn_vault_writer(scratch.clone(), pool);
        notifier2
            .sender()
            .send(VaultWriteRequest::ProjectDirty(project.id))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;

        let entries: Vec<_> = std::fs::read_dir(&scratch)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            !entries.iter().any(|n| n.contains(".atrium.bak.")),
            "a fresh writer must recognise Atrium's own prior write via the ledger; saw: {entries:?}"
        );

        let _ = notifier2.sender().send(VaultWriteRequest::Shutdown).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The startup seed: a file that is on disk and in sync with the DB
    /// but has NO ledger entry (e.g. produced by `export org`, or written
    /// by a pre-ledger build) must be recognised as ours on the next
    /// launch, so the first flush after a DB change doesn't back it up.
    #[tokio::test]
    async fn startup_seed_recognises_in_sync_file_without_prior_ledger() {
        let (writer_conn, pool, scratch) = fresh_setup("seed");
        let (handle, _changes_rx, _library_rx) = spawn_worker(writer_conn);
        let project = handle
            .create_project(NewProject {
                title: "Seed".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let task = handle
            .create_task(NewTask {
                title: "task one".to_string(),
                project_id: Some(project.id),
                ..Default::default()
            })
            .await
            .unwrap();

        // Session 1: establish the file + ledger, then shut down.
        let notifier1 = spawn_vault_writer(scratch.clone(), pool.clone());
        notifier1
            .sender()
            .send(VaultWriteRequest::ProjectDirty(project.id))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        let _ = notifier1.sender().send(VaultWriteRequest::Shutdown).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let dest = scratch.join("Seed.org");
        assert!(dest.exists());

        // Simulate a vault whose file is in sync but was never ledgered:
        // delete the ledger while the on-disk file still matches the DB.
        let ledger = scratch.join(".atrium").join("write-ledger");
        std::fs::remove_file(&ledger).unwrap();

        // Session 2: constructing the writer seeds the ledger from the
        // in-sync file (no DB change yet, so on-disk == canonical).
        let notifier2 = spawn_vault_writer(scratch.clone(), pool.clone());
        assert!(
            ledger.exists(),
            "startup seed must re-create the ledger from the in-sync file"
        );

        // Now a DB change + flush: the seeded ledger must recognise the
        // file as ours, so no backup is produced.
        handle.toggle_complete(task.id).await.unwrap();
        notifier2
            .sender()
            .send(VaultWriteRequest::ProjectDirty(project.id))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;

        let entries: Vec<_> = std::fs::read_dir(&scratch)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            !entries.iter().any(|n| n.contains(".atrium.bak.")),
            "startup seed should recognise the in-sync file as ours; saw: {entries:?}"
        );

        let _ = notifier2.sender().send(VaultWriteRequest::Shutdown).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
