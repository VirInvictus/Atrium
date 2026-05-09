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
    Area, AreaUpdate, NewArea, NewPerspective, NewProject, NewTag, NewTask, Perspective,
    PerspectiveUpdate, Project, ProjectUpdate, Tag, TagUpdate, Task, TaskUpdate,
};
use crate::error::DbError;

/// Write commands the worker accepts. Reads bypass this enum
/// entirely — they go through the read-only connection pool and
/// the free functions in [`super::read`].
///
/// Headings (the project subdivision rows in `heading`) don't have
/// Command variants today: in Simple Mode the GUI renders them
/// inline as section breaks, and the Org importer / writer
/// round-trips them via the `heading` table directly through the
/// writable connection during a one-shot import. Wiring through
/// the worker would cost an mpsc round-trip per heading on import
/// for no behavioural difference.
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
    /// Task-level analogue of MarkReviewed. Stamps
    /// `task.last_reviewed_at = now()` so the canonical Review
    /// page's weekly walk hides the row for 7 days.
    MarkTaskReviewed {
        id: i64,
        responder: oneshot::Sender<Result<Task, DbError>>,
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
    /// idempotent area-create-by-name. Returns the
    /// existing Area when the title matches case-insensitively;
    /// creates a new one otherwise. Used by the multi-file Org
    /// importer to map vault subdirectories onto Atrium areas.
    EnsureArea {
        name: String,
        responder: oneshot::Sender<Result<Area, DbError>>,
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
            Self::MarkTaskReviewed { .. } => "MarkTaskReviewed",
            Self::DeleteProject { .. } => "DeleteProject",
            Self::CreateTag { .. } => "CreateTag",
            Self::UpdateTag { .. } => "UpdateTag",
            Self::DeleteTag { .. } => "DeleteTag",
            Self::SetTaskTags { .. } => "SetTaskTags",
            Self::EnsureTag { .. } => "EnsureTag",
            Self::EnsureArea { .. } => "EnsureArea",
            Self::CreatePerspective { .. } => "CreatePerspective",
            Self::UpdatePerspective { .. } => "UpdatePerspective",
            Self::DeletePerspective { .. } => "DeletePerspective",
        }
    }
}
