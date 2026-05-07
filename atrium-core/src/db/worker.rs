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
    Area, AreaUpdate, NewArea, NewProject, NewTag, NewTask, Project, ProjectUpdate, Tag, TagUpdate,
    Task, TaskUpdate,
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
}

/// Spawn the worker on the current `tokio` runtime.
///
/// Returns the `WorkerHandle` (commands flow in), the `TaskChanges`
/// receiver (task-level deltas flow out), and the `LibraryChanges`
/// receiver (area/project deltas flow out — Phase 5b). The worker
/// exits when the last `WorkerHandle` is dropped.
pub fn spawn(
    mut conn: Connection,
) -> (
    WorkerHandle,
    mpsc::UnboundedReceiver<TaskChanges>,
    mpsc::UnboundedReceiver<LibraryChanges>,
) {
    install_profile_callback(&mut conn);

    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(COMMAND_CHANNEL_CAPACITY);
    let (changes_tx, changes_rx) = mpsc::unbounded_channel::<TaskChanges>();
    let (library_tx, library_rx) = mpsc::unbounded_channel::<LibraryChanges>();

    let worker = Worker {
        conn,
        cmd_rx,
        changes_tx,
        library_tx,
    };

    tokio::spawn(worker.run().instrument(info_span!("atrium_worker")));

    (WorkerHandle { cmd_tx }, changes_rx, library_rx)
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
                }
                let _ = responder.send(result);
            }
            Command::ToggleComplete { id, responder } => {
                let result = self.toggle_complete(id);
                if let Ok(ref task) = result {
                    let _ = self.changes_tx.send(TaskChanges {
                        updated: vec![task.clone()],
                        status_changed: vec![task.id],
                        ..Default::default()
                    });
                }
                let _ = responder.send(result);
            }
            Command::DeleteTask { id, responder } => {
                let result = self.delete_task(id);
                if result.is_ok() {
                    let _ = self.changes_tx.send(TaskChanges {
                        deleted: vec![id],
                        ..Default::default()
                    });
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
                }
                let _ = responder.send(result);
            }
            Command::UpdateProject { update, responder } => {
                let result = self.update_project(update);
                if let Ok(ref p) = result {
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
        }
    }

    fn create_task(&mut self, new: NewTask) -> Result<Task, DbError> {
        let uuid = Uuid::new_v4().to_string();
        let position = self.next_task_position(new.parent_id, new.project_id)?;

        self.conn.execute(
            "INSERT INTO task \
             (uuid, title, note, project_id, parent_id, scheduled_for, deadline, \
              defer_until, estimated_minutes, repeat_rule, position) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        bound.push(Box::new(update.id));

        let sql = format!("UPDATE task SET {} WHERE id = ?", sets.join(", "));
        let params_refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|b| b.as_ref()).collect();
        let n = self.conn.execute(&sql, &params_refs[..])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }

        read::task_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound)
    }

    fn toggle_complete(&mut self, id: i64) -> Result<Task, DbError> {
        let task = read::task_by_id(&self.conn, id)?.ok_or(DbError::NotFound)?;
        if task.is_completed() {
            self.conn.execute(
                "UPDATE task SET completed_at = NULL WHERE id = ?1",
                params![id],
            )?;
        } else {
            self.conn.execute(
                "UPDATE task SET completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                params![id],
            )?;
        }
        read::task_by_id(&self.conn, id)?.ok_or(DbError::NotFound)
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
            "INSERT INTO area (uuid, title, position) VALUES (?, ?, ?)",
            params![uuid, new.title, position],
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
        let uuid = Uuid::new_v4().to_string();
        let position = self.next_project_position(new.area_id)?;
        self.conn.execute(
            "INSERT INTO project \
             (uuid, title, note, area_id, sequential, review_interval_days, position) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                uuid,
                new.title,
                new.note,
                new.area_id,
                i32::from(new.sequential),
                new.review_interval_days,
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
