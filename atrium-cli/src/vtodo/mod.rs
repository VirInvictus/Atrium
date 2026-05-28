// SPDX-License-Identifier: MIT
//! VTODO (RFC 5545 `.ics`) import + export (Phase 19 slice 1,
//! v0.25.0).
//!
//! The CalDAV-side format used by Endeavour, Errands, Nextcloud
//! Tasks, and Planify. Per spec §7.2, Atrium does NOT act as a
//! CalDAV client: import is one-shot file read, export is a
//! one-way file dump.
//!
//! The parser + emitter are hand-rolled stdlib, matching the
//! Org parser + Todoist importer ethos documented in CLAUDE.md.
//! No `ical` crate; the VTODO subset Atrium needs is small,
//! well-defined by the RFC, and the mapping layer (typed
//! columns, RRULE round-trip, lossy report) is the bulk of the
//! work regardless of which tokeniser sits underneath.
//!
//! # Layout
//!
//! - [`parser`] — line-unfold, escape decode, parameter parse,
//!   tolerant skip of non-VTODO components (VEVENT, VJOURNAL,
//!   VTIMEZONE, X-* blocks). Produces typed [`VtodoComponent`]s
//!   with a per-component `lossy` tally.
//! - [`emit`] — VCALENDAR/VTODO writer: PRODID header, one
//!   VTODO per task, UTC for all timestamps, escape-encoded
//!   TEXT properties, 75-octet line folding.
//! - [`mapper`] — DB ↔ VTODO bridge. [`import_vtodo`] consumes
//!   parsed components and applies them through the worker;
//!   [`export_vtodo`] reads tasks via `db::read` and shapes them
//!   into [`VtodoOutput`].
//!
//! # UID round-trip anchor
//!
//! Atrium's `task.uuid` is UUID v4 by contract, but a VTODO UID
//! is free-form text. The mapper handles both: a UUID-shaped UID
//! threads directly into `task.uuid`; anything else gets a v5
//! UUID derived from [`VTODO_NAMESPACE`] + the original UID and
//! the original UID stashes into `task.extra_properties["VTODO_UID"]`
//! (the v0.24.0 column). Re-export prefers the stashed value, so
//! `task@nextcloud.example.com` round-trips through the DB
//! losslessly.

pub mod emit;
pub mod mapper;
pub mod parser;

#[cfg(test)]
mod round_trip_tests;

// Re-export the public surface so callers can use the short
// path (`vtodo::parse_ics` etc.) without reaching into each
// submodule. `allow(unused_imports)` because clippy `-D warnings`
// flags re-exports the binary's own translation unit doesn't
// consume; integration tests under `src/tests*.rs` and the
// future `tests/vtodo_round_trip.rs` do.
#[allow(unused_imports)]
pub use emit::{EmitConfig, VtodoOutput, emit_vcalendar};
#[allow(unused_imports)]
pub use mapper::{
    ExportSummary, ImportSummary, LossyEntry, LossyKind, MapError, VTODO_LOCATION_KEY,
    VTODO_NAMESPACE, VTODO_UID_KEY, export_vtodo, import_vtodo,
};
#[allow(unused_imports)]
pub use parser::{ParseError, VtodoComponent, parse_ics};
