// SPDX-License-Identifier: MIT
//! Calibre-powered search expression language for Atrium.
//!
//! Independent workspace crate as of v0.4.2 — the engine had been
//! living inside `atrium-core::search` since Phase 15.5 (v0.4.0);
//! v0.4.2 lifted it out so it can be exercised, fuzzed, and reused
//! without dragging the SQLite/worker layer along. The atrium GTK
//! binary's `ui::filter` is the primary consumer; a future TUI /
//! `atriumd` capture daemon / search-server reuses the same crate.
//!
//! ## Grammar surface
//!
//! - **Field operators:** `tag:`, `tags:`, `area:`, `project:`,
//!   `title:`, `note:`, `is:`, `due:`, `scheduled:`, `defer:`,
//!   `created:`, `modified:`, `completed:`, `estimated:`,
//!   `repeats:`.
//! - **Match modifiers:** `tag:x` (substring), `tag:"x y"` (quoted
//!   substring), `tag:=x` (exact), `tag:"=x y"` (quoted exact),
//!   `tag:~regex` (regex), `tag:?word` (fuzzy / Damerau-Levenshtein),
//!   `tag:true` / `tag:false` (boolean existence).
//! - **Boolean operators:** `AND` / `OR` (case-insensitive),
//!   implicit `AND` between bare tokens, `NOT` / `!` prefix.
//!   Standard precedence: `NOT > AND > OR`.
//! - **Comparison operators:** `=` `!=` `>` `<` `>=` `<=` on date
//!   and numeric fields.
//! - **Date keywords:** `today`, `yesterday`, `tomorrow`,
//!   `thisweek`, `lastweek`, `nextweek`, `thismonth`, `lastmonth`,
//!   `nextmonth`, `thisyear`, `Ndaysago`, `Ndaysout`.
//! - **Range syntax:** `due:2026-05-01..2026-05-31` (inclusive).
//! - **Sort modifier:** `sort:KEY` / `sort:-KEY` — metadata, not a
//!   predicate; lifted to [`parse::ParseResult::sorts`].
//! - **State predicates:** `is:open`, `is:overdue`, `is:today`,
//!   `is:inbox`, … — one-token shortcuts that read directly off
//!   task fields.
//!
//! See `spec.md` §4.3 in the repo for the full operator reference;
//! this crate is the spec's executable counterpart.
//!
//! ## Module structure
//!
//! - [`lex`] — string → tokens.
//! - [`parse`] — tokens → AST ([`Expr`]) + extracted sort modifiers.
//! - [`eval`] — AST → bool against a single
//!   [`atrium_core::Task`] (in-memory path).
//!
//! Each layer is independently testable and the module-level tests
//! exercise the round-trip: input string → parsed AST → re-rendered
//! string → re-parsed AST that matches the first.

mod ast;
mod eval;
mod lex;
mod parse;

#[cfg(test)]
mod tests;

pub use ast::{
    Comparator, DateKeyword, Expr, Field, MatchKind, SortDirection, SortKey, SortSpec, Value,
};
pub use eval::{EvalContext, evaluate};
pub use parse::{ParseError, parse};
