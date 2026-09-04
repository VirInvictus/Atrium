# Atrium — Accessibility Audit (Phase 8f)

This document captures the v0.1 accessibility audit. It's a snapshot — each change to a UI surface should re-check against the relevant section. Updated whenever a slice lands that meaningfully alters the keyboard map, sidebar, or task row.

> **v0.6.x note.** The Phase 8f findings below cover the v0.1 surface area. The Builder Mode side pane (Phase 10), Forecast (Phase 12), Review (Phase 13), Perspectives (Phase 14), the kanban renderer (Slice D1), and the Agenda canonical page (Slice D2) all carry their accessible labels by inheriting the same widget primitives audited here, but a full re-audit covering the new surfaces is owed at the next minor — track in `roadmap.md`. The keyboard map below is updated through v0.6.20.

## Round 2 — v0.35.0 (Phase 20)

The owed re-audit, covering every surface added since the v0.1 pass: the Builder Inspector pane, Forecast, Review, Perspectives, the kanban board, Agenda, Calendar, and the Tier 2/3 surfaces (the "Blocked by" group, the first-run onboarding `AdwStatusPage`, the unified import dialog, the "New from Template…" picker, the Backups preferences page).

Findings and the fixes applied:

- **Labelled widget primitives carry through.** The task row (`task_list.rs`), Inspector rows, dialogs, and `adw::PreferencesPage` / `AdwActionRow` / `AdwSwitchRow` surfaces inherit the labels + roles audited in v0.1; the new surfaces compose those primitives, so they read correctly to AT-SPI without per-surface work.
- **Empty-state pages are self-describing.** Review / Agenda / Logbook / onboarding use `AdwStatusPage` with a title + description; the icon is decorative and the text is the accessible name. No gap.
- **Icon-only buttons now carry explicit accessible names.** A tooltip is exposed as an accessible *description*, not the accessible *name* a screen reader announces. v0.35.0 adds `accessible::Property::Label` to the icon-only buttons across these surfaces: calendar month nav (prev/next), the sidebar New-Perspective add, the tag-editor add, the Inspector Notes "Link to another task" button, the "Blocked by" add + per-row remove buttons, and the preferences vault-folder picker. (Buttons that already carry visible text — Today, the month-picker, Import, the onboarding CTAs, the Backups buttons — are named by their label.)
- **Status is never colour-only.** The "Blocked" and sequential "queued" row treatments pair their colour with text (a "Blocked" pill label; the queued row also italicises) so the state survives for colour-blind users and screen readers.

A full assistive-technology pass (Orca screen reader, keyboard-only traversal of every new dialog) is owed on a real display and is Brandon's verification step; the structural labelling above is the code-side record.

## Round 3 — the de-adwaita re-audit (Phase 22 follow-through, 2026-09-04)

Phase 22 (C1 → C10) removed libadwaita, and several accessibility claims in this document silently depended on libadwaita mechanisms. This round re-verifies each against the owned stylesheet (`atrium/src/ui/theme.rs`, generated into the app at priority USER+1) and `data/style.css`:

- **Focus rings are owned and scoped.** `theme.rs` draws a 2 px accent `outline` on named interactive targets only (`button`, `entry`, `spinbutton`, `switch`, `checkbutton`, `radio`, `dropdown`, `scale`, `.atrium-swatch`, plus the sidebar/list/board rows via `:focus-visible` in `style.css`). Keyboard-only by construction — a bare modifier press never lights the whole window. This *resolves* the old "focus-ring CSS" known gap below.
- **High-contrast is NOT handled.** The old claim that surfaces "respect the user's high-contrast mode" died with libadwaita. The owned palette is fixed Kanagawa Dragon; `color_scheme.rs` reads only `color-scheme` (dark/light) from the settings portal, never `high-contrast`. Nobody ships a high-contrast sheet and none is planned for 1.0 — recorded honestly as a gap, not inherited behaviour.
- **Reduced motion gates on GTK4, not libadwaita.** The fade-in keyframe and the row transitions are plain CSS; GTK4 runs them unless `gtk-enable-animations` is off, and that setting is fed by the session's settings portal from the desktop's enable-animations preference. The net behaviour matches the old claim *when a settings portal is running*; under a bare Hyprland session without portal settings, GTK defaults to animations on. Verifying the preference end-to-end rides the Phase 21 portal display-pass items.
- **Touch targets are owned numbers now.** `theme.rs` sets explicit minimums: 34 px standard buttons, 24 px header-bar buttons and entries, 18 px checklist marks, 14 px scale sliders. That is tighter than the 44 px libadwaita defaults this document used to assert — the tiling-first design tightened chrome deliberately, and pointer users get hit-area from padding; but small targets on touch hardware are a real regression against the old numbers, so it is listed as a gap rather than papered over.

