<p align="center">
  <img src="logo.svg" alt="Atrium" width="240">
</p>

<p align="center">
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Language-Rust-blue" alt="Language: Rust"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/GNOME-50%2B-4a86cf" alt="GNOME 50+">
  <img src="https://img.shields.io/badge/Simple%20Mode-shipping-2ea44f" alt="Simple Mode: shipping">
  <img src="https://img.shields.io/badge/Builder%20Mode-shipping-2ea44f" alt="Builder Mode: shipping">
  <img src="https://img.shields.io/badge/version-0.5.0-blueviolet" alt="version 0.5.0">
</p>

---

# Atrium

**The native GNOME task manager you grow into, not out of.**

Atrium is, fundamentally, an Org-mode app wearing a Things 3 / OmniFocus disguise — built for users who want Org's data discipline (UUIDs everywhere, plain-text round-trip, deadlines and schedules and contexts as first-class data) but can't bring themselves to live in Emacs to get it. GTD shows up as a building idiom because the author grew up listening to Merlin Mann, but the mission isn't GTD. The mission is: every primitive Org gives you, available in a fast native GUI that doesn't require a config file, a reading list, or a religion.

The dual surface is in service of that. **Simple Mode** for *what am I doing right now* — Things 3 calm, six lists, no defer dates, no review queue. **Builder Mode** for the days the system needs to do the work — Forecast, Review, Perspectives, repeating tasks, sequential projects, the full Inspector pane. Same data, two surfaces, no migration. Switching modes is a UI flip; the schema stays the OmniFocus superset on day one.

Both modes ship at v0.5.0. Phases 0–9 closed Simple Mode at v0.1.0; Phases 10–15 closed Builder Mode at v0.2.0; v0.3.0 was the visual-polish minor; v0.4.0 shipped Phase 15.5 (Calibre-powered search). v0.5.0 closes the Phase 15.5 deferred-list (state predicates, sort, history, popover, fuzzy match), extracts the search engine and a full headless CLI as their own workspace crates (`atrium-search` / `atrium-cli`), and lands Phase 15.75 Slices A + B + C (visual polish + per-area accent + canonical-list colour + Weekly Review seed + Logbook day-grouping).

The v0.6.x line completed Phase 15.75 in full: **Slice D (kanban Perspective renderer + Agenda canonical page)** shipped end-to-end (v0.5.4 → v0.6.5), with drag-drop column reorganisation, an in-GUI renderer-config dialog, and a matching `atrium-cli kanban` / `atrium-cli perspective` write side. The search engine grew **FTS5 bm25 + recency ranking** for bare-text searches (v0.5.2) and a **SQL-translation evaluator** that pushes most expressions to SQLite at query time with an in-memory fallback for the rest (v0.5.3). v0.6.7 reorganised the sidebar so Agenda / Forecast / Review join the top tier alongside the canonical lists. v0.6.10–v0.6.16 was a screenshot-driven cleanup arc (soft accents, state-aware row treatment, Inspector polish, sidebar reorder). v0.6.18 finished the SQL fast-path wiring across every search-running surface. **v0.6.19 revised the roadmap** based on a gap-analysis pass against Errands / Planify / Endeavour / Things 3 / OmniFocus / Taskwarrior / Todoist: the Things 3 import phase retired (macOS-only audience), Org-mode promoted to Phase 16/17 as the must-ship two-way mirror, Todoist promoted to its own Phase 18, and a new Phase 19.5 covers nine productivity essentials (notifications, subtasks UI, iCal calendar feed, preferences window, task dependencies, drag-drop external capture, templates, onboarding, backup). **Phase 16 (Org-mode import + vault writer) is what's next.**

**Author's Note:** I'm a college student in my late thirties with no professional industry experience yet — Atrium is one in a string of native Linux desktop apps I'm building to learn the craft and assemble a portfolio. I came from Things 3 and OmniFocus on macOS / iOS, and Linux has nothing in their lane that isn't an Electron wrapper or a CalDAV form over a webview. Atrium is the answer I wanted to exist. I work on Fedora 44 on a ThinkPad T14s AMD Gen 6; that's the environment it'll be tested against. I welcome contributions but can only honestly support my own setup.

