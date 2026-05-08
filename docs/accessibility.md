# Atrium — Accessibility Audit (Phase 8f)

This document captures the v0.1 accessibility audit. It's a snapshot — each change to a UI surface should re-check against the relevant section. Updated whenever a slice lands that meaningfully alters the keyboard map, sidebar, or task row.

> **v0.6.x note.** The Phase 8f findings below cover the v0.1 surface area. The Builder Mode side pane (Phase 10), Forecast (Phase 12), Review (Phase 13), Perspectives (Phase 14), the kanban renderer (Slice D1), and the Agenda canonical page (Slice D2) all carry their accessible labels by inheriting the same widget primitives audited here, but a full re-audit covering the new surfaces is owed at the next minor — track in `roadmap.md`. The keyboard map below is updated through v0.6.20.

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
| Hamburger menu button | `data/window.ui` line 19 | ✓ `tooltip-text="Main Menu"` |
| New task button | `data/window.ui` line 84 | ✓ `tooltip-text="New Task (Ctrl+N)"` |
| Search toggle | `data/window.ui` line 94 | ✓ `tooltip-text="Search (Ctrl+F)"` |
| Selection bar Complete / Delete | `data/window.ui` lines 163-180 | ✓ Visible text labels |
| Selection bar clear icon | `data/window.ui` line 181 | ✓ `tooltip-text="Clear selection (Esc)"` |
| Task row CheckButton | `atrium/src/ui/task_list.rs::build_factory` (Phase 8f) | ✓ `tooltip-text` + `accessible::Property::Label("Task complete")` |
| Task row title `EditableLabel` | `atrium/src/ui/task_list.rs` (Phase 8f) | ✓ `tooltip-text` + `accessible::Property::Label("Task title")` |
| Sidebar canonical / area / project / tag rows | `atrium/src/ui/window.rs::sidebar_row` (Phase 8f) | ✓ `set_tooltip_text` + `accessible::Property::Label` mirror the visible label |
| Sidebar filter entry | `data/window.ui` line 47 | ✓ `placeholder-text="Filter lists…"` (announced as the entry's name) |
| Quick Entry entry | `atrium/src/quickentry/modal.rs::open` | ✓ `placeholder-text` describes the entry's purpose + hint |
| Memory Watch (debug) | `atrium/src/debug/mod.rs::open_memory_watch` (Phase 8e) | ✓ Each row pairs a key Label and a value Label |

### Conventions

- Icon-only buttons must have `tooltip-text`.
- Widgets without a visible label that AT-SPI cares about (CheckButton, EditableLabel) must call `update_property(&[gtk::accessible::Property::Label(...)])`.
- `gtk::ListBoxRow` instances built dynamically (areas, projects, tags) get a tooltip *and* an accessible label so keyboard navigation announces consistently with pointer hover.

## Contrast

CSS in `data/style.css` does not hard-code foreground or background colours. Every visible surface inherits from libadwaita's CSS variables, which respect the active light / dark theme and the user's high-contrast mode (`prefer-contrast: more`).

Hardcoded colours in the project:

| File | Where | Status |
|---|---|---|
| `logo.svg` | App icon shell + monogram | Decorative; not a UI surface. Replace before 1.0 (per the `<!-- ... -->` comment in the SVG). |
| `data/io.github.virinvictus.atrium.metainfo.xml` | `<branding>` colours | Sampled from the logo; software-center use only. |

The high-legibility font toggle (Atkinson Hyperlegible, Phase 8c) is the explicit accessibility surface for low-vision readers. It pairs with libadwaita's standard high-contrast palette without further work.

## Touch / pointer

`recommends/control` in the metainfo declares `pointing`, `keyboard`, and `touch`. Touch-targets are sized via libadwaita defaults (44 px minimum on `GtkButton`, `GtkCheckButton`, `gtk::ListBoxRow`); Atrium doesn't shrink them.

## Known gaps (deferred)

- **Focus-ring CSS**: relying on libadwaita defaults. A future Phase 8 polish pass might add a higher-contrast focus ring for the task list (currently the GTK default ring can be hard to see on dim-label rows). Tracked but not done.
- **Reduced-motion**: the `@keyframes atrium-quickentry-fade-in` and the `.atrium-task-row.completed` opacity transition both honour libadwaita's animation-disable preference (libadwaita gates `transition` declarations on `prefer-reduced-motion`). Atrium adds no motion that ignores the preference. Verified by inspection of `style.css`.
- **Voice control**: not addressed. AT-SPI's `accessible::Property::Label` is the same metadata voice-control engines consume, so labelling buttons covers the basic case; complex commands (e.g., "complete task three") need higher-level integration that lands no earlier than Phase 20.

## Re-running the audit

Whenever a Rust file under `atrium/src/ui/` adds an interactive widget:

1. Add `tooltip-text` (in `.ui`) or `set_tooltip_text` (in code) for icon-only surfaces.
2. Add `update_property(&[gtk::accessible::Property::Label(...)])` for widgets without a visible text label.
3. Append a row to the table above.

If a slice changes the keyboard map, also update `docs/keymap.md` and `atrium/src/ui/shortcuts.rs::SHORTCUTS_XML`.
