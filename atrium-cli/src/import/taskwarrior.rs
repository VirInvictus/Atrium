// SPDX-License-Identifier: MIT
//! Taskwarrior `task export` JSON importer (Phase 19, v0.26.0).
//!
//! Splits into two layers, each pure and independently testable:
//!
//! 1. **JSON parser** ([`parser::parse_export`]) — accepts both
//!    Taskwarrior's array form (`task export json.array=on`,
//!    default) and the line-stream form (`json.array=off`).
//!    Tolerant of UTF-8 BOM. Unknown JSON fields land in a
//!    `BTreeMap<String, String>` per task so the mapper can
//!    decide UDA routing.
//! 2. **Mapper + worker dispatcher** ([`mapper::import_taskwarrior`])
//!    — turns the parsed task stream into worker calls plus a
//!    lossy-fields report. Behaviour for UDA fields is gated on
//!    the `--uda-as tag|note|drop` CLI flag (default `tag`).
//!
//! Field mapping is documented in spec §7.5 (the VTODO section
//! mirrors the same lossy-report shape) and per-cut plan
//! `foamy-churning-summit.md`.
//!
//! No new dependencies: stdlib + `serde_json` (already in the
//! workspace dep graph). Matches the CLAUDE.md "hand-rolled
//! stdlib importers in atrium-cli" trick — but the Taskwarrior
//! JSON shape is well-defined enough that `serde_json::Value`
//! handles the unknown-field stash cleanly, no hand-rolled JSON
//! parser needed.

pub mod mapper;
pub mod parser;

#[cfg(test)]
mod round_trip_tests;