## Why this exists

Three forces converge here.

**Org-mode without Emacs.** Org gives you UUIDs on every node, deadlines and schedules as distinct fields, repeating tasks with three completion semantics (`+` / `++` / `.+`), tags as multi-attach metadata, and full plain-text round-trip. None of those primitives are deep — they're a few hundred lines of contract. The reason most people don't use Org isn't that the model is wrong; it's that the surface is Emacs. Atrium gives you the same primitives in a GTK4 native app, mapped 1:1, with a `:ID:` round-trip story (Phase 17) so anything you do here works in Doom or vim-orgmode or a pile of `.org` files in a git repo.

**Things 3 and OmniFocus, on Linux, done right.** The two apps that taught GTD to a generation fail in opposite ways. Things 3 is calm and beautiful, and so deliberate about what it omits that power users eventually outgrow it — no defer dates, no review queue, no forecast, no sequential projects. You leave because the tool can't keep up with your system. OmniFocus is the opposite — every GTD knob exposed, every facet editable. Its failure mode is *fiddling with fields instead of doing tasks*. Atrium's pitch: a user grows into Builder Mode when their system demands it, and falls back to Simple Mode when the system doesn't, **without abandoning their data or their app**. The schema is the OmniFocus superset on day one. Simple Mode hides the Builder fields; it doesn't lack them.

**Local-first, no exceptions.** SQLite at `$XDG_DATA_HOME/atrium/atrium.db`. WAL mode, single-writer worker, read-only connection pool. No CalDAV client, no cloud sync, no telemetry, no accounts. The Org vault (Phase 17) is filesystem mirroring, not network — your data lives on your machine and stays there unless you choose to move it. VTODO export (Phase 19) is a one-way file dump for handoff; Atrium will never become a CalDAV client.

## Screenshots

<!-- TODO: capture screenshots against a populated demo library. Suggested set
     for the README header (v0.5.0 reality):

       1. Today view in Simple Mode with a populated task list — coloured
          #tag pills, the Area › Project chip on each row, the per-area
          row-left accent stripe (v0.5.0 Slice B2), canonical-list icon
          tinting in the sidebar (v0.5.0 subtle warmth).
       2. Builder Mode with the Inspector pane open — repeat-rule editor,
          defer date, tag picker.
       3. Forecast view with day cards and the Today indicator.
       4. Review queue with a stale project surfacing.
       5. Logbook with day-band grouping (Today / Yesterday / Last 7 Days /
          Older) — v0.5.0 Slice C2.
       6. Search bar with the operator popover open (v0.4.1 `?` button).
       7. atrium-cli running in a terminal — list today, search, info.

     Drop the PNGs into `docs/screenshots/` and reference them from this section.
-->

*Screenshots are a remaining Phase 8 / 9 carryover — see `roadmap.md`.*

## Simple Mode (shipping)

A direct Things 3 analogue for GNOME. Everything below is implemented and exercised by the regression gate (`scripts/regression.sh`):

