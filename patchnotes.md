# Atrium — Patch Notes

## v0.40.1 (2026-06-20): keyboard reorder (audit Tier D, part 5)

- **`Alt+Up` / `Alt+Down` move the focused task** up or down — a keyboard alternative to drag-to-reorder, on position-ordered lists (Inbox, Anytime, Someday, project and area pages). On a date-sorted list it declines with the same toast a drag would. Reuses the existing reorder math, so the behavior matches drag exactly. Documented in the shortcuts dialog and keymap.

This closes the planned Tier D pass (v0.39.0 → v0.40.1: time-view consolidation, interaction consistency, discoverability, in-row reschedule, keyboard reorder). One larger accessibility item remains tracked but deliberately deferred: full keyboard navigation *within* the custom pages (Agenda, Forecast strip, kanban board, calendar grid) needs those rows wrapped in proper focusable list containers, which is its own careful refactor rather than a bolt-on (Tab-focusing dozens of bare rows would make tabbing tedious and non-idiomatic).

## v0.40.0 (2026-06-20): in-row quick reschedule (audit Tier D, part 4)

The audit's biggest single smoothness gap was that rescheduling a task always cost an editor round-trip: open the Inspector, change the date, apply. The most common action, "push this to tomorrow," was three steps.

- **The task row's right-click menu gains a Schedule submenu**: Today, Tomorrow, This Weekend, Next Week, Someday, Clear Schedule. One pick reschedules the task in place via a new `win.reschedule` action, no editor.
- The keyword-to-date mapping (`parse_quick_schedule`) is a pure, unit-tested helper (weekend = this week's Saturday; next week = next Monday), so the date math is verified independently of the GUI.

Because the menu is also keyboard-reachable (Menu key / Shift+F10 on a focused row), this doubles as a keyboard path to rescheduling. (Tag quick-edit already has Ctrl+T.) Workspace test count 1007 → 1011.

## v0.39.2 (2026-06-20): discoverability polish (audit Tier D, part 3)

Surfacing features the audit found buried:

- **Kanban boards have a "Configure…" button.** Columns and axis were only reachable from the perspective row's right-click menu. The board page header now carries a Configure button that opens the same renderer dialog directly.
- **Quick Entry advertises template shortcuts.** When you have a Quick Entry template with a shortcut key configured, the modal's hint line now mentions the `:key` trigger; it stays quiet when no such templates exist.
- **Drag-and-drop interactions are documented.** A new "Drag and drop" section in the keymap reference (and the docs site) catalogs the gestures that had no written home: reorder, Shift-drag to nest, drag-to-reschedule, drag-to-project, kanban column moves, and drop-to-capture.

(Inspector-pane checkbox undo parity and a per-row Shift-drag tooltip were considered and deferred: the first is low-value against a single re-click, the second would put a tooltip on every task row. The keyboard-first alternatives to drag land in the D5 pass.)

## v0.39.1 (2026-06-20): interaction consistency (audit Tier D, part 2)

Two interaction dead-ends from the audit:

- **Calendar week-strip rows open their task.** In the narrow-window week strip, task titles were bare labels: a click bubbled to the day card's peek popover instead of opening the task you clicked. Each row now opens that task directly (consistent with Agenda and Forecast rows), claiming the event so an empty-area click still peeks the day.
- **Date-sorted lists no longer swallow a reorder drag.** Dragging to reorder on a date-sorted list (Today, Upcoming, Logbook) had no persistent meaning and failed silently. It now surfaces a toast ("This list is sorted by date...") so the drag isn't a mystery, matching the Shift-drag reparent path that already toasts on rejection.

(Drag-to-reschedule remains on the Forecast "Strip" layout and the Calendar; the Agenda "Bands" layout stays read-on-click by design, since the Strip is the interactive lens of the same merged view.)

## v0.39.0 (2026-06-20): merged time-view (audit Tier D, part 1)

The audit found the app's biggest coherence problem was the "when" dimension: four surfaces (Upcoming, Agenda, Forecast, Calendar) all answer "what's coming up," they didn't explain how they differ, and Agenda and Forecast overlapped most (both lead with Overdue then near-term). This consolidates them.

- **Agenda and Forecast are now one sidebar entry.** The standalone Forecast row is gone; the Agenda view carries a centered **Bands / Strip** layout toggle in Builder Mode. Bands is the chronological Agenda (Overdue / Today / Tomorrow / This Week / Next Week); Strip is the 30-day Forecast projection with drag-to-reschedule. Same data, two lenses, one entry.
- **Simple Mode stays calm.** The Strip layout is Builder-only, so Simple Mode shows just the Bands layout with no toggle (exactly the Agenda it had before, now the only time-view entry besides Calendar and the Upcoming list). Flipping from a Strip view to Simple lands on Bands, not a bounce to Today.
- The Builder sidebar's top tier drops from five entries to four (Agenda, Calendar, Review, Logbook). No data, schema, or task-classification change: both page builders and the Org-agenda parity test are reused untouched; the toggle just switches which one renders.

This is the first of the Tier D UX pass; interaction unification, in-row quick edit, discoverability polish, and keyboard navigation of the custom pages follow.

## v0.38.3 (2026-06-20): accessibility (audit Tier C)

Third slice of the audit-driven foundation pass. Accessibility fixes:

- **Keyboard focus is visible again.** The sidebar and task-list rows set `outline: none` so the mouse selection-glow stayed clean, but that also erased the keyboard focus ring, leaving keyboard users unable to see where they were. A `:focus-visible` rule now draws an accent ring for keyboard navigation only (a mouse click keeps the clean glow).
- **Accessible names on icon-only controls.** The tag-colour swatches (whose "no colour" option read as a stray glyph to a screen reader) and the per-row recurrence icon now carry explicit accessible labels; a tooltip is not an accessible name.
- **Calendar "today" is announced.** In the month grid, today was cued only by a border colour and a bold day number, indistinguishable to a screen reader; the day number now reports "N (today)".

(Two larger audit items, keyboard-operable rows on the custom calendar/board/agenda pages and a keyboard alternative to drag operations, are being planned as a follow-up since they touch the interaction model rather than being surgical fixes. The Inspector's keyboard-dead dependency/link pickers were already fixed in v0.38.1.)

## v0.38.2 (2026-06-20): performance tightening (audit Tier B)

Second slice of the audit-driven foundation pass. Behavior-preserving:

- **Vault watcher no longer scans the whole tag table per save.** `diff_and_apply` (run on every external `.org` edit) snapshotted the entire `task_tag` table via `tag_names_per_task`, then used only the one project's tasks. A new project-scoped `tag_names_for_project` joins through `task.project_id`, so a single Emacs save costs a project-sized read, not a library-sized one. This is the one finding that genuinely threatened the §8 budget at 10K+ tasks with a live vault.
- **Column-list churn removed.** Five read queries that alias `task t` rebuilt the `t.`-prefixed column list with a `split / map / collect / join` on every call. Hoisted to a single `LazyLock<String>` (`TASK_COLUMNS_T`) computed once; the SQL produced is byte-identical.

Two regression tests added (project-scoped tag read; the corrupt-blob path from Tier A).

## v0.38.1 (2026-06-20): correctness fixes (audit Tier A)

First slice of an audit-driven foundation pass. Four correctness fixes, no new features:

- **Keyboard-dead pickers fixed.** The Builder Inspector's "Add a prerequisite" picker and the Notes "Link to another task" picker wired their click through a `GestureClick`, so Enter/Space on a focused row did nothing: a keyboard user could not add a dependency or insert a task link at all. Both now use `connect_activated`, which fires on click and keyboard alike (the rows live in a `gtk::ListBox`).
- **Silent Org data loss surfaced.** A corrupt `task.extra_properties` blob silently became an empty map on read, discarding the unmodeled `:PROPERTIES:` values that column exists to preserve. It now logs a warning before degrading, and a regression test pins the no-panic path.
- **Reminder boundary query fixed.** `next_pending_reminder` hand-formatted its comparison bound with a divergent shape (a `Z` suffix where rusqlite writes `+00:00`, and it dropped the seconds field entirely), making the "soonest reminder after now" comparison unreliable. It now binds the `DateTime` directly so both sides use rusqlite's serialization; the test fixtures use the real on-disk format.
- **Simple-mode defer leak closed.** The Simple Inspector showed a "Defer until" row, but `defer_until` is a Builder-only field (spec §3.1 / §4: "hidden in Simple"). Removed from the Simple dialog; the Builder Inspector pane still exposes it and the stored value is untouched.

## v0.38.0 (2026-06-20): status-axis kanban boards

Kanban boards gain a second grouping axis. The original tag axis groups by tag and a drag rewrites synthetic tags; dragging to a "Done" column just added a `done` tag and left the task uncompleted, so the board and the rest of the app disagreed. The new **status axis** groups by Org TODO-sequence keyword and makes the drag mean something: a card's column is its actual state, and moving it changes that state.

What changed:

- **`atrium-core::render` — status axis.** `BoardAxis::Status` buckets each task by its keyword (`orig_keyword`, falling back to canonical `TODO`/`DONE`). New `status_keyword`, `status_move` (returns the keyword + completion change a drag implies), and `parse_status_columns` / `format_status_columns` (the Org `#+TODO:` pipe convention, `TODO, NEXT | DONE, CANCELLED`). `BoardConfig` gains an optional `done_columns`; it is omitted from the JSON for tag boards, so every pre-v0.38.0 config is byte-identical and parses unchanged.
- **GUI.** The "Configure renderer…" and perspective-editor dialogs grow a third radio, "Board — columns by status (Org keywords)", with axis-aware hint text. Dragging a card on a status board sets `orig_keyword` and, for a done-column, completes the task through the normal completion path (so a repeating task rolls forward exactly as it would on a checkbox tick).
- **CLI.** `perspective create/edit` accept `--axis tag|status`; `kanban NAME` renders status boards. New `edit ID --keyword KW|none` sets or clears a non-canonical keyword, so the status board is fully driveable from the shell (the standing CLI-testability commitment).
- **No schema change.** `orig_keyword` and `completed_at` have been in the schema since 0007 / 0001; this is pure engine + UI + CLI work.

Tests: 20 new (17 in `render`, 3 in the CLI parser); workspace 1005 unit tests, all green.

## v0.37.4 (2026-05-28): README rewrite for professionalism

The README had accreted into a release log: a version-by-version "Current release" wall, a full phase-ledger Status table, and a `(v0.X.0)` tag on nearly every feature row and CLI entry. That belongs in the roadmap and these patch notes, not on the front page. This rewrite refocuses it on users and developers, with no code, schema, or behaviour change.

What changed:

- **Cut the release-tracking.** The phase ledger and the version narrative are gone; the Status section is now a short "feature-complete, heading to 1.0" note that links to `spec.md` / `roadmap.md` / `patchnotes.md` for anyone who wants the history.
- **De-versioned the features.** The Simple Mode, Builder Mode, import, and CLI sections describe capabilities, not ship dates. Every `(v0.X.0)` annotation was removed.
- **Tightened the structure.** The 400-word architecture paragraph and the "Where things live" table collapsed into a seven-crate list plus the four load-bearing decisions; the exhaustive Org-research bibliography was trimmed to a pointer into `roadmap.md`.
- **Status badge, not a version badge.** The header badge now reads "feature-complete, heading to 1.0" so it stops going stale on every patch release.

Net: 395 lines down to 199, no em-dashes, all links and screenshots verified. The benchmark was the cleaner READMEs elsewhere in the portfolio (Viaduct, Conservatory).

### Tests

No code change; the workspace stays at 985. Verification is `appstreamcli validate` clean on the metainfo and a link/screenshot existence check.

## v0.37.3 (2026-05-28): documentation-soundness sweep

A final drift pass over the doc set, with no code, schema, or behaviour change. The v0.37.2 test-count change (1008 → 985) and the accumulated patch releases had left a handful of version and test-count headers behind:

- **README**: the badge and "Current release" line moved to v0.37.3; the testing-section comment corrected to "985 tests at v0.37.3" (it still read 1008).
- **CLAUDE.md**: the status line moved to v0.37.3; the `cargo test --workspace` comment, stale at "974 at v0.27.0", corrected to "985 at v0.37.3".
- **spec.md**: the `Version:` header, lagging at 0.35.0, moved to 0.37.3, with a note that v0.37.1 through v0.37.3 were documentation and test-suite maintenance.
- **roadmap.md**: the intro "Current release" moved to v0.37.3, with the same maintenance note.

`roadmap.md`'s phase checkboxes were already accurate (every Phase 19 / 19.5 / Tier 3 item ticked; the open l10n / Flathub / screenshots / `v1.0.0`-tag items correctly still open), so only its intro line changed.

### Tests

No test change; the workspace stays at 985. Verification is `appstreamcli validate` clean on the metainfo.

## v0.37.2 (2026-05-28): consolidate the CLI parser tests

A test-suite tidy in `atrium-cli`, with no code, schema, or behaviour change.

The crate carried two test homes for one argv parser: the inline `#[cfg(test)] mod tests` in `args.rs` (38 tests, added in the v0.21.0 maintenance pass) and the older `src/tests.rs` (127 tests). 23 of the `args.rs` cases duplicated `tests.rs` (nine shared the exact function name). They are now one home: the 15 unique `args.rs` tests (the `clock` subcommand, the v0.18 `template` subcommand, `--deadline-warn` / `--warn`, and `--parent` on `add` / `edit`) moved into `tests.rs`, and the duplicate module was deleted.

Net effect is **−23 tests with no coverage loss**: `atrium-cli` goes from 165 to 142 unit tests and the workspace headline from 1008 to **985**. The audit also confirmed the CLI's `parity_*` / `falls_back_*` tests are *not* redundant: they are the only place the SQL fast-path and the in-memory evaluator are checked to return the same rows against a real database (`atrium-search`'s `sql_translate` tests only check the emitted SQL), so they stay.

### Tests

`cargo test -p atrium-cli` green at 142; `cargo clippy -p atrium-cli --all-targets -- -D warnings` clean. The moved tests are byte-for-byte the originals (only the `argv` helper renamed to the destination's `s` helper). Test count 985 (was 1008).

## v0.37.1 (2026-05-28): public-facing prose cleanup

A documentation pass over the three public-facing surfaces (README, the AppStream app description, and the mdbook handbook's guide chapters), with no code, schema, or behaviour change.

Two problems were addressed. First, drift: the README still announced **v0.20.0** with **854 tests** and marked Phase 19, most of Phase 19.5, and most of Phase 20 as "planned" though all of it had shipped. The badge, the "Current release" narrative, the Status table, and the test count are now current at **v0.37.0 / 1008 tests**, and the CLI table gained the rows it was missing (`depend`, `backup`, `task-template`, the VTODO / Taskwarrior / todo.txt importers, and `export vtodo`). Second, voice: every em-dash in the README (95), in the metainfo's evergreen `<summary>`/`<description>` (3), and in the book's narrative chapters (11) was recast to a colon, comma, period, or parenthetical for a cleaner read.

Deliberately left as historical record: the `patchnotes.md` entries below this one and the metainfo's per-release changelog. Those are a log of what was written at ship time, parallel to a git history, so they keep their original prose. New entries from here on use a colon in the header rather than an em-dash.

### Tests

No automated-test change. Verification is the standard gate (`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/regression.sh`) plus `appstreamcli validate` on the metainfo and `mdbook build book` on the handbook. Test count unchanged at 1008.

## v0.37.0 (2026-05-28) — documentation site (Phase 20)

An `mdbook` handbook under `book/`. Workspace 1008 unit tests + green; clippy `-D warnings`, fmt, `scripts/regression.sh`, and `appstreamcli validate` all clean. No schema / code change (a docs site + `.gitignore` entry).

### Structure

`book/book.toml` + `book/src/SUMMARY.md` with two parts. The **Guide** chapters (Simple & Builder modes, Quick Entry & inline syntax, search expressions, importing & exporting, the Org vault) are short, original on-ramps that point to `spec.md` for the authoritative detail — they don't restate the contract. The **Reference** chapters (keyboard map, database schema, accessibility, performance) and the GTD-patterns + Org round-trip chapters `{{#include}}` the canonical `docs/*.md` files verbatim, so the site never forks from the single source of truth.

Built with `mdbook build book` (clean; all includes resolve). The rendered HTML is git-ignored (`/book/book`); only the source is checked in. Hosting (GitHub Pages) is a follow-on.

### Phase 20 reorder

Localisation scaffolding and Flathub readiness are **deferred to a session at the sandbox** — their verification (the meson MO-build for l10n; the offline `flatpak-builder` build + screenshot capture + Flathub PR) lands in Brandon's environment, not CI. The docs site took the next contiguous slot (v0.37.0); those two cuts get later numbers when they ship, before the `v1.0.0` tag.

### Tests

No new automated tests — the verification is `mdbook build` succeeding with every include resolved. Test count unchanged.

## v0.36.0 (2026-05-28) — perf regression suite (Phase 20)

Turns the spec §8 perf budget from a one-off measurement into a repeatable, headless gate. Workspace 1008 unit tests + green; clippy `-D warnings`, fmt, `scripts/regression.sh`, and `appstreamcli validate` all clean. No schema change, no code change (a script + doc).

### `scripts/perf.sh`

Generates the Large (50K) and Stress (100K) fixtures, times generation + a full read-path load (`atrium-cli list all`), captures peak RSS via `/usr/bin/time -v`, and asserts the budgets that don't need a display: the 50K data-layer working set stays under the 80 MB idle budget, and the `atrium --version` cold-start floor beats the 250 ms first-frame budget. Exits non-zero on a breach. An opt-in `--heaptrack` arm runs a heaptrack pass when the tool is installed (external tooling, not a build dependency).

It's deliberately **separate** from `regression.sh` — generating 150K rows is too heavy for the per-commit ship gate. Run it before tagging or after touching the data layer.

### Reference numbers

| Scale | Fixture gen | Read-path load | Peak RSS |
|---|---|---|---|
| 50K | ~1.3 s | ~220 ms | ~55 MB |
| 100K | ~2.2 s | ~470 ms | ~100 MB |

50K sits comfortably under the 80 MB idle budget; cold-start floor measured 20–30 ms. The GUI active-RSS + first-interactive-frame budgets still need a real display (the in-app Memory Watch). `docs/perf-baseline.md` gains a v0.36.0 section with the numbers + the read-path-vs-fixture-only distinction.

### Tests

No new automated tests — `perf.sh` is the suite, exercised against the release binaries. Test count unchanged.

## v0.35.0 (2026-05-28) — accessibility round 2 (Phase 20)

Opens Phase 20, the 1.0 endgame. The v0.1 accessibility audit predated every Builder and Tier 2/3 surface; this is the owed re-audit + the gaps it found. Workspace 1008 unit tests + green; clippy `-D warnings`, fmt, `scripts/regression.sh`, and `appstreamcli validate` all clean. No schema change.

### What changed

A tooltip on an icon-only button is exposed to assistive tech as a *description*, not the *name* a screen reader announces — so icon-only buttons across the newer surfaces got explicit `accessible::Property::Label`s: calendar month nav (prev/next), the sidebar New-Perspective add, the tag-editor add, the Inspector Notes "Link to another task" button, the "Blocked by" add + per-row remove buttons, and the preferences vault-folder picker. Buttons with visible text were already named.

The audit (recorded in `docs/accessibility.md`'s new Round 2 section) also confirmed the surfaces compose already-labelled widget primitives, the empty-state `AdwStatusPage`s are self-describing, and status cues aren't colour-only (the "Blocked" pill and sequential "queued" rows pair colour with text).

`atriumd` (the zero-launch capture daemon) is confirmed **deferred to post-1.0** — the spec + roadmap references that called it a v1.0 / v0.2 item are corrected. A stale `atrium-cli/src/vtodo/` path (the module moved to `atrium-import` at v0.34.0) is fixed in spec §7.5.

### Tests

No new automated tests — accessible labels aren't unit-testable in isolation, and the full assistive-tech pass (Orca, keyboard-only traversal) is owed on a real display (Brandon's verification). The audit doc is the code-side record.

## v0.34.0 (2026-05-28) — import library + unified import dialog (Tier 3, arc close)

The last cut of the Post-v0.22.0 Tier 2 + 3 polish arc. Two parts: an enabling extraction and the GUI dialog it unlocks. Workspace 1008 unit tests + green; clippy `-D warnings`, fmt, `scripts/regression.sh`, and `appstreamcli validate` all clean. No schema change.

### `atrium-import` crate (extraction)

The non-Org importers — Todoist CSV, Taskwarrior `task export` JSON, todo.txt, and VTODO `.ics` — moved out of the `atrium-cli` binary into a new `atrium-import` library crate (the workspace is now seven crates). They depend only on `atrium-core`, so the GTK binary can consume them directly. `atrium-cli` re-imports the modules under their old names (`use atrium_import::{import, vtodo};`), so every existing `import::…` / `vtodo::…` call site is unchanged; `UdaPolicy` moved with the Taskwarrior importer and `atrium-cli` re-exports it. The importer test suites (and their fixtures, which resolve via `CARGO_MANIFEST_DIR`) moved with the code and stay green — the regression guard for a behaviour-neutral move. Org import/export stays in `atrium-org` (already a library).

### Unified import dialog (GUI)

A new "Import…" menu entry opens an `AdwDialog` (`atrium/src/ui/import_dialog.rs`) with a source picker (Org / Todoist / VTODO / Taskwarrior / todo.txt), a file chooser, a project-name field, a Taskwarrior UDA-handling combo, and a Dry run toggle (default on). Import runs the matching parser + mapper through the single-writer worker — mirroring the CLI's `run_import` per-source flow — and shows a summary (tasks created, tags ensured, lossy-field count) in the result pane. Dry run previews without writing.

### Tests

+2 GUI unit tests for the pure `format_import_result` summary formatter (dry-run wording + lossy note; clean wet-run). The bulk of the coverage is the importer suites that moved into `atrium-import` (now run under that crate).

### Arc closed

This closes the ten-cut Tier 2 + 3 arc (v0.26.0 → v0.34.0). The `pick-it-up.md` handoff is retired (removed in this commit). Carryover for later: the EDS calendar overlay (needs a `libecal` / `zbus` dependency sign-off), README screenshots, Flatpak font verification under the sandbox, and the Phase 20 / 1.0 endgame.

## v0.33.0 (2026-05-28) — task templates (Tier 3)

Reusable project templates: a named, optionally-nested set of tasks with per-item tags + estimates, stamped out as a fresh project on demand. Distinct from the single-line Quick Entry templates (v0.18.0). Workspace 1006 unit tests + green; clippy `-D warnings`, fmt, `scripts/regression.sh`, and `appstreamcli validate` all clean.

### Schema

Migration `0017_task_template.sql` (`user_version` 16 → 17) adds two tables. `task_template` (name UNIQUE, `project_title_seed`, `note`, `tags_json`) and `task_template_item` (title, index-based `parent_index`, `position`, `estimated_minutes`, `default_tags_json`, FK CASCADE to the template). The `parent_index` references an item's slot in the template's position-ordered list, so a template carries a nested task tree with no task ids; the worker resolves it at instantiate time. Additive; v0.32.x binaries ignore both tables.

### Worker + read

`create_task_template` inserts the template + its items; `instantiate_template` creates a fresh project (title from `project_title_seed`, falling back to the template name), walks the items in order resolving each `parent_index` to the real `parent_id`, and ensures both template-level and per-item tags. `delete_task_template` CASCADE-drops the items. New read helpers `list_task_templates`, `task_template_by_id`, `task_template_by_name`, `task_template_items`.

### CLI + GUI

`atrium-cli task-template list | create --name N [--project-title T] [--note X] [--tag T]... [--item TITLE]... | instantiate NAME | delete NAME` (CLI items are top-level; nesting is authored via the worker API). The GUI gains a "New from Template…" menu entry that opens a picker and instantiates through the worker, then navigates to the new project.

### Tests

+5: two worker (`create_and_instantiate_task_template` checks the project + nested tasks + tags + estimate; `delete_task_template_then_instantiate_is_not_found` checks delete + CASCADE) and three CLI argv (`create`, `create` requires `--name`, `instantiate`/`delete`).

## v0.32.0 (2026-05-28) — backup / restore (Tier 3)

In-app database backups, the long-standing gap the file-copy escape hatch never closed. Workspace 1001 unit tests + green; clippy `-D warnings`, fmt, `scripts/regression.sh`, and `appstreamcli validate` all clean. No schema change.

### Core (`atrium-core/src/backup.rs`)

`backup_now(db_path, dir)` writes a defragmented single-file snapshot via SQLite's `VACUUM INTO`, run on a fresh read-only connection (documented to work on a read-only DB and never mutating the source, so it never contends with the single-writer worker). File name `atrium.<UTC>.db`, with a `~N` suffix to disambiguate same-second snapshots. `prune(dir, keep)` keeps the newest N by sortable name; `latest_backup(dir)` returns the newest. New `paths::backups_dir()` (`$XDG_DATA_HOME/atrium/backups/`) and `paths::restore_marker_path()`.

### GUI (Preferences → Backups)

"Back up now" (snapshot + prune to 10), "Restore from backup…" (a `FileDialog` that writes the chosen path into a `.restore-pending` marker; `boot_data_layer` copies it over the live DB before opening, then clears stale WAL/SHM), and a "Weekly automatic backup" switch bound to the new default-off `backup-weekly` GSetting (snapshot at launch when the newest is over seven days old).

### CLI

`atrium-cli backup [--dir PATH]` — snapshot + prune, summary respects `--json` / `--tsv` / `--human`. Defaults to the data-dir backups folder.

### Tests

+5: three core (`backup_now` produces a readable snapshot, `prune` keeps N + leaves non-snapshots, prune no-op under limit) and two CLI argv (`backup`, `backup --dir`).

## v0.31.0 (2026-05-28) — first-run onboarding (Tier 3)

A pristine database (no tasks, no projects, no areas) now paints a welcoming `AdwStatusPage` instead of an empty Inbox, with three next-steps: create your first project, capture a task, or set up an Org vault. It clears itself the instant the user creates anything. No seeding, no GSetting. Workspace 996 unit tests + green; clippy `-D warnings`, fmt, `scripts/regression.sh`, and `appstreamcli validate` all clean. No schema change.

The "inline editing on row edit" item planned for this slot turned out to be already shipped (the row editor's `handle_rename` has parsed `#tag` / `@date` / `!N` into structured fields on commit since the `atrium-inline` work, and the row entry already carries the tab-completion popover), so the slot was repurposed for onboarding and that roadmap box was ticked in place.

### Mechanics

A new `atrium/src/ui/window/onboarding.rs` builds the page (added to `content_stack` as the `"onboarding"` child) and owns a cached `db_empty` flag. `recompute_db_empty` short-circuits on the first task (`count_tasks > 0`) so a populated DB never lists projects / areas. `refresh_active_list` yields to the onboarding page while the flag is set; `sync_onboarding` (called from `apply_task_changes` + `apply_library_changes`) flips the page in and out as the data crosses the empty boundary. The CTAs reuse the existing `prompt_create_project`, the `app.quick-entry` action, and `preferences::open`.

### Tests

No new automated tests — the onboarding decision is GUI-side composition of already-tested read helpers (`count_tasks`, `list_areas`, `list_projects`). Manual GUI pass flagged for the empty → populated → empty transitions.

## v0.30.0 (2026-05-28) — drag files / URLs to capture (Tier 3)

First of the Tier 3 polish run. Drop a file, a URL, or selected text anywhere on the window and Quick Entry opens pre-filled, so the capture is reviewable (add a `#tag`, a `@date`) rather than a silent insert. Workspace 996 unit tests + green; clippy `-D warnings`, fmt, `scripts/regression.sh`, and `appstreamcli validate` all clean. No schema change.

### Behavior

A window-level `gtk::DropTarget` accepts `gdk::FileList` (how file managers deliver a drag) and `String` (how a browser delivers a URL or selected text). A dropped file becomes a task named after the file (base name, extension stripped); a URL or text is kept verbatim as the title. Quick Entry then opens with the entry primed and the cursor at the end.

### Surface

- `quickentry::modal::open` gained an `initial_text: Option<String>` param (the `Ctrl+Alt+Space` action passes `None`).
- A new `atrium/src/ui/window/drop.rs` installs the target on the window; the drop-payload parsing is a pure `capture_prefill_from_drop` helper (file-URI percent-decode + base-name, URL/text passthrough) so it's unit-testable without GTK.

### Tests

+5 atrium unit tests covering the file-URI / authority / URL / multi-line-text / empty-payload shapes of `capture_prefill_from_drop`.

## v0.29.0 (2026-05-28) — task dependencies (`blocked_by`)

Tier 2's remaining feature. A task can be blocked by one or more prerequisite tasks; a blocked task is "unavailable" until every prerequisite completes. Taskwarrior-parity (the v0.26.0 importer drops `depends` with a lossy hint pointing here) and a deepening of the OmniFocus-superset story. Mirrors the v0.23.0 subtasks scaffolding. Workspace 991 unit tests + green; `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` clean; `bash scripts/regression.sh` passes; `appstreamcli validate` clean.

### Schema

Migration `0016_task_dependency.sql` adds the `task_dependency(task_id, blocked_by_id)` join table (`user_version` 15 → 16). A row means "task_id is blocked by blocked_by_id" (the latter is a prerequisite). FK CASCADE on both ends; `UNIQUE(task_id, blocked_by_id)` makes a re-added edge a no-op. Additive and backwards-compatible: v0.28.x binaries reading a v0.29.0 DB ignore the table.

### Semantics

- **`is:available`** = open AND not blocked by any open prerequisite. Dependency-only: defer is still `is:deferred`, sequential queue state is still `is:queued`. (It was a no-op stub before this release, so nothing regresses.)
- **`is:blocked`** = open AND blocked by at least one open prerequisite. A completed task is never blocked, and a prerequisite that's already done doesn't gate availability.
- Both translate to an `EXISTS` / `NOT EXISTS` subquery over `task_dependency` in the SQL fast-path; the in-memory evaluator gets a `blocked_ids` set on its `EvalContext` for the regex / fuzzy / composite fallback.
- No completion cascade (mirrors subtasks): completing a prerequisite doesn't auto-touch dependents beyond recomputing their blocked state.

### Worker

`add_dependency` / `remove_dependency` commands. The worker rejects self-dependencies and cycles via `would_create_dependency_cycle` (walks the prerequisite chain forward from the proposed blocker; mirror of the subtasks `would_create_cycle`), and absorbs duplicate edges with `ON CONFLICT DO NOTHING`. Each successful add / remove emits a `TaskChanges` refresh for the affected task so its row repaints. New `DomainError::DependencyCycle`.

### GUI

- A "Blocked" pill (amber, `.atrium-task-blocked`) plus a faint title dim (`.blocked` row class) on any task with an open prerequisite. The window queries `read::blocked_task_ids` on list load and on every diff, recomputing across the whole store so completing a prerequisite unblocks its dependents in the same frame.
- Builder Inspector gains a "Blocked by" group above Notes: each prerequisite renders as a row (click to navigate, button to remove), and a header "Add" button opens a search-as-you-type picker over other tasks that records the edge through the worker.

### CLI

`atrium-cli depend ID --on ID [--remove]` adds or drops a dependency; `info --human` lists a task's prerequisites under "Blocked by". `--on` is required; the flag round-trips through `Subcommand::Depend`.

### Read helpers

`read::blocked_task_ids(conn)` (open tasks with ≥1 open prerequisite — feeds both the pill and the eval context) and `read::list_prerequisites(conn, id)` (the tasks blocking a given task — feeds the Inspector group and CLI `info`).

### Tests

+14 unit tests (977 → 991): worker self / direct / transitive cycle rejection + duplicate no-op + remove-absent no-op; read-side `blocked_task_ids` (open vs completed prerequisites), `list_prerequisites`, and FK CASCADE on delete; an eval test for `is:blocked` / `is:available` against a blocked set; SQL-translate assertions for both predicates plus `is:queued` still declining; four CLI argv tests for `depend`.

## v0.28.0 (2026-05-28) — per-area review schedules (Post-v0.22.0 Tier 3)

The first cut of the Post-v0.22.0 Tier 3 polish arc, and a Phase 13 carryover finally landed. Until now a project only entered the Review queue when it carried its own non-NULL `review_interval_days`; every project had to be opted in by hand. An area can now set a default cadence that cascades to the projects filed under it. Workspace 977 unit tests + green; `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` clean; `bash scripts/regression.sh` passes; `appstreamcli validate` clean.

### Schema

Migration `0015_area_default_review_interval.sql` adds `area.default_review_interval_days INTEGER NULL` (`user_version` 14 → 15). Additive and backwards-compatible: v0.27.x binaries reading a v0.28.0 DB ignore the column.

### Cascade

`list_review_queue` now `LEFT JOIN`s the area and resolves the effective cadence with `COALESCE(project.review_interval_days, area.default_review_interval_days)` for both the membership predicate and the next-due date math. The project's own value always wins; the area default only fills in where the project leaves it NULL; a project in no area (or an area with no default) behaves exactly as it did before. Both-NULL keeps the project out of the queue.

### Surface

- `Area` / `NewArea` / `AreaUpdate` carry the new field; `AreaUpdate.default_review_interval_days(Option<i64>)` mirrors the existing `color` `Option<Option<_>>` builder, so `Some(None)` clears an existing default and `Some(Some(n))` sets it.
- The shared Edit Area / New Area dialog (`prompt_for_named_color`) grows an optional "Review every (days, 0 = off)" `SpinButton` row; tag dialogs pass `None` and are unchanged. The window caches each area's current value (`area_review_intervals`) to pre-fill the row on edit. 0 maps to "no default" (a clear).
- No CLI change: areas have no create/edit subcommand today, so the cascade is exercised through the data-layer tests rather than a shell surface.

### Tests

Three new atrium-core tests: `review_queue_honors_area_default_when_project_interval_null` and `review_queue_project_interval_overrides_area_default` (read-side cascade, including the override case where the project's own slower cadence keeps it out of a queue the area default would pull it into), plus `area_default_review_interval_round_trips` (worker create / update / clear-to-NULL).

## v0.27.0 (2026-05-28) — todo.txt importer (Phase 19 slice 3)

Phase 19's third slice. todo.txt is the simplest of the importer formats — one task per line, no quoting, Gina Trapani's spec at <https://github.com/todotxt/todo.txt>. Hand-rolled stdlib parser + mapper; no new deps. Workspace 974 tests + green; clippy `-D warnings` and fmt clean; `scripts/regression.sh` passes; `appstreamcli validate` clean. No schema change.

### New CLI surface

```
atrium-cli import todotxt PATH --into PROJECT [--dry-run]
```

Lines beginning with `#` and blank lines are skipped (the widely-accepted comment convention). Everything else parses positionally: optional `x ` completion marker (+ optional completion date), optional `(L)` priority letter, optional creation date, then the description with inline `@context` / `+project` / `key:value` tokens.

### Field mapping

`description` (description tokens, with inline markers stripped) → `task.title`. Completion marker `x` + optional completion date → `task.completed_at`. Priority `(A)` / `(B)` / `(C)` → `priority-1` / `priority-2` / `priority-3` tag (matching Todoist and Taskwarrior conventions); `(D)`–`(Z)` drops with one lossy entry. Creation date → `task.scheduled_for` for open tasks; completed tasks skip this path. Inline `@context` tokens → tags via `ensure_tag`. Inline `+project` tokens → dropped + one lossy entry per occurrence (the `--into` flag wins; user can't have it both ways). `due:YYYY-MM-DD` extension → `task.deadline`. `t:YYYY-MM-DD` extension (threshold / start date) → `task.defer_until`. Other `key:value` extensions → dropped + lossy entry per occurrence.

### UUID round-trip

todo.txt lines don't carry UIDs. The mapper derives `task.uuid = UUIDv5(TODOTXT_NAMESPACE, "<project_name>|<title>|<creation_date>")` from a frozen namespace constant. Re-imports of the same file onto the same project produce the same UUIDs, so an Atrium-side edit + re-import lands as an update if a future merge-by-uuid path ships, rather than a fresh row.

### Lossy report

Three `LossyKind` variants: `DroppedInlineProject` (one per `+project` token), `PriorityBelowC` (priorities `(D)`–`(Z)`), `DroppedKeyValue` (unknown extensions, one per occurrence).

### Tests

Two fixtures + 11 parser unit tests + 8 mapper unit tests + 3 integration tests:

- `tests/fixtures/todotxt/basic.txt` — single open task with priority + contexts + project + `due:`.
- `tests/fixtures/todotxt/mixed.txt` — six lines covering open + completed + threshold-deferred + bare title + `(D)` low priority + a URL token (which deliberately parses as `http://...` to exercise the `DroppedKeyValue` path).
- Parser tests: minimal title, priority + creation date, completion marker + completion date, inline `@`/`+`/`key:value` classification, comment + blank skip, `(D)` letter capture, double-letter rejection, multi-line document parse, URL-token behaviour, `x` without space rejection, empty `@`/`+` token rejection.
- Mapper tests: priority A/B/C tag mapping, tag collection, `due:` and `t:` date extraction, lossy emission, NewTask field routing, completion path, deterministic UUID, dry-run summary.
- Integration tests: basic fixture round-trip, mixed fixture status + threshold + lossy verification, comment/blank skip.

### Argv

`src/tests.rs` gains three new cases: `parse_import_todotxt_requires_into`, missing `--into` error, and the cross-importer rejection (`import todotxt --uda-as tag` errors).

## v0.26.0 (2026-05-28) — Taskwarrior `task export` JSON importer (Phase 19 slice 2)

Phase 19's second slice; the Taskwarrior bridge for the TUI / CLI crowd. Hand-rolled stdlib importer (no new deps; `serde_json` was already in the workspace). UDA fields routed via a new `--uda-as tag|note|drop` flag; UUIDs round-trip directly because Taskwarrior already uses RFC 4122. No schema change. Workspace 948 tests + green; clippy `-D warnings` and fmt clean; `scripts/regression.sh` passes; `appstreamcli validate` clean.

### New CLI surface

```
atrium-cli import taskwarrior PATH --into PROJECT [--uda-as tag|note|drop] [--dry-run]
```

Accepts both Taskwarrior's array form (`task export` default, `json.array=on`) and the line-stream form (`json.array=off`), one JSON object per non-empty line. UTF-8 BOM tolerated on the array form. Dry-run prints counts + lossy report without touching the DB.

### Field mapping

`description` → `task.title`. `status:pending` → open task; `status:waiting` → open task + `task.orig_keyword = "WAITING"` for Org round-trip parity; `status:completed` + `end` → `task.completed_at`; `status:deleted` and `status:recurring` parent templates skip with one lossy entry each. `uuid` → `task.uuid` directly (RFC 4122). `due` → `task.deadline`; `scheduled` → `task.scheduled_for` + `task.scheduled_time`; `wait` → `task.defer_until` (Atrium's exact schema home). `tags` array → `ensure_tag` per element. `priority:H|M|L` → `priority-1|2|3` tag. `recur` subset (`1d`, `3wks`, `2month`, `1year`, plus the common aliases) → RFC 5545 RRULE in `task.repeat_rule`; unrecognised forms drop with a `LossyKind::UnparseableRecurrence` entry. `annotations` flatten into `task.note` as `[YYYY-MM-DD] description` lines (one per annotation). `project` is dropped — the `--into` flag wins.

### UDA policy

The new `UdaPolicy` enum (`Tag` / `Note` / `Drop`) controls how user-defined attributes flow:

- **`Tag` (default):** each UDA `name:value` becomes a `name-value` tag via the same `ensure_tag` path Atrium uses for every other label source.
- **`Note`:** each UDA appends one `UDA: name=value` line to `task.note`. Preserves data without polluting the tag surface.
- **`Drop`:** UDAs surface in a single `LossyKind::DroppedUda` entry per task and otherwise vanish. Defensive option for users who want hand triage.

The flag is rejected on other importers (`org`, `todoist`, `vtodo`) so muscle-memory typos surface as clean argv errors.

### Lossy report

Eight `LossyKind` variants: `Deleted`, `ActiveAtImport` (the `start` field — Atrium has no per-task active state), `DroppedUntil`, `UnparseableRecurrence`, `DroppedRecurringChild` (the `parent`/`mask`/`imask` machinery), `DroppedRecurringTemplate`, `DroppedDepends` (with a hint that v0.29.0 will round-trip these once dependencies ship), `DroppedUda`. `urgency` is dropped silently — it's a computed metric with no schema home.

### Module layout

```
atrium-cli/src/import/taskwarrior.rs            ← aggregator
atrium-cli/src/import/taskwarrior/parser.rs     ← serde_json → typed TaskwarriorTask + UDA stash
atrium-cli/src/import/taskwarrior/mapper.rs     ← import_taskwarrior + LossyKind + UdaPolicy routing
atrium-cli/src/import/taskwarrior/round_trip_tests.rs  ← integration tests via tempfile DB
```

Mirrors `atrium-cli/src/import/todoist/` exactly, plus the `#[cfg(test)] mod round_trip_tests` pattern from v0.25.0's VTODO work.

### Tests

Four fixtures + 13 unit tests + 6 integration tests:

- `tests/fixtures/taskwarrior/basic.json` — single pending task with description / project / tags / priority H / due / scheduled / RFC 4122 UUID.
- `tests/fixtures/taskwarrior/multi.json` — six rows covering pending + waiting + completed + deleted + recurring parent + recurring child (each status path exercised).
- `tests/fixtures/taskwarrior/recurring.json` — four `recur` shapes: `1d`, `1week`, `3month`, plus a deliberately-unparseable `"every other Tuesday"` for the lossy assertion.
- `tests/fixtures/taskwarrior/uda.json` — two tasks with `client`/`effort`/`estimate` UDAs for the three `UdaPolicy` paths.
- Parser unit tests: array + line-stream forms, UTF-8 BOM, status lowercasing, integer `imask`, date parse, annotation parse, UDA stash, top-level rejection.
- Mapper unit tests: priority tagging, `recur` subset, UUID normalisation, UDA-policy tag collection, annotation formatting, `should_create` skips, `record_lossy` complete coverage, `build_new_task` field routing for the modeled subset.
- Integration tests: basic round-trip via tempfile DB, multi-status round-trip with `WAITING`/`COMPLETED` parity, three `UdaPolicy` paths against the UDA fixture, `recur` translation against the recurring fixture.

### Argv

`src/tests.rs` gains five new cases: default `--uda-as tag`, each of the three valid `--uda-as` values, missing `--into` error, bad `--uda-as` value error, and the cross-importer rejection (`import org --uda-as` errors).

## v0.25.0 (2026-05-28) — VTODO import + export (Phase 19 slice 1)

The CalDAV-side `.ics` bridge to Endeavour, Errands, Nextcloud Tasks, and Planify. Phase 19's first slice; Taskwarrior, todo.txt, and the unified import dialog defer to later minor cuts so each surface ships with focused tests and a focused patchnote. Workspace 910 tests + green; `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` clean; `bash scripts/regression.sh` passes; `appstreamcli validate` clean.

### Dependency call: hand-roll

The dependency-discipline question for Phase 19 was whether to take on the `ical` crate or hand-roll the VTODO subset. The hand-roll won on grounds of consistency: Atrium's Org parser was hand-rolled, the Todoist importer was hand-rolled, the vault sidecar TOML was hand-rolled. The savings from `ical` are limited to tokenisation + line-folding + escape decoding, all bounded code (~150 lines); the Atrium-specific mapping layer (typed columns, RRULE round-trip, lossy report) is the bulk of the work regardless. No new dependencies; stdlib only plus `chrono`, `uuid` (v5 feature already pulled in by the Todoist importer), `thiserror`.

### New CLI surface

- `atrium-cli import vtodo PATH --into PROJECT [--dry-run]` — read a `.ics` file, create a new project, import every VTODO. Dry-run prints counts + the lossy report without touching the DB.
- `atrium-cli export vtodo PATH [--dry-run]` — write the entire DB to a single `.ics` file. If `PATH` is a directory, the file lands at `PATH/atrium.ics`. Dry-run prints counts without writing.
- Both surfaces respect `--json` / `--tsv` / `--human` for the summary printer.

### Field mapping (spec §7.5)

| VTODO | Atrium |
|---|---|
| `SUMMARY` | `task.title` |
| `DESCRIPTION` | `task.note` |
| `DUE` | `task.deadline` (date portion only; time-of-day truncates) |
| `DTSTART` | `task.scheduled_for` + `task.scheduled_time` (the v0.19.0 column carries the time portion) |
| `COMPLETED` | `task.completed_at` |
| `STATUS:COMPLETED` | sets `completed_at = now()` only when COMPLETED property absent |
| `STATUS:IN-PROCESS` / `CANCELLED` | stashed in `task.orig_keyword` for round-trip parity |
| `PRIORITY` 1–4 | `priority-N` tag (matches Todoist shape; 5–9 emits no tag) |
| `CATEGORIES` | `task.tag` rows via `ensure_tag` (idempotent dedupe) |
| `RRULE` | `task.repeat_rule` (verbatim — RFC 5545 is RFC 5545, no translation) |
| `UID` | `task.uuid` when UUID-shaped; otherwise v5-derived + stashed in `extra_properties["VTODO_UID"]` |
| `LOCATION` | `task.extra_properties["VTODO_LOCATION"]` (lossless via v0.24.0) |
| `X-*` | `task.extra_properties[X-*]` (lossless via v0.24.0) |

### UID round-trip anchor

Atrium's `task.uuid` is UUID v4 by contract, but a VTODO UID is free-form (`task@nextcloud.example.com`, `12345`, anything). The v0.24.0 `extra_properties` column unlocks the clean round-trip: a UUID-shaped UID threads directly into `task.uuid`; anything else lands as `UUIDv5(VTODO_NAMESPACE, original_uid)` for `task.uuid` and the original gets stashed in `extra_properties["VTODO_UID"]`. On export, the stashed value wins, so receiving CalDAV apps see their UID unchanged. The frozen v5 namespace ensures re-imports of the same source land on the same row — same trick the Todoist mapper uses for its `(project_name, title)` v5 derivation.

### Scope guardrails

- **No CalDAV client.** No HTTP, no auth, no sync. Spec §7.2 carries this.
- **No VTIMEZONE generation.** Export is UTC-only; receiving CalDAV apps universally accept UTC. Non-UTC timestamps on import drop the timezone and surface `LossyKind::DroppedTimezone`.
- **No VEVENT / VJOURNAL / VFREEBUSY handling.** One lossy entry per top-level non-VTODO component, then skipped.
- **No VALARM round-trip.** Atrium's reminders are separate (`task.reminder_at`); cross-mapping VALARM ↔ reminder is deferred. Each VALARM block inside a VTODO surfaces a `LossyKind::DroppedAlarm` entry.
- **No multi-file vault.** `.ics` convention is one calendar per file; multi-file is the JSON snapshot's job.

### Parser surface (hand-rolled)

The `atrium-cli/src/vtodo/parser.rs` implements the subset Atrium needs:

- Line unfolding per RFC 5545 §3.1 (CRLF + SPACE/TAB continuation).
- TEXT-typed escape decoding per §3.3.11 (`\n`, `\N`, `\,`, `\;`, `\\`).
- Property tokenisation: `KEY[;PARAM=VALUE...]:VALUE` with quoted-value support.
- DATE / DATE-TIME parsing (`YYYYMMDD`, `YYYYMMDDTHHMMSSZ`, floating local).
- Component nesting via `BEGIN:X` / `END:X` stack walk.
- Tolerant skip of VEVENT / VJOURNAL / VTIMEZONE / X-* blocks.
- Per-VTODO lossy tally tracking alarm count, attendee count, GEO flag, PERCENT-COMPLETE, DURATION, TZID flag, unknown property names.

### Emitter surface (hand-rolled)

`atrium-cli/src/vtodo/emit.rs` writes RFC 5545-compliant `.ics`:

- VCALENDAR header with versioned `PRODID:-//Atrium//atrium-cli vX.Y.Z//EN`.
- One VTODO per task, modeled lines first then `extra_properties` in source order.
- UTC for all timestamps (`YYYYMMDDTHHMMSSZ`); date-only DTSTART / DUE carry `;VALUE=DATE:`.
- TEXT escape encoding (newlines → `\n`, commas → `\,`, semicolons → `\;`, backslashes → `\\`).
- Line folding at 75 octets with CRLF + space continuation. UTF-8 char-boundary safe.
- Atomic write via `atrium_core::sync::atomic::write_atomic`.

### Lossy report

`LossyKind` mirrors the Todoist mapper's shape. Each VTODO contributes one entry per dropped construct kind (groups duplicates by name in `UnknownProperty`). Top-level components surface as `UnsupportedComponent` without a task title. The summary printer emits TSV / JSON / human formats; scripted consumers can grep for `:DroppedAlarm:` / `:DroppedTimezone:` etc.

### Test coverage

Five fixtures + four integration tests + ~30 unit tests:

- **`tests/fixtures/vtodo/basic.ics`** — single VTODO with every modeled field (SUMMARY, DESCRIPTION with escapes, DUE, DTSTART date-only, STATUS, PRIORITY, CATEGORIES, RRULE, LOCATION, UUID-shaped UID).
- **`tests/fixtures/vtodo/multi.ics`** — three VTODOs covering open-with-schedule, COMPLETED-with-cookie, CANCELLED-status round-trip.
- **`tests/fixtures/vtodo/lossy.ics`** — full drop-coverage: VEVENT + VJOURNAL siblings, plus a VTODO with TZID timestamps, ATTENDEE × 2, GEO, PERCENT-COMPLETE, DURATION, RESOURCES, URL, two VALARM blocks, and an X-* property that should survive.
- **`tests/fixtures/vtodo/nextcloud_sample.ics`** — hand-crafted Nextcloud Tasks shape (CALSCALE, CREATED / LAST-MODIFIED, free-form UID).
- **`src/vtodo/round_trip_tests.rs`** — basic-fixture modeled-subset round-trip, Nextcloud original-UID-via-extras preservation, lossy report shape, multi-task status round-trip.
- **`src/vtodo/parser.rs#tests`** — line unfolding, escape decoding, basic VTODO parse, X-* stash, TZID flag, VALARM + ATTENDEE counts, non-VTODO skip, VTIMEZONE tolerance, error paths (unmatched END, unclosed component, mismatched END kind), property tokenisation incl. quoted params, COMPLETED date-only promotion.
- **`src/vtodo/emit.rs#tests`** — basic VTODO round-trip via parse-emit-parse, escape encoding for newlines / commas / semicolons / backslashes, 75-octet line folding, date-only DTSTART, empty VCALENDAR wrapper, extras-after-modeled ordering.
- **`src/vtodo/mapper.rs#tests`** — UID resolution (UUID-shaped, free-form v5 derive, none), priority-N tag round-trip, date+time vs date-only shape, dry-run summary counts, lossy entry emission for the full drop-coverage set.

### CLI argv

`atrium-cli/src/tests.rs` gains four `parse_args` cases verifying the new `import vtodo PATH --into PROJECT [--dry-run]` and `export vtodo PATH` shapes round-trip correctly. The `--into` requirement on import raises a clean error when omitted; the parser surfaces both `org | todoist | vtodo` in its error text.

## v0.24.0 (2026-05-28) — Custom property-drawer passthrough (Post-v0.22.0 Tier 1)

The Post-v0.22.0 Tier 1 item right after subtasks, and the last documented Org round-trip data loss. Before today, the importer cherry-picked four well-known `:PROPERTIES:` drawer keys (`ID`, `EFFORT`, `DEFER_UNTIL`, `RRULE`) and silently dropped every other `:KEY: value` line. Custom `:CATEGORY:`, `:CLIENT:`, `:URL:`, anything a user might park in a drawer: gone on the first round-trip. Spec §7.3.3 rule 1 ("preserve unknown constructs verbatim") held for body content (Org tables, source blocks, lists, links survived in `task.note`) but not for property drawers. v0.24.0 closes that gap.

Workspace 905 tests + green; `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` clean; `bash scripts/regression.sh` passes; `appstreamcli validate` clean.

### Schema

Migration `0014_task_extra_properties.sql` adds `task.extra_properties TEXT NULL`, a JSON object stashing every unmodeled `:KEY: value` from a task's `:PROPERTIES:` drawer. `user_version` 13 → 14. Backwards-compatible additive change: v0.23.x binaries reading a v0.24.0 DB ignore the column, and a v0.24.0 binary opening a v0.23.x DB applies the migration on first run.

JSON-on-column shape mirrors the v0.18.0 `quick_entry_template.default_tags` precedent: `serde_json` encode at the worker write boundary, defensive `unwrap_or_default()` decode at the read boundary so a malformed blob never poisons a query. Empty map normalises to NULL (cheaper than storing `{}`); the read boundary normalises NULL or malformed back to an empty `BTreeMap` at the Rust boundary.

### Modeled set + partition contract

A new `MODELED_PROPERTY_KEYS` constant in `atrium_org::org` is the single source of truth for which `:KEY:` names map through typed columns:

```
ID  CREATED  MODIFIED  DEFER_UNTIL  EFFORT  RRULE  ORIG_KEYWORD
```

Keys in this set are never written to `extra_properties` (the importer / watcher filter them out, the writer ignores them on merge). Everything else round-trips through the JSON column. `CREATED` / `MODIFIED` aren't currently read by the importer (Atrium's `created_at` / `modified_at` triggers stamp them at write time), but they're listed defensively: a manual user-set `:CREATED:` value would otherwise round-trip-conflict with the schema-managed timestamp. `ORIG_KEYWORD` isn't emitted as a property today (the keyword sits on the headline) but listing it future-proofs.

The helper `extras_from_properties(&HashMap<String, String>) -> BTreeMap<String, String>` is the single partition function consumed by the importer (`atrium-org/src/org/import.rs`), the vault watcher's create + diff paths (`atrium-org/src/vault_watcher.rs`), and the emit-time merge (`atrium-org/src/org/write.rs`). One contract, three consumers, no drift.

### Data layer (atrium-core)

`Task`, `NewTask`, and `TaskUpdate` all carry `extra_properties` now. `Task.extra_properties: BTreeMap<String, String>` (empty is the natural "no extras" state — no `Option<BTreeMap>` wrapper to thread). `NewTask.extra_properties: BTreeMap<String, String>` with `Default::default()` falling through cleanly. `TaskUpdate.extra_properties: Option<BTreeMap<String, String>>` (whole-map replace, not a per-key delta — the watcher rewrites the drawer on any change anyway, so the simpler shape carries its weight). `TaskUpdate::extra_properties_value(map)` is the new builder; `BTreeMap::new()` clears the column.

The worker's `create_task` adds `extra_properties` to the INSERT column list, JSON-encoded when non-empty and NULL otherwise. `update_task` pushes `extra_properties = ?` into its `sets` vec when the field is `Some(...)`. `complete_repeating_task` (the recurring-task respawn path) carries the extras forward so a `:CLIENT: Acme` on a daily standup keeps `:CLIENT: Acme` on every roll-forward instance.

`TASK_COLUMNS` in `db/read/mod.rs` gains the new column; `task_from_row` decodes it with `unwrap_or_default()` on both the NULL and malformed paths. The JSON snapshot exporter (`sync/json.rs`) round-trips automatically since `Task` derives `Serialize`/`Deserialize`; `#[serde(default)]` on the new field keeps pre-v0.24.0 snapshots loadable (missing field deserialises to an empty map).

### Vault watcher (atrium-org)

`ParsedTask::to_new_task` (the external-add path) populates `NewTask.extra_properties` via `extras_from_properties`. `ParsedTask::diff_from` (the external-edit path) builds a parsed `BTreeMap`, compares against `existing.extra_properties`, and pushes `extra_properties_value(...)` when they differ. An Emacs user adding `:CLIENT: Acme` in the drawer now flows back into the DB column; an Emacs user changing `:CLIENT: Acme` to `:CLIENT: BetaCo` syncs the new value; an Emacs user deleting the line clears the key from the column.

### Writer (atrium-org)

`task_to_org_task` (which builds the `OrgTask` shape the emitter consumes) seeds its local `properties: HashMap<String, String>` with the four modeled keys it always emitted (`ID`, `RRULE`, `EFFORT`, `DEFER_UNTIL`), then extends with `task.extra_properties` via `entry().or_insert_with`. The `or_insert_with` defends against a hand-crafted DB row that puts a modeled-name key in `extra_properties` — the typed column always wins. The existing alphabetical-sort emit pass (`org/emit.rs:202`) handles ordering uniformly; modeled keys and extras interleave by name rather than getting a special section.

### No UI

Confirmed by the Phase 18.5 research note that closed this item: real-world Org users almost never depend on arbitrary properties beyond the modeled set (Karl Voit explicitly says he uses only `CREATED`, `ID`, and link-related properties; cmdln.org-2024 doesn't mention custom keys at all). Adding an Inspector affordance would be over-design. Custom keys round-trip silently: a user who adds `:CATEGORY: Q3-deliverables` in Emacs gets it back on every vault read, and Atrium-side edits don't disturb it.

### Tests

- `org_importer_round_trips_custom_property_keys` (renamed from `documented_limit_org_importer_drops_custom_property_keys`): the previously-documented gap is now an asserted round-trip. `CATEGORY` / `CLIENT` / `URL` all survive with their exact values.
- `org_importer_round_trips_many_custom_property_keys`: a stress fixture with eight extras (`ALPHA` through `ZETA` plus `FLAG` and `URL`) — `FLAG` is an empty-value key, since the parser permits `:FLAG:` with no value, and the round-trip assertion confirms empty-string preservation.
- `db::worker::tests::create_task_persists_extra_properties`: round-trips a populated map through `create_task` + read.
- `db::worker::tests::create_task_empty_extras_round_trips_as_empty`: a freshly-created task with no extras compares `==` to one round-tripped through the column. The NULL → empty-map normalisation is part of the contract.
- `db::worker::tests::update_task_replaces_extra_properties_whole_map`: confirms whole-map replace semantics — keys absent from the replacement get dropped.
- `db::worker::tests::update_task_clears_extras_with_empty_map`: confirms `extra_properties_value(BTreeMap::new())` clears the column.
- `external_custom_property_drawer_round_trips_through_db` (vault_watcher_integration): a full Emacs-side add → DB sync → writer-side re-emit → second Emacs-side edit → DB sync cycle. Covers the create, change-value, and remove-key paths.

### Migration housekeeping

The three pinned `user_version == 13` assertions (`db::tests::migration_applies_cleanly`, `migration_is_idempotent`, `open_creates_parent_dir_and_migrates`, plus `db::read_pool::tests::acquire_release_round_trips`) all bump to 14. Standard migration-bump hygiene; no behaviour change.

## v0.23.1 (2026-05-28) — Subtasks GUI bugfixes from hands-on verification

A focused patch closing the four GUI gaps a hands-on run after v0.23.0 surfaced. The headless layer was solid; the bind / signal wiring on the row factory had three holes plus one error-formatting leak. No schema change; workspace stays at 899 tests green.

### List indent now updates live

`apply_nesting` stamps `depth` on each `AtriumTask` and re-sorts the store. The row factory's `connect_bind` reads `depth` and applies `margin_start = 12 + depth * 18` — but `connect_bind` only fires when GTK re-binds an item to a row, which the post-sort `items-changed` only triggers for rows whose visual slot moved. Rows whose depth changed without their position changing kept the old margin. The fix wires `task.connect_depth_notify` to set the row's margin in step, mirroring the existing pattern for `cookie-label` / `area-color` / `row-state`. Unit-tested values flowed through fine; the visual side had no coverage until the manual pass.

### Cookie reactivity in the list

Toggling a subtask in the Inspector pane left the parent row's `[done/total]` cookie stale. The diff applier (`apply_changes_seq`) only re-resolved cookies for tasks present in `changes.updated`, and the worker doesn't synthesise a parent-row update when a child's status changes. v0.23.1 collects every `parent_id` referenced by the `created` / `updated` deltas and refreshes those parents' cookies against the freshly-built `cookie_for` snapshot before the frame lands. The resolver's signature changed to `Fn(i64, &str) -> String` (id + note) so the refresh pass can call it against `AtriumTask` rows already in the store without a `Task` round-trip.

### Drag-to-reorder widened beyond Inbox

`handle_reorder` gated to `ActiveList::Inbox` since Phase 4 — pick-it-up's "fails safe to reorder" assumption needed reorder to actually work on project pages (where the Shift-drag-reparent verification ran). v0.23.1 allows reorder on every position-ordered view: Inbox, Anytime, Someday, Project, Area. Time-sorted canonical views (Today, Upcoming, Forecast, Logbook) and read-only debug views still no-op; reorder on those has no persistent meaning. A short debug-log line records the rejection so future-me can see when it fires.

### Drop-position bias on the target row

Plain drag-reorder now reads the cursor's vertical position on the target row: top half lands the task *above* the target, bottom half lands it *below*. Matches the GNOME / macOS / OmniFocus list idiom. Implemented as a `DropBias { Above, Below }` enum threaded from the drop callback through `handle_reorder` (the neighbour-position math now picks the prev or next row by bias instead of by `src_pos < dest_pos`, and skips `src_id` so dragging onto an adjacent row's far half doesn't snap back to the current slot). Shift+drop still reparents anywhere on the row — vertical position only matters for the reorder branch.

### Shift detection fallback for Wayland DnD

`gtk::DropTarget::current_event_state()` can return an empty modifier set during some Wayland compositor DnD sequences (the controller hasn't received an event of its own at the moment the drop completes). v0.23.1 falls through to the keyboard device's live modifier state on the display's default seat: `widget()` → `display()` → `default_seat()` → `keyboard()` → `modifier_state()`. The new `task_list::shift_held(&DropTarget)` helper centralises the read; debug tracing records the resolved value alongside the drop coords for future diagnosis.

### Error-message Debug leak

`DomainError::ParentProjectMismatch`'s thiserror format used `{parent_project:?}` for an `Option<i64>`, surfacing `Some(1)` to CLI consumers. The format now resolves the option via `map_or_else(|| "unfiled", id.to_string())`, so the rejection reads `parent task 2 is in project 2; cannot host a child claiming project 1` — bare ids, with `unfiled` for the `None` case.

### Maintenance

`pick-it-up.md`'s handoff note was retired; its three "do this first" items all landed. The demo DB's verification task (`task 43 — VERIFY v0.23.0 — drag onto a parent`) was deleted as part of cleanup.

## v0.23.0 (2026-05-27) — Subtasks UI (Phase 19.5)

The first Phase 19.5 productivity essential after the v0.20.0 foundations. `parent_id` has been in the schema since `0001_initial.sql` and the Org importer has always built the tree, but the GUI rendered tasks flat; v0.23.0 surfaces real nested tasks end to end. No schema change. Workspace 899 tests; clippy `-D warnings` + fmt clean; `appstreamcli validate` clean.

### Data layer (atrium-core)

- `TaskUpdate` gains `parent_id` (`Option<Option<i64>>`) and a `reparent()` builder so a task's parent can change after create. The worker's `update_task` applies it with validation: the same-project rule `create_task` already enforced, plus a cycle guard (`would_create_cycle` walks the parent chain and rejects self-parent and descendant cycles) surfaced as the new `DomainError::ParentCycle`.
- `read::list_subtasks(conn, parent_id)` lists a task's children ordered by position.

### CLI (atrium-cli)

- `add --parent ID` (inherits the parent's project when `--project` is omitted) and `edit --parent ID | none` (reparent / promote to top level) via the new `EditParent` enum.
- `info` shows a "Subtasks (N)" section in `--human` output; the TSV / JSON one-record shape is unchanged for jq / grep consumers.

### GUI (atrium)

- **Builder Inspector pane** gains a "Subtasks" group above Notes: lists children with completion checkboxes (toggled through the worker), each row navigates to the child, and a permanent "Add subtask" entry creates a child inheriting the parent's project. Per spec §5.1 the group is Builder-only (Simple Mode hides subtask UI).
- **List nesting:** children render indented under their parent in list views. `AtriumTask` gains `parent_id` + `depth`; a pure `nesting_order` helper orders rows depth-first (siblings by position, orphans as roots, cycle-safe); `apply_nesting` stamps depth + re-sorts and runs after both the full refresh and the incremental delta. Pinned-sort search / perspective views stay flat. The row factory indents 18 px per depth level.
- **Drag-to-reparent:** Shift+drop makes the dragged task a child of the drop target; a plain drop still reorders. Worker rejections (cycle or different project) surface a toast.
- The existing `[done/total]` row cookie already counted `parent_id` children (since v0.15.0), so it reflects subtasks with no new code.

### Naming

The v0.15.0 body-checkbox group (`- [ ]` items in the note) is renamed "Subtasks" to "Checklist" in both Inspector surfaces, freeing "Subtasks" for real nested tasks. Things-3 taxonomy: body items are a checklist; nested tasks are subtasks.

### Deliberately deferred

Completion cascade is omitted: completing a parent does not auto-complete its children (the cookie shows progress instead). Subtasks stay hidden in the Simple-mode dialog per spec §5.1.

## v0.22.1 (2026-05-27) — Metainfo capitalisation + post-v0.22.0 planning

Documentation and metadata maintenance: no code, behaviour, or schema changes.

- **Metainfo capitalisation.** Every `<release>` description's `<p>` blocks now open with a capitalised word, clearing the last `appstreamcli validate` infos. The 15 lowercase-leading paragraphs were mostly intentional proper nouns (`window.rs`, `atrium-org`, version strings); each got a short capitalised lead-in rather than mangling the noun. `appstreamcli validate` is now fully clean. These were info-level and never blocked Flathub, but the output is pristine now.
- **Post-v0.22.0 roadmap planning.** Added a prioritised `Post-v0.22.0 priority order` section to `roadmap.md`: Tier 1 (subtasks UI exposure, Org custom-property-drawer passthrough), Tier 2 (VTODO import/export, task dependencies) with full per-item todo breakdowns, and Tier 3 / Tier 4 cross-referencing Phase 19.5 / Phase 20.

## v0.22.0 (2026-05-27) — Module-tree split of the two largest source files

A structural maintenance release: no behaviour changes, no schema changes. The audit that drove the v0.21.x patches flagged `window.rs` (6105 lines) and `inspector_pane.rs` (1921 lines) as the repo's outstanding structural debt; both are now decomposed into module trees. 888 tests stay green throughout, with clippy `-D warnings` and rustfmt clean at every step.

### window.rs becomes window/

The largest file in the repo: a single 156-method `impl AtriumWindow` plus roughly 1140 lines of free-function helpers, carved into a `window/` directory.

- `mod.rs` (425, was 6105) keeps the `imp` subclass, the `glib::wrapper!`, `ActiveList`, the lifecycle constructors, and re-exports the helpers.
- `sidebar.rs`, `shell.rs`, `lists.rs`, `tasks.rs`, `views.rs`, `search.rs`, `actions.rs` each hold one `impl AtriumWindow` block for a cohesive method group.
- `widgets.rs` holds the free-function helpers (primary menu, sidebar row / badge builders, search-help popover, history ring buffer, dialog prompts).
- `tests.rs` holds the relocated test module.

Mechanism: child modules use `use super::*` to inherit `mod.rs`'s imports; cross-module methods were bumped to `pub(super)`; free-function helpers are re-exported from `mod.rs` via `pub(crate) use widgets::*`. The composite-template path was re-anchored for the deeper file location. Landed in three passes (test module, impl carve, free-function extraction), each independently green.

### inspector_pane.rs becomes inspector_pane/

- `mod.rs` (949) keeps `struct InspectorPane`, its impl, and the 735-line `build_editor` (left whole so its imports stay local).
- `fields.rs` (931) holds the field / widget builders, the repeat-rule editor, and the small parsers.
- `tests.rs` holds the relocated test module.

### Notes for future maintenance passes

- `cargo clippy --fix` with `redundant_closure_for_method_calls` is broken against our `adw` / `gtk` crate aliases: it emits the real `libadwaita` crate name and fails to compile. Exclude that lint from any `--fix` run touching the `atrium` binary.
- The `window.rs` carve was done bottom-up so upper line numbers stayed stable across successive cuts; chunk boundaries had to avoid slicing a method's doc comment away from its `fn`.

## v0.21.2 (2026-05-27) — Metainfo XML escaping fix

A packaging-correctness patch surfaced by the v0.21.1 audit. No code or behaviour changes.

The AppStream metainfo (`data/io.github.virinvictus.atrium.metainfo.xml`) had several historical release entries whose description text contained raw, unescaped XML metacharacters: `<file>` / `<UTC>` in the v0.10.x conflict-flood note, a `<2026-05-11 Mon ++1w>` SCHEDULED cookie in the v0.10.3 note, and a `bar.connect_entry(&entry)` ampersand in a later note. Most entries escaped these correctly (`&lt;` / `&gt;` / `&amp;`); these few slipped through. The result: the file failed `xmllint` well-formedness and `appstreamcli validate`, which would have blocked the Phase 20 Flathub submission.

All four spots are now escaped. `appstreamcli validate` passes (only non-blocking `description-first-word-not-capitalized` infos remain, all in older entries). `xmllint --noout` is clean.

## v0.21.1 (2026-05-27) — Documentation + lint maintenance patch

A small follow-on to v0.21.0's maintenance pass, prompted by a full-codebase audit. No behaviour changes, no schema changes; the workspace stays at 888 tests with `cargo clippy --workspace --all-targets -- -D warnings` clean and `cargo fmt --all --check` clean.

### Documentation drift fixed

The audit found the spec header had not moved with the code since v0.12.0, plus a few stale internal references. All corrected against the v0.21.0 reality:

- **`spec.md` header.** Version `0.12.0` → `0.21.0`; the phase-18 parenthetical replaced with an accurate summary of the v0.14.0 → v0.21.0 arc (Phase 18.5 power features, Phase 19.5 foundations, the v0.21.0 maintenance pass).
- **`spec.md` §3.3.** "The workspace ships four crates as of v0.5.0" corrected to six as of v0.13.0; `atrium-org` and `atrium-inline` added to the topology; the `atrium` binary's "all three above" corrected to "all five above".
- **`spec.md` §4.3.** The search grammar's home corrected from `atrium-core/src/search/` to the `atrium-search` crate (extracted at v0.4.2).
- **`CLAUDE.md`.** "Five workspace crates" → "Six"; the build block's test-count example refreshed (`817 at v0.13.0` → `888 at v0.21.0`); the codebase-map migration line corrected (`0007` / `user_version 7` → `0013` / `user_version 13`).

`roadmap.md`'s shipped-phase bullets that name the pre-extraction `atrium-core/src/sync/org/` paths were left intact deliberately: they describe where the code lived at v0.7.x, and the v0.9.0 extraction bullet already records the move.

### Clippy cleanup

Ten files received behaviour-preserving, machine-applicable clippy fixes (the residue after v0.21.0's pedantic sweep): five `unnested_or_patterns` rewrites (`Some(A) | Some(B)` → `Some(A | B)`) in the library crates, and roughly nine `map_unwrap_or` rewrites in the GTK binary (`map().unwrap_or()` → `map_or()`, with two collapsing to `is_some_and()` / `is_none_or()`). Implementation note recorded for future passes: `clippy --fix` with `redundant_closure_for_method_calls` is broken against our `adw` / `gtk` crate aliases (it emits the real `libadwaita` crate name and fails to compile), so that lint must be excluded from any `--fix` run touching the `atrium` binary.

### Audit findings deferred

The audit's larger structural items remain queued for v0.22.0 as already planned: splitting `atrium/src/ui/window.rs` (6105 lines, one 156-method `impl`) and `inspector_pane.rs` (1922 lines). A phased split plan was drafted (test module first via the `#[path]` idiom, then free helpers, then the `impl` block carved into sidebar / pages / tasks / search / dialogs / actions modules).

## v0.21.0 (2026-05-10) — Maintenance pass: rapid-growth deferred work

Seven minor releases shipped in one extended session (v0.14.0 → v0.20.0; +4972 LOC across 44 files in the bundled commit `67c7e7c`). v0.21.0 is the punctuation — a deliberate maintenance pass before any new feature work, sequenced by blast radius (small wins → focused refactors → structural splits → test coverage). No behaviour changes for the user; the audit findings are documented here so future-me has the receipts.

### Audit shape

Two-pass survey (delegated `Explore` agent first, then a deeper hand-pass after the agent's report turned out to under-rate the structural debt — the agent never opened `atrium/src/ui/window.rs`, the largest file in the repo at 6105 lines).

The deeper pass added: `cargo clippy -W clippy::pedantic` (909 raw warnings, ~250 actually actionable after stripping doc-style noise + must-use suggestions); `unwrap()` / `expect()` audit on production-only code (~38 occurrences, almost all defensive GTK template downcasts in `ui/task_list.rs`); long-function census via clippy `too_many_lines` (5 functions exceed 100 lines, `worker.rs::handle()` at 366 the standout); and a test-coverage gap pass (notably `atrium-cli/src/args.rs` at 1561 lines with zero tests).

### What landed in v0.21.0

#### Bag of small wins

- **Migration 0013 — `task_clock_entry` timestamps.** v0.17.0's CLOCK table shipped without `created_at` / `modified_at` (every other table in the schema has them). Added as nullable columns with a backfill (`created_at = started_at`; `modified_at = COALESCE(ended_at, started_at)`) plus the standard `WHEN OLD = NEW` trigger. Worker INSERT paths stamp them explicitly; readers populate from the row. `user_version` 12 → 13.
- **Stale Org parser docstring.** `atrium-org/src/org/parse.rs:37` claimed "active timestamps lose time-of-day" — wrong since v0.19.0's `task.scheduled_time`. Updated to reflect the post-v0.19.0 reality (date and time-of-day split into separate columns).
- **Quick Entry shortcut sniff validation.** The modal's `:LETTER ` sniff now rejects non-ASCII-alphanumeric trigger characters at the GUI layer, mirroring `validate_shortcut_key` in the worker. Prevents a `:🎉 ` from attempting template matching that could never succeed.
- **Reminder service `Utc::now()` consolidation.** Loop iteration captures one `now` for the lookup + sleep-window calculation rather than calling `Utc::now()` three times. The post-sleep re-check still needs a fresh timestamp (the outer `now` is from before the sleep) — that one stays.
- **Targeted clippy pedantic sweep.** Auto-fixed via `cargo clippy --fix`: 27 `format!("{}", x)` → `format!("{x}")` modernizations, 22 redundant closures (`|x| f(x)` → `f`), 18 `map().unwrap_or()` → `map_or()`, 8 `match`-as-`let-else` rewrites. Touched 24 files; no behaviour changes; clippy `-D warnings` still green.

#### Test coverage gap fill — `args.rs` newer subcommands

`atrium-cli/src/args.rs` had no inline test module (the audit's "1561 lines, 0 tests" finding). Investigation revealed a separate `atrium-cli/src/tests.rs` covering the older subcommands well, but with no coverage for v0.17.0+ additions. Added 34 tests inline in `args.rs` covering: clock (status / log / in / out + the bare-`clock`-as-status alias), template (list / add / minimal + with shortcut), `--deadline-warn` and its `--warn` alias, negative-warn rejection. Workspace total now 888 tests (was 854).

A second item from the audit — Quick Entry shortcut-sniff unit tests (item 12) — was deferred. The sniff logic is inline in a GTK closure; testing it cleanly needs extraction first, which is a separate refactor.

#### `atrium-cli/src/main.rs` partial split

`atrium-cli/src/main.rs` (2604 lines, 76 functions). Pulled out the two most isolated subcommand surfaces:

- `atrium-cli/src/clock.rs` (245 lines) — `clock status / log / in / out` + `print_one_entry`
- `atrium-cli/src/template.rs` (217 lines) — `template list / add / edit / remove` + `find_template_by_name` + `resolve_project_id`

`main.rs` is now 2155 lines (saved 449). `CliError`, `CliResult`, `json_escape` promoted to `pub(crate)` so sub-modules can use them. Dispatch sites in `main.rs` unchanged — sub-module exports re-imported by name via `use clock::*` / `use template::*` (explicit list). The remaining `perspective` / `export` / `import` orchestration stays in `main.rs` for now (later extraction passes can pull more out).

#### `read.rs` partial split

`atrium-core/src/db/read.rs` (2288 lines, second-largest in `atrium-core`). Converted to a `read/` directory with four sub-modules carved out:

- `read/clock.rs` (127 lines) — clock-entry queries (CLOCK_COLUMNS, clock_entry_*, list_clock_entries, total_clock_minutes, active_clock, clock_entries_per_project)
- `read/counts.rs` (228 lines) — counting queries (CanonicalCounts, count_open_*, count_done_total_*, count_tasks)
- `read/search.rs` (162 lines) — SQL fast-path + FTS5 (SqlBindValue, list_tasks_matching, search_tasks, bm25_for_terms)
- `read/templates.rs` (64 lines) — Quick Entry template queries
- `read/mod.rs` (1780 lines) — task / area / project / heading / tag / perspective queries; shared TASK_COLUMNS + task_from_row used by sub-modules via `super::`

Pragmatic split — pulled out the four most-isolated sections rather than fragmenting all 11 surfaces into separate files. Public API unchanged: callers continue to use `crate::db::read::function_name`. The 1051-line test module stays in `mod.rs` (uses `super::*`).

#### Worker dispatch helper methods

`atrium-core/src/db/worker.rs` `handle()` (366-line dispatch loop, the standout `too_many_lines` warning in the maintenance audit). Each Command arm used to inline a 5-7 line "send delta + maybe notify dirty" body. Factored that out into 12 small helper methods on `Worker` — `emit_task_created` / `emit_task_updated` / `emit_area_*` / `emit_project_*` / `emit_tag_*` / `emit_perspective_*` (created/updated/deleted variants per kind). Each dispatch arm is now uniformly 5-7 lines: call work fn → check Ok → call helper → respond.

Helpers chosen over a macro on the basis that regular Rust is debugger-friendly and IDE go-to-def works through normal method calls. The helpers are reusable for future commands and individually testable.

## v0.20.0 (2026-05-10) — Phase 19.5 foundations: preferences window + system-notification reminders

Phase 18.5 wrapped at v0.19.0; Phase 19.5 (productivity essentials) opens with two pieces that pair naturally — a real preferences dialog and the first notification surface, with the dialog exposing the toggle that gates the new reminder service. Both items have been deferred for several minor cycles; landing them together avoids two separate "settings shape" conversations.

### Preferences window — `AdwPreferencesDialog`, three pages

The first app-level preferences UI in Atrium. Closes a long-standing gap where the only way to adjust the app was `gsettings set io.github.virinvictus.atrium ...` from a shell. Built on `AdwPreferencesDialog` (libadwaita 1.6+) — the predecessor `AdwPreferencesWindow` is deprecated in favour of dialogs.

- **General page.** Default mode (Simple / Builder, drives the existing `mode` GSettings key), theme override (Follow system / Light / Dark — new `theme` key, applied via `adw::StyleManager::set_color_scheme` immediately on change and replayed at boot), high-legibility font toggle (Atkinson Hyperlegible — wires the existing `high-legibility-font` key), vault path with a folder picker (`gtk::FileDialog::select_folder`).
- **Capture page.** Quick Entry shortcut as a single `AdwEntryRow` accepting GTK accelerator syntax (`<Control><Alt>space`). Backed by the existing `quick-entry-shortcut` key. The runtime accelerator listener already rebinds when the key changes; no separate rebind plumbing needed.
- **Notifications page.** Master switch (`notifications-enabled`, default true) — gates the v0.20.0 reminder service. Off-by-toggle is observed by the service on every fire, so flipping the switch takes effect without restart.
- **Wiring.** `app.preferences` action with `Ctrl+Comma` accelerator; primary menu's "Preferences…" entry triggers it. Each page is a single function returning an `AdwPreferencesPage` so adding the Phase 20 Backups page later is a one-method addition.

### Reminder service — single tokio task, `gio::Notification` per fire

The first time-based notification surface. Per-task `reminder_at` UTC timestamps drive a single tokio task on the GLib MainContext that polls the next pending reminder, sleeps until it fires, and emits a notification. Designed deliberately as the GUI's reminder owner — Phase 20's `atriumd` will own out-of-process reminders later, so this service only runs while the app is open.

- **Schema.** Migration `0012_task_reminder_at.sql` adds `task.reminder_at TEXT NULL` (RFC 3339 UTC) plus a partial index `idx_task_reminder_at_open` on open future reminders only (`WHERE reminder_at IS NOT NULL AND completed_at IS NULL`) — keeps the dispatch query fast on libraries with thousands of past reminders. `user_version` 11 → 12.
- **Read helper.** `next_pending_reminder(conn, after) -> Option<(i64, DateTime<Utc>)>` — single row, soonest open future reminder. Used by the service loop.
- **Service.** `atrium::reminders::spawn(pool, app)` returns a cheap-to-clone `ReminderService` exposing `wake()`. The loop wakes from either a `tokio::time::sleep` to the next reminder OR a `tokio::sync::Notify` ping; sleep is capped at one hour as a defensive re-query against clock jumps + suspend/resume. The TaskChanges bridge (`bridge_task_changes`) calls `wake()` after every batch so freshly-set reminders take effect without a timer wait.
- **Notification shape.** `gio::Notification` titled "Reminder" with the task title as body. Notification ID is `atrium-reminder-{task_id}` — re-firing for the same task replaces rather than stacks. Default action is `app.show-task::ID` (parameterised i64 action; opens the inspector for that task).
- **Master-switch behaviour.** When `notifications-enabled` is false at fire time, the reminder is silently skipped (the loop continues). Documented limitation: disabling notifications during an open reminder window swallows that reminder permanently — re-enabling does not back-fill. A "last-fired-at" column would address this; deferred until a real user asks.
- **CLI.** `atrium-cli add --reminder "YYYY-MM-DD HH:MM"` accepts a local-time timestamp and stores it as UTC. Mirrors the `--time` flag style from v0.19.0.

### Tests + ship gate

3 new tests: `next_pending_reminder_returns_soonest_open_task` and `next_pending_reminder_returns_none_when_all_past_or_completed` (`atrium-core/src/db/read.rs`) cover the dispatcher's lookup, plus `update_task_sets_and_clears_reminder_at` in `worker_tests.rs` for the column round-trip. The notification fire path itself isn't unit-tested — it touches `gio::Application::send_notification`, which a unit test can't observe. Workspace passes 854 tests; fmt + clippy clean. Schema version 12.

## v0.19.0 (2026-05-10) — Phase 18.5 Tier-2 close-out: Org links + scheduled time

Phase 18.5 finishes with both Tier-2 items bundled into one minor: ID-based links between tasks (`[[id:UUID][label]]`) and time-of-day on the SCHEDULED cookie. **All seven Phase 18.5 items now shipped** across v0.14.0 → v0.19.0; Phase 19.5 productivity essentials are next.

### Org links between tasks

The org-roam-adjacent power feature, with zero schema impact. Karl Voit's UOMF advocates ID links for portability; Atrium already generates `:ID:` UUIDs that round-trip cleanly, so the only missing pieces were body-text recognition + clickable rendering + an insertion affordance.

- **Body-link parser.** New `atrium_core::links` module surfaces `parse_body_links(body) -> Vec<BodyLink>` with byte ranges, target UUIDs, labels, and a "has explicit label" flag. Forgiving: malformed brackets, non-`id:` link types (`file:` / `https:` / `mailto:`), and unterminated constructs are silently skipped. 10 unit tests cover each shape.
- **Inspector body rendering.** `gtk::TextTag` with foreground accent + underline applied to every link range; re-applied on every buffer change so live edits keep highlights. Click gesture walks the iter at the click position to a parsed link, invokes a navigation callback. Both modes (Builder pane + Simple Mode dialog).
- **Click navigation.** New `task_id_for_uuid` read helper resolves the link's UUID to a row id; the window routes through the existing `open_inspector_for(id)`. Stale links (UUID points to a deleted task) silently no-op rather than erroring — the user's click was a navigation attempt, not a state mutation.
- **Link… picker (Builder Mode).** `notes_group` gains a header-suffix button → popover with `gtk::SearchEntry` + filtered `gtk::ListBox` (case-insensitive substring match against title, capped at 50 rows). Clicking a row inserts `[[id:UUID][title]]` at the cursor. Lazy pool resolution via the new `pool_source` install closure — the picker stays read-pool-agnostic.

### Time-of-day on scheduled

The Todoist mapper's `DroppedTimeOfDay` lossy entry is finally closed.

- **Schema.** Migration `0011_task_scheduled_time.sql` adds `task.scheduled_time TEXT NULL` (HH:MM format). Companion-column shape rather than upgrading `ScheduledFor` to a sum type — keeps the existing TEXT sort semantics intact and avoids the API ripple of changing `ScheduledFor::Date(NaiveDate)` to a sibling variant. `user_version` 10 → 11.
- **Org parser/emitter.** Parser captures the time token between the day name and any repeater/warning suffix into `OrgTask.scheduled_time`. Emitter writes it back in canonical `<DATE Day HH:MM +Nx -Md>` order — time before suffixes per Org's standard layout. Round-trip stable. Importer + watcher diff path thread the column through new task creates and external-edit syncs. DEADLINE time-of-day is recognised but silently dropped — Atrium has no `deadline_time` column, and an explicit round-trip would need a sibling addition; defer until a real user asks.
- **GUI.** Inspector pane (Builder) gets a Time `gtk::Entry` row beneath Schedule. Visible only when scheduled_for is a Date (Someday + None can't carry a meaningful time). Parses `HH:MM` on focus-leave and dispatches `TaskUpdate::scheduled_time_value`. Forecast day cards prefix Scheduled-reason rows with `HH:MM` when present (Deadline + DeferEnds rows are time-blind). Calendar Month View shows the time on inline day-cell rows + the day-peek popover.
- **CLI.** `--time HH:MM` flag on `add` accepts a 24-hour time; combined with `--scheduled today` / `--scheduled YYYY-MM-DD` to produce a date+time capture from the shell. Todoist mapper retired its `DroppedTimeOfDay.push` site — the recurrence parser already extracted the time, the mapper now threads it into the new column.

### Tests + ship gate

10 new link-parser tests in `atrium-core::links` + SCHEDULED time round-trip in the Org parser/emitter + 1 worker `scheduled_time` set/clear test + smokes for the Calendar / Forecast time prefix render. Schema version 11.

## v0.18.0 (2026-05-10) — Phase 18.5 Tier-1: Quick Entry templates

The fifth and final Phase 18.5 Tier-1 item lands. **All five Phase 18.5 Tier-1 items are now shipped** — Atrium's surface is meaningfully different from "another GNOME todo app" in the ways the original Phase 18.5 research said real Org users would notice: per-task DEADLINE warning windows (v0.14.0), statistics cookies + body inline checkboxes (v0.15.0), custom TODO sequences (v0.16.0), CLOCK time tracking (v0.17.0), and Quick Entry templates (v0.18.0). The remaining Phase 18.5 work is Tier-2 polish (Org links between tasks, time-of-day on `scheduled_for`); after that, Phase 19.5 productivity essentials.

**Schema.** Migration `0010_quick_entry_template.sql` adds the `quick_entry_template` table — `id`, `name` (UNIQUE), `shortcut_key` (UNIQUE, NULL = no shortcut), `target_project_id` (FK SET NULL on project delete; NULL = Inbox), `prefix`, `default_tags` (JSON array string), `position`. `modified_at` trigger uses the same WHEN OLD = NEW pattern as elsewhere. `user_version` 9 → 10. Worker enforces the "single ASCII alphanumeric character" rule on `shortcut_key` (a CHECK constraint would work in SQL but the error message would be cryptic; a worker-side check yields a clear `DomainError::InvalidShortcutKey`).

**Worker + read.** Three new commands (`CreateQuickEntryTemplate`, `UpdateQuickEntryTemplate`, `DeleteQuickEntryTemplate`) plus their `WorkerHandle` shims. Read helpers: `quick_entry_template_by_id`, `list_quick_entry_templates` (ordered by position). The default_tags JSON encode/decode happens at the worker/read boundary; the GUI sees `Vec<String>`. `DomainError::InvalidShortcutKey { got }` is the new variant; falls under the existing `DbError::Domain` umbrella so no new error-tree plumbing needed.

**Quick Entry modal.** New picker bar above the entry, hidden when no templates are configured. Each template renders as a `gtk::ToggleButton` labelled `"shortcut · name"` (or just name when there's no shortcut). Clicking activates: pre-fills the entry text with the template's `prefix`, stashes the template object for commit, and toggles off any other active picker button (mutual-exclusion). Clicking the active button again deactivates and clears the entry text.

**Shortcut sniff.** The modal's `connect_changed` handler watches typed text for `:LETTER ` (colon + single character + space). When LETTER matches a template's `shortcut_key`, the template auto-activates: entry text becomes the template's prefix, the matching picker button toggles on. The trailing-space requirement avoids hijacking `:c` mid-word; the user has committed to the trigger before activation fires. Once a template is active, the sniff is dormant — no re-interpretation of `:` characters in the template's body.

**Commit semantics.** Active template threads through to `commit()`: `target_project_id` becomes the new task's project; `default_tags` merge with inline `#tag` parser output (template tags first, parsed tags appended unless they collide by case-insensitive name). Empty entry input still rejects (no template-only captures); the user has to type a real title to commit.

**CLI.** `atrium-cli template list` (TSV / JSON / human formats with project resolution). `atrium-cli template add NAME --shortcut LETTER --project NAME --prefix TEXT --tag TAG` (project resolves via case-insensitive substring match against `project.title`; multi-match errors out with the candidate list). `atrium-cli template edit NAME --rename NEW --shortcut LETTER|none --project NAME|none --prefix TEXT --tag TAG`. `atrium-cli template remove NAME` (case-insensitive lookup).

5 new tests in `worker_tests`: round-trip create, multi-char shortcut rejected, non-alphanumeric shortcut rejected, delete returns NotFound on second attempt, update changes fields. Schema version 10.

## v0.17.0 (2026-05-10) — Phase 18.5 Tier-1: CLOCK time tracking

The flagship Phase 18.5 feature. One of the top two reasons real users stay on Org-mode instead of Things or OmniFocus — actual-time tracking distinct from `estimated_minutes` (intent), accumulated across multiple work sessions, with full round-trip to Org's `:LOGBOOK:` drawer so Emacs users see the same data. The largest schema-affecting item in Phase 18.5 (one new side table, one new worker command surface, parser + emitter + watcher all extended), and the differentiator that makes Atrium meaningfully more than "another GNOME todo app."

**Schema.** Migration `0009_task_clock_entry.sql` adds the `task_clock_entry` side table — `id`, `task_id` (FK CASCADE), `started_at`, `ended_at` (NULL = running), `note`. Index on `(task_id, started_at)`. `user_version` 8 → 9. The single-active-clock invariant (at most one row across the entire table has `ended_at IS NULL`) is enforced by the worker rather than a partial unique index — keeps the schema simple, and the worker is the only writer anyway. v0.16.x binaries reading a v0.17.0 DB ignore the table.

**Worker.** Three new commands: `ClockIn { entry, responder }`, `ClockOut { task_id, responder }`, `DeleteClockEntry { id, responder }`, plus `ImportClockEntry { task_id, started_at, ended_at, note, responder }` for the watcher and importer paths (caller-provided timestamps, skips the auto-close invariant since the source file is trusted). `clock_in` on task B auto-closes any other open entry on task A first — mirrors Emacs's global clock; what every Org user expects when they `org-clock-in` on a different headline. Re-clocking on the *same* task surfaces the existing open entry rather than double-stamping (returns `previously_closed_task_id: None` so the dispatcher knows nothing changed). Both directions emit `TaskChanges` for the affected task(s) so the inspector pane refreshes.

**Read layer.** New `clock_entry_by_id`, `clock_entry_task_id`, `list_clock_entries` (per-task, newest-first), `total_clock_minutes` (closed-only, computed via SQLite's `julianday` arithmetic), `active_clock` (the single-row lookup), and `clock_entries_per_project` (the writer's batch loader; one query per project flush rather than per task).

**Inspector pane (Builder).** New "Time" group between Notes and Builder fields. Renders three things: a Start/Stop `gtk::Button` (label flips based on running state), a "Total HH:MM" row (hidden when zero), and a per-session log of `(duration, started)` rows. Open entries surface as "Running — started …". Click Start → worker.clock_in; click Stop → worker.clock_out. Auto-refreshes on TaskChanges so a CLI clock-in reflects in the open inspector.

**Org parser/emitter.** New `OrgTask.clock_entries: Vec<OrgClockEntry>` field carrying captured CLOCK lines from a `:LOGBOOK:` drawer, plus `logbook_unknown_lines` for verbatim round-trip of state-change log lines and other non-CLOCK content. Parser recognises both shapes — `CLOCK: [start]--[end] => H:MM [note]` (closed) and `CLOCK: [start]` (running). Emitter writes the closed form only; in-progress entries are deliberately suppressed (the file would churn every running second; the next clock-out flushes). Timestamps treated as UTC throughout (mirrors the existing CLOSED-cookie convention).

**Watcher.** New `sync_clock_entries(task_id, parsed_entries)` walks each task's parsed CLOCK lines after the main diff. Matches by `started_at` (sub-second precision means starts are unique per task in practice); inserts added entries via `import_clock_entry`, deletes ones the file no longer carries, refreshes ones whose `ended_at` or note changed (delete-and-reinsert). External Emacs clock-out flows back into the DB; external CLOCK-line additions create matching DB entries.

**CLI.** `atrium-cli clock in <id> [--note TEXT]` opens an entry (auto-closing any other). `clock out <id>` closes the open entry on a task (soft no-op when none). `clock log <id>` prints all entries for a task in TSV / JSON / human format with totals. Bare `atrium-cli clock` (or `clock status`) shows the currently-running entry across the DB.

**Open-by-design limitation.** Org timestamps emit + parse in UTC. Mirrors the long-standing CLOSED-cookie behavior (since v0.7.x). Users in non-UTC timezones see UTC clock times when they open the file in Emacs — accurate but unfamiliar. The Inspector pane displays in `chrono::Local` so the GUI feels right; only the file representation is UTC. Switching the on-disk representation to local time means asymmetric parse-emit (parse-as-local + emit-as-local) and risks broken round-trip across timezone changes; deferred.

12 new tests across worker (clock_in opens, auto-close-on-different-task, clock_out idempotent on no-open-clock, clock_in-then-out records duration), Org parser (4 LOGBOOK shapes including malformed-line preservation), and emitter (closed roundtrip, in-progress suppression, mixed-state emit). Schema version 9.

## v0.16.0 (2026-05-10) — Phase 18.5 Tier-1: custom TODO sequences

The most-cited Org feature in the v0.6.19 research pass — Bernt Hansen's NEXT-replaces-priority workflow, Jethro Kuan's processing pipeline, every GTD-with-Org tutorial — lands. Per-vault declared TODO keyword sequences round-trip through Atrium's vault sync end-to-end: the writer projects them as `#+TODO:` preambles, the watcher validates against them, the Inspector pane picks from them, the CLI manages them. Zero schema impact (sequences live in the sidecar; existing `task.orig_keyword` from v0.7.12 carries the labels through Atrium's TODO/DONE binary).

**Sidecar.** `<vault>/.atrium/config.toml` gains a `[[todo_sequences]]` array-of-tables slot. Each entry carries `name`, `workflow` (open keywords), and `done` (completion keywords) — mirroring Org's `#+TODO: STATE1 STATE2 | DONE1 DONE2` shape. The hand-rolled TOML parser learns one new value type (single-line inline string arrays); the rest of the schema stays unchanged. Empty `todo_sequences` emits a commented placeholder so an Emacs-side power user editing the sidecar by hand sees the section is intentional. Backward-compat: pre-v0.16.0 sidecars without the section parse cleanly to an empty Vec; existing tag colours + perspectives + mode survive untouched.

**Writer.** `write_project_to_vault` reads the sidecar's first sequence (single-sequence-per-vault is the typical Org pattern; multi-sequence support stays available in the sidecar shape but the writer projects only the first into `#+TODO:` for now). When configured, every project file's preamble carries `#+TODO: workflow | done`. The vault-writer's sidecar refresh path now reads the on-disk sequences before re-emitting (the DB doesn't carry sequences, so without this merge every flush would silently erase them).

**Watcher.** New `keyword_is_done` + `keyword_is_known` helpers walk the configured sequence's sets. The `to_new_task` and `diff_from` paths thread the sequence through; `org_keyword_to_orig` learns to stash workflow keywords under sequence-aware logic so a NEXT or WAITING task round-trips with its label intact. Unknown keywords (out of both sets) surface a new `VaultEvent::UnknownKeyword { source, keyword }` event so the GUI can prompt the user — they still preserve via the existing Custom path; never destroy data.

**Builder Mode Inspector.** New `adw::ComboRow` keyword picker between the title row and the dates group. Visible only when a sequence is configured (no sequence = no override = the title-row checkbox stays the binary toggle). Selection writes through to `task.orig_keyword` + `task.completed_at` together — picking a workflow keyword reopens; picking a done keyword stamps `now()` (or preserves the existing completion timestamp on a re-pick).

**CLI.** `atrium-cli vault sequences list/set/clear --vault PATH`. Vault path is required because atrium-cli is process-isolated from the GTK GSettings store. `set --workflow TODO,NEXT,WAITING --done DONE,CANCELLED` replaces the configured sequence outright. Output respects `--json` / `--tsv` / `--human`.

**Architecture choice flagged for follow-up.** The watcher reads the sidecar on every diff event. That's cheap (small file, buffered I/O) and means a sidecar edit takes effect immediately without restart, but if profiling ever shows it as a bottleneck the writer's existing `last_sidecar` cache pattern can mirror to the watcher.

15 new tests across the sidecar (round-trip with single + multi sequences, missing-section degrades cleanly, string-array parse with embedded escapes), the watcher (keyword_is_done + keyword_is_known + org_keyword_to_orig under varied sequence configurations), and the writer (#+TODO: emit gated on sequence presence). Schema unchanged at version 8.

## v0.15.0 (2026-05-10) — Phase 18.5: statistics cookies + body inline checkboxes

Two Phase 18.5 features land bundled because they reinforce each other: Karl Voit names both essential, and Org's `org-checkbox-hierarchical-statistics` (default on) folds body checkboxes into parent statistics cookies as one unified count. The bundle ships zero migrations and works in both modes.

**Statistics cookies — Tier-1.** The Org parser learns to recognise `[done/total]` and `[N%]` cookies on the headline between the title and tags. Captured cookies are stripped from the title text (so `task.title` stays clean) and stashed on a new `OrgTask.statistics_cookie` field whose variant preserves the user's chosen shape across the round-trip. The writer's new `stamp_statistics_cookies` walker computes the values fresh from DB state on every emit — a stale `[2/5]` self-heals to `[3/5]` the next time the writer flushes. New `count_done_total_per_project` + `count_done_total_per_parent` SQL helpers in `atrium-core::db::read` feed both the writer projection and the GUI inline cookie label.

**Body inline checkboxes — Tier-2.** New `atrium_core::checkbox` module surfaces a small forgiving parser: `- [ ]`, `- [X]`, `- [-]` (plus `+` and `*` bullet variants for Org compatibility), with a `toggle_body_checkbox(body, line_index)` helper that rewrites a single line in place. The body string stays the source of truth — the Inspector renders a projection above the Notes textview as a list of `gtk::CheckButton` rows; clicking a checkbox toggles the line in the buffer, which triggers the Inspector pane's worker dispatch (Builder Mode) or simply updates the buffer that the dialog's Apply will pick up (Simple Mode, transactional). 14 unit tests in the checkbox module cover all three states, alternative bullets, indented checkboxes, indeterminate-clears-to-unchecked toggle semantics, trailing-newline preservation, and done/total counting.

**Cookie + checkbox integration.** The cookie counter folds body-checkbox done/total alongside child-TODO counts. A parent task with three subtasks (one DONE) and four body checkboxes (two checked) gets `[3/7]`. A task with no subtasks but a body checklist still earns a cookie. Mirrors what every "Org statistics cookies aren't recursive by default" tutorial expects — child counts stay local to the immediate parent (per `org-hierarchical-todo-statistics`'s default), but body checkboxes count regardless.

**GUI surface.** Task list rows whose task has children or body checkboxes show the cookie as a small dim suffix on the title (between the title and the tag pills). Mirrors Org's headline shape: title, cookie, tags. New `.atrium-task-cookie` CSS class with tabular figures so `[3/5]` doesn't shift width when a count crosses a digit boundary. The cookie label updates live as children get toggled (the diff applier re-runs the cookie resolver on every TaskChanges update).

**Open architectural work for follow-ups (deliberately not in v0.15.0).**

- **Inline checkbox embedding inside the textview.** v0.15.0 renders checkboxes as a separate "Subtasks" group above the textview. Org-mode-style in-text widgets (a clickable `[ ]` rendered inside the rich text) require GtkTextChildAnchor + add_child_at_anchor scaffolding. Polish item; the current shape is functional.
- **Cookie display in the sidebar projects.** Sidebar projects already carry numeric badges from `count_open_per_project`; v0.15.0 doesn't touch them. The vault file gets the cookie projection, and the per-task row shows the cookie. Switching the sidebar badges to the cookie shape would be visual cleanup, not a feature.
- **Recursive cookie counting.** Org's `org-hierarchical-todo-statistics` defaults to non-recursive (immediate children only); we follow. A custom Atrium recursive variant could come if real users ask.

14 unit tests in atrium-core's new `checkbox` module + cookie-shape tests in the Org parser + emit roundtrip tests + cookie-projection tests in the writer. No GTK unit tests this release; the visible behaviour is exercised through the regression smoke. Schema unchanged at version 8.

## v0.14.0 (2026-05-10) — Phase 18.5 Tier-1: DEADLINE warning windows

First Phase 18.5 item lands. Org-mode's per-deadline `-Nd` / `--Nd` warning suffix (`DEADLINE: <2026-04-15 Wed -7d>` — "surface 7 days early") becomes a per-task override on Atrium's previously-global `TODAY_DEADLINE_WINDOW_DAYS` constant. A sensitive deadline can now surface in Today earlier than the default 7-day heads-up window without disturbing how unmarked tasks behave; users coming from Org-mode get the round-trip they expect. The roadmap calls this out as the lowest-cost Tier-1 item — schema is one additive column, the read query gains a `COALESCE`, and the Org parser/emitter learns to recognise the suffix shape it had been ignoring.

**Schema.** Migration `0008_task_deadline_warn_days.sql` adds `task.deadline_warn_days INTEGER NULL`. `user_version` 7 → 8. Append-only per the post-v0.2.0 schema rule; existing rows default NULL = "use the global default." v0.13.x binaries reading a v0.14.0 DB ignore the column.

**Today list + sidebar badge.** `db::read::list_today` + `count_open_canonical.today` both moved their static `today + N` horizon into a per-row SQL expression: `deadline ≤ date(?today, '+' || COALESCE(deadline_warn_days, ?default) || ' days')`. The badge count and the list contents stay in lockstep — one COALESCE in two queries. The pure-Rust mirrors in `task_list::CanonicalList::Today::matches` and `atrium_search::eval::is_in_today_list` got the same treatment so in-memory filter evaluation agrees with the SQL.

**Org parser/emitter.** `parse_timestamp_inner` walks the post-date tokens once and pulls the first that matches `+`/`++`/`.+` (repeater) and the first that matches `-`/`--` (warning) regardless of order — Org allows either sequence. Both warning prefixes parse to the same `u32` days; Atrium normalises onto the single-dash form on emit since there's no global-default-override concept that would distinguish them. Day units land canonically; week/month/year units (`-2w`, `-1m`, `-1y`) fold into 14/30/365-day approximations on parse so the integer-day column stays straightforward. SCHEDULED-side warnings are accepted and round-trip verbatim into a sibling `OrgTask.scheduled_warning` field — Atrium doesn't model them in the DB but won't drop them on a round-trip.

**Vault watcher diff.** External Emacs edits to the `-Nd` suffix flow back into `task.deadline_warn_days` via the existing diff path (a new `parsed_warn != existing.deadline_warn_days` arm dispatches `TaskUpdate::deadline_warn_days_value`). Tested end-to-end: append a TODO with `-7d` → DB lands at `Some(7)` and the writer round-trips the cookie; flip the suffix to `--14d` → DB lands at `Some(14)` and the writer normalises onto `-14d`; remove the suffix entirely → DB clears to NULL.

**Builder Mode UI.** `inspector_pane.rs` gains an `adw::SpinRow` ("Heads-up window", range 0–60, step 1) wired to the deadline-button callback so the row's visibility tracks whether the task has a deadline set. `0` in the SpinRow means "use the default 7"; any positive value writes the override via `TaskUpdate::deadline_warn_days_value`. Autosaves on value-changed, consistent with the existing pane's pattern. Simple Mode's modal Inspector dialog stays unchanged — Builder-only UI exposure, schema column round-trips through the vault regardless of mode (mode-as-view, per the Phase 10 acceptance test).

**CLI.** `atrium-cli add --deadline-warn N` (alias `--warn`) and `atrium-cli edit --deadline-warn N|none`. Negative integers reject at parse time; `none` clears the column. `info`'s human-readable output gains a `warn  N days before deadline` row when the column is set.

**Tests.** Six new parser tests cover `-7d`, `--7d` normalisation, repeater + warning in either order, `w`/`m`/`y` unit folding, and SCHEDULED-side verbatim round-trip. Three new emit tests cover plain warning round-trip, repeater + warning canonicalisation order, and `--7d → -7d` normalisation. Four new read-layer tests cover per-task override winning over the global default, override below the default still being honoured, `warn=0` semantics (deadline-or-past only), and badge-vs-list parity. One worker test covers set/clear via `TaskUpdate`. One end-to-end watcher integration test covers external add → DB sync → writer round-trip → external flip → DB sync → external clear → DB clear.

11 new tests across parser, emitter, read layer, worker, and end-to-end watcher scenarios; full ship gate (`scripts/regression.sh`) passes. Schema version 8.

## v0.13.5 (2026-05-09) — Vault: seed registers writes in RecentWrites

Hotfix on top of v0.13.4. The fresh-vault seed bypassed `RecentWrites` (the shared self-write filter set the writer + watcher both consult to suppress self-induced echoes), so on every fresh-vault boot the watcher saw all 50 of Brandon's seeded `.org` files as external edits, fed them through the worker as no-op updates, the writer flushed each project, and the writer's pre-flush conflict check legitimately treated each file as foreign — backing every one up to `<file>.atrium.bak.<UTC>`. Boot logs filled with 50 spurious "vault conflict: external edit" warnings; `~/Tasks` collected 50 stale backup files.

Fix wires the missing handshake. New `VaultLoopHandle::recent_writes()` accessor returns a clone of the shared `Arc<RwLock<RecentWrites>>` so out-of-band writers can register the files they wrote before `attach_watcher` consumes the handle. The seed in `boot_data_layer` snapshots this Arc before attaching the watcher, then in the per-project loop stats each file's mtime immediately after `write_project_to_vault` and records `(path, mtime)` in the set. By the time the watcher's 200 ms debounce fires, all 50 records are in — its self-write filter matches and skips the events; the worker never sees an update; the writer never wakes up to back anything up.

Validation on Brandon's box: deleted `~/Tasks/.atrium/config.toml` to re-trigger the seed path. Boot log:

```
INFO atrium: vault watcher attached
INFO atrium: fresh vault seeded from DB count=50
```

Zero conflict warnings; zero `.atrium.bak.*` files left behind. Pre-fix the same boot produced 50 of each.

Test count holds at 829. Schema unchanged at v7. The race-window analysis is documented in the commit body.

VERSION + Cargo.toml + patchnotes.md + AppStream metainfo bumped to 0.13.5.

## v0.13.4 (2026-05-09) — Vault: seed-on-first-boot for fresh vault paths

The Phase 17 vault loop is change-driven — the VaultWriter only fires when the worker emits `notify_project_dirty(project_id)`. Setting `vault-path` on a fresh empty directory and restarting Atrium therefore did *nothing* visible: the existing DB sat unmirrored until the user edited a task. The expected behaviour is "see my existing tasks mirrored to disk on first connect", and that previously took a separate `atrium-cli export org PATH` invocation.

**Fix:** on every boot, after the vault loop attaches, check for `<vault>/.atrium/config.toml`. The writer creates that sidecar on every project flush, so its absence is a reliable "never been written" signal. When absent, spawn a tokio task that calls `write_all_projects_to_vault` against a fresh read connection, then writes the sidecar via `build_from_db` + `write_sidecar`. Two-phase because `write_all_projects_to_vault` doesn't touch the sidecar itself; without the explicit sidecar write, every subsequent boot would re-detect a fresh vault and re-seed.

**Why background, not synchronous:** a 10K-task DB takes a couple of seconds to dump and the cold-start budget (spec §8) is < 250 ms. The seed spawns on the existing tokio runtime so the GTK window appears immediately while the seed runs in the background. Errors are best-effort logged; `atrium-cli export org PATH` remains available as the manual fallback.

**Backward compat is verbatim.** Existing populated vaults (sidecar present) skip the seed unchanged — only fresh-or-erased vaults trigger it. Pre-existing `.org` files in a vault that's somehow lost its sidecar get backed up to `<file>.atrium.bak.<UTC>` by the v0.10.1 conflict-detection path before the seed overwrites them — hand-edits are never lost, only relocated.

`read_vault_setup_from_settings` now returns the vault path alongside the config + loop + receiver tuple, so `boot_data_layer` can detect the sidecar without re-reading GSettings.

Manual smoke (the path Brandon hit):

```
gsettings set io.github.virinvictus.atrium vault-path ~/Tasks
mkdir -p ~/Tasks   # if absent
cargo run -p atrium
# After ~250ms the GUI is up; shortly after, ~/Tasks/ contains
# every project's .org file plus ~/Tasks/.atrium/config.toml.
```

Test count holds at 829. Schema unchanged at v7.

VERSION + Cargo.toml + patchnotes.md + AppStream metainfo bumped to 0.13.4.

## v0.13.3 (2026-05-09) — Org emit: blank line between successive headlines

Pure stylistic patch found by visual inspection of an exported vault — the Org writer was emitting headlines back-to-back with the `:END:` of one drawer butting directly against the `*` of the next headline. Org-agenda parses both forms cleanly, but Emacs's own writers (with the default `org-blank-before-new-entry`) always insert a blank line before a new headline. Reading Atrium's vault in DoomEmacs surfaced the visual cramping immediately — every headline read as part of one big block instead of as discrete entries. A single-line addition in `emit_task` pushes a trailing newline after the last piece of headline content and before recursing into children; the recursion unwinding handles every separator (parent → first child, sibling → sibling, last child → parent's next sibling). The parser tolerates blank lines anywhere outside drawers so spec §7.3.3 round-trip discipline is intact. One existing byte-exact test (`emit_no_cookie_line_when_no_dates`) updated to expect the trailing blank. Test count holds at 824. Schema unchanged at v7.

## v0.13.2 (2026-05-09) — inline-rename gets the tab-completion popover

The atrium-inline Slice 3 popover (v0.13.0) wired into the bottom-of-list capture entry and the Quick Entry modal but deliberately skipped the per-row inline-rename `Entry`: "the row's edit Entry recycles frequently and the popover lifecycle would need additional teardown bookkeeping." This patch closes that gap.

**Where it attaches:** the GtkSignalListItemFactory's `setup()` callback, which runs *once per row's lifetime* — ahead of `bind()` and across recycles. The popover lives with the title_entry widget, not with whichever task is currently bound. No teardown bookkeeping needed; recycling a row swaps the bound task, not the popover.

**Where the pool comes from:** `build_factory` gained a fourth parameter, a `PoolFn: Fn() -> Option<ReadPool> + Clone + 'static` closure. Lazy by design — the read pool isn't attached when the factory is built (factory construction happens in window setup, before `attach_data_layer`). The closure resolves the pool fresh from a weak-window ref each time setup() needs it; by the time the first row reaches setup, attach_data_layer has long since fired.

**Behaviour matches the other surfaces.** Tab/Enter accept the highlighted candidate; ↓/↑ navigate; Esc dismisses without committing. Esc dismisses *only* the popover — the existing key controller checks the popover state structurally so Esc-to-cancel-rename still reaches the row's existing handler when no popover is open. Focus-leave dismisses the popover, then the existing focus-leave handler commits the rename and switches the stack back to the display label.

**Until the user enters edit mode** (F2 / right-click → Rename / double-click into edit), the title_entry is invisible (the title_stack shows the display label) and the popover's listeners stay quiet. No overhead per paint or per scroll. The first text change fires the refresh, evaluates the inline-syntax context, and surfaces candidates if the cursor is on a `#`/`@`/`!` token.

The `inline_complete` module's docstring is updated to drop the "defer to v0.13.x" note and document the row-level rationale.

No new tests this release — the popover wiring is GTK-side glue and the underlying parser semantics are already covered by the existing 49 atrium-inline tests + 3 inline_complete byte/char tests. Test count holds at 824 across the workspace; fmt + clippy clean; regression gate clean. Schema unchanged at version 7.

VERSION + Cargo.toml + patchnotes.md + AppStream metainfo bumped to 0.13.2.

## v0.13.1 (2026-05-09) — sidecar: perspective definitions round-trip

A small follow-up patch closing the v0.10.1 sidecar carryover. The Phase 17 sidecar (`<vault>/.atrium/config.toml`) shipped tag colors + mode preference at v0.10.1; saved Perspectives reserved a `[perspectives]` placeholder section ("for future use") because perspective definitions cross more boundaries (renderer config, column lists) and the round-trip path wasn't fleshed out yet. Phase 18's Todoist mapper showed the shape — a `Vec<entry>` flowing cleanly through the worker — and v0.13.1 applies it to perspectives.

**Format:** TOML's array-of-tables (`[[perspectives]]`), one block per entry, in stored position order. Each block carries `name`, `filter`, optional `icon`, `renderer` (`"list"` / `"board"`, defaults to `"list"` when omitted), and optional `renderer_config` (opaque JSON, survives the TOML basic-string layer's escape rules including embedded double-quotes). DB-generated fields (id, uuid, timestamps, position) are deliberately omitted — they'd confuse a hand-edit and re-import assigns fresh values matching source-file order anyway.

**Backward-compat is verbatim.** Pre-v0.13.1 sidecars carried a single-bracket `[perspectives]` placeholder followed by a `# Reserved for future use.` comment. The v0.13.1 parser still parses those cleanly — the section is treated as Unknown, contents silently dropped. The empty-Vec emit path now produces a *commented* `# [[perspectives]]` example so hand-editors see the new format's intent without breaking external tools that scanned for the old placeholder.

**Implementation note.** The hand-rolled TOML parser learned one new shape — array-of-tables. The `parse_text` cursor became a typed `Cursor` enum (`Toplevel` / `Tags` / `Perspective(usize)` / `Unknown`) so key/value lines bind structurally instead of through a string-match cascade. The "no `toml` crate dependency" decision still holds; the schema growth is bounded and explicit.

**`build_from_db`** now reads `list_perspectives` (sorted by position) and projects each row into a `PerspectiveEntry`. The end-to-end test creates two perspectives through the real worker, builds the sidecar, asserts every field including the `renderer_config` JSON survives, then verifies a text round-trip preserves the structure.

**Test count: 824 across the workspace** (up from 817 at v0.13.0; 7 new sidecar tests). Schema unchanged at version 7. No new third-party crates. Regression gate clean.

VERSION + Cargo.toml + patchnotes.md + AppStream metainfo bumped to 0.13.1. spec.md and roadmap.md unchanged — the spec already named saved Perspectives as a sidecar slot at v0.10.1; this patch implements behavior the contract already documented.

## v0.13.0 (2026-05-09) — atrium-inline: shared inline-syntax engine + tab completion

A polish + extraction arc on top of v0.12.0's Phase 18 work. The inline-syntax parser (`#tag`, `@today`, etc.) was small in v0.1 and grew steadily — Phase 6c shipped the original Quick Entry parser; Phase 18 added Todoist's mapper alongside it; v0.13.0 unifies the vocabulary, expands it (`!N` priority, `@<weekday>`), lifts the parser out of `atrium-core` into its own `atrium-inline` workspace crate, and adds a tab-completion popover so the syntax becomes discoverable instead of memorised.

Three slices, six commits in this release:

**Slice 1 — inline rename routes through `quick_entry`.** F2 / right-click → Rename / double-click into edit on a task row now runs the new title through `atrium_inline::parse`. A user can rename "Wash dishes" → "Wash dishes #urgent @today" and pick up the urgent tag plus a today schedule in one keystroke instead of opening the Inspector. Plain-text renames take a fast path identical to the pre-Slice-1 single-update flow — no behaviour change for the common case. Empty title after parsing rejects (the row never goes nameless). Title + scheduled + deadline land in a single `update_task` so the listener side sees one notify cycle. Tags are *added*, never removed (the rename surface doesn't show existing tags, so a destructive merge would surprise users — the Inspector and tag editor stay the channels for tag removal). New `ParsedEntry::is_plain_title()` lets the rename path branch on a structural check rather than a string comparison.

**Slice 2 — `!priority` + `@weekday` tokens.** Two new token shapes, both matching Phase 18's Todoist mapper vocabulary so the import → edit → re-import loop stays consistent:

- `!1` / `!2` / `!3` — set priority. 1 = high, 3 = low (Todoist convention). Strict 1-3 range matches the v0.12.0 mapper's policy: priority 4 is Todoist's default "no priority" and emits no token. `!none` / `!4` / `!9` / `!high` fall through to the title verbatim. Multi-`!N` tokens — last wins (mirrors `@today` / `@tomorrow` override semantics).
- `@<weekday>` — set scheduled_for to the next occurrence of that weekday on or after today. Both 3-letter (`@mon`) and full-name (`@monday`) forms accepted, plus aliases (`@tues`, `@weds`, `@thur`, `@thurs`). Case-insensitive (`@MON`, `@Mon`, `@mOn` all parse). When today's weekday matches the target, returns today (the "you typed `@mon` on a Monday, you mean today" call). ISO `@yyyy-mm-dd` continues to win over weekday parsing.

New `priority: Option<u8>` field on `ParsedEntry`. The typed enum sticks around so a future Phase 19.5 numeric priority column can adopt it directly without a parser change. New `ParsedEntry::projected_tag_names()` augments the free-form `#tag` set with a `priority-N` projection for capture-flavoured surfaces (Quick Entry modal, bottom-of-list entry, CLI `capture`). New `is_priority_tag_name(&str) -> bool` helper for the rename surface so it can identify stale `priority-*` tags during the merge. The rename surface uses the typed `priority` field directly so it can swap one priority tag for another atomically (single-valued semantics) without losing the user's free-form `#tag` set.

Backward compat preserved verbatim. Unrecognised `@foo` still falls through to the title (regression test pinned). Plain text renames still take the same single-update fast path.

**Slice 3 — atrium-inline crate extraction + tab completion.** Two-part slice that lifts the parser into its own crate, then wires a discovery affordance on top.

- *Crate extraction.* `atrium-core::quick_entry` → `atrium-inline` workspace member. atrium-core stays inline-syntax-agnostic; the extraction goes one way, atrium-inline → atrium-core (atrium-inline pulls atrium-core for `ScheduledFor`, never the reverse). atrium-inline's dep graph stays at chrono + atrium-core — no rusqlite, tokio, or gtk reaches it, so the post-1.0 `atrium-tui` and the v1.0 `atriumd` capture daemon can pull the parser without dragging in the storage layer. atrium-cli + the GTK binary depend on `atrium-inline` directly. Same shape as the v0.9.0 atrium-org extraction.

- *Tab-completion popover.* New `atrium/src/ui/inline_complete.rs` wires the new `atrium_inline::completions` module (pure context-detection + candidate-filtering helpers, fully unit-tested) into a small `gtk::Popover` that floats below an inline-syntax-aware `gtk::Entry`. Active when the user types `#` / `@` / `!` and shows candidates that match what they've typed so far. Tab and Enter accept the highlighted candidate; ↓ opens the popover from the closed state when the cursor is on a recognised token (mirrors how a desktop-search box reveals its suggestions on first arrow-key); Escape dismisses without committing. Focus-leave dismisses too so a click elsewhere doesn't strand the popover. `accept_candidate` swaps the partial token for the chosen candidate while preserving the user's marker character. GTK ↔ atrium-inline byte / char conversion handled by tested utf8_byte_offset / char_count_at_byte helpers.

Wired into the bottom-of-list capture entry and the Quick Entry modal. The Quick Entry modal's `open()` signature gained a third argument — `tag_pool: Option<ReadPool>` — pulled via the new `AtriumWindow::read_pool_for_quickentry` accessor (mirror of the existing `worker_handle_for_quickentry`). Inline-rename in the task-list factory deliberately stays out of scope for this slice — the row's edit `Entry` recycles frequently and the popover lifecycle would need additional teardown bookkeeping. Renames still parse through atrium-inline at commit time so `@mon` blindly typed there still applies; only the visible suggestions defer to a v0.13.x patch.

**Vocabulary curation.** The popover surfaces full-name keywords (`today` / `tomorrow` / `someday` / `deadline` / `monday` … `sunday`) and the three priority levels (`1`, `2`, `3`). The 3-letter weekday shortcuts (`@mon` / `@tue` / …) stay parser-recognised but don't clutter the suggestion list. A `schedule_keywords_match_parser` regression guard fails loudly if a new full-name keyword lands in the parser without being added to the candidate list.

**spec.md §6 — Quick Entry vocabulary.** Updated to document the v0.13 tokens and to surface the architectural commitment that the same parser drives Quick Entry, the bottom-of-list entry, the inline-rename surface, and the CLI `capture`. The same parser, the same vocabulary, four surfaces.

**Test count: 817** across the workspace (up from 798 at v0.12.0). atrium-inline itself contributes 49 tests (was 31 in atrium-core::quick_entry — 18 new, of which 13 cover Slice 2's parser additions and 5 cover the new helpers); inline_complete adds 3 byte/char-conversion tests. Schema unchanged at version 7. The regression gate (`scripts/regression.sh`) stays under 2 seconds.

VERSION + Cargo.toml + spec.md + roadmap.md + patchnotes.md + README.md + CLAUDE.md + AppStream metainfo bumped to 0.13.0.

## v0.12.0 (2026-05-09) — Phase 18: Todoist CSV import

The cross-platform productivity app most likely-to-migrate Linux user is leaving behind now has a real export path into Atrium. New `atrium-cli import todoist PATH --into PROJECT_NAME [--dry-run]` reads a Todoist CSV export, walks its row stream, and materialises the project + sections + tasks + tags + recurrence rules through the single-writer worker. Anchored to the home.csv "butter test" — Brandon's daughter Rin's chore-tracker — which round-trips Todoist → DB → vault → re-parse without losing data or scrambling structure.

**Three hand-rolled stdlib parsers.** `import::todoist::parser` (CSV → typed rows), `import::todoist::recurrence` (NL phrasing → RFC 5545 RRULE + scheduled anchor), and `import::todoist::mapper` (row stream → worker calls + ImportSummary). All three are stdlib-only — no `csv` crate, no `regex` (the workspace `regex` is for the search engine; the recurrence parser uses pattern-matching by tokenised words because it's clearer for the small phrase set). The CSV parser tolerates UTF-8 BOMs, quoted fields with embedded commas, escaped double-quotes, and blank separator rows; the TYPE column gates Meta / Section / Task / Blank classification. The recurrence parser handles every phrasing in the home.csv fixture plus sensible extensions: "Every Sunday at 10am", "every 3 day at 9am" (Todoist's singular typo), "Every 1stday" (no space), "3 days ago at 15:00" (past-dated single occurrence — no rule).

**Mapper layout.** Each Todoist row becomes a worker call: `meta` records in `summary.meta_entries`; `section` calls `WorkerHandle::ensure_heading`; `task` calls `create_task` with parent_id chain inferred from INDENT (1 = top-level, 2 = subtask of preceding indent-1, etc.). The CONTENT column's `@labels` are stripped from the title and become Atrium tags via `ensure_tag` + `set_task_tags`. PRIORITY 1-3 emits a `priority-N` tag; 4 is Todoist's default and emits no tag (keeps the noise floor low for the home.csv fixture, which is all priority-4). DESCRIPTION → `task.note`. DATE column → `repeat_rule` + `scheduled_for` via the recurrence parser; failures + dropped time-of-day, timezone, duration, deadline → per-row lossy entries.

**Position layout for vault round-trip.** Heading positions come from the worker's `next_heading_position` (1.0, 2.0, …). Top-level tasks then get an explicit `update_task` to set position = `section_idx + i * 0.001` so they slot strictly between heading rows. The Org writer's `build_project_tree` (new this release) reads that ordering and emits each section's tasks as depth-2 children of the preceding heading. Subtasks inherit per-parent positions from `next_task_position(parent_id, …)` automatically.

**`WorkerHandle::ensure_heading`.** Idempotent heading-create-by-(project_id, LOWER(title)) — mirrors `ensure_area` / `ensure_tag`. New `NewHeading { project_id, title }` input type, new `Command::EnsureHeading` variant, new `read::heading_by_id` + `list_headings_in_project` supporting reads. The handler emits `notify_project_dirty(project_id)` so a configured vault writer picks the change up. Four worker_tests pin: creates-when-absent, idempotent-per-(project, title-NOCASE), scoped-to-project, increments-position.

**Org writer learns project sub-headings.** `build_org_tree` → `build_project_tree(tasks, headings, tag_names)`. The new function takes the union of (heading rows, top-level tasks) and sorts by `position` with headings winning on tie. Walks in order: each heading becomes a depth-1 keyword-less OrgTask carrying `:ID:` (uuid); subsequent top-level tasks attach as depth-2 children of the preceding heading. Tasks before any heading stay at depth 1 — the writer behaves identically to pre-v0.12 for projects with no headings. Two new tests pin the layout: `write_emits_headings_as_depth1_sections` (interleaved 5-row layout) and `write_keeps_pre_heading_tasks_at_top_level`. The previous `headings_skipped` known-limit paragraph in the writer's docstring is gone.

**Determinism via name-based UUIDs.** Each task gets a v5 UUID derived from `SHA-1(project_name || NUL || title)` under a frozen Todoist namespace. Re-running the importer onto the same project produces stable IDs, which keeps the Org-vault `:ID:` round-trip clean across re-imports. The `uuid` crate gained the additive `v5` feature flag (pulls in sha1_smol via the existing crate). atrium-cli grew direct `uuid` and `thiserror` deps (was transitive through atrium-core).

**`atrium-cli import todoist` subcommand.** `ImportSource::Org | Todoist { project_name }` replaces the unit-only enum. `parse_import` learns `--into PROJECT_NAME`; trying it on `import org` errors out (the org file's `#+TITLE:` is canonical). The dispatcher reads the CSV, parses + maps it through the three layers, and renders the summary in TSV / `--json` / `--human`. JSON shape mirrors the Org importer's; human mode prints heading + task + tag counts plus `meta_entries` and per-row lossy notes.

**The home.csv butter test.** `home_csv_round_trips_through_db_and_vault` is the closing acceptance gate: parse the sanitised home.csv (10 sections, 46 tasks), apply through the mapper, write to a vault directory via `atrium_org::org::write_project_to_vault`, re-parse the emitted .org file, assert structural fidelity. Pinned invariants: 10 sections + 46 tasks land; 2 distinct tags survive (`chore`, `home`); first section is "Sunday: Prep for the week", last is "One offs"; total task count across the recursive tree is 46; "Check for essentials" lands at depth 2 with 7 nested children at depth 3; "Check for milk, add to list" preserves its embedded comma; the recurring parent task carries `:RRULE: FREQ=WEEKLY;BYDAY=SU` in the property drawer; `@chore` / `@home` survive as Org headline tags; no `@`-prefixed leftovers remain in any title.

**`PRIORITY=4` policy.** Todoist treats 4 as "no priority" (the default). Atrium emits no tag for it — Brandon's bias toward signal over noise, and the home.csv fixture is uniformly priority-4 so emitting `priority-4` would pollute every task. Priority 1-3 (user-elevated) does emit `priority-N` tags. When Phase 19.5's numeric priority column lands, the mapper will switch from tag projection to direct column writes; the public surface (`ImportSummary`) stays stable.

**Test count: 798** across the workspace (up from 765 at v0.11.0). Schema unchanged at version 7. The regression gate (`scripts/regression.sh`) stays under 2 seconds.

VERSION + Cargo.toml + spec.md + roadmap.md + patchnotes.md + README.md + CLAUDE.md + AppStream metainfo bumped to 0.12.0.

## v0.11.0 (2026-05-09) — Phase 12.5: Calendar Month View

Builder Mode gains a third lens over the same task data Forecast and Agenda already cover. Forecast is the 30-day strip; Agenda is the chronological-band view (Overdue / Today / Tomorrow / This Week / Next Week); Calendar is the paper-calendar grid for users who think in calendar pages. The earlier roadmap framing called Phase 12.5 "subsumed by Agenda"; that turned out to be wrong — the calendar lens is a different mental model and v0.11 re-engages it as a Builder-only canonical page (mirroring Forecast's shape, not a Perspective renderer).

**The grid.** New `atrium/src/ui/calendar.rs` ships pure date-math helpers (`first_of_month`, `grid_anchor`, `grid_end`, `last_day_of_month`, `week_rows`, `previous_month` / `next_month`, `build_month_grid`) and the GTK widget tree built on top. The grid is 7×N (Mon-start ISO weeks; matches the Agenda buckets). Each `DayCell` shows: day number, count badge when there are tasks, up to 3 inline task titles, and a "+N more" overflow popover when the day has more than fits. Today's cell carries an emphasis class; out-of-month leading / trailing cells render muted so the focal month reads cleanly.

**Navigation.** Header strip carries Prev / Today / Next buttons + a month/year `MenuButton` that opens a 4×3 month-picker popover. Page Up / Page Down step months when the calendar has focus (scoped via `gtk::ShortcutController` so the keys stay free for other surfaces). `Ctrl+Shift+M` opens the page from anywhere.

**Drag-to-reschedule.** Mirrors Forecast's pattern: each inline task title is a `DragSource` carrying the task id; each cell is a `DropTarget` accepting `i64` and updating `scheduled_for` via the worker. Out-of-month leading and trailing cells accept drops too, so users can drag into the previous or next month from the visible rows. Spec mentions a Shift-modifier for deadline-vs-schedule but defers the decision; v0.11 ships plain schedule and the modifier becomes a v0.11.x patch if Brandon asks for it.

**Click-day-to-filter.** Single-click on a cell opens a peek popover with the day's full task list — each task is a flat button that opens the inspector. Empty days surface a "Nothing scheduled" line so the affordance is consistent. Double-click drills into the standard list view via a `scheduled:YYYY-MM-DD` search expression — the user gets full editing affordances (drag, multi-select, complete) instead of being stuck in the calendar peek.

**Narrow-window collapse.** Below 600 px (`COMPACT_WIDTH_THRESHOLD`), the month grid swaps for a vertical week strip — 7 day cards stacked vertically, focused on the week containing today (or the first week of the viewed month if today's outside it). Each card shows the day's full task list inline. The window watches its own `notify::default-width` and rebuilds the calendar when the threshold flips; a `Cell<Option<bool>>` cache avoids rebuild storms during a drag-resize.

**Builder-only.** Sidebar entry "Calendar" sits between Forecast and Review in Builder Mode's top-tier extras; mode-flip filters it out in Simple. The `Ctrl+Shift+M` accelerator stays bound system-wide (`AtriumWindow::show_calendar` no-ops when in Simple) so users in Builder always get the shortcut without leaking the Builder feature into Simple's surface.

**Tests: 13 new calendar lib tests.** Cover the date-math edge cases: month boundaries (Jan 31 → Feb 1), leap February (29 days in 2024), DST transitions (March 2026 starts Sunday → 31 in-month days; November 2026 → 30), short and long months (5 vs 6 row grids), year wrap on prev/next, today-cell marking, out-of-month flagging, completed-task and deadline-only-task exclusion (the paper-calendar idiom uses the When-axis only).

**ActiveList::Calendar variant** added with `canonical_title()` returning "Calendar". `top_tier_extras(builder=true)` now produces 5 entries (Agenda, Forecast, Calendar, Review, Logbook) — the existing test pinned this and was updated.

**Test count: 650** across the workspace (up from 637 at v0.10.3). Schema unchanged at version 7. No new third-party dependencies.

VERSION + Cargo.toml + spec.md + roadmap.md + patchnotes.md + README.md + CLAUDE.md + AppStream metainfo bumped to 0.11.0.

## v0.10.3 (2026-05-09) — Phase 17 closer: RRULE canonicalisation + divergence detection + agenda-parity acceptance

v0.10.3 closes the Phase 17 patch arc. The RRULE canonicalisation contract (spec §7.3.3 rule 3) now runs end-to-end: writer emits both the best-fit Org cookie and the full `:RRULE:` property drawer entry; watcher catches the case where a user edits only the cookie in Emacs and rewrites the file to match the canonical `:RRULE:` (DB stays canonical). The agenda-parity acceptance test pins Atrium's Agenda canonical page against a spec-derived reference org-agenda classifier. **Phase 17 (vault → DB two-way sync) is closed.**

**`rrule_cookie` helpers.** New `atrium-org/src/rrule_cookie.rs` ships three pure functions:

- `rrule_to_org_cookie(rrule_text, mode)` and the typed sibling `rrule_to_org_repeater` — RRULE → cookie. `FREQ=WEEKLY` → `++1w`; `FREQ=DAILY;INTERVAL=3` → `++3d`; `FREQ=MONTHLY;BYMONTHDAY=1` → `++1m` (lossy — the BYMONTHDAY clause stays canonical in `:RRULE:`). Returns `None` only on malformed input (missing or unknown FREQ).
- `org_repeater_to_rrule(repeater)` — cookie → `FREQ=WEEKLY` or `FREQ=DAILY;INTERVAL=3`. The inverse projection; cookies can only express FREQ + INTERVAL.
- `cookie_matches_rrule(repeater, rrule_text)` — the equality check used by divergence detection. BY-clauses in the stored RRULE don't count as divergence; the cookie can't express them by design. Only flags as diverged when the user actually changed the cookie's frequency or interval.

Hand-rolled FREQ + INTERVAL parser; no `toml`-style dependency.

**Writer wiring.** `scheduled_repeater_from_task` was a `None`-returning placeholder since v0.7.10 with a comment about flipping it on later. v0.10.3 flips it on: reads `task.repeat_rule` + `task.repeat_mode`, runs them through `rrule_to_org_repeater`, returns the typed `OrgRepeater` the emitter consumes. SCHEDULED lines for repeating tasks now emit `<2026-05-11 Mon ++1w>`; the canonical `:RRULE:` still lives in the property drawer as the source of truth. Stock `org-agenda` renders the cookie; Atrium reads `:RRULE:` on read-back.

**Watcher fixes two related v0.10.0 → v0.10.2 gaps.** The `:RRULE:` property had no path through the watcher: `to_new_task` ignored it on create and `diff_from` didn't compare it on update. A user adding `BYDAY=MO,WE` to the property in Emacs would not propagate to DB. Fix: `to_new_task` now reads `:RRULE:` and threads it through `NewTask.repeat_rule`; `diff_from` compares against `existing.repeat_rule` and uses `TaskUpdate.repeat_rule_value`.

**Divergence detection.** New `collect_rrule_divergences` walks parsed headlines and flags any task whose `scheduled_repeater` (cookie) doesn't match its `:RRULE:` property under `cookie_matches_rrule`. For each divergence the watcher:

1. Emits `VaultEvent::RruleDiverged { source, title, cookie, rrule }`.
2. After the diff applies, synchronously calls `write_project_to_vault` to rewrite the file. The writer's `scheduled_repeater_from_task` projects the canonical `:RRULE:` back to the right cookie, so the file becomes self-consistent. The user's cookie edit is reverted; `RecentWrites` swallows the resulting inotify echo.

The toast: *"<title>: Org cookie diverged from `:RRULE:` — DB kept the canonical rule"*.

**Phase 17 closing acceptance test.** New `agenda_parity_with_reference_org_agenda` in `atrium/src/ui/agenda.rs` synthesises a vault with tasks across every bucket plus the "shouldn't appear" edge cases:

- `today_scheduled` / `today_deadline` → Today
- `tomorrow_scheduled` → Tomorrow
- `this_week_after_tomorrow` / `this_week_deadline` → This Week
- `next_week_start` / `next_week_end` → Next Week
- `beyond_next_week` → None
- `overdue_deadline` → Overdue
- `overdue_with_today_schedule` → Overdue (precedence)
- `no_anchor` / `someday` → None
- `completed` → None
- `deferred_future` → None

Both Atrium's `classify` and a spec-derived reference org-agenda classifier (mirroring Org's `agenda-list` day-window logic) run over each task and must agree. Visual layout / sort order between the two surfaces still differs (GTK card sections vs Emacs text agenda); the test pins SEMANTIC parity only — the contract spec §17 closes with.

**Multi-day RRULE round-trip fixture.** New `tests/fixtures/org/rrule_patterns.org` covers the three migration cases plus a daily-with-interval control:

- Weekly single-day (BYDAY=SU) — cookie `++1w` lossless when SCHEDULED is a Sunday.
- Weekly multi-day (BYDAY=MO,WE) — cookie `++1w` best-fit; canonical in `:RRULE:`.
- Monthly day-of-month (BYMONTHDAY=1) — cookie `++1m` best-fit; canonical in `:RRULE:`.
- Daily INTERVAL=3 — both representations express it.

All four round-trip through Atrium with the canonical `:RRULE:` preserved verbatim in the property drawer.

**Phase 17 status: closed.** Every checkbox under roadmap §17 ticks. The patch arc:

- v0.10.0: `notify`-backed watcher; `RecentWrites` self-write filter; reader → DB diff by `:ID:`; `:ID:` allocation on read.
- v0.10.1: GUI wiring (`spawn_vault_loop`); writer-side conflict detection; `<vault>/.atrium/config.toml` sidecar; `VaultEvent` channel; real `DomainError` / `UiError` / `AtriumError`.
- v0.10.2: malformed-file pause/resume; custom-keyword preservation (two real bugs fixed); file-removal toast; concurrent-edit + 1K-task parse latency tests; new `ParseRecovered` + `FileRemoved` events.
- v0.10.3: `rrule_cookie` helpers; writer emits both cookie + `:RRULE:`; watcher syncs `:RRULE:` to DB; divergence detection rewrites cookie-only edits to match canonical; multi-day round-trip fixture; agenda-parity acceptance test.

**What's next.** v0.11 opens Phase 18 (Todoist CSV). Phase 12.5 (Calendar Month View) is re-engaged from its earlier "subsumed by Agenda" framing — the calendar lens is a different mental model than the chronological-band Agenda or the 30-day Forecast strip; tasks are tracked but it slots after the v0.10 work closes.

**Test count: 637** across the workspace (up from 616 at v0.10.2). Schema unchanged at version 7. No new third-party dependencies.

VERSION + Cargo.toml + spec.md + roadmap.md + patchnotes.md + README.md + CLAUDE.md + AppStream metainfo bumped to 0.10.3.

## v0.10.2 (2026-05-09) — Phase 17 reliability slice: malformed-file pause/resume, custom-keyword fix, concurrent-edit hardening

The v0.10.0 / v0.10.1 vault loop ran the happy path. v0.10.2 hardens the unhappy ones — malformed files, custom keywords, file removals, concurrent edits — and adds three of the four roadmap §17 test scenarios. Two real v0.10.0 bugs surface and get fixed in the process.

**Malformed-file pause/resume.** When the watcher hits a parse error on a vault file, sync pauses for that file until it parses cleanly again. The user sees one `VaultEvent::ParseFailed` toast on the pause transition, then silence until a `VaultEvent::ParseRecovered` toast confirms sync resumed. Repeated bad saves no longer re-toast on every inotify event. The watcher's `paused: HashSet<PathBuf>` (shared via `Arc<Mutex<>>` across the run loop) tracks state; `mark_paused` returns whether the path was already in the set so the toast only fires on transitions; `clear_paused` returns `true` exactly once when the file goes back to clean.

**Custom-keyword preservation fixed (two real bugs from v0.10.0).** Spec §7.3.3 rule 1 requires `WAITING` / `IN-PROGRESS` / `BLOCKED` and other non-canonical Org keywords to round-trip verbatim via `task.orig_keyword`. The importer always honoured this, but the v0.10.0 watcher had two gaps:

1. `ParsedTask::to_new_task` only handled `OrgKeyword::Cancelled` — the `Custom` variant fell through and the keyword was lost on create. Result: a fresh `WAITING` headline appearing in the vault would land in DB as a plain `TODO`.
2. `diff_from` didn't compare `orig_keyword` at all, and `TaskUpdate` had no `orig_keyword` field anyway. Result: an external flip from `WAITING` to `IN-PROGRESS` on an existing task would not sync — DB kept the old keyword forever.

Fix: new `TaskUpdate.orig_keyword: Option<Option<String>>` field + builder method; the worker's `update_task` SQL builder threads it through; `is_noop` updated. New private helper `org_keyword_to_orig` in `vault_watcher.rs` drives both create + diff paths so they stay in lockstep. Pinned by `external_custom_keyword_round_trips_through_orig_keyword`.

**File removal: toast + retain (spec §3.5).** When a user `rm`s a vault file or moves it out of the vault, Atrium now retains the project's tasks (DB canonical, vault projected) and surfaces a `VaultEvent::FileRemoved` toast. A stray `rm` no longer silently leaves stale rows; the next project flush recreates the file from DB. Per-headline deletion (a TODO removed from a file that still exists) is unaffected — it already round-trips through `diff_and_apply`'s "in DB but not in parsed → delete" branch.

**Concurrent-edit test scenario.** New `concurrent_atrium_and_external_edit_preserves_user_content_as_bak` integration test drives the full Phase 17 race: spawn the loop, seed a project + task, fs::write external content, immediately update the same task title via the worker. Asserts the writer-side conflict detection catches the race, snapshots the user's content to `.atrium.bak.*`, the main file ends up with the DB rename, the user's content does not propagate to DB (writer beat watcher), and a `ConflictBackup` event surfaces.

**Large-file parse latency test.** New `large_file_parses_under_budget` lib test generates a 1000-headline `.org` file with realistic shape (file-level `:PROPERTIES:`, per-task SCHEDULED + DEADLINE cookies, body content) and asserts the parse stays under 500 ms wall (debug-mode budget; real machines see low tens of ms). The number to watch: if this ever reports >100 ms in debug, it's a hint to look at parser allocation patterns before users with big vaults hit it.

**What's still open in the v0.10.x patch arc:**

- **v0.10.3:** `rrule_to_org_cookie` helper; writer emits both Org cookie + `:RRULE:` property; RRULE divergence detection on read-back; multi-day RRULE round-trip test; agenda-parity acceptance test (Phase 17 closer).

**Test count: 616** across the workspace (up from 611). Five new tests:

- `malformed_file_pauses_then_recovers` (vault_watcher_integration)
- `external_custom_keyword_round_trips_through_orig_keyword` (vault_watcher_integration)
- `concurrent_atrium_and_external_edit_preserves_user_content_as_bak` (vault_watcher_integration)
- `external_file_removal_preserves_tasks_and_toasts` (vault_watcher_integration)
- `large_file_parses_under_budget` (org/parse lib tests)

Schema unchanged at version 7. No new third-party dependencies.

VERSION + Cargo.toml + spec.md + roadmap.md + patchnotes.md + README.md + CLAUDE.md + AppStream metainfo bumped to 0.10.2.

## v0.10.1 (2026-05-09) — Phase 17 next slice: GUI wiring, conflict detection, sidecar; cleanup pass

The v0.10.0 first slice landed the watcher mechanics but kept the GTK binary running write-only. v0.10.1 takes the loop the rest of the way: a save in Doom Emacs against the configured vault now lands in Atrium's task list within ~250 ms, and the conflict / parse-fail surfaces show up as toasts. Plus a cleanup pass — one bug fix, one round of comment surgery, and the four-year-old `#![allow(dead_code)]` scaffolding around `AtriumError` finally earns its keep.

**GUI wiring + VaultEvent channel.** New `atrium_org::spawn_vault_loop(root, pool)` replaces the broken `spawn_org_vault_with_watcher` (which took a `WorkerHandle` that didn't exist at the natural call point — chicken-and-egg). The new shape returns `(VaultConfig, VaultLoopHandle, events_rx)`: pass the `VaultConfig` into `spawn_worker_with_vault` so the worker boots with the writer half installed, then feed the resulting `WorkerHandle` into `VaultLoopHandle::attach_watcher` to finish the wiring. The events receiver carries `VaultEvent` notices the GUI bridges to `AtriumWindow::show_toast`. `boot_data_layer` switched to the new builder; the GTK binary boots with both halves of the loop wired and the toast bridge active when a `vault-path` GSetting is configured.

**Conflict detection (spec §7.3.3 rule 5).** The writer now stats the destination file before each atomic-overwrite. If the file's mtime isn't in `RecentWrites` — meaning Doom Emacs / vim-orgmode / any external editor touched it since Atrium's last self-write — the current contents snapshot to `<file>.atrium.bak.<UTC-timestamp>` first. The format is filesystem-safe (no colons), UTC, and sortable so multiple backups for the same file order chronologically when listed. Spec rule 5 — last-writer-wins by mtime, the loser is preserved — is now mechanically enforced; without this guard the sequence "Atrium GUI mutates DB at T1, user saves in Emacs at T1+50, writer flushes at T1+110" silently destroyed the user's external edit. A `VaultEvent::ConflictBackup` event surfaces the source / backup pair; the GTK binary toasts it.

**Sidecar config (Phase 16 carryover).** New `atrium-org/src/sidecar.rs` ships `<vault>/.atrium/config.toml` with tag colours round-tripped to disk. Hand-rolled minimal TOML (no `toml` crate dependency — same ethos as the hand-rolled Org parser; the schema is small enough that a focused emitter / parser beats fighting a full-toml AST). The vault writer refreshes the sidecar at the end of every flush burst that touches tag state and skips the IO when content is unchanged via a `last_sidecar` cache. Mode and saved-perspective slots are reserved (the file always emits the section headers so Emacs-side tools see the shape) but not yet written — mode lives in GSettings (only the GTK binary knows it), and perspectives need a paired importer.

**Worker domain invariants.** `DomainError` was a four-year-old placeholder with one unconstructed `Invariant(String)` variant. v0.10.1 gives it real, enforced rules:

- `ParentProjectMismatch` — the schema's FK ensures a subtask's `parent_id` exists, but can't express "lives in the same project as the subtask itself." The worker checks before insert in `create_task` and catches the move-orphans-parent case in `update_task`. Subtask hierarchies must stay within a project.
- `EmptyFilterExpr` — perspectives with a blank filter have no rows; rejected in `create_perspective` + `update_perspective` so the GUI editor surfaces the failure rather than producing a no-op sidebar entry.

`DbError` gained `#[from] DomainError` so domain rejections flow through the existing `Result<_, DbError>` API. The `UiError` + `AtriumError` types in the GTK binary lost their `#![allow(dead_code)]` lid; `UiError::VaultPathInvalid` is now constructed when the user's `vault-path` GSetting points at an uncreatable directory, and `boot_data_layer` returns `Result<BootedDataLayer, AtriumError>` instead of `anyhow::Result`.

**Bug fix — `flatten_one` recursion.** The v0.10.0 vault watcher silently dropped TODOs nested under non-keyword headings:

```text
* Backlog
** TODO Real task
```

`Real task` would never land in the DB on external sync — the watcher's `flatten_one` bailed on the first non-keyword headline and returned without visiting children. The importer (`org/import.rs::import_task`) always handled this correctly per spec §7.3.1 ("project sub-headings are organisational, not structural"); the watcher now matches. Pinned by a new `external_add_under_subheading_creates_db_task` integration test.

**Comment audit.** Six doc-comment sites carrying band-aid framing from earlier patch arcs (`atrium-core/src/db/command.rs`, `atrium-org/src/org/{mod,parse,import,write}.rs`, `atrium-org/src/vault_watcher.rs`) were rewritten. The rule: state the current behaviour, name any genuine constraint, point at the open roadmap item by section. No more "lands in v0.7.X" / "for now" / "follows in" voice.

**What's still deferred** in the v0.10.x patch arc per the Phase 17 roadmap entry:

- **v0.10.2:** malformed-file pause/resume — repeated parse failures on the same file pause sync for that file (current behaviour: warn + drop event, retry on next event).
- **v0.10.3:** RRULE divergence detection on read-back; agenda-parity acceptance test gating the v0.10.x → v0.11.0 close.

**Test count: 611** across the workspace (up from 590), all green: 5 worker-domain tests, 1 watcher integration regression, 3 conflict-detection unit/integration tests, 8 sidecar lib tests + 1 integration test, plus the new `spawn_vault_loop` end-to-end. Schema unchanged at version 7. No new third-party dependencies.

VERSION + Cargo.toml + spec.md + roadmap.md + patchnotes.md + README.md + CLAUDE.md + AppStream metainfo bumped to 0.10.1.

## v0.10.0 (2026-05-09) — Phase 17 first slice: vault → DB sync

The DB → vault direction has been live since v0.7.16 / Phase 16. v0.10.0 closes the loop: edits made in Emacs / Doom / vim-orgmode against the configured vault flow back into the SQLite store within ~250 ms.

**The watcher.** New `atrium-org/src/vault_watcher.rs` hosts a tokio task that pairs with the existing `VaultWriter`. It uses the `notify` crate (sign-off granted; v8.x; the canonical Rust file-watcher used by watchexec / cargo-watch) to subscribe to `.org` create / modify / delete events under the vault root. Events debounce 200 ms keyed on file path (last-deadline-wins, matching the writer's pattern); after debounce the watcher parses the file through the existing `parse_org_file_with_meta`, computes a diff against current DB state, and submits writes through `WorkerHandle`.

**The self-write filter.** Without coordination, every write the writer emits would echo back through inotify and trigger a redundant read/diff cycle. New `atrium-org/src/self_write.rs` exposes `RecentWrites`, an `Arc<RwLock<>>`-shared set the writer pushes to and the watcher consults. The match is **mtime-based exact tuple equality** on `(path, mtime_just_written)`, not a TTL window on path alone. The first design used path+TTL and the integration tests immediately surfaced the failure mode: an external edit happening within the TTL window after Atrium's own write got swallowed because the writer's record was still "recent" when the watcher's debounce fired. mtime-based matching is exact — Linux ext4 stores nanosecond mtimes so two distinct writes never collide; Atrium-from-Atrium echoes match exactly; real external edits produce a different mtime and fall through. The TTL stays as a memory bound (2 seconds) but doesn't gate the match.

**The diff.** `vault_watcher::diff_and_apply` resolves the project by file-level `:ID:` (creating one if the file is new), snapshots current DB tasks for that project, and walks the parsed headline tree:

- Tasks present in parsed but missing in DB → `WorkerHandle::create_task`. Headlines parsed without `:ID:` get a freshly-minted UUIDv4; the worker's auto `notify_project_dirty` after the create triggers the writer to rewrite the file with the now-stable property, and the self-write filter swallows the resulting inotify event.
- Tasks present in DB but missing in parsed → `WorkerHandle::delete_task`.
- Tasks present in both → `WorkerHandle::update_task` for any field difference (title, schedule, deadline, completed_at) plus `WorkerHandle::set_task_tags` for tag-set differences.

**`TaskUpdate.completed_at`.** Atrium previously had only `toggle_complete` (which stamps `now()`) for state transitions. The vault watcher needs to round-trip `CLOSED: [2026-04-01 Wed 09:00]` cookies verbatim — the source timestamp must land in the DB. New `TaskUpdate.completed_at: Option<Option<DateTime<Utc>>>` field + builder method; the worker SQL builder gained the matching branch. `Some(None)` clears (re-opens), `Some(Some(when))` sets. Schema unchanged; no migration.

**The wiring.** New ergonomic builder `atrium_org::spawn_org_vault_with_watcher(root, pool, worker_handle)` spawns the writer + the watcher sharing one `RecentWrites` set, returning the `VaultConfig` ready to thread into `spawn_worker_with_vault`. The legacy `spawn_org_vault` (write-only — the v0.8.0 / v0.9.0 shape) stays available for callers that want write-only behaviour or just the writer half (tests).

**Three integration tests** at `atrium-org/tests/vault_watcher_integration.rs` pin the working slice end-to-end:

- `external_add_creates_db_task` — append a new TODO headline to a vault file via `fs::write`; assert the DB has the new task and the rewritten file gained an `:ID:` property.
- `external_edit_completes_db_task` — flip TODO → DONE in the file; assert `task.completed_at` lands.
- `external_delete_removes_db_task` — splice a headline out of the file; assert the matching DB row is gone.

**What's deferred to the v0.10.x patch arc** per the Phase 17 roadmap entry:

- v0.10.1: conflict detection (mtime race → loser preserved at `<file>.atrium.bak.<timestamp>`); GUI wiring (`spawn_vault_watcher` from the GTK boot path).
- v0.10.2: malformed-file pause/resume (parse error → pause that file, toast surfaced; auto-resume when it parses again).
- v0.10.3: RRULE divergence detection on read-back (per the canonicalisation contract spec §3.5 + roadmap Phase 17).
- v0.10.4: agenda-parity acceptance test gating the v0.10.x → v0.11.0 close.

**Test count: 590** (up 8 — three integration tests + four `RecentWrites` unit tests + one watcher diff test bundled into the integration suite). Schema unchanged at version 7. New direct dependency: `notify` v8 in `atrium-org` (sign-off granted in this patch). Ship-gate runs in under 2 seconds.

VERSION + Cargo.toml + spec + roadmap + patchnotes + README + CLAUDE.md + AppStream metainfo bumped to 0.10.0.

## v0.9.0 (2026-05-09) — `atrium-org` crate extraction

The Phase 16 Org projection — parser, emitter, importer, vault writer task — moves out of `atrium-core::sync` into its own workspace crate, `atrium-org`. atrium-core stays Org-agnostic; the worker hooks into the projection through a new `VaultDirtyNotifier` trait. Workspace is now five crates (atrium-core, atrium-search, atrium-org, atrium-cli, atrium). Pre-Phase-17 housekeeping; no behaviour change, no schema change, test count unchanged at 582.

**The split.** What moved into `atrium-org`:

- `atrium-core/src/sync/org/{parse,emit,import,write}.rs` → `atrium-org/src/org/*`. Same public API; the only path change for callers is `atrium_core::sync::org::*` → `atrium_org::org::*`.
- `atrium-core/src/sync/vault_writer.rs` → `atrium-org/src/vault_writer.rs`. Now uses an `OrgVaultNotifier` wrapper that impls `atrium_core::VaultDirtyNotifier`.
- `atrium-core/tests/org_roundtrip.rs` (+ the five fixture `.org` files) → `atrium-org/tests/`. The Org-related worker_tests entries (`import_org_file_*` / `import_org_directory_*` / `spawn_with_vault_writes_org_file_on_task_create`) moved to a new integration test `atrium-org/tests/worker_org_integration.rs`.

What stayed in `atrium-core`:

- `atrium-core/src/sync/atomic.rs` (write-temp + fsync + rename helper — generic, not Org-specific).
- `atrium-core/src/sync/json.rs` (lossless DB snapshot — works on any projection).

**The trait abstraction.** New `atrium-core/src/db/vault_hook.rs` exposes:

```rust
pub trait VaultDirtyNotifier: Send + Sync {
    fn notify_project_dirty(&self, project_id: i64);
}

pub struct VaultConfig {
    pub notifier: Arc<dyn VaultDirtyNotifier>,
}
```

The atrium-core worker holds an `Option<Arc<dyn VaultDirtyNotifier>>` instead of a concrete `mpsc::Sender<VaultWriteRequest>`. atrium-org's `OrgVaultNotifier` wraps the sender and provides the impl. Ergonomic helper `atrium_org::spawn_org_vault(root, pool)` returns a ready-to-use `VaultConfig` so the GUI / CLI boot paths stay one-call.

**Schema rule cleanup.** `atrium-core::db::migrations` was `pub(crate)`; promoted to `pub` so atrium-org's integration tests can reach in for fresh-DB setup without depending on `atrium_core::db::open` for every test fixture. Production code never calls migrations directly; `db::open` remains the public entry point.

**Why now?** Phase 17 (vault → DB `inotify` sync) is the next chunk of code, and it'll grow the projection layer further. Splitting the surface before that work starts keeps atrium-core's ~5K-line data layer focused on the worker / read pool / domain model, and gives atrium-org a clean home for the inotify watcher when it arrives.

The Phase 18 Todoist importer (when it lands) will follow the same shape: another sibling crate, depending on atrium-core, with its own write side. The architectural commitment that every non-GUI surface stays CLI-testable still holds — atrium-cli depends on atrium-org directly for the `import org` / `export org` / `export json` paths.

Workspace version bumped to **0.9.0** across `Cargo.toml`, `VERSION`, spec, roadmap, README, CLAUDE.md, AppStream metainfo. Schema version unchanged at 7. No new dependencies; atrium-org borrows from the same locked workspace set.

## v0.8.0 (2026-05-09) — Phase 16 stamp + maintenance pass

Phase 16 (Org-mode import + DB → vault writer) ships, capping the eleven-patch v0.7.6 → v0.7.18 build-out. The GTK binary, `atrium-cli`, and the hand-rolled `atrium-core::sync::org` parser/emitter let a user keep a vault at the configured path, edit tasks in Atrium, and have the `.org` files reflect the change inside ~150 ms — readable in stock `org-agenda`, Doom, or any other Org-aware tool. All Phase 16 roadmap bullets are now `[x]` except the deferred `<vault>/.atrium/config.toml` sidecar (Phase 17 follow-up).

The maintenance pass that release discipline requires of every major:

- **Worker test split.** `atrium-core/src/db/worker.rs` (2622 lines, half tests) split into `worker.rs` (1469 lines, source only) and `worker_tests.rs` (1161 lines) loaded via `#[cfg(test)] #[path = "worker_tests.rs"] mod tests;`. Same coverage; tractable file size.
- **Dead-code prune in the Org writer.** `build_org_tree` carried a `HashMap<i64, usize>` populated then discarded with `let _ = by_index;` — scaffolding from the v0.7.10 iteration. Removed.
- **Comment audit.** Bulk pass across `atrium-core/src/sync` and `atrium-core/src/db` reduced per-patch `// v0.7.X — …` markers from 74 → 26. The survivors flag load-bearing context (additive migrations, spec rules, schema columns); the rest were navigation noise.

Four-doc sweep landed on `spec.md`, `roadmap.md`, `patchnotes.md`, README, CLAUDE.md, and the AppStream metainfo. Schema unchanged at version 7; no new dependencies; 582 tests, all green.

Phase 17 (vault → DB `inotify` sync) is next.

## v0.7.18 (2026-05-09) — GUI vault integration

The GTK binary now reads the `vault-path` GSettings key on boot and routes through `spawn_worker_with_vault` when the key is non-empty, closing the loop opened by v0.7.16's auto-debounced worker write hook. Until v0.7.18, no GUI caller was passing a `VaultConfig` — every DB write needed `atrium-cli` to flush.

`boot_data_layer` builds the `ReadPool` first (the `VaultConfig` needs it), reads `vault-path` via `gio::Settings::new(APP_ID)`, and either passes `Some(VaultConfig)` (auto-creating the directory if missing) or `None` (DB-only mode, current behaviour). Misconfigured paths log a `tracing::warn!` and fall through to `None` so the app never refuses to start over a vault config error.

`atrium-core` re-exports `VaultConfig` + `spawn_with_vault as spawn_worker_with_vault` from the crate root so callers don't dive into the worker module path.

A graphical *Settings → Org Vault → Choose folder* UI to manage the key is deferred to Phase 19.5's `AdwPreferencesWindow`. Until then: `gsettings set io.github.virinvictus.atrium vault-path /path/to/vault`.

Pure additive change: no schema, no dependency changes, 582 tests still green.

## v0.7.17 (2026-05-09) — Round-trip test fixture + two importer fixes

The Phase 16 roadmap requirement: "import → export → diff = empty (modulo whitespace and section ordering)." `atrium-core/tests/org_roundtrip.rs` delivers it across five fixtures at `atrium-core/tests/fixtures/org/`:

- `kitchen_sink.org` — every spec §7.3 feature mixed (TODO/DONE/CANCELLED keywords, SCHEDULED/DEADLINE/CLOSED with repeaters, headline tags, `:PROPERTIES:` drawer, body with bullets, nested subtasks, file-level metadata).
- `custom_keywords.org` — WAITING / BLOCKED / IN-PROGRESS preservation via `orig_keyword`.
- `deep_nesting.org` — 4+ levels of subtask hierarchy.
- `project_metadata.org` — file-level `#+TITLE:` + `:PROPERTIES:` block with `:SEQUENTIAL:` / `:REVIEW_INTERVAL:` / `:LAST_REVIEWED:`.
- `unicode.org` — CJK, Cyrillic, emoji, accented Latin.

Each test imports the fixture through the worker, exports back to a fresh path, parses both source and regenerated, and asserts AST equality on a paired-normalised shape. Normalisation strips fields that intentionally don't preserve (`:CREATED:` / `:MODIFIED:` — schema-auto-stamped; round-trip-added `:ID:` per §7.3.3 rule 2; tag order — sets, not lists). Strict on title, keyword (incl. custom), tags content, cookie dates, property values, body, subtask hierarchy, and file-level metadata.

The fixture surfaced two real importer gaps:

1. **`NewTask.completed_at: Option<DateTime<Utc>>`** — previously the DONE/CANCELLED path called `toggle_complete` after create, stamping `now()` instead of the source CLOSED cookie's timestamp. The importer now threads `org.closed` directly into `NewTask.completed_at`. Toggle still fires when the source had a TODO/DONE/CANCELLED keyword but no CLOSED cookie. All `NewTask` call sites updated (atrium-cli `run_add`, the worker's repeating-task respawn, the GUI undo restore — undo now preserves the original completion timestamp too).

2. **CANCELLED via `orig_keyword`** — Atrium's domain has TODO/DONE only; `completed_at` doesn't distinguish "completed normally" from "cancelled." v0.7.12's `orig_keyword` for non-canonical keywords (WAITING etc.) now also stashes CANCELLED. The writer's orig-keyword-first lookup picks it up automatically and round-trip preserves the keyword exactly.

## v0.7.16 (2026-05-09) — Auto-debounced worker write hook (DB → vault)

Every Task / Project write through the SQLite worker now triggers a background rewrite of the affected project's `.org` file in the configured vault. Atrium and Emacs can run side-by-side against the same vault and stay in sync (DB → vault direction; vault → DB is Phase 17's `inotify` watcher).

**`atrium-core::sync::vault_writer`** — new module hosting the
background writer:

- `VaultWriteRequest::ProjectDirty(i64)` is the request type.
- `VaultWriter` owns the vault root + a `ReadPool` + a
  `pending: HashMap<i64, Instant>` keyed by project_id where
  the value is the deadline after which the project should be
  flushed.
- `run()` is a `tokio::select!` loop: receive requests +
  tick on a 50ms interval. Receiving extends a project's
  deadline by 100ms (last-deadline-wins coalescing); the tick
  flushes any project past its deadline.
- `spawn_vault_writer(root, pool)` spins up the task and
  returns the request sender.

**Latency:** ~150 ms (debounce 100ms + tick 50ms) from a DB
write landing to the corresponding `.org` file appearing.
Below human-perceptible threshold.

**Worker integration.** New `spawn_with_vault(conn, vault:
Option<VaultConfig>)` entry point alongside the existing
`spawn`. `VaultConfig { root: PathBuf, read_pool: ReadPool }`.
The worker stashes a `vault_tx: Option<mpsc::Sender<VaultWriteRequest>>`
internally; a `notify_project_dirty(project_id)` helper
non-blockingly `try_send`s through it (full channel → drop,
not block — under absurd load the worst case is one stale
vault file). `spawn(conn)` becomes a thin wrapper that
delegates with `vault: None`, so atrium-cli and tests stay
unchanged.

**Dispatch sites.** Every Worker command that mutates a
project's task set or project metadata now calls
`notify_project_dirty`:

- `CreateTask` / `UpdateTask` / `ToggleComplete` —
  `task.project_id`
- `DeleteTask` — captures the project_id BEFORE deleting
  (since the row goes away)
- `CreateProject` / `UpdateProject` / `ArchiveProject` /
  `MarkReviewed` — the project's id
- `MarkTaskReviewed` — `task.project_id`
- `SetTaskTags` — `task.project_id`

**Architecture choices documented in the module doc:** why a
separate task (single-writer SQLite discipline; vault writes
shouldn't block command processing on large projects); why
debounce inside the writer (keeps worker dispatch sites
trivial); why mpsc instead of broadcast (single consumer +
overflow tolerable).

**Tests:**

- `vault_writer_emits_project_file_on_dirty_request` — the
  isolated writer task: send a request, wait, verify file
  appears.
- `vault_writer_debounces_burst_into_one_write` — 5 rapid
  requests over 50ms collapse into one final write.
- `spawn_with_vault_writes_org_file_on_task_create` — the
  end-to-end story: spawn the worker with a vault, create a
  task, the file lands automatically.

**What's NOT in v0.7.16** (deferred to v0.8.0's maintenance
pass): GUI integration with the GSettings `vault-path` key
(the worker accepts a vault config but no caller passes one
yet — atrium-cli stays unchanged, the GTK binary still uses
the no-vault `spawn`); rollback to `.atrium.bak.<timestamp>`
on integrity failure (v0.7.15's Err return is the
detection layer; the recovery layer needs the v0.7.16 hook
to make decisions on, which it now has).

## v0.7.15 (2026-05-09) — Post-write Org integrity check

With the importer + writer +
multi-file walk in place, every vault write now goes through a
post-write parse-back assertion: the file we just wrote must
re-read cleanly through Atrium's own parser. If it doesn't, the
emit returns an `io::Error::Other` describing the divergence,
and a `tracing::warn` lands in the log so the failure is visible
even when the caller swallows the error.

**`emit_org_file_with_meta` now calls `verify_emitted_file`
after the atomic rename.** The verification path:

1. Re-read the just-written file from disk.
2. Parse it via `parse_org_file_with_meta`.
3. On success → `Ok(())`. On any read or parse error →
   `Err(io::Error::Other)` with the underlying error
   wrapped + a `tracing::warn` event.

The hand-rolled parser is intentionally permissive — anything it
doesn't recognise lands in body or unknown_lines — so "rejects"
in practice means an `io::Error` from the read itself (e.g. the
file mysteriously vanished mid-write, or the user hit a
permission flip on the parent directory). It's the minimum bar
the spec calls for: "newly-written file parses cleanly with
Atrium's own reader."

**Rollback to `.atrium.bak.<timestamp>`** is the second half of
the spec rule (§7.3.3 rule 5: "Conflicts are surfaced, not
silenced"). It defers to v0.7.16+ alongside the auto-debounced
worker write hook, since both paths need to know how to recover
gracefully — preserving the previous file content before the
atomic rename + writing it back to a `.bak` on integrity
failure is a meaningful infrastructure piece on its own.
v0.7.15's Err return lets callers (the v0.7.16 worker hook) make
that decision.

## v0.7.14 (2026-05-09) — Multi-file vault walk + ensure_area

With v0.7.6 → v0.7.13 in
place, Atrium can round-trip a single `.org` file through the
DB. v0.7.14 lifts the importer to the vault-as-directory level
so users can pull an entire `~/Tasks/` into Atrium with one
command.

**`WorkerHandle::ensure_area` (mirror of ensure_tag).**
`Command::EnsureArea { name, responder }` + an idempotent
inner helper. Probes the area table for a row whose title
matches `name` case-insensitively (the `area.title` column
isn't NOCASE-collated, so the match runs at the query level
via `LOWER(title) = LOWER(?1)`); returns the existing row when
found, creates a new one otherwise. Used by the multi-file
importer to map vault subdirectories onto Atrium areas
without duplicating existing rows on re-import. Test covers
case-insensitive dedup + creation of distinct names.

**`import_org_file_with_area`.** v0.7.9's `import_org_file`
becomes a thin wrapper that delegates with `area_id = None`;
the new `_with_area` form accepts an `Option<i64>` so the
directory walker can file projects under their resolved area.

**`import_org_directory(handle, vault_root, dry_run) ->
Vec<ImportSummary>`.** Walks the vault root:

- Files at `<vault_root>/<project>.org` → unfiled Project.
- Files at `<vault_root>/<area>/<project>.org` → Project
  filed under Area `<area>` (created via ensure_area when
  absent).
- Skips dot-prefixed entries (`.atrium/`, `.git/`, hidden
  temp files) for safety.
- Skips non-`.org` files silently.
- Sub-directories nested deeper than one level get a
  warning and skip — spec §7.3.1 has exactly one level of
  areas.

Returns one `ImportSummary` per imported file plus a synthetic
trailing summary for stragglers when only-skipped warnings
need a home.

**atrium-cli routing.** `run_import` stats the path and routes
file → `import_org_file`, directory →
`import_org_directory`. New `print_import_directory_summary`
aggregates counts across files for the human banner +
expands per-project detail underneath. `--json` output for
scripts.

**End-to-end smoke** verified manually: a 3-file vault
(`Inbox.org` at root, `Personal/Errands.org`, `Work/Q3.org`)
imports into 3 projects, 2 areas (auto-created), 2 tags
(auto-created via ensure_tag); `atrium-cli list projects`
renders the hierarchy as `Personal › Errands` and `Work › Q3`.

## v0.7.13 (2026-05-09) — File-level Org metadata round-trip

v0.7.12 closed the per-task
half of the round-trip discipline; v0.7.13 closes the
per-project half. With both in place, an .org file's preamble
+ headlines + drawer entries all survive a vault → Atrium →
vault round-trip cleanly.

**Parser additions.** `parse.rs` gains an additive
`parse_org_text_with_meta` / `parse_org_file_with_meta` pair
that returns an `OrgFile { directives, file_properties,
headlines }` instead of just a `Vec<OrgTask>`. The legacy
`parse_org_text` / `parse_org_file` keep their shape (call the
with-meta path and discard the preamble) so existing callers
don't break. Directives keys are upper-cased on parse for
case-insensitive lookups (`#+title:` and `#+TITLE:` both
produce the key `"TITLE"`). The :PROPERTIES: state machine
now distinguishes file-level (no current headline) from
headline-attached drawers; the former lands in
`file_properties`, the latter stays on the OrgTask.

**Emitter additions.** `emit.rs` gains
`emit_org_text_with_meta` / `emit_org_file_with_meta` that
takes the OrgFile shape. Directives sorted before emit so
`HashMap` iteration order can't perturb round-trips. A blank
line separates preamble from the first headline only when both
exist.

**Importer threading.** `import_org_file` reads `#+TITLE:`
(falls back to the file stem) and the file-level :PROPERTIES:
drawer for `:ID:` / `:SEQUENTIAL:` / `:REVIEW_INTERVAL:` /
`:LAST_REVIEWED:` / `:ARCHIVED:`. NewProject grows additive
`last_reviewed_at` and `archived_at` fields (Option<DateTime>)
to receive the imported values. The worker's `create_project`
SQL extends to include the two columns.

**Writer threading.** `write_project_to_vault` now builds an
OrgFile with `#+TITLE:` directive + a file-level :PROPERTIES:
block carrying every project metadata field that's set,
emitted via `emit_org_file_with_meta`. Project-level fields
that are NULL / default don't emit, keeping clean projects'
preambles minimal.

**Round-trip test** (`project_metadata_round_trips_through_db`)
imports a vault file with full project metadata, verifies the
DB row carries the expected values, exports back, and asserts
the regenerated file's preamble matches the source's project-
level fields. With this in place, projects round-trip cleanly
without losing project-scope flags.

## v0.7.12 (2026-05-09) — Custom-keyword Org round-trip (migration 0007)

Closes the loop on spec
§7.3.3 rule 1 — "Custom keywords map to a sentinel state on
import; the original is stashed in :ORIG_KEYWORD: and restored
on export" — at the data-model level rather than as a generic
property string in the .org file.

**Migration `0007_task_orig_keyword.sql`** adds a `task.orig_keyword`
TEXT NULL column. user_version 6 → 7. Existing tasks default
NULL = "no custom keyword recorded." v0.7.11 binaries reading a
v0.7.12 DB ignore the column.

**Domain Task + NewTask gain `orig_keyword: Option<String>`.**
Threaded through the read mapper, the worker INSERT, and every
NewTask / Task literal site (test_support, worker.rs's repeating-
task respawn, atrium-cli's run_add, atrium/src/ui/window.rs's
undo restore). Repeating-task respawn carries the value forward
so a `WAITING` task that completes still respawns as `WAITING`.

**Importer maps `OrgKeyword::Custom(name)` → `orig_keyword =
Some(name)` + canonical TODO sentinel.** No more lossy note;
the original is preserved in the DB.

**Writer's `task_to_org` checks `orig_keyword` first** when
choosing the headline keyword. Falls back to canonical TODO /
DONE based on `completed_at` when the column is NULL. Atrium's
UI never surfaces the column — completion semantics still flow
through `completed_at` alone.

**Why a column instead of `:ORIG_KEYWORD:`?** Atrium's task
model already has typed columns for everything else (tags,
defer, repeat, etc.); a generic property bag would be
out-of-character. The column is purely a round-trip anchor; if
a user removes the source vault file, the original keyword
still survives in the DB. The downside — a non-vault user
sees `WAITING` tasks rendered as TODO in Atrium's UI — is
intentional: Atrium's three canonical states are the surface
contract; the orig_keyword is upstream interop.

End-to-end test (`custom_keyword_round_trips_through_db`)
imports a file with `WAITING`, `IN-PROGRESS`, and `TODO`
headlines; exports the resulting DB; the regenerated file's
keyword sequence matches the source. Without this column the
test would fail with three `TODO` headlines.

## v0.7.11 (2026-05-09) — JSON snapshot export

The Org vault projection (v0.7.6
→ v0.7.10) is interoperable with Emacs / vim-orgmode but lossy on
constructs Atrium doesn't fully model (custom keywords fold to
TODO; project sub-headings drop through the writer; etc.). The
roadmap explicitly calls for a complementary lossless format:
"Atrium native JSON export ships in this phase too — universal
lossless backup format." v0.7.11 delivers it.

**`atrium-core::sync::json`.** New module. Top-level
[`Snapshot`] struct holds a `Vec<T>` per domain table:
`areas` / `projects` / `headings` / `tasks` / `tags` /
`task_tags` (as `(task_id, tag_id)` pairs) / `perspectives`.
Plus metadata: `version` ("1" for the v0.7.11 schema),
`exported_at` UTC timestamp, `atrium_version` (CARGO_PKG_VERSION).
Every domain type already derives Serialize / Deserialize so
the serializer is mostly composition.

`build_snapshot(conn)` reads every relevant table; uses
`list_all_projects` (a new additive read primitive that
includes archived projects, unlike the active-only
`list_projects`) so the backup is complete. New read
primitives `list_headings` and `list_task_tags` cover the
remaining tables.

`export_db_to_json_text(conn)` returns pretty-printed JSON.
`export_db_to_json_file(conn, path)` goes through the v0.7.6
`write_atomic` helper so a crash mid-write leaves any
previous backup intact.

**`atrium-cli export json PATH [--dry-run]`.** New export
target. Mirrors the `export org` shape: dry-run reports the
snapshot dimensions (counts per table) without writing; real
mode writes a single `.json` file at PATH. Output: human
(default) or `--json` (machine-readable summary).

**Re-import is deferred** — the use case is restore-from-
backup, not a hot path. A snapshot → DB importer can land
when there's a concrete need (cross-version migration, etc.).

**`DbError::Sync(String)` variant added** for serialization-
layer failures. Currently only the JSON exporter touches it
(serde_json failures, vanishingly rare).

## v0.7.10 (2026-05-09) — Vault writer + atrium-cli export org

v0.7.9 gave us the importer
(Org → DB); v0.7.10 lands the writer (DB → Org) so users can
round-trip in both directions. With this patch, an Atrium DB
can be projected to a vault directory, edited with Emacs / vim-
orgmode / any Org tool, and re-imported — the round-trip
discipline holds for every spec §7.3 construct already covered
by the importer.

**`atrium-core::sync::org::write::write_project_to_vault`.**
Reads a project + every task in it (open + done) + tag names
through a read-only `Connection`, builds an `OrgTask` tree
mirroring spec §7.3.2's field mapping in reverse:

- Task title → headline text
- Task note → body verbatim
- Task tags → headline `:tag1:tag2:`
- completed_at present → DONE keyword + CLOSED cookie
- completed_at None → TODO keyword
- scheduled_for / deadline → SCHEDULED / DEADLINE cookies
- task.uuid → `:ID:` property
- repeat_rule → `:RRULE:` property
- estimated_minutes → `:EFFORT:` `H:MM`
- defer_until → `:DEFER_UNTIL:` `YYYY-MM-DD`
- parent_id chain → nested headlines (depth = parent.depth + 1)

The destination path is `<vault_root>/<area_title>/<project_title>.org`
(or `<vault_root>/<project_title>.org` for unfiled projects).
Filename sanitization replaces filesystem-hostile chars with
`_` and collapses runs; empty / all-bad titles default to
"untitled". Emit goes through the v0.7.8 `emit_org_file` →
v0.7.6 `write_atomic` so a crash mid-write leaves the previous
file intact (spec §7.3.3 rule 6).

**`write_all_projects_to_vault`** walks `list_projects` and
calls `write_project_to_vault` for each. Used by the new CLI.

**New read primitive `list_all_in_project`.** The existing
`list_project` filters `completed_at IS NULL`; for the writer we
need open + done so the projected file reflects the full
project state. Additive — doesn't change the existing read API.

**`atrium-cli export org PATH [--dry-run]`.** New subcommand
parsed via `args::parse_export`. Walks every project in the DB
and writes one `.org` file per project under PATH. Dry-run mode
walks the project list and prints what *would* be written
without touching disk. Output: human (default) or `--json`
(machine-readable summary with per-project counts + paths).

**Limitations consciously deferred to v0.7.11+:** Project
sub-headings (the `heading` table) aren't emitted yet — they
round-trip as the importer's `headings_skipped` count grows on
each cycle. Custom keywords (`WAITING`, etc.) round-trip back
to TODO; the `:ORIG_KEYWORD:` machinery follows. File-level
project metadata (`#+TITLE:`, `:SEQUENTIAL:`,
`:REVIEW_INTERVAL:`, `:LAST_REVIEWED:`, `:ARCHIVED:`) not yet
emitted. Auto-debounced worker write hook (Atrium → vault on
TaskChanges) lands as a separate patch.

## v0.7.9 (2026-05-08) — Org importer (`atrium-cli import org`)

v0.7.6–v0.7.8 gave us the
foundation, parser, and emitter; v0.7.9 lands the one-shot
importer that lets users pull an existing .org file into the DB
through `atrium-cli`.

**`NewTask.uuid` / `NewProject.uuid` (additive).** Both creator
structs gain an `Option<String>` UUID field. `None` (and empty
strings) keep the historical "worker generates a fresh v4"
behaviour; `Some(s)` lets the importer preserve `:ID:` from the
source vault file (spec §7.3.3 rule 2: ":ID: is the round-trip
anchor"). All existing call sites updated. Three new worker
tests cover the round-trip + the empty-string fallback.

**`atrium-core::sync::org::import_org_file`.** Parses the file
through `parse_org_file`, derives the project title from the
file stem, and walks the headline tree creating tasks via the
worker. Field mapping per spec §7.3.2:

- Headline → Task.title
- Headline `:tags:` → Atrium tags via `ensure_tag` (idempotent),
  attached via `set_task_tags`
- Body → Task.note (verbatim)
- TODO / DONE / CANCELLED → keyword (DONE/CANCELLED toggled
  via `toggle_complete` after create)
- Custom keywords → folded to TODO with a lossy note
- SCHEDULED → `scheduled_for`, DEADLINE → `deadline`
- `:ID:` → `Task.uuid`
- `:RRULE:` → `Task.repeat_rule` (verbatim)
- `:EFFORT:` (`H:MM` or `Hh[Mm]` form) → `estimated_minutes`
- `:DEFER_UNTIL:` → `defer_until`
- Children → tasks with `parent_id` set

**Dry-run mode.** `import_org_file(handle, path, dry_run=true)`
walks the parse tree and tallies what *would* be created
without touching the DB. The atrium-cli surface is
`atrium-cli import org PATH --dry-run`.

**Limitations consciously deferred:** project sub-headings
(headlines without a TODO keyword) skipped and counted in
`headings_skipped` — heading-table writes follow in v0.7.10+.
DONE / CANCELLED tasks have `completed_at = now()`, not the
CLOSED cookie's timestamp — surfaced as a lossy note. Repeater
suffixes on SCHEDULED / DEADLINE recorded in the parsed tree
but not converted to RFC 5545 RRULE; use `:RRULE:` for canonical
round-trips. Multi-file vault walk lands in v0.7.10+. Re-imports
always create new rows; full bidirectional sync (Phase 17) adds
upsert-by-`:ID:`.

**`atrium-cli import org PATH [--dry-run]`.** New subcommand
parsed via `args::parse_import`, dispatched through the existing
worker-runtime helper. Output formats: human (default),
`--json` (machine-readable summary).

## v0.7.8 (2026-05-08) — Org-mode emitter (round-trip safe)

v0.7.6 + v0.7.7 gave us the
foundation + the parser; v0.7.8 lands the emitter that pairs
with it to satisfy spec §7.3.3's round-trip discipline. With
both halves in place, Atrium can now read an Org vault file
and write it back without losing or reordering the constructs
the spec §7.3 mapping pins down.

**`atrium-core::sync::org::emit_org_text`** takes a `&[OrgTask]`
and returns the Org text. Per-task layout:

- Headline: `*` × depth + `KEYWORD` (if any) + title + ` :tag1:tag2:` (if tags).
- Cookie line below the headline (only when at least one of
  scheduled / deadline / closed is set): SCHEDULED/DEADLINE
  rendered as active timestamps (`<YYYY-MM-DD Day [+repeater]>`)
  joined by single spaces; CLOSED rendered as inactive
  (`[YYYY-MM-DD Day HH:MM]`, with the time elided when it's the
  parser's noon-UTC default — matches Emacs's "date-only CLOSED"
  shape).
- `:PROPERTIES:` drawer (only when there are properties or
  unknown_lines): keys emitted in sorted order so `HashMap`
  iteration randomness can't perturb round-trips. Empty values
  emit as bare `:KEY:` per Org's canonical form.
- Body preserved verbatim from `OrgTask::body`; trailing newline
  added on emit (parser strips it on read).
- Children rendered recursively at depth+1 immediately after the
  parent's body.

**`atrium-core::sync::org::emit_org_file`** wraps the text emit
through the v0.7.6 `write_atomic` helper, satisfying spec
§7.3.3 rule 6. A crash mid-write leaves the previous version of
the file intact.

**Round-trip discipline.** 13 dedicated `roundtrip_*` tests
parse a representative input, emit it, re-parse, and assert the
two parsed trees are equal. Coverage spans every spec §7.3
construct: simple TODO, DONE+CLOSED, scheduled+deadline,
all three repeater modes (`+1d`, `++1w`, `.+2w`), headline
tags, properties drawer, body verbatim preservation, nested
subtasks, project sub-headings (no keyword), custom keywords
(WAITING), unknown-lines preservation inside the drawer, and a
kitchen-sink test combining everything in one document.

## v0.7.7 (2026-05-08) — Hand-rolled Org-mode parser

v0.7.6 laid the foundation
(sync module + atomic write + GSettings); v0.7.7 lands the
parser that everything from here on builds on. No third-party
dep — the v0.7.6 dep-research established that orgize and
starsector were both too dormant to take, and a focused
passthrough parser fits the use-case better anyway.

**`atrium-core::sync::org::parse_org_text` / `parse_org_file`.**
Reads Org text → `Vec<OrgTask>` tree. Coverage matches spec §7.3:

- Headlines `*+ [KEYWORD ]title [:tag1:tag2:]`. Stars give the
  depth; `KEYWORD` recognised as TODO / DONE / CANCELLED, with
  custom keywords (e.g. `WAITING`) preserved verbatim under
  `OrgKeyword::Custom`.
- Cookies on the line below a headline: SCHEDULED, DEADLINE
  (active timestamps `<...>`), and CLOSED (inactive `[...]`).
  All three can co-exist on one line.
- Repeater suffixes on SCHEDULED / DEADLINE: `+1w`, `++1w`,
  `.+1w` parsed into `OrgRepeater { mode, interval, unit }`.
- `:PROPERTIES:` drawer with `:KEY: value` entries until `:END:`.
  Keys preserve case. Garbage lines inside the drawer are
  preserved into the task's `unknown_lines` field for
  round-trip safety.
- Headline tags `:foo:bar:` validated for the canonical Org
  shape (rejects `:foo bar:` with whitespace inside).
- Nested subtasks: depth-based tree resolution. Headlines
  without a TODO keyword become project sub-headings per spec
  §7.3.1; deeper headlines nest under their nearest shallower
  ancestor.
- Headline body — everything between cookies/properties and the
  next headline — captured verbatim. Source blocks, tables,
  custom drawers, latex, links: all flow through unchanged so
  v0.7.8's emitter can re-emit them without loss.

**The "preserve unknown constructs verbatim" rule (spec §7.3.3
rule 1) is satisfied at two layers** — body content stays
verbatim; properties drawer entries that don't pattern-match
land in `OrgTask::unknown_lines` for explicit round-trip.

**Limitations consciously deferred to v0.7.8+:** multi-line
property values, active-timestamp time-of-day (date-only matches
Atrium's `scheduled_for`), file-level `#+TITLE:` capture (lands
when the importer needs the project title).

Pure additive change. No schema changes. No new dependencies.

## v0.7.6 (2026-05-08) — Phase 16 foundation (Org vault projection)

The roadmap calls for Org-mode
import + two-way vault sync, staged across v0.7.6 → v0.8.0 with
each patch shippable on its own. v0.7.6 lands the foundation
pieces that everything later builds on, plus the dep-research
decision that reverses the original plan.

**Org parser dep-research and the reversal.** CLAUDE.md listed
`orgize` as a pending dep for Phase 16. The v0.7.6 survey turned
up two practical issues: orgize's last stable release (`0.9.0`,
November 2021) is four years old; the active line has been in
alpha (`0.10.0-alpha.X`) since November 2023. The obvious
alternative — `starsector 1.0.1` — looked cleaner on paper
("structural parser/emitter with emphasis on avoiding edits
unrelated to changes") but its last release was October 2022 and
it pulls orgize-alpha as a transitive anyway. Conclusion:
hand-roll the Org subset Atrium needs, fitting the
CalibreQuarry stdlib-only ethos. The "preserve unknown
constructs verbatim" rule (spec §7.3.3 rule 1) is actually
*easier* in a focused passthrough parser: capture every
unrecognised line into the task's `unknown_lines` field and
re-emit verbatim on write. CLAUDE.md's dependency-discipline
section now records this decision so future passes don't
re-litigate it.

**Sync module skeleton.** `atrium-core/src/sync/mod.rs`
declares the module structure for the Phase 16 work coming in
v0.7.7+: `atomic` (write-temp + fsync + rename helper, lands
this patch) and `org` (hand-rolled parser + emitter, lands in
v0.7.7).

**Atomic write helper.** `atrium-core/src/sync/atomic.rs`
implements `write_atomic(path, contents) -> io::Result<()>` per
spec §7.3.3 rule 6: write to `<path>.atrium.tmp` in the same
directory, fsync, rename atomically. Best-effort cleanup of the
temp file on failure. Five tests cover the happy path,
overwrite, no-temp-file-leftover, and error cases (missing
parent dir, root path).

**Vault-path GSettings key.** New `vault-path` key in
`data/io.github.virinvictus.atrium.gschema.xml`, default empty
string (= "no vault configured"). Atrium runs DB-only when
unset, which is a valid configuration per spec §3.5. The proper
Settings → Org Vault → Choose folder UI lands in Phase 19.5;
v0.7.7+ patches will read the key directly via gio::Settings
when wiring the importer / writer / sync hook.

Test count: 119 + 174 + 1 + 106 + 106 = **506** (up 3 from
v0.7.5's 503 — the new atomic-write tests). Pure additive
change. No schema changes. No new dependencies.

## v0.7.5 (2026-05-08) — Visual refinement pass

The polish list deferred from v0.7.3 / v0.7.4 finally lands. Five
items, all aimed at reducing remaining boxiness on the rows and
panes after v0.7.0–v0.7.2 set the foundation.

**Tag pill softening.** The `.atrium-task-tags` chip retired the
visible bg-color (`alpha(@accent_bg_color, 0.15)`) in favour of
bare colored Pango spans. Each `<span foreground=HEX>#tag</span>`
still renders the per-tag colour for a row with multiple tags
the colors stack inline, reading as typography rather than as a
Bootstrap-style badge stuck onto the row. The `.completed`
override goes away (no more chip background to dim; the row's
existing opacity does the work).

**Inspector empty state.** The big AdwStatusPage with the
edit-symbolic icon and "No task selected" / "Select a row to
edit it here." was claiming the full pane during navigation;
v0.7.5 swaps it for a small centred caption ("Select a task to
edit it here.") near the top of the pane. The pane's
atmospheric tint signals the inspector's home; the caption is
just a hint.

**Sidebar filter ghost.** The "Filter lists…" search entry got
the same opacity-on-hover/focus treatment the v0.7.0 quick-add
introduced. New `.atrium-filter-ghost` class on the GtkSearchEntry,
mirroring `.atrium-quick-add` semantics — dim at rest, full on
:hover / :focus / :focus-within.

**Row separator fade.** The `.atrium-task-listview > row`
border-bottom alpha dropped 0.30 → 0.12. After the v0.7.0 row
margin bump (6 → 9 px) the separators were reading as ledger-
grid against the new whitespace; the lower alpha keeps a quiet
scan-tracking line without outshouting the spacing.

**Sidebar selection soft-fill.** The `:selected` state on
sidebar rows gained `border-radius: 8px`, an `outline: none`
override, and a 4 px horizontal margin so the rounded fill has
breathing room to bloom rather than clipping at the listbox
edge. Mirrors the v0.7.0 task-row selection treatment —
selection becomes a glow, not a flat-bottomed rectangle.

Pure CSS + small UI tweaks. No schema changes. No new
dependencies. 503 tests still green.

## v0.7.4 (2026-05-08) — Task-level Mark Reviewed (migration 0006)

The Review page's "This week" weekly walk shipped at v0.7.2 with
no way to acknowledge an item — clicking through it revealed the
gap. v0.7.4 closes it with a true Mark Reviewed action mirroring
the Phase 13 project-level pattern.

**Schema.** Migration `0006_task_last_reviewed_at.sql` adds an
additive `task.last_reviewed_at TEXT NULL` column. Mirror of
`project.last_reviewed_at` from migration 0001. Existing user
DBs migrate cleanly; user_version 5 → 6.

**Worker.** `Command::MarkTaskReviewed { id, responder }` +
`WorkerHandle::mark_task_reviewed(id)` mirror the project-side
wiring exactly. Handler runs `UPDATE task SET last_reviewed_at
= strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1`, fetches
the updated row, emits `TaskChanges{updated: vec![task]}`. Two
new tests cover the round-trip and the not-found case.

**UI.** Each row in the Review page's "This week" section now
carries a trailing flat **Mark Reviewed** button (the agenda
row treatment stays exactly the same — it's wrapped in a
horizontal Box with the button as a sibling). Clicking the
button dispatches `worker.mark_task_reviewed`; the row drops
out via the TaskChanges-driven page rebuild.
`apply_task_changes` now routes Review the same way it routes
Forecast / Logbook / Agenda / Perspective — full page rebuild
on any delta.

**Filter.** `refresh_review_page` now excludes tasks whose
`last_reviewed_at` is within the last 7 days from `today`.
After 7 days the row resurfaces if it still matches the
weekly-walk filter. A small inline note above the section
("Mark items reviewed to hide them for 7 days.") tells users
what the button does.

Test count: 119 + 171 + 1 + 106 + 106 = **503** (up 2 from
v0.7.3's 501 — the two MarkTaskReviewed worker tests).
docs/schema.md picked up migration 0006 + the new column entry.
Pure additive change. No spec semantics shifted; no new
dependencies.

## v0.7.3 (2026-05-08) — Inspector check-off + perspective editor

Two functional gaps Brandon caught after living with v0.7.2:
the inspector had no way to mark a task complete (he had to
bounce back to the row to click the checkbox), and there was
still no GUI path to add or edit a saved perspective (only the
shared rename/delete actions and the renderer-config dialog had
landed; creating a new perspective required `atrium-cli`).

**Inspector check-off.** A circular CheckButton now sits at the
leading edge of the inspector's title row, mirroring the row-
checkbox in the task list (same `.selection-mode` class). State
reflects `task.completed_at`; clicks dispatch through
`worker.toggle_complete(id)`. A `Cell<bool>` latches the
persisted state so the worker round-tripping the toggle doesn't
ping-pong with the user click. Reachable while the inspector is
open without leaving the pane.

**Perspective editor.** A new `prompt_edit_perspective` dialog
covers all four perspective fields in one place: name, filter
expression, renderer (List / Board radios), and columns
(comma-separated tag names; sensitive only when Board is
selected). Used in two flows:

- **Create.** A "+" affordance trailing the *Perspectives*
  sidebar section header opens the dialog in create mode. On
  Save, dispatches `worker.create_perspective(NewPerspective{
  name, filter_expr, renderer, renderer_config, .. })`.
- **Edit.** The right-click context menu on a perspective row
  collapsed from three items (Rename / Configure renderer /
  Delete) to two (**Edit…** / Delete). Edit opens the dialog
  pre-filled with the existing values; on Save, dispatches a
  full `worker.update_perspective(PerspectiveUpdate)` covering
  name + filter + renderer + renderer_config in one round-trip.

The previous Rename + Configure renderer flows still exist as
plumbing (the `win.rename-active` and `win.configure-renderer`
actions are unchanged), they just no longer appear in the
perspective context menu — Edit subsumes both. Other surfaces
that fire `win.rename-active` against a perspective (none
currently) would still work.

Pure code patch. No schema changes, no new dependencies. Test
count unchanged at 501. Ship-gate runs in under 2 seconds.
## v0.7.2 (2026-05-08) — Confusion-killer patch

Brandon's after-v0.7.1 review of the Review page surfaced two
problems we'd previously planned to fix in tier 3 of the v0.7
polish arc but hadn't gotten to: the canonical Review page and
the seeded "Weekly Review" perspective both lived in the sidebar
under almost the same name and showed completely different
content (Review page: "All caught up"; Weekly Review perspective:
a long list of tasks). And the upper-left corner still had the
centered "Lists" header from libadwaita's default sidebar
auto-title, which contradicted the magazine-spread treatment
v0.7.0 introduced for the right side.

**v0.7.2 fixes both:**

1. **Review = Weekly Review merge.** The canonical Review page
   now renders two sections in one surface — "Projects to
   review" (the existing Phase 13 review queue) followed by
   "This week" (the open-tasks-this-week filter that was
   formerly seeded as a saved perspective). Both sections show
   inline notes when empty; the page falls back to "All caught
   up" only when both are empty. Section 2 reuses
   `agenda::build_row` for visual consistency with the Agenda
   canonical page; clicking a row opens the Inspector for that
   task. The seeded "Weekly Review" perspective is retired (the
   `seed_initial_perspectives` helper, the
   `WEEKLY_REVIEW_NAME` constant, and the four
   `seed_weekly_review_*` tests removed; the filter constant
   survives as `REVIEW_WEEKLY_WALK_FILTER`, used by the GUI's
   refresh path). Existing user DBs keep their row (we don't
   delete data); fresh DBs and fixtures land clean.

2. **Drop the "Lists" centered title.** The sidebar's
   AdwHeaderBar now carries an empty AdwWindowTitle as its
   title-widget, suppressing the auto-rendered "Lists" label.
   The header becomes pure chrome (which is empty, since
   show-end-title-buttons=false), and the filter entry below
   acts as the sidebar's visual top. Mirrors the
   title-suppression on the content side from v0.7.0.

Pure code patch — no schema changes, no new dependencies, no
spec semantics shifted. Roadmap: this is the tier-3 functional
work from the v0.7 polish arc landing earlier than planned, at
Brandon's call. The visual refinement (tag pills, inspector
empty-state, filter ghost, row separators, sidebar selection
softening) ships next as v0.7.3.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to
**0.7.2**.

## v0.7.1 (2026-05-08) — Surface continuity (kill the colour breaking)

Brandon's first reaction to v0.7.0: the magazine-spread title
landed, but the upper-left corner now showed visible "colour
breaking" — distinct horizontal bands of tone where the headerbar,
filter entry, and listbox met. Three things were stacking
unhelpfully:

1. The v0.6.10 standalone `headerbar` accent gradient — painted a
   leading-edge accent on every headerbar in the app, including
   the inner sidebar + content headerbars.
2. The libadwaita-default headerbar background — the inner
   headerbars had their own elevated bg-color sitting on top of
   whatever surface I'd painted underneath.
3. The v0.7.0 surface gradients — applied only to the inner
   widgets (`.navigation-sidebar` listbox, `.atrium-inspector-pane`
   PreferencesPage), not to the headerbar / filter / scrolled-window
   above them. The atmospheric tint started mid-surface, leaving
   a visible band where it began.

v0.7.1 simplifies all three:

- **Drop the v0.6.10 standalone headerbar gradient.** Surface
  gradients do the atmospheric work now; the headerbar layer was
  redundant. Replaced with a scoped `.atrium-main-toolbar
  headerbar { background: transparent; box-shadow: none; }` rule
  so the surface flows continuously behind the headerbars.
- **Replace the v0.7.0 directional surface gradients with a flat
  per-pane tint.** The 160deg / -20deg gradients were painting
  banded tones across surfaces; the flat
  `background-color: alpha(@accent_color, 0.04)` paints a uniform
  warmer tone across the whole sidebar / inspector. No bands.
- **Move the tint from the inner widget to the whole pane.** Class
  `.atrium-sidebar-pane` on the sidebar's AdwToolbarView (so the
  tint covers the headerbar area, the filter entry, and the
  listbox in one continuous fill); the inner widgets are made
  transparent so the parent's tint shows through. Same for
  `.atrium-inspector-pane`.

Net effect: the upper-left corner is no longer three stacked
horizontal bands of slightly different tone. The sidebar reads as
one continuous warmer surface from the top of the window down.
Same for the inspector on the opposite side. The neutral content
area in between is the calm centre.

Sacrifice: the directional gradient (warm at the corners, fading
toward the centre) is gone. v0.7.0's "OF4-atmospheric" was
ambitious; the flat tint is more "Things-3-calm" — uniform tone
distinguishing the panes by hue rather than by gradient drama.
The visible-banding cost wasn't worth the directional warmth.

Pure CSS + window.ui patch. No code changes. All 505 tests still
green. VERSION / Cargo.toml / patchnotes / AppStream metainfo
bump to **0.7.1**.

## v0.7.0 (2026-05-08) — Visual fusion + whitespace pass

The first big polish minor of the v0.7 line. Addresses Brandon's
critical-eye review of the v0.6.21 screenshot: the app didn't feel
"living" yet — accents had hard boundaries, the three panes were
visually identical rectangles separated by 1 px verticals,
selection states read as outlines instead of glows, and Linux-app
disregard for whitespace had crept into the row rhythm and the
inspector. Two tiers:

**Tier 1 — Living surface (the fusion pass):**

- **Three-pane atmosphere.** The sidebar's existing soft-accent
  gradient bumps from 0.025 → 0.05 alpha; the inspector pane gains
  a mirrored gradient on its leading edge (`-20deg` so the warm
  corner is on the opposite side). The two side panels now flank a
  neutral content area; the eye reads three connected spaces
  instead of one rectangle bisected by hard verticals. `data/style.css`.
- **Selection state on task rows is no longer a rectangle.** The
  default libadwaita selection paints a strong accent fill plus a
  focus outline; combined with the area-stripe and the row
  separator, selected rows looked like 1 px orange bordered boxes.
  v0.7.0 ships a soft accent fill (alpha 0.14, no border, no
  outline, rounded corners) — selection becomes a glow, not a
  frame. `data/style.css`.
- **Area accent moved from a 3 px hard left stripe to a row-wide
  gradient bleed.** The stripe approach made each row read as
  "rectangle with stripe stuck on" — the eye saw the stripe as a
  separate decorative element. The gradient (alpha 0.10 fading to
  transparent at 40% width) makes the *row* read as area-tinted.
  Six per-color rules updated; the reserved 3 px left-border on
  `.atrium-task-row` retired. `data/style.css`.
- **Sidebar section headers softened.** The v0.3.0 treatment was
  uppercase + tight tracking + a top-border divider — read as a
  partition. v0.7.0 retires the all-caps and the divider for
  medium weight, mixed case, breathing room above and below. The
  headers introduce the rows that follow rather than separating
  them from above. `data/style.css`.
- **Quick-add entry as a ghost.** The "Add task…" row at the
  bottom of the list was always-visible and always-bordered. v0.7.0
  dims it to ~0.45 opacity by default; hover or focus inside the
  box brings it back to full presence with a 180 ms ease-out
  transition. `data/window.ui` + `data/style.css`.

**Tier 2 — Whitespace pass (Brandon's specific call-out):**

- **Task-row vertical rhythm.** Margin top + bottom 6 → 9 px on
  every row. Things 3 / OmniFocus leave real air between rows;
  Linux apps habitually do not. The change adds 6 px of total
  vertical breathing per row without touching density on the row
  content. `atrium/src/ui/task_list.rs`.
- **Inspector pane field clustering.** Was: Schedule + Deadline +
  Project in one group, Tags alone in its own one-row group (an
  orphan card the eye couldn't justify). Now: dates_group carries
  only the date fields, and Project + Tags collapse into a new
  Classify cluster — both fields answer the question "where does
  this task live?" so the eye groups them naturally. Five visual
  groups overall, none of them orphans. `atrium/src/ui/inspector_pane.rs`.
- **Magazine-spread page title.** "Today" (and every other view
  name) was centered in the AdwHeaderBar — read as a tabular UI
  heading, not a page title. v0.7.0 suppresses the auto-title in
  the header bar and adds a strip below carrying the view name as
  a large left-aligned heading + an optional supporting subtitle
  beneath. The subtitle ships for Today (today's date in long
  form), Upcoming ("Next 7 days"), and Forecast ("Next 30 days");
  hidden on views without a useful subhead. `data/window.ui` +
  `atrium/src/ui/window.rs` + `data/style.css`.

No schema changes. No new dependencies. All 505 tests still
green; ship-gate runs in under 2 seconds.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to
**0.7.0**.

## v0.6.21 (2026-05-08) — Documentation housekeeping pass

Pure docs patch — bringing references that hadn't been touched
in several minors back into alignment with the current state.
No code touched. Ship-gate green.

The post-v0.4.0 release arc landed a lot in a hurry: the
search-engine extraction (`atrium-search`), the headless CLI
(`atrium-cli`), FTS5 ranking, the SQL-translation evaluator,
two new migrations (`area.color` + `perspective.renderer` /
`renderer_config`), the kanban renderer, the Agenda page, the
v0.6.x screenshot-driven cleanup arc, and the v0.6.19 / v0.6.20
roadmap revision. Several reference docs lagged behind. This
patch pulls them current.

**`README.md`:**
- Version badge `0.5.0` → `0.6.20`.
- "Both modes ship at v0.5.0." paragraph rewritten to "Both
  modes shipped early." with the current release noted.

**`CLAUDE.md`:**
- Status section: collapsed the "v0.6.0 carryover" framing
  (carryover is all shipped) and replaced it with three
  consolidated paragraphs walking the v0.5.0 → v0.5.4
  search-engine arc, the v0.6.0 → v0.6.5 Slice D arc, the
  v0.6.6 → v0.6.10 perf / sidebar / soft-accent arc, and the
  v0.6.11 → v0.6.20 screenshot-cleanup + roadmap-revision arc.
- Authoritative documents: `roadmap.md` description updated
  (now four sub-phases — 12.5, 15.5, 15.75, 19.5 — not three);
  `patchnotes.md` description updated ("v0.3.0 is the most
  recent release" → "v0.6.20 is the most recent release").
- Codebase map: header `v0.4.x` → `v0.6.20`. Added the missing
  files: `atrium-search/{dates,rank,sql_translate}.rs`,
  `atrium-core/{quick_entry,render}.rs`, migrations
  `0004_area_color.sql` + `0005_perspective_renderer.sql`,
  `atrium/src/ui/{agenda,board,logbook}.rs`. Updated read.rs
  / command.rs descriptions to mention the surfaces added in
  v0.5.x and v0.6.x. Removed the lifted `quickentry/parser.rs`
  entry (parser moved to `atrium-core::quick_entry` at v0.4.5).
- Test counts: `82 + 165 + 1 = 248 tests as of v0.4.0` →
  `119 + 173 + 1 + 106 + 106 = 505 tests as of v0.6.20`.

**`docs/schema.md`:**
- Removed the "No mid-v0.1 schema changes" framing — the v0.1
  freeze ended at v0.2.0.
- Added a migration-history table covering 0001 → 0005,
  including the v0.5.0 additions (`area.color`,
  `perspective.renderer` / `renderer_config`).
- ER diagram: added `AREA.color`, `TASK.repeat_mode`, the
  full `PERSPECTIVE` entity (was missing entirely), and the
  saved-search relation.
- Per-table rationale: added `repeat_mode` to the task
  notes, added the missing `perspective` section, and added
  `color` to the area section.

**`docs/perf-baseline.md`:**
- Refreshed the v0.0.28 capture against current binaries.
  Cold start: 30–40 ms / ~34 MB (was 25–33 ms / ~32 MB).
  Fixture generation across small / medium / large scales
  remains under 39 MB peak RSS at 50K tasks; the data layer
  is still nowhere near the §8 budget. Numbers within noise
  of the original capture despite four major arcs of feature
  work intervening.
- Note added: search-engine evolution did not regress the
  data-layer budget.

**`docs/regression.md`:**
- Step table: added the 5.5 `atrium-cli` end-to-end smoke
  (added at v0.5.x, grown through v0.6.x), with notes on
  what it covers — read paths over every canonical list,
  write paths over every CRUD subcommand, the kanban smoke
  against the fixture-seeded "Fixture Board" perspective,
  and the v0.6.5 perspective write-side smoke.
- Cold-start observed numbers updated to match the refreshed
  perf baseline.

**`docs/keymap.md`:**
- Removed the "*(view lands Phase 5)*" suffixes — all six
  canonical lists shipped at v0.1.0.
- Added a note about Agenda / Forecast / Review joining the
  top-tier sidebar at v0.6.7 / v0.6.16 without dedicated
  number accels.
- Search-filter section rewritten — the flat AND-only
  grammar grew into a full expression language at v0.4.0
  / v0.5.0; pointed at `spec.md` §4.3 as the canonical
  reference and called out the `?` operator-reference
  popover.
- Builder Mode chord table reframed — Builder Mode shipped
  at v0.2.0 but the `Ctrl+Shift+F` / `Ctrl+P` / `Ctrl+D`
  chords are still aspirational slots (these features ship
  via the sidebar / Inspector today, not via accels).

**`docs/accessibility.md`:**
- Header note added: the Phase 8f findings cover the v0.1
  surface area; the Builder Mode side pane, Forecast,
  Review, Perspectives, kanban renderer, and Agenda all
  inherit the same widget primitives but owe a full re-audit
  at the next minor.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to
**0.6.21**.

## v0.6.20 (2026-05-08) — Phase 19.5 calendar item: iCal feed → Evolution Data Server

Brandon course-corrected the original "read-only iCal calendar
feed" item that landed in v0.6.19's Phase 19.5 list. The right
integration model for a GNOME-native client running on Fedora
isn't a `.ics` file feed — it's reading the system's calendar
service.

GNOME 50's default calendar app (`gnome-calendar`) doesn't store
its own calendar data; it consumes Evolution Data Server (EDS),
the GNOME-wide calendar/contacts/tasks backend. The user has
already configured their accounts (Google, Nextcloud, local,
exchange-web-services, …) in EDS via GNOME Online Accounts. An
iCal-file feed would either duplicate that work or sit awkwardly
alongside it.

Updated framing: Atrium reads EDS via D-Bus and overlays calendar
events onto the Forecast / Today views as read-only context.
Endeavour does the same shape for *tasks* — Atrium does it for
*calendars* without becoming a calendar client. Dependency check
deferred: either `libecal` / `libedataserver` bindings or a
hand-rolled `zbus` D-Bus client. No `.ics` file plumbing.

Files touched:
- `roadmap.md` — Phase 19.5 third item rewritten.
- `spec.md` — no change needed (it didn't reference the iCal
  framing; the calendar overlay isn't in the import / export
  table because it's not import / export — it's read-side
  display-only context).
- `CLAUDE.md` — "Phase 16 is what's next" paragraph item list
  updated.
- `README.md` — landing-paragraph item list updated.
- `data/io.github.virinvictus.atrium.metainfo.xml` — v0.6.19
  release description updated to match.

Pure docs change; no code touched. Ship-gate green.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to
0.6.20.

## v0.6.19 (2026-05-08) — roadmap revision: drop Things 3, elevate Org-mode + Todoist, add Phase 19.5 (productivity essentials)

Pure docs change. Brandon commissioned a feature-survey pass against
competing native-Linux + cross-platform todo apps to identify gaps
in Atrium's roadmap. The findings drove a four-part revision.

**1. Phase 16 (Things 3 Import) retired.** `.things` JSON requires
a macOS export step Linux users don't have access to. As Brandon
put it: "how many people using GNOME are gonna be Things 3 users?"
Things 3 stays in the inspiration paragraph (Simple Mode's calm
+ six-list shape comes from there) but the import phase goes
away. Same logic applied indirectly to OmniFocus — kept open as a
Phase 19 long-tail entry rather than its own phase, since
`.ofocus` has the same macOS-only access problem.

**2. Org-mode promoted to Phase 16 + 17 (was 17 + 17.5).** Brandon's
"MUST" interop direction. Atrium's vault is fully compatible with
Emacs / Doom / vim-orgmode out of the box: open the same
`~/Tasks/` directory in `org-agenda` and the result should look
like Atrium's Agenda canonical page. The two-stage plan (one-shot
import + DB→vault writer at Phase 16; full two-way `inotify` sync
at Phase 17) stays, but the framing tightens to a single must-ship
goal and a new acceptance test pins the agenda parity (with a
synthesised vault, both Atrium's Agenda and `M-x org-agenda`
should bucket tasks the same way).

**3. Todoist promoted to its own Phase 18.** Was bundled into the
Phase 19 long-tail. Brandon's gap-analysis prompt explicitly said
"Todoist would be a good one" — its install base on Linux is real
(web client + Linux Electron app) and CSV export is friction-free.
Now first-class with its own phase. Phase 19 becomes the long-tail
batch (Taskwarrior, VTODO, todo.txt, TaskPaper, OmniFocus).

**4. Phase 19.5 added — productivity essentials.** The gap-analysis
surfaced nine items competing apps have that Atrium doesn't:

- **System notifications / time-based reminders.** Things 3 /
  OmniFocus / Planify all push reminders via the system
  notification daemon. Atrium has zero notification code
  (`libnotify` / `gio::Notification` not imported anywhere).
  For a productivity app this is the biggest 1.0 blocker.
- **Subtasks UI exposure.** `parent_id` has been in the schema
  since `0001_initial.sql` but the GUI doesn't render the
  hierarchy. Schema-supported, UI-missing.
- **Evolution Data Server (EDS) calendar overlay — read-only.**
  Brandon course-corrected the original "iCal feed" framing:
  Atrium is a GNOME-native client on a desktop that already
  has a calendar service. EDS is the GNOME-wide
  calendar/contacts/tasks backend that GNOME Calendar
  (`gnome-calendar`, default in GNOME 50) consumes; the user
  has already configured their accounts there. Read whatever
  EDS exposes via D-Bus and overlay events onto Forecast /
  Today. No `.ics` file plumbing — that would duplicate what
  EDS already does properly. Endeavour does the same shape
  for *tasks*; Atrium does it for *calendars* without
  becoming a calendar client.
- **`AdwPreferencesWindow`.** No app-level preferences dialog
  exists; GSettings keys are set programmatically. Build one.
- **Task dependencies (`blocked_by`).** Taskwarrior treats this
  as fundamental. New `task_dependency` table; `is:available`
  predicate extends to dependency-blocked tasks too.
- **Drag external files / URLs to capture.** Standard Linux
  desktop pattern; explicit in Errands / Planify.
- **Task templates.** Reusable shapes (project + standard
  subtasks). Todoist; Org-mode capture templates as
  conceptual reference.
- **First-run / onboarding.** Sample tasks, welcome project,
  guided three-step intro. Standard commercial-app pattern.
- **Backup / restore UI.** SQLite file-copy is the existing
  escape hatch but no in-app affordance.

Each Phase 19.5 item names its source in `roadmap.md`.

**Sources** (read public README/docs/feature pages — no code
copied):

- Errands — GTK4 / Python — subtasks, drag-drop, accent colors,
  CalDAV / Nextcloud sync.
- Planify — GTK4 / Vala — Todoist + Nextcloud + CalDAV sync,
  multi-reminder, attachments, recurring patterns.
- Endeavour — GTK4 / C — GNOME Online Accounts integration.
- Things 3 — macOS native — Today / This Evening / Upcoming /
  Anytime / Someday / Logbook canonical lists, magic plus
  button, calendar integration, share extensions, Things URL
  scheme, Siri / Shortcuts.
- OmniFocus 4 — macOS native — sequential vs parallel projects,
  Mail Drop, Omni Automation, web access, weekly review, focus
  mode.
- Taskwarrior — CLI — real task dependencies, virtual tags,
  urgency formula, UDA fields, hooks API, named dates, snooze.
- Todoist — cross-platform — natural language input, sub-tasks,
  sections, comments, file attachments, custom filters,
  list/board/calendar view toggle, templates.
- Super Productivity blog comparison piece — open-source
  productivity-app survey.

Files touched: `roadmap.md` (full Phase 16-19.5 rewrite),
`spec.md` (§7.1 import sources table cleaned, §7.4 Linux
landscape table updated, version line bumped), `CLAUDE.md`
("Phase 16 is what's next" line updated), `README.md` (landing
paragraph + Imports section + new Acknowledgments section).
No code changes. No tests touched.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to
0.6.19.

## v0.6.18 (2026-05-08) — efficiency pass: SQL fast-path everywhere search runs

Brandon asked for a top-to-bottom efficiency pass. After surveying
the codebase the honest answer is: Atrium is already pretty efficient
by construction (single-writer worker, read pool, prepared statements
via `prepare_cached`, WAL + tuned pragmas, cold start consistently
20–30 ms, ship-gate runs in under 2 seconds). The clippy pedantic
pass surfaced 250+ items but they're cosmetic — `doc-markdown` nits,
`module-name-repetitions`, etc. — not real efficiency wins.

The actual hot-path wins came from finishing two earlier deferrals
plus eliminating one duplicate DB query:

- **List-renderer perspective path uses the SQL fast-path.** v0.5.3
  shipped the SQL translation evaluator and v0.6.6 wired it into the
  kanban refresh; the deferred case noted in the v0.5.3 patchnote
  was the regular *list*-renderer perspective path — saved
  Perspectives whose renderer is `"list"`. v0.6.18 wires the
  fast-path here too. Translatable filters (most: `is:open`,
  `tag:work`, `due:today`, …) load only matching rows from SQLite
  instead of pulling every task and filtering in Rust. At
  fixture scale (1k tasks) the win is measurable; at 10k+ it
  dominates. Untranslatable expressions (regex / fuzzy / composite
  `is:today` / etc.) keep the in-memory `filter::apply` path —
  no semantic change.

- **Search-bar (SearchResults) path uses the SQL fast-path.** Same
  shape. The bar fired `list_all_tasks` on every keystroke (after
  the 200ms debounce) when the parser successfully built an
  expression; now it fires `list_tasks_matching` with the
  translated `WHERE` clause instead. Same fallback behaviour for
  expressions the translator can't yet express.

- **Eliminate duplicate tag-map DB query on perspective + search
  refresh.** Both paths fetched `tag_names_per_task` *and*
  `tag_info_per_task` back-to-back — same JOIN with one extra
  column on the second query. New helper
  `crate::ui::task_list::tag_names_from_pills(&TagPillMap) ->
  TagMap` derives the name-only view from the colour-bearing pill
  map locally, so we fetch once and project twice. Saves one DB
  roundtrip per refresh.

What I deliberately *didn't* do:

- **Did not download other Rust to-do apps for inspiration.**
  Brandon authorised it but the time cost is high and the
  marginal value is low — Atrium's architecture already follows
  the canonical patterns (worker queue, read pool, GtkListView
  factories with property bindings, FTS5 + bm25 ranking). The
  three wins above came from our own deferred work, not from
  external patterns. If a specific external technique becomes
  relevant later we can attribute it then.

- **Did not chase the 250+ pedantic clippy warnings.** They're
  cosmetic — `doc-markdown`, `module-name-repetitions`,
  `must-use-candidate`, etc. The standard `cargo clippy
  --workspace --all-targets -- -D warnings` is and stays clean.

- **Did not refactor HashMap closure captures into `Rc<HashMap>`.**
  At our scale (typical user has < 200 areas + projects + tags
  combined) the per-refresh clones cost less than 1ms. The
  cleaner ownership model isn't worth the API churn until a real
  workload pushes back.

- **Did not chase `LIKE %x%` table scans.** Bare-text matches in
  the SQL translator emit `LOWER(t.title) LIKE %?% ESCAPE '\\'`,
  which can't use an index. At 100k tasks this would matter; at
  fixture scale (1k) it's ~5ms. The right answer is FTS5 for
  bare text — already used for bm25 ranking — but plumbing it
  through the translator is a bigger surgery best done when
  someone's actually feeling the pain.

## v0.6.17 (2026-05-08) — Forecast view: click-to-open

Brandon flagged that clicking a task in the Forecast view did
nothing. The forecast row had a `gtk::DragSource` (so drag-to-
reschedule worked) but no `gtk::GestureClick` — the row was a
visual dead-end for tap-to-open.

v0.6.17 adds the same `on_row_click` callback shape board and
agenda already use. Single-click on any forecast row (including
the trailing rows under the Overdue card) activates
`win.edit-details-for(id)` and opens the task in the Inspector.
GTK4's drag-threshold means the click + drag controllers
coexist cleanly: a press that doesn't drift past the threshold
fires as a click; a press-and-drag past the threshold initiates
the reschedule drag.

What's in the patch:

- **`atrium/src/ui/forecast.rs`.** `build_page` /
  `build_overdue_block` / `build_day_card` / `build_entry_row`
  all gain an `on_row_click: F` parameter (`F: Fn(i64) +
  'static + Clone`). The callback plumbs from `build_page`
  through the day cards down to each row's `GestureClick`.
- **`atrium/src/ui/window.rs::refresh_forecast_page`.** Builds
  the closure with a `downgrade`d window weak ref and routes
  through `WidgetExt::activate_action(window, "win.edit-details-for", id)`.
  Identical pattern to the board and agenda click handlers.

This closes the last "row doesn't open" gap I'm aware of —
list / kanban / agenda / forecast all open Inspector on
single-click now.

## v0.6.16 (2026-05-08) — sidebar order: Logbook bookends the top tier

Brandon flagged that Logbook in the middle of the top-tier set
(between Someday and Agenda) read as out of place — completed
work was interrupting the flow of active / future-facing lists.
v0.6.16 moves Logbook to the trailing slot so the past lives
where the past belongs.

New top-tier order (both modes):

```
Inbox       capture
Today       today's plate
Upcoming    future scheduled
Anytime     no time commitment
Someday     parked
Agenda      now-picture across days
Forecast    calendar projection (Builder-only)
Review      project review queue (Builder-only)
Logbook     completed past
```

The active/future-facing lists run unbroken from Inbox through
Agenda; Builder mode inserts Forecast + Review without
disturbing the bookends; Logbook closes the top tier so the
sidebar reads as "now → future → past" top to bottom.

What's in the patch:

- **`atrium/src/ui/window.rs`.** Logbook removed from
  `CANONICAL_LISTS` (now five entries: Inbox / Today / Upcoming
  / Anytime / Someday). `top_tier_extras` extended to always
  include Agenda + Logbook; Forecast + Review still gated on
  Builder mode and still sit between Agenda and Logbook.
- **`refresh_canonical_badges`** updated. Logbook's count
  badge moved from the canonical Vec to its own
  `logbook_badge: Option<gtk::Label>` cell; the refresher
  updates both. The `rebuild_dynamic_sidebar` loop captures the
  Logbook badge as it builds the top-tier rows so the count
  stays live across `TaskChanges`.
- **Three unit tests updated** to match the new shape (`CANONICAL_LISTS.len()` is 5, simple-mode extras are Agenda + Logbook, builder-mode extras are Agenda + Forecast + Review + Logbook).

CSS, behaviour, and badge tinting are unchanged — Logbook keeps
its `.atrium-canonical-logbook` purple-2 accent, just at a
different visual position.

## v0.6.15 (2026-05-08) — Memory Watch background + Debug → Generate Fixtures fix

Two real bugs Brandon surfaced testing v0.6.14:

- **Memory Watch dialog had no visible body / background.** Labels
  appeared to float against the system desktop. `adw::Window` with
  an `AdwToolbarView` content slot doesn't auto-paint a window
  background on every theme — the toolbar's content slot is
  transparent and the window underneath wasn't rendering its
  `@window_bg_color`. CSS fix: explicit
  `window.atrium-debug-pane { background-color: @window_bg_color }`.
  The class was already on the window for the monospace font; we
  reused it.

- **Debug menu's *Generate Fixtures* did nothing visible.** The
  action handler was opening a fresh writable connection, writing
  rows directly, but never told the GUI to re-read. The worker's
  read pool kept showing the old cached state, so the sidebar /
  list looked exactly the same after the user clicked. Brandon
  worked around it by running fixture generation via the CLI,
  which spins a fresh process and starts cold. The fix is to
  queue a `rebuild_dynamic_sidebar` + `refresh_active_list` after
  the spawn_blocking write completes — the read pool then
  re-queries the DB and the new rows appear.

Code:

- `data/style.css` — one CSS rule binds `@window_bg_color` to the
  Memory Watch window class.

- `atrium/src/ui/window.rs` — `rebuild_dynamic_sidebar` was
  private; promoted to `pub` so the binary's debug action handler
  can call it.

- `atrium/src/main.rs` — `install_fixture_action` rewritten. The
  DB write now runs via `gio::spawn_blocking` (off the main
  thread; ~30 ms small / ~150 ms medium so a UI freeze would be
  visible), and on completion the closure resumes on the GTK main
  thread to call the window's refresh methods. The previous code
  used `runtime().spawn` (tokio) and tried to capture the
  `adw::Application`, which isn't `Send` — the rewrite uses
  glib's main-context-local spawn which avoids the Send
  requirement entirely.

This closes the two bugs from the v0.6.14 screenshot. The
soft-accent + screenshot-cleanup arc is still done; this is just
the fixture/debug surface catching up.

## v0.6.14 (2026-05-08) — Patch D (reframed): visible row separators + recurrence icon

The original Patch D was "day-band grouping in the main task list."
Walking through the implementation surfaced a scope problem: the
Today list is a single-day view by definition (every row would
read "Today"), Logbook already has day-bands (Slice C2), and Agenda
is the explicit "everything across days" view. Day-band grouping
inside Today / Inbox / Anytime would duplicate Agenda; the only
sensible target was Upcoming, which is a single-view scope rather
than a main-list-wide change.

Reframed Patch D as two smaller polish wins that actually address
what the screenshot showed:

- **Visible row separators.** `GtkListView`'s `show-separators=true`
  was on (window.ui) but the default separator on dark themes was
  so faint that 20+ rows read as a wall of text. v0.6.14 adds a
  1px `@borders`-tinted bottom border to each task row (constrained
  by `:has(.atrium-task-row)` so kanban / agenda card rows don't
  inherit). The eye now has a clear stride between rows without the
  list looking like a heavy table.

- **Recurrence icon (#9b).** Tasks whose `repeat_rule` is set now
  show a small `view-refresh-symbolic` icon at the right edge of
  the row, with a tooltip "Repeating task." The icon is a derived
  state cue — the original screenshot bug was the *fixture* shoving
  emoji into title strings (#9a, fixed in Patch A); the icon now
  reads correctness from `repeat_rule` regardless of what the title
  says. New `repeating: bool` glib property on `AtriumTask`,
  computed at construction + on `refresh_from`. The row factory
  appends a `gtk::Image` after the deadline pill (preserves the
  existing `next_sibling` chain so other bind logic stays
  unchanged) and toggles its visibility via
  `connect_repeating_notify`. Handler stashed under
  `atrium-repeating-handler` and disconnected on unbind. The icon
  picks up the row-state tint when the task is overdue or today,
  matching the date-pill pattern from Patch B.

This closes the four-patch screenshot-cleanup arc:
- v0.6.11 Patch A — eight quick wins (eight files, low risk).
- v0.6.12 Patch B — state-aware row treatment (the biggest visual
  win; overdue red / today amber / upcoming accent).
- v0.6.13 Patch C — Inspector Notes placeholder.
- v0.6.14 Patch D — visible row separators + recurrence icon.

## v0.6.13 (2026-05-08) — Patch C: Inspector Notes placeholder

Small focused patch off the screenshot-cleanup arc. The Inspector
pane's Notes field used to be a blank dark rectangle — first-run
users had no way to know it was editable. v0.6.13 adds a
placeholder hint that disappears the moment the user types.

GtkTextView doesn't have a native placeholder property the way
GtkEntry does, so the implementation is the standard GTK4 idiom:
overlay a `GtkLabel` (set to `set_can_target(false)` so clicks
pass through to the underlying TextView) inside a `GtkOverlay`
that wraps the TextView. The label's visibility tracks the
buffer's character count — visible when zero, hidden otherwise —
via `connect_changed`. The TextBuffer's autosave-on-focus-out
behaviour is unchanged.

Placeholder text reads "What / why / next step — autosaves on
focus-out" so users who haven't read the docs (most of them, most
of the time) understand both *what kind of content* belongs in
the field and *when their input will be saved*.

The recurrence icon piece originally bundled with this patch
(#9b — derive an icon from `repeat_rule`) was deferred — issue
#9 was really about the fixture's emoji-prefixed titles, which
Patch A already fixed. The derived recurrence icon is a polish
"would be nice" rather than a screenshot-bug, so it can wait
for a real use case to push it.

Patch D (day-band grouping in the main task list — Today /
Tomorrow / This Week / Later headers between rows) is the last
one in the four-patch arc.

## v0.6.12 (2026-05-08) — Patch B: state-aware row treatment

The biggest visual win in the screenshot-cleanup arc. Each row now
classifies into one of three states based on its dates + completion
state, and the leading checkbox + the right-hand schedule / deadline
pills tint accordingly. The eye picks up "needs attention" without
reading the dates.

States (mirrors the in-memory evaluator + agenda classify rules):

- **Overdue** — open AND deadline < today. Strong red on checkbox
  + deadline pill. The eye doesn't get to look anywhere else.
- **Today** — open AND most-imminent date == today (where
  most-imminent = `min(scheduled_for, deadline)`). Warm amber.
  "What you said you'd do today."
- **Upcoming** — open AND most-imminent date > today. Theme accent
  (blue by default) at lower alpha so the cue reads as quiet "on
  the way" rather than competing with the urgent states above.
- **Neutral** — no time anchor, completed, or scheduled-someday.
  No special tint; rows look as they did pre-v0.6.12.

Completed tasks (the existing `.completed` class) override the
state tints — a finished task should read as settled regardless
of when its deadline used to be.

What's in the patch:

- **`atrium/src/ui/task_object.rs`.** New `row_state` glib property on `AtriumTask` (`""` / `"overdue"` / `"today"` / `"upcoming"`). New `classify_row_state(&Task) -> String` function that walks the same rules `agenda::classify` uses. Both `from_task_with_tags` and `refresh_from` call it so the property updates on every worker delta — a task whose deadline rolls past today flips state on the next refresh.
- **`atrium/src/ui/task_list.rs`.** Row factory `bind` adds the matching CSS class on initial bind, then a `connect_row_state_notify` keeps it in sync as the property mutates. Three classes (`atrium-task-row-overdue` / `atrium-task-row-today` / `atrium-task-row-upcoming`) are mutually exclusive — the factory drops all three before adding the current one. Handler stashed under `atrium-row-state-handler` and disconnected on unbind.
- **`data/style.css`.** Three CSS rules per state, targeting `checkbutton check` (the GtkCheckButton's checkmark) for the leading colour cue and `.atrium-task-deadline` / `.atrium-task-schedule` for the date-pill colour. A fourth rule resets the colours when the row also has `.completed` so the strike-through treatment isn't fighting the state colour.

Patch C (Notes placeholder + derived recurrence icon) and Patch D
(day-band grouping in the main task list) follow.

## v0.6.11 (2026-05-08) — screenshot-issue cleanup, Patch A (eight quick wins)

First patch off the screenshot-driven issue list logged in v0.6.10.
Eight tightly-scoped low-risk fixes that ship together because each
touches one file and the visual benefit is immediate. The harder
items (state-aware row treatment, Notes placeholder, day-band
grouping) follow in their own patches.

- **Inspector "Defer until: Available now" → "Not deferred."** "Available now" read as a status (every undeferred task is "available now"), not the date-shaped fact the row promises. The new copy treats the absence of a defer date as a date-shaped value.
- **Inspector "Builder" subsection rename.** The pane only renders in Builder Mode, so the "Fields exposed only in Builder Mode" subtitle was redundant noise. Title now reads *Schedule depth*; subtitle dropped.
- **"Inbox" project chip suppressed on the Inbox view.** Every row on that view is in Inbox by definition; the chip just duplicated what the page header said.
- **Window title reflects the active view** — `Atrium · Today` / `Atrium · Inbox` / `Atrium · Q3 plans`. The window-level title shows in window managers, alt-tab overlays, and screencast picker UIs; the bare `Atrium` was a brand sticker, not a context cue.
- **Fixture areas get colours from the six-swatch palette.** Per-area accent stripes (Slice B2, v0.5.0) were invisible in `--fixture small` because no fixture area had a colour set. Now they cycle through the palette, demonstrating the feature without manual setup.
- **Fixture tags get colours from the same palette** (staggered by one entry from areas). Pango-coloured tag pills (v0.3.0) had been monotone in screenshots because the fixture left every tag colour-less.
- **Fixture cleanup: drop emoji prefixes** on `Buy {item}` / `Reminder: …` titles. Those characters were title text masquerading as derived state; a real "this is a recurring reminder" cue should come from `repeat_rule`, not a literal emoji in the title. (The derived recurrence-icon bit lands in Patch C.)
- **`AdwClamp` max-content-size 720 → 960.** Slice B1's 720 px cap left a visible dead zone on wide windows when the inspector pane was visible flush-right (sidebar + main + inspector + the centered clamp's gap). 960 reclaims that space without losing the paper-list calm.

This is one focused commit per the four-patch screenshot-cleanup plan logged in v0.6.10. Patch B is state-aware row treatment (overdue red, today amber, upcoming accent), Patch C is the Notes placeholder + recurrence icon, Patch D is day-band grouping in the main task list.

## v0.6.10 (2026-05-08) — soft-accent pass: warmth without obnoxiousness

The default Adwaita dark theme reads as a uniform grey wall when an
app fills it edge-to-edge with content. v0.6.10 layers a thin
accent-warmth pass across six surfaces — barely perceptible per
rule, additive across the window — so the eye picks up structure
without any single surface screaming. Everything uses libadwaita's
named colour tokens (`@accent_color`, `@warning_color`,
`@success_color`, etc.), so light / dark / high-contrast themes
stay in lockstep.

What got tinted:

- **Sidebar background.** A diagonal accent-color gradient at 2.5%
  → 0% alpha. Almost invisible on its own, but it gives the
  sidebar a subtle directional cue that separates it from the
  main content without a hard divider.
- **Header bars.** Whisper of accent on the leading 35% (4% alpha
  fading to 0). The bar is otherwise a uniform grey strap; this
  hints at the accent without covering any controls.
- **Page title in the header bar.** "Today", "Inbox", "Agenda",
  etc. now render at weight 600 with a hair of letter-spacing.
  The page identity reads as a *headline* rather than just a
  label.
- **Sidebar count badges.** Those "131 / 75 / 178" numbers next to
  Inbox / Today / Upcoming are no longer plain grey — each picks
  up its row's canonical accent (Inbox → blue, Today → yellow,
  Upcoming → green, etc.) at the same alpha as the icon tint, so
  badge and icon read as a kindred set.
- **Sidebar section headers.** "AREAS" / "PERSPECTIVES" / "TAGS"
  pick up a hint of `@accent_color` so they nudge away from pure
  grey toward the accent's hue.
- **Sidebar selection.** Selected row uses a softer accent-tinged
  background (12% alpha) instead of the system's stark selected
  state. The canonical icon tint stays readable when the row is
  selected.
- **Inspector pane group headings.** "Title" / "Schedule" / "Tags"
  / "Notes" / "Builder" pick up an accent-warmth tint so the
  inspector reads as a curated detail panel rather than a cold
  form.
- **Task row hover.** Replaces v0.6.6's instant grey hover with an
  instant accent-tinged hover. Same speed (no transition — drag
  motion stays cheap), warmer hue.

This is a CSS-only patch. No code changes, no schema changes, no
tests touched. The "Brandon ran v0.6.9 and surfaced two warnings"
flow from the previous patch is unchanged — log is still quiet.

What's *not* in this patch (called out in the screenshot
analysis but deferred to follow-up patches):

- State-aware status circles (red for overdue, amber for today,
  etc.) — needs a code-side CSS class per row state.
- State-aware date column (the "May 1" / "May 2" text picking up
  red on past-due, accent on today). Same shape — code-side
  per-row class.
- Inspector "Defer until: Available now" rephrasing — the value
  reads as a status, not a date.
- "Inbox" project chip on no-project tasks — duplicates the
  canonical-list selection signal.
- AdwClamp-induced dead zone on wide windows — the inspector
  pane lives flush against the right edge while the main task
  column is centered with empty space on either side.

## v0.6.9 (2026-05-08) — fix two startup-log warnings

Brandon ran the v0.6.8 binary and surfaced two real warnings in
the log that were going unnoticed in CI:

- **CSS theme parser error at `style.css:488`.** A no-op
  placeholder rule from v0.6.1 used `:not(:last-child)::after`,
  which GTK4's CSS doesn't recognise (`:not()` and pseudo-element
  combinators differ from browser CSS). The rule never rendered
  anything anyway — replaced with a one-line comment explaining
  that visual separation between metadata segments comes from
  the parent box's spacing, not a pseudo-element.

- **Search bar warning on every keystroke.** GTK was emitting
  *"The search bar does not have an entry connected to it. Call
  `gtk_search_bar_connect_entry()` to connect one."* on every
  captured key event. The fix is a one-liner — `bar.connect_entry(&entry)`
  in `wire_search_bar`. This had been missing because the entry
  lives inside a wrapper Box (so the `?` help button can sit
  alongside it), and `GtkSearchBar` only auto-discovers an entry
  that's a direct child. Without the explicit connection, the
  bar's `key-capture-widget=task_list_view` had nowhere to route
  forwarded keystrokes — they fell through and the warning fired.

Both fixes are surgical and surfaced no other warnings in the
log Brandon shared.

## v0.6.8 (2026-05-08) — v0.6.x cleanup pass: docs catch-up + small code hygiene

End-of-session maintenance pass. Eleven v0.6.x releases shipped
since the v0.5.0 line (atrium-cli runtime fix → broken-pipe fix →
FTS5 bm25 → SQL-translation evaluator → Slice D foundation →
kanban GUI → kanban polish → renderer-config dialog → drag-drop →
Agenda canonical page → atrium-cli perspective write side →
kanban CPU mitigation → sidebar top-tier reorg). The contract
docs (`spec.md`, `roadmap.md`, `README.md`) lagged behind the
patches; this release brings them back into alignment per the
"Spec discipline" rule in `CLAUDE.md`.

What's in the patch:

- **`spec.md`** — version header bumped from 0.5.0 to 0.6.7 with a one-line summary of what 0.6.x delivered. Three new sections added without renumbering the existing tail: §4.4 (FTS5) gains a "Bm25 + recency ranking" subsection documenting the saturating-relevance + half-life math; §4.5 (SQL-translation evaluator) describes the all-or-nothing translation rule, the parity-test backstop, and the current coverage / fall-back set; §4.6 (Perspective renderers) documents the `'list'` / `'board'` axis and the Slice D locked rules (leftmost-match-wins, "Other" trailing column, case-insensitive matching, `move_to_column` drag-rewrite). The original §4.5 (Migrations) renumbers to §4.7. §5.2 (Builder Mode) gains a description of the kanban board renderer; new "Mode-agnostic additions" subsection covers Agenda + the v0.6.7 sidebar reorganisation.
- **`roadmap.md`** — Phase 15.75 rewritten to reflect what actually shipped. All seven previously-deferred items are now `[x]`-checked with their landing versions (Slice C v0.5.0 → v0.6.0, Slice D v0.5.4 → v0.6.5, FTS5 bm25 v0.5.2, SQL pushdown v0.5.3, sidebar reorg v0.6.7, CLI bulk operations v0.4.6, regression-script integration v0.5.x). Each line traces the actual code paths so the roadmap reads as a "what shipped where" map rather than a planning document.
- **`README.md`** — landing paragraph extended with a v0.6.x summary covering Slice D, FTS5 bm25, the SQL-translation evaluator, and the sidebar reorg. The detailed feature surface in the lower sections still describes v0.5.0 capabilities accurately, so a full README rewrite isn't due until the next major.
- **Code hygiene.** `print_perspective_after_write` had a dead `&Connection` parameter (introduced when refactoring perspective output); dropped it and the now-unused parameter through `run_perspective_create`. Two stale "Phase X will" promise comments updated — the SQL-translation comment in `window.rs::refresh_active_list` no longer claims "Stage 3 will add" (Stage 3 shipped at v0.5.3), and `task_list::ActiveList::task_matches`'s old "Phase 5c will revisit" promise is now an accurate description of the current behaviour.
- **Workspace clippy clean.** `cargo clippy --workspace --all-targets -- -D warnings` reports zero warnings.
- **Regression-script ship gate green at v0.6.8.**

What's *not* in this patch (deliberately deferred — these are larger surgeries that warrant their own changes):

- `atrium/src/ui/window.rs` is at ~5000 lines. A `ui::sidebar` extraction is the obvious next refactor target; the composite-template wiring couples a lot to it though, so it's a careful surgery not a quick cleanup.
- The list-renderer Perspective path in `refresh_active_list` doesn't yet use the SQL fast-path (only the board path does, as of v0.6.6). Adding it is the same shape but the sort-spec / bm25 plumbing needs to align.

## v0.6.7 (2026-05-08) — sidebar reorganisation: Agenda / Forecast / Review join the top tier

The "Builder" sidebar header is gone. Agenda / Forecast / Review
no longer hide at the bottom of the sidebar in Builder mode — they
now sit in the top tier alongside Inbox / Today / Upcoming /
Anytime / Someday / Logbook, with their own accent tints:

- **Agenda** appears in *both* Simple and Builder modes (the
  agenda is a pure read view with no Builder-only concepts;
  it makes sense to surface it everywhere). Accent: warning
  red on the alarm-clock icon, so urgency reads at a glance.
- **Forecast** + **Review** stay Builder-only but join the top
  tier in that mode. Accents: cool blue (calendar) and success
  green (checkmark).

Perspectives section moves up from the bottom of the sidebar to
right under the top-tier group — above Areas, below "the Inbox
grouping," exactly as the user wanted.

Final sidebar order:

- **Both modes:** Inbox, Today, Upcoming, Anytime, Someday,
  Logbook, Agenda
- **Builder mode adds:** Forecast, Review (still in the top
  tier), then a "Perspectives" section header + its rows
  underneath
- **Both modes continue with:** Areas (and nested projects),
  Unfiled projects, Tags

What's in the patch:

- **`atrium/src/ui/window.rs`.** New `top_tier_extras(builder)` helper returns the post-canonical rows that should appear in the current mode. `rebuild_dynamic_sidebar` now appends those rows + the Perspectives section *before* Areas, instead of the old "Builder" section header at the bottom. `canonical_accent_class` extended to cover Agenda / Forecast / Review.
- **`data/style.css`.** Three new accent rules (`.atrium-canonical-agenda` → `@warning_color`, `.atrium-canonical-forecast` → `@accent_color`, `.atrium-canonical-review` → `@success_color`). Same alpha treatment the canonical rows already use, so they sit alongside without screaming.
- **Three new unit tests** pin the top-tier shape (Simple = just Agenda; Builder = Agenda + Forecast + Review in that order) and the accent-class wiring so a future tweak can't quietly drop the tints.

## v0.6.6 (2026-05-08) — kanban drag-drop CPU mitigation

Two targeted optimisations to address the CPU spike Brandon
reported during kanban drag operations:

- **Drop the hover transition on board / agenda task rows.**
  v0.6.1 added a `transition: background-color 120ms ease-out`
  on `.atrium-board-task-row` (and Agenda inherited the same
  pattern). During a drag, the cursor crosses many rows in
  succession; each crossing fired a 120ms CSS animation
  producing continuous repaint work and a visible CPU spike.
  The hover background still applies — it's just instant now,
  so there's no per-frame paint cost.

- **SQL fast-path on board refresh.** v0.5.3 added the SQL
  translation evaluator to atrium-cli; v0.6.6 wires it into
  the GUI's `refresh_board_page`. When the perspective's
  filter expression translates cleanly to SQL (most do — the
  fixture's `is:open` does), we now load only the matching
  task rows from SQLite instead of pulling every row and
  filtering in Rust on every drop. At 1000-task scale that
  cuts the per-drop work meaningfully; at 10K+ it'll
  dominate. Falls back to the in-memory evaluator for
  expressions the translator doesn't yet cover (regex /
  fuzzy / composite is:today / etc.).

What's also in the patch:

- **`atrium_core::SqlBindValue` enum.** Pulled the binding
  conversion out of atrium-cli's local helper and into a
  proper public type on atrium-core. The atrium GUI binary
  now bridges to it without needing a direct rusqlite dep.
  `From<atrium_search::SqlValue> for atrium_core::SqlBindValue`
  lives in atrium-search so call sites just say `.into()`.
- **`filter::sort_tasks_by_specs`.** Tiny re-export of the
  sort-spec helper so the SQL fast-path in window.rs can
  apply explicit `sort:` modifiers without re-running the
  full `filter::apply` pipeline.

If the CPU spike persists after this patch, the next move is
either (a) profile with `tracing` spans around the rebuild
to find the dominant cost, or (b) coalesce/debounce
TaskChanges-driven refreshes so rapid drops only trigger one
rebuild at the end. Both are clean follow-ups for a fresh
session.

## v0.6.5 (2026-05-08) — atrium-cli perspective write side

Closes the gap that the only way to create or convert a saved
perspective from the shell was via direct SQL. Three new sub-
subcommands under `atrium-cli perspective`:

```bash
# Create a list-renderer perspective.
atrium-cli perspective create 'Q3 plans' --filter 'project:"Q3 plans"'

# Convert it to a kanban board.
atrium-cli perspective edit 'Q3 plans' --renderer board \
  --columns 'todo,doing,done'

# Update the column list in place (renderer stays as board).
atrium-cli perspective edit 'Q3 plans' --columns 'backlog,todo,doing,done'

# Rename + re-icon + retune the filter in one shot.
atrium-cli perspective edit 'Q3 plans' \
  --rename 'Q3 plans (rev 2)' \
  --icon view-grid-symbolic \
  --filter 'project:"Q3 plans" AND is:open'

# Back to a flat list.
atrium-cli perspective edit 'Q3 plans (rev 2)' --renderer list

# Tear it down.
atrium-cli perspective delete 'Q3 plans (rev 2)'
```

Locked semantics:
- **Name lookup is case-insensitive exact** for write paths
  (edit / delete) — substring fallback would risk editing the
  wrong perspective on a typo. Read-only `kanban NAME` keeps
  its substring fallback because there's no such risk.
- **`--renderer board` requires `--columns`** on create. On edit,
  `--columns` alone is allowed *if the perspective is already a
  board* — that's the in-place column-list update.
- **`--icon none`** clears the icon (back to the default); a
  bare value sets it.
- **`perspective edit` with no flags is a noop** — prints the
  existing row so the user gets a confirmation that they
  matched the right name.

What's in the patch:

- **`atrium-cli/src/args.rs`.** New `Subcommand::Perspective(PerspectiveSub)`; new `PerspectiveSub` enum (Create / Edit / Delete) and `PerspectiveArgs` flag bundle; new `EditIcon` tri-state for the `--icon` flag; new `parse_perspective` body parser that supports multi-word names + the full flag vocabulary. USAGE help text extended with the new shape.
- **`atrium-cli/src/main.rs`.** New `run_perspective` dispatcher + `run_perspective_create` / `run_perspective_edit` / `run_perspective_delete` handlers. Helper functions `build_renderer_config`, `synthesise_renderer_for_edit`, `parse_columns`, `resolve_perspective_exact` keep the renderer/columns logic in one place.
- **13 argv-parsing tests.** Cover create-minimum, missing --filter, board+columns, --rename rejection on create, invalid renderer, edit-with-all-flags, --icon none, edit-noop, delete-name-only, delete-rejects-body-flags, unknown sub, no-sub, multi-word names.
- **Regression-script smoke (step 5.5).** Now exercises the full create → edit (convert to board) → edit (update columns) → edit (back to list) → delete round-trip plus a `perspective edit … (no flags)` noop and a `--json list perspectives` post-condition assertion.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to 0.6.5.

## v0.6.4 (2026-05-08) — Slice D2: Agenda canonical page

Org-mode-style "everything you should think about right now" view.
A new canonical page (sidebar entry next to Forecast / Review) that
groups open tasks into five chronological sections:

- **Overdue** — open AND `deadline < today`. Surfaces past-due
  work first so it isn't buried under future scheduling.
  Heading is rendered in red to flag urgency at a glance.
- **Today** — most-imminent date == today. "Most-imminent" is
  `min(scheduled_for, deadline)`. Same rule the regular Today
  list uses, plus deadline-today.
- **Tomorrow** — most-imminent == today + 1.
- **This Week** — most-imminent within the rest of the current
  ISO Mon-start week (after Tomorrow). Empty on Sunday.
- **Next Week** — most-imminent within next ISO Mon-start week.
- Tasks farther out live in Forecast; tasks without a time
  anchor (no scheduled, no deadline) don't appear; completed
  and deferred-future tasks don't appear.

Each section is an Adwaita card with a heading + count and a
vertical task list. Rows show title + date chip + project name
+ tag pills. Click any row → opens in the Inspector. Empty
agenda gets an `AdwStatusPage` "Nothing on the agenda" banner.

What's in the patch:

- **`atrium/src/ui/agenda.rs`.** New module. `AgendaSection` enum, `classify(task, today)` (returns `None` when not on agenda), `group_by_section(tasks, today)` returning `Vec<(AgendaSection, Vec<Task>)>` in canonical order, `build_page(today, tasks, …)` returning the GTK widget. **14 unit tests** covering the classification rules: completed-skip, deferred-future-skip, no-anchor-skip, someday-skip, overdue precedence, scheduled-today / deadline-today / scheduled-tomorrow, this-week / next-week boundaries, beyond-next-week-skip, most-imminent-wins-when-both-dates-set, group_by_section ordering and filtering.
- **`ActiveList::Agenda` variant.** Added to `task_list::ActiveList`; matched everywhere ActiveList is exhaustive.
- **Sidebar entry.** Builder-mode sidebar gains an "Agenda" row between Forecast and Review (same group, same shape).
- **`refresh_agenda_page` + content stack page.** `data/window.ui` adds an `agenda_host` AdwBin in a new GtkStackPage `"agenda"`; `refresh_active_list` and `apply_task_changes` route `ActiveList::Agenda` through it.
- **CSS.** `.atrium-agenda-section` + `.atrium-agenda-overdue` (heading turns red) + `.atrium-agenda-row-meta` styling so the agenda reads as a focused composite view rather than another flat list.

The agenda is currently Builder-only (matches the pattern Forecast / Review / Perspectives use). A future polish pass could surface it in Simple Mode too — the underlying data is mode-agnostic.

## v0.6.3 (2026-05-08) — kanban drag-drop between columns

The kanban is no longer read-only. Drag a task row to a different
column → the task's tag set is rewritten so the kanban grouper
buckets it under the new column on the next refresh:

- The leftmost configured-column tag in the task's current set
  is removed (that was the source column).
- The destination column's tag is added if not already present.
- Non-column tags pass through unchanged.
- Dropping on the trailing "Other" column just removes the
  source column tag — the task lands in Other for not matching
  any configured column.

The tag-set-rewrite logic is `atrium_core::move_to_column` —
pure-Rust, no GUI dependencies, eight unit tests cover the
combinatorial cases (move-to-column, move-to-other, move-to-same,
non-column passthrough, no-source, case-insensitive,
no-duplicate-on-existing, leftmost-only-removal).

The GUI side is plain GTK4 DnD: each row registers a
`gtk::DragSource` carrying the task id, each column card a
`gtk::DropTarget` accepting `i64`. The drop callback walks the
task's current tag names through `move_to_column`, then
dispatches `worker.ensure_tag` for each new name and
`worker.set_task_tags` to install the result. No-op short-circuit
when the new tag list is set-equal (case-insensitive) to the old
one — covers the common "drop on the same column" case without a
worker round-trip.

## v0.6.2 (2026-05-08) — perspective renderer-config dialog

Closes the v0.6.0 gap that the only way to make a Perspective
render as a kanban was direct SQL or the test fixture. Right-
clicking a Perspective row in the sidebar now exposes a
"Configure renderer…" item that opens an `AdwAlertDialog`:

- Two radio toggles: **List** (default flat task list) /
  **Board** (kanban columns).
- When Board is selected, a comma-separated entry takes the
  column list — pre-populated with the existing columns when
  editing an already-configured board.
- Save → writes `perspective.renderer` and
  `perspective.renderer_config` via the worker.
  `apply_library_changes` re-renders the active perspective
  immediately, so the column layout appears without needing
  a sidebar refresh.

What's also in the patch:

- **`BoardConfig::to_json` / `BoardConfig::from_json`.** The
  GUI dialog uses these to round-trip the JSON shape without
  pulling `serde_json` into the GTK binary. Pinned by two
  unit tests — one for the round-trip, one for the exact
  emitted shape so a future serde derive tweak can't silently
  rename the JSON keys.

The CLI doesn't yet have a board-renderer setter (the v0.5.4
`atrium-cli kanban NAME` only renders an *existing* board). A
sibling patch will add `atrium-cli perspective …` for the
write side; for now, perspective creation/config from the
shell is "edit the DB directly or use the GUI dialog."

## v0.6.1 (2026-05-08) — kanban polish: row metadata + interactive checkbox

The first polish pass on the v0.6.0 kanban. Two gaps closed:

- **Row metadata line.** Project name, the most-relevant date
  (deadline trumps scheduled; Someday renders as the literal
  "Someday"), and tag pills (using the same Pango-coloured
  markup the regular task list uses) now appear under the title
  when any of them are set. Tasks with no metadata stay tight —
  the metadata row is suppressed entirely rather than rendering
  empty.
- **Interactive checkbox.** Clicking the checkbox toggles the
  task's completion via the worker, same as the regular list
  view. The board re-renders on the next `apply_task_changes`
  delta. Previously the checkbox was render-only.

Drag-drop between columns and a board-renderer editing UI are
still the next slices.

## v0.6.0 (2026-05-08) — Slice D1 GUI (read-only kanban board page)

The first GUI consumer for the v0.5.0 `perspective.renderer` /
`renderer_config` columns. A saved Perspective whose `renderer =
"board"` now renders as a horizontal column layout in the GTK
binary instead of a flat list. Each column is a tag — leftmost
match wins, "Other" trailing column for tasks that don't match
any configured column. Same engine the v0.5.4 `atrium-cli kanban`
subcommand uses (`atrium_core::render::group_into_board`).

What's interactive in v0.6.0:

- Click any task row → opens it in the Inspector (same
  `win.edit-details-for(i64)` action the regular list and
  keyboard shortcuts go through).
- Vertical scrolling per column for tall task lists.
- Horizontal scrolling across the whole board when the column
  set exceeds the viewport.

What's *not* interactive yet (deferred to a follow-up patch):

- Drag-drop between columns. Today, moving a task between
  columns is "edit the task's tags from the Inspector or via
  `atrium-cli edit ID --tag X --remove-tag Y`."
- The completion checkbox renders the state but isn't
  click-toggleable from the board view (use the regular task
  list or the Inspector).
- No board-renderer editing UI yet — to convert a Perspective
  to a board, edit `renderer` and `renderer_config` directly.
  An editing dialog ships in a future slice.

What's in the commit:

- **`atrium/src/ui/board.rs`.** New module. `build_page(name, columns, on_row_click)` returns a horizontally-scrolling `gtk::Box` with one card-styled column per `Column<'_>`. Per-column scrolling caps at 420px tall; per-row click activates the inspector via the supplied callback.
- **`data/window.ui`.** New `GtkStackPage` named `"board"` with an `AdwBin id="board_host"` host, mirroring the forecast/review/logbook pattern.
- **`atrium/src/ui/window.rs`.** Window struct gains a `board_host` template child. New `refresh_board_page(perspective)` method orchestrates load → filter → bm25 rank → group → mount. The `ActiveList::Perspective(id)` branch in the active-list refresh checks the perspective's renderer; `"board"` switches to the board stack page, anything else falls through to the existing list rendering.
- **`data/style.css`.** Adwaita-`card`-class kanban columns, subtle hover tint on rows, transparent scroller backgrounds so the board reads as one surface rather than nested boxes.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to 0.6.0.

## v0.5.4 (2026-05-08) — Slice D1 foundation (kanban renderer + atrium-cli)

The first slice of Slice D — saved Perspectives can now render as
kanban boards. v0.5.4 ships the *headless* foundation: parser,
grouping engine, and a complete CLI consumer; v0.6.0 will land the
GUI rendering on top of these pieces.

The kanban contract is small and opinionated:

- **Schema reused.** `perspective.renderer = "board"` plus
  `perspective.renderer_config = '{"axis":"tag","columns":["…"]}'`.
  These columns shipped at v0.5.0 (Slice A); this is what they're
  *for*.
- **Leftmost match wins.** A task with multiple matching tags
  appears in only the leftmost matching column. Kanban is a state
  view — a task is in *one* state at a time.
- **"Other" trailing column.** Tasks that don't match any
  configured column always appear in a final `"Other"` bucket so
  the kanban stays honest about coverage. Users who want a
  tighter view tighten the perspective filter (e.g.,
  `is:open AND tag:true`).
- **Case-insensitive tag matching.** Mirrors the rest of the
  search-engine tag rules.

What landed:

- **`atrium-core::render` module.** New file. `Renderer::from_columns(renderer, config_json)` parses the `(renderer, renderer_config)` pair into a typed `Renderer` enum. `group_into_board(tasks, &cfg, &tag_names_per_task)` walks a task list and emits one `Column<'_>` per configured column plus the trailing `Other`. 17 unit tests cover parsing rejection (unknown axis, blank columns, missing config, unknown kind), grouping rules (untagged → Other, leftmost-wins, case-insensitive, input-order preservation, empty input).
- **`atrium-cli kanban NAME`.** New subcommand. Resolves a perspective by case-insensitive name (exact first, substring fallback), parses its renderer_config, runs the perspective's filter expression through the v0.5.3 SQL fast-path / in-memory eval, groups by tag, and prints columns. TSV / JSON / `--human` formats. Errors clearly when the perspective is missing or its renderer is `"list"` instead of `"board"`.
- **Fixture board perspective.** `--fixture small` seeds a `"Fixture Board"` perspective with three tag columns (`tag-0`, `urgent-3`, `home-4`) so the kanban subcommand has something to render in test contexts and the CLI smoke step can exercise it without seeding a perspective by hand.
- **Regression-script kanban smoke.** `scripts/regression.sh` step 5.5 now exercises `atrium-cli kanban Fixture Board` in TSV / JSON / human formats plus the negative case (`atrium-cli kanban Weekly Review` must error with `"is a list, not a board"` since the seeded Weekly Review is a list-renderer perspective).

The GUI rendering of board perspectives — switching from a flat list to a horizontal column layout, drag-drop between columns rewriting the underlying tag — lands in v0.6.0. The agenda/overview view (Slice D2) follows.

## v0.5.3 (2026-05-08) — SQL-translation evaluator (atrium-cli)

The fourth v0.6.x carryover. The Calibre-style search expression
language now executes at the SQLite layer instead of pulling every
row into memory and filtering in Rust — for queries that translate
cleanly. The translator's "all-or-nothing" rule keeps semantics
unchanged: anything that can't be expressed in SQL (regex match
modifiers, fuzzy matches, sequential-project state, the composite
`is:today` family) falls back to the in-memory evaluator. The two
paths are pinned to identical behaviour by 21 parity integration
tests in atrium-cli.

The win matters most at the 100K-task scale (spec §8 perf budget).
A search that previously loaded 100K rows + iterated them in Rust
now lets SQLite's query planner do the work using its existing
indexes. Wired into atrium-cli for v1; the GUI search-bar +
saved-Perspective wiring follows in a sibling patch.

- **`atrium-search::sql_translate`.** New module. `try_translate(&Expr, today) -> Option<SqlClause>` walks the parsed AST and emits a SQL `WHERE` fragment + parameter list when every node maps cleanly to SQL. Returns `None` for any subtree containing `MatchKind::Regex`, `MatchKind::Fuzzy`, `State::Available`/`Queued`, `State::Today`/`Inbox`/`Upcoming`/`Anytime`/`Someday` (composite list-membership), `State::InArea`/`Archived`, `Field::Project`/`Area` (deferred — would need joins), or any unsupported `Field`/`MatchKind` combination. 21 unit tests.
- **`atrium-search::dates`.** Extracted from `eval.rs` so the SQL translator and the in-memory evaluator share the same date-keyword arithmetic (`today`, `thisweek`, `5daysago`, …). Single source of truth — no drift possible between paths.
- **`atrium-core::db::read::list_tasks_matching`.** New helper that runs a pre-built SQL `WHERE` fragment + bound params against the `task` table and decodes the resulting rows. Plain `prepare` (not `prepare_cached`) since the WHERE clause varies per query — caching would unboundedly grow the per-connection statement cache.
- **`atrium-cli::filtered_tasks`.** New private helper consumed by `run_search` and `resolve_matching_tasks`. Calls `try_translate` first; on `Some`, executes via `list_tasks_matching`; on `None`, falls back to the existing `list_all_tasks` + in-memory `evaluate` path. Same input expression → same task ID set on both paths.
- **Parity tests.** 21 cross-validation tests in `atrium-cli/src/tests.rs::sql_parity` seed a small mixed-shape fixture (open + done + overdue + scheduled + deferred + repeating + tagged tasks), run a battery of expressions through both paths, and assert identical id sets. Includes negative tests confirming `try_translate` correctly rejects regex / fuzzy / `is:today`.

## v0.5.2 (2026-05-08) — FTS5 bm25 + recency ranking on bare-text searches

The third v0.6.x carryover off the deferred list. Bare-text searches
(`atrium-cli search milk`, the GUI search bar with a freeform word)
now rank by FTS5's `bm25` blended with a 30-day half-life recency
factor. Stronger matches and freshly-touched tasks rise to the top
instead of every result coming back in `task.position` order.

- **`atrium-search::rank` module.** Two pure helpers — `collect_text_terms` walks the parsed AST for `Expr::Text` nodes, `blend_relevance` maps `bm25` + `days_since_modified` → a single comparable score on a stable scale. Twelve unit tests cover the math (saturating relevance, recency half-life, clamped negative days, AND/OR/NOT walking, field-scoped exclusion).
- **`atrium-core::db::read::bm25_for_terms`.** Queries FTS5 with the term set unioned via `OR`, returns `HashMap<task_id, bm25>` for the matching rows. User input is double-quote-stripped + phrase-quoted so a stray `"` can't inject MATCH operators. Six tests cover the empty / blank / quote-injection edge cases plus a term-frequency rank check.
- **CLI wiring (`atrium-cli`).** `run_search` calls the rank helper after the in-memory evaluator, only when the query has bare text and no explicit `sort:` modifier. Skipped automatically when `sort:` is present so power users keep their explicit ordering.
- **GUI wiring (`atrium/ui/filter::rank_by_bm25_recency` + window.rs).** Same fast-path applied to both the search-bar's transient SearchResults list and saved Perspectives whose filter contains bare text. Four window-side unit tests cover the no-op / strong-match / recency-tiebreak / unscored-fallback cases.
- **No new dependencies.** Sits on the existing FTS5 `task_fts` virtual table that's been in place since migration `0001_initial.sql`.

## v0.5.1 (2026-05-08) — atrium-cli runtime fix + ship-gate smoke + broken-pipe fix

A focused patch with three small, coupled fixes that the v0.5.0 ship-gate hadn't been wide enough to catch.

- **atrium-cli runtime nesting fix.** `with_writer` previously called `Handle::current().block_on(...)` from inside an outer `runtime.block_on(...)`, which is a "Cannot start a runtime from within a runtime" panic the moment any write subcommand ran. Reshaped to spawn the worker inside `block_on` and exit, then pass `&Runtime` to each `run_X` so subsequent `block_on`s run outside async context. The worker future stays alive on the runtime; each `handle.foo()` awaits a single mpsc round-trip. No behavioural change at the user level — the panic was hit by every write path.
- **Ship-gate end-to-end smoke for atrium-cli.** `scripts/regression.sh` step 5.5 exercises every read subcommand, every search-operator class shipped at v0.5.0, the JSON formatter (now via `head -c 1` to also exercise the broken-pipe path), the add → info → search → edit → complete → delete write round-trip, and the bulk `delete --where` dry-run / `--force` flow. Closes the architectural commitment that every non-GUI surface stays CLI-testable — without this step, the runtime nesting panic would have shipped silently in v0.5.0.
- **Broken-pipe behaviour.** Rust's default-installed SIGPIPE handler is `SIG_IGN`, which means a `println!` to a closed stdout panics on the next write. Atrium-cli now resets SIGPIPE to `SIG_DFL` at startup (inline `unsafe extern fn signal` so we don't add a `libc` dep) — pipes into `head`, `head -c N`, `q`-pressed pagers, etc. now exit cleanly instead of dumping a Rust panic message onto the user's terminal.

## v0.5.0 (2026-05-08) — atrium-cli, search engine evolution, Phase 15.75 visual polish

A meaty minor — this release rolls together fifteen post-v0.4.0 patches into one shippable boundary. Three threads finished and one started:

1. **Phase 15.75 (partial) — visual polish + per-area accent.** Foundation migrations, beauty pass, and per-area colour rendering all landed. The board view (Slice D) and GTD-audit work (Slice C) remain for v0.6.0 / Phase 15.75 finish.
2. **Phase 15.5 deferred-list — closed.** Every search-engine line item the v0.4.0 release punted into "v0.4.x patch" territory shipped: state-predicate coverage, `sort:` modifier, ↑/↓ history, `?` operator-reference popover, fuzzy match, plus the SQL-translation evaluator and FTS5 ranking still pending for a future patch.
3. **Architectural extraction — atrium-search + atrium-cli.** The search engine and a full headless CLI both live as their own workspace crates. The GTK binary is no longer the gatekeeper for the search engine or the data layer.
4. **CLI-testable everything.** Every non-GUI surface is now exercisable from the shell. Foundation for the 2.0-era TUI / atriumd capture daemon.

### Phase 15.75 visual polish

- **Foundation (Slice A).** Two additive migrations — `0004_area_color.sql` (one new column on `area`) and `0005_perspective_renderer.sql` (two new columns on `perspective`: `renderer TEXT NOT NULL DEFAULT 'list'` and `renderer_config TEXT NULL`). Domain types and worker SQL grew alongside; user_version 3 → 5. No UI consumer yet for the perspective renderer columns — that's Slice D's board view, deferred to v0.6.0.
- **Visual rhythm (Slice B1).** `.atrium-task-row:hover` gains a subtle inset bottom border (`@card_shade_color` 1px) plus alpha bump 0.08 → 0.10 for a "lift" cue. `.atrium-sidebar-section` letter-spacing 0.04em → 0.06em — section headers read more clearly as labels. `.atrium-note-body` picks up `font-style: italic` + tighter line-height (1.55 → 1.6); both Inspector surfaces (Simple-mode dialog + Builder-mode pane) now attach the class to their notes TextView so the editable Notes field reads as a writing surface, not a clone of the row chrome. Task list wrapped in an `AdwClamp` (max 720 px) so rows don't stretch into runway on wide windows.
- **Per-area accent (Slice B2).** `prompt_for_tag` generalised to `prompt_for_named_color` with a `placeholder` parameter. Tag callers (3 sites) pass "Tag name"; new area callers (2 sites) pass "Area name". `prompt_create_area` and the Area arm of `prompt_rename_active` now both surface the six-swatch picker. `build_area_row` mirrors `build_tag_row`'s coloured-dot pattern when `area.color` is set. `AtriumTask` gains an `area_color` glib property; `apply_area_accent` toggles the matching `.atrium-area-accent-{color}` CSS class on bind + on every notify so a project move that shifts a task under a differently-coloured area updates the stripe in place. Six new CSS rules paint `border-left-color` at alpha 0.7 on each `.atrium-area-accent-{color}` class. `replace_store_with_tags_seq` + `apply_changes_seq` grow an `area_color_for: G` closure parameter alongside the existing `context_for`; three call sites in `window.rs` pass the new resolver via `build_area_color_resolver`.
- **About-dialog icon resolution.** `typography::register_icon_search_paths` walks three candidate paths (ATRIUM_DATADIR runtime env, compile-time install, `CARGO_MANIFEST_DIR`-relative dev fallback) and registers each existing one with `gtk::IconTheme::for_display`, so AdwAboutDialog's `application_icon(APP_ID)` lookup finds the bundled SVG during `cargo run` development. Installed builds were always fine.
- **Subtle warmth.** Each canonical sidebar list now carries a quiet accent on its leading symbolic icon — Things-3-style. Inbox `@blue_3`, Today `@yellow_5`, Upcoming `@green_4`, Anytime unchanged (intentional neutral beat), Someday `@purple_3`, Logbook `@purple_2` (faded). All wrapped in alpha 0.75–0.95 so accents read as personality, not signage. Also fixed the "cancel symbol" tag icons — `tag-outline-symbolic` isn't in the GNOME standard set; switched to `tag-symbolic`.

### Search engine evolution (Phase 15.5 deferred-list closure)

- **Canonical-list state predicates.** Five new `is:NAME` shortcuts mirroring the canonical sidebar lists per spec §4.2: `is:today`, `is:inbox`, `is:upcoming`, `is:anytime`, `is:someday`. Each pairs with `!is:NAME` for the inverse. Closes the user-mental-model gap that `due:today` (correctly exact-match on Deadline) doesn't surface tasks scheduled for today — `is:today` is the broader Today-list mirror.
- **`sort:` modifier.** `sort:KEY` (ascending) / `sort:-KEY` (descending) with primary → secondary composition. Recognised keys: `due` (alias `deadline`), `scheduled` (alias `when`), `defer`, `created`, `modified`, `completed`, `estimated`, `title`, `position`. NULLs sort last regardless of direction (SQL convention). Implemented as a parser-time AST extraction (the `Expr::Pass` placeholder + `ParseResult.sorts` metadata) so the evaluator never sees a sort modifier as a predicate.
- **Fuzzy `?` modifier.** `tag:?work` matches with Damerau-Levenshtein within a length-aware threshold (≤4 chars → 1, 5–7 → 2, ≥8 → 3). Damerau (vs plain Levenshtein) counts a transposition of adjacent characters as a single edit, so `tag:?wrok` matches `work` — the most common typing slip survives fuzzy without falling back to substring.
- **Search history (↑ / ↓).** 20-entry in-memory ring buffer of recent committed queries. ↑ steps back, ↓ moves toward newer entries; pressing ↓ off the most-recent entry returns to the live entry. Pure-Rust `push_history_entry` + `cycle_history_cursor` helpers keep the state-machine logic out of GTK glue and unit-testable.
- **Operator-reference popover (`?` button).** The search bar grew a `?` GtkMenuButton; clicking opens a structured quick-reference organised by section (Boolean, Fields, Modifiers, Comparison & range, Date keywords, State, Sort). Closes the discoverability gap — without this the search-engine power was invisible to anyone who hadn't read spec §4.3.

### atrium-search workspace crate (v0.4.2)

`atrium-core/src/search/` was lifted into its own sibling workspace crate `atrium-search`. Same code, same tests, no behaviour change — the move means the parser/evaluator can be fuzzed, benchmarked, and reused (atrium-cli + future TUI / atriumd / search server) without dragging the SQLite/worker layer along. atrium-core no longer depends on `regex`. The codebase map in `CLAUDE.md` documents the four-crate workspace.

### atrium-cli — headless data + search access

A whole new headless binary, sibling to the GTK app:

- **Read commands.** `search EXPR` (full search expression language, sort modifiers honoured), `list NAME` (canonical task lists: inbox, today, upcoming, anytime, someday, logbook, all; metadata lists: areas, projects, tags, perspectives), `info ID` (full task detail).
- **Write commands.** `add TITLE [flags]` (full NewTask flag soup with date keywords, project resolution by case-insensitive substring, tag attachment via ensure_tag), `capture LINE` (Quick-Entry-style one-shot capture using the same inline-syntax parser the GUI's bottom-of-list entry uses — lifted from `atrium/src/quickentry/parser.rs` to `atrium-core/src/quick_entry.rs` at v0.4.5), `edit ID [flags]` (diff-based field updates including additive tag flags `--tag X` / `--remove-tag X` / `--clear-tags`), `complete ID` (toggle), `delete ID`.
- **Output formats.** `--tsv` (default — header row + sanitised columns; `cut`/`grep`-friendly), `--json` (serde_json array; `jq`-friendly), `--human` (pretty columns with truncation; for terminal viewing).
- **Database resolution.** `--db PATH` flag → `ATRIUM_DB_PATH` env → XDG default. Read commands open `SQLITE_OPEN_READ_ONLY` so a buggy query attempting an INSERT errors at the engine — no CLI invocation can corrupt the user's database through a read path.

### Numbers

- **362 tests pass total** (89 atrium + 63 atrium-cli + 136 atrium-core + 73 atrium-search + 1 mode-flip integration). Up from 248 at v0.4.0 (+114).
- **Workspace shape:** four crates (`atrium-core`, `atrium-search`, `atrium-cli`, `atrium`).
- **Schema version:** 5 (was 3 at v0.4.0; +0004 area_color, +0005 perspective_renderer).
- **Migrations log:** `0001_initial.sql` (Phase 1) → `0005_perspective_renderer.sql` (v0.5.0 / Phase 15.75 Slice A).

### Spec discipline

- `spec.md` §3.3 Process Topology rewritten to reflect the four-crate workspace + the architectural commitment that every non-GUI surface stays CLI-testable.
- `spec.md` §4.3 search expression language updated with the new operators (state predicates, sort modifier, fuzzy match) and §4.5 migrations log records 0004 + 0005.
- `roadmap.md` Phase 15.75 records partial progress (Slices A + B done; C/D/E pending). Phase 15.5 deferred-list moves to "closed" with the line items shipped at v0.4.x.
- `CLAUDE.md` codebase map shows the four-crate layout and includes atrium-cli's structure.

### Phase 15.75 carryover into v0.6.0

Three slices remain on Phase 15.75's plan:
- **Slice C — GTD audit fixes.** Weekly-Review seed Perspective on first-run; Logbook day-grouping headers (Today / Yesterday / Last 7 Days / Older); `docs/gtd-patterns.md` documenting the `#waiting` user-tag idiom.
- **Slice D — Board view.** Saved Perspectives gain a `renderer = 'board'` option that renders the filter expression as a kanban with tag-axis columns. The schema columns shipped at v0.5.0 (Slice A); UI is Slice D.
- **Slice E — Documentation polish.** Already partly subsumed by this v0.5.0 release notes entry; what remains is the fuller spec / roadmap / patchnotes pass that goes with the next minor.

### Other deferred to v0.6.x

- **SQL-translation evaluator** for the search engine. Translates the AST to a SQL `WHERE` clause when expressible; falls back to in-memory eval for regex / complex tag predicates. Pure perf optimization — the in-memory path handles 100K tasks within budget today.
- **FTS5 bm25 + recency ranking** on bare-text searches. Currently search returns matches unranked.
- **CLI bulk operations.** `atrium-cli complete --where 'is:overdue'` to bulk-complete matched tasks. The pieces are all in place; just needs a flag-driven dispatcher.
- **Regression-script integration.** `scripts/regression.sh` should exercise atrium-cli end-to-end against a fixture DB so the architectural commitment is automatically verified at every release.

## v0.4.0 (2026-05-07) — Phase 15.5: Calibre-Powered Search

The search bar's filter language grew from a flat key:value shape into a full expression grammar. Saved Perspectives inherit it for free since they store filter expressions verbatim. Full reference in `spec.md` §4.3.

Boolean composition with grouping (`AND` / `OR` / `NOT` / `!`, parens, `NOT > AND > OR` precedence). Calibre match modifiers on every text field (`tag:work` substring, `tag:=work` exact, `tag:~regex.*` regex, `tag:true` / `tag:false` existence). Comparison + range on date and numeric fields (`due:>today`, `due:2026-05-01..2026-05-31`, `estimated:>=30`). Date keywords (`today`, `thisweek`, `Ndaysago`, `Ndaysout`, etc.). State predicates as `is:NAME` shortcuts (`is:overdue`, `is:scheduled`, `is:repeating`, etc.). New field operators: `area:`, `project:`, `title:`, `note:`, `created:`, `modified:`, `completed:`, `estimated:`, `repeats:`.

Implementation: new `atrium-core/src/search/` module — lexer (Token stream), AST (Expr enum + supporting types with round-trip-shaped Display impls), recursive-descent parser, single-pass in-memory evaluator with lazy regex compilation cached per-query. `regex` crate added as a direct dependency (sign-off granted; already transitively present via tracing-subscriber).

Yellow `.warning` accent on the search entry when the parsed expression has unrecognised tokens; tooltip surfaces the typos. Three line items deferred to v0.4.x patches: SQL-translation evaluator, `↑/↓` history ring buffer, `?` operator-reference popover — all polish, not correctness.

## v0.3.0 (2026-05-07) — Visual polish pass

Tag colours wired end-to-end (six-swatch picker, sidebar dots, Pango-coloured pills via the existing `markup` property). Row hover states. Completion micro-animation (200 ms fade on toggle). Per-list empty-state warmth — distinct copy per canonical list instead of a generic "Nothing here." Sidebar section dividers. Header-bar `Area › Project` breadcrumb that updates as selection changes. Inspector-pane card treatment.

`prompt_for_tag` extends `adw::AlertDialog` with a custom extra-child Box for the swatch row — first non-trivial AlertDialog use beyond plain confirmations. Fully reactive: dragging the colour onto a tag instantly updates every visible pill via the existing `LibraryChanges` channel.

## v0.2.2 (2026-05-07) — Audit-pass bug fixes

Filter-typo toast warnings (when an unknown field token is parsed away to freeform text, surface a toast so the user knows). Sidebar zero-state hint ("Add an area or project to get started"). Screen-reader badge labels (count badges in the sidebar gain `accessible-description` attributes). Inbox chip fallback on tasks lacking an explicit context.

## v0.2.1 (2026-05-07) — Tag pill update fix + Area › Project chip

Fixed: editing a tag's colour did not propagate to already-rendered pills until the row was re-laid-out (Pango markup re-render gap). Each `LibraryChanges::tag` update now triggers a per-task pill rebuild keyed on the tag id. `Area › Project` row context chip surfaces parent context inline so the eye doesn't have to track the sidebar.

## v0.2.0 (2026-05-07) — Phase 15: Repeating Tasks (Builder Mode milestone)

Closes Phases 10–15 → Builder Mode shipped. Full RFC 5545 RRULE support via the `rrule` crate (sign-off granted before implementation). Three Org-mode completion semantics: `+1d` (regenerate from completion date), `++1d` (regenerate from scheduled date), `.+1d` (regenerate from a "now" sentinel — only the days/weeks shift). Migration `0003_repeat_mode.sql` — first ALTER post-v0.1 (the v0.1 schema freeze ends here; backwards-compatible migrations are now allowed per the schema discipline).

Inspector-pane repeat editor: dropdown → human label, RRULE preview shown live as the user adjusts. Worker regenerates the next occurrence on `ToggleComplete` for repeating tasks; user sees the new row pop in via `TaskChanges` without a refresh.

## v0.1.17 (2026-05-07) — Phase 14: Saved Perspectives

Saved searches as first-class sidebar entries. `Save Search as Perspective…` in the primary menu captures the current search-bar expression + view metadata into the new `perspective` table (migration `0002_perspectives.sql`, additive). Renaming and deleting via the sidebar context menu. Perspectives inherit the full search expression language (Phase 15.5 will retroactively give them grammar improvements without schema changes).

## v0.1.10 → v0.1.16 — Builder polish + interaction fixes

Phase 12 Forecast (30-day calendar-axis, drag-to-reschedule) shipped at v0.1.3. Phase 13 Review queue at v0.1.16. Builder Mode UI shell at v0.1.1; defer dates + sequential-project rendering at v0.1.2. The v0.1.4 → v0.1.9 run resolved Inspector-pane edge cases (synchronous mode flip, Builder Inspector chord, Inspector hide-on-Simple-flip, populate-on-mount). The v0.1.10 → v0.1.15 run was the **double-click hardening arc** — getting double-click to open the Inspector / start inline edit reliably across `GtkColumnView::activate`, gesture interception, and edit-start race conditions. The fix that stuck: listen to `GtkListView::activate` (not `pressed`), defer edit-start to idle, and gate on the gesture-stream timing.

## v0.1.0 (2026-05-07) — Simple Mode ships

Closes Phases 0–9. Six canonical lists (Inbox / Today / Upcoming / Anytime / Someday / Logbook), areas + projects + tags + multi-tag, Quick Entry (Ctrl+Alt+Space), FTS5 search + flat filter expressions, multi-select + undo, Inspector + tag editor dialogs, sidebar find-as-you-type, full keyboard map, typography + accessibility, debug-pane Memory Watch, ship-gate regression script.

Three Phase 9 follow-ups carry to v0.1.x: the actual `v0.1.0` git tag, Flatpak publish, public announcement. Two Phase 8 carryovers: README screenshots, Flatpak font-load verification.

## v0.0.30 → v0.0.38 — Pre-v0.1 polish + bugsweep

The pre-1.0 cleanup arc. Phase 8h silenced two startup/shutdown GTK warnings. Phase 9a built the regression gate (`scripts/regression.sh`: fmt + clippy + test + cold-start sanity). Phase 9b finalised the README. v0.0.33 → v0.0.36 closed the Phase 7 follow-up surface (per-task tag editor, Inspector dialog, layout pass, double-click reliability, stop-eating-spaces in entries). v0.0.37 was the dialog primitives bugsweep: standardised on `adw::Dialog` for in-window modals (Inspector, tag editor); `adw::Window` for non-grab observers (Quick Entry, Memory Watch); `adw::AlertDialog` for confirmations. v0.0.38 added the deadlines-approaching heads-up to Today.

## v0.0.23 → v0.0.29 — Phase 8 (typography, accessibility, perf, debug)

Bundled-font typography polish (Inter cv11/ss01 features, tabular figures audit on every numeric column). Atkinson Hyperlegible accessibility toggle (~80 KB SIL OFL, runtime-swappable). Packaging artefacts (desktop entry, AppStream metainfo, gschema XML, Flatpak manifest). Animation audit + Quick Entry fade-in keyframe. Memory Watch debug pane (`/proc/self/status` sampler, surfaces RSS + heap with a "drop caches" affordance). Accessibility audit (semantic roles, focus rings, screen-reader labels). Performance baseline against `spec.md` §8 budget — release build hits all four targets on Brandon's T14s.

## v0.0.17 → v0.0.22 — Phase 7 (search, undo, multi-select, sidebar, keymap)

FTS5-backed search (Phase 7a). Undo for toggle-complete + delete via a per-action undo stack; toast surfaces the affordance (Phase 7b). Multi-select + bulk operations — bulk complete / move / tag (Phase 7c). Filter expressions in the search bar — flat key:value shape that Phase 15.5 grew into the full grammar (Phase 7d). Find-as-you-type sidebar filter (Phase 7e). Full keyboard map — Ctrl+Z, F2 to rename, etc. (Phase 7f); written reference at `docs/keymap.md`.

## v0.0.14 → v0.0.16 — Phase 6 (tags + Quick Entry)

Tag CRUD + sidebar Tags section (Phase 6a). Tag pills + inline `#tag` / `@date` parser — typing `#work @today` in any task entry creates the tag if absent and applies the date (Phase 6b). Quick Entry modal — Ctrl+Alt+Space anywhere on the desktop drops a tiny `adw::Window` for capture without grabbing focus from the prior application; same parser; closes on Enter (Phase 6c).

## v0.0.10 → v0.0.13 — Phase 5 (areas, projects, sidebar hierarchy)

Sidebar hierarchy + remaining canonical lists (Phase 5a). Area / Project CRUD + the `LibraryChanges` delta channel paralleling `TaskChanges` for area/project mutations (Phase 5b). Count badges + drag-to-project (Phase 5c). Right-click context menus + sidebar selection refinement (Phase 5.5).

## v0.0.6 → v0.0.9 — Phases 2–4 (data layer, application shell, lists)

Single-writer worker + read-only pool (Phase 2): `Command` enum, `TaskChanges` delta, `WorkerHandle`, IO instrumentation via rusqlite's `trace` feature routing every SQL statement into a `tracing` span. Application shell (Phase 3): GTK4 + libadwaita window, sidebar shell, GSettings schema, font-install-on-first-run via fontconfig. Phase 4 brought Inbox + Today + the Calendar Month View item onto the roadmap. Phase 4.5 patched in drag-to-reorder + bottom-of-list entry.

## v0.0.3 → v0.0.5 — Phases 0 + 1 + roadmap horizon

Phase 0 (v0.0.3): Cargo workspace (`atrium` binary + `atrium-core` library), v0.1 dependency set locked, `--debug` skeleton, Meson wrapper, GitHub Actions CI. Phase 1 (v0.0.4): OmniFocus-superset schema in migration `0001_initial.sql` (every Builder column present from day one), FTS5 virtual table + sync triggers, `modified_at` triggers with `WHEN old = new` clauses, stress-fixture generator at four scales. v0.0.5 added the "Beyond 1.0" roadmap section (post-1.0 horizon for `atrium-tui`).

## v0.0.0 → v0.0.2 — Pre-implementation contract refinement

Spec, roadmap, README, LICENSE, VERSION, logo. Org vault as a projection — SQLite canonical, `.org` files downstream — formalised in `spec.md` §3.5 + the §7.3 round-trip rules. Debug-first architecture (`spec.md` §3.4) — `--debug` opens an in-app debug surface for stress generators, edge-case fixtures, IO instrumentation, memory watch — built into the binary, not bolted on. Release discipline written down: every minor or major change touches `spec.md`, `roadmap.md`, `patchnotes.md`, and `VERSION` together; every major bump includes a maintenance pass.
