// SPDX-License-Identifier: MIT
//! Quick Entry capture modal.
//!
//! Phase 6b ships the inline parser shared with the bottom-of-list
//! entry. Phase 6c adds the modal `adw::Window` and `Ctrl+Alt+Space`
//! accelerator. The OS-global shortcut (true zero-launch capture)
//! is `atriumd` daemon — Phase 20.

pub mod modal;
pub mod parser;
