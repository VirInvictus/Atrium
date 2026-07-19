// SPDX-License-Identifier: MIT
//! The owned application stylesheet (Phase 22 C9): Kanagawa Dragon baked
//! into one generated sheet, replacing libadwaita's named-colour palette
//! and restyling the base widgets to Atrium's design language (spec §3.7):
//! flat and calm, but gently rounded (controls ~8px, cards / popovers /
//! toasts ~12px, switches and pills fully round), with a soft drop shadow
//! on floating panels. The square, brutalist stance of the sibling
//! de-adwaita apps was softened here after seeing it live — Atrium is a
//! Things-3-style surface, not a utilitarian tool, so it carries rounding.
//!
//! Two jobs:
//!
//! 1. **`@define-color` block.** `data/style.css` still references the
//!    adwaita colour names (`@accent_color`, `@card_shade_color`,
//!    `@window_bg_color`, the `@blue_3` / `@yellow_5` palette scale, …).
//!    libadwaita supplies those today and vanishes at C10, so this sheet
//!    defines every one in a Kanagawa hue. Loaded a step above USER
//!    priority, these win over adwaita's definitions now (recolouring
//!    adwaita's own widgets in lockstep) and stand alone once the toolkit
//!    is gone.
//!
//! 2. **Flat/square base rules.** Override adwaita's rounded, gradient
//!    chrome so the window, header bars, buttons, rows, and inputs match
//!    the owned design language. `data/style.css` layers its per-surface
//!    tweaks on top (it loads at the same priority but later, so its
//!    specific rules still win where they overlap).
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
pub const BG_WINDOW: &str = "#181616"; // dragonBlack3
pub const BG_VIEW: &str = "#12120f"; // dragonBlack1
pub const BG_HEADER: &str = "#1d1c19"; // dragonBlack2
pub const BG_CARD: &str = "#1d1c19"; // dragonBlack2
pub const FG: &str = "#c5c9c5"; // dragonWhite
pub const FG_DIM: &str = "#a6a69c"; // dragonGray
pub const GRID: &str = "#393836"; // dragonBlack5 (hairlines, borders)
pub const ACCENT: &str = "#c4b28a"; // dragonYellow (the app accent)
pub const ON_ACCENT: &str = "#12120f"; // dragonBlack1 (dark text on accent)
pub const WARN: &str = "#dca561"; // autumnYellow (brighter than the accent)
pub const ERR: &str = "#c4746e"; // dragonRed
pub const OK: &str = "#87a987"; // dragonGreen

// ── The six swatch / area-accent hues (migration 0020) ──────────
pub const SW_BLUE: &str = "#8ba4b0"; // dragonBlue2
pub const SW_GREEN: &str = "#87a987"; // dragonGreen
pub const SW_YELLOW: &str = "#c4b28a"; // dragonYellow
pub const SW_ORANGE: &str = "#b6927b"; // dragonOrange
pub const SW_RED: &str = "#c4746e"; // dragonRed
pub const SW_PURPLE: &str = "#8992a7"; // dragonViolet

/// The sheet template. `%TOKENS%` are replaced by the hexes above in
/// [`sheet`]; nothing else is substituted, so literal CSS braces are safe.
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

/* ── Base widgets — the owned flat-but-rounded replacement for the
   adwaita / GTK-default widget styling. Comprehensive on purpose: Atrium
   must look the same with or without a system GTK theme underneath, and
   after libadwaita is gone (C10). data/style.css layers its per-surface
   tweaks on top (same priority, loaded later, so its specifics win). */
window, .background { background-color: %BG_WINDOW%; color: %FG%; }
window.csd, decoration { box-shadow: none; }

headerbar {
  background-color: %BG_HEADER%;
  background-image: none;
  color: %FG%;
  box-shadow: none;
  border-bottom: 1px solid %GRID%;
  min-height: 34px;
  padding: 0 4px;
}
headerbar button { min-height: 24px; }

paned > separator {
  background-color: %GRID%;
  background-image: none;
  min-width: 1px;
  min-height: 1px;
}

