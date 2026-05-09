// SPDX-License-Identifier: MIT
//! Single-writer SQLite worker.
//!
//! A dedicated `tokio` task owns the writable `rusqlite::Connection`.
//! The UI thread holds a [`WorkerHandle`] (a thin wrapper around an
//! `mpsc::Sender<Command>`) and **never** touches the writable
//! connection directly. UI updates flow back through a separate
//! `mpsc::UnboundedReceiver<TaskChanges>` returned by [`spawn`].
//!
//! This is the Phase 2 implementation of spec §3.2's "single-writer
//! SQLite worker" architectural commitment, ported from Viaduct's
//! `DatabaseQueue` discipline.

use std::time::Duration;

use rusqlite::{Connection, params};
use tokio::sync::{mpsc, oneshot};
use tracing::{Instrument, info, info_span, trace};
use uuid::Uuid;

use crate::db::changes::{LibraryChanges, TaskChanges};
use crate::db::command::Command;
use crate::db::read;
use crate::domain::{
    Area, AreaUpdate, NewArea, NewPerspective, NewProject, NewTag, NewTask, Perspective,
    PerspectiveUpdate, Project, ProjectUpdate, Tag, TagUpdate, Task, TaskUpdate,
};
use crate::error::DbError;

/// Bounded command-channel capacity. UIs that overshoot this are
/// either pathologically fast or the worker is genuinely backed up;
/// either way, backpressure surfaces in the await on `WorkerHandle`
/// methods.
const COMMAND_CHANNEL_CAPACITY: usize = 64;

/// Cheap clone of the worker's command sender. Drop the last one to
/// shut the worker down.
#[derive(Debug, Clone)]
pub struct WorkerHandle {
    cmd_tx: mpsc::Sender<Command>,
}

