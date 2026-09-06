// SPDX-License-Identifier: MIT
//! The owned application stylesheet (Phase 22 C9, restructured 2026-09-06):
//! Kanagawa Dragon baked into one generated sheet over vir-gtk's shared
//! base. The unanimous flat/square widget core now lives in
//! `vir_gtk::theme::base_css` (installed at USER + 1); this sheet keeps what
//! makes Atrium Atrium, installed at USER + 2 (`install_app_stylesheet`) so
//! it wins by priority: flat and calm, but gently rounded (controls ~8px,
//! cards / popovers / toasts ~12px, switches and pills fully round), with a
//! soft drop shadow on floating panels, circular checkbox discs, and
//! painted accent selection. The square stance of the sibling de-adwaita
//! apps was softened here after seeing it live — Atrium is a Things-3-style
//! surface, not a utilitarian tool, so it carries rounding.
//!
//! Two jobs:
//!
//! 1. **`@define-color` block.** `data/style.css` still references the
//!    adwaita colour names (`@accent_color`, `@card_shade_color`,
//!    `@window_bg_color`, the `@blue_3` / `@yellow_5` palette scale, …).
//!    That block stays app-side by design (the base sheet carries no
//!    adwaita aliases), and wins over adwaita's definitions at this
//!    priority.
//!
//! 2. **The rounded overrides.** Everything here either restates a base
//!    rule with Atrium's rounding (rows, buttons, entries, popovers,
//!    tooltips, scrollbars), paints what the base paints differently
//!    (selection, checked), or is Atrium-only (circles, swatches, the
//!    generic separator). `data/style.css` still layers its per-surface
//!    tweaks on top (installed just after this sheet at the same tier, so
//!    its specifics win).
//!
//! Custom properties are avoided (one fixed palette; `@define-color` is
//! enough and keeps the sheet legible), so hexes are spliced by `%TOKEN%`
//! replacement — plain CSS braces stay untouched. Typography (the three
//! bundled font families) stays in `data/style.css`; this sheet carries no
//! `font-family` rule.
//!
//! The accent is **dragonYellow** (`#c4b28a`) — Brandon's pick, matching
//! the app icon's courtyard floor.
// ── The Dragon roles ────────────────────────────────────────────
// ── The six swatch / area-accent hues (migration 0020) ──────────
const SW_BLUE: &str = "#8ba4b0"; // dragonBlue2
const SW_GREEN: &str = "#87a987"; // dragonGreen
const SW_YELLOW: &str = "#c4b28a"; // dragonYellow
const SW_ORANGE: &str = "#b6927b"; // dragonOrange
const SW_RED: &str = "#c4746e"; // dragonRed
const SW_PURPLE: &str = "#8992a7"; // dragonViolet

/// The palette with Atrium's accent override.
fn palette() -> vir_gtk::theme::Palette {
    let mut p = vir_gtk::theme::Palette::dragon();
    p.accent = "#c4b28a";
    p.on_accent = "#12120f";
    p
}

/// The shared base sheet, spliced with Atrium's palette. Installed at
/// USER + 1 by [`install`]; this module's sheet at USER + 2 overrides it.
pub fn base() -> String {
    vir_gtk::theme::base_css(&palette())
}

/// The app-owned sheet template. `%TOKENS%` are replaced by the hexes above
/// in [`sheet`]; nothing else is substituted, so literal CSS braces are safe.
const TEMPLATE: &str = "\
/* ── Adwaita named-colour replacement (consumed by data/style.css) ── */
@define-color window_bg_color %BG_WINDOW%;
@define-color window_fg_color %FG%;
@define-color view_bg_color %BG_VIEW%;
@define-color view_fg_color %FG%;
@define-color accent_color %ACCENT%;
@define-color accent_bg_color %ACCENT%;
@define-color accent_fg_color %ON_ACCENT%;
@define-color card_bg_color %BG_CARD%;
@define-color card_fg_color %FG%;
@define-color card_shade_color %GRID%;
@define-color borders %GRID%;
@define-color success_color %OK%;
@define-color warning_color %WARN%;
@define-color warning_bg_color %WARN%;
@define-color error_color %ERR%;
@define-color destructive_color %ERR%;
@define-color destructive_bg_color %ERR%;
@define-color blue_3 %SW_BLUE%;
@define-color yellow_5 %SW_YELLOW%;
@define-color green_4 %SW_GREEN%;
@define-color purple_3 %SW_PURPLE%;
@define-color purple_2 %SW_PURPLE%;
/* ── Atrium's rounded idiom over the square base ── */
row { border-radius: 8px; }
row.activatable:hover { background-color: alpha(currentColor, 0.05); }
row:selected { background-color: alpha(%ACCENT%, 0.26); color: %FG%; }
.navigation-sidebar { background-color: %BG_VIEW%; padding: 2px 6px; }
.navigation-sidebar > row {
  padding: 6px 10px;
  border-radius: 8px;
  margin: 1px 0;
}
.card, list.boxed-list {
  background-color: %BG_CARD%;
  color: %FG%;
  border: 1px solid %GRID%;
  border-radius: 12px;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.22);
}
list.boxed-list > row { border-bottom: 1px solid alpha(%GRID%, 0.6); }
button {
  background-color: %BG_CARD%;
  background-image: none;
  color: %FG%;
  border: 1px solid %GRID%;
  border-radius: 8px;
  box-shadow: none;
  min-height: 24px;
  padding: 3px 12px;
  transition: background-color 120ms ease, border-color 120ms ease;
}
button:disabled { opacity: 0.5; color: %FG%; border-color: %GRID%; background-color: %BG_CARD%; }
button.circular { border-radius: 999px; padding: 4px; }
button.flat:hover, button.circular:hover { background-color: alpha(%FG%, 0.10); }
button.suggested-action:hover { background-color: %WARN%; border-color: %WARN%; }
button.pill { border-radius: 999px; padding: 5px 16px; }
.osd { background-color: alpha(%BG_WINDOW%, 0.85); color: %FG%; border-radius: 12px; }
entry, spinbutton, .entry {
  background-color: %BG_VIEW%;
  background-image: none;
  color: %FG%;
  border: 1px solid %GRID%;
  border-radius: 8px;
  box-shadow: none;
  transition: border-color 120ms ease;
}
entry > image { color: %FG_DIM%; }
spinbutton > button { border-width: 0; border-radius: 6px; background-color: transparent; }
/* Checkboxes render as clean circles (the Things-3 / Reminders idiom, and
   what the .selection-mode task checkbox wants). An outline when open, a
   filled dragonYellow disc when done. Radios are already round. Owned here
   so it does not depend on whatever theme sits underneath. */
