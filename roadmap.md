# Atrium — Roadmap

What's done, what's next, what's deferred. Sequenced for a clean Simple Mode v0.1, a Builder Mode v0.2 expansion, and a 1.0 with broad import/export across the Linux task-app ecosystem. Updated as of v0.0.0 (pre-implementation).

---

## North Star

Twenty phases mapping the journey from empty repo to 1.0.

- **Phases 0–9:** Simple Mode → **v0.1**
- **Phases 10–15:** Builder Mode → **v0.2**. Phase 12.5 adds a Calendar Month View alongside Forecast (same data, different lens).
- **Phases 16–19:** Import/export across Things 3, OmniFocus, Org-mode, Taskwarrior, Todoist, VTODO, todo.txt, TaskPaper. Phase 17 splits into 17 (one-shot import + DB → vault writes) and 17.5 (two-way `inotify`-driven sync).
- **Phase 20:** Polish, localisation, Flathub → **v1.0**

Each phase ends with a `heaptrack` checkpoint against the §8 budget. Every phase that adds a third-party crate calls it out — *no third-party deps without prior sign-off*.

The **debug harness** (spec §3.4 — `--debug` flag, stress generators, IO instrumentation, memory watch) lands as a skeleton in Phase 0 and grows alongside the features that need it: schema-aware fixtures in Phase 1, SQLite IO tracing in Phase 2, live RSS/heap surfacing in Phase 8. It is not a one-time deliverable.

---

## Phase 0: Scaffolding
*Repo bones. Standard project layout, Rust toolchain, CI baseline. No domain code.*