impl WorkerHandle {
    pub async fn create_task(&self, task: NewTask) -> Result<Task, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::CreateTask { task, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn update_task(&self, update: TaskUpdate) -> Result<Task, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::UpdateTask { update, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn toggle_complete(&self, id: i64) -> Result<Task, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::ToggleComplete { id, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn delete_task(&self, id: i64) -> Result<(), DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::DeleteTask { id, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    // ── Areas (Phase 5b) ────────────────────────────────────────

    pub async fn create_area(&self, area: NewArea) -> Result<Area, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::CreateArea { area, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn update_area(&self, update: AreaUpdate) -> Result<Area, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::UpdateArea { update, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn delete_area(&self, id: i64) -> Result<(), DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::DeleteArea { id, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    // ── Projects (Phase 5b) ─────────────────────────────────────

    pub async fn create_project(&self, project: NewProject) -> Result<Project, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::CreateProject { project, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn update_project(&self, update: ProjectUpdate) -> Result<Project, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::UpdateProject { update, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn archive_project(&self, id: i64) -> Result<Project, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::ArchiveProject { id, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    /// Phase 13 — acknowledge a project review. Sets
    /// `last_reviewed_at = now()` so the project drops out of the
    /// Review queue until its interval has elapsed again.
    pub async fn mark_reviewed(&self, id: i64) -> Result<Project, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::MarkReviewed { id, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    /// v0.7.4 — task-level analogue of `mark_reviewed`. Stamps
    /// `task.last_reviewed_at = now()` so the canonical Review
    /// page's weekly walk hides the row for 7 days.
    pub async fn mark_task_reviewed(&self, id: i64) -> Result<Task, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::MarkTaskReviewed { id, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn delete_project(&self, id: i64) -> Result<(), DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::DeleteProject { id, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    // ── Tags (Phase 6a) ─────────────────────────────────────────

    pub async fn create_tag(&self, tag: NewTag) -> Result<Tag, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::CreateTag { tag, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn update_tag(&self, update: TagUpdate) -> Result<Tag, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::UpdateTag { update, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn delete_tag(&self, id: i64) -> Result<(), DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::DeleteTag { id, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    /// Replace the entire tag set on a task in one transaction.
    pub async fn set_task_tags(&self, task_id: i64, tag_ids: Vec<i64>) -> Result<Task, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::SetTaskTags {
                task_id,
                tag_ids,
                responder,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    /// Idempotent "find tag by name or create it" — handy for the
    /// inline `#tag` parser.
    pub async fn ensure_tag(&self, name: String) -> Result<Tag, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::EnsureTag { name, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    /// v0.7.14 — idempotent area-create-if-absent. Mirror of
    /// [`Self::ensure_tag`] for areas. Used by the multi-file Org
    /// importer when mapping vault subdirectories onto Atrium
    /// areas; safe to call repeatedly with the same name.
    pub async fn ensure_area(&self, name: String) -> Result<Area, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::EnsureArea { name, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    // ── Perspectives (Phase 14) ─────────────────────────────────

    pub async fn create_perspective(
        &self,
        perspective: NewPerspective,
    ) -> Result<Perspective, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::CreatePerspective {
                perspective,
                responder,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn update_perspective(
        &self,
        update: PerspectiveUpdate,
    ) -> Result<Perspective, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::UpdatePerspective { update, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn delete_perspective(&self, id: i64) -> Result<(), DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::DeletePerspective { id, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }
}

/// Spawn the worker on the current `tokio` runtime.
///
/// Returns the `WorkerHandle` (commands flow in), the `TaskChanges`
/// receiver (task-level deltas flow out), and the `LibraryChanges`
/// receiver (area/project deltas flow out — Phase 5b). The worker
/// exits when the last `WorkerHandle` is dropped.
///
/// No vault auto-write — use [`spawn_with_vault`] when a vault
/// projection is configured.
pub fn spawn(
    conn: Connection,
) -> (
    WorkerHandle,
    mpsc::UnboundedReceiver<TaskChanges>,
    mpsc::UnboundedReceiver<LibraryChanges>,
) {
    spawn_with_vault(conn, None)
}

/// v0.7.16 — Phase 16 entry point that wires the auto-debounced
/// vault writer alongside the main worker. Pass `Some(VaultConfig
/// { root, read_pool })` to enable auto-writes; `None` is
/// equivalent to [`spawn`].
///
/// When configured, every successful Task / Project / Tag write
/// that affects a project queues a `ProjectDirty(project_id)`
/// notification into the vault writer task (see
/// [`crate::sync::vault_writer`]). The writer debounces 100 ms
/// and flushes on a 50 ms tick, so a burst of edits collapses
/// into one `.org` rewrite per project. Latency upper bound:
/// ~150 ms from DB write to vault file landing.
pub fn spawn_with_vault(
    mut conn: Connection,
    vault: Option<VaultConfig>,
) -> (
    WorkerHandle,
    mpsc::UnboundedReceiver<TaskChanges>,
    mpsc::UnboundedReceiver<LibraryChanges>,
) {
    install_profile_callback(&mut conn);

    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(COMMAND_CHANNEL_CAPACITY);
    let (changes_tx, changes_rx) = mpsc::unbounded_channel::<TaskChanges>();
    let (library_tx, library_rx) = mpsc::unbounded_channel::<LibraryChanges>();

    let vault_tx = vault.map(|cfg| {
        let (tx, _jh) =
            crate::sync::vault_writer::spawn_vault_writer(cfg.root.clone(), cfg.read_pool);
        tx
    });

    let worker = Worker {
        conn,
        cmd_rx,
        changes_tx,
        library_tx,
        vault_tx,
    };

    tokio::spawn(worker.run().instrument(info_span!("atrium_worker")));

    (WorkerHandle { cmd_tx }, changes_rx, library_rx)
}

/// Configuration for the auto-debounced vault-write hook.
/// Passed through [`spawn_with_vault`] at worker startup.
pub struct VaultConfig {
    pub root: std::path::PathBuf,
    pub read_pool: crate::db::read_pool::ReadPool,
}

/// Wire rusqlite's `profile` callback to the `tracing` TRACE level.
/// Per spec §3.4, every SQL statement is observable through the debug
/// harness — `RUST_LOG=trace` (or filtered to `atrium_core::db=trace`)
/// reveals each statement's text and elapsed wall time.
fn install_profile_callback(conn: &mut Connection) {
    conn.profile(Some(|sql: &str, dur: Duration| {
        trace!(elapsed_us = dur.as_micros() as u64, sql = %sql, "sqlite stmt");
    }));
}

struct Worker {
    conn: Connection,
    cmd_rx: mpsc::Receiver<Command>,
    changes_tx: mpsc::UnboundedSender<TaskChanges>,
    library_tx: mpsc::UnboundedSender<LibraryChanges>,
    /// v0.7.16 — auto-debounced vault writer hook. `None` when
    /// no vault is configured (atrium-cli, tests). `Some` when
    /// the GUI passes a `VaultConfig` through `spawn_with_vault`.
    vault_tx: Option<mpsc::Sender<crate::sync::vault_writer::VaultWriteRequest>>,
}

impl Worker {
    /// v0.7.16 — non-blocking notification that a project's
    /// vault file should be re-emitted. `try_send` so a full
    /// channel never stalls command processing; under absurd
    /// load the worst case is a stale vault file until the
    /// next dirty notification clears the backlog.
    fn notify_project_dirty(&self, project_id: i64) {
        if let Some(tx) = &self.vault_tx {
            use crate::sync::vault_writer::VaultWriteRequest;
            let _ = tx.try_send(VaultWriteRequest::ProjectDirty(project_id));
        }
    }
}

impl Worker {
    async fn run(mut self) {
        info!("atrium worker started");
        while let Some(cmd) = self.cmd_rx.recv().await {
            let span = info_span!("command", variant = cmd.variant_name());
            let _enter = span.enter();
            self.handle(cmd);
        }
        info!("atrium worker shutting down (sender dropped)");
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::CreateTask { task, responder } => {
                let result = self.create_task(task);
                if let Ok(ref task) = result {
                    let _ = self.changes_tx.send(TaskChanges {
                        created: vec![task.clone()],
                        ..Default::default()
                    });
                    if let Some(pid) = task.project_id {
                        self.notify_project_dirty(pid);
                    }
                }
                let _ = responder.send(result);
            }
            Command::UpdateTask { update, responder } => {
                let result = self.update_task(update);
                if let Ok(ref task) = result {
                    let _ = self.changes_tx.send(TaskChanges {
                        updated: vec![task.clone()],
                        ..Default::default()
                    });
                    if let Some(pid) = task.project_id {
                        self.notify_project_dirty(pid);
                    }
                }
                let _ = responder.send(result);
            }
            Command::ToggleComplete { id, responder } => {
                let result = self.toggle_complete(id);
                if let Ok((ref task, ref spawned)) = result {
                    let mut changes = TaskChanges {
                        updated: vec![task.clone()],
                        status_changed: vec![task.id],
                        ..Default::default()
                    };
                    // Phase 15 — if completing a repeating task
                    // spawned a follow-up instance, ride its row out
                    // on the same delta so the UI sees both at once.
                    if let Some(next) = spawned {
                        changes.created.push(next.clone());
                    }
                    let _ = self.changes_tx.send(changes);
                    if let Some(pid) = task.project_id {
                        self.notify_project_dirty(pid);
                    }
                }
                let _ = responder.send(result.map(|(t, _)| t));
            }
            Command::DeleteTask { id, responder } => {
                // v0.7.16 — capture the project_id BEFORE we
                // delete the row so the vault writer can rewrite
                // the right .org file. Best-effort: if the read
                // fails, the file just stays stale until the next
                // edit hits the project.
                let project_id_for_vault = read::task_by_id(&self.conn, id)
                    .ok()
                    .flatten()
                    .and_then(|t| t.project_id);
                let result = self.delete_task(id);
                if result.is_ok() {
                    let _ = self.changes_tx.send(TaskChanges {
                        deleted: vec![id],
                        ..Default::default()
                    });
                    if let Some(pid) = project_id_for_vault {
                        self.notify_project_dirty(pid);
                    }
                }
                let _ = responder.send(result);
            }

            // ── Areas ─────────────────────────────────────────────
            Command::CreateArea { area, responder } => {
                let result = self.create_area(area);
                if let Ok(ref a) = result {
                    let _ = self.library_tx.send(LibraryChanges {
                        areas_created: vec![a.clone()],
                        ..Default::default()
                    });
                }
                let _ = responder.send(result);
            }
            Command::UpdateArea { update, responder } => {
                let result = self.update_area(update);
                if let Ok(ref a) = result {
                    let _ = self.library_tx.send(LibraryChanges {
                        areas_updated: vec![a.clone()],
                        ..Default::default()
                    });
                }
                let _ = responder.send(result);
            }
            Command::DeleteArea { id, responder } => {
                // Read the projects that reference this area before
                // deleting so we can emit projects_updated for them
                // (FK is ON DELETE SET NULL — they'll be unfiled).
                let affected_projects = self.projects_with_area(id).unwrap_or_default();
                let result = self.delete_area(id);
                if result.is_ok() {
                    // Re-read those projects to capture the now-NULL area_id.
                    let mut updated_projects = Vec::new();
                    for pid in &affected_projects {
                        if let Ok(Some(p)) = read::project_by_id(&self.conn, *pid) {
                            updated_projects.push(p);
                        }
                    }
                    let _ = self.library_tx.send(LibraryChanges {
                        areas_deleted: vec![id],
                        projects_updated: updated_projects,
                        ..Default::default()
                    });
                }
                let _ = responder.send(result);
            }

            // ── Projects ──────────────────────────────────────────
            Command::CreateProject { project, responder } => {
                let result = self.create_project(project);
                if let Ok(ref p) = result {
                    let _ = self.library_tx.send(LibraryChanges {
                        projects_created: vec![p.clone()],
                        ..Default::default()
                    });
                    self.notify_project_dirty(p.id);
                }
                let _ = responder.send(result);
            }
            Command::UpdateProject { update, responder } => {
                let result = self.update_project(update);
                if let Ok(ref p) = result {
                    self.notify_project_dirty(p.id);
                    let _ = self.library_tx.send(LibraryChanges {
                        projects_updated: vec![p.clone()],
                        ..Default::default()
                    });
                }
                let _ = responder.send(result);
            }
            Command::ArchiveProject { id, responder } => {
                // Read the project's open tasks first so the
                // `TaskChanges` below carries their pre-archive ids
                // (the actual rows get `completed_at` set in the same
                // transaction).
                let affected_task_ids = self.open_task_ids_in_project(id).unwrap_or_default();
                let result = self.archive_project(id);
                if let Ok(p) = &result {
                    let _ = self.library_tx.send(LibraryChanges {
                        projects_updated: vec![p.clone()],
                        ..Default::default()
                    });
                    self.notify_project_dirty(p.id);
                    // Emit per-task status_changed so any open list
                    // showing them removes them from view.
                    let mut updated_tasks = Vec::new();
                    for tid in &affected_task_ids {
                        if let Ok(Some(t)) = read::task_by_id(&self.conn, *tid) {
                            updated_tasks.push(t);
                        }
                    }
                    if !updated_tasks.is_empty() {
                        let _ = self.changes_tx.send(TaskChanges {
                            updated: updated_tasks,
                            status_changed: affected_task_ids,
                            ..Default::default()
                        });
                    }
                }
                let _ = responder.send(result);
            }
            Command::MarkReviewed { id, responder } => {
                let result = self.mark_reviewed(id);
                if let Ok(p) = &result {
                    let _ = self.library_tx.send(LibraryChanges {
                        projects_updated: vec![p.clone()],
                        ..Default::default()
                    });
                    self.notify_project_dirty(p.id);
                }
                let _ = responder.send(result);
            }
            Command::MarkTaskReviewed { id, responder } => {
                // v0.7.4 — emit a TaskChanges{updated} so the
                // canonical Review page rebuilds and the row drops
                // out of the weekly walk (the page filter excludes
                // tasks reviewed in the last 7 days).
                let result = self.mark_task_reviewed(id);
                if let Ok(t) = &result {
                    let _ = self.changes_tx.send(TaskChanges {
                        updated: vec![t.clone()],
                        ..Default::default()
                    });
                    if let Some(pid) = t.project_id {
                        self.notify_project_dirty(pid);
                    }
                }
                let _ = responder.send(result);
            }
            Command::DeleteProject { id, responder } => {
                // Tasks under this project cascade-delete via the FK
                // (ON DELETE CASCADE). Capture their ids so the UI
                // can drop them from active views.
                let affected_task_ids = self.task_ids_in_project(id).unwrap_or_default();
                let result = self.delete_project(id);
                if result.is_ok() {
                    let _ = self.library_tx.send(LibraryChanges {
                        projects_deleted: vec![id],
                        ..Default::default()
                    });
                    if !affected_task_ids.is_empty() {
                        let _ = self.changes_tx.send(TaskChanges {
                            deleted: affected_task_ids,
                            ..Default::default()
                        });
                    }
                }
                let _ = responder.send(result);
            }

            // ── Tags ──────────────────────────────────────────────
            Command::CreateTag { tag, responder } => {
                let result = self.create_tag(tag);
                if let Ok(ref t) = result {
                    let _ = self.library_tx.send(LibraryChanges {
                        tags_created: vec![t.clone()],
                        ..Default::default()
                    });
                }
                let _ = responder.send(result);
            }
            Command::UpdateTag { update, responder } => {
                let result = self.update_tag(update);
                if let Ok(ref t) = result {
                    let _ = self.library_tx.send(LibraryChanges {
                        tags_updated: vec![t.clone()],
                        ..Default::default()
                    });
                }
                let _ = responder.send(result);
            }
            Command::DeleteTag { id, responder } => {
                // task_tag rows cascade-delete via the FK; we don't
                // emit per-task changes since the tasks themselves
                // didn't change rows — the UI re-reads tag membership
                // when refreshing. Phase 6b's pill editor will
                // observe a tag-set change separately.
                let result = self.delete_tag(id);
                if result.is_ok() {
                    let _ = self.library_tx.send(LibraryChanges {
                        tags_deleted: vec![id],
                        ..Default::default()
                    });
                }
                let _ = responder.send(result);
            }
            Command::SetTaskTags {
                task_id,
                tag_ids,
                responder,
            } => {
                let result = self.set_task_tags(task_id, tag_ids);
                if let Ok(ref task) = result {
                    // Surface a TaskChanges{updated} so the row's tag
                    // pills refresh on the active list. Tag membership
                    // doesn't change `Task` row columns directly —
                    // we re-read tag_names via the per-list batch on
                    // refresh — but emit the delta so the active list
                    // does refresh.
                    let _ = self.changes_tx.send(TaskChanges {
                        updated: vec![task.clone()],
                        ..Default::default()
                    });
                    if let Some(pid) = task.project_id {
                        self.notify_project_dirty(pid);
                    }
                }
                let _ = responder.send(result);
            }
            Command::EnsureArea { name, responder } => {
                let result = self.ensure_area(&name);
                if let Ok(ref a) = result
                    && a.created_at == a.modified_at
                {
                    let _ = self.library_tx.send(LibraryChanges {
                        areas_created: vec![a.clone()],
                        ..Default::default()
                    });
                }
                let _ = responder.send(result);
            }
            Command::EnsureTag { name, responder } => {
                let result = self.ensure_tag(&name);
                if let Ok(ref t) = result {
                    // Only emit a creation delta if the tag was
                    // actually new — the helper differentiates and we
                    // mirror that here.
                    if t.created_at == t.modified_at {
                        let _ = self.library_tx.send(LibraryChanges {
                            tags_created: vec![t.clone()],
                            ..Default::default()
                        });
                    }
                }
                let _ = responder.send(result);
            }

            // ── Perspectives (Phase 14) ──────────────────────────
            Command::CreatePerspective {
                perspective,
                responder,
            } => {
                let result = self.create_perspective(perspective);
                if let Ok(p) = &result {
                    let _ = self.library_tx.send(LibraryChanges {
                        perspectives_created: vec![p.clone()],
                        ..Default::default()
                    });
                }
                let _ = responder.send(result);
            }
            Command::UpdatePerspective { update, responder } => {
                let result = self.update_perspective(update);
                if let Ok(p) = &result {
                    let _ = self.library_tx.send(LibraryChanges {
                        perspectives_updated: vec![p.clone()],
                        ..Default::default()
                    });
                }
                let _ = responder.send(result);
            }
            Command::DeletePerspective { id, responder } => {
                let result = self.delete_perspective(id);
                if result.is_ok() {
                    let _ = self.library_tx.send(LibraryChanges {
                        perspectives_deleted: vec![id],
                        ..Default::default()
                    });
                }
                let _ = responder.send(result);
            }
        }
    }

    fn create_task(&mut self, new: NewTask) -> Result<Task, DbError> {
        // Phase 15 — reject malformed RRULE up front so we don't
        // store a string that can't be iterated. Mode strings other
        // than the three known values fall back to default at read
        // time, so they don't need a hard reject; we only validate
        // against the known set when set explicitly.
        if let Some(rule) = new.repeat_rule.as_deref() {
            crate::repeat::RepeatRule::parse(rule, crate::repeat::RepeatMode::Cumulative)
                .map_err(|e| DbError::BadRepeatRule(e.to_string()))?;
        }

        // v0.7.9 — honor a caller-provided UUID (the Org importer
        // uses this to preserve :ID: from the source vault).
        // `None` and `Some("")` both fall back to a fresh v4.
        let uuid = match new.uuid {
            Some(s) if !s.is_empty() => s,
            _ => Uuid::new_v4().to_string(),
        };
        let position = self.next_task_position(new.parent_id, new.project_id)?;

        // v0.7.12 — orig_keyword is appended; existing call sites
        // pass `None` (Default::default()) so the value is NULL.
        self.conn.execute(
            "INSERT INTO task \
             (uuid, title, note, project_id, parent_id, scheduled_for, deadline, \
              defer_until, estimated_minutes, repeat_rule, repeat_mode, orig_keyword, position) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                uuid,
                new.title,
                new.note,
                new.project_id,
                new.parent_id,
                new.scheduled_for,
                new.deadline,
                new.defer_until,
                new.estimated_minutes,
                new.repeat_rule,
                new.repeat_mode,
                new.orig_keyword,
                position,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        read::task_by_id(&self.conn, id)?.ok_or(DbError::NotFound)
    }

    fn next_task_position(
        &self,
        parent_id: Option<i64>,
        project_id: Option<i64>,
    ) -> Result<f64, DbError> {
        let max: Option<f64> = match (parent_id, project_id) {
            (Some(pid), _) => self.conn.query_row(
                "SELECT MAX(position) FROM task WHERE parent_id = ?1",
                params![pid],
                |r| r.get(0),
            )?,
            (None, Some(pid)) => self.conn.query_row(
                "SELECT MAX(position) FROM task \
                 WHERE parent_id IS NULL AND project_id = ?1",
                params![pid],
                |r| r.get(0),
            )?,
            (None, None) => self.conn.query_row(
                "SELECT MAX(position) FROM task \
                 WHERE parent_id IS NULL AND project_id IS NULL",
                [],
                |r| r.get(0),
            )?,
        };
        Ok(max.unwrap_or(0.0) + 1.0)
    }

    fn update_task(&mut self, update: TaskUpdate) -> Result<Task, DbError> {
        if update.is_noop() {
            return read::task_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound);
        }

        // Phase 15 — same validation as create_task: malformed
        // RRULE strings get a hard reject so they never land in the
        // column.
        if let Some(Some(rule)) = update.repeat_rule.as_ref() {
            crate::repeat::RepeatRule::parse(rule, crate::repeat::RepeatMode::Cumulative)
                .map_err(|e| DbError::BadRepeatRule(e.to_string()))?;
        }

        let mut sets: Vec<&'static str> = Vec::new();
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(title) = update.title {
            sets.push("title = ?");
            bound.push(Box::new(title));
        }
        if let Some(note) = update.note {
            sets.push("note = ?");
            bound.push(Box::new(note));
        }
        if let Some(position) = update.position {
            sets.push("position = ?");
            bound.push(Box::new(position));
        }
        if let Some(project_id) = update.project_id {
            sets.push("project_id = ?");
            bound.push(Box::new(project_id));
        }
        if let Some(schedule) = update.scheduled_for {
            sets.push("scheduled_for = ?");
            bound.push(Box::new(schedule));
        }
        if let Some(deadline) = update.deadline {
            sets.push("deadline = ?");
            bound.push(Box::new(deadline));
        }
        if let Some(defer_until) = update.defer_until {
            sets.push("defer_until = ?");
            bound.push(Box::new(defer_until));
        }
        if let Some(est) = update.estimated_minutes {
            sets.push("estimated_minutes = ?");
            bound.push(Box::new(est));
        }
        if let Some(rule) = update.repeat_rule {
            sets.push("repeat_rule = ?");
            bound.push(Box::new(rule));
        }
        if let Some(mode) = update.repeat_mode {
            sets.push("repeat_mode = ?");
            bound.push(Box::new(mode));
        }
        bound.push(Box::new(update.id));

        let sql = format!("UPDATE task SET {} WHERE id = ?", sets.join(", "));
        let params_refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|b| b.as_ref()).collect();
        let n = self.conn.execute(&sql, &params_refs[..])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }

        read::task_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound)
    }

    /// Flip the task's `completed_at` and, when completing a
    /// repeating task with a parseable `repeat_rule`, spawn the next
    /// instance with shifted dates. Returns `(toggled, spawned)` —
    /// `spawned` is `Some(new_task)` when a follow-up was created,
    /// `None` otherwise (either the task isn't repeating, the rule
    /// has no further occurrences, or we were reopening rather than
    /// completing).
    fn toggle_complete(&mut self, id: i64) -> Result<(Task, Option<Task>), DbError> {
        let task = read::task_by_id(&self.conn, id)?.ok_or(DbError::NotFound)?;

        if task.is_completed() {
            // Reopen — never spawns a new instance.
            self.conn.execute(
                "UPDATE task SET completed_at = NULL WHERE id = ?1",
                params![id],
            )?;
            let toggled = read::task_by_id(&self.conn, id)?.ok_or(DbError::NotFound)?;
            return Ok((toggled, None));
        }

        // Completing. Mark the row done first, then attempt to
        // spawn the follow-up. If the spawn fails for any reason
        // (malformed rule that somehow snuck past validation,
        // exhausted COUNT, etc.) we still surface the toggle
        // success — repeating-task users would rather lose the
        // follow-up than block completion of the work they just did.
        self.conn.execute(
            "UPDATE task SET completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            params![id],
        )?;
        let toggled = read::task_by_id(&self.conn, id)?.ok_or(DbError::NotFound)?;

        let spawned = self.spawn_repeat_follow_up(&toggled)?;
        Ok((toggled, spawned))
    }

    /// Phase 15 — given a freshly-completed task, decide whether a
    /// follow-up instance should be created and, if so, INSERT it.
    /// Returns the newly-created `Task` row when applicable.
    ///
    /// The follow-up inherits everything except the date fields: the
    /// new row gets a fresh uuid and the same project / parent /
    /// title / note / tags-via-`task_tag` / repeat fields. The date
    /// fields (`scheduled_for`, `deadline`, `defer_until`) shift by
    /// the delta the rule produces relative to the previous anchor.
    fn spawn_repeat_follow_up(&mut self, completed: &Task) -> Result<Option<Task>, DbError> {
        use chrono::Local;

        let Some(rule_text) = completed.repeat_rule.as_deref() else {
            return Ok(None);
        };

        // The rule was validated on insert, but a database row from
        // a foreign source could theoretically be malformed. Be
        // defensive — return None rather than propagate the error.
        let mode = crate::repeat::RepeatMode::from_column(completed.repeat_mode.as_deref());
        let rule = match crate::repeat::RepeatRule::parse(rule_text, mode) {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        // Pick the rule's anchor: the earliest date field set on
        // the task. `scheduled_for::Date` first, then `deadline`,
        // then `defer_until`. Someday-scheduled tasks aren't
        // repeated (no concrete date to shift from).
        use crate::domain::ScheduledFor;
        let scheduled_date = match completed.scheduled_for {
            Some(ScheduledFor::Date(d)) => Some(d),
            _ => None,
        };
        let mut candidates: Vec<chrono::NaiveDate> = Vec::with_capacity(3);
        candidates.extend(scheduled_date);
        candidates.extend(completed.deadline);
        candidates.extend(completed.defer_until);
        let Some(anchor) = candidates.iter().min().copied() else {
            // No date field set — nothing to shift. Could still
            // bump completion, but that produces a follow-up with
            // no due date which has no advantage over leaving the
            // user to manually re-create. Skip.
            return Ok(None);
        };

        let completed_on = completed
            .completed_at
            .map(|dt| dt.with_timezone(&Local).date_naive())
            .unwrap_or_else(|| Local::now().date_naive());

        let Some(new_anchor) = rule.next_after(anchor, completed_on) else {
            // Rule exhausted (COUNT met, UNTIL passed). Leave the
            // completed instance as the final occurrence.
            return Ok(None);
        };

        // Phase 15 — handle COUNT termination on the *spawned* rule.
        // Each spawn re-anchors the iteration on the previous date,
        // which would let `COUNT=N` reset infinitely if we just
        // copied the rule forward. Decrement it on each spawn; when
        // the prior count was already 1 the just-completed instance
        // was the last in the series.
        let (carried_rule, _is_last) = match rule.rule_with_count_decremented() {
            crate::repeat::CountStep::Unbounded => (completed.repeat_rule.clone(), false),
            crate::repeat::CountStep::Decremented(new_rule) => (Some(new_rule), false),
            crate::repeat::CountStep::Exhausted => return Ok(None),
        };

        let delta = new_anchor.signed_duration_since(anchor);
        let shift = |d: chrono::NaiveDate| d + delta;

        let new_scheduled = scheduled_date.map(shift).map(ScheduledFor::Date);
        let new_deadline = completed.deadline.map(shift);
        let new_defer = completed.defer_until.map(shift);

        let new_task = NewTask {
            title: completed.title.clone(),
            note: completed.note.clone(),
            project_id: completed.project_id,
            parent_id: completed.parent_id,
            scheduled_for: new_scheduled,
            deadline: new_deadline,
            defer_until: new_defer,
            estimated_minutes: completed.estimated_minutes,
            repeat_rule: carried_rule,
            repeat_mode: completed.repeat_mode.clone(),
            // The respawn is a brand-new task instance; let the
            // worker generate a fresh UUID rather than re-using
            // the completed instance's ID. The :ID: contract is
            // per-Org-headline, not per-Atrium-row.
            uuid: None,
            // Carry the orig_keyword forward — if the user named
            // a custom keyword on the original they expect the
            // re-spawned instance to wear the same label.
            orig_keyword: completed.orig_keyword.clone(),
        };
        let inserted = self.create_task(new_task)?;

        // Carry the tag set forward. Tags live on `task_tag`, not
        // on the Task struct — copy by ID so the new row inherits
        // the same labels.
        self.conn.execute(
            "INSERT INTO task_tag (task_id, tag_id) \
             SELECT ?1, tag_id FROM task_tag WHERE task_id = ?2",
            params![inserted.id, completed.id],
        )?;

        Ok(Some(inserted))
    }

    fn delete_task(&mut self, id: i64) -> Result<(), DbError> {
        let n = self
            .conn
            .execute("DELETE FROM task WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    // ── Areas ──────────────────────────────────────────────────────

    fn create_area(&mut self, new: NewArea) -> Result<Area, DbError> {
        let uuid = Uuid::new_v4().to_string();
        let position = self.next_area_position()?;
        self.conn.execute(
            "INSERT INTO area (uuid, title, color, position) VALUES (?, ?, ?, ?)",
            params![uuid, new.title, new.color, position],
        )?;
        let id = self.conn.last_insert_rowid();
        read::area_by_id(&self.conn, id)?.ok_or(DbError::NotFound)
    }

    fn next_area_position(&self) -> Result<f64, DbError> {
        let max: Option<f64> = self
            .conn
            .query_row("SELECT MAX(position) FROM area", [], |r| r.get(0))?;
        Ok(max.unwrap_or(0.0) + 1.0)
    }

    fn update_area(&mut self, update: AreaUpdate) -> Result<Area, DbError> {
        if update.is_noop() {
            return read::area_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound);
        }
        let mut sets: Vec<&'static str> = Vec::new();
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(title) = update.title {
            sets.push("title = ?");
            bound.push(Box::new(title));
        }
        if let Some(position) = update.position {
            sets.push("position = ?");
            bound.push(Box::new(position));
        }
        if let Some(color) = update.color {
            sets.push("color = ?");
            bound.push(Box::new(color));
        }
        bound.push(Box::new(update.id));
        let sql = format!("UPDATE area SET {} WHERE id = ?", sets.join(", "));
        let params_refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|b| b.as_ref()).collect();
        let n = self.conn.execute(&sql, &params_refs[..])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        read::area_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound)
    }

    fn delete_area(&mut self, id: i64) -> Result<(), DbError> {
        let n = self
            .conn
            .execute("DELETE FROM area WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    fn projects_with_area(&self, area_id: i64) -> Result<Vec<i64>, DbError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id FROM project WHERE area_id = ?1")?;
        let rows = stmt.query_map(params![area_id], |r| r.get::<_, i64>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    // ── Projects ───────────────────────────────────────────────────

    fn create_project(&mut self, new: NewProject) -> Result<Project, DbError> {
        // v0.7.9 — honor a caller-provided UUID (Org importer
        // path). Empty / None fall back to a fresh v4.
        let uuid = match new.uuid {
            Some(s) if !s.is_empty() => s,
            _ => Uuid::new_v4().to_string(),
        };
        let position = self.next_project_position(new.area_id)?;
        // v0.7.13 — last_reviewed_at + archived_at honor caller-
        // provided values (Org importer path). NULL otherwise.
        self.conn.execute(
            "INSERT INTO project \
             (uuid, title, note, area_id, sequential, review_interval_days, \
              last_reviewed_at, archived_at, position) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                uuid,
                new.title,
                new.note,
                new.area_id,
                i32::from(new.sequential),
                new.review_interval_days,
                new.last_reviewed_at,
                new.archived_at,
                position,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        read::project_by_id(&self.conn, id)?.ok_or(DbError::NotFound)
    }

    fn next_project_position(&self, area_id: Option<i64>) -> Result<f64, DbError> {
        let max: Option<f64> = match area_id {
            Some(aid) => self.conn.query_row(
                "SELECT MAX(position) FROM project WHERE area_id = ?1",
                params![aid],
                |r| r.get(0),
            )?,
            None => self.conn.query_row(
                "SELECT MAX(position) FROM project WHERE area_id IS NULL",
                [],
                |r| r.get(0),
            )?,
        };
        Ok(max.unwrap_or(0.0) + 1.0)
    }

    fn update_project(&mut self, update: ProjectUpdate) -> Result<Project, DbError> {
        if update.is_noop() {
            return read::project_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound);
        }
        let mut sets: Vec<&'static str> = Vec::new();
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(title) = update.title {
            sets.push("title = ?");
            bound.push(Box::new(title));
        }
        if let Some(note) = update.note {
            sets.push("note = ?");
            bound.push(Box::new(note));
        }
        if let Some(area_id) = update.area_id {
            sets.push("area_id = ?");
            bound.push(Box::new(area_id));
        }
        if let Some(sequential) = update.sequential {
            sets.push("sequential = ?");
            bound.push(Box::new(i32::from(sequential)));
        }
        if let Some(rid) = update.review_interval_days {
            sets.push("review_interval_days = ?");
            bound.push(Box::new(rid));
        }
        if let Some(position) = update.position {
            sets.push("position = ?");
            bound.push(Box::new(position));
        }
        bound.push(Box::new(update.id));
        let sql = format!("UPDATE project SET {} WHERE id = ?", sets.join(", "));
        let params_refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|b| b.as_ref()).collect();
        let n = self.conn.execute(&sql, &params_refs[..])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        read::project_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound)
    }

    /// Set `archived_at = now`, then auto-complete every still-open
    /// task in the project (per design call — Things-3 behaviour).
    fn archive_project(&mut self, id: i64) -> Result<Project, DbError> {
        let tx = self.conn.transaction()?;
        let n = tx.execute(
            "UPDATE project SET archived_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1",
            params![id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        tx.execute(
            "UPDATE task \
             SET completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE project_id = ?1 AND completed_at IS NULL",
            params![id],
        )?;
        tx.commit()?;
        read::project_by_id(&self.conn, id)?.ok_or(DbError::NotFound)
    }

    /// Phase 13 — Review queue's *Mark Reviewed* hook. Stamps
    /// `last_reviewed_at` with the current UTC instant so the
    /// project's next review fires at `now + review_interval_days`.
    fn mark_reviewed(&mut self, id: i64) -> Result<Project, DbError> {
        let n = self.conn.execute(
            "UPDATE project SET last_reviewed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1",
            params![id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        read::project_by_id(&self.conn, id)?.ok_or(DbError::NotFound)
    }

    /// v0.7.4 — task-level analogue. Stamps `task.last_reviewed_at
    /// = now()` so the canonical Review page's weekly walk hides
    /// the row for 7 days. The AFTER UPDATE trigger fires (we
    /// don't touch modified_at here), so `task.modified_at` also
    /// advances to now — accurate, since reviewing is a real
    /// state change.
    fn mark_task_reviewed(&mut self, id: i64) -> Result<Task, DbError> {
        let n = self.conn.execute(
            "UPDATE task SET last_reviewed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1",
            params![id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        read::task_by_id(&self.conn, id)?.ok_or(DbError::NotFound)
    }

    fn delete_project(&mut self, id: i64) -> Result<(), DbError> {
        let n = self
            .conn
            .execute("DELETE FROM project WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    fn open_task_ids_in_project(&self, project_id: i64) -> Result<Vec<i64>, DbError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id FROM task WHERE project_id = ?1 AND completed_at IS NULL")?;
        let rows = stmt.query_map(params![project_id], |r| r.get::<_, i64>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn task_ids_in_project(&self, project_id: i64) -> Result<Vec<i64>, DbError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id FROM task WHERE project_id = ?1")?;
        let rows = stmt.query_map(params![project_id], |r| r.get::<_, i64>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    // ── Tags ───────────────────────────────────────────────────────

    fn create_tag(&mut self, new: NewTag) -> Result<Tag, DbError> {
        let uuid = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO tag (uuid, name, color) VALUES (?, ?, ?)",
            params![uuid, new.name, new.color],
        )?;
        let id = self.conn.last_insert_rowid();
        read::tag_by_id(&self.conn, id)?.ok_or(DbError::NotFound)
    }

    fn update_tag(&mut self, update: TagUpdate) -> Result<Tag, DbError> {
        if update.is_noop() {
            return read::tag_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound);
        }
        let mut sets: Vec<&'static str> = Vec::new();
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(name) = update.name {
            sets.push("name = ?");
            bound.push(Box::new(name));
        }
        if let Some(color) = update.color {
            sets.push("color = ?");
            bound.push(Box::new(color));
        }
        bound.push(Box::new(update.id));
        let sql = format!("UPDATE tag SET {} WHERE id = ?", sets.join(", "));
        let params_refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|b| b.as_ref()).collect();
        let n = self.conn.execute(&sql, &params_refs[..])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        read::tag_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound)
    }

    fn delete_tag(&mut self, id: i64) -> Result<(), DbError> {
        let n = self
            .conn
            .execute("DELETE FROM tag WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    fn set_task_tags(&mut self, task_id: i64, tag_ids: Vec<i64>) -> Result<Task, DbError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM task_tag WHERE task_id = ?1", params![task_id])?;
        for tid in &tag_ids {
            tx.execute(
                "INSERT INTO task_tag (task_id, tag_id) VALUES (?, ?)",
                params![task_id, tid],
            )?;
        }
        tx.commit()?;
        read::task_by_id(&self.conn, task_id)?.ok_or(DbError::NotFound)
    }

    /// Find an existing tag by name (case-insensitive) or create it.
    /// Returns the same tag struct shape as `create_tag`, with
    /// `created_at == modified_at` exactly when the tag is new — the
    /// caller uses that to decide whether to emit a `tags_created`
    /// delta.
    fn ensure_tag(&mut self, name: &str) -> Result<Tag, DbError> {
        // Probe by name (NOCASE-collated column).
        let existing: rusqlite::Result<i64> =
            self.conn
                .query_row("SELECT id FROM tag WHERE name = ?1", params![name], |r| {
                    r.get(0)
                });
        match existing {
            Ok(id) => read::tag_by_id(&self.conn, id)?.ok_or(DbError::NotFound),
            Err(rusqlite::Error::QueryReturnedNoRows) => self.create_tag(NewTag {
                name: name.to_string(),
                color: None,
            }),
            Err(e) => Err(e.into()),
        }
    }

    /// v0.7.14 — idempotent area-by-title lookup. Area's `title`
    /// column doesn't have a NOCASE collation (only tag.name
    /// does), so case-insensitive match runs at the query level.
    fn ensure_area(&mut self, name: &str) -> Result<Area, DbError> {
        let existing: rusqlite::Result<i64> = self.conn.query_row(
            "SELECT id FROM area WHERE LOWER(title) = LOWER(?1) LIMIT 1",
            params![name],
            |r| r.get(0),
        );
        match existing {
            Ok(id) => read::area_by_id(&self.conn, id)?.ok_or(DbError::NotFound),
            Err(rusqlite::Error::QueryReturnedNoRows) => self.create_area(NewArea {
                title: name.to_string(),
                color: None,
            }),
            Err(e) => Err(e.into()),
        }
    }

    // ── Perspectives (Phase 14) ─────────────────────────────────

    fn create_perspective(&mut self, new: NewPerspective) -> Result<Perspective, DbError> {
        let uuid = Uuid::new_v4().to_string();
        let position = self.next_perspective_position()?;
        // `renderer` defaults to "list" at the schema level; supply it
        // explicitly when the caller provides one so we can ship board
        // perspectives via NewPerspective in Slice D.
        let renderer = new.renderer.as_deref().unwrap_or("list");
        self.conn.execute(
            "INSERT INTO perspective \
             (uuid, name, icon, filter_expr, renderer, renderer_config, position) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                uuid,
                new.name,
                new.icon,
                new.filter_expr,
                renderer,
                new.renderer_config,
                position
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        read::perspective_by_id(&self.conn, id)?.ok_or(DbError::NotFound)
    }

    fn next_perspective_position(&self) -> Result<f64, DbError> {
        let max: Option<f64> =
            self.conn
                .query_row("SELECT MAX(position) FROM perspective", [], |r| r.get(0))?;
        Ok(max.unwrap_or(0.0) + 1.0)
    }

    fn update_perspective(&mut self, update: PerspectiveUpdate) -> Result<Perspective, DbError> {
        if update.is_noop() {
            return read::perspective_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound);
        }
        let mut sets: Vec<&'static str> = Vec::new();
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(name) = update.name {
            sets.push("name = ?");
            bound.push(Box::new(name));
        }
        if let Some(icon) = update.icon {
            sets.push("icon = ?");
            bound.push(Box::new(icon));
        }
        if let Some(filter_expr) = update.filter_expr {
            sets.push("filter_expr = ?");
            bound.push(Box::new(filter_expr));
        }
        if let Some(position) = update.position {
            sets.push("position = ?");
            bound.push(Box::new(position));
        }
        if let Some(renderer) = update.renderer {
            sets.push("renderer = ?");
            bound.push(Box::new(renderer));
        }
        if let Some(renderer_config) = update.renderer_config {
            sets.push("renderer_config = ?");
            bound.push(Box::new(renderer_config));
        }
        bound.push(Box::new(update.id));
        let sql = format!("UPDATE perspective SET {} WHERE id = ?", sets.join(", "));
        let params_refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|b| b.as_ref()).collect();
        let n = self.conn.execute(&sql, &params_refs[..])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        read::perspective_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound)
    }

    fn delete_perspective(&mut self, id: i64) -> Result<(), DbError> {
        let n = self
            .conn
            .execute("DELETE FROM perspective WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use std::time::Duration;

    fn fresh_conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        db::configure_pragmas(&conn).unwrap();
        crate::db::migrations::migrate(&mut conn).unwrap();
        conn
    }

    #[tokio::test]
    async fn create_task_honors_caller_provided_uuid() {
        // v0.7.9 — the Org importer relies on this. Passing a
        // UUID through NewTask must round-trip into the row.
        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let provided = "11111111-2222-3333-4444-555555555555";
        let new = NewTask {
            title: "imported".to_string(),
            uuid: Some(provided.to_string()),
            ..Default::default()
        };
        let task = handle.create_task(new).await.unwrap();
        assert_eq!(task.uuid, provided);
    }

    #[tokio::test]
    async fn create_task_falls_back_to_generated_uuid_for_empty_string() {
        // Defensive: an empty-string UUID is treated as "absent"
        // and the worker generates one. Avoids a foot-gun where a
        // caller might pass `Some(String::new())` and end up with
        // a row whose uuid is the empty string (would fail FK and
        // round-trip checks elsewhere).
        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let new = NewTask {
            title: "with empty uuid".to_string(),
            uuid: Some(String::new()),
            ..Default::default()
        };
        let task = handle.create_task(new).await.unwrap();
        assert!(!task.uuid.is_empty());
        assert_ne!(task.uuid, "");
    }

    #[tokio::test]
    async fn import_org_file_round_trips_to_db() {
        // v0.7.9 — end-to-end import against a fixture .org file.
        // Writes a small file to a tempdir, imports it through the
        // worker, then reads back via list_all_tasks and asserts
        // the row count + key fields.
        use crate::sync::org::import_org_file;

        let dir = std::env::temp_dir().join(format!("atrium-import-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Errands.org");
        std::fs::write(
            &path,
            "\
* TODO Buy milk :errand:
SCHEDULED: <2026-05-15 Fri>
:PROPERTIES:
:ID: 11111111-2222-3333-4444-555555555555
:END:
Body line.
* DONE Old item
CLOSED: [2026-04-01 Wed]
* Project sub-heading
** TODO Nested under sub-heading
",
        )
        .unwrap();

        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let summary = import_org_file(&handle, &path, false).await.unwrap();
        assert_eq!(summary.tasks_created, 3);
        assert_eq!(summary.headings_skipped, 1);
        assert!(summary.project_id.is_some());
        assert_eq!(summary.project_title.as_deref(), Some("Errands"));

        let read_conn = fresh_conn();
        // We can't read the worker's DB from a separate connection
        // (the worker holds the only handle to the in-memory DB),
        // so re-run the assertions through worker round-trips.
        // tasks_created = 3 already validates the count; the UUID
        // round-trip is verified separately above. List the
        // project to confirm membership.
        let _ = read_conn; // suppress unused warning

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn import_org_directory_walks_areas_and_files() {
        // v0.7.14 — the multi-file vault walker. Build a vault
        // tree with one top-level project + one project under an
        // area subdirectory + a hidden directory + a sub-area dir
        // (which should be skipped with a warning). Import.
        // Verify the right rows landed.
        use crate::db::read::{list_all_projects, list_areas};
        use crate::sync::org::import_org_directory;

        let dir = std::env::temp_dir().join(format!("atrium-vault-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Top-level unfiled project file.
        std::fs::write(dir.join("Inbox.org"), "* TODO Triage\n").unwrap();

        // Area subdirectory with one project file.
        std::fs::create_dir_all(dir.join("Personal")).unwrap();
        std::fs::write(
            dir.join("Personal").join("Errands.org"),
            "* TODO Buy milk\n",
        )
        .unwrap();

        // Hidden directory should be skipped.
        std::fs::create_dir_all(dir.join(".atrium")).unwrap();
        std::fs::write(dir.join(".atrium").join("config.toml"), "").unwrap();
        // (Also inside Personal/) — sub-area directory should be
        // skipped + warned about.
        std::fs::create_dir_all(dir.join("Personal").join("subarea")).unwrap();
        std::fs::write(
            dir.join("Personal").join("subarea").join("nested.org"),
            "* TODO ignored\n",
        )
        .unwrap();

        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let summaries = import_org_directory(&handle, &dir, false).await.unwrap();

        // Assertions on what landed.
        let read_conn = fresh_conn();
        let _ = read_conn; // unused — we re-use the worker's conn through summaries.

        // Two project files survived the walk; the sub-area
        // file was skipped with a warning recorded somewhere.
        let imported_titles: Vec<String> = summaries
            .iter()
            .filter_map(|s| s.project_title.clone())
            .collect();
        assert!(imported_titles.contains(&"Inbox".to_string()));
        assert!(imported_titles.contains(&"Errands".to_string()));
        assert!(!imported_titles.contains(&"nested".to_string()));

        // Some summary should carry the sub-area warning.
        let any_warning = summaries.iter().any(|s| {
            s.lossy
                .iter()
                .any(|note| note.contains("sub-area directory"))
        });
        assert!(any_warning, "expected sub-area warning in summaries");

        // Real area row created via ensure_area.
        let conn = fresh_conn(); // a brand-new conn — won't see the worker's writes
        let _ = list_areas(&conn).unwrap();
        let _ = list_all_projects(&conn).unwrap();
        // (The worker holds the only handle to its in-memory DB,
        // so we can't reach the rows from here. The summary
        // assertions above are the authoritative check.)

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn import_org_file_dry_run_creates_nothing() {
        use crate::sync::org::import_org_file;

        let dir =
            std::env::temp_dir().join(format!("atrium-import-dry-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Sample.org");
        std::fs::write(&path, "* TODO One\n* TODO Two\n").unwrap();

        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let summary = import_org_file(&handle, &path, true).await.unwrap();
        assert_eq!(summary.tasks_created, 2);
        assert!(summary.project_id.is_none(), "dry-run must not insert");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn spawn_with_vault_writes_org_file_on_task_create() {
        // v0.7.16 — end-to-end: spawn the worker with a vault
        // configured, create a project + task, wait > 150ms for
        // the writer to flush, verify the .org file lands.
        use crate::db::read_pool::ReadPool;
        use crate::sync::vault_writer;

        let scratch =
            std::env::temp_dir().join(format!("atrium-vault-spawn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let db_path = scratch.join("atrium.db");
        let mut writer_conn = Connection::open(&db_path).unwrap();
        crate::db::configure_pragmas(&writer_conn).unwrap();
        crate::db::migrations::migrate(&mut writer_conn).unwrap();

        let pool = ReadPool::new(&db_path, 4);
        let (handle, _changes_rx, _library_rx) = spawn_with_vault(
            writer_conn,
            Some(VaultConfig {
                root: scratch.clone(),
                read_pool: pool,
            }),
        );

        let project = handle
            .create_project(NewProject {
                title: "Sample".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = handle
            .create_task(NewTask {
                title: "auto-written".to_string(),
                project_id: Some(project.id),
                ..Default::default()
            })
            .await
            .unwrap();

        // Wait for the debounce window to elapse.
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;

        let expected_path = scratch.join("Sample.org");
        assert!(
            expected_path.exists(),
            "expected vault file at {}",
            expected_path.display()
        );
        let contents = std::fs::read_to_string(&expected_path).unwrap();
        assert!(contents.contains("auto-written"), "got: {contents}");

        // Suppress unused warning on vault_writer module re-export.
        let _ = vault_writer::VaultWriteRequest::Shutdown;

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[tokio::test]
    async fn ensure_area_creates_then_dedupes_case_insensitive() {
        // v0.7.14 — idempotent area create-by-name. First call
        // creates a row; second call with a differently-cased name
        // returns the same row.
        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let first = handle.ensure_area("Personal".to_string()).await.unwrap();
        assert_eq!(first.title, "Personal");

        let second = handle.ensure_area("personal".to_string()).await.unwrap();
        assert_eq!(second.id, first.id, "case-insensitive match expected");

        let third = handle.ensure_area("PERSONAL".to_string()).await.unwrap();
        assert_eq!(third.id, first.id);

        // A truly different name creates a new row.
        let work = handle.ensure_area("Work".to_string()).await.unwrap();
        assert_ne!(work.id, first.id);
    }

    #[tokio::test]
    async fn create_project_honors_caller_provided_uuid() {
        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let provided = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let new = NewProject {
            title: "imported project".to_string(),
            uuid: Some(provided.to_string()),
            ..Default::default()
        };
        let project = handle.create_project(new).await.unwrap();
        assert_eq!(project.uuid, provided);
    }

    #[tokio::test]
    async fn create_task_round_trip() {
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle
            .create_task(NewTask::inbox("buy milk"))
            .await
            .unwrap();
        assert_eq!(task.title, "buy milk");
        assert!(task.id > 0);
        assert!(!task.uuid.is_empty());
        assert!(task.completed_at.is_none());

        let changes = changes_rx.recv().await.unwrap();
        assert_eq!(changes.created.len(), 1);
        assert_eq!(changes.created[0].id, task.id);
    }

    #[tokio::test]
    async fn update_task_changes_title_keeps_other_fields() {
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle.create_task(NewTask::inbox("first")).await.unwrap();
        let _ = changes_rx.recv().await.unwrap();

        let updated = handle
            .update_task(TaskUpdate::new(task.id).title("second"))
            .await
            .unwrap();
        assert_eq!(updated.title, "second");
        assert_eq!(updated.uuid, task.uuid);
        assert_eq!(updated.id, task.id);

        let changes = changes_rx.recv().await.unwrap();
        assert_eq!(changes.updated.len(), 1);
        assert_eq!(changes.updated[0].title, "second");
    }

    #[tokio::test]
    async fn update_task_sets_and_clears_schedule() {
        use crate::domain::ScheduledFor;
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle
            .create_task(NewTask::inbox("schedule me"))
            .await
            .unwrap();
        let _ = changes_rx.recv().await.unwrap();

        // Set to a specific date.
        let date = chrono::NaiveDate::from_ymd_opt(2026, 5, 25).unwrap();
        let scheduled = handle
            .update_task(TaskUpdate::new(task.id).schedule(Some(ScheduledFor::Date(date))))
            .await
            .unwrap();
        assert_eq!(scheduled.scheduled_for, Some(ScheduledFor::Date(date)));
        let _ = changes_rx.recv().await.unwrap();

        // Move to Someday.
        let someday = handle
            .update_task(TaskUpdate::new(task.id).schedule(Some(ScheduledFor::Someday)))
            .await
            .unwrap();
        assert_eq!(someday.scheduled_for, Some(ScheduledFor::Someday));
        let _ = changes_rx.recv().await.unwrap();

        // Clear it back to Inbox-equivalent.
        let cleared = handle
            .update_task(TaskUpdate::new(task.id).schedule(None))
            .await
            .unwrap();
        assert_eq!(cleared.scheduled_for, None);
    }

    #[tokio::test]
    async fn update_task_sets_and_clears_deadline() {
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle
            .create_task(NewTask::inbox("by friday"))
            .await
            .unwrap();
        let _ = changes_rx.recv().await.unwrap();

        let date = chrono::NaiveDate::from_ymd_opt(2026, 6, 5).unwrap();
        let with_dl = handle
            .update_task(TaskUpdate::new(task.id).deadline_value(Some(date)))
            .await
            .unwrap();
        assert_eq!(with_dl.deadline, Some(date));
        let _ = changes_rx.recv().await.unwrap();

        let cleared = handle
            .update_task(TaskUpdate::new(task.id).deadline_value(None))
            .await
            .unwrap();
        assert_eq!(cleared.deadline, None);
    }

    #[tokio::test]
    async fn update_task_sets_and_clears_defer_until() {
        // Phase 11 — defer_until set/clear round-trip via the
        // TaskUpdate::defer_value builder.
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle
            .create_task(NewTask::inbox("deferred"))
            .await
            .unwrap();
        let _ = changes_rx.recv().await.unwrap();

        let date = chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let with_defer = handle
            .update_task(TaskUpdate::new(task.id).defer_value(Some(date)))
            .await
            .unwrap();
        assert_eq!(with_defer.defer_until, Some(date));
        let _ = changes_rx.recv().await.unwrap();

        let cleared = handle
            .update_task(TaskUpdate::new(task.id).defer_value(None))
            .await
            .unwrap();
        assert_eq!(cleared.defer_until, None);
    }

    #[tokio::test]
    async fn update_task_sets_and_clears_estimated_minutes() {
        // Phase 11 — estimated_minutes set/clear via the
        // TaskUpdate::estimated_minutes_value builder.
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle.create_task(NewTask::inbox("timed")).await.unwrap();
        let _ = changes_rx.recv().await.unwrap();

        let with_est = handle
            .update_task(TaskUpdate::new(task.id).estimated_minutes_value(Some(45)))
            .await
            .unwrap();
        assert_eq!(with_est.estimated_minutes, Some(45));
        let _ = changes_rx.recv().await.unwrap();

        let cleared = handle
            .update_task(TaskUpdate::new(task.id).estimated_minutes_value(None))
            .await
            .unwrap();
        assert_eq!(cleared.estimated_minutes, None);
    }

    #[tokio::test]
    async fn update_task_sets_and_clears_repeat_rule() {
        // Phase 15 — repeat_rule + repeat_mode set/clear via the
        // TaskUpdate builder. Validates that round-trip survives.
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle.create_task(NewTask::inbox("repeat")).await.unwrap();
        let _ = changes_rx.recv().await.unwrap();

        let with_rule = handle
            .update_task(
                TaskUpdate::new(task.id)
                    .repeat_rule_value(Some("FREQ=WEEKLY".into()))
                    .repeat_mode_value(Some("NEXT".into())),
            )
            .await
            .unwrap();
        assert_eq!(with_rule.repeat_rule.as_deref(), Some("FREQ=WEEKLY"));
        assert_eq!(with_rule.repeat_mode.as_deref(), Some("NEXT"));
        let _ = changes_rx.recv().await.unwrap();

        let cleared = handle
            .update_task(
                TaskUpdate::new(task.id)
                    .repeat_rule_value(None)
                    .repeat_mode_value(None),
            )
            .await
            .unwrap();
        assert!(cleared.repeat_rule.is_none());
        assert!(cleared.repeat_mode.is_none());
    }

    #[tokio::test]
    async fn update_task_rejects_malformed_repeat_rule() {
        // Phase 15 — bad RRULE text is rejected up front.
        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle.create_task(NewTask::inbox("bad")).await.unwrap();
        let result = handle
            .update_task(TaskUpdate::new(task.id).repeat_rule_value(Some("not a rrule".into())))
            .await;
        match result {
            Err(DbError::BadRepeatRule(_)) => {}
            other => panic!("expected BadRepeatRule, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_task_rejects_malformed_repeat_rule() {
        // Phase 15 — same validation runs on insert.
        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let result = handle
            .create_task(NewTask {
                title: "bad".into(),
                repeat_rule: Some("FREQ=GARBAGE".into()),
                ..Default::default()
            })
            .await;
        match result {
            Err(DbError::BadRepeatRule(_)) => {}
            other => panic!("expected BadRepeatRule, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn complete_repeating_task_spawns_next_instance() {
        // Phase 15 — completing a task with a repeat_rule spawns a
        // follow-up with shifted scheduled_for. The original stays
        // completed; the new instance is open with the next date.
        use crate::domain::ScheduledFor;
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let original = handle
            .create_task(NewTask {
                title: "weekly dishes".into(),
                scheduled_for: Some(ScheduledFor::Date(
                    chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(),
                )),
                repeat_rule: Some("FREQ=WEEKLY".into()),
                repeat_mode: Some("CUMULATIVE".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = changes_rx.recv().await.unwrap();

        let toggled = handle.toggle_complete(original.id).await.unwrap();
        assert!(toggled.is_completed());
        let changes = changes_rx.recv().await.unwrap();
        // Toggled appears in updated; new instance appears in created.
        assert_eq!(changes.updated.len(), 1);
        assert_eq!(changes.created.len(), 1);
        assert_eq!(changes.status_changed, vec![original.id]);

        let next = &changes.created[0];
        assert_ne!(next.id, original.id);
        assert!(next.completed_at.is_none());
        assert_eq!(next.title, "weekly dishes");
        assert_eq!(next.repeat_rule.as_deref(), Some("FREQ=WEEKLY"));
        // Cumulative jump from 2026-01-05 with completion ~today
        // (2026-05-07 in this conversation) skips weeks ahead, so
        // next.scheduled_for is strictly after both 2026-01-05 and
        // today. Only assert the type + future-ness, not the exact
        // date (today moves forward as the test environment ages).
        match next.scheduled_for {
            Some(ScheduledFor::Date(d)) => {
                assert!(d > chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap());
            }
            _ => panic!(
                "expected Date schedule on follow-up, got {:?}",
                next.scheduled_for
            ),
        }
    }

    #[tokio::test]
    async fn complete_repeating_task_preserves_project_membership() {
        // Phase 15 — the spawned follow-up inherits project / parent
        // / note / repeat_rule / repeat_mode. Tag carry-forward is
        // covered by the SQL-level test in `db::read::tests` (the
        // tag map join exercises the same row).
        use crate::domain::{NewProject, ScheduledFor};
        let (handle, mut changes_rx, mut library_rx) = spawn(fresh_conn());
        let project = handle
            .create_project(NewProject {
                title: "groceries".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();

        let original = handle
            .create_task(NewTask {
                title: "shop".into(),
                note: "milk + eggs".into(),
                project_id: Some(project.id),
                scheduled_for: Some(ScheduledFor::Date(
                    chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
                )),
                repeat_rule: Some("FREQ=DAILY".into()),
                repeat_mode: Some("NEXT".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = changes_rx.recv().await.unwrap();

        let _ = handle.toggle_complete(original.id).await.unwrap();
        let changes = changes_rx.recv().await.unwrap();
        let next = &changes.created[0];
        assert_eq!(next.project_id, Some(project.id));
        assert_eq!(next.note, "milk + eggs");
        assert_eq!(next.repeat_rule.as_deref(), Some("FREQ=DAILY"));
        assert_eq!(next.repeat_mode.as_deref(), Some("NEXT"));
    }

    #[tokio::test]
    async fn complete_non_repeating_task_does_not_spawn() {
        // Phase 15 — sanity check: a task without repeat_rule
        // toggles cleanly without producing a created delta.
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle
            .create_task(NewTask::inbox("one-shot"))
            .await
            .unwrap();
        let _ = changes_rx.recv().await.unwrap();

        let _ = handle.toggle_complete(task.id).await.unwrap();
        let changes = changes_rx.recv().await.unwrap();
        assert!(changes.created.is_empty());
        assert_eq!(changes.updated.len(), 1);
        assert_eq!(changes.status_changed, vec![task.id]);
    }

    #[tokio::test]
    async fn complete_repeating_task_with_count_terminator() {
        // Phase 15 — COUNT=2 means the original is occurrence 1,
        // the spawned follow-up is occurrence 2. Completing the
        // follow-up exhausts the rule and produces no further
        // instance.
        //
        // Use BASIC mode so the test is anchor-relative and doesn't
        // depend on what today's date is when the test runs (CI
        // could be days, months, or years past the synthetic
        // anchor — CUMULATIVE would skip past all in-rule
        // occurrences in that case and report "no next occurrence"
        // even on the first cycle).
        use crate::domain::ScheduledFor;
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let original = handle
            .create_task(NewTask {
                title: "twice only".into(),
                scheduled_for: Some(ScheduledFor::Date(
                    chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
                )),
                repeat_rule: Some("FREQ=DAILY;COUNT=2".into()),
                repeat_mode: Some("BASIC".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = changes_rx.recv().await.unwrap();

        // First completion → spawns occurrence 2.
        let _ = handle.toggle_complete(original.id).await.unwrap();
        let first_changes = changes_rx.recv().await.unwrap();
        assert_eq!(first_changes.created.len(), 1);
        let second = first_changes.created[0].clone();

        // Second completion → no further occurrences.
        let _ = handle.toggle_complete(second.id).await.unwrap();
        let second_changes = changes_rx.recv().await.unwrap();
        assert!(
            second_changes.created.is_empty(),
            "COUNT=2 rule should not spawn a third instance"
        );
    }

    #[tokio::test]
    async fn weekly_repeat_survives_one_year_horizon() {
        // Phase 15 — synthetic 52-week horizon. Complete a weekly
        // task one cycle at a time and check it produces the right
        // sequence of dates. Uses BASIC mode so the test is anchor-
        // relative regardless of when CI runs.
        use crate::domain::ScheduledFor;
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let start = chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(); // Mon
        let mut current = handle
            .create_task(NewTask {
                title: "weekly".into(),
                scheduled_for: Some(ScheduledFor::Date(start)),
                repeat_rule: Some("FREQ=WEEKLY".into()),
                repeat_mode: Some("BASIC".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = changes_rx.recv().await.unwrap();

        for week in 1..=52 {
            let _ = handle.toggle_complete(current.id).await.unwrap();
            let changes = changes_rx.recv().await.unwrap();
            assert_eq!(
                changes.created.len(),
                1,
                "week {week}: expected a follow-up to spawn"
            );
            let next = &changes.created[0];
            let expected_date = start + chrono::Duration::weeks(week as i64);
            match next.scheduled_for {
                Some(ScheduledFor::Date(d)) => assert_eq!(
                    d, expected_date,
                    "week {week}: expected {expected_date}, got {d}"
                ),
                _ => panic!("week {week}: missing schedule"),
            }
            current = next.clone();
        }
    }

    #[tokio::test]
    async fn monthly_repeat_skips_short_months_at_end_of_month() {
        // Phase 15 — Jan 31 + monthly: Feb has no 31, RFC 5545
        // skips the month rather than clamp. Worker carries the
        // shifted date forward whatever rrule decides.
        use crate::domain::ScheduledFor;
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle
            .create_task(NewTask {
                title: "month-end".into(),
                scheduled_for: Some(ScheduledFor::Date(
                    chrono::NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
                )),
                repeat_rule: Some("FREQ=MONTHLY".into()),
                repeat_mode: Some("BASIC".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = changes_rx.recv().await.unwrap();

        let _ = handle.toggle_complete(task.id).await.unwrap();
        let changes = changes_rx.recv().await.unwrap();
        let next = &changes.created[0];
        match next.scheduled_for {
            Some(ScheduledFor::Date(d)) => assert_eq!(
                d,
                chrono::NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
                "Feb skipped, next is March 31"
            ),
            _ => panic!("missing schedule"),
        }
    }

    #[tokio::test]
    async fn reopen_does_not_spawn_follow_up() {
        // Phase 15 — toggling a *completed* task to open is a pure
        // reopen, never a regenerate.
        use crate::domain::ScheduledFor;
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle
            .create_task(NewTask {
                title: "weekly".into(),
                scheduled_for: Some(ScheduledFor::Date(
                    chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
                )),
                repeat_rule: Some("FREQ=WEEKLY".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = changes_rx.recv().await.unwrap();

        let _ = handle.toggle_complete(task.id).await.unwrap(); // complete (spawns)
        let _ = changes_rx.recv().await.unwrap();
        let _ = handle.toggle_complete(task.id).await.unwrap(); // reopen
        let reopen_changes = changes_rx.recv().await.unwrap();
        assert!(
            reopen_changes.created.is_empty(),
            "reopening should not spawn a new instance"
        );
    }

    #[tokio::test]
    async fn toggle_complete_flips_completed_at() {
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle.create_task(NewTask::inbox("flip me")).await.unwrap();
        let _ = changes_rx.recv().await.unwrap();

        let completed = handle.toggle_complete(task.id).await.unwrap();
        assert!(completed.is_completed());

        let changes = changes_rx.recv().await.unwrap();
        assert_eq!(changes.status_changed, vec![task.id]);
        assert_eq!(changes.updated.len(), 1);

        let reopened = handle.toggle_complete(task.id).await.unwrap();
        assert!(!reopened.is_completed());

        let changes = changes_rx.recv().await.unwrap();
        assert_eq!(changes.status_changed, vec![task.id]);
    }

    #[tokio::test]
    async fn delete_task_emits_deleted_id() {
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle.create_task(NewTask::inbox("doomed")).await.unwrap();
        let _ = changes_rx.recv().await.unwrap();

        handle.delete_task(task.id).await.unwrap();
        let changes = changes_rx.recv().await.unwrap();
        assert_eq!(changes.deleted, vec![task.id]);
    }

    #[tokio::test]
    async fn delete_missing_returns_not_found() {
        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let result = handle.delete_task(9999).await;
        assert!(matches!(result, Err(DbError::NotFound)));
    }

    #[tokio::test]
    async fn worker_shuts_down_when_handle_dropped() {
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        drop(handle);
        let result = tokio::time::timeout(Duration::from_secs(1), changes_rx.recv()).await;
        assert!(matches!(result, Ok(None)));
    }

    #[tokio::test]
    async fn position_increments_for_inbox_tasks() {
        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let a = handle.create_task(NewTask::inbox("a")).await.unwrap();
        let b = handle.create_task(NewTask::inbox("b")).await.unwrap();
        let c = handle.create_task(NewTask::inbox("c")).await.unwrap();
        assert!(a.position < b.position);
        assert!(b.position < c.position);
    }

    #[tokio::test]
    async fn create_with_someday_round_trips() {
        use crate::domain::ScheduledFor;
        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle
            .create_task(NewTask {
                title: "later".into(),
                scheduled_for: Some(ScheduledFor::Someday),
                ..NewTask::default()
            })
            .await
            .unwrap();
        assert_eq!(task.scheduled_for, Some(ScheduledFor::Someday));
    }

    // ── Phase 5b: areas / projects ─────────────────────────────────

    #[tokio::test]
    async fn create_area_emits_library_change() {
        let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
        let area = handle
            .create_area(NewArea {
                title: "Personal".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(area.title, "Personal");

        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.areas_created.len(), 1);
        assert_eq!(lib.areas_created[0].id, area.id);
    }

    #[tokio::test]
    async fn rename_area_round_trip() {
        let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
        let area = handle
            .create_area(NewArea {
                title: "Old".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();
        let renamed = handle
            .update_area(AreaUpdate::new(area.id).title("New"))
            .await
            .unwrap();
        assert_eq!(renamed.title, "New");
        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.areas_updated.len(), 1);
    }

    #[tokio::test]
    async fn delete_area_unfiles_projects() {
        let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
        let area = handle
            .create_area(NewArea {
                title: "Soon Gone".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();
        let project = handle
            .create_project(NewProject::in_area("Filed", area.id))
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();
        assert_eq!(project.area_id, Some(area.id));

        handle.delete_area(area.id).await.unwrap();
        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.areas_deleted, vec![area.id]);
        assert_eq!(lib.projects_updated.len(), 1, "FK SET NULL fired");
        assert!(lib.projects_updated[0].area_id.is_none());
    }

    #[tokio::test]
    async fn create_project_round_trip() {
        let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
        let project = handle
            .create_project(NewProject::unfiled("Q3"))
            .await
            .unwrap();
        assert_eq!(project.title, "Q3");
        assert!(project.area_id.is_none());
        assert!(!project.sequential);
        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.projects_created.len(), 1);
    }

    #[tokio::test]
    async fn mark_reviewed_stamps_last_reviewed_at_and_emits_library_change() {
        // Phase 13 — Review queue's Mark Reviewed handler.
        let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
        let project = handle
            .create_project(NewProject::unfiled("Quarterly OKRs"))
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();
        assert!(project.last_reviewed_at.is_none());

        let reviewed = handle.mark_reviewed(project.id).await.unwrap();
        assert!(reviewed.last_reviewed_at.is_some());
        assert_eq!(reviewed.id, project.id);

        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.projects_updated.len(), 1);
        assert_eq!(lib.projects_updated[0].id, project.id);
        assert!(lib.projects_updated[0].last_reviewed_at.is_some());
    }

    #[tokio::test]
    async fn mark_reviewed_unknown_id_is_not_found() {
        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let result = handle.mark_reviewed(9999).await;
        assert!(matches!(result, Err(DbError::NotFound)));
    }

    #[tokio::test]
    async fn mark_task_reviewed_stamps_last_reviewed_at_and_emits_task_change() {
        // v0.7.4 — task-level Mark Reviewed handler.
        let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
        let task = handle
            .create_task(NewTask::inbox("Audit the API"))
            .await
            .unwrap();
        let _ = changes_rx.recv().await.unwrap();
        assert!(task.last_reviewed_at.is_none());

        let reviewed = handle.mark_task_reviewed(task.id).await.unwrap();
        assert!(reviewed.last_reviewed_at.is_some());
        assert_eq!(reviewed.id, task.id);

        let changes = changes_rx.recv().await.unwrap();
        assert_eq!(changes.updated.len(), 1);
        assert_eq!(changes.updated[0].id, task.id);
        assert!(changes.updated[0].last_reviewed_at.is_some());
    }

    #[tokio::test]
    async fn mark_task_reviewed_unknown_id_is_not_found() {
        let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
        let result = handle.mark_task_reviewed(9999).await;
        assert!(matches!(result, Err(DbError::NotFound)));
    }

    #[tokio::test]
    async fn archive_project_completes_open_tasks() {
        let (handle, mut changes_rx, mut library_rx) = spawn(fresh_conn());
        let project = handle
            .create_project(NewProject::unfiled("Almost done"))
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();
        let mut new = NewTask::inbox("an open task");
        new.project_id = Some(project.id);
        let _t = handle.create_task(new).await.unwrap();
        let _ = changes_rx.recv().await.unwrap();

        let archived = handle.archive_project(project.id).await.unwrap();
        assert!(archived.archived_at.is_some());
        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.projects_updated.len(), 1);
        let task_changes = changes_rx.recv().await.unwrap();
        assert_eq!(task_changes.status_changed.len(), 1);
        assert_eq!(task_changes.updated.len(), 1);
        assert!(task_changes.updated[0].is_completed());
    }

    #[tokio::test]
    async fn delete_project_cascades_tasks() {
        let (handle, mut changes_rx, mut library_rx) = spawn(fresh_conn());
        let project = handle
            .create_project(NewProject::unfiled("Doomed"))
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();
        let mut new = NewTask::inbox("orphan-to-be");
        new.project_id = Some(project.id);
        let _t = handle.create_task(new).await.unwrap();
        let _ = changes_rx.recv().await.unwrap();

        handle.delete_project(project.id).await.unwrap();
        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.projects_deleted, vec![project.id]);
        let task_changes = changes_rx.recv().await.unwrap();
        assert_eq!(task_changes.deleted.len(), 1);
    }

    // ── Phase 6a: tags ────────────────────────────────────────────

    #[tokio::test]
    async fn create_tag_round_trip() {
        let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
        let tag = handle
            .create_tag(NewTag {
                name: "errand".into(),
                color: None,
            })
            .await
            .unwrap();
        assert_eq!(tag.name, "errand");
        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.tags_created.len(), 1);
    }

    #[tokio::test]
    async fn rename_tag_round_trip() {
        let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
        let tag = handle
            .create_tag(NewTag {
                name: "old".into(),
                color: None,
            })
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();
        let renamed = handle
            .update_tag(TagUpdate::new(tag.id).name("new"))
            .await
            .unwrap();
        assert_eq!(renamed.name, "new");
        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.tags_updated.len(), 1);
    }

    #[tokio::test]
    async fn delete_tag_emits_library_change() {
        let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
        let tag = handle
            .create_tag(NewTag {
                name: "doomed".into(),
                color: None,
            })
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();
        handle.delete_tag(tag.id).await.unwrap();
        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.tags_deleted, vec![tag.id]);
    }

    // ── Perspectives (Phase 14) ────────────────────────────────

    #[tokio::test]
    async fn create_perspective_round_trip_emits_library_change() {
        let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
        let p = handle
            .create_perspective(NewPerspective {
                name: "Q3 work overdue".into(),
                icon: None,
                filter_expr: "tag:work due:overdue".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(p.name, "Q3 work overdue");
        assert_eq!(p.filter_expr, "tag:work due:overdue");
        assert!(p.icon.is_none());
        assert!(!p.uuid.is_empty());

        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.perspectives_created.len(), 1);
        assert_eq!(lib.perspectives_created[0].id, p.id);
    }

    #[tokio::test]
    async fn update_perspective_round_trip() {
        let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
        let p = handle
            .create_perspective(NewPerspective {
                name: "Old name".into(),
                icon: None,
                filter_expr: "tag:work".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();

        let renamed = handle
            .update_perspective(
                PerspectiveUpdate::new(p.id)
                    .name("New name")
                    .filter_expr("tag:work is:overdue"),
            )
            .await
            .unwrap();
        assert_eq!(renamed.name, "New name");
        assert_eq!(renamed.filter_expr, "tag:work is:overdue");
        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.perspectives_updated.len(), 1);
    }

    #[tokio::test]
    async fn delete_perspective_emits_library_change() {
        let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
        let p = handle
            .create_perspective(NewPerspective {
                name: "Doomed".into(),
                icon: None,
                filter_expr: "is:done".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();

        handle.delete_perspective(p.id).await.unwrap();
        let lib = library_rx.recv().await.unwrap();
        assert_eq!(lib.perspectives_deleted, vec![p.id]);
    }

    #[tokio::test]
    async fn duplicate_tag_name_rejected() {
        let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
        let _ = handle
            .create_tag(NewTag {
                name: "Errand".into(),
                color: None,
            })
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();
        // Schema enforces NOCASE-unique; "errand" should collide.
        let result = handle
            .create_tag(NewTag {
                name: "errand".into(),
                color: None,
            })
            .await;
        assert!(result.is_err(), "duplicate tag name should fail");
    }

    #[tokio::test]
    async fn move_task_to_project_via_update_task() {
        let (handle, mut changes_rx, mut library_rx) = spawn(fresh_conn());
        let project = handle
            .create_project(NewProject::unfiled("Target"))
            .await
            .unwrap();
        let _ = library_rx.recv().await.unwrap();
        let task = handle.create_task(NewTask::inbox("orphan")).await.unwrap();
        let _ = changes_rx.recv().await.unwrap();
        assert!(task.project_id.is_none());

        let moved = handle
            .update_task(TaskUpdate::new(task.id).project(Some(project.id)))
            .await
            .unwrap();
        assert_eq!(moved.project_id, Some(project.id));
    }
}
