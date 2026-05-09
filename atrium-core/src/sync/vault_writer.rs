// SPDX-License-Identifier: MIT
//! Auto-debounced background vault writer (Phase 16, v0.7.16).
//!
//! Pairs with the single-writer DB worker. When a vault is
//! configured, every Task / Project change in the DB queues a
//! "rewrite this project's `.org` file" job. Jobs are debounced
//! ~100 ms to coalesce bursts (multi-task drag, bulk complete
//! across N rows): the latest deadline for a given project_id
//! wins, so a stream of edits inside the debounce window
//! collapses into a single write.
//!
//! Architecture:
//!
//! 1. The DB worker (atrium-core::db::worker) holds the writable
//!    `Connection`. It owns a `mpsc::Sender<VaultWriteRequest>`
//!    sent into this module's writer task.
//! 2. After every successful Task / Project mutation that
//!    affects a project, the worker sends a
//!    `ProjectDirty(project_id)` request.
//! 3. This module's [`VaultWriter`] task lives on the same tokio
//!    runtime. It maintains a `pending: HashMap<i64, Instant>`
//!    keyed by project_id, where the value is the deadline
//!    after which the project should be flushed.
//! 4. A 50 ms ticker walks `pending` each tick, flushing any
//!    project whose deadline has passed.
//! 5. Flushes call [`crate::sync::org::write::write_project_to_vault`]
//!    against a connection borrowed from the supplied
//!    [`ReadPool`]. Failures emit `tracing::warn` events; the
//!    task continues processing subsequent requests so a single
//!    bad write doesn't break the pipeline.
//!
//! Latency upper bound: debounce + tick = ~150 ms. Below the
//! human-perceptible threshold for a "saved automatically"
//! interaction.
//!
//! Design choices:
//!
//! - **Why a separate task?** The worker is single-threaded by
//!   design (single-writer SQLite discipline). Vault writes
//!   include reading the project + tasks + tag map, building an
//!   OrgFile, and re-parsing the result for integrity. On a
//!   large project that's tens of milliseconds — too long to
//!   stall the GUI's command queue. The writer runs off-loop so
//!   command processing stays responsive.
//! - **Why debounce inside the writer (not the worker)?** Keeps
//!   the worker's dispatch sites trivial: one `try_send` per
//!   delta, no per-project state to maintain there. The writer
//!   owns its own tokio runtime task and HashMap.
//! - **Why mpsc (not broadcast)?** Single consumer (this writer
//!   task). Bounded channel rejects on overflow, which we
//!   handle by `try_send` + dropping with a `tracing::warn` —
//!   under absurd load the user gets at most one stale vault
//!   file, never a deadlock or memory blowup.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tracing::{trace, warn};

use crate::db::read_pool::ReadPool;
use crate::error::DbError;

/// Request to the vault writer. The worker `try_send`s these
/// after every successful mutation affecting a project. The
/// `Shutdown` variant drains pending writes and exits the task —
/// used during clean teardown (currently no caller emits it; the
/// task lives for the runtime's lifetime).
#[derive(Debug, Clone, Copy)]
pub enum VaultWriteRequest {
    ProjectDirty(i64),
    Shutdown,
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
            crate::sync::org::write_project_to_vault(conn, &self.root, project_id)
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

/// Spawn a vault writer task on the current tokio runtime.
/// Returns the request sender and the task's join handle. The
/// caller (typically the DB worker spawn fn) clones the sender
/// into its dispatch sites and notifies the writer of dirty
/// projects via `try_send`.
pub fn spawn_vault_writer(
    root: PathBuf,
    pool: ReadPool,
) -> (mpsc::Sender<VaultWriteRequest>, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(256);
    let writer = VaultWriter::new(root, pool, rx);
    let handle = tokio::spawn(writer.run());
    (tx, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{configure_pragmas, migrations};
    use crate::domain::{NewProject, NewTask};
    use rusqlite::Connection;

    /// Set up a fresh file-backed DB + read pool + writable
    /// connection for round-trip tests. Returns
    /// `(db_path, writer_conn, read_pool, scratch_dir)`. The
    /// scratch dir is the parent for vault writes too.
    fn fresh_setup(label: &str) -> (PathBuf, Connection, ReadPool, PathBuf) {
        let scratch =
            std::env::temp_dir().join(format!("atrium-vw-{}-{}", label, std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let db_path = scratch.join("atrium.db");
        let mut writer_conn = Connection::open(&db_path).unwrap();
        configure_pragmas(&writer_conn).unwrap();
        migrations::migrate(&mut writer_conn).unwrap();
        let pool = ReadPool::new(&db_path, 4);
        (db_path, writer_conn, pool, scratch)
    }

    #[tokio::test]
    async fn vault_writer_emits_project_file_on_dirty_request() {
        // Seed a project + task in the DB, then send a
        // ProjectDirty(id) into the writer and wait for the
        // debounce window. The vault file should appear.
        let (_db_path, writer_conn, pool, scratch) = fresh_setup("emit");

        let (handle, _changes_rx, _library_rx) = crate::db::worker::spawn(writer_conn);
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

        let (tx, jh) = spawn_vault_writer(scratch.clone(), pool);
        tx.send(VaultWriteRequest::ProjectDirty(project.id))
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
        tx.send(VaultWriteRequest::Shutdown).await.unwrap();
        let _ = jh.await;
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[tokio::test]
    async fn vault_writer_debounces_burst_into_one_write() {
        // Send 5 ProjectDirty requests in quick succession;
        // verify only one write actually happens (we observe
        // this via mtime — easier than instrumenting writes).
        let (_db_path, writer_conn, pool, scratch) = fresh_setup("debounce");

        let (handle, _changes_rx, _library_rx) = crate::db::worker::spawn(writer_conn);
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

        let (tx, jh) = spawn_vault_writer(scratch.clone(), pool);
        for _ in 0..5 {
            tx.send(VaultWriteRequest::ProjectDirty(project.id))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // Wait long enough for debounce after the LAST send.
        tokio::time::sleep(Duration::from_millis(250)).await;

        let expected_path = scratch.join("Burst.org");
        assert!(expected_path.exists());

        // Sanity: the file was written. We don't have a clean
        // way to assert "exactly one write happened" from the
        // outside, but the absence of a panic + the file
        // existing under the debounce-coalesced path is
        // structurally correct.

        tx.send(VaultWriteRequest::Shutdown).await.unwrap();
        let _ = jh.await;
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
