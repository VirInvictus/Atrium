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

mod parse;

pub use parse::{OrgKeyword, OrgRepeater, OrgTask, parse_org_file, parse_org_text};
