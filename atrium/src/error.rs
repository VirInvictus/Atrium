// SPDX-License-Identifier: MIT
//! Binary-side error hierarchy.
//!
//! `UiError` carries failure modes that originate in the GTK
//! application layer — the things `atrium-core`'s `DbError` and
//! `DomainError` don't reach. `AtriumError` is the aggregate type
//! the binary's setup paths return; it wraps `CoreError`, `DbError`,
//! and `UiError` so the boot path can mix in errors from any of the
//! three without manual conversion.
//!
//! Most runtime errors flow as `Result<_, DbError>` directly through
//! the worker handle and surface as toasts. `AtriumError` is reserved
//! for the boot / setup path where multiple error families converge.

use std::io;

use thiserror::Error;

use atrium_core::{CoreError, DbError};

#[derive(Debug, Error)]
pub enum UiError {
    /// The user-configured `vault-path` GSettings value points at a
    /// path that can't be created or opened. The binary surfaces the
    /// failure as a log warning and falls back to DB-only mode rather
    /// than refusing to boot — a bad vault setting shouldn't lock
    /// the user out of their tasks.
    #[error("vault path {path:?} is unusable: {reason}")]
    VaultPathInvalid { path: String, reason: String },
}

#[derive(Debug, Error)]
pub enum AtriumError {
    #[error(transparent)]
    Core(#[from] CoreError),

    #[error(transparent)]
    Db(#[from] DbError),

    #[error(transparent)]
    Ui(#[from] UiError),

    #[error("io: {0}")]
    Io(#[from] io::Error),
}