The screen-reader labelling layer (tooltips + `accessible::Property::Label`) is untouched by the toolkit swap — plain GTK4 exposes the same AT-SPI properties — so Round 2's findings and the conventions below carry over unchanged.

## Keyboard end-to-end

Every common operation has a chord; mouse is optional. Full table lives in [`docs/keymap.md`](keymap.md). Highlights:

| Surface | Ops bindable from the keyboard |
|---|---|
| App | New task (`Ctrl+N`), Quick Entry (`Ctrl+Alt+Space`), Search (`Ctrl+F`), Quit (`Ctrl+Q`), Shortcuts dialog (`Ctrl+?` / `F1`) |
| Navigation | Inbox / Today / Upcoming / Anytime / Someday / Logbook (`Ctrl+1` … `Ctrl+6`) |
| Task list | Toggle complete (`Space`), Delete (`Delete`), Inline edit (`F2`), Select all (`Ctrl+A`), Clear selection (`Esc`), Bulk Complete / Delete (toolbar buttons keyboard-focusable) |
| Sidebar | Filter focus (`Ctrl+L`), Rename active (`F2`), Delete active (`Ctrl+Shift+Delete`), New Project (`Ctrl+Shift+N`), New Area (`Ctrl+Shift+A`), New Tag (`Ctrl+Shift+T`) |
| Undo | `Ctrl+Z` invokes the active toast's callback (Phase 7f) |
| Sidebar filter | `Esc` clears (matches `gtk::SearchEntry` default `stop-search`) |

`docs/keymap.md` is the source of truth; the `Ctrl+?` Shortcuts dialog renders the same chords. Both are kept in lock-step manually — see the "Adding a shortcut" section in `keymap.md`.

## Screen reader labels

Atrium tags every interactive widget with either a visible label, a `tooltip-text`, or an `accessible::Property::Label` so AT-SPI consumers (Orca, Speakup, Newsbeuter ATs) have something to announce.

### Audit findings

