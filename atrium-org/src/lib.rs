// SPDX-License-Identifier: MIT
//! Org-mode projection for the Atrium task manager (Phase 16,
//! extracted into its own crate at v0.9.0).
//!
//! Provides the hand-rolled parser + emitter, the one-shot importer,
//! the project → `.org` writer, and the auto-debounced `VaultWriter`
//! task that hooks into atrium-core's worker via the
//! [`atrium_core::VaultDirtyNotifier`] trait.
//!
//! The crate is split into two modules:
//!
//! - [`org`] — parser, emitter, importer, writer. The pure
//!   functional surface; no tokio task lives here. Used directly
//!   by `atrium-cli`'s `import org` / `export org` subcommands.
//! - [`vault_writer`] — the background task that turns
//!   `ProjectDirty(project_id)` notifications into debounced
//!   `.org` file writes. Used by the GTK binary's boot path.
//!
//! Convenience builder [`spawn_org_vault`] gives the GUI / CLI a
//! single entry point: pass a vault root and a [`ReadPool`], get
//! back a [`VaultConfig`] ready to thread into
//! [`atrium_core::spawn_worker_with_vault`].
//!
//! No third-party Org crate. `orgize` and `starsector` were both
//! surveyed at Phase 16 and rejected as dormant — see CLAUDE.md's
//! dependency-discipline section. The hand-roll satisfies the
//! "preserve unknown constructs verbatim" rule (spec §7.3.3 rule 1)
//! by capturing every unrecognised line into the task's
//! `unknown_lines` field and re-emitting on write.

use std::path::PathBuf;
use std::sync::Arc;

use atrium_core::VaultConfig;
use atrium_core::db::read_pool::ReadPool;

pub mod org;
pub mod vault_writer;

pub use vault_writer::{OrgVaultNotifier, VaultWriteRequest, VaultWriter, spawn_vault_writer};

/// One-shot builder: spin up an Org `VaultWriter` against `root` +
/// `pool` and return a [`VaultConfig`] ready to pass into
/// [`atrium_core::spawn_worker_with_vault`].
///
/// The writer task lives on the current tokio runtime for the
/// lifetime of the worker. Drops cleanly when the worker drops.
pub fn spawn_org_vault(root: PathBuf, pool: ReadPool) -> VaultConfig {
    let notifier = spawn_vault_writer(root, pool);
    VaultConfig {
        notifier: Arc::new(notifier),
    }
}
