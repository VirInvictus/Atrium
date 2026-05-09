// SPDX-License-Identifier: MIT
//! Todoist CSV importer (Phase 18, v0.12.0).
//!
//! Splits into three layers, each pure and independently testable:
//!
//! 1. **CSV parser** ([`parse_csv`]) — tolerant of UTF-8 BOM, quoted
//!    fields with embedded commas, blank-row separators. Returns a
//!    `Vec<TodoistRow>` typed by the `TYPE` column.
//! 2. **Recurrence parser** (lives in [`recurrence`] sibling
//!    module) — natural-language `DATE` strings → RFC 5545 RRULE.
//! 3. **Mapper + worker dispatcher** (lives in [`importer`]
//!    sibling) — turns a parsed row stream into worker calls
//!    plus a lossy-fields report.
//!
//! Anchored to `atrium-cli/tests/fixtures/todoist/home.csv` —
//! the gold-standard fixture from Brandon's daughter Rin's
//! chore-tracker. Roadmap §18 makes that file the round-trip
//! acceptance contract.

pub mod parser;
pub mod recurrence;
