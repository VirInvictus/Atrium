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
//! section + the v0.7.6 patchnotes for the full reasoning behind
//! choosing this over `orgize` / `starsector`.
//!
//! v0.7.7 ships the parser + tests. The emitter, importer, and
//! writer follow in v0.7.8+.

mod emit;
mod import;
mod parse;
mod write;

pub use emit::{emit_org_file, emit_org_file_with_meta, emit_org_text, emit_org_text_with_meta};
pub use import::{ImportError, ImportSummary, import_org_file};
pub use parse::{
    OrgFile, OrgKeyword, OrgRepeater, OrgTask, parse_org_file, parse_org_file_with_meta,
    parse_org_text, parse_org_text_with_meta,
};
pub use write::{WriteError, WriteSummary, write_all_projects_to_vault, write_project_to_vault};
