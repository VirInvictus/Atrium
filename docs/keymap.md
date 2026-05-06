# Atrium — Keyboard Map

The canonical written reference for every keyboard shortcut Atrium binds. The in-app **Keyboard Shortcuts** dialog (`Ctrl+?` / `F1`) renders the same map; both stay manually aligned with `atrium/src/main.rs::install_accels`.

## General

| Shortcut | Action | Status |
|---|---|---|
| `Ctrl+N` | New task in active list | ✓ Phase 4 |
| `Ctrl+?` / `F1` | Show this dialog | ✓ Phase 4 |
| `Ctrl+Q` | Quit | ✓ Phase 3 |

## Navigation (Simple Mode lists)

| Shortcut | List |
|---|---|
| `Ctrl+1` | Inbox |
| `Ctrl+2` | Today |
| `Ctrl+3` | Upcoming *(view lands Phase 5)* |
| `Ctrl+4` | Anytime *(view lands Phase 5)* |
| `Ctrl+5` | Someday *(view lands Phase 5)* |
| `Ctrl+6` | Logbook *(view lands Phase 5)* |

The accel works regardless of whether the list itself is fully implemented — pressing it switches the active list; an unimplemented list shows a placeholder.

## List actions

These act on the focused row in the current list.

| Shortcut | Action |
|---|---|
| `Space` | Toggle completion |
| `Delete` | Delete focused task |
| `Enter` | Edit title inline *(land Phase 4 stretch — currently double-click on the title)* |

## Library (Phase 5b)

These manage the area / project hierarchy in the sidebar.

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+N` | New Project (lands inside the active area when one is selected) |
| `Ctrl+Shift+A` | New Area |
| `F2` | Rename the active project or area |
| `Ctrl+Shift+Delete` | Delete the active project or area (with confirmation) |

## Builder Mode (sketched, not yet bound)

Phase 10+ will add these. Listed here so the slots stay reserved.

| Shortcut | Action | Lands in |
|---|---|---|
| `Ctrl+I` | Toggle Inspector pane | Phase 10 |
| `Ctrl+Shift+F` | Open Forecast | Phase 12 |
| `Ctrl+Shift+M` | Open Calendar Month View | Phase 12.5 |
| `Ctrl+Shift+R` | Open Review queue | Phase 13 |
| `Ctrl+P` | Perspective picker | Phase 14 |
| `Ctrl+D` | Defer-date editor | Phase 11 |

## Reserved (stub bindings)

These are wired in `install_accels` so muscle memory works once the feature lands; activating them today is a no-op or shows a "coming in Phase X" toast.

| Shortcut | Action | Lands in |
|---|---|---|
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / Redo | Phase 7 |
| `Ctrl+F` | Open search | Phase 7 |
| `Ctrl+,` | Preferences | Phase 8 |

## Discovery rules

- **Every visible action carries its accel in the menu** — `Ctrl+Q` next to "Quit", etc.
- **The Shortcuts dialog is always one keypress away** — `Ctrl+?` from anywhere.
- **No silent overrides of OS conventions** — no rebinding `Ctrl+C`, `Ctrl+V`, etc.; if an action conflicts with what the user expects in a text field, the text field wins.

## Adding a shortcut

1. Add the action in `install_actions` (or a window-scoped action via `install_window_actions`).
2. Bind the accel in `install_accels`.
3. Add a row to the matching section above.
4. Add a `<GtkShortcutsShortcut>` entry to `atrium/src/ui/shortcuts.rs::SHORTCUTS_XML`.

The four edits land in the same change so docs / dialog / source stay in step.
