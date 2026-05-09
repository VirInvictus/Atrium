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
use std::path::PathBuf;
use std::time::{Duration, Instant};

use atrium_core::DbError;
use atrium_core::VaultDirtyNotifier;
use atrium_core::db::read_pool::ReadPool;
use tokio::sync::mpsc;
use tracing::{trace, warn};

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
/// pending-writes map.
pub struct VaultWriter {
    root: PathBuf,
    pool: ReadPool,
    rx: mpsc::Receiver<VaultWriteRequest>,
    pending: HashMap<i64, Instant>,
    debounce: Duration,
}

impl VaultWriter {
    /// 50 ms tick — the upper bound on detection latency. Total
    /// latency from a DB write to a vault file landing is
    /// `tick + debounce` ≈ 150 ms.
    const TICK: Duration = Duration::from_millis(50);

    pub fn new(root: PathBuf, pool: ReadPool, rx: mpsc::Receiver<VaultWriteRequest>) -> Self {
        Self {
            root,
            pool,
            rx,
            pending: HashMap::new(),
            debounce: Duration::from_millis(100),
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
            }
            Err(e) => {
                warn!(project_id, error = %e, "vault write failed");
            }
        }
    }
}

/// Spawn a vault writer task on the current tokio runtime and
/// return the [`OrgVaultNotifier`] the atrium-core worker pings.
/// The task lives for the runtime's lifetime; its `JoinHandle` is
/// detached.
pub fn spawn_vault_writer(root: PathBuf, pool: ReadPool) -> OrgVaultNotifier {
    let (tx, rx) = mpsc::channel(256);
    let writer = VaultWriter::new(root, pool, rx);
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
}