| Surface | Source | Status |
|---|---|---|
| Hamburger menu button | `data/window.ui` `id="menu_button"` | ✓ `tooltip-text="Main Menu"` |
| New task button | `data/window.ui` `id="new_task_button"` | ✓ `tooltip-text="New Task (Ctrl+N)"` |
| Search toggle | `data/window.ui` `id="search_button"` | ✓ `tooltip-text="Search (Ctrl+F)"` |
| Selection bar Complete / Delete | `data/window.ui` bulk toolbar | ✓ Visible text labels |
| Selection bar clear icon | `data/window.ui` `win.bulk-clear` button | ✓ `tooltip-text="Clear selection (Esc)"` |
| Task row CheckButton | `atrium/src/ui/task_list.rs::build_factory` (Phase 8f) | ✓ `tooltip-text` + `accessible::Property::Label("Task complete")` |
| Task row title `EditableLabel` | `atrium/src/ui/task_list.rs` (Phase 8f) | ✓ `tooltip-text` + `accessible::Property::Label("Task title")` |
| Sidebar canonical / area / project / tag rows | `atrium/src/ui/window/widgets.rs::sidebar_row` (Phase 8f) | ✓ `set_tooltip_text` + `accessible::Property::Label` mirror the visible label |
| Sidebar filter entry | `data/window.ui` `id="sidebar_filter"` | ✓ `placeholder-text="Filter lists…"` (announced as the entry's name) |
| Quick Entry entry | `atrium/src/quickentry/modal.rs::open` | ✓ `placeholder-text` describes the entry's purpose + hint |
| Memory Watch (debug) | `atrium/src/debug/mod.rs::open_memory_watch` (Phase 8e) | ✓ Each row pairs a key Label and a value Label |

### Conventions

- Icon-only buttons must have `tooltip-text`.
- Widgets without a visible label that AT-SPI cares about (CheckButton, EditableLabel) must call `update_property(&[gtk::accessible::Property::Label(...)])`.
- `gtk::ListBoxRow` instances built dynamically (areas, projects, tags) get a tooltip *and* an accessible label so keyboard navigation announces consistently with pointer hover.

## Contrast

`data/style.css` does not hard-code foreground or background colours: every visible surface leans on CSS colour variables (`@accent_color`, `@window_bg_color`, `@card_bg_color`, …). Since Phase 22 those names are *defined by the owned sheet* — `theme.rs` redefines all 24 of them in Kanagawa Dragon hues via `@define-color` — rather than inherited from libadwaita's palette. The values are fixed: there is no light theme (Lotus is post-1.0) and no high-contrast variant (see Round 3).

Hardcoded colours in the project:

| File | Where | Status |
|---|---|---|
| `logo.svg` | App icon shell + monogram | Decorative; not a UI surface. Replace before 1.0 (per the `<!-- ... -->` comment in the SVG). |
| metainfo `<branding>` colours | Software-center branding | Sampled from the palette; software-center use only. |

The high-legibility font toggle (Atkinson Hyperlegible, Phase 8c) is the explicit accessibility surface for low-vision readers. It changes type only; it does not (and cannot yet) raise contrast, because the palette is fixed.

## Touch / pointer

`recommends/control` in the metainfo declares `pointing`, `keyboard`, and `touch`. Target sizing is owned by `theme.rs` since the de-adwaita: 34 px standard buttons, 24 px header-bar buttons and entries, 18 px checklist marks, 14 px scale sliders. That is tighter than libadwaita's 44 px defaults, which this document previously asserted; small targets on touch hardware are a recorded regression (Round 3), traded deliberately for tiling-first chrome density. Pointer and keyboard behaviour is unaffected.

## Known gaps (deferred)

- **~~Focus-ring CSS~~: resolved.** The Phase 22 C9 sheet draws a scoped, high-contrast accent ring on named interactive targets (see Round 3); the old "hard to see" GTK default ring is gone.
- **High-contrast palette**: none. The owned Kanagawa sheet is fixed; the portal's `high-contrast` hint is not read. Post-1.0 alongside any light theme.
- **Touch-target density**: owned minimums are tighter than the 44 px they replaced (Round 3). Revisit if touch use ever becomes real rather than declared.
- **Reduced-motion**: gated on GTK4's `gtk-enable-animations` (portal-fed). The session-level end-to-end check (preference off → animations off in Atrium) rides the Phase 21 portal display-pass items; by inspection Atrium adds no motion of its own that bypasses the setting.
- **Voice control**: not addressed. AT-SPI's `accessible::Property::Label` is the same metadata voice-control engines consume, so labelling buttons covers the basic case; complex commands (e.g., "complete task three") need higher-level integration that lands no earlier than Phase 20.

## Re-running the audit

Whenever a Rust file under `atrium/src/ui/` adds an interactive widget:

1. Add `tooltip-text` (in `.ui`) or `set_tooltip_text` (in code) for icon-only surfaces.
2. Add `update_property(&[gtk::accessible::Property::Label(...)])` for widgets without a visible text label.
3. Append a row to the table above.

If a slice changes the keyboard map, also update `docs/keymap.md` and `atrium/src/ui/shortcuts.rs::SHORTCUTS_XML`.
