# Atrium — Keyboard Map

The canonical written reference for every keyboard shortcut Atrium binds. The in-app **Keyboard Shortcuts** dialog (`Ctrl+?` / `F1`) renders the same map; both stay manually aligned with `atrium/src/main.rs::install_accels`.

## General

| Shortcut | Action | Status |
|---|---|---|
| `Ctrl+N` | New task in active list | ✓ Phase 4 |
| `Ctrl+L` | Focus the sidebar filter (find-as-you-type) | ✓ Phase 7e |
| `Ctrl+Z` | Undo last toggle / delete (matches the active toast) | ✓ Phase 7f |
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

These act on the focused row in the current list. From Phase 7h, the three input-conflicting chords (`Space`, `Delete`, `Ctrl+A`) are bound at the list-view level (`gtk::ShortcutController` with `ShortcutScope::Managed`) so they only fire when the task list itself or one of its rows has keyboard focus. Type-into-an-entry behaviour (Space inserts a space, Delete forward-deletes a character, Ctrl+A selects entry text) works everywhere else.

| Shortcut | Action |
|---|---|
| `Space` | Toggle completion |
| `Delete` | Delete focused task |
| `F2` | Start inline editing on the focused row's title. Same surface as double-click. |
| `Double-click` | Start inline editing on the row's title (v0.1.10). Single click selects + holds focus. |
| `Ctrl+T` | Open the tag editor for the focused / first-selected task (Phase 7g). Right-click on a task row also surfaces *Edit Tags…* |
| `Ctrl+I` | Open the Inspector (full task editor — title, notes, schedule, deadline, project, tags) for the focused / first-selected task (Phase 7i). Right-click → *Edit Details…* is the menu equivalent. |
| `Ctrl+Click` | Toggle row in the multi-selection (Phase 7c) |
| `Shift+Click` | Extend the multi-selection range (Phase 7c) |
| `Ctrl+A` | Select all in the active list (Phase 7c) |
| `Esc` | Clear multi-selection (Phase 7c) |

## Search filter expressions (Phase 7d)

Mix freeform text and filter clauses inside the search bar. AND semantics — every filter must match.

| Token | Meaning |
|---|---|
| `tag:NAME` | Task bears the named tag (case-insensitive) |
| `is:open` | Open task (`completed_at IS NULL`) |
| `is:done` / `is:completed` / `is:complete` | Completed task |
| `is:overdue` / `due:overdue` | Open task with `deadline < today` |
| `due:today` | Open task with `deadline == today` |

Examples: `Q3 tag:work` · `tag:errand is:open` · `due:overdue` · `email tag:family is:done`.

## Library (Phase 5b)

These manage the area / project hierarchy in the sidebar.

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+N` | New Project (lands inside the active area when one is selected) |
| `Ctrl+Shift+A` | New Area |
| `F2` | Rename the active project or area |
| `Ctrl+Shift+Delete` | Delete the active project or area (with confirmation) |

## Builder Mode (sketched, not yet bound)

Phase 10+ will add these. Listed here so the slots stay reserved. Phase 10's Inspector pane reuses the existing `Ctrl+I` chord — Simple Mode already binds it to "Open Inspector dialog", and Builder Mode will toggle the side-pane variant on the same key (the data flow is identical, only the host widget differs).

| Shortcut | Action | Lands in |
|---|---|---|
| `Ctrl+Shift+F` | Open Forecast | Phase 12 |
| `Ctrl+Shift+M` | Open Calendar Month View | Phase 12.5 |
| `Ctrl+Shift+R` | Open Review queue | Phase 13 |
| `Ctrl+P` | Perspective picker | Phase 14 |
| `Ctrl+D` | Defer-date editor | Phase 11 |

## Reserved (stub bindings)

These are wired in `install_accels` so muscle memory works once the feature lands; activating them today is a no-op or shows a "coming in Phase X" toast.

| Shortcut | Action | Lands in |
|---|---|---|
| `Ctrl+Shift+Z` | Redo | Phase 11+ (Builder Mode work history) |
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
