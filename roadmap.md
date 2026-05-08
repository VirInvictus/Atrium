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

- [x] **FTS5-backed search bar (7a):** `Ctrl+F` opens a `GtkSearchBar` in the content header; `GtkSearchEntry` debounces 200 ms via its `search-delay` property; results render via `db::read::search_tasks` (joins `task_fts` MATCH + ORDER BY rank). `ActiveList::SearchResults(query)` renders identically to the canonical lists. Esc closes the bar; clearing the entry falls back to Today. Recency-multiplier ranking lands as a Phase 8 polish item — relevance-only is the v0.1 base.
- [x] **Filter expressions (7d):** the search bar accepts `tag:NAME` / `is:open` / `is:done` / `is:overdue` / `due:today`. `atrium/src/ui/filter.rs` parses queries into `FilterQuery { text, filters }`; the freeform text goes to FTS5, the filters apply in Rust against the `tag_names_per_task` map and today's date. `area:` / `project:` filters are Phase 8 polish (need name → id resolution against the sidebar caches).
- [x] **Full keyboard map (7f):** every common op is bound. The chord scheme is documented in `docs/keymap.md` and surfaced in-app via `Ctrl+?` / `F1`. `Ctrl+Z` invokes whatever undo callback is currently live (the toast button and the accel share a single `UndoCell`; whoever fires first consumes it). `F2` starts inline editing on the focused task row's `EditableLabel` and falls through to the sidebar rename when focus lives on an Area / Project / Tag instead. The remaining stub binding is `Ctrl+,` (Preferences) which lands with Phase 8 settings; redo (`Ctrl+Shift+Z`) defers to Phase 11+ alongside the Builder-mode action history.
- [x] **Multi-select (7c):** `gtk::MultiSelection` model — `Ctrl+Click` toggles, `Shift+Click` ranges, `Ctrl+A` Select All all out-of-the-box. `Esc` clears. A revealing toolbar above the task list shows "N selected" with **Complete** and **Delete** (destructive-styled) buttons + a Clear icon. Bulk handlers fire individual worker calls in a loop and skip per-item undo toasts; a single coalesced "N of M deleted" summary toast appears after bulk-delete completes. Bulk-tag / bulk-move are Phase 8 polish items (need a project / tag picker dialog).
- [x] **Undo (7b):** completion toggle and task delete are undoable via `adw::Toast` (6 s window). Toggle undo re-toggles via the existing worker call. Delete undo captures the full task state + tag attachments before delete and recreates via `create_task` + `set_task_tags`. Cascade-deleted subtasks aren't recovered (Phase 8 polish could capture the full subtree). Move-to-project / archive undo land alongside their menu entries in Phase 8.
- [x] **Per-task Inspector (7i):** double-click any task row, right-click → *Edit Details…*, or `Ctrl+I` opens a modal `adw::Window` (`atrium/src/ui/inspector.rs`) with the editable Simple Mode fields that previously had no UI: title, notes (multi-line `gtk::TextView`), schedule (When), deadline, and project assignment. Tags delegate to the existing Phase 7g editor via an *Edit Tags…* hand-off button. `TaskUpdate` extended in `atrium-core` with `scheduled_for` + `deadline` (`Option<Option<…>>` for set/clear semantics); the worker SQL builder applies them transactionally. Schedule + deadline pickers use a popover with Today / Tomorrow / Someday / Clear presets plus a `gtk::Calendar` for arbitrary dates. Apply diffs against the opened snapshot and dispatches a single `update_task` with only the changed fields.
- [x] **Per-task tag editor (7g):** right-click on a task row surfaces *Edit Tags…*; `Ctrl+T` does the same for the focused / first-selected task. Opens an `adw::Window` (`atrium/src/ui/tag_editor.rs`) with a `boxed-list` checkbox per existing tag (current ones pre-checked) plus an inline "Add a new tag…" entry. Apply pipelines `worker.ensure_tag(name).await` for each new name then `worker.set_task_tags(task_id, ids).await` — single transactional write per accept. Cancel / Esc dismiss. The visible inline tag display also got a CSS lift: chip-shaped `.atrium-task-tags` with the libadwaita accent palette (`alpha(@accent_bg_color, 0.15)` background + `@accent_color` text), 6 px radius, blends into the row fade when the task completes. Closes the Phase 6 patchnote that said "per-row autocomplete popover lands in Phase 8 polish" — picked up here in Phase 7 follow-up since v0.1 daily-driver work needs it.
- [x] **Find-as-you-type (7e):** the sidebar grows a `GtkSearchEntry` above the row list. Live substring match against area / project / tag titles; canonical lists (Inbox, Today, …) always stay visible; section headers ("Areas", "Unfiled", "Tags") hide automatically when none of their children pass. `Ctrl+L` focuses and selects-all in the entry; `Esc` clears. Pure visibility logic factored into `compute_sidebar_visibility` with 6 unit tests.