listview, list, columnview { background-color: %BG_VIEW%; color: %FG%; }
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
list.boxed-list > row:last-child { border-bottom: none; }

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
button:hover { background-color: %GRID%; }
button:active, button:checked { background-color: %ACCENT%; color: %ON_ACCENT%; border-color: %ACCENT%; }
button:disabled { opacity: 0.5; }
button.flat {
  background-color: transparent;
  background-image: none;
  border-color: transparent;
  box-shadow: none;
}
button.circular { border-radius: 999px; padding: 4px; }
button.flat:hover, button.circular:hover { background-color: alpha(%FG%, 0.10); }
button.suggested-action { background-color: %ACCENT%; color: %ON_ACCENT%; border-color: %ACCENT%; }
button.suggested-action:hover { background-color: %WARN%; border-color: %WARN%; }
button.destructive-action { background-color: %ERR%; color: %ON_ACCENT%; border-color: %ERR%; }
button.pill { border-radius: 999px; padding: 5px 16px; }
.linked > button:not(:first-child) { border-left-width: 0; }
.toolbar { padding: 4px 6px; }
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
entry:focus-within, spinbutton:focus-within { border-color: %ACCENT%; }
entry > image { color: %FG_DIM%; }
spinbutton > button { border-width: 0; border-radius: 6px; background-color: transparent; }
spinbutton > button:hover { background-color: %GRID%; }
dropdown > button { background-color: %BG_CARD%; }

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
modelbutton:hover { background-color: %ACCENT%; color: %ON_ACCENT%; }
popover.menu separator, menu separator { background-color: %GRID%; min-height: 1px; margin: 4px 2px; }

tooltip, tooltip.background {
  background-color: %BG_HEADER%;
  color: %FG%;
  border: 1px solid %GRID%;
  border-radius: 8px;
  box-shadow: none;
  padding: 4px 8px;
}

scrollbar { background-color: transparent; }
scrollbar slider { background-color: %GRID%; border-radius: 999px; min-width: 6px; min-height: 6px; }
scrollbar slider:hover { background-color: %FG_DIM%; }

separator { background-color: %GRID%; min-width: 1px; min-height: 1px; }
selection { background-color: alpha(%ACCENT%, 0.35); color: %FG%; }

/* Adwaita utility classes Atrium leans on across the binary (weight / size /
   colour only; font-family stays in data/style.css). They vanish when
   libadwaita is dropped at C10, so the owned sheet carries them. */
.title-1 { font-weight: 800; font-size: 170%; }
.title-2 { font-weight: 800; font-size: 140%; }
.title-3 { font-weight: 700; font-size: 120%; }
.title-4 { font-weight: 700; font-size: 105%; }
.large-title { font-weight: 300; font-size: 200%; }
.heading { font-weight: 700; }
.caption { font-size: 82%; }
.caption-heading { font-weight: 700; font-size: 82%; }
.dim-label { color: %FG_DIM%; }
.success { color: %OK%; }
.warning { color: %WARN%; }
.error { color: %ERR%; }
.accent { color: %ACCENT%; }
.numeric { font-feature-settings: 'tnum'; }

/* The single, deliberately scoped focus ring. spec §3.7 forbids a
   universal star-selector focus ring (it lit up every row and label
   in Colophon's sheet), so this names its targets explicitly. */
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

