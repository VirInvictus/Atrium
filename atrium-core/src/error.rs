// SPDX-License-Identifier: MIT
//! Shared error hierarchy for the headless core.
//!
//! `DbError` covers anything the SQLite layer can produce, plus
//! domain-invariant rejections wrapped in via the `Domain` variant.
//! `DomainError` carries the typed invariants the worker enforces
//! before a write touches the database. `CoreError` is the
//! aggregate the binary's `AtriumError` flows from.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("migration {version} failed: {source}")]
    Migration {
        version: i64,
        source: rusqlite::Error,
    },

    #[error("not found")]
    NotFound,

    /// Returned when a `WorkerHandle` operation is attempted but the
    /// worker has shut down (the cmd channel is closed or the
    /// responder dropped).
    #[error("worker channel closed")]
    WorkerClosed,

    /// The caller supplied a `repeat_rule` text that failed RFC 5545
    /// parsing. Carries the diagnostic from the underlying parser so
    /// the UI editor can surface it.
    #[error("invalid repeat rule: {0}")]
    BadRepeatRule(String),

    /// Sync / serialization-layer error. Used by the JSON snapshot
    /// exporter when serde_json fails (extremely rare — would require
    /// a domain type whose Serialize impl rejects its own valid
    /// state, which we don't have).
    #[error("sync error: {0}")]
    Sync(String),

    /// Domain invariant rejected at write time.
    #[error("domain: {0}")]
    Domain(#[from] DomainError),
}

/// Domain-layer invariants the worker enforces before committing a
/// write. Returned via [`DbError::Domain`] from the worker handle so
/// callers don't need a separate error tree.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    /// A subtask was created or moved such that its `parent_id` lives
    /// in a different project than the subtask itself. Subtask
    /// hierarchies must stay within a project — moving a parent task
    /// without its children would otherwise orphan them across the
    /// project boundary. The schema's FK ensures the parent row
    /// exists; this rule catches the cross-project case the FK
    /// can't.
    // v0.23.1 — message format previously leaked `Some(N)` Debug
    // output to user-facing surfaces. Project ids render bare; the
    // unfiled (`None`) case prints `unfiled`.
    #[error(
        "parent task {parent_task} is in project {parent}; \
         cannot host a child claiming project {claimed}",
        parent = parent_project.map_or_else(|| String::from("unfiled"), |id| id.to_string()),
        claimed = claimed_project.map_or_else(|| String::from("unfiled"), |id| id.to_string()),
    )]
    ParentProjectMismatch {
        parent_task: i64,
        parent_project: Option<i64>,
        claimed_project: Option<i64>,
    },

    /// A reparent would create a cycle: the requested parent is the
    /// task itself or one of its descendants. Walking the parent
    /// chain up from the requested parent reaches the task being
    /// moved. Rejected so the hierarchy stays a tree.
    #[error("reparenting task {task} under {parent} would create a cycle")]
    ParentCycle { task: i64, parent: i64 },

    /// A perspective was created or updated with an empty or
    /// whitespace-only filter expression. A perspective with no
    /// predicate has no rows; reject at write time so the GUI
    /// surfaces the editor error rather than producing a blank
    /// sidebar entry.
    #[error("perspective filter expression is empty")]
    EmptyFilterExpr,

    /// v0.18.0 — Phase 18.5 Tier-1. A Quick Entry template's
    /// `shortcut_key` must be a single ASCII alphanumeric
    /// character (or NULL = no shortcut). The constraint can't
    /// be expressed cleanly in SQL without a check trigger; the
    /// worker enforces.
    #[error("shortcut_key must be a single ASCII alphanumeric character, got {got:?}")]
    InvalidShortcutKey { got: String },
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error(transparent)]
    Db(#[from] DbError),

    #[error(transparent)]
    Domain(#[from] DomainError),
}
