// SPDX-License-Identifier: MIT
//! Phase 16 — Org-mode vault projection.
//!
//! `atrium-core::sync` houses the import / export / two-way mirror
//! pipeline that bridges Atrium's SQLite-canonical model to the
//! Org vault projection (spec §7.3, roadmap Phase 16).
//!
//! Module layout:
//!
//! - [`atomic`] — `write-temp + fsync + rename` helper. Every
//!   vault write goes through this so a crash mid-write never
//!   leaves a partial file (spec §7.3.3 rule 6).
//! - [`org`] — hand-rolled Org-mode parser + emitter. Lands in
//!   v0.7.7. Reads `.org` files into an intermediate `OrgTask`
//!   tree and emits the same shape back. The "preserve unknown
//!   constructs verbatim" rule (spec §7.3.3 rule 1) is satisfied
//!   by capturing every unrecognised line into the task's
//!   `unknown_lines` field and re-emitting verbatim on write.
//!
//! v0.7.6 lands the foundation (atomic-write helper + GSettings
//! key for the vault path); the parser, importer, writer, and
//! worker hook follow in v0.7.7 → v0.7.10. v0.8.0 stamps Phase
//! 16 complete with the round-trip fixture and a maintenance
//! pass.

pub mod atomic;
pub mod org;
