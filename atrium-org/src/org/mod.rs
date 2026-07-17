// SPDX-License-Identifier: MIT
//! Hand-rolled Org-mode parser + emitter for the Atrium vault
//! projection (Phase 16, v0.7.7).
//!
//! `atrium-core::sync::org` exposes a focused, passthrough parser
//! for the Org subset spec §7.3 maps to: headlines, TODO/DONE/
//! CANCELLED keywords, SCHEDULED/DEADLINE/CLOSED cookies, headline
//! tags, `:PROPERTIES:` drawers, and body text. Anything
//! Atrium doesn't model (custom TODO keywords, source blocks,
//! tables, latex, links, drawers other than :PROPERTIES:) is
//! captured into the task's `unknown_lines` field and re-emitted
//! verbatim on write — satisfying spec §7.3.3 rule 1 ("Never
//! destroy data").
//!
//! No third-party crates; the parser fits the CalibreQuarry
//! stdlib-only ethos. See CLAUDE.md's dependency-discipline
//! section for the full reasoning behind choosing this over
//! `orgize` / `starsector`.

mod emit;
mod import;
mod parse;
mod write;

pub use emit::{emit_org_file, emit_org_file_with_meta, emit_org_text, emit_org_text_with_meta};
pub use import::{
    ImportError, ImportSummary, import_org_directory, import_org_file, import_org_file_with_area,
};
pub use parse::{
    OrgClockEntry, OrgFile, OrgKeyword, OrgRepeater, OrgTask, parse_org_file,
    parse_org_file_with_meta, parse_org_text, parse_org_text_with_meta,
};
pub use write::{
    WriteError, WriteSummary, project_vault_path, render_project_to_string,
    write_all_projects_to_vault, write_project_to_vault,
};

use std::collections::{BTreeMap, HashMap};

/// v0.24.0 — Property-drawer keys the schema models through typed
/// columns. The Org importer + vault watcher consume these into
/// `NewTask` fields directly; everything else stashes into
/// `task.extra_properties` via [`extras_from_properties`] for
/// verbatim round-trip per spec §7.3.3 rule 1.
///
/// `CREATED` / `MODIFIED` aren't currently read by the importer
/// (Atrium's `created_at` / `modified_at` triggers stamp them at
/// write time), but they're listed here defensively — a manual
/// user-set `:CREATED:` value would otherwise round-trip-conflict
/// with the schema-managed timestamp on a re-emit.
///
/// `ORIG_KEYWORD` isn't emitted as a property today (the keyword
/// itself sits on the headline), but listing it future-proofs
/// against a writer-side change.
pub const MODELED_PROPERTY_KEYS: &[&str] = &[
    "ID",
    "CREATED",
    "MODIFIED",
    "DEFER_UNTIL",
    "EFFORT",
    "RRULE",
    "ORIG_KEYWORD",
];

/// Partition a parsed `:PROPERTIES:` drawer into the unmodeled-key
/// extras Atrium stashes on `task.extra_properties`. The
/// modeled-key set ([`MODELED_PROPERTY_KEYS`]) is filtered out;
/// everything else lands in the returned [`BTreeMap`]. Case-
/// sensitive uppercase match — the parser uppercases keys on
/// capture, so the same casing applies on both ends.
pub fn extras_from_properties(properties: &HashMap<String, String>) -> BTreeMap<String, String> {
    properties
        .iter()
        .filter(|(k, _)| !MODELED_PROPERTY_KEYS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
