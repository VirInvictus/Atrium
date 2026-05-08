// SPDX-License-Identifier: MIT
//! Quick Entry capture modal.
//!
//! Phase 6c adds the modal `adw::Window` and `Ctrl+Alt+Space`
//! accelerator. The OS-global shortcut (true zero-launch capture)
//! is `atriumd` daemon — Phase 20.
//!
//! v0.4.5 — the inline parser (`#tag` / `@today` / `@deadline ...`)
//! moved to `atrium_core::quick_entry` so atrium-cli's `capture`
//! subcommand and any future TUI / daemon surface can reuse it.
//! The GTK modal is the only thing that lives here now.

pub mod modal;
