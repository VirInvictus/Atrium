<p align="center">
  <img src="logo.svg" alt="Atrium" width="240">
</p>

<p align="center">
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Language-Rust-blue" alt="Language: Rust"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/GNOME-50%2B-4a86cf" alt="GNOME 50+">
  <img src="https://img.shields.io/badge/status-feature--complete%20%C2%B7%20heading%20to%201.0-2ea44f" alt="Status: feature-complete, heading to 1.0">
</p>

---

# Atrium

**The native GNOME task manager you grow into, not out of.**

Atrium pairs Org-mode's data discipline (UUIDs on every node, plain-text round-trip, three repeater semantics, a full bidirectional `.org` vault) with a Things 3 / OmniFocus surface, over a single local-first SQLite store. Two surfaces share one schema: **Simple Mode** for *what am I doing right now* (six calm lists, no defer dates, no review queue), and **Builder Mode** for when the system needs to do the work (Forecast, Calendar, Review, Perspectives, repeating tasks, sequential projects, the always-visible Inspector). Mode is a UI-layer flip that never touches the database; the schema is the OmniFocus superset on day one, so Simple Mode hides Builder fields, it doesn't lack them.

## Why this exists

**Org-mode without Emacs.** Org gives you UUIDs on every node, deadlines and schedules as distinct fields, repeating tasks with three completion semantics (`+` / `++` / `.+`), tags as multi-attach metadata, and full plain-text round-trip. None of those primitives are deep; the reason most people don't use Org is that the surface is Emacs. Atrium maps the same primitives 1:1 into a GTK4 app, and a two-way `inotify`-driven vault means edits in Doom or vim-orgmode flow back within ~200 ms. Atrium isn't an Org client; the vault is a peer projection.

**Things 3 and OmniFocus, on Linux, done right.** The two apps that taught GTD to a generation fail in opposite ways. Things is calm and beautiful but omits so much that power users outgrow it (no defer dates, no review, no forecast). OmniFocus exposes every knob, and its failure mode is fiddling with fields instead of doing tasks. Atrium lets you grow into Builder Mode when your system demands it and fall back to Simple Mode when it doesn't, without changing apps or migrating data.

**Calibre's search vocabulary, everywhere search runs.** A real boolean expression grammar (`AND` / `OR` / `NOT`, parens, precedence), match modifiers on every text field (substring, exact, regex, fuzzy), comparison and range on dates and numerics, and `is:` state predicates. The same grammar parses in the search bar, drives saved Perspectives, runs through the CLI, and translates to SQL fast-paths when expressible. Power users get power; everyone else sees a search box.

**Local-first, no exceptions.** SQLite at `$XDG_DATA_HOME/atrium/atrium.db`, WAL mode, single-writer worker, read-only connection pool. No CalDAV client, no cloud sync, no telemetry, no accounts. The Org vault is filesystem mirroring, not network. Your data lives on your machine and stays there.

**Author's Note:** I'm a college student in my late thirties with no professional industry experience yet; Atrium is one in a string of native Linux desktop apps I'm building to learn the craft and assemble a portfolio. I came from Things 3 and OmniFocus on macOS / iOS, and Linux has nothing in their lane that isn't an Electron wrapper or a CalDAV form over a webview. Atrium is the answer I wanted to exist. I work on Fedora 44 on a ThinkPad T14s AMD Gen 6; that's the environment it's tested against. I welcome contributions but can only honestly support my own setup.

## Screenshots

<p align="center">
  <img src="docs/Screenshots/Today%20View%20-%20Simple%20Mode.png" alt="Today View, Simple Mode" width="820">
</p>

<p align="center"><em>Today, Simple Mode: six canonical lists, coloured <code>#tag</code> pills, the Area › Project chip on each row, the per-area accent stripe.</em></p>

<p align="center">
  <img src="docs/Screenshots/Today%20View%20-%20Builder%20Mode.png" alt="Today View, Builder Mode" width="820">
</p>

<p align="center"><em>Today, Builder Mode: same data, same rows, with the always-visible Inspector exposing the fields Simple Mode hides.</em></p>

<p align="center">
  <img src="docs/Screenshots/Upcoming%20View%20-%20Builder%20Mode.png" alt="Upcoming View, Builder Mode" width="820">
</p>

<p align="center"><em>Upcoming, Builder Mode: defer-aware filtering, sequential-project dimming, Inspector open.</em></p>

<p align="center">
  <img src="docs/Screenshots/Project%20View.png" alt="Project View" width="820">
</p>

<p align="center"><em>Project page: the area accent paints the row-left stripe; the header breadcrumb anchors Area › Project.</em></p>

## Simple Mode

A Things 3 analogue for GNOME: calm, opinionated, keyboard-first.

