# Atrium — Patch Notes

## v0.0.16 (2026-05-06) — Phase 6c: Quick Entry modal

`Ctrl+Alt+Space` opens a focused capture surface. Same parser as the bottom-of-list entry, lighter UI, drops straight into Inbox. Phase 6 is now complete for v0.1's purposes — true OS-global capture (the *zero-launch* version) lands with the Phase 20 `atriumd` daemon.

### What shipped

- **`atrium::quickentry::modal`** — `open(parent, worker)` builds an `adw::Window` (`transient_for(main)`, `set_modal(false)`, 480×120, non-resizable) holding an `AdwToolbarView` with an empty `AdwHeaderBar`, a single `gtk::Entry`, and a small dim hint label.
- **Esc dismisses** via a window-scoped `gtk::EventControllerKey` that intercepts `gtk::gdk::Key::Escape`. Enter commits via the `Entry::activate` signal — same idiom as the bottom-of-list entry.
- **`commit` runs the same parser** as Phase 6b: `parse(raw_input)` → `worker.create_task(NewTask)` → optional per-tag `worker.ensure_tag` + `worker.set_task_tags`. Empty input (no title and no tags) is silently ignored.
- **App action** `app.quick-entry` bound to `<Primary><Alt>space` (in-app accelerator). The hamburger menu's New section gained a "Quick Entry" entry alongside "New Task". `gtk::ShortcutsWindow` (`Ctrl+?` / `F1`) and `docs/keymap.md` both pick it up.
- **`AtriumWindow::worker_handle_for_quickentry`** public accessor — Quick Entry isn't a window method (it's its own surface), so it pulls the worker handle from the active window without a round-trip.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 103 tests still green (parser tested in 6b; modal interaction is end-to-end UX, exercised on every keystroke).

### Try it

```bash
cargo run -p atrium

# Anywhere in the app, press Ctrl+Alt+Space.
# Type "Buy milk #errand @tomorrow", press Enter.
# Open Inbox in the sidebar — the task is there with the tag attached
# and scheduled for tomorrow.
```

### What's deferred to Phase 20

- **OS-global Quick Entry shortcut** — the `Ctrl+Alt+Space` Atrium binds today is an in-app accelerator. The `atriumd` capture daemon (Phase 20) registers a real OS-level keybinding so capture works even when Atrium isn't focused or running. Spec §6 explicitly puts true zero-launch capture there; this slice gets us most of the experience for users who already have Atrium open.

### Phase 6 wrap-up

With 6a / 6b / 6c shipped (v0.0.14 → v0.0.15 → v0.0.16), every roadmap Phase 6 item except the cold-start daemon is checked. Tags are first-class everywhere: sidebar section + count badges, click-through tag pages, inline `#tag` syntax in both the bottom-of-list entry and Quick Entry, schema-NOCASE deduplication, F2/Ctrl+Shift+Delete reusing the existing CRUD actions, right-click context menus on tag rows.

### What didn't change

