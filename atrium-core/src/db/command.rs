// SPDX-License-Identifier: MIT
//! `Command` — write operations sent to the single-writer worker.
//!
//! Each variant carries its own `oneshot::Sender` for the per-call
//! result. The worker matches on the variant, runs the SQL, sends the
//! result back, and emits a `TaskChanges` (task mutations) or
//! `LibraryChanges` (area / project mutations) delivery on the
//! corresponding channel. Reads do **not** flow through this enum —
//! they go directly through the read-only connection pool.

use tokio::sync::oneshot;

use crate::domain::{
    AreaUpdate, NewArea, NewPerspective, NewProject, NewTag, NewTask, Perspective,
    PerspectiveUpdate, Project, ProjectUpdate, Tag, TagUpdate, Task, TaskUpdate,
};
use crate::error::DbError;

/// Library-side commands grow alongside Simple Mode CRUD: tasks land
/// in Phase 2; areas / projects land in Phase 5b. Heading commands
/// follow in Phase 5.5 with the Inspector pane.
pub enum Command {
    // ── Tasks (Phase 2) ─────────────────────────────────────────
    CreateTask {
        task: NewTask,
        responder: oneshot::Sender<Result<Task, DbError>>,
    },
    UpdateTask {
        update: TaskUpdate,
        responder: oneshot::Sender<Result<Task, DbError>>,
    },
    ToggleComplete {
        id: i64,
        responder: oneshot::Sender<Result<Task, DbError>>,
    },
    DeleteTask {
        id: i64,
        responder: oneshot::Sender<Result<(), DbError>>,
    },

    // ── Areas (Phase 5b) ────────────────────────────────────────
    CreateArea {
        area: NewArea,
        responder: oneshot::Sender<Result<crate::domain::Area, DbError>>,
    },
    UpdateArea {
        update: AreaUpdate,
        responder: oneshot::Sender<Result<crate::domain::Area, DbError>>,
    },
    DeleteArea {
        id: i64,
        responder: oneshot::Sender<Result<(), DbError>>,
    },

    // ── Projects (Phase 5b) ─────────────────────────────────────
    CreateProject {
        project: NewProject,
        responder: oneshot::Sender<Result<Project, DbError>>,
    },
    UpdateProject {
        update: ProjectUpdate,
        responder: oneshot::Sender<Result<Project, DbError>>,
    },
    /// Sets `archived_at = now()`. Open tasks are completed too — see
    /// the `archive_project` handler in the worker.
    ArchiveProject {
        id: i64,
        responder: oneshot::Sender<Result<Project, DbError>>,
    },
    /// Phase 13 — sets `last_reviewed_at = now()`. Used by the
    /// Review queue's *Mark Reviewed* button to acknowledge a
    /// project review and advance it past its interval.
    MarkReviewed {
        id: i64,
        responder: oneshot::Sender<Result<Project, DbError>>,
    },
    DeleteProject {
        id: i64,
        responder: oneshot::Sender<Result<(), DbError>>,
    },

    // ── Tags (Phase 6a) ─────────────────────────────────────────
    CreateTag {
        tag: NewTag,
        responder: oneshot::Sender<Result<Tag, DbError>>,
    },
    UpdateTag {
        update: TagUpdate,
        responder: oneshot::Sender<Result<Tag, DbError>>,
    },
    DeleteTag {
        id: i64,
        responder: oneshot::Sender<Result<(), DbError>>,
    },
    /// Replace the entire tag set on a task with the given tag ids.
    /// Used by the inline `#tag` parser and (Phase 6b) the per-row
    /// pill editor. New tag names that don't exist yet must be
    /// created by the caller via `CreateTag` first — this command
    /// only operates on existing tag ids.
    SetTaskTags {
        task_id: i64,
        tag_ids: Vec<i64>,
        responder: oneshot::Sender<Result<Task, DbError>>,
    },
    /// Idempotent "create tag if absent" — returns the existing tag
    /// when the name (case-insensitive) already maps to one,
    /// otherwise creates it. Phase 6b's inline parser uses this to
    /// avoid spurious duplicate-name errors from `CreateTag`.
    EnsureTag {
        name: String,
        responder: oneshot::Sender<Result<Tag, DbError>>,
    },

    // ── Perspectives (Phase 14) ────────────────────────────────
    CreatePerspective {
        perspective: NewPerspective,
        responder: oneshot::Sender<Result<Perspective, DbError>>,
    },
    UpdatePerspective {
        update: PerspectiveUpdate,
        responder: oneshot::Sender<Result<Perspective, DbError>>,
    },
    DeletePerspective {
        id: i64,
        responder: oneshot::Sender<Result<(), DbError>>,
    },
}

impl Command {
    /// Static name for tracing spans / debug dumps.
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::CreateTask { .. } => "CreateTask",
            Self::UpdateTask { .. } => "UpdateTask",
            Self::ToggleComplete { .. } => "ToggleComplete",
            Self::DeleteTask { .. } => "DeleteTask",
            Self::CreateArea { .. } => "CreateArea",
            Self::UpdateArea { .. } => "UpdateArea",
            Self::DeleteArea { .. } => "DeleteArea",
            Self::CreateProject { .. } => "CreateProject",
            Self::UpdateProject { .. } => "UpdateProject",
            Self::ArchiveProject { .. } => "ArchiveProject",
            Self::MarkReviewed { .. } => "MarkReviewed",
            Self::DeleteProject { .. } => "DeleteProject",
            Self::CreateTag { .. } => "CreateTag",
            Self::UpdateTag { .. } => "UpdateTag",
            Self::DeleteTag { .. } => "DeleteTag",
            Self::SetTaskTags { .. } => "SetTaskTags",
            Self::EnsureTag { .. } => "EnsureTag",
            Self::CreatePerspective { .. } => "CreatePerspective",
            Self::UpdatePerspective { .. } => "UpdatePerspective",
            Self::DeletePerspective { .. } => "DeletePerspective",
        }
    }
}
