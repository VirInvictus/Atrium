// SPDX-License-Identifier: MIT
//! In-app debug surface (spec §3.4).
//!
//! Activated by the `--debug` CLI flag. Phase 0 ships the shell — the
//! `Pane` is a no-op. Stress generators land in Phase 1, IO
//! instrumentation in Phase 2, memory watch in Phase 8.

use tracing::debug;

/// Debug pane gated on the `--debug` CLI flag. Mounts into the
/// application shell as a hidden side panel from Phase 3 onward; in
/// Phase 0 it logs that it's active and does nothing else.
#[derive(Debug, Default)]
pub struct Pane;

impl Pane {
    pub fn new() -> Self {
        debug!("debug::Pane initialised (Phase 0 stub — no widget mounted yet)");
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_constructs() {
        let _ = Pane::new();
    }
}
