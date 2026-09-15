// SPDX-License-Identifier: MIT
//! Quick Entry capture modal.
//!
//! Phase 6c added the modal window and the `Ctrl+Alt+Space`
//! accelerator (an `adw::Window` then; a non-modal `gtk::Window`
//! since C8). The OS-global shortcut (true zero-launch capture)
//! is `atriumd` daemon — Phase 20.
//!
//! v0.4.5 — the inline parser (`#tag` / `@today` / `@deadline ...`)
//! moved to `atrium_core::quick_entry` so atrium-cli's `capture`
//! subcommand and any future TUI / daemon surface can reuse it.
//! v0.13.0 lifted that module into its own `atrium-inline` crate
//! (atrium-inline Slice 3) so the parser ships independently of
//! the storage layer. The GTK modal is the only thing that lives
//! here now.

pub mod modal;
