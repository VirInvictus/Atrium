// SPDX-License-Identifier: MIT
//! GTK / libadwaita widget tree.
//!
//! - [`typography`]: bundle the three font families and the base CSS.
//! - [`window`]: the `AdwApplicationWindow` subclass via composite
//!   template (`data/window.ui`).
//! - [`about`]: the "About Atrium" `adw::AboutDialog`.
//!
//! Phase 4 will add list views and the inline editor; Phase 6 the
//! Quick Entry modal; Phase 10 the Inspector pane.

pub mod about;
pub mod agenda;
pub mod board;
pub mod calendar;
pub mod filter;
pub mod forecast;
pub mod import_dialog;
pub mod inline_complete;
pub mod inspector;
pub mod inspector_pane;
pub mod logbook;
pub mod preferences;
pub mod review;
pub mod shortcuts;
pub mod tag_editor;
pub mod task_list;
pub mod task_object;
pub mod typography;
pub mod window;