| | |
|---|---|
| **Lists** | Inbox · Today · Upcoming · Anytime · Someday · Logbook (with v0.5.0 day-band grouping) |
| **Hierarchy** | Areas → Projects → Tasks |
| **Tags** | Multi-tag, orthogonal to areas/projects, with their own pages — inline `#tag` edit syntax. Tag colours wired end-to-end (v0.3.0): six-swatch picker in the editor, coloured dot in the sidebar, coloured `#pill` on every task row. |
| **Areas (v0.5.0)** | Same six-swatch palette tags use. Coloured area paints a 3 px row-left stripe on every task row whose project lives under it — cross-list views (Today, Forecast) show at a glance which area a task came from without you reading the chip. |
| **Dates** | Distinct *When* (scheduled-for) and *Deadline* — the Things 3 detail most clones get wrong. Plus `defer_until` available in Builder Mode. |
| **Quick Entry** | `Ctrl+Alt+Space` → small modal → drops to Inbox without stealing focus; supports `#tag` / `@today` / `@tomorrow` / `@someday` / `@yyyy-mm-dd` / `@deadline 2026-04-15` inline syntax (parser lifted to `atrium_core::quick_entry` at v0.4.5 so the CLI's `capture` reuses it identically). |
| **Search** | `Ctrl+F` opens an FTS5-backed bar with the **Calibre-powered expression grammar** (v0.4.0–v0.5.0). Boolean (`AND` / `OR` / `NOT`, parens), comparison + range on date and numeric fields (`due:>today`, `due:2026-05-01..2026-05-31`), date keywords (`today`, `thisweek`, `5daysago`, `Ndaysout`), state predicates (`is:open`, `is:overdue`, `is:repeating`, plus v0.4.1's canonical-list mirrors `is:today` / `is:inbox` / `is:upcoming` / `is:anytime` / `is:someday`), match modifiers (`tag:work` substring, `tag:=work` exact, `tag:~mystery` regex, `tag:?wrok` fuzzy / Damerau-Levenshtein, `tag:true` existence), `sort:KEY` / `sort:-KEY` modifier with primary→secondary composition, `↑` / `↓` history (20-entry ring buffer), `?` operator-reference popover. Full operator reference in [`spec.md`](spec.md) §4.3. |
| **Area › Project context chip** | Each task row shows its parent project (and area, when set) on cross-list views like Today, Inbox, Upcoming — so you always know where a task lives without leaving the view (v0.2.1). |
| **Find-as-you-type sidebar** | `Ctrl+L` filters the area / project / tag rows live |
| **Multi-select** | `Ctrl+Click` toggle, `Shift+Click` range, `Ctrl+A` select all; bulk Complete + Delete with summary toast |
| **Undo** | `Ctrl+Z` invokes the active toast (toggle-complete + delete recover with their tag attachments intact) |
| **Drag-reorder** | Drag a row to reorder within the list; drag onto a project / Inbox sidebar row to file or unfile |
| **Keyboard-first** | Every common op bindable; mouse optional — full chord scheme in [`docs/keymap.md`](docs/keymap.md) |
| **Accessibility** | Bundled Atkinson Hyperlegible toggle; AT-SPI labels on every interactive widget (sidebar count badges read as "5 open tasks", not bare "5"); libadwaita variables (no hard-coded colors) — see [`docs/accessibility.md`](docs/accessibility.md) |
| **Visual rhythm (v0.5.0)** | Hover-row "lift" cue (subtle accent tint + `@card_shade_color` hairline), AdwClamp-bounded task list (720 px max so rows don't stretch into runway on wide windows), Source Serif 4 italic on the Inspector Notes textview, canonical-list icon tinting (Inbox blue, Today amber, Upcoming green, Someday purple, Logbook faded purple). |
| **Storage** | One SQLite file at `$XDG_DATA_HOME/atrium/atrium.db`; single-writer worker thread; WAL mode; UI never blocks on I/O |
| **Local-first** | No network, no telemetry, no accounts, no CalDAV. Optional Org-mode vault projection lands in Phase 17 / 17.5 |
| **Debug harness** | `atrium --debug` opens *Debug → Memory Watch* for live VmRSS / VmHWM / VmData against the §8 perf budget; fixture generators (1K / 10K / 50K / 100K) for stress-testing |

## Builder Mode (shipping)

Same schema. Same data. Adds:

| | |
|---|---|
| **Defer dates** | Tasks invisible in Today/Anytime until their `defer_until` passes (Phase 11) |
| **Sequential projects** | Only the next incomplete task is "available" — the rest dim (Phase 11) |
| **Forecast** | Calendar-axis layout of the next 30 days; drag to reschedule between days (Phase 12) |
| **Review** | Projects with stale `last_reviewed_at` surface in a queue, oldest first; per-card *Mark Reviewed* button (Phase 13) |
| **Perspectives** | Saved filter expressions as first-class sidebar entries; *Save Search as Perspective…* in the primary menu (Phase 14, v0.1.17). v0.6.0 will add `renderer = 'board'` so a Perspective can render as a kanban; the schema columns shipped at v0.5.0. |
| **Weekly Review (v0.5.0)** | A seeded Perspective on first install — filter is `is:overdue OR scheduled:thisweek OR (is:deadline AND due:nextweek) OR (is:deferred AND defer:<=today)`. Closes the spec §4.2 weekly-review ritual. Rename, retune, or delete it freely. See [`docs/gtd-patterns.md`](docs/gtd-patterns.md) for the workflow. |
| **Inspector pane** | Always-visible right-side `AdwOverlaySplitView` exposing every Builder field, autosaves on focus-out / Enter (Phase 10) |
| **Repeating tasks** | RFC 5545 RRULE-driven via the `rrule` crate; respects all three Org repeater modes — `+1w` (Basic), `++1w` (Cumulative — the default), `.+1w` (Next-from-completion). Spawns the next instance on completion with shifted dates and carried tags (Phase 15, v0.2.0) |
| **Project › Area breadcrumb** | Header bar shows `Area › Project` when viewing a project under an area, anchoring users in their hierarchy (v0.3.0) |

Mode flips are pure UI re-renders. The schema is the superset; Builder Mode just exposes the columns Simple Mode keeps hidden. Verified by an integration test that snapshots schema + rows before and after a switch (`tests/mode_flip_snapshot.rs`).

## Headless CLI (`atrium-cli`)

v0.5.0 ships a fourth workspace crate, `atrium-cli`, that exposes the search engine and full task CRUD from the shell. Architectural commitment recorded in `CLAUDE.md`: every non-GUI surface stays CLI-testable. The 2.0-era TUI (`atrium-tui`) will be the same shape — another headless consumer of `atrium-core` + `atrium-search`.

| Subcommand | Effect |
|---|---|
| `atrium-cli search EXPR` | Run a search expression (full grammar, sort modifiers honoured) and print matches |
| `atrium-cli list NAME` | Print a canonical list. NAME ∈ task lists (`inbox`, `today`, `upcoming`, `anytime`, `someday`, `logbook`, `all`) or metadata lists (`areas`, `projects`, `tags`, `perspectives`) |
| `atrium-cli info ID` | Full details of a single task |
| `atrium-cli add TITLE [FLAGS]` | Create a task. Flags: `--note`, `--project NAME`, `--tag NAME` (repeatable), `--scheduled DATE`, `--due DATE`, `--defer DATE`, `--estimated MIN` |
| `atrium-cli capture LINE` | Quick-Entry-style one-shot. Parses `#tag` / `@today` / `@deadline yyyy-mm-dd` syntax via the same parser the GUI's bottom-of-list entry uses |
| `atrium-cli edit ID [FLAGS]` | Diff-based modify. Same flag vocabulary as `add`; pass `none` to clear a field. `--tag X` / `--remove-tag X` / `--clear-tags` for tag editing |
| `atrium-cli complete ID` | Toggle completion (same semantics as the GUI checkbox; calling twice un-completes). Aliases: `done`, `toggle` |
| `atrium-cli delete ID` | Delete a task. Prints the row before deletion so the action is auditable in pipelines. Alias: `rm` |

Output formats (mutually exclusive global flags):

- `--tsv` (default) — `id\tstatus\ttitle\tscheduled\tdeadline\tproject\tarea\ttags`. Header row first; `cut`/`grep`-friendly.
- `--json` — serde_json array (or single object for `info`); `jq`-friendly.
- `--human` — pretty columns with truncation; for terminal viewing.

Database resolution: `--db PATH` → `ATRIUM_DB_PATH` env → XDG default. Reads open `SQLITE_OPEN_READ_ONLY` so a buggy query attempting an INSERT errors at the engine — no CLI invocation can corrupt the user's database through a read path.

```bash
atrium-cli list today
atrium-cli search 'tag:work AND is:overdue sort:-due'
atrium-cli --json search 'is:repeating' | jq '.[] | .title'
atrium-cli info 42 --human
atrium-cli capture 'Buy milk #errand @today'
atrium-cli edit 42 --tag urgent --due tomorrow
atrium-cli complete 42
```

## Imports and exports (toward 1.0)

Direct importers ship for the apps Linux users *actually* migrate from. The list trimmed in v0.6.19 — Things 3 retired (macOS export-only; vanishingly small GNOME audience), Org and Todoist promoted to first-class slots:

- **Org-mode** (two-way `.org` interop, with UUID round-trip via `:ID:`) — Phase 16 (one-shot import + DB→vault writer), Phase 17 (full two-way `inotify` sync). Atrium's primary covenant; the agenda-parity test pins Atrium's Agenda canonical page against stock `org-agenda` over the same vault.
- **Todoist** (CSV via the official export tool) — Phase 18
- **VTODO / RFC 5545** (`.ics`) — covers Endeavour, Errands, Apple Reminders, Nextcloud Tasks, Planify — Phase 19
- **Taskwarrior** (`task export` JSON) — Phase 19
- **todo.txt** and **TaskPaper** (plain text) — Phase 19
- **OmniFocus** (`.ofocus` bundle, macOS-export-only) — Phase 19 long-tail; small audience but the GTD-lineage migration path stays open

VTODO export is one-way — Atrium does not become a CalDAV client. The plan is to reach the Linux task ecosystem through two interop covenants — Org-mode (primary) and VTODO (cross-app baseline) — rather than per-app importer sprawl.

### Acknowledgments

The v0.6.19 roadmap revision (retired Things 3 import; promoted Org-mode + Todoist; added Phase 19.5 productivity essentials) drew on a feature-survey pass against the apps below. No code was copied — the analysis read public README/docs/feature-pages and identified gaps relative to Atrium's existing roadmap. Each Phase 19.5 item names its source in `roadmap.md`.

- [Errands](https://github.com/mrvladus/Errands) — GTK4 / Python; subtasks, drag-drop, accent colors, CalDAV/Nextcloud sync.
- [Planify](https://github.com/alainm23/planify) — GTK4 / Vala; Todoist + Nextcloud sync, multi-reminder-per-task, attachments.
- [Endeavour](https://gitlab.gnome.org/World/Endeavour) — GTK4 / C; GNOME Online Accounts integration.
- [Things 3](https://culturedcode.com/things/features/) — macOS native; the calm-six-lists model Atrium's Simple Mode still echoes.
- [OmniFocus 4](https://support.omnigroup.com/documentation/omnifocus/) — macOS native; the GTD-knob model Atrium's Builder Mode still echoes.
- [Taskwarrior](https://taskwarrior.org/docs/) — CLI; the dependency-and-urgency model Phase 19.5 borrows from.
- [Todoist](https://todoist.com/features) — cross-platform; the natural-language and template patterns Phase 18 will need.
- [Super Productivity](https://super-productivity.com/blog/open-source-productivity-apps-comparison/) — the open-source comparison piece that anchored the survey.

## Status

**Phases 0–15 closed at v0.2.0 (Builder Mode milestone). v0.3.0 visual polish minor shipped. v0.4.0 shipped Phase 15.5. v0.5.0 closes Phase 15.5's deferred list, extracts the four-crate workspace, and lands Phase 15.75 Slices A + B + C.** The journey to v1.0 lives in [`roadmap.md`](roadmap.md), broken into 20 numbered phases plus three sub-phases (12.5, 15.5, 17.5):

- **Phases 0–9** — Simple Mode → tagged as **v0.1.0**
- **Phases 10–15** — Builder Mode → tagged as **v0.2.0**
- **Phase 15.5** — Calibre-powered search → tagged as **v0.4.0** (deferred-list closed at v0.5.0)
- **Phase 15.75 (partial)** — visual polish + per-area accent + atrium-search/atrium-cli extraction + GTD audit → tagged as **v0.5.0**
- **Phases 16–19** — imports and exports across the Linux productivity-app ecosystem
- **Phase 20** — l10n, accessibility round 2, capture daemon (`atriumd`), Flathub → **v1.0**
- **Beyond 1.0** — `atrium-tui` (full headless TUI sharing the same `atrium-core` + `atrium-search` plumbing the CLI uses today) → **v2.0**

[`patchnotes.md`](patchnotes.md) tracks every release entry, newest at top.

## Architecture (in one paragraph)

Four workspace crates: **`atrium-core`** is the headless data layer (domain types, single-writer SQLite worker, paths, errors, repeat-rule wrapper). **`atrium-search`** is the Calibre-style search expression language (lex / parse / ast / eval; depends on `atrium-core` for `Task` and `ScheduledFor`). **`atrium-cli`** is the headless CLI (depends on both above). **`atrium`** is the GTK4 binary (depends on all three). The data layer uses SQLite in WAL mode with the schema modeled as the OmniFocus superset; a dedicated `tokio` worker task owns the writable connection while the UI reads through a separate read-only connection pool. Updates arrive as `TaskChanges` and `LibraryChanges` deltas via a `glib::MainContext` channel, never as full reloads. Mode (Simple / Builder) is a per-app GSettings flag — flipping it never touches the DB. An optional Org vault (default `~/Tasks/`) projects task state to `.org` files for editing in Emacs / Doom / any Org tool — SQLite stays canonical, the vault is a projection. A `--debug` CLI flag opens an in-app debug surface for stress fixtures and live memory watch. See [`spec.md`](spec.md) §3 for the full architecture and §4 for the schema.

## Stack

- **Rust 2024 Edition**
- **GTK 4.16+** / **libadwaita 1.7+**
- **SQLite** via `rusqlite` (`bundled`, `chrono`, `trace` features) — single-writer worker, WAL mode, FTS5 for search
- **`tokio`** runtime; **`chrono`** for dates; **`serde`** / **`serde_json`** for export formats; **`anyhow`** / **`thiserror`** for errors; **`tracing`** for diagnostics
- **`rrule`** for RFC 5545 RRULE iteration; **`regex`** (in `atrium-search` only) for the `tag:~regex` modifier; **`uuid`** for `:ID:` round-trip
- **Meson** wrapper over Cargo so Flatpak packaging is straightforward
- **Bundled fonts** (SIL OFL): Inter Variable, Source Serif 4, JetBrains Mono, Atkinson Hyperlegible — installed via fontconfig at first run; no host fonts assumed
- **Memory budget:** < 80 MB idle, < 200 MB active on a 10K-task DB, < 250 ms cold start on 5K tasks, < 50 ms Quick Entry latency. Baselines captured in [`docs/perf-baseline.md`](docs/perf-baseline.md).

No third-party crate gets added without per-phase sign-off — see the dependency-check items in `roadmap.md`. The full v0.1 dependency set is locked.

## Build requirements

- **Rust toolchain** — Rust 2024 Edition (stable channel works as of late 2025; check `Cargo.toml` if a build fails on an older toolchain).
- **GTK 4.16+** development headers — `gtk4-devel` on Fedora, `libgtk-4-dev` on Debian/Ubuntu.
- **libadwaita 1.7+** development headers — `libadwaita-devel` (Fedora) / `libadwaita-1-dev` (Debian/Ubuntu).
- **SQLite 3** — bundled via `rusqlite`'s `bundled` feature, but the system libsqlite3 must be present for some build paths. `sqlite-devel` / `libsqlite3-dev`.
- **glib-compile-schemas** for the GSettings schema — installed with GTK on most distros.
- **Meson 0.59+** (optional, for Flatpak packaging) — `meson` package on Fedora / Debian.
- **`fc-cache` from fontconfig** — used at first run to register the bundled fonts; pre-installed on every desktop Linux distribution.

GNOME 50+ is the target runtime. Earlier libadwaita / GTK versions may work but aren't tested.

## Build and run

```bash
# Native (development).
cargo run -p atrium

# Native (release).
cargo build --release
target/release/atrium

# Headless CLI (development).
cargo run -p atrium-cli -- list today
cargo run --release -p atrium-cli -- search 'is:today AND tag:work sort:-due'

# Run the regression gate (fmt + clippy + tests + 1K-fixture smoke + cold-start sanity).
scripts/regression.sh

# Generate stress fixtures (writes to $XDG_DATA_HOME/atrium/atrium.db).
atrium --fixture small      # 1,000 tasks
atrium --fixture medium     # 10,000 tasks
atrium --fixture large      # 50,000 tasks
atrium --fixture stress     # 100,000 tasks

# Open the debug pane (memory watch + fixture menu in the primary menu).
atrium --debug

# Flatpak (developer build).
flatpak-builder --user --install --force-clean build-dir \
  data/io.github.virinvictus.atrium.yml
flatpak run io.github.virinvictus.atrium
```

## Testing and debugging

```bash
# Full workspace test suite — 375 tests at v0.5.0.
cargo test --workspace

# Single test (any crate).
cargo test -p atrium-core search::tests::eval_due_today_bare_keyword

# Lint with warnings-as-errors (CI gate).
cargo clippy --workspace --all-targets -- -D warnings

# Format check.
cargo fmt --all --check

# Mode-flip snapshot — verifies the Simple↔Builder switch never
# touches schema or rows. Independent integration test.
cargo test -p atrium-core --test mode_flip_snapshot

# CLI-driven verification against your real database (read-only,
# can't corrupt anything):
atrium-cli list today
atrium-cli search 'tag:work AND is:overdue'
atrium-cli list tags --json | jq '.[] | select(.color != null)'

# CLI against a test database (won't touch your real one):
ATRIUM_DB_PATH=/tmp/test.db atrium --fixture small
ATRIUM_DB_PATH=/tmp/test.db atrium-cli list today
```

The debug surface (`atrium --debug`):

- **Memory Watch** — live VmRSS / VmHWM / VmData sampling against the §8 perf budget.
- **Fixture menu** — re-roll the database with a stress generator without restarting.
- **SQL trace** — every SQLite statement logged via `tracing` at TRACE level. `RUST_LOG=trace` (or scoped `RUST_LOG=atrium_core::db=trace`) reveals each statement plus its elapsed wall time.

## Where things live

| File / dir | What it is |
|---|---|
| [`spec.md`](spec.md) | The contract. Architecture, schema, UI deltas, search expression language, import/export mapping, perf budget. |
| [`roadmap.md`](roadmap.md) | 20-phase plan from empty repo to 1.0 (plus Phase 15.75, 17.5 sub-phases). |
| [`patchnotes.md`](patchnotes.md) | Release notes, newest at top. |
| `CLAUDE.md` | Per-project guidance for AI-assisted development. |
| [`docs/keymap.md`](docs/keymap.md) | Full keyboard shortcut table. |
| [`docs/accessibility.md`](docs/accessibility.md) | AT-SPI label audit + accessibility conventions. |
| [`docs/perf-baseline.md`](docs/perf-baseline.md) | §8 budget vs measured RSS / startup numbers. |
| [`docs/regression.md`](docs/regression.md) | What `scripts/regression.sh` covers and when to run it. |
| [`docs/gtd-patterns.md`](docs/gtd-patterns.md) | GTD idioms documented as Atrium-flavored conventions (the `#waiting` tag, weekly-review workflow, contexts-as-tags, etc.). |
| `data/` | `.ui` XML, icons, GSettings schema, AppStream metainfo, Flatpak manifest, bundled fonts. |
| `atrium-core/` | Headless library — schema, worker, fixtures, paths, repeat rules, Quick Entry parser. |
| `atrium-search/` | Calibre-powered search expression language (lex / parse / ast / eval). v0.4.2 extracted. |
| `atrium-cli/` | Headless CLI binary. v0.4.3 onward; full task CRUD + metadata reads. |
| `atrium/` | GTK4 binary. |
| `scripts/` | Developer scripts (regression gate, etc.). |

## Influences and acknowledgements

- **Org-mode** (Carsten Dominik, Bastien Guerry, et al.) — the data discipline this whole project is in service of. Atrium's UUID `:ID:` round-trip, dual-date-field schema, three repeater semantics, and headline-tag model all come from Org.
- **Things 3** (Cultured Code) — the calm-mode ideal Atrium opens with. The six canonical lists, the When/Deadline distinction, the paper-list rhythm, the day-band Logbook are all Things-shaped.
- **OmniFocus** (The Omni Group) — the depth-mode ideal Atrium grows into. Defer dates, sequential projects, the review queue, perspectives, the always-visible Inspector pane.
- **Calibre** (Kovid Goyal et al.) — the search expression language. Match modifiers, date keywords, state predicates, the forgiving-parser-with-warnings shape.
- **Merlin Mann** — for an embarrassing share of the GTD + craft-of-software mental furniture this project rests on. *43 Folders*, *Back to Work*, the throwaway tossed-off *Reconcilable Differences* line about "the dignity of the medium."
- **NetNewsWire** (Brent Simmons) — the single-writer SQLite worker discipline lifted from Viaduct's Atrium-shaped sibling.

## License

MIT. See [`LICENSE`](LICENSE).
