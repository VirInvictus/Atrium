// SPDX-License-Identifier: MIT
//! Keyboard-shortcuts dialog (`Ctrl+?` / `F1`).
//!
//! Loads a `gtk::ShortcutsWindow` from `data/shortcuts.ui`, compiled
//! in via `include_str!` (v0.47.0 — the XML moved out of an inline
//! const so xgettext can extract the translatable titles). The
//! source-of-truth for what binds to what is `main.rs::install_accels`;
//! `docs/keymap.md` is the human-readable cousin and stays manually
//! aligned.

const SHORTCUTS_XML: &str = include_str!("../../../data/shortcuts.ui");

pub fn build_shortcuts_window() -> gtk::ShortcutsWindow {
    let builder = gtk::Builder::from_string(SHORTCUTS_XML);
    builder
        .object::<gtk::ShortcutsWindow>("shortcuts_window")
        .expect("shortcuts_window in inline XML")
}

// No unit tests here: `gtk::Builder::from_string` needs a fully
// initialised GTK process, which conflicts with parallel test
// scheduling. The XML is exercised end-to-end every time the user
// hits Ctrl+? (or F1); a parse failure surfaces immediately.
