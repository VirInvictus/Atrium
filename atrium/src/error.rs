// SPDX-License-Identifier: MIT
//! Binary-side error hierarchy.
//!
//! `UiError` covers GTK / widget-layer failures; `AtriumError` is the
//! main result type, wrapping core errors and UI errors so the
//! application can propagate cleanly.

// Phase 0 ships the error scaffolding before anything propagates through
// it; the GTK shell (Phase 3) and data layer (Phase 1+) will start using
// these. The `allow(dead_code)` is a Phase 0–2 thing; remove once Phase 3
// wires the application shell and `AtriumError` becomes the main result.
#![allow(dead_code)]

use thiserror::Error;

use atrium_core::CoreError;

#[derive(Debug, Error)]
pub enum UiError {
    #[error("ui error: {0}")]
    Generic(String),
    // GTK-specific variants land in Phase 3 with the application shell.
}

#[derive(Debug, Error)]
pub enum AtriumError {
    #[error(transparent)]
    Core(#[from] CoreError),

    #[error(transparent)]
    Ui(#[from] UiError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
