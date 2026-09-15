// SPDX-License-Identifier: MIT
//! Import sources for the CLI's `atrium-cli import <source>` family.
//!
//! Each submodule owns one source format. The atrium-org Org-mode
//! importer + writer live in their own workspace crate; this
//! crate is that lifted home for the non-Org sources (extracted
//! from atrium-cli at v0.34.0 when the GUI import dialog became
//! a second consumer, the exact trigger the v0.12 note anticipated).

pub mod taskwarrior;
pub mod todoist;
pub mod todotxt;
