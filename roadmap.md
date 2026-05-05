# Atrium — Roadmap

What's done, what's next, what's deferred. Sequenced for a clean Simple Mode v0.1, a Builder Mode v0.2 expansion, and a 1.0 with broad import/export across the Linux task-app ecosystem. Updated as of v0.0.0 (pre-implementation).

---

## North Star

Twenty phases mapping the journey from empty repo to 1.0.

- **Phases 0–9:** Simple Mode → **v0.1**
- **Phases 10–15:** Builder Mode → **v0.2**
- **Phases 16–19:** Import/export across Things 3, OmniFocus, Org-mode, Taskwarrior, Todoist, VTODO, todo.txt, TaskPaper
- **Phase 20:** Polish, localisation, Flathub → **v1.0**

Each phase ends with a `heaptrack` checkpoint against the §8 budget. Every phase that adds a third-party crate calls it out — *no third-party deps without prior sign-off*.

---

## Phase 0: Scaffolding
*Repo bones. Standard project layout, Rust toolchain, CI baseline. No domain code.*

- [ ] **Cargo skeleton:** `Cargo.toml` with `gtk4`, `libadwaita`, `tokio`, `rusqlite` (`bundled`, `chrono` features), `serde`, `serde_json`, `chrono`, `anyhow`, `thiserror`, `tracing`, `tracing-subscriber`. *(matches Viaduct's choices)*
- [ ] **Module layout:** `src/{db,domain,ui,quickentry,main.rs}` with empty modules.
- [ ] **Application identifier:** `io.github.virinvictus.atrium` for desktop entry, GSettings, AppStream.
- [ ] **License + headers:** MIT `LICENSE`, SPDX line in every source file.
- [ ] **Project metadata:** `VERSION`, `README.md` (refreshed tagline reflecting dual-mode), `roadmap.md` (this file), `spec.md`, `patchnotes.md`, `CLAUDE.md`, `logo.svg` placeholder.
- [ ] **Meson wrapper:** mirrors Viaduct's pattern so Flatpak packaging is straightforward later.
- [ ] **XDG path helpers:** `$XDG_DATA_HOME/atrium/` for the DB, `$XDG_CACHE_HOME/atrium/` for caches.
- [ ] **Error type hierarchy:** `thiserror`-driven `DbError`, `DomainError`, `UiError`, `AtriumError`.
- [ ] **`tracing-subscriber`** with env-filter (default: `info,atrium=debug`).
- [ ] **GitHub Actions CI:** `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` on Linux.

## Phase 1: Schema Design — The OmniFocus Superset
*The schema decision that makes Mode-as-View work. Get it right once.*

- [ ] **Initial migration:** `0001_initial.sql` covering all tables in spec §4: `task`, `project`, `area`, `tag`, `task_tag`, `heading`.
- [ ] **Builder-superset columns:** every Builder-only field exists at v0.1 with sane NULL/false defaults.
- [ ] **WAL mode + pragmas:** `synchronous=NORMAL`, `temp_store=MEMORY`, `mmap_size=268435456`.
- [ ] **`user_version` PRAGMA** drives migration state; `migrate.rs` runs them on startup.
- [ ] **FTS5 virtual table** `task_fts` indexing `title` + `note`, with triggers for sync.
- [ ] **`modified_at` triggers** auto-update on `task` and `project` UPDATE.
- [ ] **Indexes:** `(project_id, completed_at)`, `(scheduled_for)`, `(defer_until)`, `(deadline)`, `(completed_at)` for fast list queries.
- [ ] **Schema documentation:** `docs/schema.md` with ER diagram and rationale per column.

## Phase 2: Data Layer — Single-Writer Worker
*Viaduct's pattern, ported. The UI thread never blocks on the DB.*

- [ ] **Domain types:** Rust structs for `Task`, `Project`, `Area`, `Tag`, `Heading`, with `serde` derives.
- [ ] **`TaskChanges` batch type:** `{ created, updated, deleted, status_changed }` — the unit of UI delivery.
- [ ] **Single-writer task:** owns the writable `rusqlite::Connection`; receives commands via `mpsc`.
- [ ] **Read-only connection pool:** for SELECT queries from the UI side; the writer never serves reads.
- [ ] **Command/Query split:** `enum Command { CreateTask, CompleteTask, … }` and read fns return `Result<Vec<Row>>` directly.
- [ ] **`glib::MainContext::channel`** wiring: `TaskChanges` reach the UI as deltas, not refreshes.
- [ ] **Coalescer:** suppresses UI notification storms during batch ops (imports, bulk completes).
- [ ] **In-memory test harness:** every Phase 2 op tested against `:memory:` SQLite.

## Phase 3: Application Shell
*Window opens, settings persist. No tasks yet.*

- [ ] **`adw::Application` skeleton** with `io.github.virinvictus.atrium` ID.
- [ ] **`AdwApplicationWindow` + `AdwNavigationSplitView`** root tree from `data/window.ui`.
- [ ] **GSettings schema:** mode (Simple/Builder), window state, sidebar width, Quick Entry shortcut.
- [ ] **Mode-switch plumbing:** GAction `app.mode` writes to GSettings; the rest of the app reads it.
- [ ] **Empty Simple Mode shell:** sidebar with placeholder list rows, empty content pane.
- [ ] **Light/dark follow-system** via `AdwStyleManager`.
- [ ] **About dialog** with version, license, repo link.
- [ ] **First-run state:** opens to Today, empty.

## Phase 4: Simple Mode — Inbox & Today
*The two views every Things user opens fifty times a day. Get them perfect first.*

- [ ] **Inbox view:** `GtkListView` + `GtkSignalListItemFactory` over a `gio::ListModel` backed by Phase 2's read API.
- [ ] **Today view:** SELECT per spec §4.2; same cell factory, different model.
- [ ] **Task row widget:** completion circle (animated check), title, optional date pill, optional tag pills.
- [ ] **Inline create:** `Ctrl+N` or `+` button → new row in current list, focus title field.
- [ ] **Inline edit:** click title to edit, Esc cancels, Enter commits.
- [ ] **Completion toggle:** click circle → fade row → moves to Logbook on next refresh.
- [ ] **Drag-to-reorder** within Inbox (Today is auto-sorted by date).
- [ ] **Empty states:** illustration + helper text per list, matching `AdwStatusPage`.

## Phase 5: Simple Mode — Areas, Projects, Anytime, Someday, Logbook
*Hierarchy and the rest of the lists. Now it's a real Things 3 analogue.*

- [ ] **Sidebar `TreeListModel`:** Lists section + Areas (with nested Projects) + Tags section.
- [ ] **Area CRUD:** create/rename/delete via right-click and keyboard shortcuts.
- [ ] **Project CRUD:** projects in an area or unfiled; project page shows tasks + headings.
- [ ] **Anytime / Someday / Upcoming / Logbook:** each is a SELECT-backed list; same cell factory.
- [ ] **Task move:** drag from one list to another; commits a `project_id` update.
- [ ] **Project completion:** marking a project complete archives it (`archived_at`) and moves it to Logbook.
- [ ] **Sidebar count badges:** integer badges on Inbox and Today; project pages show open-task counts.

## Phase 6: Simple Mode — Tags & Quick Entry
*Capture without context-switch. The feature that makes a task app stick.*

- [ ] **Tag CRUD:** `Tags` section in sidebar; create on first use, rename, delete (with confirmation if used).
- [ ] **Multi-tag editor** in the task row: pill UI with autocomplete.
- [ ] **Tag pages:** clicking a tag opens a list of all open tasks bearing it.
- [ ] **Inline tag syntax in capture:** typing `#errand` in title creates the tag and attaches it.
- [ ] **Inline date syntax:** `@today`, `@tomorrow`, `@yyyy-mm-dd`, `@deadline yyyy-mm-dd` parsed; `chrono` handles dates.
- [ ] **Quick Entry shortcut:** GTK accelerator (default `Ctrl+Alt+Space`) opens a small `AdwWindow` modal.
- [ ] **Quick Entry behaviour:** drops to Inbox; closes on Enter; never steals focus from other apps.
- [ ] **Quick Entry cold-start:** if Atrium isn't running, launches minimised and posts the task. (True zero-launch capture deferred to v0.2 daemon.)

## Phase 7: Simple Mode — Search, Filtering, Keyboard Map
*Power-user surface. Mouse-optional.*

- [ ] **FTS5-backed search bar:** `Ctrl+F` opens; debounce 200 ms; ranks by recency × relevance.
- [ ] **Filter expressions:** every list supports `tag:foo`, `area:bar`, `due:today`, `overdue:`.
- [ ] **Full keyboard map:** all common ops bindable; default chord scheme published in `docs/keymap.md`.
- [ ] **Multi-select:** Shift-click + Ctrl-click; bulk complete / bulk-tag / bulk-move.
- [ ] **Undo:** every destructive op (complete, delete, move) is undoable for ~30s via `AdwToast`.
- [ ] **Find-as-you-type** in sidebar (jump to area/project/tag).

## Phase 8: Simple Mode — Polish, Typography, Packaging
*Visual identity. Make it feel inevitable, not improvised.*

- [ ] **Typography pass:** bundled type system (Inter Variable for UI, IBM Plex Mono for any monospace surfaces) registered via `pango::FontMap::add_font_file`.
- [ ] **Logo / icon:** scalable SVG following GNOME icon guidelines; install to hicolor.
- [ ] **Desktop entry:** `io.github.virinvictus.atrium.desktop` (validated by `desktop-file-validate`).
- [ ] **AppStream metainfo:** `io.github.virinvictus.atrium.metainfo.xml` with screenshots, OARS rating.
- [ ] **Flatpak manifest:** `data/io.github.virinvictus.atrium.yml` against GNOME 50 runtime.
- [ ] **Animations:** task completion check, list transitions, modal fade — match libadwaita timings.
- [ ] **Memory profile:** `heaptrack` baseline against §8 targets.
- [ ] **Accessibility audit:** keyboard end-to-end, screen-reader labels, contrast on tag pills.

## Phase 9: Simple Mode v0.1 Release
*Ship.*

- [ ] **Full regression on a 1,000-task seed DB.**
- [ ] **Patchnotes + version bump:** `VERSION`, `Cargo.toml`, `metainfo.xml` release entry.
- [ ] **Tag `v0.1.0`**, publish Flatpak.
- [ ] **README finalised** with screenshots and the "Simple Mode now / Builder Mode next" framing.
- [ ] **First public release announcement** on `VirInvictus.github.io`.

---

## Phase 10: Builder Mode — UI Shell
*The mode switch becomes real. Inspector pane lands. No new logic — just exposure.*

- [ ] **Mode toggle in primary menu:** `Settings → Mode → [Simple, Builder]`.
- [ ] **Inspector pane:** `AdwOverlaySplitView` adds a third pane on the right with the full task editor (all Builder fields).
- [ ] **Builder-only sidebar entries:** `Forecast`, `Review`, `Perspectives` appear when mode = Builder.
- [ ] **Project page extras:** Sequential toggle, Review interval picker (visible only in Builder).
- [ ] **No data changes:** flipping mode never touches the DB. Verified by an integration test that snapshots schema + rows before and after a switch.

## Phase 11: Builder Mode — Defer Dates & Sequential Projects
*OmniFocus mechanics. Tasks with future defer dates become "available" later.*

- [ ] **`defer_until` editor** in Inspector.
- [ ] **List filter logic:** `Today` and `Anytime` exclude deferred tasks until their defer date.
- [ ] **Sequential project rendering:** in a sequential project, only the first incomplete task renders as "available"; later tasks render dimmed/disabled.
- [ ] **"Available" task count:** sidebar projects show available-task count instead of open-task count when mode = Builder.

## Phase 12: Builder Mode — Forecast View
*OmniFocus's killer view: a calendar-axis layout of next ~30 days.*

- [ ] **Forecast page:** vertical day-blocks, each showing scheduled / deadline / deferred tasks for that day.
- [ ] **Drag-to-reschedule:** drag a task to a different day → updates `scheduled_for`.
- [ ] **Today indicator and overdue surfacing.**
- [ ] **Compact / expanded toggles** for dense schedules.

## Phase 13: Builder Mode — Review Queue
*The GTD discipline. Surface stale projects so they don't rot.*

- [ ] **`review_interval_days` per project**, editable in project page.
- [ ] **Review perspective:** projects with `last_reviewed_at + interval ≤ today` surface here, oldest first.
- [ ] **"Mark Reviewed" action** updates `last_reviewed_at = now()`.
- [ ] **Per-area review schedules:** an area can default an interval new projects inherit.

## Phase 14: Builder Mode — Perspectives (Saved Views)
*OmniFocus Perspectives. Filter expressions become first-class objects.*

- [ ] **Perspective domain type:** `name`, `filter_expression` (subset of Phase 7's filter language), `sort`, `grouping`.
- [ ] **CRUD for perspectives:** create from current filter state, rename, delete.
- [ ] **Perspectives sidebar section** (Builder-only) with custom icon per perspective.
- [ ] **Export perspective definition** to JSON for sharing.

## Phase 15: Repeating Tasks
*The rabbit hole, addressed properly. RFC 5545 RRULE-based.*

- [ ] **`repeat_rule` editor:** UI for daily / weekly / monthly / yearly + custom RRULE.
- [ ] **Regenerate-on-complete logic:** when a repeating task is completed, the worker spawns the next instance.
- [ ] **Defer-vs-due semantics:** repeats can move SCHEDULED, DEADLINE, both, or neither — user choice (matches Org-mode's `+`, `++`, `.+` cookies).
- [ ] **Edge cases:** end-of-month rules, skipped occurrences, "after N completions" termination.
- [ ] **Tests:** RRULE round-trip + regenerate logic over a synthetic 1-year horizon.
- [ ] **Dependency check:** evaluate `rrule` crate vs hand-rolled subset; flag for sign-off if added.

---

## Phase 16: Things 3 Import
*Brandon's source app. JSON via Things' URL scheme on macOS — exported externally, imported here.*

- [ ] **Format research:** confirm current Things 3 JSON shape (URL scheme `things:///add-json`, AppleScript export).
- [ ] **Importer module:** `src/import/things3.rs` — parser, mapper, dry-run mode.
- [ ] **Mapping table:** Areas → areas, Projects → projects, Headings → headings, To-Dos → tasks, Tags → tags, "When" → `scheduled_for`, Deadline → `deadline`, Notes → `note`.
- [ ] **Conflict handling:** existing UUID match → update; no match → create.
- [ ] **Post-import report:** counts, lossy fields surfaced, file-by-file log.
- [ ] **Test fixtures:** sample exports in `tests/fixtures/things3/`.

## Phase 17: Org-Mode Import & Export
*Two-way `.org` interop. The plain-text covenant.*

- [ ] **Org parser research:** evaluate the `orgize` crate vs hand-rolled subset; flag for sign-off if `orgize` is added.
- [ ] **Importer:** `src/import/orgmode.rs` accepting one file or a directory tree (each file = an area, configurable).
- [ ] **Coverage:** headlines, TODO/DONE/CANCELLED keywords, SCHEDULED/DEADLINE/CLOSED cookies, headline tags, `:PROPERTIES:` drawers, body text.
- [ ] **Mapping per spec §7.3:** UUID round-trip via `:ID:` property.
- [ ] **Exporter:** `src/export/orgmode.rs` emitting one `.org` per area or one combined file.
- [ ] **Round-trip test fixture:** import → export → diff = empty (modulo whitespace and section ordering).
- [ ] **Atrium native JSON export ships in this phase too** — the universal lossless backup format.

## Phase 18: OmniFocus Import
*The OF half of Atrium's bloodline. `.ofocus` is a bundle of XML files with a transaction log.*

- [ ] **`.ofocus` format research:** archive structure, transaction folding, content vs metadata files.
- [ ] **Importer:** `src/import/omnifocus.rs` — handles the bundle as a directory.
- [ ] **Mapping:** Folders → areas, Projects → projects with `sequential` flag, Actions → tasks, Contexts/Tags → tags, Defer → `defer_until`, Due → `deadline`, Estimated → `estimated_minutes`, Repeat → `repeat_rule`.
- [ ] **Perspective definitions** imported as Atrium Perspectives where the filter language allows.
- [ ] **Test fixture:** sanitised sample `.ofocus` bundle in `tests/fixtures/omnifocus/`.

## Phase 19: Taskwarrior, Todoist, VTODO, todo.txt, TaskPaper
*Round out the import surface. One pass per source, sharing parser scaffolding. VTODO export ships here too.*

- [ ] **Taskwarrior:** `task export` JSON; UDA fields → tags or notes per user choice.
- [ ] **Todoist:** CSV via Todoist's official export tool; project hierarchy mapping; comments → notes.
- [ ] **VTODO (RFC 5545) import:** `.ics` parser; cover the standard properties; covers Endeavour, Errands, Apple Reminders, Nextcloud Tasks, Planify (CalDAV-side).
- [ ] **VTODO export:** one-way `.ics` for hand-off to CalDAV apps. *Atrium does not become a CalDAV client.*
- [ ] **todo.txt:** plain text with `(A)` priority, `+project`, `@context`, `due:` extension.
- [ ] **TaskPaper:** plain text headlines, `@tags`, `@done` metadata.
- [ ] **Unified import dialog:** picks source, runs parser in worker, shows pre-import report, commits in batch (Phase 2 coalescer earns its keep).
- [ ] **Dependency checks:** evaluate `ical` / `rustical` crates for VTODO; flag for sign-off if added.

## Phase 20: 1.0 — Polish, Localisation, Flathub
*The release that says Atrium is finished, not just shipped.*

- [ ] **Capture daemon (`atriumd`):** small binary running under user systemd; handles global Quick Entry shortcut even when the main app is closed; IPCs to it via D-Bus or local socket.
- [ ] **Accessibility audit (round 2)** with assistive-tech tooling.
- [ ] **Localisation scaffolding:** `gettext-rs`, `data/po/`, English template, two pilot translations.
- [ ] **Documentation site:** `mdbook` from `docs/`; covers user manual, keymap, import guide, schema reference, RRULE supported subset.
- [ ] **Final icon and brand pass.**
- [ ] **AppStream screenshots refresh** — Simple and Builder both featured.
- [ ] **Flathub submission.**
- [ ] **Performance regression suite:** scripted load against a 50K-task DB; results published in `docs/perf.md`.
- [ ] **`v1.0.0` tag, release notes, retrospective.**
