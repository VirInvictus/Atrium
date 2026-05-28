// SPDX-License-Identifier: MIT
//! Non-Org import/export formats for Atrium.
//!
//! Extracted from `atrium-cli` at v0.34.0 so both the CLI and the GTK
//! binary's import dialog share one implementation. Org-mode import +
//! export keep living in `atrium-org` (also a library); this crate
//! holds the formats that were CLI-only until the GUI dialog earned
//! their graduation:
//!
//! - [`import`] — Todoist CSV, Taskwarrior `task export` JSON, todo.txt.
//! - [`vtodo`] — VTODO (`.ics`) import + one-way export.
//!
//! All parsers are hand-rolled stdlib (no `csv` / `ical` / `regex`
//! crate), matching the Org parser + sidecar ethos. Each mapper drives
//! the `atrium-core` single-writer worker and returns an
//! `ImportSummary` (counts + a per-source `LossyKind` report).

pub mod import;
pub mod vtodo;

pub use import::taskwarrior::UdaPolicy;