| | |
|---|---|
| **Lists** | Inbox, Today, Upcoming, Anytime, Someday, Logbook (with day-band grouping). |
| **Hierarchy** | Areas → Projects → Tasks, with multi-tag orthogonal to both. |
| **Dates** | Distinct *When* (scheduled) and *Deadline*, the Things detail most clones get wrong. |
| **Tags & areas** | A six-swatch palette; tags render as coloured `#pills`, an area paints a stripe down every row whose project lives under it. |
| **Quick Entry** | A global modal (`Ctrl+Alt+Space`) that drops to the Inbox without stealing focus, with inline `#tag` / `@today` / `@<weekday>` / `@deadline` / `!1`-`!3` syntax and tab-completion. Saved templates pre-fill it. |
| **Search** | An FTS5-backed bar with the full Calibre-style grammar: boolean, comparison, ranges, date keywords, `is:` predicates, match modifiers, `sort:`, and a `?` operator reference. |
| **Editing flow** | Multi-select with bulk complete/delete, undo on every destructive action, drag to reorder or refile, find-as-you-type sidebar filter, and drop a file or URL onto the window to capture it. |
| **Reminders** | A per-task timestamp fires a system notification that opens the task on click. |
| **Storage** | One SQLite file, a single-writer worker, WAL mode; the UI never blocks on I/O. Built-in one-click backups with optional weekly snapshots. |

## Builder Mode

The same schema and the same rows, with OmniFocus depth layered on:

| | |
|---|---|
| **Defer dates & sequential projects** | Tasks stay hidden until their defer date passes; in a sequential project only the next task is available. |
| **Forecast & Calendar** | A 30-day calendar-axis strip and a paper-calendar month grid, both with drag-to-reschedule. |
| **Review** | The stale-project queue plus a weekly open-task walk, with per-area default review cadences. |
| **Perspectives** | Saved search expressions as sidebar entries, optionally rendered as a drag-drop kanban board. |
| **Inspector pane** | An always-visible editor that autosaves per field: dates, estimates, repeat rule, reminders, deadline-warning window, subtasks, a "Blocked by" dependency picker, and CLOCK time tracking. |
| **Repeating tasks** | RFC 5545 RRULE-driven, honouring all three Org repeater modes; completing a task spawns the next instance with shifted dates and carried tags. |
| **Dependencies** | A task can be blocked by prerequisites; blocked rows carry a pill and `is:blocked` / `is:available` filter on it. |
| **Time tracking** | Start/stop CLOCK sessions distinct from the time estimate, round-tripping to Org's `:LOGBOOK:` drawer. |

Mode flips are pure UI re-renders, verified by an integration test that snapshots the schema and rows before and after the switch.

## Org-mode vault

When you set a vault path, Atrium mirrors task state to `.org` files you can edit in any Org-aware tool. The DB stays canonical and the vault is projected downstream, but the sync is two-way: a save in Emacs flows back into the database within ~200 ms via an `inotify` watcher, and a write from Atrium re-emits the file atomically with a post-write integrity check. The round-trip is data-preserving by contract: unknown constructs are kept verbatim, `:ID:` is the anchor, and edit conflicts are surfaced (the loser is backed up) rather than silently dropped.

`demos/showcase/` is a deliberately rich fixture (three projects, every keyword and cookie and repeater mode, nested subtasks, source blocks, tables, Unicode) for seeing the conversion end to end:

```bash
gsettings set io.github.virinvictus.atrium vault-path ~/Tasks
cargo run -p atrium-cli -- import org demos/showcase/
cargo run -p atrium
```

Edit a task in either Atrium or Emacs, save, and watch the other side pick it up. The full preserved/limits contract is in [`docs/org-roundtrip.md`](docs/org-roundtrip.md).

## Import & export

The plan is to reach the Linux task ecosystem through two interop covenants, Org-mode (primary) and VTODO (the cross-app baseline), rather than per-app importer sprawl. A unified **Import** dialog drives every source with a dry-run preview, and each format also runs from the CLI.

| Format | Direction | Notes |
|---|---|---|
| **Org-mode** | two-way | The primary covenant: vault sync plus one-shot import and a lossless JSON snapshot. |
| **Todoist** | import | CSV from the official export; sections, subtasks, labels, priorities, recurrence. |
| **VTODO / RFC 5545** | import + one-way export | The CalDAV-ecosystem bridge (Endeavour, Errands, Nextcloud Tasks, Planify). Export is a file dump, not a CalDAV client. |
| **Taskwarrior** | import | `task export` JSON, with a configurable user-defined-attribute policy. |
| **todo.txt** | import | One task per line. |

## Headless CLI

Every non-GUI surface stays reachable from the shell. `atrium-cli` exposes the search engine, full task and perspective CRUD, and all import/export, so the data layer can be scripted and tested without the GUI (the same property a future TUI will lean on). Reads open the database read-only, so no CLI invocation can corrupt it.

```bash
atrium-cli list today
atrium-cli search 'tag:work AND is:overdue sort:-due'
atrium-cli --json search 'is:repeating' | jq '.[] | .title'
atrium-cli capture 'Buy milk #errand @today'
atrium-cli add "Draft proposal" --project Work --due friday --estimated 90
atrium-cli edit 42 --tag urgent --due tomorrow
atrium-cli depend 42 --on 17
atrium-cli import todoist export.csv --into Inbox --dry-run
atrium-cli export org ~/Tasks
```

