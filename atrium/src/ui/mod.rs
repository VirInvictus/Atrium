// SPDX-License-Identifier: MIT
//! GTK4 widget tree (libadwaita dropped at Phase 22 C10).
//!
//! - [`typography`]: bundle the three font families and the base CSS.
//! - [`window`]: the `AdwApplicationWindow` subclass via composite
//!   template (`data/window.ui`).
//! - [`about`]: the "About Atrium" `gtk::AboutDialog`.
//! - [`status_page`]: owned empty-state composite (the de-adwaita
//!   `adw::StatusPage` replacement).
//!
//! Phase 4 will add list views and the inline editor; Phase 6 the
//! Quick Entry modal; Phase 10 the Inspector pane.

pub mod about;
pub mod agenda;
pub mod board;
pub mod calendar;
pub mod clamp;
pub mod dialogs;
pub mod filter;
pub mod forecast;
pub mod import_dialog;
pub mod inline_complete;
pub mod inspector;
pub mod inspector_pane;
pub mod logbook;
pub mod preferences;
pub mod review;
pub mod rows;
pub mod shortcuts;
pub mod status_page;
pub mod tag_editor;
pub mod task_list;
pub mod task_object;
pub mod theme;
pub mod typography;
pub mod window;

/// Phase 21 — the app-wide compact-width idiom. Below this width a
/// window counts as "compact" and surfaces trade chrome for fit: the
/// Calendar Month View swaps its grid for a week strip (Phase 12.5),
/// and the window carries a `compact` CSS class the bulk toolbars
/// tighten under. The window watches `default-width` and caches the
/// flag so resizes don't thrash (`window/shell.rs`). Originally a
/// Calendar-only constant; promoted once a second consumer landed.
pub const COMPACT_WIDTH_THRESHOLD: i32 = 600;