/// The full generated sheet: the template with every `%TOKEN%` replaced by
/// its baked Dragon hex. Longest tokens first so no name is a prefix of the
/// span another replace would touch (`%BG_WINDOW%` before `%BG_VIEW%`, the
/// `%SW_*%` swatch tokens before the shorter roles).
pub fn sheet() -> String {
    TEMPLATE
        .replace("%ON_ACCENT%", ON_ACCENT)
        .replace("%BG_WINDOW%", BG_WINDOW)
        .replace("%BG_HEADER%", BG_HEADER)
        .replace("%BG_VIEW%", BG_VIEW)
        .replace("%BG_CARD%", BG_CARD)
        .replace("%FG_DIM%", FG_DIM)
        .replace("%FG%", FG)
        .replace("%GRID%", GRID)
        .replace("%ACCENT%", ACCENT)
        .replace("%WARN%", WARN)
        .replace("%ERR%", ERR)
        .replace("%OK%", OK)
        .replace("%SW_BLUE%", SW_BLUE)
        .replace("%SW_GREEN%", SW_GREEN)
        .replace("%SW_YELLOW%", SW_YELLOW)
        .replace("%SW_ORANGE%", SW_ORANGE)
        .replace("%SW_RED%", SW_RED)
        .replace("%SW_PURPLE%", SW_PURPLE)
}

/// Install the generated sheet display-wide at `USER + 1`, matching
/// `data/style.css` (`typography::apply_bundled_stylesheet`). Must be called
/// **before** the bundled sheet so, at equal priority, style.css's later
/// per-surface rules still win the ties while this sheet supplies the
/// `@define-color` names and the flat base. One step above USER also keeps
/// it authoritative over a system `~/.config/gtk-4.0/gtk.css` (the Colophon
/// discovery, Phase 22 C1).
pub fn install() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&sheet());
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER + 1,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_hex_reaches_the_sheet() {
        let sheet = sheet();
        // The role palette + the four swatch hues the sheet exposes as
        // adwaita palette-scale @define-colors. SW_ORANGE lives only in
        // data/style.css (no adwaita `@orange_*` consumer here) and SW_RED
        // coincides with ERR, so neither is asserted against this sheet.
        for hex in [
            BG_WINDOW, BG_VIEW, BG_HEADER, BG_CARD, FG, FG_DIM, GRID, ACCENT, ON_ACCENT, WARN, ERR,
            OK, SW_BLUE, SW_GREEN, SW_YELLOW, SW_PURPLE,
        ] {
            assert!(sheet.contains(hex), "missing {hex}");
        }
    }

    #[test]
    fn no_unreplaced_tokens_remain() {
        // A token is `%UPPERCASE…%`; a stray one means it was added to the
        // template but not to `sheet`'s replace chain. CSS percentages like
        // `font-size: 170%` are a `%` after a digit and are fine — so flag
        // only a `%` immediately followed by an ASCII uppercase letter.
        let sheet = sheet();
        let bytes = sheet.as_bytes();
        for i in 0..bytes.len().saturating_sub(1) {
            assert!(
                !(bytes[i] == b'%' && bytes[i + 1].is_ascii_uppercase()),
                "unreplaced token near: {}",
                &sheet[i..(i + 12).min(sheet.len())]
            );
        }
    }

    #[test]
    fn defines_every_adwaita_name_style_css_uses() {
        let sheet = sheet();
        for name in [
            "@define-color window_bg_color",
            "@define-color window_fg_color",
            "@define-color accent_color",
            "@define-color accent_bg_color",
            "@define-color card_bg_color",
            "@define-color card_shade_color",
            "@define-color borders",
            "@define-color success_color",
            "@define-color warning_color",
            "@define-color warning_bg_color",
            "@define-color error_color",
            "@define-color destructive_color",
            "@define-color destructive_bg_color",
            "@define-color blue_3",
            "@define-color yellow_5",
            "@define-color green_4",
            "@define-color purple_3",
            "@define-color purple_2",
        ] {
            assert!(sheet.contains(name), "missing {name}");
        }
    }

    #[test]
    fn carries_no_font_family_rule() {
        // Typography stays in data/style.css (the three bundled families).
        // Match the property (`font-family:`), not the bare word — a CSS
        // comment in the template mentions font-family in prose.
        assert_eq!(sheet().matches("font-family:").count(), 0);
    }

    #[test]
    fn focus_ring_is_scoped_not_universal() {
        // spec §3.7 forbids a universal `*:focus-visible` (Colophon's bug).
        let sheet = sheet();
        assert!(sheet.contains(":focus-visible"));
        assert!(!sheet.contains("*:focus-visible"));
    }
}
