// SPDX-License-Identifier: MIT
//! Import sources for the CLI's `atrium-cli import <source>` family.
//!
//! Each submodule owns one source format. The atrium-org Org-mode
//! importer + writer live in their own workspace crate; the
//! Todoist CSV importer lives here for v0.12 and lifts to its
//! own crate only if a non-CLI consumer (TUI, atriumd) earns it
//! later. Mirrors the placement decision documented in roadmap
//! §18 + CLAUDE.md's dependency discipline section.
//!
//! `dead_code` is allowed at the module scope while the
//! Todoist importer comes online across the v0.12 commit stack:
//! the parser, recurrence parser, and mapper land in separate
//! commits and aren't all wired through the CLI subcommand
//! until the closing commit. The lid comes off when the
//! subcommand dispatcher hits this module.
#![allow(dead_code)]

pub mod todoist;
