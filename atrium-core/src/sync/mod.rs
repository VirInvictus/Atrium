// SPDX-License-Identifier: MIT
//! Generic sync helpers and the lossless DB snapshot exporter.
//!
//! atrium-core's sync surface is intentionally small: an atomic-write
//! helper used by every projection writer + the lossless JSON snapshot
//! that's not specific to any one projection. The Org-mode parser /
//! emitter / importer / writer + the auto-debounced `VaultWriter` task
//! live in the sibling `atrium-org` crate (extracted at v0.9.0).
//!
//! Keep this module focused on projection-agnostic primitives. If a
//! second projection (e.g., Markdown, TaskPaper) ever lands, extract
//! it into its own sibling crate and have it depend on `atomic` here.

pub mod atomic;
pub mod json;