checkbutton check, check, radio, .selection-mode check {
  border-radius: 999px;
  border: 2px solid alpha(%FG%, 0.40);
  background-color: transparent;
  background-image: none;
  box-shadow: none;
  min-width: 18px;
  min-height: 18px;
  transition: background-color 120ms ease, border-color 120ms ease;
}
check:hover, radio:hover, .selection-mode check:hover { border-color: %ACCENT%; }
check:checked, radio:checked, .selection-mode check:checked {
  background-color: %ACCENT%;
  color: %ON_ACCENT%;
  border-color: %ACCENT%;
}
switch {
  border-radius: 999px;
  background-color: %BG_VIEW%;
  border: 1px solid %GRID%;
  min-width: 40px;
}
switch:checked { background-color: %ACCENT%; border-color: %ACCENT%; }
switch > slider {
  border-radius: 999px;
  background-color: %FG%;
  margin: 2px;
  min-width: 18px;
  min-height: 18px;
}
scale { padding: 4px 0; }
scale > trough { background-color: %GRID%; border-radius: 999px; min-height: 4px; }
scale > trough > highlight { background-color: %ACCENT%; border-radius: 999px; }
scale > trough > slider { border-radius: 999px; background-color: %FG%; min-width: 14px; min-height: 14px; }
popover > arrow { background-color: %BG_CARD%; border: 1px solid %GRID%; }
popover > contents, .popover > contents {
  background-color: %BG_CARD%;
  color: %FG%;
  border: 1px solid %GRID%;
  border-radius: 12px;
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.38);
  padding: 6px;
}
popover.menu modelbutton { border-radius: 6px; padding: 5px 8px; }
popover.menu separator, menu separator { background-color: %GRID%; min-height: 1px; margin: 4px 2px; }
tooltip, tooltip.background {
  background-color: %BG_HEADER%;
  color: %FG%;
  border: 1px solid %GRID%;
  border-radius: 8px;
  box-shadow: none;
  padding: 4px 8px;
}
scrollbar slider { background-color: %GRID%; border-radius: 999px; min-width: 6px; min-height: 6px; }
separator { background-color: %GRID%; min-width: 1px; min-height: 1px; }
/* The single, deliberately scoped focus ring, at Atrium's 2px weight and
   with the radio and swatch targets the shared base's ring does not name.
   spec §3.7 forbids a universal star-selector focus ring (it lit up every
   row and label in Colophon's sheet), so this names its targets explicitly. */
button:focus-visible, entry:focus-visible, spinbutton:focus-visible,
switch:focus-visible, checkbutton:focus-visible, check:focus-visible,
radio:focus-visible, dropdown:focus-visible, scale:focus-visible,
.atrium-swatch:focus-visible {
  outline: 2px solid %ACCENT%;
  outline-offset: -1px;
}
.toast {
  background-color: %BG_CARD%;
  color: %FG%;
  border: 1px solid %GRID%;
  border-radius: 12px;
  padding: 8px 14px;
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.38);
}
";
/// The full generated app sheet: the template with every `%TOKEN%` replaced
/// by its baked Dragon hex. Longest tokens first so no name is a prefix of
/// the span another replace would touch (`%BG_WINDOW%` before `%BG_VIEW%`,
/// the `%SW_*%` swatch tokens before the shorter roles).
pub fn sheet() -> String {
    palette().replace_tokens(TEMPLATE)
        .replace("%SW_BLUE%", SW_BLUE)
        .replace("%SW_GREEN%", SW_GREEN)
        .replace("%SW_YELLOW%", SW_YELLOW)
        .replace("%SW_ORANGE%", SW_ORANGE)
        .replace("%SW_RED%", SW_RED)
        .replace("%SW_PURPLE%", SW_PURPLE)
}
pub fn install() {
    vir_gtk::theme::install_stylesheet(&base());
    vir_gtk::theme::install_app_stylesheet(&sheet());
}
