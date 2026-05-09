// SPDX-License-Identifier: MIT
//! Vault-projection hook abstraction (v0.9.0).
//!
//! atrium-core's worker doesn't know what "the vault" is — that's
//! atrium-org's concern (or any future sibling that wires up a
//! different projection). This module exposes the minimal contract
//! the worker needs: a notifier the worker pings after every
//! successful Task / Project / Tag mutation that affects a project.
//!
//! Phase 16 (v0.7.16) wired this directly to a concrete `mpsc::Sender`
//! into atrium-core's old `sync::vault_writer` module. v0.9.0 lifted
//! the projection layer into atrium-org and replaced the concrete
//! sender with this trait so atrium-core stays Org-agnostic.

use std::sync::Arc;

/// Implemented by a downstream consumer that turns "this project is
/// dirty" notifications into projected file writes (atrium-org's
/// `VaultWriter` task is the only impl today).
///
/// Implementations must be `Send + Sync` so they can live on the
/// worker (a single-threaded tokio task) while accepting non-blocking
/// notifications from the same thread. They must be cheap to call —
/// the worker fires one notification per successful mutation; any
/// real work (debouncing, file IO) happens behind the impl.
pub trait VaultDirtyNotifier: Send + Sync {
    /// Non-blocking signal that `project_id`'s persisted projection
    /// (e.g., its `.org` file) needs re-emitting. Implementations
    /// should never block — the worker's command loop calls this
    /// inline after every commit.
    fn notify_project_dirty(&self, project_id: i64);
}

/// Optional vault configuration passed through
/// [`crate::db::worker::spawn_with_vault`] at startup. The notifier
/// is implemented by atrium-org (`atrium_org::OrgVaultNotifier` from
/// `atrium_org::spawn_org_vault`); atrium-core only knows it as a
/// trait object.
pub struct VaultConfig {
    pub notifier: Arc<dyn VaultDirtyNotifier>,
}
