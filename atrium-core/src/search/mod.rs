// SPDX-License-Identifier: MIT
//! Calibre-powered search expression language — Phase 15.5.
//!
//! Replaces the v0.1 flat filter parser ([`crate::ui::filter`] in the
//! atrium binary) with a full Calibre-shaped grammar:
//!
//! - **Field operators:** `tag:`, `tags:`, `area:`, `project:`,
//!   `title:`, `note:`, `is:`, `due:`, `scheduled:`, `defer:`,
//!   `created:`, `modified:`, `completed:`, `estimated:`,
//!   `repeats:`.
//! - **Match modifiers:** `tag:x` (substring), `tag:"x y"` (quoted
//!   substring), `tag:=x` (exact), `tag:"=x y"` (quoted exact),
//!   `tag:~regex` (regex), `tag:true` / `tag:false` (boolean
//!   existence).
//! - **Boolean operators:** `AND` / `OR` (case-insensitive),
//!   implicit `AND` between bare tokens, `NOT` / `!` prefix.
//!   Standard precedence: `NOT > AND > OR`.
//! - **Comparison operators:** `=` `!=` `>` `<` `>=` `<=` on date
//!   and numeric fields.
//! - **Date keywords:** `today`, `yesterday`, `tomorrow`,
//!   `thisweek`, `lastweek`, `nextweek`, `thismonth`, `lastmonth`,
//!   `nextmonth`, `thisyear`, `Ndaysago`, `Ndaysout`.
//! - **Range syntax:** `due:2026-05-01..2026-05-31` (inclusive).
//!
//! See [`spec.md`] §4.3 for the full reference; this module is the
//! spec's executable counterpart.
//!
//! ## Module structure
//!
//! - [`lex`] — string → tokens.
//! - [`parse`] — tokens → AST ([`Expr`]).
//! - [`eval`] — AST → bool against a single [`Task`] (in-memory path).
//! - [`sql`] — AST → SQL `WHERE` clause when expressible;
//!   `Cannot::Regex` etc. when not (caller falls back to `eval`).
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

pub use ast::{Comparator, DateKeyword, Expr, Field, MatchKind, Value};
pub use eval::{EvalContext, evaluate};
pub use parse::{ParseError, parse};