- Schema (Phase 1's `0001_initial.sql`), single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates in 6c.
- All Phase 0–6b features still work.

`VERSION`: 0.0.15 → 0.0.16 (patch — Phase 6c slice).

## v0.0.15 (2026-05-06) — Phase 6b: tag pills + inline `#tag` / `@date` parser

The bottom-of-list entry stops being a dumb title field. Type `Buy milk #errand @tomorrow` and the parser splits it into a clean title, a tag attachment, and a scheduled-for date — the worker creates the task, ensures the tag exists, and binds them in three round-trips.

### What shipped

- **Inline parser** at `atrium/src/quickentry/parser.rs`: `ParsedEntry { title, tag_names, scheduled_for, deadline }`. Tokens recognised:
  - `#word` → tag name (case-insensitive resolution at the worker)
  - `@today` / `@tomorrow` / `@someday`
  - `@yyyy-mm-dd` → `scheduled_for`
  - `@deadline yyyy-mm-dd` → `deadline`
  - Anything unrecognised stays in the title verbatim. **12 parser tests** including the combined-syntax case.
- **`Command::SetTaskTags`** + worker handler — wraps `DELETE FROM task_tag WHERE task_id = ?` and per-tag `INSERT` in one transaction; emits `TaskChanges{updated}` so the row's pill display refreshes.
- **`Command::EnsureTag`** + worker handler — idempotent "find by name (NOCASE) or create". Used by the inline parser to avoid spurious duplicate-name errors. Emits `LibraryChanges{tags_created}` only when the tag was actually new.
- **`WorkerHandle::set_task_tags(task_id, Vec<i64>)` / `ensure_tag(name)`** async methods.
- **`db::read::tag_names_per_task`** — single batched query returning `HashMap<i64, Vec<String>>`. Replaces what would have been per-row N+1 in the row factory.
- **`AtriumTask.tag_names_csv`** GObject property + `from_task_with_tags(task, tag_names)` constructor. The factory binds `tag-names-csv` to a small dim Label rendered after the title (e.g., `#errand #urgent`).
- **`task_list::TagMap`** type + new `replace_store_with_tags` and extended `apply_changes(..., tag_map)`. Window's `refresh_active_list` and `apply_task_changes` reload the tag map and feed it to both paths so pills stay current across worker deltas.
- **Bottom-of-list entry** now goes through the parser. Empty `parsed.title` (after stripping tags / dates) with no tags is treated as a no-op so accidental `Enter` on a blank field doesn't create empty tasks.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **103 tests** (up from 91): 32 in `atrium` (12 new for the parser), 71 in `atrium-core`. Worker tests for `SetTaskTags` and `EnsureTag` to follow in the v0.0.16 batch alongside Quick Entry.

### Try it

```bash
cargo run -p atrium

# In the bottom entry:
#   "Email João about Q3"           → plain task
#   "Buy milk #errand"              → tagged task
#   "Send report @tomorrow"          → scheduled
#   "File taxes @deadline 2026-04-15" → deadline
#   "Buy milk #errand @today"        → all of the above

# Click the new "errand" tag in the sidebar — Phase 6a's tag page
# now shows the tagged task.
```

### What's deferred (Phase 8 polish)

- **Per-row tag-editor popover** (click-to-edit autocomplete on each row's pill area). Edits today happen via the inline `#tag` syntax in the entry or via Quick Entry (6c). The popover is a polish UX win, not a v0.1 blocker.

### Coming in 6c (v0.0.16)

- **Quick Entry modal** (`Ctrl+Alt+Space`) — same parser, lighter UI surface, transient over the main window.
- **Worker tests** for `SetTaskTags` / `EnsureTag` round-trips.

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Every Phase 0–6a feature still works.

`VERSION`: 0.0.14 → 0.0.15 (patch — Phase 6b slice).

## v0.0.14 (2026-05-06) — Phase 6a: Tag CRUD + sidebar Tags section

Tags are first-class. The sidebar gains a Tags section, every tag has its own page (read-only at this slice), and create / rename / delete flow through the same worker / action / dialog plumbing Phase 5b laid down for areas and projects.

### What shipped

- **Domain types**: `NewTag`, `TagUpdate` (builder, `Option<Option<String>>` for nullable color). Re-exported from `atrium_core`.
- **Worker commands**: `CreateTag` / `UpdateTag` / `DeleteTag`. `WorkerHandle::create_tag` / `update_tag` / `delete_tag` async methods. Each emits `LibraryChanges{tags_*}` for the sidebar bridge.
- **`LibraryChanges` extended** with `tags_created` / `tags_updated` / `tags_deleted` (kept on the same channel as area/project changes — tags are library-shape).
- **Read functions**: `tag_by_id`, `list_tags` (NOCASE-ordered), `list_tasks_with_tag(id)` (joins through `task_tag`), `tag_ids_for_task(id)` (Phase 6b will use it for the pill editor), `count_open_per_tag` for the sidebar badges.
- **`ActiveList::Tag(i64)`** parallel to Project / Area. `task_matches` returns `false` for the Tag variant (membership lives on the join, not on Task) — `apply_changes` falls back to refresh-on-update, same pattern as Area.
- **Sidebar Tags section** populated from `list_tags` after the read pool attaches. Right-click context menu on each tag row (Rename / Delete) with destructive-action confirmation. The Phase 5 placeholder ("Tags · lands in Phase 6") is gone.
- **Tag count badges** in the sidebar (open-task count per tag, hidden when zero — same idiom as projects/areas).
- **`ActiveList::Tag(id)` content pane**: title renders as `#tagname`; empty state copy "{} is empty / No open tasks bear this tag."
- **Actions + accels**: `app.new-tag` triggers `prompt_create_tag`. `Ctrl+Shift+T` accelerator. Hamburger menu's New section gained "New Tag". `win.rename-active` / `win.delete-active` already-installed actions extended their match arms to handle `ActiveList::Tag(_)`.
- **Schema's NOCASE uniqueness** surfaces as a friendly behaviour: creating a tag with the same case-insensitive name as an existing one returns `DbError::Sqlite` (the constraint violation), which the UI maps to a console warning today and a toast in the Phase 8 polish pass.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **91 tests** (up from 87): 20 in `atrium`, 71 in `atrium-core`. **4 new** worker tests cover tag create / rename / delete (with library delta) and the NOCASE-unique constraint rejection.

### Try it

```bash
cargo run -p atrium

# Hamburger menu → New Tag → "errand" → Enter
# Click "errand" in the sidebar to see its tagged tasks (none yet —
# Phase 6b ships the pill editor that attaches tags to tasks).
# Right-click → Rename → "Errands" → F2 also works.
```

### Coming in 6b (v0.0.15)

- **Multi-tag pill editor** on task rows. Pills appear after the title; click opens a popover with autocomplete over existing tags. Worker gains `SetTaskTags(task_id, Vec<i64>)`.
- **Inline `#tag` syntax** in the bottom-of-list entry: typing `Buy milk #errand` creates the task and attaches the tag (creating the tag if needed).

### Coming in 6c (v0.0.16)

- **Quick Entry modal** (`Ctrl+Alt+Space` in-app — true OS-global shortcut deferred to Phase 20 daemon).
- **Inline parser** for `#tag` and `@today` / `@tomorrow` / `@yyyy-mm-dd` / `@deadline yyyy-mm-dd` inside Quick Entry.

### What didn't change

- Schema (Phase 1's `0001_initial.sql`), single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged.
- Every Phase 0–5.5 feature still works.

`VERSION`: 0.0.13 → 0.0.14 (patch — Phase 6a slice).

## v0.0.13 (2026-05-06) — Phase 5.5 polish: right-click context menus + sidebar selection

Two small UX wins. The first stretch items from Phase 5 close out before Phase 6 begins.

### What shipped

- **Right-click context menus** on sidebar rows:
  - Project rows: **Rename** / **Archive** / **Delete**.
  - Area rows: **Rename** / **Delete** (areas don't archive).
  - Implementation: `gtk::GestureClick::set_button(BUTTON_SECONDARY)` per row + a `gtk::PopoverMenu::from_model(&gio::Menu)`. The menu items target the existing `win.rename-active` / `win.archive-active-project` / `win.delete-active` actions; the gesture sets `active_list` to the right-clicked row's project / area before popping the menu, so the actions operate on the right entity.
- **Sidebar selection preserved across `LibraryChanges`**:
  - `apply_library_changes` now remembers the active list before rebuild, calls `select_sidebar_row_for(active)` after, and only falls back to Today when the active entity was actually deleted.
  - `select_sidebar_row_for(active)` walks the freshly-built `sidebar_targets` for the matching `Some(active)` and restores the highlight. No more "selection bounces to top of sidebar after every rename" flicker.

### What's deferred

- **Heading CRUD** is not in this patch. Schema-side it's been there since Phase 1 (`heading` table); display as section breaks within a project page is spec §5.1 territory. We're slipping it to **Phase 10** where the Builder Mode Inspector pane provides the natural editing surface — Simple Mode users don't need a Heading editor at v0.1, and a half-implemented one in Phase 5.5 would be more confusing than useful.
- **Smarter sidebar diff applier** that preserves scroll position (not just selection) — left for Phase 8 polish if perf demands. Current full-rebuild is sub-millisecond on a 100-area / 500-project tree.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 87 tests still green (no new tests added; right-click and selection-preserve are interactive UX, not unit-testable without a display).

### What didn't change

- Schema, single-writer worker, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- All Phase 0–5c features still work.

`VERSION`: 0.0.12 → 0.0.13 (patch — Phase 5.5 polish, no contract change).

## v0.0.12 (2026-05-06) — Phase 5c: count badges + drag-to-project

The sidebar gets numbers, and tasks move between lists by drag. Phase 5 closes here for the Simple Mode hierarchy work — Phase 6 is next, with tags and Quick Entry.

### What shipped

- **Count read functions** in `atrium-core::db::read`:
  - `CanonicalCounts` struct + `count_open_canonical(today)` — six SELECTs in one call, returning open-task counts for Inbox / Today / Upcoming / Anytime / Someday / Logbook.
  - `count_open_per_project()` — `HashMap<i64, i64>` from a single `GROUP BY` query.
  - `count_open_per_area()` — `HashMap<i64, i64>` aggregating across the area's projects via the `task` ↔ `project` join.
- **Sidebar count badges**:
  - Every sidebar row (canonical, area, project) now renders an optional integer badge on the right. Hidden when the count is zero per the Phase 5 design call — visual calm over OmniFocus-style always-visible.
  - Badges use the `numeric` CSS class — tabular figures (set up in Phase 3 typography) keep digits from dancing.
  - `apply_badge_label(label, count)` flips visibility based on count; `refresh_canonical_badges` / `refresh_dynamic_badges` walk the stored label refs (no full sidebar rebuild on every TaskChanges).
  - The window's imp struct gained `canonical_counts` / `project_counts` / `area_counts` (data) and `canonical_badges` / `project_badges` / `area_badges` (widget refs). Three small `RefCell`s on top of Phase 5b's caches.
- **Drag-to-project**: every project sidebar row is now a `GtkDropTarget` accepting `i64` (the task id provided by Phase 4.5's per-row `GtkDragSource`). On drop the window calls `worker.update_task(TaskUpdate::new(task_id).project(Some(project_id)))` and the `TaskChanges{updated}` delta drops the task from the source list and the `LibraryChanges` re-emit isn't needed (no library mutation). The Inbox row is also a drop target — dropping a task there sets `project_id = NULL` to unfile it.
- **Live updates**: `apply_task_changes` and `apply_library_changes` both call `refresh_counts() + refresh_canonical_badges() + refresh_dynamic_badges()` — every mutation that could move a count refreshes the badges. The library bridge already triggered a sidebar rebuild; that path now picks up fresh counts on the new rows automatically.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **87 tests** (up from 83): 20 in `atrium`, 67 in `atrium-core`. **4 new** read tests cover `count_open_canonical` distribution (with the spec-correct expectation that scheduled-but-unfiled tasks count in Inbox AND Today), per-project grouping, and per-area aggregation.

### Try it

```bash
cargo run -p atrium

# Generate fixtures via the --debug menu, then:
# - Watch sidebar badges populate as you toggle complete on tasks.
# - Drag any Inbox task onto a sidebar project — it moves there
#   and the badge ticks up.
# - Drag any project task onto Inbox — it unfiles back to Inbox.
```

### Phase 5 wrap-up

With 5a / 5b / 5c shipped, Phase 5 of the roadmap is complete. The Simple Mode hierarchy is live: Areas + Projects nested in the sidebar with badges, every canonical list reads from real data, area / project / heading-pending CRUD via menu and keyboard, drag-to-project, archive-with-cascade, FK-aware delta emission. Phase 6 (tags + Quick Entry capture modal) is next.

### Coming in Phase 5.5 patch

- Right-click context menus on sidebar rows (Rename / Archive / Delete).
- Heading CRUD + sectioned project pages (skipped from Phase 5 to keep slices tight).
- Smarter sidebar diff applier that preserves selection / scroll across `LibraryChanges`.

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Every Phase 0–5b feature still works.

`VERSION`: 0.0.11 → 0.0.12 (patch — Phase 5c slice). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.11 (2026-05-06) — Phase 5b: Area / Project CRUD + LibraryChanges

The hierarchy stops being read-only. Areas and projects can be created, renamed, archived, and deleted from the menu and keyboard, with confirmations on destructive operations. The worker grew a parallel `LibraryChanges` channel so the sidebar updates immediately on every mutation, and `TaskUpdate` gained `project_id` so tasks can be moved between projects.

### What shipped

- **New domain types** (`atrium-core::domain`): `NewArea`, `AreaUpdate` (builder); `NewProject` (with `unfiled` / `in_area` constructors), `ProjectUpdate` (builder with `Option<Option<i64>>` for nullable `area_id` and `review_interval_days`). `TaskUpdate` extended with `project_id: Option<Option<i64>>` + `.project(Option<i64>)` builder method, so `update_task` can move a task to a project (or back to Inbox via `Some(None)`).
- **`LibraryChanges`** (`atrium-core::db::changes`): parallel delta type carrying `areas_created` / `areas_updated` / `areas_deleted` / `projects_created` / `projects_updated` / `projects_deleted`. `merge` for coalescing matches `TaskChanges`. Sidebar listens here; the task list keeps listening on `TaskChanges` — separate channels keep subscribers focused.
- **Worker commands** (Phase 5b set): `CreateArea`, `UpdateArea`, `DeleteArea`, `CreateProject`, `UpdateProject`, `ArchiveProject`, `DeleteProject`. Each carries its own `oneshot::Sender` and gets a `WorkerHandle` async method. `spawn_worker` now returns `(WorkerHandle, mpsc::UnboundedReceiver<TaskChanges>, mpsc::UnboundedReceiver<LibraryChanges>)`.
- **Cascade-aware delta emission**:
  - `DeleteArea` reads the area's projects before the SQL fires, then emits `LibraryChanges{areas_deleted, projects_updated}` so the sidebar reflects the FK-driven `area_id = NULL` unfiling.
  - `DeleteProject` reads the project's tasks before deletion, then emits both `LibraryChanges{projects_deleted}` and `TaskChanges{deleted}` so list views drop the cascade-deleted rows.
  - `ArchiveProject` runs `archived_at = now` *and* `completed_at = now` on open tasks inside a single transaction (per design call — Things-3 behaviour), then emits both deltas with the right `status_changed` set.
- **Window plumbing**:
  - `bridge_library_changes` consumes the new receiver via `glib::MainContext::spawn_local` and dispatches to `window.apply_library_changes`.
  - `apply_library_changes` rebuilds the dynamic sidebar from scratch (small enough for v0.1) and falls back to Today if the active list referenced a deleted project / area.
  - `prompt_create_area` / `prompt_create_project` / `prompt_rename_active` / `prompt_delete_active` / `archive_active_project` methods. Each opens an `adw::AlertDialog` (`prompt_for_text` for entry, `prompt_confirm_destructive` for confirms with `ResponseAppearance::Destructive`).
  - New project defaults to the active area when one is selected.
- **Actions + accels** (full keymap reference: `docs/keymap.md`):
  - `app.new-area` — `Ctrl+Shift+A`
  - `app.new-project` — `Ctrl+Shift+N`
  - `win.rename-active` — `F2`
  - `win.delete-active` — `Ctrl+Shift+Delete`
  - `win.archive-active-project` — menu only (destructive enough that we don't bind a default accel)
- **Hamburger menu** gained a Library section ("Rename Active", "Archive Project", "Delete Active") and the New section grew "New Project" / "New Area".
- **`gtk::ShortcutsWindow`** (`Ctrl+?` / `F1`) gained a Library group surfacing the new shortcuts.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **83 tests** (up from 76): 20 in `atrium`, 63 in `atrium-core`. **7 new** worker tests cover area create/rename/delete (with project-unfile cascade), project create, archive (with auto-complete-open-tasks), delete (with cascade-task-delete), and `update_task(project)` for move-to-project.

### Try it

```bash
cargo run -p atrium

# Hamburger menu → New Area → "Personal" → Enter
# Click "Personal" in the sidebar
# Hamburger menu → New Project → "Errands" → Enter
# (creates Errands inside Personal)
# F2 to rename, Ctrl+Shift+Delete to delete
# Hamburger menu → Archive Project to archive
```

### Coming in 5c (v0.0.12)

- Sidebar count badges (open task count per list/project/area, hidden when zero per design call).
- Drag tasks onto sidebar projects to move them.
- Right-click context menus on sidebar rows (rename / delete / move).

### Coming in Phase 5.5 patch

- Heading CRUD + sectioned project pages.
- Smarter sidebar diff applier (preserve scroll/selection across `LibraryChanges`).

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- All Phase 0–5a features still work — every list, sidebar navigation, drag-reorder, bottom-of-list entry, completion toggle, inline edit, the keymap, ShortcutsWindow.

`VERSION`: 0.0.10 → 0.0.11 (patch — Phase 5b slice). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.10 (2026-05-06) — Phase 5a: sidebar hierarchy + remaining lists

The first slice of Phase 5. Sidebar grows beyond the six canonical lists into a real Areas → Projects hierarchy, and the four lists Phase 4 stubbed (Upcoming / Anytime / Someday / Logbook) all render real data now. Every list is one read function in `atrium-core::db::read`, all built on the same `gio::ListStore<AtriumTask>` machinery from Phase 4.

### What shipped (Phase 5a)

- **`atrium-core::db::read` additions** — `list_anytime(today)`, `list_someday`, `list_upcoming(today)`, `list_logbook`, `list_project(id)`, `list_area(id)` (joins `task` with `project` to aggregate across an area's projects), `list_areas`, `list_projects`. Each one a small, indexed query; `list_logbook` orders by `completed_at DESC`. **11 new tests** cover Someday-sentinel exclusion, deferred-task handling, archived-project exclusion, area aggregation across projects, NULL/non-NULL area_id grouping. Total core tests: **56** (up from 45).
- **`ActiveList::Project(i64)` and `ActiveList::Area(i64)`** added to the existing enum. `task_matches` extends to all variants — Inbox / Today / Upcoming / Anytime / Someday / Logbook fully predicate-checked; Project matches by `project_id`; Area returns `false` (lookup needs project→area mapping that isn't on `Task`, so the diff applier falls back to refresh-on-update for that case). The old `implemented_in_phase_4` gate is gone — every variant is implemented.
- **`AtriumWindow` sidebar rewrite** — `build_sidebar` ships canonical rows on construction (Phase 4 behaviour); `rebuild_dynamic_sidebar` runs from `attach_data_layer` and appends Areas + Projects + Unfiled + Tags-placeholder sections from the read pool. Non-selectable header rows separate sections without breaking `GtkListBox` arrow-key navigation. Project rows indent under their area. The window holds `sidebar_targets: Vec<Option<ActiveList>>` aligned with row indices, plus `project_titles` and `area_titles` `HashMap<i64, String>` caches for content-pane title resolution.
- **`refresh_active_list` dispatches all variants** — every list type maps to its `db::read::*` function. The content pane title now flows through `title_for(active)` which consults the caches for Project/Area; the canonical lists return their static label.
- **Empty states for every list** — distinct copy per variant ("Inbox is empty / Press Ctrl+N", "Nothing today", "No anytime tasks", "Logbook is empty / Completed tasks accumulate here, newest first", project-named for `Project(_)` etc).
- **`Ctrl+1..6` still works** as in Phase 4 — limited to the canonical lists. Project / area shortcuts are reserved for Phase 5b's CRUD pass.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **76 tests** (up from 65): 20 in `atrium`, 56 in `atrium-core`.
- **Live launch confirmed**: `cargo run -p atrium` opens the window, sidebar reads areas/projects from the DB, click switches the content pane.

### Try it

```bash
# Generate a fixture DB to see real areas/projects
rm -rf ~/.local/share/atrium/atrium.db*
cargo run -p atrium -- --fixture small  # 5 areas, 50 projects, 1000 tasks

# Open the window — every list works now
cargo run -p atrium

# Click any area in the sidebar to see aggregated tasks across its projects.
# Click any project to see that project's open tasks.
# Click Upcoming/Anytime/Someday/Logbook — all populated.
```

### Coming in 5b (v0.0.11)

- `Command::CreateArea` / `RenameArea` / `DeleteArea` and same for Project, Heading.
- New keyboard shortcuts: `Ctrl+Shift+N` for new project, right-click context menus.
- Hamburger menu items for "New project" / "New area".
- `LibraryChanges` parallel delta type for live sidebar refresh.

### Coming in 5c (v0.0.12)

- Sidebar count badges (open task count, hide when zero per design call).
- Project completion → archive workflow with auto-complete-tasks-with-toast-cancel.
- Drag tasks onto sidebar projects to move them.

### What didn't change

- Schema, single-writer worker, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- All Phase 0–4.5 features still work — Inbox, Today, drag-reorder, bottom-of-list entry, completion toggle, inline edit, the keymap, ShortcutsWindow, the menu, fixture generator, etc.

`VERSION`: 0.0.9 → 0.0.10 (patch — Phase 5a slice). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.9 (2026-05-06) — Phase 4.5 patch: drag-to-reorder + bottom-of-list entry

The two stretch items v0.0.8 explicitly slipped land here. Pure UI work; no schema or contract changes.

### What shipped

- **Bottom-of-list inline-create entry** (`data/window.ui`, `atrium/src/ui/window.rs`): a `GtkEntry` ("Add task…") sits below the `GtkListView`. `Ctrl+N` (and the `+` toolbar button) now focuses this entry instead of immediately spawning a "New task" placeholder. Enter commits → `worker.create_task(NewTask)`; the entry clears so rapid capture stays fluid. This is the Things-3 idiom — type the title, hit Enter.
- **Drag-to-reorder within Inbox** (`atrium/src/ui/task_list.rs`, `window.rs`):
  - `build_factory` gained an `on_reorder` callback parameter. Every row now carries a `gtk::DragSource` (provides the task id as `i64` content) and a `gtk::DropTarget` (accepts `i64`, calls `on_reorder(src_id, dest_id)` on drop).
  - `window::handle_reorder` snapshots the active store's positions, finds source and destination, computes a midpoint (`(dest.pos + neighbour.pos) / 2.0`) so the source lands adjacent to the destination, and fires one `worker.update_task(TaskUpdate::new(id).position(new))`. Inbox-only — the other lists return early since they auto-sort by date.
  - `task_list::sort_by_position` re-sorts the `gio::ListStore` after `apply_changes`, so the reorder becomes visible as soon as the worker's `TaskChanges` delta applies. Same sort runs after every full-list reload.
- **Roadmap Phase 4 boxes updated**: drag-to-reorder and inline-create rows now check off, with the implementation details captured.

### Caveats / known limitations

- **No drag visual feedback yet** — the drop target accepts the drop but there's no highlight on hover or insertion line. Works functionally; polish (cursor change, drop-position indicator, animated row movement) lands with the broader Phase 8 polish pass.
- **Drag-reorder respects active-list semantics**: dragging in Today / Logbook / etc. is silently ignored (those lists auto-sort by date or completion time, not user-driven position).

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 65 tests still green (unchanged from v0.0.8; the new code path is exercised end-to-end at runtime via the window).
- **Live launch confirmed**: `Ctrl+N` focuses the entry; typing + Enter creates a task; dragging an Inbox row onto another reorders it.

### What didn't change

- Schema, single-writer worker, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- All Phase 0–4 features still work.

`VERSION`: 0.0.8 → 0.0.9 (patch — Phase 4.5 stretch landings, no contract changes). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.8 (2026-05-06) — Phase 4: Simple Mode — Inbox & Today + Calendar Month View on roadmap

The first phase Atrium becomes *usable*. `cargo run -p atrium` opens the window, real tasks render in the sidebar's Inbox / Today views, completion toggles, inline title edits, and `Ctrl+N` task creation all flow through the single-writer worker and reach the UI via the `TaskChanges` bridge that landed in Phase 3.

Plus a roadmap addition: **Phase 12.5 — Calendar Month View (Builder)** — the traditional month-grid view that complements Forecast's day-block layout.

### What shipped (Phase 4)

- **`db::read::list_today(today)`** in `atrium-core` per spec §4.2 — open tasks scheduled-or-deadline ≤ today, not deferred, Someday sentinel explicitly excluded (the lexical-sort bug the comment in `read.rs` calls out). 8 new tests cover scheduled / overdue / Someday / completed / deferred / deadline-only edge cases.
- **`AtriumTask`** GObject (`atrium/src/ui/task_object.rs`): `id` / `uuid` / `title` / `note` / `completed` / `schedule_label` / `deadline_label` / `position` exposed as `glib::Properties` for bidirectional widget binding. `from_task` / `refresh_from` keep it in sync with `atrium_core::Task`.
- **`task_list` module** (`atrium/src/ui/task_list.rs`): `ActiveList` enum (Inbox / Today / Upcoming / Anytime / Someday / Logbook) with `task_matches(task, today)` predicate mirroring the spec's filter rules. `build_factory(on_toggle, on_rename)` produces a `gtk::SignalListItemFactory` that builds rows imperatively (checkbox + `GtkEditableLabel` title + schedule pill + deadline pill). `replace_store` for full reloads on list switch; `apply_changes` for in-place TaskChanges diff (created / updated / deleted / status_changed handled per active-list membership).
- **Window subclass rewrite** (`atrium/src/ui/window.rs`): sidebar built programmatically with click + selection-changed handlers; `AdwNavigationSplitView` content pane hosts a `gtk::Stack` between an `AdwStatusPage` empty state and the `gtk::ListView`. `attach_data_layer(worker, pool)` plugs in after `boot_data_layer` succeeds; `apply_task_changes` runs the diff applier on the active store.
- **TaskChanges bridge wired to the window**: `glib::MainContext::default().spawn_local` consumes the worker's `mpsc::UnboundedReceiver<TaskChanges>` and calls `window.apply_task_changes` on the GTK thread. Window weak-ref keeps the bridge alive only as long as the window exists.
- **CRUD plumbing**: row toggle → `worker.toggle_complete`; inline title edit → `worker.update_task(TaskUpdate::title)`; `Ctrl+N` → `worker.create_task(NewTask)`; `Delete` → `worker.delete_task` on focused row; `Space` → `worker.toggle_complete` on focused row. All async, dispatched through `spawn_local` on the GTK thread.
- **Comprehensive keymap** centralised in `main.rs::install_accels`: `Ctrl+N` (new), `Ctrl+1..6` (jump to lists), `Ctrl+Q` (quit), `Ctrl+?` / `F1` (shortcuts dialog), `Space` / `Delete` (focused-row actions). Stub bindings reserved for `Ctrl+Z` / `Ctrl+F` / `Ctrl+,` (undo / search / preferences — wired in Phase 7+).
- **`gtk::ShortcutsWindow`** (`atrium/src/ui/shortcuts.rs`) loaded from inline XML; opens via `Ctrl+?` / `F1` / hamburger menu. Three sections: General / Navigation / List actions.
- **`docs/keymap.md`** — written reference for the keymap, Builder Mode growth sketched (`Ctrl+I`, `Ctrl+Shift+F`, `Ctrl+Shift+M`, etc.), discovery rules, and the four-edit checklist for adding a shortcut.
- **Empty states**: per-list `AdwStatusPage` swapped via `gtk::Stack` — "Inbox is empty / Press Ctrl+N", "Nothing today", placeholder for Phase 5+ lists.

### What shipped (roadmap addition)

- **Phase 12.5 — Calendar Month View (Builder)** added to `roadmap.md` between Forecast (Phase 12) and Review (Phase 13). 8-bullet item: month-grid widget with task-count badges, drag-to-reschedule between days, click-day-to-filter, today indicator, month nav with `Ctrl+Shift+M`, narrow-window collapse to week strip, Builder-only sidebar entry, tests for date-filter correctness across month boundaries / DST / leap day. Doesn't disturb the v0.2 phase numbering — sub-phase under Phase 12's calendar-view domain.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **65 tests** (up from 49): 20 in `atrium` (CLI parsing × 6, debug pane × 1, typography × 3, window menu × 3, ScheduledFor × 0 [moved], task_object × 3, task_list × 4) + 45 in `atrium-core` (37 prior + 8 new for `list_today`).
- **Live launch confirmed**: `cargo run -p atrium` opens the window with the sidebar populated, Today selected, and (when fixture data exists in the DB) tasks rendering in the content pane.

### Deferred to Phase 4.5 patch

The original Phase 4 plan included two stretch items that didn't make it into v0.0.8 but are explicitly slipped (not dropped):

- **Drag-to-reorder within Inbox.** `update_task` already accepts a `position` field; the remaining work is binding `GtkDragSource` + `GtkDropTarget` on rows and computing midpoint positions. Lands in v0.0.9 (Phase 4.5 patch — pure UI work, no schema or contract impact).
- **Bottom-of-list inline-create entry.** Today, `Ctrl+N` creates a task titled "New task" that the user immediately renames via the existing inline editor — functional but not the Things-3 idiom. The dedicated entry widget that focuses on `+` lands in 4.5.

### What didn't change

- Phase 0–3 surfaces unchanged. `--debug`, `--fixture <scale>`, `--version`, `--help` all still work.
- v0.1 dependency set: `chrono` enabled in `atrium`'s `[dependencies]` (already locked in workspace deps from Phase 0). No new crates introduced.
- Schema (`0001_initial.sql`) unchanged.
- Mode-as-view, single-writer worker, vault projection, debug-first, dependency discipline, release discipline.

`VERSION`: 0.0.7 → 0.0.8 (patch — Phase 4 ship + roadmap addition). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.7 (2026-05-06) — Phase 3: Application Shell

The first phase Atrium becomes lookable. `cargo run -p atrium` opens a real `AdwApplicationWindow`, the bundled type system installs on first run, the worker plugs into a tokio runtime that coexists with glib's main loop, and `TaskChanges` deltas reach the UI thread via the canonical `spawn_local` bridge.

### What shipped

- **GTK + libadwaita application** (`atrium/src/main.rs` rewrite): `adw::Application` with `io.github.virinvictus.atrium` ID. Tokio multi-thread runtime built once via `OnceLock<Runtime>`, lives until exit. `connect_startup` installs fonts and CSS; `connect_activate` opens the DB, spawns the worker, bridges `TaskChanges` to the GTK main loop, and presents the window.
- **`AtriumWindow`** (`atrium/src/ui/window.rs` + `data/window.ui`): `AdwApplicationWindow` subclass via `gtk::CompositeTemplate`. `AdwToolbarView` + `AdwHeaderBar` + hamburger menu over `AdwNavigationSplitView`. Sidebar lists the six canonical Simple-Mode rows (Inbox / Today / Upcoming / Anytime / Someday / Logbook); content pane is a placeholder `AdwStatusPage` that Phase 4 replaces with real list views.
- **About dialog** (`atrium/src/ui/about.rs`): `adw::AboutDialog` with version, MIT, repo + issue URLs, designer/developer credits, an acknowledgement section (Things 3, OmniFocus, Org-mode, NetNewsWire), and a bundled-fonts legal section.
- **GSettings schema** (`data/io.github.virinvictus.atrium.gschema.xml`): `mode` enum (Simple/Builder), `window-width`/`window-height`/`window-maximized`, `sidebar-width`, `quick-entry-shortcut` (declared, bound in Phase 6).
- **`atrium/build.rs`**: runs `glib-compile-schemas` against `data/`, exports `ATRIUM_GSCHEMA_DIR` and `ATRIUM_DATADIR` via `cargo:rustc-env` so `cargo run` finds the compiled schema and the data tree without needing `meson install`.
- **Mode action**: stateful `gio::SimpleAction` `app.mode` (parameter `s`) writes to GSettings; state mirrors back. Builder Mode is wired but currently identical to Simple Mode visually — Inspector / Forecast / Review are Phase 10+.
- **Quit action** with `<Primary>q` accel.
- **Window state persistence**: width / height / maximized read from GSettings on construction, written on `close-request`. Verified: resize, close, reopen → same size.
- **Light/dark follow-system** via libadwaita's default style manager.
- **Typography foundation** (spec / roadmap landing): Inter Variable + Italic (UI), Source Serif 4 Variable Roman + Italic (note bodies), JetBrains Mono Variable + Italic (debug pane / monospace) — all SIL OFL 1.1, bundled at `data/fonts/` (~4 MB total). Installed to `$XDG_DATA_HOME/fonts/atrium/` on first run with `fc-cache` refresh (proven Viaduct pattern, idempotent). `data/style.css` loaded via `gtk::CssProvider`; tabular figures default-on for `.numeric` selectors. Fallback to system fonts if the bundled files are missing — non-fatal.
- **`--debug` integration**: when set, the hamburger menu gains a Debug section with a fixture-generator submenu (Small / Medium / Large / Stress). Activations route through `tokio::task::spawn_blocking` so the GTK thread isn't blocked.
- **`TaskChanges` UI bridge**: `glib::MainContext::default().spawn_local` consumes `mpsc::UnboundedReceiver<TaskChanges>` directly. tokio's mpsc receiver futures use runtime-agnostic wakers, so glib's executor drives them without `tokio-stream`, `async-channel`, or any other extra crate.
- **Worker handle stash**: spawned `WorkerHandle` lives on a thread-local on the GTK main thread (`thread_local!` `RefCell<Option<WorkerHandle>>`). Phase 4+ pulls it via accessor when the UI starts sending commands.
- **Meson updates**: installs the gschema XML to `$datadir/glib-2.0/schemas/` (with post-install `glib-compile-schemas`), `data/fonts/` to `$datadir/atrium/fonts/`, `data/style.css` to `$datadir/atrium/`. `ATRIUM_DATADIR` exported into the cargo build environment so the runtime resolver lands on the install path.

### Defaults captured (from Phase 3 plan)

- **tokio + glib coexistence**: glib owns the main thread, tokio runs in a separate multi-thread runtime — the canonical GTK4-rs pattern Viaduct uses.
- **CompositeTemplate `.ui` files** for window structure (sidebar list rows declarative); menu built imperatively in code so the `--debug` section can be conditional.
- **Fonts via fontconfig** (Viaduct's pattern) instead of in-process `pango::FontMap::add_font_file`. Simpler and proven.
- **`build.rs` for GSettings compile** so `cargo run` works without manual install.
- **`adw::AboutDialog`** with explicit acknowledgements section (portfolio-piece detail).

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **49 tests** (up from 37): 12 in `atrium` binary (CLI parsing × 6, debug pane × 1, typography × 3, window menu × 2) + 37 in `atrium-core` (unchanged from v0.0.6).
- **Live launch test**: `cargo run -p atrium` opens the window cleanly. Fonts install to `~/.local/share/fonts/atrium/`, `fc-cache` succeeds, stylesheet applies, DB opens at `~/.local/share/atrium/atrium.db`, worker starts. `--debug` adds the debug pane stub. `--fixture small` still bypasses GTK and exits with the summary.

### What's deferred

- **`heaptrack` baseline** (roadmap Phase 3 closing item): heaptrack isn't installed on the development machine right now. Will land as a `docs/perf.md` entry once Brandon installs it (`sudo dnf install heaptrack`); this is purely measurement, no code impact. Phase 3's idle binary opens a window with no task data — well below the §8 80 MB target by inspection, but the empirical number is missing.

### What didn't change

- Phase numbering, mode-as-view, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- Schema (`0001_initial.sql` from Phase 1) is unchanged.
- v0.1 dependency set: `gtk` / `adw` / `tokio` enabled in `atrium`'s `[dependencies]` (already locked in workspace deps from Phase 0). No new crates introduced.

`VERSION`: 0.0.6 → 0.0.7 (patch — Phase 3 ship). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.6 (2026-05-05) — Phase 2: Data Layer (Single-Writer Worker)

The architectural-commitment-2 pattern lands. Domain types, single-writer worker, read-only pool, IO instrumentation, `TaskChanges` deltas. UI doesn't exist yet but the headless data layer it'll plug into is real and tested.

### What shipped

- **Domain types** (`atrium-core::domain`): `Task` (full row), `NewTask` (insert input with `inbox()` helper), `TaskUpdate` (builder-style partial update), `Project`, `Area`, `Tag`, `Heading`. All `serde`-derived. `ScheduledFor` enum (`Someday | Date(NaiveDate)`) with custom `rusqlite::ToSql` / `FromSql` impls so the schema's "ISO date OR `__someday__` sentinel" is type-safe in Rust — `parse()` / `Display` round-trip cleanly.
- **`TaskChanges`** (`atrium-core::db::changes`): `{ created, updated, deleted, status_changed }` per spec §3.2. `merge()` folds deltas for the coalescer.
- **`Command`** enum (`atrium-core::db::command`): Phase 2 set is `CreateTask`, `UpdateTask`, `ToggleComplete`, `DeleteTask`. Each variant carries its own `oneshot::Sender` for the per-call result. Project / area / tag / heading commands follow naturally in Phase 5 with their UI.
- **Single-writer worker** (`atrium-core::db::worker`): a dedicated `tokio` task owns the writable connection. `WorkerHandle` is `Clone`; the worker shuts down when the last handle drops. Spawn returns `(WorkerHandle, mpsc::UnboundedReceiver<TaskChanges>)`. Position auto-computed as `MAX(position) + 1` per sibling list (parent → children, project → tasks, inbox).
- **Read-only connection pool** (`atrium-core::db::read_pool`): `Mutex<Vec<Connection>>` with lazy on-demand connection creation. `PRAGMA query_only = ON` per connection — SQLite enforces read-only at the engine level. `with(|conn| ...)` API. Soft cap on idle connections; excess dropped on release.
- **Read functions** (`atrium-core::db::read`): `task_by_id`, `list_inbox`, `list_all_tasks`, `count_tasks` — free functions taking `&Connection` so they compose with both worker and pool connections.
- **IO instrumentation** (spec §3.4): rusqlite `Connection::profile` callback routes every SQL statement (text + elapsed micros) through `tracing` at TRACE level. `RUST_LOG=trace` (or scoped `atrium_core::db=trace`) reveals each statement. Required adding rusqlite's `trace` feature — feature flip on an existing locked dep, no new crate.
- **`DbError::WorkerClosed`** for "command sent but channel closed" / "responder dropped"; **`DbError::NotFound`** for "no row matched."
- **`atrium-core` lib exports:** `TaskChanges`, `WorkerHandle`, `spawn_worker`, all domain types, all errors flow through the crate root.

### Defaults captured (from Phase 2 plan)

- **`ScheduledFor` as enum, not string** — schema's "TEXT (ISO date OR sentinel)" maps to a sum type in Rust. Type-safe at the boundary; round-trips through rusqlite via custom `ToSql`/`FromSql`.
- **Worker channels:** bounded mpsc (capacity 64) for commands so backpressure surfaces in `WorkerHandle::*` awaits; unbounded mpsc for `TaskChanges` so a slow UI subscriber never stalls writes.
- **Per-variant `oneshot::Sender`:** boilerplate-y `WorkerHandle` methods but each operation's response type is statically checked. No `CommandResult` super-enum.
- **Read pool: lazy on-demand**, soft cap on idle, no hard cap on total opens. Pragmatic for v0.1 single-user concurrency; bounded variant can land later if perf demands.
- **IO instrumentation always-on, gated by `RUST_LOG`.** Trace level so default INFO logging stays clean. Phase 3 will surface the stream visually in the debug pane.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **37 tests** (up from 27): 30 in `atrium-core` (paths × 3, db schema × 12, fixtures × 4, ScheduledFor × 4, ReadPool × 3, TaskChanges × 2, Worker × 8) + 7 in `atrium`. Worker tests cover create/update/toggle/delete round-trips, NotFound on missing id, position auto-increment, Someday round-trip, clean worker shutdown on handle drop.

### What didn't change

- Phase 0 binary surface: `--debug` and `--fixture <scale>` work exactly as before. The Phase 2 worker is library-only; Phase 3 will wire it into the GTK + tokio main loop.
- v0.1 dependency set: tokio enabled in `atrium-core`'s `[dependencies]` (already locked in workspace deps from Phase 0). `rusqlite` got the `trace` feature added — no new crate, feature flip only.
- Schema (Phase 1's `0001_initial.sql`) is the contract; no changes there per "no mid-v0.1 schema changes."

### Open / deferred

- **`glib::MainContext::channel` bridge** (roadmap Phase 2 item): explicitly slipped to Phase 3 since it requires GTK on the binary side. Phase 2 ships `mpsc::UnboundedReceiver<TaskChanges>`; Phase 3 spawns the bridging glue.

`VERSION`: 0.0.5 → 0.0.6 (patch — Phase 2 ship). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.5 (2026-05-05) — Roadmap addition: Beyond 1.0

Roadmap horizon extended past Phase 20.

### What changed

- **`roadmap.md` — new "Beyond 1.0" section** after Phase 20. Captures **Toward 2.0 — Full TUI mode (`atrium-tui`)** as the first post-1.0 horizon item: keyboard-first terminal frontend over the same headless core, three-pane layout, Simple / Builder mode reused, FTS5 search via `/`, dependency check on a TUI crate (likely `ratatui`) to land before adoption. Not committed to a phase number yet.
- The workspace split done in Phase 0 (`atrium-core` headless + `atrium` GTK binary) is the load-bearing decision that makes this cheap — `atriumd` (Phase 20) is already a second consumer; a TUI would be the third.
- Items still explicitly out of scope per spec §9 (network sync, mobile/web, multi-user, time tracking, calendar event creation, AI) remain out of scope and are *not* on the horizon either.

`VERSION`: 0.0.4 → 0.0.5 (patch — roadmap refinement, no code change).

## v0.0.4 (2026-05-05) — Phase 1: Schema Design

The OmniFocus superset lives in SQL. Migration `0001_initial.sql` ships once and stays — backwards-compatible migrations begin at v0.2 per CLAUDE.md commitment.

### What shipped

- **`atrium-core/src/db/migrations/0001_initial.sql`** — full schema per spec §4: `area`, `project`, `heading`, `task`, `tag`, `task_tag`. Every Builder-only column (`defer_until`, `estimated_minutes`, `sequential`, `review_interval_days`, `last_reviewed_at`, `repeat_rule`, `parent_id`) exists from day one. `task_fts` virtual table (FTS5, content='task', tokenize='unicode61') with insert/update/delete sync triggers. `modified_at` triggers on all five timestamped tables, with `WHEN old = new` clauses that prevent recursion *and* let explicit writes survive (import-time timestamp preservation).
- **`atrium-core::db::open(path)`** — opens (or creates) the database, ensures `$XDG_DATA_HOME/atrium/` exists, applies pragmas (`WAL`, `synchronous=NORMAL`, `temp_store=MEMORY`, `mmap_size=256 MB`, `foreign_keys=ON`), runs pending migrations.
- **`atrium-core::db::migrations::migrate`** — `PRAGMA user_version`-driven runner. Each migration runs inside a transaction; failed migrations roll back without leaving the schema half-applied. Idempotent.
- **`atrium-core::db::fixtures`** — stress generator at four scales (`Small` 1K, `Medium` 10K, `Large` 50K, `Stress` 100K). Realistic distribution (~20 tasks per project, ~14 % inbox-only, mix of scheduled / completed / Someday, ~30 % tagged, unicode-hostile titles). Wired into `--fixture <scale>` CLI flag.
- **CLI surface expanded:** `--fixture small|medium|large|stress` triggers fixture generation against `$XDG_DATA_HOME/atrium/atrium.db`; default behaviour now opens the DB and runs migrations on every invocation.
- **`DbError` fleshed out:** `Sqlite(rusqlite::Error)` via `From`, `Migration { version, source }` for nicer reporting.
- **`docs/schema.md`** — Mermaid ER diagram, per-table/column rationale, cross-referenced to spec §4 (contract) and `0001_initial.sql` (canonical SQL).

### Design calls captured here

- **`uuid` crate added** (sign-off granted in Phase 1 plan). Pure-SQL UUID v4 generation was the alternative; rejected on ergonomics — UUIDs would be opaque to Rust code without a roundtrip.
- **Hard-delete only** — no `deleted_at` columns. Logbook holds completed tasks; deleted tasks are gone forever. Soft-delete can land in v0.2 if it earns its keep.
- **`unicode61` tokenizer** for FTS5 — predictability beats fuzzy matching for short task titles. English stemming (`porter unicode61`) considered for v0.2 as an option.
- **Basic stress generator** now (vs skeleton + flesh-out later). Realistic-shape generator runs at 1K-100K scales without being elaborate.
- **Datetimes stay TEXT (ISO 8601)** — INTEGER unix can't represent the `'__someday__'` sentinel for `scheduled_for` (spec §4.2), conflates date vs datetime granularity, and forces conversion on every Org / VTODO interop call.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **20 / 20** (up from 7): 16 in `atrium-core` (paths × 3, db schema/triggers/FK/cascade × 9, fixtures × 4) + 7 in `atrium` (CLI parsing × 6, debug pane × 1). Includes `migration_is_idempotent`, `explicit_modified_at_survives_trigger`, `tag_name_is_case_insensitive_unique`, `project_cascade_deletes_tasks`, `area_set_null_on_delete`, FTS sync on insert/update/delete.
- **Perf smoke** (release build, T14s AMD Gen 6): `--fixture small` (1K) → 59 ms; `--fixture medium` (10K) → 203 ms. The 10K-task DB exists in well under the 250 ms cold-start budget for Phase 8's eventual application shell.

### What didn't change

- Phase numbering, mode-as-view, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- Phase 2 (single-writer worker) is the next phase. v0.1 dependency set still locked; tokio enters atrium-core's `[dependencies]` with the worker.

`VERSION`: 0.0.3 → 0.0.4 (patch — Phase 1 ship). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.3 (2026-05-05) — Phase 0: Scaffolding

First code lands. Cargo workspace, module skeleton, error hierarchy, tracing, `--debug` flag, Meson wrapper, GitHub Actions CI. Binary builds clean and runs; no UI surface yet.

### What shipped

- **Cargo workspace** at the repo root: `atrium/` (binary) and `atrium-core/` (headless library). Workspace `Cargo.toml` locks the v0.1 dependency set per spec — `tokio`, `rusqlite` (`bundled`, `chrono` features), `serde`, `serde_json`, `chrono`, `anyhow`, `thiserror`, `tracing`, `tracing-subscriber`, `gtk4` (`v4_16`), `libadwaita` (`v1_7`). Each crate's `[dependencies]` lists only what its phase actually uses; later phases pull more from the workspace as they need them.
- **Module skeleton** with `SPDX-License-Identifier: MIT` on every Rust file:
  - `atrium-core/src/{lib,error,paths}.rs` + `db/`, `domain/` placeholders.
  - `atrium/src/{main,error}.rs` + `ui/`, `quickentry/`, `debug/` placeholders.
- **XDG paths** (`atrium-core::paths`): stdlib-only — no `directories` / `xdg` crate. Honours `XDG_DATA_HOME` / `XDG_CACHE_HOME`, falls back to `$HOME/.local/share` / `$HOME/.cache`. Exposes `data_dir()`, `cache_dir()`, `db_path()`, and the `APP_ID` const (`io.github.virinvictus.atrium`).
- **Error hierarchy** (`thiserror`): `DbError`, `DomainError`, `CoreError` in core; `UiError`, `AtriumError` in the binary. Phase 0 ships the scaffolding; concrete variants land with the data layer (Phase 1+) and the application shell (Phase 3).
- **`--debug` flag plumbing:** stdlib argv parser, `Config` struct, `debug::Pane` stub gated on the flag. The pane logs that it's active in Phase 0; the actual widget mounts in Phase 3 with the application shell.
- **`tracing-subscriber`** initialised with `EnvFilter` (default `info,atrium=debug,atrium_core=debug`), compact format, target on. Honours `RUST_LOG` overrides.
- **CLI surface:** `--debug`, `--version` / `-V`, `--help` / `-h`. Unknown args ignored (no `clap` until we need it).
- **Meson wrapper** (`meson.build`): mirrors Viaduct's pattern — thin `cargo build --release` orchestration, installs binary to `$bindir`. GSettings / desktop entry / AppStream metainfo / icons grow in with Phases 3 and 8. Verified via `meson setup builddir && meson compile -C builddir` against the local toolchain.
- **GitHub Actions CI** (`.github/workflows/ci.yml`): Ubuntu 24.04, apt installs `libgtk-4-dev` / `libadwaita-1-dev` / `libsqlite3-dev` / `pkg-config`, runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- **`.gitignore`:** standard Rust + Meson + editor patterns.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ (7 tests: 3 in `atrium-core` covering `paths`, 4 in `atrium` covering `Config` parsing + `debug::Pane`)
- `cargo run -p atrium -- --debug` ✓ — logs version, debug_mode=true, app_id, pane init, exit
- `cargo run -p atrium -- --version` ✓ — prints `atrium 0.0.3`
- `meson setup builddir && meson compile -C builddir` ✓ — produces a 1.6 MB release ELF that runs cleanly

### Decisions captured here

- **Workspace over single-crate** (roadmap originally specced single-crate `src/{db,domain,ui,quickentry,debug,main.rs}`): workspace mirrors Viaduct's discipline and pre-empts the Phase 20 `atriumd` daemon split. Roadmap module-layout item updated to reflect.
- **Stdlib XDG / argv:** no `directories` / `clap` crate added. Phase 0 needs are small enough that hand-rolled is honest, and it keeps the locked dependency set true to spec. `clap` revisited if/when the CLI grows beyond a handful of flags.
- **Per-phase patch bumps** per the new release discipline: Phase 0 ships as v0.0.3. Phase 1 → v0.0.4, Phase 2 → v0.0.5, ..., Phase 9 → v0.1.0.

### Skipped intentionally

- **Heaptrack baseline:** Phase 0 binary does no allocation worth measuring; the §8 perf budget targets an active app on a 10K-task DB. First meaningful heaptrack lands at the end of Phase 3 when there's a GTK window to measure.

`VERSION`: 0.0.2 → 0.0.3 (patch — Phase 0 ship).

## v0.0.2 (2026-05-05) — Org vault projection + typography foundation

Pre-implementation. Two contract refinements: Org-mode integration grew into a first-class two-way mirror, and the typography foundation moved earlier so later UI phases develop into it.

### What changed

- **Org vault as projection** (`spec.md` §3.5, `CLAUDE.md` commitment #5): an optional two-way Org-mode mirror — SQLite stays canonical, a `.org` directory tree at `~/Tasks/` (configurable) reflects task state and accepts edits back from Emacs / Doom / vim-orgmode / any Org tool. Atrium runs cleanly with no vault configured; vault is downstream of the DB. The §7.3 mapping expanded to a full round-trip contract: vault layout (`<vault>/<Area>/<Project>.org`, `inbox.org` at root, `.atrium/config.toml` sidecar), every Atrium field's Org home, and six round-trip rules covering data preservation, `:ID:` anchoring, best-effort RRULE rendering, sidecar policy, conflict surfacing (no silent loss), and atomic file writes.
- **Roadmap split (Option B):** Phase 17 reworked into "Org-Mode Import & Read-Only Sync (DB → Vault)" — Atrium writes a clean vault any Org tool reads, plus one-shot import from existing Org libraries. Phase 17.5 added: "Two-Way Org Sync (Vault → DB)" — `inotify` watcher, `:ID:` allocation on read, conflict detection with `<file>.atrium.bak.<timestamp>` fallback, malformed-file recovery, RRULE divergence detection.
- **Typography foundation moved to Phase 3:** bundled font set lands with the Application Shell so Phases 4–7 develop into the type system instead of being re-skinned at Phase 8. Set: **Inter Variable** (UI), **Source Serif 4 Variable** (note bodies), **JetBrains Mono** (debug pane / monospace) — all SIL OFL or Apache 2.0, registered via `pango::FontMap::add_font_file`. Tabular figures (`tnum`) default-on for numeric contexts.
- **Typography polish (Phase 8) expanded** from one bullet to: Inter OpenType feature opt-ins (`cv11`, `ss01`), tabular-figures audit across every numeric column, optional Atkinson Hyperlegible accessibility toggle (SIL OFL, ~80 KB), surface-by-surface size/weight/leading pass, Flatpak font-load verification.
- **`CLAUDE.md` commitment #3 clarified:** "Local-first, no sync" → "Local-first, no *network* sync". Local file mirroring (the Org vault) is fine; CalDAV/cloud is still out.
- **`CLAUDE.md` commitment #5 added:** vault projection rule formalised — DB canonical, vault projected, don't pivot to vault-as-storage (perf budget would not survive at 10K-task scale).
- **`README.md` architecture paragraph** updated with the vault and `--debug` mentions; trimmed for length.

### What didn't change

- Single-writer SQLite worker (commitment #2) is unchanged. Org vault is downstream of the DB, not parallel to it.
- 20-phase brand intact; 17.5 is a sub-phase under Phase 17's Org-sync domain, not a renumbering.
- Mode-as-view, debug-first architecture, dependency discipline, and release discipline are unchanged.
- v0.1 dependency set unchanged. Phase 17 still flags `orgize` as a sign-off check before adoption.

`VERSION`: 0.0.1 → 0.0.2 (patch — contract refinement, no feature shipped).

## v0.0.1 (2026-05-05) — Contract refinement

Pre-implementation still. The contract gained a fourth architectural commitment and a written release discipline; no code shipped.

### What changed

- **Debug-first architecture** (`spec.md` §3.4, `CLAUDE.md` commitment #4): a `--debug` CLI flag opens an in-app debug surface for stress generators, pre-canned edge-case fixtures, SQLite/IO instrumentation through `tracing` spans, and live RSS/heap watch. Built into the binary, not bolted on. Skeleton in Phase 0; harness grows phase by phase.
- **Roadmap aligned:** Phase 0 lands the `--debug` skeleton and `debug::Pane` shell; module layout adds `src/debug/`. Phase 1 grows the stress fixture generator (10K / 50K / 100K presets + edge-case states). Phase 2 wires the IO instrumentation onto the single-writer worker. Phase 8 surfaces the live memory watch. North Star calls out the rhythm.
- **Release discipline** (`CLAUDE.md`): every minor or major change updates `spec.md`, `roadmap.md`, `patchnotes.md`, and `VERSION` together — if you can't write the patchnotes line, the change isn't done. Patch releases still bump `VERSION` and `patchnotes.md`. **Every major bump includes a maintenance pass** (refactor, deferred bugfixes, dead-code prune), called out in `patchnotes.md`.
- **Logical version bumps:** patch for fixes-only, minor for additive features that don't break the spec, major for spec-changing or breaking work. The bump rides with the change that earns it.

### What didn't change

- 20-phase sequence is intact: v0.1 (Simple Mode) ends at Phase 9, v0.2 (Builder Mode) at Phase 15, v1.0 at Phase 20.
- Dependency set is intact. The debug harness rides on `tracing` / `tracing-subscriber`, both already in Phase 0's locked set.
- Mode-as-view, single-writer SQLite worker, and local-first commitments are unchanged.

`VERSION`: 0.0.0 → 0.0.1 (patch — contract refinement, no feature shipped).

## v0.0.0 (2026-05-05) — Pre-implementation

Repository established. Specification, roadmap, and project conventions in place. No code yet — Phase 0 begins after sign-off.

### What's there

- **`spec.md`** — full application specification, 10 sections covering mission, mandates, architecture (mode-as-view, single-writer SQLite worker), data model (OmniFocus-superset schema), Simple/Builder UI deltas, Quick Entry contract, imports/exports with the Linux productivity-app landscape, perf budget, scope boundaries.
- **`roadmap.md`** — 20-phase plan. Phases 0–9 land Simple Mode (v0.1). Phases 10–15 add Builder Mode (v0.2). Phases 16–19 cover imports across Things 3, OmniFocus, Org-mode, Taskwarrior, Todoist, VTODO, todo.txt, TaskPaper. Phase 20 closes 1.0.
- **`README.md`** — public-facing introduction.
- **`LICENSE`** — MIT.
- **`VERSION`** — single source of truth (`0.0.0`).
- **`logo.svg`** — placeholder mark.

### Confirmed for v0.1

- **Stack:** Rust 2024, GTK4 ≥ 4.16, libadwaita ≥ 1.7, single-writer SQLite worker (Viaduct's pattern).
- **Direct deps:** `gtk4`, `libadwaita`, `tokio`, `rusqlite`, `serde`/`serde_json`, `chrono`, `anyhow`, `thiserror`, `tracing`/`tracing-subscriber`. Anything else gets a per-phase sign-off.
- **License:** MIT.

The first real release entry will land at the end of Phase 9 as **v0.1.0 — Simple Mode**.
