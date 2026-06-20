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
| `Ctrl+3` | Upcoming |
| `Ctrl+4` | Anytime |
| `Ctrl+5` | Someday |
| `Ctrl+6` | Logbook |

All six canonical lists shipped at v0.1.0; the v0.6.x sidebar reorder (v0.6.7 / v0.6.16) joined Agenda + Review to the top tier alongside them but those derived pages don't have their own number accels — reach them via the sidebar. (v0.39.0 merged the former separate Forecast entry into the Agenda view's Builder-only Bands/Strip layout toggle.)

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

## Search filter expressions

Mix freeform text and filter clauses inside the search bar. The grammar landed at v0.4.0 (Phase 15.5) and grew through v0.5.0; the canonical reference is **`spec.md` §4.3**. Press `?` while the search entry is focused to open the in-app operator-reference popover.

Highlights — boolean operators (`AND` / `OR` / `NOT`, with `NOT > AND > OR` precedence + parens for grouping); comparison + range operators on date and numeric fields; date keywords (`today`, `tomorrow`, `thisweek`, `5daysago`, etc.); state predicates (`is:open`, `is:done`, `is:overdue`, `is:scheduled`, `is:repeating`, `is:deferred`, `is:someday`, `is:inbox`); Calibre-style match modifiers on textual fields (`tag:x` substring, `tag:=x` exact, `tag:~regex`, `tag:true` / `tag:false`); fuzzy match (`title:?term`); the `sort:` modifier.

Examples: `Q3 tag:work` · `tag:errand AND is:open` · `is:overdue` · `(tag:home OR tag:family) AND is:open` · `deadline:<thisweek` · `is:repeating sort:scheduled_for`.

## Library (Phase 5b)

These manage the area / project hierarchy in the sidebar.

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+N` | New Project (lands inside the active area when one is selected) |
| `Ctrl+Shift+A` | New Area |
| `F2` | Rename the active project or area |
| `Ctrl+Shift+Delete` | Delete the active project or area (with confirmation) |

## Builder Mode (reserved chords, not yet bound)

Builder Mode shipped at v0.2.0 — Inspector pane, Forecast, Review queue, Perspectives, defer dates and repeating tasks all reachable via the sidebar / Inspector. The chords below are still **aspirational slots**: shipped today via the sidebar / mode toggle, not via these accelerators. Listed here so they stay reserved for the binding pass.

| Shortcut | Action | Status |
|---|---|---|
| `Ctrl+Shift+F` | Open Forecast | Shipped via sidebar (Phase 12) — chord pending |
| `Ctrl+Shift+M` | Open Calendar Month View | Phase 12.5 |
| `Ctrl+Shift+R` | Open Review queue | Shipped via sidebar (Phase 13) — chord pending |
| `Ctrl+P` | Perspective picker | Shipped via sidebar Perspectives section (Phase 14) — chord pending |
| `Ctrl+D` | Defer-date editor | Shipped via Inspector (Phase 11) — chord pending |

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
