// SPDX-License-Identifier: MIT
//! Shared error hierarchy for the headless core.
//!
//! `DbError` covers anything the SQLite layer can produce; `DomainError`
//! covers domain-layer invariant violations. `CoreError` is the wrapper
//! the binary's `AtriumError` flows from.

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

    /// Phase 15 — the caller supplied a `repeat_rule` text that
    /// failed RFC 5545 parsing. Carries the diagnostic from the
    /// underlying parser so the UI editor can surface it.
    #[error("invalid repeat rule: {0}")]
    BadRepeatRule(String),
}

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("domain invariant violated: {0}")]
    Invariant(String),
    // Concrete variants land in Phase 2 alongside the domain types.
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error(transparent)]
    Db(#[from] DbError),

    #[error(transparent)]
    Domain(#[from] DomainError),
}