- [x] **Cargo skeleton:** `Cargo.toml` with `gtk4`, `libadwaita`, `tokio`, `rusqlite` (`bundled`, `chrono` features), `serde`, `serde_json`, `chrono`, `anyhow`, `thiserror`, `tracing`, `tracing-subscriber`. *(matches Viaduct's choices)*
- [x] **Module layout:** workspace split — `atrium-core/src/{db,domain,paths,error,lib.rs}` (headless lib) and `atrium/src/{ui,quickentry,debug,error,main.rs}` (binary). Roadmap originally specced single-crate `src/{...}`; workspace adopted in v0.0.3 to mirror Viaduct and pre-empt the Phase 20 `atriumd` daemon split.
- [x] **Application identifier:** `io.github.virinvictus.atrium` exposed as `atrium_core::APP_ID`; will appear in desktop entry / GSettings / AppStream / Flatpak manifest as those land.
- [x] **License + headers:** MIT `LICENSE` from day one; `SPDX-License-Identifier: MIT` on every Rust source file.
- [x] **Project metadata:** `VERSION` (0.0.3), `README.md`, `roadmap.md`, `spec.md`, `patchnotes.md`, `CLAUDE.md`, `logo.svg`. `VERSION` ↔ `Cargo.toml` workspace version ↔ `meson.build` project version kept in sync per Release discipline.
- [x] **Meson wrapper:** mirrors Viaduct's pattern (`cargo build --release` → install to `$bindir`); GSettings / desktop / metainfo install_data calls grow into it in Phase 3 and Phase 8.
- [x] **XDG path helpers:** `atrium_core::paths::{data_dir, cache_dir, db_path}` — stdlib-only, honour `XDG_DATA_HOME` / `XDG_CACHE_HOME` with `$HOME/.local/share` / `$HOME/.cache` fallbacks.
- [x] **Error type hierarchy:** `thiserror`-driven `DbError`, `DomainError`, `CoreError` in `atrium-core`; `UiError`, `AtriumError` in `atrium`. Phase 0 ships the scaffolding; concrete variants land in Phases 1–3.
- [x] **`tracing-subscriber`** with env-filter (default: `info,atrium=debug,atrium_core=debug`); compact format, target on.
- [x] **`--debug` flag plumbing:** stdlib argv parse, `Config` struct, `debug::Pane` stub gated on the flag. Logged at startup. The Phase 3 application shell will mount the actual widget. Features grow into the harness per spec §3.4.
- [x] **GitHub Actions CI:** `.github/workflows/ci.yml` — Ubuntu 24.04, apt installs gtk4/libadwaita/sqlite, runs `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace`.

## Phase 1: Schema Design — The OmniFocus Superset
*The schema decision that makes Mode-as-View work. Get it right once.*

- [x] **Initial migration:** `0001_initial.sql` covering all tables in spec §4: `task`, `project`, `area`, `tag`, `task_tag`, `heading`.
- [x] **Builder-superset columns:** every Builder-only field exists at v0.1 with sane NULL/false defaults.
- [x] **WAL mode + pragmas:** `synchronous=NORMAL`, `temp_store=MEMORY`, `mmap_size=268435456`, `foreign_keys=ON`. Configured per-connection via `db::configure_pragmas`.
- [x] **`user_version` PRAGMA** drives migration state; `migrations::migrate` runs pending migrations inside per-version transactions on `db::open`.
- [x] **FTS5 virtual table** `task_fts` (content='task', tokenize='unicode61') indexing `title` + `note`; insert/update/delete triggers keep it synced.
- [x] **`modified_at` triggers** auto-update on `task`, `project`, `area`, `tag`, `heading` UPDATE; `WHEN old.modified_at = new.modified_at` clause prevents recursion and lets explicit writes survive (import preservation).
- [x] **Indexes:** `(project_id, completed_at)`, partial `(scheduled_for/deadline/defer_until WHERE completed_at IS NULL)`, `(completed_at WHERE completed_at IS NOT NULL)`, `(parent_id WHERE parent_id IS NOT NULL)`, `(area_id)`, `(archived_at)`, `(heading.project_id)`, `(task_tag.tag_id)`. Partial indexes shrink the scanned subset.
- [x] **Schema documentation:** `docs/schema.md` with Mermaid ER diagram and per-table/column rationale; cross-references spec §4 (contract).
- [x] **Stress fixture generator** (debug harness, spec §3.4): `--fixture <small|medium|large|stress>` (1K/10K/50K/100K) calls `atrium_core::db::fixtures::generate`. Realistic distribution (~20 tasks/project, ~14 % inbox, mix of scheduled/completed/Someday, unicode-hostile titles). Reused by integration tests. Phase 3 will move it behind a debug-pane menu.

## Phase 2: Data Layer — Single-Writer Worker
*Viaduct's pattern, ported. The UI thread never blocks on the DB.*

- [x] **Domain types:** Rust structs for `Task`, `Project`, `Area`, `Tag`, `Heading` with `serde` derives, plus `ScheduledFor` enum (`Someday | Date(NaiveDate)`) with custom `ToSql`/`FromSql` so the schema's "ISO date OR `__someday__` sentinel" stays type-safe in Rust.
- [x] **`TaskChanges` batch type:** `{ created, updated, deleted, status_changed }` — the unit of UI delivery. `merge()` for coalescing.
- [x] **Single-writer task:** `db::worker::Worker` owns the writable `rusqlite::Connection`; receives commands via bounded `mpsc` (capacity 64). `WorkerHandle` is `Clone`; the worker exits when the last handle drops.
- [x] **Read-only connection pool:** `db::read_pool::ReadPool` — lazy on-demand `Mutex<Vec<Connection>>`, soft cap on idle connections, `PRAGMA query_only = ON` per connection so SQLite enforces read-only at the engine level.
- [x] **Command/Query split:** `enum Command { CreateTask, UpdateTask, ToggleComplete, DeleteTask }` (Phase 2 set; project/area/tag commands follow in Phase 5). Read fns in `db::read` (`task_by_id`, `list_inbox`, `list_all_tasks`, `count_tasks`) take `&Connection` so they compose with both worker and pool connections.
- [x] **`glib::MainContext::channel`** wiring: `TaskChanges` reach the UI thread via `glib::MainContext::default().spawn_local(async move { while let Some(c) = rx.recv().await { … } })`. tokio mpsc receivers are runtime-agnostic at the waker layer, so glib's executor drives them without an extra crate.
- [x] **Coalescer foundations:** `TaskChanges::merge` folds multiple deltas into one; the worker emits one `TaskChanges` per command. Aggressive coalescing (time-debounced batching for bulk import) lands with the importers in Phase 16+.
- [x] **In-memory test harness:** every Phase 2 op tested against `:memory:` SQLite — CreateTask round-trip, UpdateTask preserves other fields, ToggleComplete flips `completed_at` and emits `status_changed`, DeleteTask emits `deleted`, NotFound on missing id, position increments per sibling-list, ScheduledFor::Someday round-trips, worker shuts down cleanly on handle drop.
- [x] **IO instrumentation** (debug harness, spec §3.4): rusqlite `Connection::profile` callback routes every SQL statement (text + elapsed wall time) to `tracing` at TRACE level. `RUST_LOG=trace` (or scoped `atrium_core::db=trace`) reveals each statement; the Phase 3 debug pane will surface it visually. Required adding the `trace` feature to `rusqlite` (no new crate; feature flip on existing dep).

## Phase 3: Application Shell
*Window opens, settings persist. No tasks yet.*

- [x] **`adw::Application` skeleton** with `io.github.virinvictus.atrium` ID. Tokio multi-thread runtime built once in `main` (held in `OnceLock<Runtime>`); GTK owns the main thread via `app.run`.
- [x] **`AdwApplicationWindow` + `AdwNavigationSplitView`** root tree from `data/window.ui` via `gtk::CompositeTemplate`. Sidebar holds the six canonical Simple-Mode list rows (Inbox / Today / Upcoming / Anytime / Someday / Logbook); content pane shows an `AdwStatusPage` placeholder until Phase 4 lands real lists.
- [x] **GSettings schema:** `data/io.github.virinvictus.atrium.gschema.xml` declares `mode` enum (Simple/Builder), `window-width`/`window-height`/`window-maximized`, `sidebar-width`, `quick-entry-shortcut`. `atrium/build.rs` runs `glib-compile-schemas` and bakes `ATRIUM_GSCHEMA_DIR` so `cargo run` works without install. Meson installs to `$datadir/glib-2.0/schemas/` and recompiles in a post-install hook.
- [x] **Mode-switch plumbing:** stateful `gio::SimpleAction` `app.mode` (parameterised on `s`) writes the selected mode to GSettings; the action's state mirrors back. UI surfaces wire to `gsettings.connect_changed("mode", …)` as they grow.
- [x] **Empty Simple Mode shell:** sidebar with placeholder list rows (Inbox/Today/Upcoming/Anytime/Someday/Logbook), empty content pane saying "No tasks yet". Today is selected on first run.
- [x] **Light/dark follow-system** via libadwaita's default `AdwStyleManager` color-scheme (`Default` follows the host).
- [x] **Typography foundation:** Inter Variable + Italic, Source Serif 4 Variable Roman + Italic, JetBrains Mono Variable + Italic — all SIL OFL 1.1 — bundled at `data/fonts/`. Installed to `$XDG_DATA_HOME/fonts/atrium/` on first run with `fc-cache` refresh (the proven Viaduct pattern). `data/style.css` loaded via `gtk::CssProvider`; tabular figures (`tnum`) default-on for `.numeric` selectors so badges and dates don't dance.
- [x] **About dialog** (`adw::AboutDialog`) with version (compile-time `CARGO_PKG_VERSION`), MIT, repo + issues URL, designer/developer credits, acknowledgement section (Things 3, OmniFocus, Org-mode, NetNewsWire), bundled-fonts legal section.
- [x] **First-run state:** opens to Today (sidebar pre-selected); empty `AdwStatusPage` in content. Window size/maximized state persisted to / restored from GSettings on close-request and construction.

## Phase 4: Simple Mode — Inbox & Today
*The two views every Things user opens fifty times a day. Get them perfect first.*

- [x] **Inbox view:** `GtkListView` + `GtkSignalListItemFactory` over a `gio::ListStore<AtriumTask>` populated from `db::read::list_inbox` via the read pool. Sidebar click switches to it.
- [x] **Today view:** `db::read::list_today(today)` per spec §4.2 — open + scheduled-or-deadline ≤ today + not deferred + Someday sentinel excluded. Same factory, swapped model. Date computed via `chrono::Local::now()`.
- [x] **Task row widget:** completion checkbox (CSS-circular), `GtkEditableLabel` title (inline edit on click), schedule pill, deadline pill. Tag pills land Phase 6 with the tag editor; CSS fade-on-completed lands as the animation polish in Phase 8.
- [x] **Inline create:** bottom-of-list `GtkEntry` ("Add task…") above the new-task button. `Ctrl+N` focuses the entry; Enter commits via `worker.create_task(NewTask)`. The entry clears on each commit so several captures in a row stay fluid.
- [x] **Inline edit:** `GtkEditableLabel` swaps to entry on click; Enter commits → `worker.update_task(TaskUpdate::title)`; Esc cancels.
- [x] **Completion toggle:** click circle → `worker.toggle_complete(id)`; row leaves Today (or Inbox) on the worker's TaskChanges delta; the fade animation polish lands in Phase 8 along with the typography pass.
- [x] **Drag-to-reorder** within Inbox (Today is auto-sorted by date). `GtkDragSource` + `GtkDropTarget` per row carry the task id; window's `handle_reorder` computes a midpoint position between the dest row and its neighbour and fires a single `update_task`. Store re-sorts by position via `task_list::sort_by_position` after `apply_changes`.
- [x] **Empty states:** `AdwStatusPage` per list, swapped via `gtk::Stack`. Per-list copy ("Inbox is empty", "Nothing today", "Logbook is empty"); placeholder for Phase 5+ lists.

## Phase 5: Simple Mode — Areas, Projects, Anytime, Someday, Logbook
*Hierarchy and the rest of the lists. Now it's a real Things 3 analogue.*

- [x] **Sidebar tree (5a):** canonical lists section + Areas (with nested Projects) + Unfiled projects + Tags placeholder. Built dynamically on `attach_data_layer` from `db::read::list_areas` + `list_projects`. Per design call, `GtkListBox` with non-selectable header rows rather than `GtkTreeListModel` — simpler given Phase 5's overall scope; can be re-skinned in Phase 8 polish if perf demands.
- [x] **Area CRUD (5b):** `Command::CreateArea` / `UpdateArea` / `DeleteArea` through the worker. Menu items "New Area", `F2` Rename, `Ctrl+Shift+Delete` Delete (with destructive confirmation). `Ctrl+Shift+A` for new area. FK `ON DELETE SET NULL` unfiles the area's projects, and the worker emits `LibraryChanges{areas_deleted, projects_updated}` so the sidebar reflects the unfiling immediately. Right-click context menus deferred to Phase 5.5.
- [x] **Project CRUD (5b):** `Command::CreateProject` / `UpdateProject` / `DeleteProject` through the worker. Menu items "New Project", `Ctrl+Shift+N`, plus `F2` Rename and `Ctrl+Shift+Delete` Delete via the same window-scoped actions. New project defaults to the active area when one is selected (otherwise unfiled). `ON DELETE CASCADE` removes the project's tasks; the worker emits both `LibraryChanges{projects_deleted}` and `TaskChanges{deleted}` so list views drop the rows. Headings (sectioned project page) deferred to Phase 5.5.
- [x] **Anytime / Someday / Upcoming / Logbook (5a):** each backed by a `db::read::*` function. Same factory as Inbox/Today; just swap models. Logbook orders by `completed_at DESC`; Upcoming groups by date in render (date headers a Phase 5.5 polish item).
- [x] **Task move (5c):** drag a task row onto any sidebar project (or onto Inbox to unfile) → fires `worker.update_task(TaskUpdate::project(Some(id)))` (or `None` for Inbox). Reuses Phase 4.5's `GtkDragSource` on rows; `GtkDropTarget` lives on each project / Inbox sidebar row.
- [x] **Project completion (5b):** `Command::ArchiveProject` sets `archived_at = now` *and* completes every still-open task in the project inside the same SQL transaction (per design call — Things-3 behaviour). Menu item "Archive Project". The worker emits `LibraryChanges{projects_updated}` and `TaskChanges{updated, status_changed}` for the affected tasks so list views drop them and Logbook picks them up.
- [x] **Sidebar count badges (5c):** integer badges on every canonical list, area, and project. Hidden when zero (per design call). `count_open_canonical(today)`, `count_open_per_project`, `count_open_per_area` in `atrium-core::db::read` populate the caches; `refresh_canonical_badges` / `refresh_dynamic_badges` update labels in place on every `TaskChanges` / `LibraryChanges` so values stay live.

## Phase 6: Simple Mode — Tags & Quick Entry
*Capture without context-switch. The feature that makes a task app stick.*

- [x] **Tag CRUD (6a):** `Command::CreateTag` / `UpdateTag` / `DeleteTag` through the worker. Sidebar Tags section appears below Areas/Unfiled, populated from `db::read::list_tags`. Right-click context menu (Rename / Delete with confirmation). `Ctrl+Shift+T` for new tag; `F2` rename / `Ctrl+Shift+Delete` delete reuse the same actions Phase 5b wired. Schema's `NOCASE` uniqueness on `tag.name` makes "Errand" and "errand" the same tag.
- [x] **Multi-tag display (6b)** in the task row: tag names render as `#tag` inline after the title via `tag_names_csv` on `AtriumTask`. Tag map loads in one batched `read::tag_names_per_task` call per refresh; the diff applier consults it on `apply_changes`. Editing currently happens via inline `#tag` syntax in the entry or Quick Entry (6c) — a per-row autocomplete popover lands in Phase 8 polish.
- [x] **Tag pages (6a):** `ActiveList::Tag(i64)` + `db::read::list_tasks_with_tag(id)` join through `task_tag`. Clicking a tag in the sidebar swaps the content pane to that tag's open tasks; sidebar count badges and content-pane title (`#tagname`) update from the tag-title cache.
- [x] **Inline tag syntax (6b):** typing `#errand` in the bottom entry strips the token from the title, ensures the tag exists (NOCASE-resolved against existing names; created on first use), and attaches it via `worker.set_task_tags` after the task is created. Multiple `#tag` tokens compose. Phase 6c reuses the same parser inside Quick Entry.
- [x] **Inline date syntax (6b):** `@today`, `@tomorrow`, `@someday`, `@yyyy-mm-dd`, `@deadline yyyy-mm-dd` parsed by `quickentry::parser::parse`. `@today` resolves via `chrono::Local::now().date_naive()`. Unrecognised `@foo` strings stay in the title verbatim — no silent data loss. 12 parser tests cover every form including the combined-syntax case (`Buy milk #errand #grocery @today @deadline 2026-05-20`).
- [x] **Quick Entry shortcut (6c):** `Ctrl+Alt+Space` opens an `adw::Window` (transient, non-modal) with a `gtk::Entry` and a hint label. Esc dismisses; Enter commits via `worker.create_task` + `ensure_tag` + `set_task_tags`. Same `quickentry::parser` the bottom-of-list entry uses, so `#tag` / `@today` / `@deadline yyyy-mm-dd` syntax works identically in both.
- [x] **Quick Entry behaviour (6c):** drops to Inbox; closes on Enter (committed) and Esc (cancel). Modal is `set_modal(false)` and `transient_for(main)` so it sits above the parent without GTK's strict modal grab. The "doesn't steal focus from previously focused window" guarantee is honoured to the extent in-app accelerators allow — true zero-launch capture is a Phase 20 `atriumd` story.
- [ ] **Quick Entry cold-start:** if Atrium isn't running, launches minimised and posts the task. *Deferred to Phase 20 (`atriumd` capture daemon) per spec §6 — the in-app `Ctrl+Alt+Space` shipped in v0.0.16 only fires while Atrium has focus.*

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

- [ ] **Typography polish — Inter OpenType features:** selectively enable `cv11` (curved-l) and `ss01` (single-storey-a) on UI labels for visual distinction.
- [ ] **Typography polish — tabular figures audit:** verify `tnum` lands on every count badge, date column, and timestamp; no dancing digits.
- [ ] **Typography polish — accessibility option:** Settings → Accessibility → "Use high-legibility font" toggle bundling Atkinson Hyperlegible (SIL OFL, ~80 KB), designed for low-vision readers.
- [ ] **Typography polish — surface-by-surface pass:** size, weight, leading, letter-spacing for every label / heading / list-row / inspector field hand-tuned.
- [ ] **Flatpak font verification:** confirm `flatpak run` ships fonts identically to native install (Pango font-map registration is in-process, but verify under sandbox).
- [ ] **Logo / icon:** scalable SVG following GNOME icon guidelines; install to hicolor.
- [ ] **Desktop entry:** `io.github.virinvictus.atrium.desktop` (validated by `desktop-file-validate`).
- [ ] **AppStream metainfo:** `io.github.virinvictus.atrium.metainfo.xml` with screenshots, OARS rating.
- [ ] **Flatpak manifest:** `data/io.github.virinvictus.atrium.yml` against GNOME 50 runtime.
- [ ] **Animations:** task completion check, list transitions, modal fade — match libadwaita timings.
- [ ] **Memory profile:** `heaptrack` baseline against §8 targets.
- [ ] **Memory watch surface** (debug harness, spec §3.4): live RSS / heap sample readout in the debug pane with a "drop caches" affordance, so leaks and retention surface without leaving the app.
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

## Phase 12.5: Builder Mode — Calendar Month View
*The other side of Forecast — a familiar month grid for users who think in calendar pages.*

- [ ] **Month-grid widget:** `GtkGrid` of 7 columns × 5–6 weeks; each cell is a day. Optional ISO week-number column on the left.
- [ ] **Per-day task rendering:** count badge for normal density; up to ~3 task titles inline; "+N more" overflow link that opens a popover with the full day's list.
- [ ] **Today indicator** + overdue/due-today emphasis + month/year header that updates with navigation.
- [ ] **Month nav:** prev / next / "go to today" + month picker; `Page Up` / `Page Down` for keyboard-driven traversal; `Ctrl+Shift+M` opens the view.
- [ ] **Drag-to-reschedule between days:** dropping a task on a different cell updates `scheduled_for` (or `deadline` with a modifier — see UX call before implementing).
- [ ] **Click-day-to-filter:** clicking a day cell opens a side panel (or popover) listing that day's tasks; double-click swaps the content pane to a date-scoped filter.
- [ ] **Narrow-window collapse:** below a breakpoint, the month grid collapses to a vertical week strip so the view stays usable on small windows / mobile-shaped portrait sizes.
- [ ] **Builder-only sidebar entry** `Calendar` next to `Forecast` (visible when mode = Builder).
- [ ] **Tests:** date-filter SELECT correctness across month boundaries, DST edges, and leap-day February.

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

## Phase 17: Org-Mode Import & Read-Only Sync (DB → Vault)
*Atrium writes a clean vault any Org tool can open and read; existing Org libraries import in. The plain-text covenant, half realised.*

- [ ] **Org parser/emitter research:** evaluate the `orgize` crate vs hand-rolled subset; flag for sign-off if `orgize` is added.
- [ ] **Vault discovery + GSettings:** `vault-path` key; default `~/Tasks/`; Settings → Org Vault → Choose folder; "no vault" remains a valid configuration (Atrium runs DB-only).
- [ ] **One-shot importer (`src/sync/org/import.rs`):** point at a directory or single file, dry-run mode showing what would land. Coverage: TODO/DONE/CANCELLED keywords, SCHEDULED/DEADLINE/CLOSED cookies, headline tags, `:PROPERTIES:` drawers, body text, nested subtasks. Maps per spec §7.3.
- [ ] **Writer (`src/sync/org/write.rs`):** emits `<vault>/<Area>/<Project>.org` per spec §7.3 — `#+TITLE:` headers, `:PROPERTIES:` drawers, SCHEDULED/DEADLINE/CLOSED cookies, headline tags, full field mapping.
- [ ] **`:ID:` allocation:** every task/project on first vault write receives a stable UUID; imported tasks keep their `:ID:` if present, get one assigned (and the file rewritten) if absent.
- [ ] **Atomic file writes:** `write-temp + fsync + rename` for every vault write. Crash-safe.
- [ ] **Sidecar (`<vault>/.atrium/config.toml`):** tag colors, perspectives placeholder, mode preference. Read on startup, written on relevant changes. Other Org tools ignore.
- [ ] **Worker write hook:** every `TaskChanges` commit queues a vault-write job for affected projects; debounced 100 ms to coalesce bursts.
- [ ] **Post-write integrity check:** newly-written file parses cleanly with Atrium's own reader; mismatch → toast + rollback.
- [ ] **Atrium native JSON export ships in this phase too** — universal lossless backup format.
- [ ] **Round-trip test fixture:** import → export → diff = empty (modulo whitespace and section ordering).

## Phase 17.5: Two-Way Org Sync (Vault → DB)
*Emacs / Doom / vim-orgmode edits flow back. The covenant fulfilled.*

- [ ] **`inotify` watcher:** vault root + subdirectories; events debounced 200 ms.
- [ ] **Self-write filter:** worker tracks `(file_path, mtime)` of its own writes briefly; matching events ignored so the loop doesn't echo.
- [ ] **Reader → DB diff:** parse changed file; diff against DB by `:ID:`; submit INSERT/UPDATE/DELETE through the worker as TaskChanges.
- [ ] **`:ID:` allocation on read:** tasks added in Emacs without `:ID:` get one assigned, file rewritten back with the property.
- [ ] **Conflict detection:** mtime race → loser saved as `<file>.atrium.bak.<timestamp>`; UI toast surfaced. Never silent overwrite.
- [ ] **Malformed-file handling:** parse failure → vault sync paused for that file, DB version preserved, toast surfaced; auto-resume when the file parses again.
- [ ] **Custom-keyword + unknown-construct preservation:** verbatim round-trip per spec §7.3.3 rule 1.
- [ ] **RRULE divergence detection:** SCHEDULED cookie semantically diverged from `:RRULE:` → surface in post-sync report; DB keeps the canonical RRULE.
- [ ] **Test scenarios:** synthesized concurrent edit, malformed-file recovery, round-trip across all field types, large-file (1K-task project) parse latency.

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

---

## Beyond 1.0

Post-1.0 horizon. Not committed to phase numbers yet — sketched here so that contract decisions made before 1.0 don't accidentally close these doors. Scope and ordering will be re-litigated when the time comes.

### Toward 2.0 — Full TUI mode (`atrium-tui`)
*A keyboard-first terminal frontend over the same headless core that powers the GTK binary.*

The workspace split done in Phase 0 (`atrium-core` headless library + `atrium` GTK binary) is what makes this cheap — the data layer was designed to support multiple frontends (the Phase 20 `atriumd` capture daemon is already a second consumer). A TUI is the third.

Rough shape, to flesh out closer to the time:

- Three-pane layout cribbing the same Things-3 idiom in cells: lists sidebar, content pane, optional inspector.
- Quick Entry from any terminal context (Vim-style `:` or a global capture key inside the TUI).
- Same Simple / Builder mode-switch — the data is the same.
- FTS5 search via `/`, filter expressions reused from spec §7's filter language.
- Composes with `tmux` and `screen`; respects `NO_COLOR`, `COLORTERM`, and 256-colour vs truecolour terminals.
- Dependency check: a TUI crate (likely `ratatui`) gets a sign-off pass before it lands.

### Not currently slated

Items in spec §9 (network sync of any kind, mobile/web clients, multi-user, time-tracking, calendar event creation, AI features) remain out of scope through 1.0 and are not on this horizon either. Adding any of them is a separate conversation.