Output is TSV by default (`--json` and `--human` are also available), and the full subcommand set (clock, templates, perspectives, backup, vault config) is in `atrium-cli --help`.

## Architecture

Seven crates, split so every non-GUI surface stays headless and testable from the shell; the GTK binary is just one consumer.

- **`atrium-core`** is the data layer: domain types, the single-writer SQLite worker, the read-only connection pool, and the atomic-write and JSON-snapshot helpers.
- **`atrium-search`** is the Calibre-style search expression language (lex / parse / eval, with a SQL fast-path).
- **`atrium-org`** is the Org-mode projection: parser, emitter, importer, and the vault writer plus `inotify` watcher.
- **`atrium-inline`** is the inline-syntax parser (`#tag` / `@date` / `!N`) shared by every capture surface.
- **`atrium-import`** holds the non-Org import/export formats (Todoist, VTODO, Taskwarrior, todo.txt).
- **`atrium-cli`** is the headless CLI.
- **`atrium`** is the GTK4 / libadwaita binary.

Four decisions are load-bearing. **Mode is a view, not a schema:** the OmniFocus superset exists on day one and a flip never migrates data. **One writer:** a dedicated tokio task owns the writable connection, the UI reads through a pool and never blocks on I/O, and updates arrive as deltas, not reloads. **Local-first:** no network sync or telemetry, ever. **The vault is a projection, not the store:** SQLite is canonical and the Org vault mirrors it downstream. The full architecture and schema are in [`spec.md`](spec.md) §3–§4.

## Stack

- **Rust 2024 Edition**, **GTK 4.16+** / **libadwaita 1.7+**
- **SQLite** via `rusqlite` (`bundled`, `chrono`, `trace`): single-writer worker, WAL mode, FTS5
- **`tokio`** runtime; **`chrono`** dates; **`serde`** / **`serde_json`**; **`anyhow`** / **`thiserror`**; **`tracing`**
- **`rrule`** (RRULE iteration), **`regex`** (`tag:~` modifier), **`uuid`** (`:ID:` round-trip), **`notify`** (vault watcher)
- **Bundled fonts** (SIL OFL): Inter, Source Serif 4, JetBrains Mono, Atkinson Hyperlegible; no host fonts assumed
- **Meson** wrapper over Cargo for Flatpak packaging
- **Memory budget:** < 80 MB idle, < 200 MB active on a 10K-task DB, < 250 ms cold start on 5K tasks

## Building

Atrium targets **GNOME 50+ / GTK 4.16 / libadwaita 1.7**. Fedora 44 build dependencies:

```bash
sudo dnf install gtk4-devel libadwaita-devel sqlite-devel
```

```bash
# Run the app
cargo run -p atrium

# Run the CLI
cargo run -p atrium-cli -- list today

# Ship gate: fmt, clippy, tests, smoke, cold-start sanity
scripts/regression.sh
```

`atrium --debug` opens an in-app debug surface (live memory watch against the perf budget, plus 1K/10K/50K/100K stress-fixture generators). The Meson wrapper at `meson.build` is for Flatpak; native development uses Cargo directly.

## Status

**Feature-complete and heading to 1.0.** Every functional phase has shipped: both modes, Calibre-powered search, two-way Org-mode sync, the kanban and calendar surfaces, every importer, and the productivity essentials (subtasks, dependencies, templates, backups, reminders, onboarding). What remains before the 1.0 tag is packaging, not features: localisation scaffolding and a Flathub submission.

- [`spec.md`](spec.md): the contract (architecture, schema, search grammar, import/export mapping, perf budget).
- [`roadmap.md`](roadmap.md): the phase plan, what shipped and what's next.
- [`patchnotes.md`](patchnotes.md): release notes, newest first.
- [`docs/`](docs/): references including [keymap](docs/keymap.md), [schema](docs/schema.md), [Org round-trip](docs/org-roundtrip.md), [accessibility](docs/accessibility.md), [performance](docs/perf-baseline.md), and [GTD patterns](docs/gtd-patterns.md).

## Influences

- **Org-mode** (Carsten Dominik, Bastien Guerry, et al.): the data discipline the whole project serves. The UUID round-trip, dual-date schema, three repeater semantics, and headline-tag model all come from Org. The Phase 18.5 power-feature selection drew on the public workflows of the Org community (norang, Karl Voit, Jethro Kuan, Sacha Chua, and the Worg survey); the full reading list is in [`roadmap.md`](roadmap.md).
- **Things 3** (Cultured Code): the calm-mode ideal. Six canonical lists, the When/Deadline distinction, the day-band Logbook.
- **OmniFocus** (The Omni Group): the depth-mode ideal. Defer dates, sequential projects, review, perspectives, the Inspector.
- **Calibre** (Kovid Goyal et al.): the search expression language and its forgiving-parser-with-warnings shape.
- **NetNewsWire** (Brent Simmons): the single-writer SQLite worker discipline, by way of its Linux-shaped sibling [Viaduct](https://github.com/VirInvictus/Viaduct).

## License

MIT. See [`LICENSE`](LICENSE).
