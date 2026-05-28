// SPDX-License-Identifier: MIT
//! todo.txt importer (Phase 19, v0.27.0).
//!
//! todo.txt is the simplest of the importer formats. One task
//! per line, plain text, no quoting. The Gina Trapani spec is
//! at <https://github.com/todotxt/todo.txt>.
//!
//! Layout:
//!
//! ```text
//! [x DATE] [(A-Z)] [YYYY-MM-DD] description tokens
//! ```
//!
//! Where:
//!
//! - Leading `x ` marks completed; an optional second field is
//!   the completion date (`YYYY-MM-DD`).
//! - Optional `(L)` priority letter A through Z.
//! - Optional creation date `YYYY-MM-DD`.
//! - Description carries inline `@context` (→ tag) and `+project`
//!   (→ dropped lossy; `--into` wins) tokens plus arbitrary
//!   `key:value` extensions.
//!
//! Module layout mirrors the v0.26.0 Taskwarrior importer:
//! parser produces a typed `TodoTxtTask`; mapper applies via
//! `WorkerHandle`; `LossyKind` enum tracks dropped constructs.
//!
//! No new dependencies; stdlib only.

pub mod mapper;
pub mod parser;

#[cfg(test)]
mod round_trip_tests;
