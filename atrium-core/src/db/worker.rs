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

use rusqlite::{Connection, OptionalExtension, params};
use tokio::sync::{mpsc, oneshot};
use tracing::{Instrument, info, info_span, trace};
use uuid::Uuid;

use std::sync::Arc;

use crate::db::changes::{LibraryChanges, TaskChanges};
use crate::db::command::Command;
use crate::db::read;
use crate::db::vault_hook::{VaultConfig, VaultDirtyNotifier};
use crate::domain::{
    Area, AreaUpdate, Heading, NewArea, NewClockEntry, NewHeading, NewPerspective, NewProject,
    NewQuickEntryTemplate, NewTag, NewTask, Perspective, PerspectiveUpdate, Project, ProjectUpdate,
    QuickEntryTemplate, QuickEntryTemplateUpdate, Tag, TagUpdate, Task, TaskClockEntry, TaskUpdate,
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

    // ── Task dependencies (v0.29.0) ─────────────────────────────

    /// Record that `task_id` is blocked by `blocked_by_id` (the latter
    /// is a prerequisite of the former). Rejects self-dependencies and
    /// cycles; a duplicate edge is a silent no-op.
    pub async fn add_dependency(&self, task_id: i64, blocked_by_id: i64) -> Result<(), DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::AddDependency {
                task_id,
                blocked_by_id,
                responder,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    /// Drop the dependency edge "`task_id` blocked by `blocked_by_id`".
    /// A no-op when the edge doesn't exist.
    pub async fn remove_dependency(&self, task_id: i64, blocked_by_id: i64) -> Result<(), DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::RemoveDependency {
                task_id,
                blocked_by_id,
                responder,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    // ── Task templates (v0.33.0) ────────────────────────────────

    /// Create a reusable project template plus its items in one call.
    pub async fn create_task_template(
        &self,
        template: crate::domain::NewTaskTemplate,
    ) -> Result<crate::domain::TaskTemplate, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::CreateTaskTemplate {
                template,
                responder,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn delete_task_template(&self, id: i64) -> Result<(), DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::DeleteTaskTemplate { id, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    /// Stamp out a fresh project from a template, returning the new
    /// project. The created tasks ride out on a `TaskChanges` delta.
    pub async fn instantiate_template(&self, id: i64) -> Result<crate::domain::Project, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::InstantiateTemplate { id, responder })
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

    /// Task-level analogue of `mark_reviewed`. Stamps
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

    /// v0.41.0 — record that a reminder fired for `(task_id,
    /// reminder_at)`. The reminder service awaits this after firing a
    /// notification, before re-querying, so the same reminder isn't
    /// returned (and re-fired) on the next poll.
    pub async fn mark_reminder_fired(
        &self,
        task_id: i64,
        reminder_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::MarkReminderFired {
                task_id,
                reminder_at,
                responder,
            })
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

    /// idempotent area-create-if-absent. Mirror of
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

    /// Idempotent heading-create-if-absent. Looks up by
    /// `(project_id, LOWER(title))`; returns the existing row or
    /// creates a fresh one at end-of-project-position. Used by
    /// the Phase 18 Todoist importer to map sections onto
    /// headings; safe to call repeatedly.
    pub async fn ensure_heading(&self, project_id: i64, title: String) -> Result<Heading, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::EnsureHeading {
                project_id,
                title,
                responder,
            })
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

    // ── Clock entries (Phase 18.5 Tier-1, v0.17.0) ─────────────

    /// Open a fresh clock entry on `task_id` (with optional note).
    /// Single-active-clock — any other open entry across the
    /// table closes first.
    pub async fn clock_in(&self, task_id: i64, note: String) -> Result<TaskClockEntry, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::ClockIn {
                entry: NewClockEntry { task_id, note },
                responder,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    /// Close the running entry on `task_id`. `Ok(None)` when the
    /// task had no open clock (soft no-op — scripts don't need to
    /// check first).
    pub async fn clock_out(&self, task_id: i64) -> Result<Option<TaskClockEntry>, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::ClockOut { task_id, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    /// Delete a single clock entry by id.
    pub async fn delete_clock_entry(&self, id: i64) -> Result<(), DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::DeleteClockEntry { id, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    /// v0.17.0 — importer + watcher entry point. Inserts a clock
    /// entry with caller-provided timestamps. Used when ingesting
    /// existing CLOCK lines from a vault file.
    pub async fn import_clock_entry(
        &self,
        task_id: i64,
        started_at: chrono::DateTime<chrono::Utc>,
        ended_at: Option<chrono::DateTime<chrono::Utc>>,
        note: String,
    ) -> Result<TaskClockEntry, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::ImportClockEntry {
                task_id,
                started_at,
                ended_at,
                note,
                responder,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    // ── Quick Entry templates (Phase 18.5 Tier-1, v0.18.0) ────

    pub async fn create_quick_entry_template(
        &self,
        template: NewQuickEntryTemplate,
    ) -> Result<QuickEntryTemplate, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::CreateQuickEntryTemplate {
                template,
                responder,
            })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn update_quick_entry_template(
        &self,
        update: QuickEntryTemplateUpdate,
    ) -> Result<QuickEntryTemplate, DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::UpdateQuickEntryTemplate { update, responder })
            .await
            .map_err(|_| DbError::WorkerClosed)?;
        rx.await.map_err(|_| DbError::WorkerClosed)?
    }

    pub async fn delete_quick_entry_template(&self, id: i64) -> Result<(), DbError> {
        let (responder, rx) = oneshot::channel();
        self.cmd_tx
            .send(Command::DeleteQuickEntryTemplate { id, responder })
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

/// Phase 16 entry point that wires a downstream vault projection
/// alongside the main worker. Pass `Some(VaultConfig { notifier })`
/// to enable per-mutation `ProjectDirty(project_id)` notifications
/// to the projection (atrium-org's `VaultWriter` is the only impl
/// today); `None` is equivalent to [`spawn`].
///
/// When configured, every successful Task / Project / Tag mutation
/// that touches a project calls `notifier.notify_project_dirty(pid)`.
/// The notifier is responsible for any debouncing or IO; the worker
/// fires synchronously and never blocks on it.
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

    let vault_notifier: Option<Arc<dyn VaultDirtyNotifier>> = vault.map(|cfg| cfg.notifier);

    let worker = Worker {
        conn,
        cmd_rx,
        changes_tx,
        library_tx,
        vault_notifier,
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
    /// Vault projection notifier. `None` when no vault is
    /// configured (atrium-cli, tests). `Some` when the GUI passes
    /// a `VaultConfig` through `spawn_with_vault` — the impl
    /// lives in atrium-org and turns the notification into a
    /// debounced `.org` file write.
    vault_notifier: Option<Arc<dyn VaultDirtyNotifier>>,
}

impl Worker {
    /// non-blocking notification that a project's
    /// vault file should be re-emitted. The notifier impl is
    /// responsible for any debouncing or IO; this fires inline
    /// after every commit and must never block.
    fn notify_project_dirty(&self, project_id: i64) {
        if let Some(n) = &self.vault_notifier {
            n.notify_project_dirty(project_id);
        }
    }

    /// v0.17.0 — clock changes happen against a `task_id` but
    /// the vault projection is per-project, so resolve the
    /// task's project before notifying. Tasks without a project
    /// (Inbox) have nothing to flush; the notification is a
    /// no-op for them. Skips the notify entirely when the task
    /// lookup fails (the task was deleted in a race) — the
    /// vault is already in the right state by definition.
    fn notify_task_dirty(&self, task_id: i64) {
        if let Ok(Some(task)) = read::task_by_id(&self.conn, task_id)
            && let Some(project_id) = task.project_id
        {
            self.notify_project_dirty(project_id);
        }
    }

    /// v0.17.0 — emit a TaskChanges with the affected task in
    /// the `updated` vec. Used by clock_in / clock_out / delete
    /// clock entry — the task itself didn't change columns, but
    /// the inspector pane re-binds on TaskChanges so this is
    /// the right surface for "the task's display shape moved."
    fn emit_task_refresh(&self, task_id: i64) {
        if let Ok(Some(task)) = read::task_by_id(&self.conn, task_id) {
            let _ = self.changes_tx.send(TaskChanges {
                updated: vec![task],
                ..Default::default()
            });
        }
    }

    // ── Dispatch-side delta helpers (v0.21.0 maintenance) ────────
    //
    // The `handle()` dispatch loop used to inline a 5-7 line
    // "send delta + maybe notify dirty" body per Command arm,
    // repeated for every Create / Update / Delete across Task,
    // Area, Project, Tag, Perspective. The repetition was
    // mechanical — same shape, different field name. These
    // helpers factor out the per-kind work so each dispatch arm
    // is one logical line.

    /// Send a `TaskChanges{created}` delta and notify the task's
    /// project dirty (if any). The standard "created a new task"
    /// post-write step.
    fn emit_task_created(&self, task: &Task) {
        let _ = self.changes_tx.send(TaskChanges {
            created: vec![task.clone()],
            ..Default::default()
        });
        if let Some(pid) = task.project_id {
            self.notify_project_dirty(pid);
        }
    }

    /// Send a `TaskChanges{updated}` delta and notify the task's
    /// project dirty (if any). The standard "edited a task"
    /// post-write step.
    fn emit_task_updated(&self, task: &Task) {
        let _ = self.changes_tx.send(TaskChanges {
            updated: vec![task.clone()],
            ..Default::default()
        });
        if let Some(pid) = task.project_id {
            self.notify_project_dirty(pid);
        }
    }

    fn emit_area_created(&self, area: &Area) {
        let _ = self.library_tx.send(LibraryChanges {
            areas_created: vec![area.clone()],
            ..Default::default()
        });
    }

    fn emit_area_updated(&self, area: &Area) {
        let _ = self.library_tx.send(LibraryChanges {
            areas_updated: vec![area.clone()],
            ..Default::default()
        });
    }

    fn emit_project_created(&self, project: &Project) {
        let _ = self.library_tx.send(LibraryChanges {
            projects_created: vec![project.clone()],
            ..Default::default()
        });
        self.notify_project_dirty(project.id);
    }

    fn emit_project_updated(&self, project: &Project) {
        let _ = self.library_tx.send(LibraryChanges {
            projects_updated: vec![project.clone()],
            ..Default::default()
        });
        self.notify_project_dirty(project.id);
    }

    fn emit_tag_created(&self, tag: &Tag) {
        let _ = self.library_tx.send(LibraryChanges {
            tags_created: vec![tag.clone()],
            ..Default::default()
        });
    }

    fn emit_tag_updated(&self, tag: &Tag) {
        let _ = self.library_tx.send(LibraryChanges {
            tags_updated: vec![tag.clone()],
            ..Default::default()
        });
    }

    fn emit_tag_deleted(&self, id: i64) {
        let _ = self.library_tx.send(LibraryChanges {
            tags_deleted: vec![id],
            ..Default::default()
        });
    }

    fn emit_perspective_created(&self, p: &Perspective) {
        let _ = self.library_tx.send(LibraryChanges {
            perspectives_created: vec![p.clone()],
            ..Default::default()
        });
    }

    fn emit_perspective_updated(&self, p: &Perspective) {
        let _ = self.library_tx.send(LibraryChanges {
            perspectives_updated: vec![p.clone()],
            ..Default::default()
        });
    }

    fn emit_perspective_deleted(&self, id: i64) {
        let _ = self.library_tx.send(LibraryChanges {
            perspectives_deleted: vec![id],
            ..Default::default()
        });
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
                if let Ok(ref t) = result {
                    self.emit_task_created(t);
                }
                let _ = responder.send(result);
            }
            Command::UpdateTask { update, responder } => {
                let result = self.update_task(update);
                if let Ok(ref t) = result {
                    self.emit_task_updated(t);
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
                // capture the project_id BEFORE we
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

            // ── Task dependencies (v0.29.0) ───────────────────────
            Command::AddDependency {
                task_id,
                blocked_by_id,
                responder,
            } => {
                let result = self.add_dependency(task_id, blocked_by_id);
                if result.is_ok() {
                    // The blocked task's availability changed; refresh
                    // its row so the GUI repaints the Blocked pill.
                    self.emit_task_refresh(task_id);
                }
                let _ = responder.send(result);
            }
            Command::RemoveDependency {
                task_id,
                blocked_by_id,
                responder,
            } => {
                let result = self.remove_dependency(task_id, blocked_by_id);
                if result.is_ok() {
                    self.emit_task_refresh(task_id);
                }
                let _ = responder.send(result);
            }

            // ── Task templates (v0.33.0) ──────────────────────────
            Command::CreateTaskTemplate {
                template,
                responder,
            } => {
                let _ = responder.send(self.create_task_template(template));
            }
            Command::DeleteTaskTemplate { id, responder } => {
                let _ = responder.send(self.delete_task_template(id));
            }
            Command::InstantiateTemplate { id, responder } => match self.instantiate_template(id) {
                Ok((project, tasks)) => {
                    let _ = self.library_tx.send(LibraryChanges {
                        projects_created: vec![project.clone()],
                        ..Default::default()
                    });
                    if !tasks.is_empty() {
                        let _ = self.changes_tx.send(TaskChanges {
                            created: tasks,
                            ..Default::default()
                        });
                    }
                    self.notify_project_dirty(project.id);
                    let _ = responder.send(Ok(project));
                }
                Err(e) => {
                    let _ = responder.send(Err(e));
                }
            },

            // ── Areas ─────────────────────────────────────────────
            Command::CreateArea { area, responder } => {
                let result = self.create_area(area);
                if let Ok(ref a) = result {
                    self.emit_area_created(a);
                }
                let _ = responder.send(result);
            }
            Command::UpdateArea { update, responder } => {
                let result = self.update_area(update);
                if let Ok(ref a) = result {
                    self.emit_area_updated(a);
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
                    self.emit_project_created(p);
                }
                let _ = responder.send(result);
            }
            Command::UpdateProject { update, responder } => {
                let result = self.update_project(update);
                if let Ok(ref p) = result {
                    self.emit_project_updated(p);
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
                    self.emit_project_updated(p);
                }
                let _ = responder.send(result);
            }
            Command::MarkTaskReviewed { id, responder } => {
                // emit a TaskChanges{updated} so the
                // canonical Review page rebuilds and the row drops
                // out of the weekly walk (the page filter excludes
                // tasks reviewed in the last 7 days).
                let result = self.mark_task_reviewed(id);
                if let Ok(t) = &result {
                    self.emit_task_updated(t);
                }
                let _ = responder.send(result);
            }
            Command::MarkReminderFired {
                task_id,
                reminder_at,
                responder,
            } => {
                // Side-table write only — no task row change, so no
                // TaskChanges (the UI doesn't track reminder-fired
                // state; the reminder service is the sole consumer).
                let result = self.mark_reminder_fired(task_id, reminder_at);
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
                    self.emit_tag_created(t);
                }
                let _ = responder.send(result);
            }
            Command::UpdateTag { update, responder } => {
                let result = self.update_tag(update);
                if let Ok(ref t) = result {
                    self.emit_tag_updated(t);
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
                    self.emit_tag_deleted(id);
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
                    self.emit_task_updated(task);
                }
                let _ = responder.send(result);
            }
            Command::EnsureArea { name, responder } => {
                let result = self.ensure_area(&name);
                if let Ok(ref a) = result
                    && a.created_at == a.modified_at
                {
                    self.emit_area_created(a);
                }
                let _ = responder.send(result);
            }
            Command::EnsureHeading {
                project_id,
                title,
                responder,
            } => {
                let result = self.ensure_heading(project_id, &title);
                // Headings don't currently have their own
                // LibraryChanges deltas (no GUI surface lists them
                // as a top-level concern). Notifying the project
                // dirty so the vault writer re-emits the file is
                // the right shape for now.
                if result.is_ok() {
                    self.notify_project_dirty(project_id);
                }
                let _ = responder.send(result);
            }
            Command::EnsureTag { name, responder } => {
                let result = self.ensure_tag(&name);
                if let Ok(ref t) = result
                    && t.created_at == t.modified_at
                {
                    // Only emit a creation delta if the tag was
                    // actually new — the helper differentiates and
                    // we mirror that here.
                    self.emit_tag_created(t);
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
                    self.emit_perspective_created(p);
                }
                let _ = responder.send(result);
            }
            Command::UpdatePerspective { update, responder } => {
                let result = self.update_perspective(update);
                if let Ok(p) = &result {
                    self.emit_perspective_updated(p);
                }
                let _ = responder.send(result);
            }
            Command::DeletePerspective { id, responder } => {
                let result = self.delete_perspective(id);
                if result.is_ok() {
                    self.emit_perspective_deleted(id);
                }
                let _ = responder.send(result);
            }

            // ── Clock entries (Phase 18.5 Tier-1, v0.17.0) ─────
            Command::ClockIn { entry, responder } => {
                let task_id = entry.task_id;
                let result = self.clock_in(entry);
                if let Ok(opened) = &result {
                    // The task's clock state changed but no column
                    // on `task` itself moved — surface a refresh
                    // by re-emitting the touched task (and any
                    // task auto-closed by the single-active-clock
                    // invariant) in TaskChanges. Inspector pane
                    // re-binds on this; the writer's notify-dirty
                    // pushes the LOGBOOK projection to the vault.
                    self.emit_task_refresh(task_id);
                    if let Some(closed_id) = opened.previously_closed_task_id
                        && closed_id != task_id
                    {
                        self.emit_task_refresh(closed_id);
                        self.notify_task_dirty(closed_id);
                    }
                    self.notify_task_dirty(task_id);
                }
                let _ = responder.send(result.map(|opened| opened.entry));
            }
            Command::ClockOut { task_id, responder } => {
                let result = self.clock_out(task_id);
                if let Ok(Some(_)) = &result {
                    self.emit_task_refresh(task_id);
                    self.notify_task_dirty(task_id);
                }
                let _ = responder.send(result);
            }
            Command::DeleteClockEntry { id, responder } => {
                // Look up the task before delete so we know which
                // task to refresh + re-emit. If the lookup itself
                // errors we still proceed with the delete; the
                // refresh/notify just gets skipped.
                let task_id = read::clock_entry_task_id(&self.conn, id).ok().flatten();
                let result = self.delete_clock_entry(id);
                if result.is_ok()
                    && let Some(tid) = task_id
                {
                    self.emit_task_refresh(tid);
                    self.notify_task_dirty(tid);
                }
                let _ = responder.send(result);
            }
            Command::ImportClockEntry {
                task_id,
                started_at,
                ended_at,
                note,
                responder,
            } => {
                let result = self.import_clock_entry(task_id, started_at, ended_at, note);
                if result.is_ok() {
                    self.emit_task_refresh(task_id);
                    // Don't notify_task_dirty — the import path
                    // is the watcher, which is reading from the
                    // file we'd notify about. Triggering a flush
                    // would cause the writer to overwrite the
                    // user's file with the freshly-ingested
                    // entries, which is fine in steady state but
                    // wasteful. The next non-import write naturally
                    // flushes.
                }
                let _ = responder.send(result);
            }

            // ── Quick Entry templates (Phase 18.5 Tier-1, v0.18.0)
            Command::CreateQuickEntryTemplate {
                template,
                responder,
            } => {
                let result = self.create_quick_entry_template(template);
                let _ = responder.send(result);
            }
            Command::UpdateQuickEntryTemplate { update, responder } => {
                let result = self.update_quick_entry_template(update);
                let _ = responder.send(result);
            }
            Command::DeleteQuickEntryTemplate { id, responder } => {
                let result = self.delete_quick_entry_template(id);
                let _ = responder.send(result);
            }
        }
    }

    fn create_task(&mut self, new: NewTask) -> Result<Task, DbError> {
        // Reject malformed RRULE up front so we don't store a string
        // that can't be iterated. Mode strings other than the three
        // known values fall back to default at read time, so they
        // don't need a hard reject; we only validate against the
        // known set when set explicitly.
        if let Some(rule) = new.repeat_rule.as_deref() {
            crate::repeat::RepeatRule::parse(rule, crate::repeat::RepeatMode::Cumulative)
                .map_err(|e| DbError::BadRepeatRule(e.to_string()))?;
        }

        // Domain rule: a subtask must live in the same project as its
        // parent. The schema's FK enforces "parent exists" but can't
        // express "parent is in the same project," so the worker
        // checks before insert.
        if let Some(parent_id) = new.parent_id
            && let Some(parent) = read::task_by_id(&self.conn, parent_id)?
            && parent.project_id != new.project_id
        {
            return Err(DbError::Domain(
                crate::error::DomainError::ParentProjectMismatch {
                    parent_task: parent_id,
                    parent_project: parent.project_id,
                    claimed_project: new.project_id,
                },
            ));
        }

        // honor a caller-provided UUID (the Org importer
        // uses this to preserve :ID: from the source vault).
        // `None` and `Some("")` both fall back to a fresh v4.
        let uuid = match new.uuid {
            Some(s) if !s.is_empty() => s,
            _ => Uuid::new_v4().to_string(),
        };
        let position = self.next_task_position(new.parent_id, new.project_id)?;

        // orig_keyword appended; existing call sites
        // pass `None` (Default::default()) so the value is NULL.
        // completed_at appended so the Org importer
        // can preserve the source CLOSED cookie.
        let scheduled_time_str = new.scheduled_time.map(|t| t.format("%H:%M").to_string());
        // v0.24.0 — empty extras map → NULL (cheaper than `{}`),
        // matching the `default_tags` JSON precedent. Encode
        // failures bubble out as a Sync error rather than
        // crashing — a malformed BTreeMap shouldn't be possible
        // at the Rust boundary, but we don't silently drop.
        let extra_properties_json = if new.extra_properties.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&new.extra_properties)
                    .map_err(|e| DbError::Sync(format!("extra_properties JSON encode: {e}")))?,
            )
        };
        self.conn.execute(
            "INSERT INTO task \
             (uuid, title, note, project_id, parent_id, scheduled_for, deadline, \
              defer_until, estimated_minutes, repeat_rule, repeat_mode, orig_keyword, \
              completed_at, deadline_warn_days, scheduled_time, reminder_at, \
              extra_properties, position) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
                new.completed_at,
                new.deadline_warn_days,
                scheduled_time_str,
                new.reminder_at,
                extra_properties_json,
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

    /// Subtasks (Phase 19.5) — true when reparenting `task_id` under
    /// `new_parent` would form a cycle. Walks the parent chain up from
    /// `new_parent`; if it reaches `task_id`, the move would make
    /// `task_id` its own ancestor. The degenerate self-parent case
    /// (`new_parent == task_id`) is caught on the first step. The
    /// depth guard defends against a pre-existing corrupt cycle in the
    /// stored data rather than looping forever.
    fn would_create_cycle(&self, task_id: i64, new_parent: i64) -> Result<bool, DbError> {
        let mut cursor = Some(new_parent);
        let mut depth = 0;
        while let Some(c) = cursor {
            if c == task_id {
                return Ok(true);
            }
            depth += 1;
            if depth > 10_000 {
                break;
            }
            cursor = read::task_by_id(&self.conn, c)?.and_then(|t| t.parent_id);
        }
        Ok(false)
    }

    /// Task dependencies (v0.29.0) — record that `task_id` is blocked
    /// by `blocked_by_id`. Rejects the self-edge and any edge that
    /// would close a cycle; a duplicate edge is absorbed by the UNIQUE
    /// constraint (`ON CONFLICT DO NOTHING`). Both tasks must exist.
    fn add_dependency(&mut self, task_id: i64, blocked_by_id: i64) -> Result<(), DbError> {
        if task_id == blocked_by_id {
            return Err(DbError::Domain(
                crate::error::DomainError::DependencyCycle {
                    task: task_id,
                    blocked_by: blocked_by_id,
                },
            ));
        }
        read::task_by_id(&self.conn, task_id)?.ok_or(DbError::NotFound)?;
        read::task_by_id(&self.conn, blocked_by_id)?.ok_or(DbError::NotFound)?;
        if self.would_create_dependency_cycle(task_id, blocked_by_id)? {
            return Err(DbError::Domain(
                crate::error::DomainError::DependencyCycle {
                    task: task_id,
                    blocked_by: blocked_by_id,
                },
            ));
        }
        self.conn.execute(
            "INSERT INTO task_dependency (task_id, blocked_by_id) VALUES (?1, ?2) \
             ON CONFLICT(task_id, blocked_by_id) DO NOTHING",
            params![task_id, blocked_by_id],
        )?;
        Ok(())
    }

    fn remove_dependency(&mut self, task_id: i64, blocked_by_id: i64) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM task_dependency WHERE task_id = ?1 AND blocked_by_id = ?2",
            params![task_id, blocked_by_id],
        )?;
        Ok(())
    }

    /// Task dependencies (v0.29.0) — true when adding the edge
    /// "`task_id` blocked by `blocked_by_id`" would form a cycle.
    /// Walks the prerequisite graph forward from `blocked_by_id`
    /// (following each node's own `blocked_by_id` edges); if it reaches
    /// `task_id`, then `blocked_by_id` already depends on `task_id` and
    /// the new edge would make them block each other. Mirrors
    /// `would_create_cycle` (subtasks); the `seen` set + depth guard
    /// defend against a pre-existing corrupt cycle rather than looping.
    fn would_create_dependency_cycle(
        &self,
        task_id: i64,
        blocked_by_id: i64,
    ) -> Result<bool, DbError> {
        let mut stack = vec![blocked_by_id];
        let mut seen = std::collections::HashSet::new();
        let mut depth = 0;
        while let Some(cur) = stack.pop() {
            if cur == task_id {
                return Ok(true);
            }
            if !seen.insert(cur) {
                continue;
            }
            depth += 1;
            if depth > 100_000 {
                break;
            }
            let mut stmt = self
                .conn
                .prepare_cached("SELECT blocked_by_id FROM task_dependency WHERE task_id = ?1")?;
            let rows = stmt.query_map(params![cur], |r| r.get::<_, i64>(0))?;
            for r in rows {
                stack.push(r?);
            }
        }
        Ok(false)
    }

    fn update_task(&mut self, update: TaskUpdate) -> Result<Task, DbError> {
        if update.is_noop() {
            return read::task_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound);
        }

        // Same validation as create_task: malformed RRULE strings
        // get a hard reject so they never land in the column.
        if let Some(Some(rule)) = update.repeat_rule.as_ref() {
            crate::repeat::RepeatRule::parse(rule, crate::repeat::RepeatMode::Cumulative)
                .map_err(|e| DbError::BadRepeatRule(e.to_string()))?;
        }

        // Domain rule: if the caller is moving this task to a new
        // project AND the task has a parent, the parent must already
        // be in (or moving to) that same project. We don't auto-fix
        // — the GUI either moves the parent first or unfiles the
        // child.
        if let Some(claimed_project) = update.project_id
            && let Some(existing) = read::task_by_id(&self.conn, update.id)?
            && let Some(parent_id) = existing.parent_id
            && let Some(parent) = read::task_by_id(&self.conn, parent_id)?
            && parent.project_id != claimed_project
        {
            return Err(DbError::Domain(
                crate::error::DomainError::ParentProjectMismatch {
                    parent_task: parent_id,
                    parent_project: parent.project_id,
                    claimed_project,
                },
            ));
        }

        // Subtasks (Phase 19.5) — reparenting validation. When the
        // caller sets a concrete new parent (Some(Some(id))) we reject
        // cycles (the parent is the task itself or one of its
        // descendants) and enforce the same same-project rule
        // create_task applies. Promoting to top-level (Some(None))
        // needs no validation.
        if let Some(Some(new_parent)) = update.parent_id {
            if self.would_create_cycle(update.id, new_parent)? {
                return Err(DbError::Domain(crate::error::DomainError::ParentCycle {
                    task: update.id,
                    parent: new_parent,
                }));
            }
            let parent = read::task_by_id(&self.conn, new_parent)?.ok_or(DbError::NotFound)?;
            let existing = read::task_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound)?;
            let effective_project = match update.project_id {
                Some(p) => p,
                None => existing.project_id,
            };
            if parent.project_id != effective_project {
                return Err(DbError::Domain(
                    crate::error::DomainError::ParentProjectMismatch {
                        parent_task: new_parent,
                        parent_project: parent.project_id,
                        claimed_project: effective_project,
                    },
                ));
            }
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
        if let Some(parent_id) = update.parent_id {
            sets.push("parent_id = ?");
            bound.push(Box::new(parent_id));
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
        if let Some(completed) = update.completed_at {
            sets.push("completed_at = ?");
            bound.push(Box::new(completed));
        }
        if let Some(orig) = update.orig_keyword {
            sets.push("orig_keyword = ?");
            bound.push(Box::new(orig));
        }
        if let Some(warn) = update.deadline_warn_days {
            sets.push("deadline_warn_days = ?");
            bound.push(Box::new(warn));
        }
        if let Some(time) = update.scheduled_time {
            sets.push("scheduled_time = ?");
            bound.push(Box::new(time.map(|t| t.format("%H:%M").to_string())));
        }
        if let Some(reminder) = update.reminder_at {
            sets.push("reminder_at = ?");
            bound.push(Box::new(reminder));
        }
        if let Some(extras) = update.extra_properties {
            sets.push("extra_properties = ?");
            // Empty map normalises to NULL on the column for
            // parity with the create path; the read boundary
            // turns either back into an empty BTreeMap.
            let encoded: Option<String> = if extras.is_empty() {
                None
            } else {
                Some(
                    serde_json::to_string(&extras)
                        .map_err(|e| DbError::Sync(format!("extra_properties JSON encode: {e}")))?,
                )
            };
            bound.push(Box::new(encoded));
        }
        bound.push(Box::new(update.id));

        let sql = format!("UPDATE task SET {} WHERE id = ?", sets.join(", "));
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            bound.iter().map(std::convert::AsRef::as_ref).collect();
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

        let completed_on = completed.completed_at.map_or_else(
            || Local::now().date_naive(),
            |dt| dt.with_timezone(&Local).date_naive(),
        );

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
            // The respawn is a fresh open instance — no completion
            // timestamp until the user toggles it complete again.
            completed_at: None,
            // Per-task warning window carries forward — the
            // sensitivity of the deadline doesn't change just
            // because the previous instance closed.
            deadline_warn_days: completed.deadline_warn_days,
            // Time-of-day carries forward — a daily 9 AM
            // standup keeps the 9 AM on its respawn.
            scheduled_time: completed.scheduled_time,
            // Reminders are deliberately *not* carried forward —
            // a "remind me at 3 PM" reminder fired on the
            // previous instance; the respawn shouldn't re-fire
            // it. Users can re-set the reminder if they want
            // it to repeat alongside the task.
            reminder_at: None,
            // v0.24.0 — custom property-drawer extras carry
            // forward; the user put them on the task once
            // and expects them on every recurrence.
            extra_properties: completed.extra_properties.clone(),
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
            "INSERT INTO area (uuid, title, color, default_review_interval_days, position) \
             VALUES (?, ?, ?, ?, ?)",
            params![
                uuid,
                new.title,
                new.color,
                new.default_review_interval_days,
                position
            ],
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
        if let Some(interval) = update.default_review_interval_days {
            sets.push("default_review_interval_days = ?");
            bound.push(Box::new(interval));
        }
        bound.push(Box::new(update.id));
        let sql = format!("UPDATE area SET {} WHERE id = ?", sets.join(", "));
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            bound.iter().map(std::convert::AsRef::as_ref).collect();
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
        // honor a caller-provided UUID (Org importer
        // path). Empty / None fall back to a fresh v4.
        let uuid = match new.uuid {
            Some(s) if !s.is_empty() => s,
            _ => Uuid::new_v4().to_string(),
        };
        let position = self.next_project_position(new.area_id)?;
        // last_reviewed_at + archived_at honor caller-
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
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            bound.iter().map(std::convert::AsRef::as_ref).collect();
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

    /// Task-level analogue. Stamps `task.last_reviewed_at
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

    /// v0.41.0 — record that a reminder fired for `(task_id,
    /// reminder_at)` in the `task_reminder_fired` side table. One row
    /// per task (INSERT OR REPLACE on the `task_id` PK); storing the
    /// `reminder_at` that fired re-arms the reminder if the user later
    /// moves it to a new time (the read query joins on `reminder_at`).
    /// Side-table only, so `task.modified_at` is untouched.
    fn mark_reminder_fired(
        &mut self,
        task_id: i64,
        reminder_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO task_reminder_fired (task_id, reminder_at) \
             VALUES (?1, ?2)",
            params![task_id, reminder_at],
        )?;
        Ok(())
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
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            bound.iter().map(std::convert::AsRef::as_ref).collect();
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

    // ── Task templates (v0.33.0) ────────────────────────────────

    fn create_task_template(
        &mut self,
        new: crate::domain::NewTaskTemplate,
    ) -> Result<crate::domain::TaskTemplate, DbError> {
        let uuid = Uuid::new_v4().to_string();
        let tags_json = serde_json::to_string(&new.tags).unwrap_or_else(|_| "[]".to_string());
        self.conn.execute(
            "INSERT INTO task_template (uuid, name, project_title_seed, note, tags_json) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![uuid, new.name, new.project_title_seed, new.note, tags_json],
        )?;
        let template_id = self.conn.last_insert_rowid();
        for (i, item) in new.items.iter().enumerate() {
            let item_tags =
                serde_json::to_string(&item.default_tags).unwrap_or_else(|_| "[]".to_string());
            self.conn.execute(
                "INSERT INTO task_template_item \
                 (template_id, title, parent_index, position, estimated_minutes, default_tags_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    template_id,
                    item.title,
                    item.parent_index,
                    i as f64,
                    item.estimated_minutes,
                    item_tags
                ],
            )?;
        }
        read::task_template_by_id(&self.conn, template_id)?.ok_or(DbError::NotFound)
    }

    fn delete_task_template(&mut self, id: i64) -> Result<(), DbError> {
        let n = self
            .conn
            .execute("DELETE FROM task_template WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    /// Stamp out a fresh project from a template: create the project,
    /// then create each item as a task (resolving `parent_index` to the
    /// real `parent_id` of the already-created item at that index) and
    /// attach the template's + the item's tags. Returns the project and
    /// the created tasks so the dispatch arm can emit deltas.
    fn instantiate_template(&mut self, id: i64) -> Result<(Project, Vec<Task>), DbError> {
        let template = read::task_template_by_id(&self.conn, id)?.ok_or(DbError::NotFound)?;
        let items = read::task_template_items(&self.conn, id)?;

        let title = if template.project_title_seed.trim().is_empty() {
            template.name.clone()
        } else {
            template.project_title_seed.clone()
        };
        let mut new_project = NewProject::unfiled(title);
        new_project.note = template.note.clone();
        let project = self.create_project(new_project)?;

        // Ensure the template-level tags once; reused per item.
        let mut template_tag_ids = Vec::with_capacity(template.tags.len());
        for name in &template.tags {
            template_tag_ids.push(self.ensure_tag(name)?.id);
        }

        let mut created: Vec<Task> = Vec::with_capacity(items.len());
        let mut created_ids: Vec<i64> = Vec::with_capacity(items.len());
        for item in &items {
            // `parent_index` refers to an earlier item's slot; resolve
            // to its real id. A forward / out-of-range reference
            // degrades to a top-level task rather than failing.
            let parent_id = item
                .parent_index
                .and_then(|pi| usize::try_from(pi).ok())
                .and_then(|pi| created_ids.get(pi).copied());
            let task = self.create_task(NewTask {
                title: item.title.clone(),
                project_id: Some(project.id),
                parent_id,
                estimated_minutes: item.estimated_minutes,
                ..NewTask::default()
            })?;
            let mut tag_ids = template_tag_ids.clone();
            for name in &item.default_tags {
                let tid = self.ensure_tag(name)?.id;
                if !tag_ids.contains(&tid) {
                    tag_ids.push(tid);
                }
            }
            let task = if tag_ids.is_empty() {
                task
            } else {
                self.set_task_tags(task.id, tag_ids)?
            };
            created_ids.push(task.id);
            created.push(task);
        }
        Ok((project, created))
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

    /// Idempotent heading lookup-or-create scoped to a project.
    /// Same case-insensitive title match as `ensure_area`. New
    /// headings land at end-of-project position (max + 1.0).
    fn ensure_heading(&mut self, project_id: i64, title: &str) -> Result<Heading, DbError> {
        let existing: rusqlite::Result<i64> = self.conn.query_row(
            "SELECT id FROM heading \
             WHERE project_id = ?1 AND LOWER(title) = LOWER(?2) LIMIT 1",
            params![project_id, title],
            |r| r.get(0),
        );
        match existing {
            Ok(id) => read::heading_by_id(&self.conn, id)?.ok_or(DbError::NotFound),
            Err(rusqlite::Error::QueryReturnedNoRows) => self.create_heading(NewHeading {
                project_id,
                title: title.to_string(),
            }),
            Err(e) => Err(e.into()),
        }
    }

    fn create_heading(&mut self, new: NewHeading) -> Result<Heading, DbError> {
        let uuid = Uuid::new_v4().to_string();
        let position = self.next_heading_position(new.project_id)?;
        self.conn.execute(
            "INSERT INTO heading (uuid, project_id, title, position) \
             VALUES (?, ?, ?, ?)",
            params![uuid, new.project_id, new.title, position],
        )?;
        let id = self.conn.last_insert_rowid();
        read::heading_by_id(&self.conn, id)?.ok_or(DbError::NotFound)
    }

    fn next_heading_position(&self, project_id: i64) -> Result<f64, DbError> {
        let max: Option<f64> = self.conn.query_row(
            "SELECT MAX(position) FROM heading WHERE project_id = ?1",
            params![project_id],
            |r| r.get(0),
        )?;
        Ok(max.unwrap_or(0.0) + 1.0)
    }

    /// idempotent area-by-title lookup. Area's `title`
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
                default_review_interval_days: None,
            }),
            Err(e) => Err(e.into()),
        }
    }

    // ── Perspectives (Phase 14) ─────────────────────────────────

    fn create_perspective(&mut self, new: NewPerspective) -> Result<Perspective, DbError> {
        // Domain rule: filter expression must be non-empty. A blank
        // perspective has no rows; the GUI editor should surface the
        // rejection rather than silently produce a no-op sidebar
        // entry.
        if new.filter_expr.trim().is_empty() {
            return Err(DbError::Domain(crate::error::DomainError::EmptyFilterExpr));
        }
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
        // Same rule as create_perspective: if the caller is changing
        // the filter expression, it must be non-empty. Other update
        // shapes (rename, icon swap, renderer flip) leave the filter
        // alone and pass through.
        if let Some(expr) = update.filter_expr.as_deref()
            && expr.trim().is_empty()
        {
            return Err(DbError::Domain(crate::error::DomainError::EmptyFilterExpr));
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
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            bound.iter().map(std::convert::AsRef::as_ref).collect();
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

    // ── Clock entries (Phase 18.5 Tier-1, v0.17.0) ─────────────

    /// Open a fresh clock entry on `entry.task_id`. Single-active-
    /// clock invariant: any other open entry across the table
    /// gets closed first (its `ended_at` set to `now()`). Returns
    /// the freshly-inserted entry plus the id of any task that
    /// had its previous open clock auto-closed (so the dispatcher
    /// can refresh both inspector views and notify both vault
    /// projections).
    fn clock_in(&mut self, new: NewClockEntry) -> Result<ClockInResult, DbError> {
        // Validate the task exists. The FK constraint would catch
        // it on insert, but a clean DbError::NotFound up front
        // gives the CLI a less cryptic error message than a
        // FOREIGN KEY constraint failed.
        if read::task_by_id(&self.conn, new.task_id)?.is_none() {
            return Err(DbError::NotFound);
        }

        // Single-active-clock invariant: close any other open
        // entry first. The "any other" wording matters — clocking
        // in on the same task while it already has an open entry
        // is the user re-affirming the clock; we don't double-stamp,
        // we leave the existing open entry alone and reject the
        // new clock-in (returning the existing entry would surprise
        // the caller; returning an error is honest).
        let existing_open: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT id, task_id FROM task_clock_entry WHERE ended_at IS NULL LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;

        let mut previously_closed_task_id: Option<i64> = None;
        if let Some((existing_id, existing_task_id)) = existing_open {
            if existing_task_id == new.task_id {
                // Already clocked into this task — surface the
                // existing entry so the caller doesn't double-
                // stamp. Marked as not-newly-opened so the
                // dispatcher can decide whether to notify (it
                // doesn't need to; nothing changed).
                let entry =
                    read::clock_entry_by_id(&self.conn, existing_id)?.ok_or(DbError::NotFound)?;
                return Ok(ClockInResult {
                    entry,
                    previously_closed_task_id: None,
                });
            }
            // Auto-close the other task's clock first.
            self.conn.execute(
                "UPDATE task_clock_entry SET ended_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1",
                params![existing_id],
            )?;
            previously_closed_task_id = Some(existing_task_id);
        }

        // Insert the new open entry. started_at = now() via SQL
        // so the worker doesn't need a chrono call here.
        // created_at + modified_at stamped by the same now() value
        // (migration 0013 made these explicit; the trigger keeps
        // modified_at fresh on subsequent UPDATEs).
        self.conn.execute(
            "INSERT INTO task_clock_entry (task_id, started_at, ended_at, note, created_at, modified_at) \
             VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), NULL, ?2, \
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![new.task_id, new.note],
        )?;
        let id = self.conn.last_insert_rowid();
        let entry = read::clock_entry_by_id(&self.conn, id)?.ok_or(DbError::NotFound)?;
        Ok(ClockInResult {
            entry,
            previously_closed_task_id,
        })
    }

    /// Close the open clock entry on `task_id`. Returns the
    /// just-closed entry, or `Ok(None)` when the task had no
    /// running clock (soft no-op so scripts don't have to check).
    fn clock_out(&mut self, task_id: i64) -> Result<Option<TaskClockEntry>, DbError> {
        // Find the open entry.
        let open_id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM task_clock_entry \
                 WHERE task_id = ?1 AND ended_at IS NULL LIMIT 1",
                params![task_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(id) = open_id else {
            return Ok(None);
        };
        self.conn.execute(
            "UPDATE task_clock_entry SET ended_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1",
            params![id],
        )?;
        let entry = read::clock_entry_by_id(&self.conn, id)?.ok_or(DbError::NotFound)?;
        Ok(Some(entry))
    }

    /// Delete a single clock entry by id. NotFound when the row
    /// doesn't exist — caller usually treats that as a soft
    /// success since the user's intent was "make this entry go
    /// away."
    fn delete_clock_entry(&mut self, id: i64) -> Result<(), DbError> {
        let n = self
            .conn
            .execute("DELETE FROM task_clock_entry WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    /// Insert a clock entry with caller-provided timestamps.
    /// Importer + watcher entry point; doesn't enforce
    /// single-active-clock since the source file is trusted.
    fn import_clock_entry(
        &mut self,
        task_id: i64,
        started_at: chrono::DateTime<chrono::Utc>,
        ended_at: Option<chrono::DateTime<chrono::Utc>>,
        note: String,
    ) -> Result<TaskClockEntry, DbError> {
        if read::task_by_id(&self.conn, task_id)?.is_none() {
            return Err(DbError::NotFound);
        }
        // Stamp created_at to now() (the row is being inserted now,
        // regardless of when the source CLOCK line claims work began).
        // modified_at = COALESCE(ended_at, started_at) mirrors the
        // backfill in migration 0013 — for a closed entry the most
        // recent edit was the close; for an open entry it was the
        // open. Trigger keeps modified_at fresh on later UPDATEs.
        self.conn.execute(
            "INSERT INTO task_clock_entry (task_id, started_at, ended_at, note, created_at, modified_at) \
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), COALESCE(?3, ?2))",
            params![task_id, started_at, ended_at, note],
        )?;
        let id = self.conn.last_insert_rowid();
        read::clock_entry_by_id(&self.conn, id)?.ok_or(DbError::NotFound)
    }

    // ── Quick Entry templates (Phase 18.5 Tier-1, v0.18.0) ─────

    fn create_quick_entry_template(
        &mut self,
        new: NewQuickEntryTemplate,
    ) -> Result<QuickEntryTemplate, DbError> {
        validate_shortcut_key(new.shortcut_key.as_deref())?;
        let position = self.next_quick_entry_template_position()?;
        let tags_json = serde_json::to_string(&new.default_tags)
            .map_err(|e| DbError::Sync(format!("default_tags JSON encode: {e}")))?;
        self.conn.execute(
            "INSERT INTO quick_entry_template \
             (name, shortcut_key, target_project_id, prefix, default_tags, position) \
             VALUES (?, ?, ?, ?, ?, ?)",
            params![
                new.name,
                new.shortcut_key,
                new.target_project_id,
                new.prefix,
                tags_json,
                position,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        read::quick_entry_template_by_id(&self.conn, id)?.ok_or(DbError::NotFound)
    }

    fn update_quick_entry_template(
        &mut self,
        update: QuickEntryTemplateUpdate,
    ) -> Result<QuickEntryTemplate, DbError> {
        if update.is_noop() {
            return read::quick_entry_template_by_id(&self.conn, update.id)?
                .ok_or(DbError::NotFound);
        }
        if let Some(Some(key)) = update.shortcut_key.as_ref() {
            validate_shortcut_key(Some(key.as_str()))?;
        }
        let mut sets: Vec<&'static str> = Vec::new();
        let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(name) = update.name {
            sets.push("name = ?");
            bound.push(Box::new(name));
        }
        if let Some(shortcut) = update.shortcut_key {
            sets.push("shortcut_key = ?");
            bound.push(Box::new(shortcut));
        }
        if let Some(target) = update.target_project_id {
            sets.push("target_project_id = ?");
            bound.push(Box::new(target));
        }
        if let Some(prefix) = update.prefix {
            sets.push("prefix = ?");
            bound.push(Box::new(prefix));
        }
        if let Some(tags) = update.default_tags {
            let json = serde_json::to_string(&tags)
                .map_err(|e| DbError::Sync(format!("default_tags JSON encode: {e}")))?;
            sets.push("default_tags = ?");
            bound.push(Box::new(json));
        }
        if let Some(position) = update.position {
            sets.push("position = ?");
            bound.push(Box::new(position));
        }
        bound.push(Box::new(update.id));
        let sql = format!(
            "UPDATE quick_entry_template SET {} WHERE id = ?",
            sets.join(", ")
        );
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            bound.iter().map(std::convert::AsRef::as_ref).collect();
        let n = self.conn.execute(&sql, &params_refs[..])?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        read::quick_entry_template_by_id(&self.conn, update.id)?.ok_or(DbError::NotFound)
    }

    fn delete_quick_entry_template(&mut self, id: i64) -> Result<(), DbError> {
        let n = self.conn.execute(
            "DELETE FROM quick_entry_template WHERE id = ?1",
            params![id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    fn next_quick_entry_template_position(&self) -> Result<f64, DbError> {
        let max: Option<f64> =
            self.conn
                .query_row("SELECT MAX(position) FROM quick_entry_template", [], |r| {
                    r.get(0)
                })?;
        Ok(max.unwrap_or(0.0) + 1.0)
    }
}

/// v0.18.0 — Phase 18.5 Tier-1 shortcut-key validation. Quick
/// Entry templates accept at most a single ASCII alphanumeric
/// character (or NULL = no shortcut). The constraint can't be
/// expressed cleanly in SQL without a check trigger we'd rather
/// not maintain; the worker checks before insert / update.
fn validate_shortcut_key(value: Option<&str>) -> Result<(), DbError> {
    let Some(s) = value else { return Ok(()) };
    let chars: Vec<char> = s.chars().collect();
    if chars.len() != 1 || !chars[0].is_ascii_alphanumeric() {
        return Err(DbError::Domain(
            crate::error::DomainError::InvalidShortcutKey { got: s.to_string() },
        ));
    }
    Ok(())
}

/// v0.17.0 — return shape from `Worker::clock_in`. Carries the
/// freshly-opened entry plus, when the single-active-clock
/// invariant auto-closed a different task's clock, that task's
/// id (so the dispatcher can refresh both inspector views and
/// notify both vault projections).
struct ClockInResult {
    entry: TaskClockEntry,
    previously_closed_task_id: Option<i64>,
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