## Phase 8: Simple Mode — Polish, Typography, Packaging
*Visual identity. Make it feel inevitable, not improvised.*

- [x] **Typography polish — Inter OpenType features (8a):** `cv11` (curved-l) and `ss01` (single-storey-a) land on every UI surface via the `--atrium-inter-features` CSS variable, applied at the `window` selector so all descendants inherit. Note bodies (`.atrium-note-body`, serif) and the debug pane (`.atrium-debug-pane`, mono) explicitly opt out with `font-feature-settings: normal`.
- [x] **Typography polish — tabular figures audit (8a):** `tnum` + `font-variant-numeric: tabular-nums` land on `.numeric` (sidebar count badges), `.atrium-task-schedule`, and `.atrium-task-deadline`. Selectors corrected from the Phase 3 placeholders (`.task-row .date` etc.) to the real CSS classes added in `task_list.rs`.
- [x] **Typography polish — accessibility option (8c):** Atkinson Hyperlegible bundled at `data/fonts/AtkinsonHyperlegible-{Regular,Italic,Bold,BoldItalic}.ttf` (~220 KB, SIL OFL 1.1, © 2020 Braille Institute of America). Designed for low-vision readers — high inter-character distinguishability. GSettings key `high-legibility-font` (boolean, default false). Primary menu → *Mode → Accessibility → Use High-Legibility Font* toggles a stateful `win.high-legibility-font` action which writes the GSetting and adds the `atrium-high-legibility` CSS class to the window. The CSS swaps `font-family` to "Atkinson Hyperlegible" for all descendants and resets `font-feature-settings` (Inter's `cv11`/`ss01` don't apply to Atkinson). Tabular figures stay on for numeric surfaces regardless of which face is active. External GSetting changes (dconf-editor, another window) flow back via `connect_changed` so the action state and CSS class stay coherent. Settings dialog (proper UI) lands in Phase 8d alongside Preferences (`Ctrl+,`).
- [x] **Typography polish — surface-by-surface pass (8a):** task-row title at 1.0em / weight 450 / letter-spacing −0.005em for scan-density without shouting; schedule + deadline pills at 0.92em (deadline gets weight 500 so "Due tomorrow" reads ahead of "Today"); inline tag display at 0.88em with +0.005em tracking; sidebar badges at 0.88em.
- [ ] **Flatpak font verification:** confirm `flatpak run` ships fonts identically to native install (Pango font-map registration is in-process, but verify under sandbox).
- [x] **Logo / icon (8b):** placeholder logo (`logo.svg`) installed at `data/icons/hicolor/scalable/apps/io.github.virinvictus.atrium.svg` via `install_data` in `meson.build`. Final icon design pass remains a Phase 9 / pre-1.0 task — the placeholder is "replace before 1.0" as noted in its comment.
- [x] **Desktop entry (8b):** `data/io.github.virinvictus.atrium.desktop` — Categories `GTK;Office;ProjectManagement;`, StartupWMClass tied to the app-id, Keywords cover todo/gtd/omnifocus/things/org-mode. `desktop-file-validate` clean.
- [x] **AppStream metainfo (8b):** `data/io.github.virinvictus.atrium.metainfo.xml` — id, name, summary, description, OARS 1.1 content rating, branding colors, releases for v0.0.0 → v0.0.23 condensed. Three `url-not-reachable` warnings during local validation are expected — the GitHub URLs are aspirational until the repo goes public; structurally the file is correct. Screenshots section deferred to Phase 9 (need a release build to capture against).
- [x] **Flatpak manifest (8b):** `data/io.github.virinvictus.atrium.yml` — GNOME 50 runtime + `org.freedesktop.Sdk.Extension.rust-stable` for cargo, meson buildsystem, minimal sandbox (`--share=ipc --socket=wayland --socket=fallback-x11 --device=dri --filesystem=home`), no network at runtime per spec §3 (local-first). Vendored cargo sources for offline Flathub builds defer to Phase 9. PNG icon ladder generated post-install via `rsvg-convert` so software centers don't need a librsvg pixbuf loader.
- [x] **Animations (8a + 8d):** task completion fade ✓ (8a — `.atrium-task-row` opacity fades to 0.55 over 180ms ease-out, title gets line-through). List transitions ✓ (libadwaita's `crossfade` on `content_stack` + `slide-down` on `selection_revealer`, set in `data/window.ui` — Phase 8d audit confirmed both match libadwaita's standard timings). Modal fade ✓ (libadwaita handles `adw::Toast` and `adw::AlertDialog` natively; Quick Entry uses a plain `adw::Window` for non-modal transient-for-main behavior, so 8d adds a `.atrium-quickentry-window` class with a 150 ms `@keyframes` opacity fade-in to match libadwaita's dialog presentation feel). Custom CSS only fills the gaps libadwaita's defaults don't cover; the policy is documented inline in `data/style.css`.
- [x] **Memory profile (8g):** release-mode baseline captured in `docs/perf-baseline.md`. Cold start ~25–33 ms in ~32 MB. Data-layer cost flat with task count: 1K → 35 MB / 10K → 37 MB / 50K → 37 MB peak RSS. Throughput ~45K tasks/sec under transactional inserts. All four §8 budgets are met or trending well under at the data-layer level; GUI-mode RSS captured via the Memory Watch (Phase 8e) once an interactive session is run against. `heaptrack` is the next-deeper dive when growth surprises — not in CI but documented as the escalation tool.
- [x] **Memory watch surface (8e)** (debug harness, spec §3.4): `atrium/src/debug/mod.rs::open_memory_watch` mounts a transient `adw::Window` from *Debug → Memory Watch* in the primary menu (visible only when `--debug` is on). One-second `glib::timeout_add_local` reads `/proc/self/status` and surfaces VmRSS, VmHWM (peak), VmData (heap) plus a sample counter. Pretty-formatted in MB (or KB below 1 MB). The "drop caches" affordance defers to a follow-up — needs a `Command::TrimMemory` worker variant that issues SQLite `PRAGMA shrink_memory` on the writable connection. 5 unit tests cover the `/proc/self/status` parser + KiB formatter.
- [x] **Accessibility audit (8f):** every common op has a keyboard chord (full table in `docs/keymap.md`); every interactive widget has either a visible label, `tooltip-text`, or an `accessible::Property::Label` for AT-SPI consumers (task-row CheckButton + EditableLabel + every dynamically-built sidebar row got labelled in this slice). CSS doesn't hardcode foreground/background colours — every surface inherits from libadwaita's variables and respects light/dark + `prefer-contrast: more`. Reduced-motion + high-legibility toggle (Phase 8c) cover the explicit accessibility surfaces. Findings + conventions documented in `docs/accessibility.md` so the audit is repeatable as new widgets land.

## Phase 9: Simple Mode v0.1 Release
*Ship.*

- [x] **Full regression on a 1,000-task seed DB (9a):** `scripts/regression.sh` runs fmt → clippy → tests → release build → 1K-task fixture smoke (against an isolated `XDG_DATA_HOME`) → cold-start sanity ×3 (asserts <500 ms each). Single command, fail-fast, ends with a `PASS`/`FAIL` line that carries `VERSION`. Documented in `docs/regression.md` as the canonical ship gate.
- [x] **Patchnotes + version bump (9c):** `VERSION` 0.0.38 → 0.1.0; `Cargo.toml` workspace + `meson.build` synced; `data/io.github.virinvictus.atrium.metainfo.xml` gains the v0.1.0 `<release type="stable">` entry summarizing Simple Mode for software centers; `patchnotes.md` v0.1.0 entry frames the milestone (six canonical lists, hierarchy, Quick Entry, FTS5+filter expressions, Inspector, multi-select+undo, sidebar filter, keyboard map, typography+a11y, debug surface) and points at Phase 10. Closes the v0.1 doc-set discipline with all four sources of truth in agreement.
- [ ] **Tag `v0.1.0`**, publish Flatpak.
- [x] **README finalised (9b)** with the "Simple Mode now / Builder Mode next" framing — pre-implementation badge swapped for Simple Mode shipping / Builder Mode next; full feature table reflects everything actually shipped (search, filter expressions, multi-select, undo, sidebar filter, full keyboard map, accessibility, debug harness, bundled fonts including Atkinson Hyperlegible); Build and Run section documents the regression gate + fixture commands + Flatpak invocation. Screenshots section ships as a TODO placeholder for capture against the v0.1.0 tag.
- [ ] **First public release announcement** on `VirInvictus.github.io`.

---

## Phase 10: Builder Mode — UI Shell
*The mode switch becomes real. Inspector pane lands. No new logic — just exposure.*

- [x] **Mode toggle in primary menu:** `Settings → Mode → [Simple, Builder]` — `app.mode` action wired since Phase 3 now actually drives a re-render. `AtriumWindow::install_mode_observer` subscribes to `gsettings.connect_changed("mode", …)`; flips route through `apply_mode(&str)`, the single function every Builder-only widget consults for visibility.
- [x] **Inspector pane:** `data/window.ui` now wraps the `AdwNavigationSplitView` in an `AdwOverlaySplitView` whose right-side sidebar holds an `AdwBin` mounted with `atrium/src/ui/inspector_pane.rs::InspectorPane`. The pane swaps between an `AdwStatusPage` empty state and a per-task editor (same layout as the Phase 7i dialog Inspector but with a Builder group exposing `estimated_minutes` as a live `SpinRow` and `defer_until` / `repeat_rule` as disabled placeholder rows pointing at Phase 11 / 15). Auto-save on focus-out / Enter; no Apply button. `show-sidebar` is `false` in Simple Mode and `true` in Builder.
- [x] **Builder-only sidebar entries:** `ActiveList::Forecast` / `Review` / `Perspectives` variants appear under a `Builder` section header in `rebuild_dynamic_sidebar` when `mode = builder`. Selecting one routes through the existing content stack to an `AdwStatusPage` placeholder (icon + copy referencing the phase that lands the real content). No DB query runs for these views.
- [x] **Project page extras:** A `GtkRevealer` above the task list (`project_extras_revealer` in `data/window.ui`) holds a Sequential `GtkSwitch` and Review-interval `GtkSpinButton` (0–365 days). Visible only when `ActiveList::Project(id)` AND `mode = builder`. Wired to `worker.update_project(ProjectUpdate::sequential(_))` and `update_project(ProjectUpdate::review_interval_days(_))`; new `ProjectUpdate::review_interval_days` builder added to `atrium-core::domain` for the second one.
- [x] **No data changes:** `apply_mode` is purely UI — its only DB-layer reach is `ReadPool` (which sets `PRAGMA query_only = ON`). Enforced by `atrium-core/tests/mode_flip_snapshot.rs` — populates the Small fixture (1K tasks across 50 projects in 5 areas, 20 tags), snapshots every row of every user table, exercises the same read traffic a mode flip triggers (sidebar reads + Today read + canonical-counts read), asserts a write attempt through the read pool fails, snapshots again, and asserts byte-identical state. The doc comment on `apply_mode` cites the architectural argument inline.

## Phase 11: Builder Mode — Defer Dates & Sequential Projects
*OmniFocus mechanics. Tasks with future defer dates become "available" later.*

- [x] **`defer_until` editor** in Inspector — both the modal `inspector.rs` and the Builder side pane `inspector_pane.rs` now have a real Defer-until row using the same Today / Tomorrow / Calendar / Clear popover that drives Schedule and Deadline. `TaskUpdate` gains a `defer_value(Option<NaiveDate>)` builder method; the worker SQL builder picks up the new field. Modal Inspector commits via the existing Apply diff; side pane auto-saves on popover commit.
- [x] **List filter logic:** Today and Anytime already filtered `defer_until > today` since Phase 4 (the SQL was in place; only the editor was missing). With the Phase 11 editor live, the predicate finally has something to act on. Tests `today_excludes_deferred_to_future`, `anytime_excludes_future_deferred`, and `today_includes_deferred_now_active` cover the boundaries.
- [x] **Sequential project rendering:** `AtriumTask` gains a `queued` glib property; `task_list::compute_queued_state` flags every row past the first incomplete one as queued when viewing a sequential project. The factory's bind handler and a `connect_queued_notify` hook apply / remove the `.queued` CSS class so already-bound rows update when the head row gets completed (which promotes the next). `data/style.css` adds `.atrium-task-row.queued { opacity: 0.45; font-style: italic on title }`. `apply_changes_seq` recomputes the state after every TaskChanges delta on a sequential project view, so completion-toggling the head row demotes it and promotes the next in the same frame.
- [x] **"Available" task count:** `refresh_dynamic_badges` consults `project_meta` per project and runs an `available_count(open, sequential)` helper — sequential projects clamp to 0 or 1; parallel projects show their open count. Builder Mode only; Simple Mode keeps showing open count regardless. Three new unit tests cover the math.

## Phase 12: Builder Mode — Forecast View
*OmniFocus's killer view: a calendar-axis layout of next ~30 days.*

- [x] **Forecast page:** new `atrium/src/ui/forecast.rs` builds a vertical column of card-shaped day blocks for the next `FORECAST_WINDOW_DAYS` (30) days. Each card lists open tasks that touch the date via `scheduled_for`, `deadline`, or `defer_until`, each row tagged with a reason chip ("Scheduled" / "Deadline" / "Defer ends"). Empty days render with a single em-dash. Two new SQL queries — `list_forecast(conn, today, days)` and `list_overdue(conn, today)` — pull the data; pure-function `group_by_date` buckets it for rendering. The window content stack gains a `"forecast"` `GtkStackPage` hosting an `AdwBin id="forecast_host"`; `ActiveList::Forecast` mounts the freshly-built page on every refresh.
- [x] **Drag-to-reschedule:** every forecast row carries a `GtkDragSource` with the task id; every day card carries a `GtkDropTarget` that on drop fires `worker.update_task(TaskUpdate::new(id).schedule(Some(ScheduledFor::Date(target))))`. Dropping on the Overdue block is intentionally a no-op (overdue is the consequence of dates, not a target date). `apply_task_changes` re-renders the forecast page on any `TaskChanges` so the drop's resulting move is visible immediately.
- [x] **Today indicator and overdue surfacing:** today's day card adds the `.today` CSS class, which `data/style.css` paints with an accent border + accent heading. The Overdue pseudo-block sits above all day cards with destructive-accent styling and a "Caught up." subtitle when empty. Header titles also promote `Today · Wed May 7` and `Tomorrow · Thu May 8` for the first two days; further days show weekday + date.
- [ ] **Compact / expanded toggles** for dense schedules. *Deferred to Phase 12.5 / Phase 20 polish — Phase 12 ships the dense default. Compact / expanded as a per-card toggle requires a per-card state model that's worth its own follow-up.*

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

- [x] **`review_interval_days` per project** — Phase 11's project-page Review-interval `GtkSpinButton` already writes the column. v0.1.2 closed this slot ahead of Phase 13.
- [x] **Review perspective:** new `read::list_review_queue(conn, today)` SQL selects projects with `review_interval_days IS NOT NULL`, `archived_at IS NULL`, and `last_reviewed_at + review_interval_days days ≤ today` (or never reviewed). Order: never-reviewed first, then by oldest `last_reviewed_at`, then by `position`. The Phase 10 Review sidebar stub now mounts a real `atrium/src/ui/review.rs` page that renders one card per queued project (title, area subtitle, "Last reviewed N days ago" / "Never reviewed" caption, Mark Reviewed button). Empty queue shows an `AdwStatusPage` "All caught up" placeholder.
- [x] **"Mark Reviewed" action** — `Command::MarkReviewed { id }` worker variant + `WorkerHandle::mark_reviewed(id)` API. Handler runs `UPDATE project SET last_reviewed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1`, emits `LibraryChanges{projects_updated}`. The card's Mark Reviewed button dispatches via `glib::MainContext::default().spawn_local`, disables itself while in flight to prevent double-fire, and drops out of the visible queue when `apply_library_changes` triggers a page rebuild.
- [ ] **Per-area review schedules:** an area can default an interval new projects inherit. *Deferred — adds a `default_review_interval_days` column to the `area` table. v0.2.0 ended the schema freeze (per spec §4.5) so this can land as an additive migration any time; punted because the per-project interval (Phase 11's SpinButton) already gives users full control and this is quality-of-life on top, not a blocking gap.*

## Phase 14: Builder Mode — Perspectives (Saved Views)
*OmniFocus Perspectives. Filter expressions become first-class objects.* Shipped v0.1.17.

- [x] **Perspective domain type:** `name`, `filter_expression` (subset of Phase 7's filter language), `sort`, `grouping`. Backwards-compatible `0002_perspectives.sql` adds the table; `sort_order` and `grouping` columns ship now (UI consumers come later, no further migration needed).
- [x] **CRUD for perspectives:** create from current filter state via *Save Search as Perspective…* in the primary menu (only enabled on `SearchResults`); right-click row for rename / delete.
- [x] **Perspectives sidebar section** (Builder-only) — always present in Builder Mode, even when empty, so users know where new perspectives land. Per-row icon falls back to `view-grid-symbolic` when none is set.
- [ ] **Export perspective definition** to JSON for sharing. *Deferred to Phase 16 alongside the rest of the export work — the file format belongs with the other exports rather than as a one-off here.*

## Phase 15: Repeating Tasks
*The rabbit hole, addressed properly. RFC 5545 RRULE-based.* Shipped v0.2.0 — Builder Mode milestone tag.

- [x] **`repeat_rule` editor:** UI for daily / weekly / monthly / yearly + custom RRULE. Inspector pane (Builder-only): frequency dropdown + interval spin + mode dropdown + custom rule entry. Local + worker-side validation.
- [x] **Regenerate-on-complete logic:** when a repeating task is completed, the worker spawns the next instance with shifted dates, carried tags, preserved project / parent / repeat config. The completed instance stays in the Logbook.
- [x] **Defer-vs-due semantics:** all three Org cookies — `+1w` (Basic), `++1w` (Cumulative, default), `.+1w` (Next-from-completion). Persisted in a new `repeat_mode TEXT` column via `0003_repeat_mode.sql`.
- [x] **Edge cases:** end-of-month skip (Jan 31 + monthly = March 31), `COUNT=N` termination via per-spawn decrement, `UNTIL=` honored by the rrule iterator. Open tasks: `BYDAY` / `EXDATE` aren't exposed in the preset UI but Custom rule mode passes them through verbatim.
- [x] **Tests:** RRULE round-trip in `repeat.rs` (14 tests), regen-on-complete in worker (7 tests), 52-week horizon (one test running the regen loop 52 times). Total 212 tests pass.
- [x] **Dependency check:** `rrule` crate v0.14 (MIT/Apache) — sign-off granted before implementation. Default features only.

## Phase 15.5: Calibre-Powered Search
*Power-user search bar. Expansive boolean grammar over Phase 7d's foundation.* Shipped v0.4.0.

- [x] **Grammar design + spec.** `spec.md` §4.3 documents the full operator set with examples, EBNF-style grammar production, and precedence table.
- [x] **Lexer + parser** in `atrium-core/src/search/` — hand-rolled recursive-descent (no new parser-combinator dep), matches the convention set by `repeat.rs` and `quickentry/parser.rs`.
- [x] **AST + in-memory evaluator.** Single-pass traversal with short-circuiting AND / OR; lazy regex compilation cached per-query.
- [x] **Field operators.** `tag:`, `tags:`, `area:`, `project:`, `title:`, `note:`, `due:`, `scheduled:`, `defer:`, `created:`, `modified:`, `completed:`, `estimated:`, `repeats:`. Aliases (`tags`/`tag`, `deadline`/`due`, `est`/`estimated`, etc.) per the spec table.
- [x] **Boolean operators + grouping.** `AND` / `OR` (case-insensitive), implicit `AND` between bare tokens, parenthesised sub-expressions, `NOT` / `!` prefix. **`NOT > AND > OR`** precedence — matches Calibre, SQL, Python.
- [x] **Comparison operators.** `=` `!=` `>` `<` `>=` `<=` on date and numeric fields.
- [x] **Calibre-style match modifiers.** `tag:x` substring, `tag:"x y"` quoted substring, `tag:=x` exact, `tag:"=x y"` quoted exact, `tag:~regex` regex (via the `regex` crate; sign-off granted), `tag:true` / `tag:false` boolean existence.
- [x] **Date keywords.** `today` / `yesterday` / `tomorrow` / `thisweek` / `lastweek` / `nextweek` / `thismonth` / `lastmonth` / `nextmonth` / `thisyear` / `Ndaysago` / `Ndaysout`. Mon-start ISO weeks.
- [x] **Range syntax.** `due:2026-05-01..2026-05-31` inclusive.
- [x] **Quote escape.** `\"` and `\\` inside quoted values.
- [x] **State predicates.** `is:open`, `is:done`, `is:overdue`, `is:scheduled`, `is:deadline`, `is:deferred`, `is:repeating`, `is:archived`, `is:logbook`, `is:project`, `is:area`, `is:tagged`, `is:queued`, `is:available`. Each pairs with `!is:NAME` (or `NOT is:NAME`).
- [x] **Search-bar visual feedback.** Yellow `.warning` accent on the search entry when the parsed expression has unknown tokens; toast surfaces the typo'd field names. Cleared the moment the user fixes the typo.
- [x] **Tests.** 46 search-module tests in `atrium-core` covering parser round-trips, evaluator correctness across all operators, regex matching, boolean composition, range bounds, date-keyword resolution. Plus 4 binary-side `filter` tests for the window-side shim.
- [x] **Search history ring buffer** — closed at v0.5.0. `↑` / `↓` cycle the last 20 committed queries. In-memory only for now; GSettings persistence is a future polish item if usage warrants.
- [x] **Operator reference popover** — closed at v0.5.0. `?` button on the search bar opens a structured AdwPopover organised by section (Boolean, Fields, Modifiers, Comparison, Date keywords, State, Sort).
- [ ] **SQL-translation evaluator** *(deferred to v0.6.x patch)*. Translate the AST to a `WHERE` clause when expressible; fall back to in-memory for `~regex` and complex tag predicates. Pure perf optimization — the in-memory path handles 100K tasks within budget today.

## Phase 15.75: Atrium polish, search-engine evolution, atrium-cli
*Visual polish + per-area accent + Phase 15.5 deferred-list closure + the search engine and a full headless CLI extracted as their own workspace crates.* Slices A + B + the v0.4.x patch sequence shipped at **v0.5.0**; Slices C + D carry into v0.6.0.

- [x] **Slice A — schema foundation (v0.5.0).** Two additive migrations: `0004_area_color.sql` (one new column on `area`) and `0005_perspective_renderer.sql` (`renderer TEXT NOT NULL DEFAULT 'list'` + `renderer_config TEXT NULL`). `Area`, `NewArea`, `AreaUpdate`, `Perspective`, `NewPerspective`, `PerspectiveUpdate` types extended; worker SQL grew alongside; `user_version` 3 → 5.
- [x] **Slice B — visual rhythm + per-area accent (v0.5.0).** Hover-row "lift" cue (inset bottom border + alpha bump). Sidebar section letter-spacing 0.04em → 0.06em. `.atrium-note-body` italic + 1.6 line-height attached to both Inspector surfaces. Task list wrapped in AdwClamp (max 720 px). Per-area accent — six-swatch picker on the area edit dialog, coloured-dot sidebar row, 3 px row-left stripe via `.atrium-area-accent-{color}` class. Canonical-list icon tinting (Inbox blue, Today amber, Upcoming green, Someday purple, Logbook purple-2). Tag-icon fix: `tag-outline-symbolic` → `tag-symbolic`. About-dialog icon resolution: `register_icon_search_paths` walks runtime / compile-time / cargo-manifest fallbacks.
- [x] **v0.4.x search engine evolution (closed at v0.5.0).** Five canonical-list state predicates (`is:today` / `is:inbox` / `is:upcoming` / `is:anytime` / `is:someday`). `sort:KEY` / `sort:-KEY` modifier with primary→secondary composition + NULLs-last ordering. ↑/↓ search history (20-entry in-memory ring buffer). `?` operator-reference popover. Fuzzy `tag:?word` modifier (Damerau-Levenshtein, length-aware threshold).
- [x] **atrium-search workspace crate (v0.4.2).** Lifted `atrium-core/src/search/` into its own sibling crate. Same code, same tests; the search engine can be exercised, fuzzed, and reused (atrium-cli + future TUI / atriumd / search server) without dragging the SQLite/worker layer along.
- [x] **atrium-cli workspace crate (v0.4.3 → v0.4.7).** Headless binary with full task CRUD plus metadata reads. Reads (`search`, `list`, `info`) open `SQLITE_OPEN_READ_ONLY`; writes (`add`, `capture`, `edit`, `complete`, `delete`) spin up the worker on a current-thread tokio runtime. TSV (default) / JSON / human output formats. Quick Entry parser (`#tag` / `@today` / `@deadline ...`) lifted from the GTK binary into `atrium_core::quick_entry` so atrium-cli's `capture` reuses the exact same grammar.
- [ ] **Slice C — GTD audit fixes (deferred to v0.6.0).** First-run seed of a Weekly Review Perspective; Logbook day-grouping headers (Today / Yesterday / Last 7 Days / Older); `docs/gtd-patterns.md` documenting the `#waiting` user-tag idiom for "waiting on someone."
- [ ] **Slice D — Board view (deferred to v0.6.0).** Saved Perspectives gain a `renderer = 'board'` option that renders the filter expression as a kanban with tag-axis columns. Schema columns shipped at v0.5.0 (Slice A); UI is Slice D.
- [ ] **CLI bulk operations.** `atrium-cli complete --where 'is:overdue'` to bulk-toggle matched tasks. The pieces are all in place; just needs a flag-driven dispatcher.
- [ ] **Regression-script integration.** `scripts/regression.sh` should exercise atrium-cli end-to-end against a fixture DB so the architectural commitment ("every non-GUI surface stays CLI-testable") is verified at every release.

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
