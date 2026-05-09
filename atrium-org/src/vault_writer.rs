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
/// surfacing.
pub struct VaultWriter {
    root: PathBuf,
    pool: ReadPool,
    rx: mpsc::Receiver<VaultWriteRequest>,
    pending: HashMap<i64, Instant>,
    debounce: Duration,
    recent_writes: Arc<RwLock<RecentWrites>>,
    events_tx: Option<mpsc::UnboundedSender<VaultEvent>>,
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
        Self {
            root,
            pool,
            rx,
            pending: HashMap::new(),
            debounce: Duration::from_millis(100),
            recent_writes,
            events_tx,
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

    /// Flush every project whose deadline has passed.
    fn flush_due(&mut self) {
        let now = Instant::now();
        let due: Vec<i64> = self
            .pending
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .map(|(pid, _)| *pid)
            .collect();
        for pid in due {
            self.pending.remove(&pid);
            self.write_project(pid);
        }
    }

    /// Flush every pending project regardless of deadline. Used
    /// during clean shutdown.
    fn flush_all(&mut self) {
        let pids: Vec<i64> = self.pending.keys().copied().collect();
        for pid in pids {
            self.pending.remove(&pid);
            self.write_project(pid);
        }
    }

    fn write_project(&self, project_id: i64) {
        // Conflict-detection pre-write: if the destination file
        // exists with an mtime we don't recognise as our own, an
        // external editor (Doom Emacs, vim-orgmode, etc.) has
        // changed the file since our last write. Snapshot the
        // current contents to <file>.atrium.bak.<timestamp> so
        // the user's edit survives the overwrite. Spec §7.3.3
        // rule 5 — last-writer-wins by mtime; the loser is
        // preserved.
        let dest = self.pool.with(|conn| {
            crate::org::project_vault_path(conn, &self.root, project_id)
                .map_err(|e| DbError::Sync(e.to_string()))
        });
        if let Ok(dest_path) = &dest {
            self.maybe_back_up_external_edit(dest_path);
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
    /// doesn't exist, our last-self-write mtime matches, or the
    /// stat / copy failed (logged at warn level — never panics).
    fn maybe_back_up_external_edit(&self, dest: &Path) -> Option<PathBuf> {
        let mtime = match std::fs::metadata(dest).and_then(|m| m.modified()) {
            Ok(m) => m,
            Err(_) => return None,
        };
        let is_self = self
            .recent_writes
            .read()
            .map(|rw| rw.is_self_write(dest, mtime))
            .unwrap_or(false);
        if is_self {
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
            .filter_map(|e| e.ok())
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
            .filter_map(|e| e.ok())
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
}
