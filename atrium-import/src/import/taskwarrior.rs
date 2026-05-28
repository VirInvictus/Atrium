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

/// How a Taskwarrior user-defined attribute (UDA) is mapped on import.
/// Moved here from `atrium-cli` at v0.34.0 (the extraction) since it's
/// import-domain; the CLI's `--uda-as tag|note|drop` flag parses into
/// it via [`UdaPolicy::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UdaPolicy {
    /// Each UDA becomes a tag of the form `name-value` (default —
    /// matches how Atrium treats every other importer's labels).
    #[default]
    Tag,
    /// Each UDA appends one `UDA: name=value` line to `task.note`.
    /// Preserves data without polluting the tag surface.
    Note,
    /// Each UDA surfaces in the lossy report and otherwise drops.
    /// Most defensive — useful for hand triage.
    Drop,
}

impl UdaPolicy {
    /// Parse the `--uda-as` flag value. Returns an error message
    /// suitable for the CLI's argv layer on an unknown value.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "tag" => Ok(Self::Tag),
            "note" => Ok(Self::Note),
            "drop" => Ok(Self::Drop),
            other => Err(format!(
                "invalid --uda-as value {other:?} (expected tag, note, or drop)"
            )),
        }
    }
}
