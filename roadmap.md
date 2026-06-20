# Atrium — Roadmap

What's done, what's next, what's deferred. Sequenced for a clean Simple Mode v0.1, a Builder Mode v0.2 expansion, and a 1.0 with broad import/export across the Linux task-app ecosystem. Current release: **v0.40.1**. Phase 19.5 foundations (preferences dialog + system-notification reminders) shipped at v0.20.0; v0.21.x and v0.22.0 were maintenance releases (documentation sync, a clippy-cleanup pass, a metainfo XML fix, and splitting the two largest source files, `window.rs` and `inspector_pane.rs`, into module trees). v0.23.0 added subtasks UI exposure (the first Phase 19.5 productivity essential after the v0.20.0 foundations); v0.24.0 closed the Org property-drawer round-trip gap (custom `:KEY: value` entries stash into the new `task.extra_properties` JSON column and re-emit verbatim, reinforcing spec §7.3.3 rule 1 for property drawers). v0.25.0 opens Phase 19 with VTODO (RFC 5545) import + export — the CalDAV-side `.ics` bridge to Endeavour, Errands, Nextcloud Tasks, and Planify; the parser + emitter + mapper are hand-rolled stdlib (no `ical` crate), and UID round-trip rides the v0.24.0 `extra_properties` column. v0.26.0 and v0.27.0 closed the Phase 19 importer arc (Taskwarrior JSON + todo.txt). v0.28.0 opens the Post-v0.22.0 Tier 3 polish arc with per-area review schedules: a nullable `area.default_review_interval_days` (migration 0015) that the Review query falls back to via `COALESCE(project.review_interval_days, area.default_review_interval_days)`. v0.29.0 adds task dependencies (`blocked_by`): a `task_dependency` join table (migration 0016) powering `is:blocked` / `is:available`, a Builder Inspector "Blocked by" picker, a row pill, and `atrium-cli depend`. v0.30.0 begins the Tier 3 polish run: drag external files / URLs onto the window to capture (pre-filled Quick Entry). v0.31.0 adds first-run onboarding (a self-clearing `AdwStatusPage` for a pristine library); the inline-editing-on-row-edit item was found already shipped, so its slot was repurposed. v0.32.0 adds a backup / restore UI (`VACUUM INTO` snapshots, keep-10, optional weekly, restore-on-next-launch) plus `atrium-cli backup`. v0.33.0 adds task templates (migration 0017's `task_template` + `task_template_item`, `atrium-cli task-template`, GUI "New from Template…"). v0.34.0 closes the Tier 3 arc: the non-Org importers are extracted into a new `atrium-import` library crate and a unified GUI import dialog drives all five sources with a dry-run preview. v0.35.0 opens Phase 20 (the 1.0 endgame) with the accessibility round-2 pass (explicit accessible names on icon-only buttons across the Builder + Tier 2/3 surfaces; `atriumd` confirmed deferred post-1.0). v0.36.0 adds the `scripts/perf.sh` regression suite (50K / 100K fixtures, read-path load + peak RSS, headless §8-budget assertions). v0.37.0 adds the `mdbook` documentation site under `book/` (Guide on-ramps + Reference chapters that include the canonical `docs/*.md`); localisation + Flathub readiness are deferred to a sandbox session (their build/packaging verification needs Brandon's environment). v0.37.1 through v0.37.3 were documentation and test-suite maintenance: a README staleness + em-dash cleanup, the `atrium-cli` parser-test consolidation (1008 → 985 tests, no coverage loss), and a version/test-count header reconciliation across the doc set. v0.37.4 rewrote the README for professionalism. v0.38.0 adds a second kanban grouping axis (spec §4.6): status-axis boards group by Org TODO-sequence keyword and dragging a card changes real task state (keyword + completion) instead of rewriting synthetic tags, with a new `atrium-cli edit --keyword` flag keeping the board shell-driveable (no schema change; workspace 1005 unit tests). v0.38.1 → v0.38.3 were an audit-driven foundation pass (correctness fixes, perf tightening, accessibility). v0.39.0 opened the Tier D UX pass by consolidating the overlapping time-views: Agenda and Forecast merged into one sidebar entry with a Builder-only Bands/Strip layout toggle (Bands = chronological agenda, Strip = 30-day forecast), trimming the sidebar's "when" surfaces without losing capability. v0.39.1 → v0.40.0 continued the Tier D pass: interaction consistency (calendar week-strip rows open their task; date-sorted lists toast instead of swallowing a reorder drag), discoverability polish (kanban Configure button; Quick Entry `:key` hint; drag-gesture docs), and in-row quick reschedule (the task row's right-click Schedule submenu). The inline-syntax parser (`#tag`, `@today`, etc.) was small in v0.1 and grew steadily through Phase 6c (Quick Entry) + Phase 18 (Todoist mapper); v0.13.0 unifies the vocabulary across every capture surface, expands it (`!N` priority + `@<weekday>`), lifts the parser into its own `atrium-inline` workspace crate (atrium-core stays inline-syntax-agnostic), and wires a tab-completion popover into the bottom-of-list entry and Quick Entry modal so the syntax becomes discoverable. Phase 18 (Todoist CSV import) shipped at v0.12.0. Phase 17 (vault → DB two-way sync) closed at v0.10.3; Phase 12.5 (Calendar Month View) closed at v0.11.0. Phase 18.5 (Org-mode power features) and Phase 19.5 (productivity essentials) are next.

---

## North Star

Twenty phases mapping the journey from empty repo to 1.0.

- **Phases 0–9:** Simple Mode → **v0.1**
- **Phases 10–15:** Builder Mode → **v0.2**. Phase 12.5 adds a Calendar Month View alongside Forecast (same data, different lens).
- **Phases 16–19:** Org-mode + Todoist + plain-text + VTODO interop. Phase 16 (one-shot import + DB → vault writer) shipped at v0.8.0; Phase 17 closes the loop with `inotify`-driven vault → DB sync. Phase 18.5 mines Org-mode's interaction patterns (CLOCK time tracking, LOGBOOK drawer, custom `:PROPERTIES:`, habit grid, statistics cookies, deadline warning windows, active/inactive timestamps) for Builder Mode's Inspector pane — features neither Things nor OmniFocus expose.
- **Phase 20:** Polish, localisation, Flathub → **v1.0**

Each phase ends with a `heaptrack` checkpoint against the §8 budget. Every phase that adds a third-party crate calls it out — *no third-party deps without prior sign-off*.

The **debug harness** (spec §3.4 — `--debug` flag, stress generators, IO instrumentation, memory watch) lands as a skeleton in Phase 0 and grows alongside the features that need it: schema-aware fixtures in Phase 1, SQLite IO tracing in Phase 2, live RSS/heap surfacing in Phase 8. It is not a one-time deliverable.

---

## Post-v0.22.0 priority order

v0.22.0 closed the maintenance backlog (the `window.rs` / `inspector_pane.rs` splits). The remaining pre-1.0 work is ranked here by value-to-effort rather than phase order. Tiers 1 and 2 carry detailed todos; Tiers 3 and 4 cross-reference their phase sections below.

### Tier 1 — shipped

**Subtasks UI exposure** *(Phase 19.5; shipped v0.23.0).* `parent_id` had shipped since `0001_initial.sql` and the Org importer already built the tree; v0.23.0 exposes it in the GUI + CLI.

- [x] `atrium-core`: `list_subtasks(conn, parent_id)` read helper; `TaskUpdate.parent_id` + `reparent()` builder; worker cycle guard (`would_create_cycle`) + same-project rule; `DomainError::ParentCycle`.
- [x] `atrium-cli`: `add --parent ID` and `edit --parent ID | none` flags; `info` shows children in `--human`. (`list --tree` deferred; not needed for the slice.)
- [x] Task list: children render indented under parents (`nesting_order` + `apply_nesting`; `AtriumTask.depth` drives an 18 px/level row indent), reusing `position` within a parent.
- [x] Inspector pane (**Builder-only** per spec §5.1, not both modes): a "Subtasks" group above Notes with per-child checkboxes, click-to-navigate, and an "Add subtask" entry.
- [x] Drag-to-reparent: Shift+drop sets `parent_id` (plain drop still reorders); worker rejection surfaces a toast.
- [x] Completion semantics: no cascade (a parent does not auto-complete children); the `[done/total]` cookie (`count_done_total_per_parent`) shows progress.
- [x] Tests: `list_subtasks` + reparent + self-parent + descendant-cycle (core), `--parent` parse round-trips (cli), `nesting_order` depth/orphan/cycle (gui). Workspace 899.
- [x] Schema: none (already present).

**Custom property-drawer passthrough** *(correctness; shipped v0.24.0).* `documented_limit_org_importer_drops_custom_property_keys` flipped to `org_importer_round_trips_custom_property_keys`; spec §7.3.3 rule 1 now holds for property drawers as well as body content.

- [x] Schema: migration `0014_task_extra_properties.sql` adds `task.extra_properties TEXT NULL` (JSON object of unmodeled key to value); `user_version` 13 → 14.
- [x] `atrium-org`: the parser already collected every key into `OrgTask.properties`; the importer now partitions via `extras_from_properties` (sharing the new `MODELED_PROPERTY_KEYS` constant in `org/mod.rs`) and stashes the non-modeled keys.
- [x] `atrium-org` emitter: `task_to_org_task` merges `task.extra_properties` back into the local properties HashMap before emit. The existing alphabetical-sort emit pass handles ordering.
- [x] Worker / read: `NewTask.extra_properties` (BTreeMap) + `TaskUpdate.extra_properties_value` thread through `create_task` / `update_task`. JSON encode/decode at the boundary, mirroring the `default_tags` precedent. Empty map normalises to NULL on the column; read boundary normalises NULL or malformed back to empty BTreeMap.
- [x] Watcher diff path: `ParsedTask::to_new_task` and `diff_from` partition the parsed drawer; whole-map replace via `TaskUpdate::extra_properties_value`.
- [x] Tests: existing test flipped + multi-key stress fixture; four worker_tests covering CRUD; integration test exercising the watcher's external-edit path.
- [x] No UI (per the Phase 18.5 research note: lossless passthrough, not a surface).

### Tier 2 (high value, bigger lift)

**Phase 19: VTODO (RFC 5545) import + export** *(shipped v0.25.0).* The GNOME / CalDAV bridge — Endeavour, Errands, Nextcloud Tasks, Planify. Hand-rolled stdlib parser, mapper, emitter; UID round-trip rides the v0.24.0 `extra_properties` column.

- [x] Dependency sign-off: hand-roll wins. `ical` crate evaluated and declined for consistency with the Org + Todoist precedents (CLAUDE.md "Project tricks").
- [x] Importer (`atrium-cli import vtodo PATH --into PROJECT [--dry-run]`): SUMMARY → title, DESCRIPTION → note, DUE → deadline (date portion), DTSTART → `scheduled_for` + `scheduled_time`, COMPLETED → `completed_at`, STATUS → open/done plus `orig_keyword` for IN-PROCESS / CANCELLED, PRIORITY 1–4 → `priority-N` tag, CATEGORIES → tags via `ensure_tag`, RRULE → `repeat_rule` (verbatim), UID → `uuid` (v5-derived + stashed in `extra_properties["VTODO_UID"]` when not UUID-shaped).
- [x] Lossy report: 8 `LossyKind` variants covering UnsupportedComponent / DroppedAlarm / DroppedAttendee / DroppedGeo / DroppedPercentComplete / DroppedDuration / DroppedTimezone / UnknownProperty. Mirrors the Todoist mapper shape.
- [x] Exporter (`atrium-cli export vtodo PATH`): one-way `.ics` dump, one VCALENDAR + one VTODO per task. UTC timestamps, no VTIMEZONE. Atomic write via `atrium_core::sync::atomic::write_atomic`. Not a CalDAV client (spec §7.2).
- [x] Tests: four fixtures (basic / multi / lossy / nextcloud_sample), parser + emit unit tests covering line folding + escape encoding + round-trip, plus four DB-round-trip integration tests under `src/vtodo/round_trip_tests.rs` (modeled-subset round-trip, original UID preservation via extras, lossy report shape, multi-task status round-trip).
- [x] Follow-on Taskwarrior shipped v0.26.0 (`atrium-cli import taskwarrior` with `--uda-as tag|note|drop`). todo.txt shipped v0.27.0 (`atrium-cli import todotxt`). Unified import dialog shipped v0.34.0 (with the `atrium-import` crate extraction that made the importers GUI-reachable).

**Task dependencies (`blocked_by`)** *(Phase 19.5; Taskwarrior-parity; shipped v0.29.0).*

- [x] Schema: migration `0016_task_dependency.sql`: `task_dependency(task_id, blocked_by_id)` with FK CASCADE both ends + `UNIQUE`; `user_version` 15 → 16.
- [x] Worker: `add_dependency` / `remove_dependency` commands; `would_create_dependency_cycle` guard (rejects self + cycles); CASCADE on task delete; `emit_task_refresh` so the row repaints.
- [x] Read / eval: `is:available` = open AND not blocked (dependency-only — defer stays `is:deferred`); new `is:blocked` predicate; both translate to an EXISTS / NOT EXISTS SQL fast-path with an in-memory `blocked_ids` fallback. New `read::blocked_task_ids` + `list_prerequisites`.
- [x] Row treatment: amber "Blocked" pill + `.blocked` row class; live recompute across the store after every diff so completing a prerequisite unblocks dependents in the same frame.
- [x] Inspector (Builder): a "Blocked by" group with per-prerequisite rows (navigate + remove) and a search-as-you-type "Add" picker.
- [x] `atrium-cli`: `depend ID --on ID` / `--remove`; `info --human` shows a "Blocked by" section.
- [x] Tests: self / direct / transitive cycle rejection, dup no-op, CASCADE, `blocked_task_ids` + `list_prerequisites`, `is:available` / `is:blocked` eval + SQL parity. Workspace 991 unit tests.

### Tier 3 (pre-1.0 polish; lives in Phase 19.5)

See the Phase 19.5 section. Recommended order: first-run / onboarding, then backup-restore UI, then drag external files / URLs, then inline editing on row edit. All small to medium, no new deps, high perceived-quality payoff. EDS calendar overlay and task templates also sit in Phase 19.5 but rank lower (EDS needs a `libecal` / `zbus` sign-off; templates are nice-to-have).

### Tier 4 (the 1.0 endgame; Phase 20)

See the Phase 20 section: `atriumd` capture daemon (also closes the Phase 6c zero-launch carryover), localisation scaffolding, the `mdbook` docs site, AppStream screenshots, Flathub submission, the 50K-task perf regression suite, accessibility round 2. Hold until Tiers 1 to 3 make the app feature-complete.

### Quick wins (grab anytime)

- [x] Metainfo `appstreamcli` capitalisation infos cleared (v0.22.x maintenance).
- [ ] README screenshots (Simple + Builder): needs a manual capture pass (Phase 9 / Phase 20 carryover).
- [ ] Flatpak font verification under the sandbox: needs a `flatpak-builder` run (Phase 8 carryover).
- [x] Per-area review schedules *(shipped v0.28.0)*: additive `area.default_review_interval_days` migration (0015) + `COALESCE(project.review_interval_days, area.default_review_interval_days)` in the review query + an Edit Area review-interval row (Phase 13 deferred; small feature, minor bump).

---

## Shipped (Phases 0 → 16, plus v0.9.0 housekeeping)

The completed phases, condensed. Open carryover items are listed at the end of this section under *Deferred but still on the table*.

**v0.9.0 — `atrium-org` crate extraction.** Phase 16's Org projection (parser, emitter, importer, vault writer task) moves out of `atrium-core::sync` into a new `atrium-org` workspace crate. atrium-core gains a `VaultDirtyNotifier` trait so it stays Org-agnostic; atrium-cli + the GTK binary pick up `atrium-org` directly. Pre-Phase-17 housekeeping; no behaviour change. Workspace is now five crates.

**Phases 0–9 → Simple Mode (v0.1.0).**

- **Phase 0 — Scaffolding** (v0.0.3). Cargo workspace (`atrium` binary + `atrium-core` library), v0.1 dependency set locked, `--debug` skeleton, Meson wrapper, GitHub Actions CI.
- **Phase 1 — Schema** (v0.0.4). OmniFocus-superset schema in `0001_initial.sql`. FTS5 virtual table + triggers. `modified_at` triggers with `WHEN old=new` clauses. Stress-fixture generator at four scales.
- **Phase 2 — Data layer** (v0.0.6). Single-writer worker + read-only pool. `Command` enum, `TaskChanges` delta, IO instrumentation via rusqlite's `trace` feature.
- **Phase 3 — Application shell** (v0.0.7). GTK4 + libadwaita window, sidebar, GSettings schema, font-install-on-first-run via fontconfig, About dialog.
- **Phase 4 — Inbox + Today** (v0.0.8 → v0.0.9). Six canonical lists wired. Inline create + edit + completion. Drag-to-reorder.
- **Phase 5 — Areas + Projects + remaining lists** (v0.0.10 → v0.0.13). Sidebar hierarchy, Area / Project CRUD, `LibraryChanges` delta, count badges, drag-to-project.
- **Phase 6 — Tags + Quick Entry** (v0.0.14 → v0.0.16). Tag CRUD + sidebar section. Inline `#tag` / `@date` parser. Quick Entry modal (`Ctrl+Alt+Space`) sharing the parser.
- **Phase 7 — Search + keyboard map** (v0.0.17 → v0.0.22, plus Phase 7g/7i in v0.0.33–v0.0.36). FTS5 search, undo, multi-select, find-as-you-type sidebar, full keyboard map, per-task tag editor + Inspector dialog.
- **Phase 8 — Polish + packaging** (v0.0.23 → v0.0.30). Typography (Inter cv11/ss01, tabular figures, Atkinson Hyperlegible). AppStream metainfo, `.desktop`, Flatpak manifest. Animations. Memory Watch debug pane. Accessibility audit.
- **Phase 9 — v0.1.0 release** (v0.1.0). Regression gate (`scripts/regression.sh`), README finalisation, milestone tag.

**Phases 10–15 → Builder Mode (v0.2.0).**

- **Phase 10 — UI shell** (v0.1.1). Mode toggle, Inspector pane (always-visible right sidebar in Builder), project Sequential / Review extras. Zero schema impact — Phase 10's acceptance test is `mode_flip_snapshot.rs`.
- **Phase 11 — Defer dates + sequential rendering** (v0.1.2). `defer_until` editor; queued-row CSS for sequential projects; `available_count` math.
- **Phase 12 — Forecast view** (v0.1.3). 30-day calendar-axis page; drag-to-reschedule; today indicator + overdue surfacing; click-to-open (v0.6.17).
- **Phase 13 — Review queue** (v0.1.16). `list_review_queue` SQL; per-project Mark Reviewed. Task-level Mark Reviewed via migration 0006 + 7-day exclusion landed at v0.7.4.
- **Phase 14 — Saved Perspectives** (v0.1.17). Migration `0002_perspectives.sql`. *Save Search as Perspective…* + sidebar section. Full editor dialog landed at v0.7.3.
- **Phase 15 — Repeating tasks** (v0.2.0). RFC 5545 RRULE via the `rrule` crate (sign-off granted). Three Org-mode completion semantics (`+1d` / `++1d` / `.+1d`) via migration `0003_repeat_mode.sql` — first ALTER post-v0.1, ending the schema freeze.

**Phase 15.5 → Calibre-Powered Search (v0.4.0).** Boolean expression grammar (`AND` / `OR` / `NOT`, parens, `NOT > AND > OR` precedence). Calibre match modifiers on every text field (`tag:x` substring, `tag:=x` exact, `tag:~regex`, `tag:true` / `tag:false`). Comparison + range on date/numeric fields. Date keywords. State predicates as `is:NAME`. New field operators (`area:` / `project:` / `title:` / `note:` / `created:` / `modified:` / `completed:` / `estimated:` / `repeats:`). `regex` crate added (sign-off granted).

**Phase 15.75 → Polish + extraction + Slice D (v0.5.0 → v0.6.5).**

- **Slice A — schema foundation** (v0.5.0). Migrations `0004_area_color.sql` + `0005_perspective_renderer.sql`. `user_version` 3 → 5.
- **Slice B — visual rhythm + per-area accent** (v0.5.0). Hover-row lift; sidebar letter-spacing; `.atrium-note-body` italic + 1.6 line-height; AdwClamp 720 px (later 960 at v0.6.11). Per-area accent (six-swatch picker, sidebar dots, 3 px row stripe). Canonical-list icon tinting.
- **v0.4.x search engine evolution** (closed at v0.5.0). Five canonical-list state predicates, `sort:KEY` / `-KEY`, ↑/↓ history (20-entry ring buffer), `?` operator-reference popover, fuzzy `tag:?word`.
- **`atrium-search` crate** (v0.4.2). Lifted out of `atrium-core`. Same code, same tests; no SQLite/worker drag.
- **`atrium-cli` crate** (v0.4.3 → v0.6.5). Headless binary with full task CRUD + perspective write side. TSV / JSON / human output. Reads via `SQLITE_OPEN_READ_ONLY`; writes spin up the worker on a current-thread tokio runtime. Quick Entry parser lifted to `atrium_core::quick_entry` so the shell shares the GUI's grammar exactly. Bulk operations (`atrium-cli complete --where 'is:overdue'`); delete defaults to dry-run for safety.
- **Regression-script integration** (v0.5.x). `scripts/regression.sh` exercises atrium-cli end-to-end against `--fixture small`.
- **Slice C — GTD audit** (v0.5.0 → v0.6.0). Weekly Review Perspective seed (later absorbed into the canonical Review page at v0.7.2). Logbook day-grouping (Today / Yesterday / Last 7 Days / Older). `docs/gtd-patterns.md`.
- **Slice D — kanban + Agenda** (v0.5.4 → v0.6.5). `atrium-core::render` typed `Renderer { List | Board(BoardConfig) }` enum + `move_to_column` helper. Saved Perspectives with `renderer = "board"` render as kanban with drag-drop column moves. Agenda canonical page (Overdue / Today / Tomorrow / This Week / Next Week) with the Overdue heading rendered in `@warning_color`.
- **FTS5 bm25 + recency ranking** (v0.5.2). Bare-text search ranks via FTS5 `bm25` blended with a 30-day half-life recency factor.
- **SQL-translation evaluator** (v0.5.3). `atrium-search::sql_translate::try_translate` converts an `Expr` to a SQL `WHERE` fragment + bound params; in-memory fallback for regex / fuzzy / composite.
- **Sidebar reorganisation** (v0.6.7). Agenda / Forecast / Review join the top tier alongside Inbox / Today / etc. Logbook bookends the top tier (v0.6.16).
- **v0.6.x screenshot-driven cleanup** (v0.6.10 → v0.6.16). Soft-accent pass; state-aware row treatment (overdue red / today amber / upcoming accent on checkbox + date pills); Inspector Notes placeholder; visible row separators + recurrence icon for `repeat_rule != NULL`; fixture colours; AdwClamp expansion.
- **v0.6.19 roadmap revision** (v0.6.19). Things 3 import retired (macOS-only); Org-mode promoted to Phase 16/17 as the must-ship two-way mirror; Todoist promoted to its own Phase 18; new Phase 19.5 covers nine productivity essentials surfaced by the gap-analysis pass.

**v0.7.x → visual fusion + Phase 16 build-out (v0.7.0 → v0.7.18).**

- **Visual-fusion polish** (v0.7.0 / v0.7.1 / v0.7.5). Surface continuity, whitespace pass, refinement.
- **Review absorbs Weekly Review** (v0.7.2). The canonical Review page renders two sections — *Projects to review* + *This week* — and the standalone Weekly Review Perspective seed retired.
- **Inspector check-off + perspective editor dialog** (v0.7.3).
- **Task-level Mark Reviewed** (v0.7.4). Migration 0006 + per-row button on the weekly walk + 7-day exclusion.
- **Phase 16 — Org-mode import + DB → vault writer** (v0.7.6 → v0.7.18, eleven patches; stamped at v0.8.0). See the *Phase 16* section below for the round-tripped roadmap bullets and the version each landed in.

**v0.8.0 — Phase 16 stamp + maintenance pass.** Worker test split (`worker_tests.rs`). Dead-code prune in the Org writer. Comment audit (74 → 26 surviving v0.7.X markers).

### Deferred but still on the table

These items belong to shipped phases but didn't land — listed here so they don't slip off the radar:

- **Phase 6c — Quick Entry cold-start** (post-1.0 / `atriumd`). The in-app shortcut only fires while Atrium has focus; true zero-launch capture is a daemon problem, deferred out of the 1.0 endgame.
- **Phase 9 follow-ups**: the actual `v0.1.0` git tag, the Flatpak publish, the public announcement on `VirInvictus.github.io`. Two Phase 8 carryovers also outstanding (README screenshots, Flatpak font verification under sandbox).
- **Phase 12 — Compact / expanded Forecast cards**. Per-card state model needed; folded into Phase 12.5 / Phase 20 polish.
- **Phase 13 — Per-area review schedules**. `area.default_review_interval_days` additive migration would unlock it; quality-of-life on top of the per-project SpinButton.
- **Phase 14 — Export perspective definition** to JSON. Subsumed by Phase 16's `atrium-cli export json` (the snapshot includes perspectives).

---

## Phase 12.5: Builder Mode — Calendar Month View — **shipped at v0.11.0**
*The other side of Forecast — a familiar month grid for users who think in calendar pages.*

The earlier framing called this subsumed by the Agenda canonical page, but Agenda's chronological-band layout (Overdue / Today / Tomorrow / This Week / Next Week) and Forecast's 30-day strip are both linear — neither gives the paper-calendar lens users coming from `cal`, GNOME Calendar, Apple Calendar, etc. expect. Calendar Month View is a third lens over the same data: paper-calendar grid + month nav + drag-to-reschedule + peek/drill clicks + narrow-window collapse. Builder-only canonical page sitting between Forecast and Review.

- [x] **Month-grid widget** (v0.11.0): `GtkGrid` 7 columns × 5–6 weeks via `atrium/src/ui/calendar.rs::build_month_grid` + `build_grid`. Mon-start ISO weeks; out-of-month leading / trailing cells flagged so they render muted.
- [x] **Per-day task rendering** (v0.11.0): count badge in cell header; up to 3 task titles inline; "+N more" overflow `MenuButton` with a popover that opens each task in the inspector.
- [x] **Today indicator + month/year header** (v0.11.0): today's cell tagged with `atrium-calendar-cell-today` for accent painting; magazine-spread page subtitle binds "<Month> <Year>" so the title strip tracks navigation.
- [x] **Month nav** (v0.11.0): Prev / Today / Next buttons + month/year `MenuButton` opening a 4×3 picker. `Page_Up` / `Page_Down` via a local-scope `gtk::ShortcutController`. `Ctrl+Shift+M` opens the page (`app.show-list::calendar` action; mode-gated to no-op in Simple).
- [x] **Drag-to-reschedule between days** (v0.11.0): each task title is a `gtk::DragSource` carrying the task id; each cell is a `gtk::DropTarget` accepting `i64` and updating `scheduled_for` via the worker. Out-of-month cells accept drops too. Shift-modifier for deadline-vs-schedule deferred per spec.
- [x] **Click-day-to-filter** (v0.11.0): single-click opens a peek popover with the day's full task list (each task is a flat button that opens the inspector); double-click drills into a date-scoped search via `scheduled:YYYY-MM-DD` so the user gets the standard list view's editing affordances.
- [x] **Narrow-window collapse** (v0.11.0): below the 600 px `COMPACT_WIDTH_THRESHOLD`, the grid swaps for a vertical week strip — 7 day cards stacked vertically, focused on the week containing today. Window watches `notify::default-width` and rebuilds on threshold flips (cached compact-mode flag prevents rebuild storms during drag-resize).
- [x] **Builder-only sidebar entry** (v0.11.0): `top_tier_extras(builder=true)` produces 5 entries (Agenda, Forecast, Calendar, Review, Logbook); Calendar sits between Forecast and Review.
- [x] **Tests** (v0.11.0): 13 lib tests in `atrium/src/ui/calendar.rs::tests` cover date math (month boundaries, leap February, DST transitions), week-row counts (5 vs 6), year-wrap on prev/next, today-cell marking, out-of-month flagging, completed + deadline-only task exclusion (the calendar uses the When-axis only; deadline-only surfaces in Forecast / Agenda).

---
## Phase 16: Org-Mode Import + Two-Way Vault Sync — Atrium ↔ Emacs Parity (was 17 + 17.5) — **shipped at v0.8.0**
*Brandon's primary-direction interop target. Atrium's vault is fully compatible with Emacs / Doom / vim-orgmode out of the box: open the same `~/Tasks/` directory in `org-agenda` and the result should look like Atrium's Agenda canonical page. v0.6.19 elevated this from a deferred two-stage plan (read-only first, then bidirectional) to a single must-ship goal.*

The contract: a user can run Atrium and Emacs side-by-side against the same vault, edit tasks in either, and the other reflects the change without manual reconciliation. Org's `:ID:` property is the round-trip anchor; SCHEDULED / DEADLINE / CLOSED cookies map to `scheduled_for` / `deadline` / `completed_at`; headline tags map to Atrium tags; `:PROPERTIES:` drawers carry per-task metadata that doesn't have a native Org cookie. Conflict handling is explicit (loser preserved at `<file>.atrium.bak.<timestamp>`), never silent.

The full Phase 16 surface landed across the eleven-patch v0.7.6 → v0.7.18 arc; v0.8.0 stamps it complete. Phase 17 (vault → DB `inotify` driver) is what's next — Atrium can now write the vault, but reads-back-from-vault is still gated on Phase 17.

- [x] **Org parser/emitter research:** evaluated `orgize` and `starsector` (both dormant) against a hand-rolled subset and chose hand-rolled — no new crates needed. (v0.7.6 → v0.7.7)
- [x] **Vault discovery + GSettings:** `vault-path` key, default empty (no-vault is valid); GTK binary reads on boot and routes through `spawn_worker_with_vault` when set. (v0.7.6 + v0.7.18)
- [x] **One-shot importer (`atrium-core/src/sync/org/import.rs`):** `atrium-cli import org PATH [--dry-run]` covers TODO/DONE/CANCELLED keywords, SCHEDULED/DEADLINE/CLOSED cookies, headline tags, `:PROPERTIES:` drawers, body text, nested subtasks; multi-file vault walk added in v0.7.14 with `<vault>/<area>/<project>.org` mapping subdirectories onto Atrium areas. (v0.7.9 + v0.7.14)
- [x] **Writer (`atrium-core/src/sync/org/write.rs`):** `atrium-cli export org PATH` emits `<vault>/<Area>/<Project>.org` per spec §7.3 — `#+TITLE:`, top-level + per-task `:PROPERTIES:` drawers, SCHEDULED/DEADLINE/CLOSED cookies, headline tags, full field mapping. Output reads cleanly in stock `org-agenda`. (v0.7.10 + v0.7.13)
- [x] **`:ID:` allocation:** importer preserves source `:ID:` (`NewTask.uuid` additive field); writer emits `:ID:` for every task/project. New rows get UUIDv4 from the existing schema default.
- [x] **Atomic file writes:** `write-temp + fsync + rename` helper added in v0.7.6; every `emit_org_file` call routes through it.
- [x] **Sidecar (`<vault>/.atrium/config.toml`):** tag colours land at v0.10.1 — hand-rolled minimal TOML in `atrium-org/src/sidecar.rs`, refreshed by the writer at the end of each flush burst that touches tag state. Mode + perspective slots are reserved (the file always emits the section headers) but not yet written; mode lives in GSettings (only the GTK binary sees it), and perspectives need a paired importer.
- [x] **Worker write hook:** `WorkerHandle::spawn_with_vault(conn, VaultConfig { root, read_pool })` spawns a `VaultWriter` task that receives `ProjectDirty(project_id)` notifications from every Task / Project / Tag write the worker dispatches, debounced ~100ms (50ms tick), and rewrites the project's `.org` via the v0.7.10 writer. (v0.7.16)
- [x] **Post-write integrity check:** every `emit_org_file_with_meta` re-reads the file and verifies it parses cleanly through Atrium's own reader; failure propagates as `io::Error`. (v0.7.15)
- [x] **Atrium native JSON export:** `atrium-cli export json PATH` writes the entire DB state (areas / projects / headings / tasks / tags / task_tags / perspectives) into a versioned snapshot via `atrium-core::sync::json::Snapshot`. (v0.7.11)
- [x] **Round-trip test fixture:** five complicated `.org` files round-tripped through importer + writer + parser, asserting AST equality between source and regenerated trees. Surfaced + fixed two real importer gaps (CLOSED cookie preservation via `NewTask.completed_at`, CANCELLED keyword preservation via `task.orig_keyword`). (v0.7.17)
- [x] **Custom-keyword round-trip:** migration 0007's `task.orig_keyword` column stashes non-canonical TODO keywords (WAITING, BLOCKED, IN-PROGRESS, CANCELLED) so headlines round-trip without losing their label. (v0.7.12 + v0.7.17)
- [x] **Multi-file vault walk:** `WorkerHandle::ensure_area` idempotent-create-by-name helper backs the `<vault>/<area>/<project>.org` mapping. (v0.7.14)
- [x] **GUI vault integration:** GTK binary reads `vault-path` GSettings key on boot and, when non-empty, calls `spawn_worker_with_vault` so every DB write auto-flushes to the vault. (v0.7.18)

## Phase 17: Two-Way Org Sync — Vault → DB (was 17.5) — **closed at v0.10.3**
*Emacs / Doom / vim-orgmode edits flow back. Atrium's Agenda view and Emacs's `org-agenda` buffer both read the same source of truth; whichever you edit, the other catches up.*

**RRULE canonicalisation contract** (lifted into Phase 17 because Phase 18's Todoist importer surfaces the same shape — see Phase 18). Atrium's `task.repeat_rule` is full RFC 5545 RRULE (via the `rrule` crate, sign-off granted Phase 15). Org's native repeater syntax (`+1w`, `++1w`, `.+1w`) only encodes interval — it can't represent multi-weekday patterns like `BYDAY=MO,WE` or month-day-of-month patterns like `BYMONTHDAY=1`. The vault writer therefore emits **both** representations on every repeating task:

1. A best-fit Org repeater on the SCHEDULED cookie so stock `org-agenda` surfaces a sensible repeat. Single-weekday patterns (`BYDAY=SU`) are lossless: SCHEDULED on a Sunday + `+1w`. Multi-weekday or unusual patterns degrade to "nearest interval" — `org-agenda` shows the wrong frequency, but the task isn't broken.
2. The full canonical RRULE in the task's `:PROPERTIES:` drawer (`:RRULE: FREQ=WEEKLY;BYDAY=MO,WE`). Stock `org-agenda` ignores it; Atrium re-parses it on read.

The contract: **`:RRULE:` is canonical. Org cookie is best-fit projection.** When the user edits the SCHEDULED cookie in Emacs, divergence detection (see below) flags it; DB keeps the `:RRULE:` value.

- [x] **`inotify` watcher** (v0.10.0): `notify` v8 backend; vault root + subdirectories; events debounced 200 ms keyed on file path (last-deadline-wins).
- [x] **Self-write filter** (v0.10.0): writer records `(path, mtime_just_written)` into a shared `RecentWrites` set; watcher matches inotify events by exact tuple equality. mtime-based (not path-only TTL) so external edits within the TTL window aren't swallowed — the integration tests immediately surfaced the failure mode of the path-only design.
- [x] **Reader → DB diff** (v0.10.0): `vault_watcher::diff_and_apply` matches parsed tasks to DB tasks by `:ID:`; CREATE / UPDATE / DELETE submitted through `WorkerHandle`. Field coverage: title, schedule, deadline, completed_at, tag set. Subtasks via `parent_id` from the parsed tree.
- [x] **`:ID:` allocation on read** (v0.10.0): headlines parsed without `:ID:` get a freshly-minted UUIDv4 in `vault_watcher::flatten_with_uuids`; the worker's auto `notify_project_dirty` after the create triggers the writer to rewrite the file with the now-stable property. Self-write filter swallows the resulting inotify echo.
- [x] **Conflict detection** (v0.10.1): the writer stats the destination file before each atomic-overwrite; if the file's mtime isn't in `RecentWrites` (an external editor touched it since Atrium's last self-write), the current contents snapshot to `<file>.atrium.bak.<UTC-timestamp>` first. `VaultEvent::ConflictBackup` flows back to the GUI for toast surfacing.
- [x] **GUI wiring** (v0.10.1): new `atrium_org::spawn_vault_loop` builder replaces the broken `spawn_org_vault_with_watcher`. `boot_data_layer` passes the resulting `VaultConfig` into `spawn_worker_with_vault`, then feeds the worker handle back through `VaultLoopHandle::attach_watcher` to finish the wiring. Events bridge to `AtriumWindow::show_toast`. `atrium-cli` stays write-only.
- [x] **Malformed-file handling** (v0.10.2): parse failure pauses sync for that file via `paused: HashSet<PathBuf>`; `ParseFailed` event fires once on the transition, `ParseRecovered` fires once on recovery. Repeated bad saves stay quiet.
- [x] **Custom-keyword + unknown-construct preservation** (v0.10.2): verbatim round-trip per spec §7.3.3 rule 1. Two v0.10.0 bugs fixed — watcher's create path dropped `OrgKeyword::Custom`, and `TaskUpdate` had no `orig_keyword` field so external keyword changes on existing rows didn't sync. New `TaskUpdate.orig_keyword` + builder; the watcher's create + diff paths route through a shared helper. File removal handled separately: `VaultEvent::FileRemoved` retains tasks (spec §3.5: DB canonical) and surfaces a toast; the next project flush recreates the file.
- [x] **RRULE canonicalisation on emit** (v0.10.3): writer emits both the best-fit Org cookie and the full `:RRULE:` property. New `atrium_org::rrule_to_org_cookie` / `rrule_to_org_repeater` helpers in `atrium-org/src/rrule_cookie.rs`. `scheduled_repeater_from_task` (the v0.7.10 None-returning placeholder) now flips on. Three migration cases tested via the `rrule_patterns.org` fixture round-trip: weekly single-day (BYDAY=SU), weekly multi-day (BYDAY=MO,WE), monthly day-of-month (BYMONTHDAY=1).
- [x] **RRULE divergence detection** (v0.10.3): `cookie_matches_rrule` helper compares the cookie's implied RRULE against the stored `:RRULE:` on the FREQ + INTERVAL axis (BY-clauses don't count as divergence — the cookie can't express them by design). When the cookie disagrees, the watcher surfaces `VaultEvent::RruleDiverged` and synchronously rewrites the file via `write_project_to_vault` so the cookie matches canonical. DB stays canonical; user's Emacs cookie edit gets reverted; toast surfaces the diff.
- [x] **Agenda parity acceptance test** (v0.10.3): `agenda_parity_with_reference_org_agenda` in `atrium/src/ui/agenda.rs` synthesises a vault with tasks across every bucket plus the "shouldn't appear" edge cases (completed / deferred-future / no-anchor / Someday / beyond-next-week) and asserts Atrium's `classify` agrees with a spec-derived reference org-agenda classifier on every task. Visual style differs; semantic groupings agree.
- [x] **Test scenarios** (closed v0.10.3): synthesized concurrent edit (`concurrent_atrium_and_external_edit_preserves_user_content_as_bak`), malformed-file recovery (`malformed_file_pauses_then_recovers`), large-file 1K-task parse latency (`large_file_parses_under_budget`), multi-day RRULE round-trip (`fixture_rrule_patterns`).

## Phase 18: Todoist Import (was bundled into Phase 19)
*The cross-platform productivity app most likely-to-migrate Linux user is leaving behind. Web client + Linux Electron app + paid sync; users have a real export path. Anchored to a real artifact: `Home.csv` from Brandon's Downloads — Rin's chore-tracker — is the gold-standard fixture this phase must round-trip cleanly.*

The `Home.csv` shape pins the format to test against:

- **`TYPE` column gates row class** — `meta`, `section`, `task`, or empty (separator). Empty rows are noise; skip.
- **`meta` rows carry project-level UI hints** — `view_style=board` (we map to a Perspective with `renderer="board"`), `view_style=list` (default; ignore).
- **`section` rows become Atrium headings** within the project.
- **`INDENT` is the subtask depth** — 1 = top-level under section, 2 = subtask of preceding indent-1, etc. Maps to `parent_id`. The fixture goes 4 deep.
- **Inline `@tag` syntax in `CONTENT`** — Todoist's labels appear inside the title (`Check for essentials @chore @home`). Strip from title; map each `@x` to a tag relation.
- **`PRIORITY` 1–4** — Todoist 4 = highest. Map to a `priority-N` tag, or to the numeric-priority column if 19.5's lands first.
- **`DATE` is natural language** — needs forgiving parsing: `Every Sunday at 10am`, `every 3 day at 9am` (typo: "day" not "days"), `every 3 month` (singular), `Every 1stday` (no space), `3 days ago at 15:00`. Map to `scheduled_for` + `repeat_rule` (RRULE conversion). Lossy or unparseable → preserve raw string in the task note + lossy-fields report.
- **`DEADLINE`** is a separate column from DATE — distinct from `scheduled_for`; maps to `deadline`.
- **`AUTHOR` / `RESPONSIBLE`** are user IDs — drop (single-user app); surface in lossy-fields report if non-empty.

- [x] **Format research:** done — Todoist CSV column set documented above. JSON via their API also available; CSV is the canonical input (no auth required), API path documented as alternative for power users with tokens.
- [x] **Importer module:** `atrium-cli/src/import/todoist/{parser,recurrence,mapper}.rs` — parser, mapper, dry-run mode. Mirrors the `import org` ergonomics — `atrium-cli import todoist PATH --into PROJECT_NAME [--dry-run]`.
- [x] **Mapping:** Projects → projects, `section` rows → headings, `task` rows → tasks, `INDENT` → `parent_id` chain, inline `@labels` → tags, `PRIORITY` (1-3) → `priority-N` tag (4 is Todoist's default; emits no tag), `DATE` → `scheduled_for` + `repeat_rule`, `DEADLINE` → lossy-report (no Atrium-side home for separate deadline-from-schedule on a Todoist row in v0.12), `DESCRIPTION` → task note, `meta view_style=board` recorded in `meta_entries` (board-Perspective auto-creation deferred — landing in 19.5).
- [x] **Natural-language recurrence parser:** dedicated module that handles Todoist's loose phrasing. Output is RFC 5545 RRULE per the canonicalisation contract documented in Phase 17 — `task.repeat_rule` is canonical; the Org vault projection emits a best-fit cookie + the full `:RRULE:` drawer property on write. Concrete mappings the parser must handle (driven by the `Home.csv` fixture):
  - `Every Sunday at 10am` → `FREQ=WEEKLY;BYDAY=SU` + `scheduled_for` time `10:00`. Org cookie: `+1w`, scheduled on a Sunday.
  - `Every Monday and Wednesday` → `FREQ=WEEKLY;BYDAY=MO,WE`. Org cookie: best-fit `+1w` from one of the days; canonical lives in `:RRULE:`. (Org-agenda shows wrong frequency; `:RRULE:` keeps Atrium correct.)
  - `every 3 day at 9am` (typo: "day" not "days") → `FREQ=DAILY;INTERVAL=3` + scheduled time `09:00`. Org cookie: `+3d`.
  - `every 3 month` (singular) → `FREQ=MONTHLY;INTERVAL=3`. Org cookie: `+3m`.
  - `every month` → `FREQ=MONTHLY`. Org cookie: `+1m`.
  - `Every 1stday` / `every 1st day` → `FREQ=MONTHLY;BYMONTHDAY=1`. Org cookie: best-fit `+1m`; canonical in `:RRULE:`.
  - `every 3 weeks` → `FREQ=WEEKLY;INTERVAL=3`. Org cookie: `+3w`.
  - `Every day at 9pm` → `FREQ=DAILY` + scheduled time `21:00`. Org cookie: `+1d`.
  - `3 days ago at 15:00` → `scheduled_for` only (`now() - 3 days`); no `repeat_rule`. Past-dated single occurrence.

  Failures preserved verbatim in the note + flagged in the post-import report (`unparseable recurrence: "<raw string>"`). The acceptance test asserts every `Home.csv` row parses non-lossily or shows up in the report.
- [x] **Conflict handling:** v5 UUID derived from `(project_name, label-stripped title)` under a frozen Todoist namespace. Re-imports onto the same project produce stable IDs; the Org-vault `:ID:` round-trip is invariant across re-runs. Atrium's existing `task.uuid UNIQUE` constraint is what surfaces conflict detection — repeat imports without `--into` change error out cleanly. Full UPDATE-on-match is deferred to a follow-up patch on top of v0.12.0.
- [x] **Post-import report:** counts (sections / tasks / tags created) + per-row lossy fields. The `LossyKind` enum covers `UnparseableRecurrence`, `DroppedTimeOfDay`, `DroppedTimezone`, `DroppedDuration`, `DroppedDeadline`. AUTHOR/RESPONSIBLE drops surfaced via `meta_entries` rather than per-row entries (single-user app — they're noise per row, useful as a one-shot annotation).
- [x] **Test fixtures:** sanitised `Home.csv` lands at `atrium-cli/tests/fixtures/todoist/home.csv` — author IDs scrubbed (`User`), content kept verbatim. The acceptance test (`home_csv_round_trips_through_db_and_vault`) drives the full Todoist → DB → vault → re-parse loop and asserts: 10 sections + 46 tasks land, 2 distinct tags survive, every section emits as a depth-1 keyword-less headline, the "Check for essentials" parent task carries 7 nested children at depth 3, embedded commas in task titles round-trip cleanly, recurring tasks carry `:RRULE: FREQ=WEEKLY;BYDAY=SU` in their drawer, `@chore` / `@home` labels survive as Org headline tags, and no `@`-prefixed leftovers remain in any title.

**Phase 18 closed at v0.12.0.** ensure_heading worker API + Org writer heading-emit (project sub-headings as depth-1 keyword-less headlines; tasks interleave by position) are the v0.12.0 prerequisites that earned the round-trip; the Todoist parser, NL recurrence parser, mapper, CLI subcommand, and butter test land in the same release.

## Phase 18.5: Org-mode power features for Builder Mode
*Org-mode is a four-decade research project on what task data wants to look like. Atrium already speaks the data layer (UUIDs, schedule/deadline/closed cookies, repeaters, properties, headline tags, two-way vault). This phase mines the **interaction patterns** Org built on top of that data layer that neither Things nor OmniFocus expose, and brings them into Builder Mode's Inspector pane so the synthesis is visible, not just structural. Sequenced after Todoist (18) so the next minor wave delivers actually-novel UX, not catch-up work.*

**Research grounding.** The original seven features here were AI guesswork; this revision is the result of a multi-source research pass (Bernt Hansen's norang workflow, Karl Voit's UOMF posts, cmdln.org-2024, Sacha Chua, Jethro Kuan, the Worg survey, Doom's `lang/org` module, the Org manual, Jeff Bradberry's checkbox + priority posts, Christian Tietze's checkbox-cycling). Two findings re-shaped the list: (1) **capture templates** are the most-cited Org feature across every source — Atrium already ships the Quick Entry surface but not template multiplicity, which is the largest underrecognised opportunity here; (2) **habits + custom-property interactive UI + active/inactive timestamps** all looked important on paper but research doesn't show real users leaning on them — habits are covered by existing repeaters + a streak counter, custom properties want lossless passthrough not a UI surface (split out as adjacent v0.14.x work), active/inactive timestamps already round-trip verbatim through body content. The list below is what real-world Org users actually touch every day, ranked.

The acceptance test for the whole phase: a Builder Mode user can do at least one thing in Atrium that Things 3 + OmniFocus + Todoist users genuinely cannot, and that Org-mode-via-Emacs users will recognise as familiar. Five Tier-1 features carry the phase; the three Tier-2 items follow if scope allows.

### Tier 1 — daily-use, ships in 18.5 (5 items)

- [x] **Custom TODO sequences (Org `#+TODO: TODO NEXT WAITING | DONE CANCELLED`)** *(v0.16.0)*. Sidecar gains a `[[todo_sequences]]` array-of-tables slot (workflow + done keyword sets per entry); the hand-rolled TOML parser learns one new value shape (string arrays). Writer projects the configured sequence as `#+TODO: STATE1 STATE2 | DONE1 DONE2` on every project file's preamble. Watcher reads the sidecar per diff-event, maps in-set done keywords to Atrium's completed state (with `orig_keyword` preserving the source label), and surfaces `VaultEvent::UnknownKeyword` for keywords outside the configured sets — graceful degradation, never destroy data. Builder Mode Inspector pane gains an `adw::ComboRow` keyword picker (visible only when a sequence is configured); CLI ships `atrium-cli vault sequences list/set/clear --vault PATH`. Zero schema. *Source: norang, Doom, Worg, Jethro Kuan.*

- [x] **CLOCK time tracking (Org `org-clock-in` / `org-clock-out`) with LOGBOOK projection** *(v0.17.0)*. New `task_clock_entry` side table (migration `0009_task_clock_entry.sql`, `user_version` 8 → 9). Worker enforces the single-active-clock invariant — opening a clock on task B auto-closes any other running clock first (mirrors Emacs's global clock). Inspector pane (Builder) gains a Time group with a Start/Stop button, "Total HH:MM" row, and per-session log. CLI: `atrium-cli clock in <id> [--note]` / `clock out <id>` / `clock log <id>` / bare `clock` for active-clock status. Org parser/emitter learn the `:LOGBOOK:` drawer + `CLOCK: [start]--[end] => HH:MM` line; in-progress entries are deliberately suppressed by the writer so the file doesn't churn while the clock runs. The watcher diffs CLOCK lines per-task (matching by started_at), inserting added entries via a new `import_clock_entry` worker command and deleting removed ones. Custom drawer entries the user adds inside `:LOGBOOK:` (state-change log lines, etc.) round-trip verbatim via `logbook_unknown_lines`. *Source: norang ("clocking fanatic"), Sacha Chua's clock-tables, cmdln.org-2024.*

- [x] **Statistics cookies on parent headlines (`[2/5]` / `[40%]`)** *(v0.15.0)*. Org parser captures `[N/M]` and `[N%]` cookies and strips them from titles; emitter projects them back at write time after recomputing from DB state (source-shape preservation: counter stays counter, percent stays percent). The writer's `stamp_statistics_cookies` walker counts immediate child TODOs (Done|Cancelled = done) + body-checkbox completions, mirroring Org's `org-checkbox-hierarchical-statistics` default. New `count_done_total_per_parent` SQL helper feeds the GUI inline cookie label on parent task rows; both modes (mode-as-view). Zero schema. *Source: Karl Voit, every Org project-management tutorial.*

- [x] **DEADLINE warning windows (Org `-Nd` / `--Nd`)** *(v0.14.0)*. Per-deadline lead times — `DEADLINE: <2026-05-15 Fri -7d>` means "surface 7 days early." Migration `0008_task_deadline_warn_days.sql` adds `deadline_warn_days INTEGER NULL` (`user_version` 7 → 8). `list_today` + `count_open_canonical.today` SQL gain `COALESCE(deadline_warn_days, TODAY_DEADLINE_WINDOW_DAYS)` so per-task overrides win without disturbing the global default for unmarked rows. Org parser/emitter recognise both `-` and `--` prefixes and canonicalise onto `-` (Atrium has no global-default-override concept). The watcher's diff path syncs external Emacs edits to the warning suffix back into the column. Builder Mode Inspector pane gains a SpinRow visible only when a deadline is set; CLI `add` / `edit` accept `--deadline-warn N`. Forecast leaves the deadline at its actual date for now — surfacing the warning window in the strip is a follow-up if it earns its way in. *Source: nvim-orgmode issue tracker, orgmode.discourse.*

- [x] **Quick Entry templates (Atrium's read of Org capture templates)** *(v0.18.0)*. Migration `0010_quick_entry_template.sql` adds the table (`user_version` 9 → 10) with name + shortcut_key (single ASCII alnum, validated by the worker since SQL can't express it cleanly) + target_project_id + prefix + default_tags (JSON array). Quick Entry modal grows a picker bar above the entry: each configured template renders as a `gtk::ToggleButton`; clicking activates (pre-fills the entry with `prefix`, stashes project + tags for commit). The Emacs convention of `:c ` as a leading shortcut trigger lands too — the modal's text watcher sniffs `:LETTER ` patterns and auto-activates the matching template, replacing the trigger with the template's prefix. Active template merges with inline `#tag` syntax at commit (template tags first, inline tags appended unless they conflict by name). CLI: `atrium-cli template list/add/edit/remove`. Both modes — the picker simply doesn't render when no templates are configured, so Simple Mode users who never set any up see the unchanged Quick Entry shape. *Source: Worg survey, norang, cmdln.org-2024, every "how I org" post.*

### Tier 2 — high-value-for-some, ship if scope allows (3 items)

- [x] **Org links between tasks (`[[id:UUID][label]]`)** *(v0.19.0)*. New `atrium_core::links` module surfaces `parse_body_links(body)` returning byte-range + UUID + label + has-explicit-label triples (10 unit tests). Inspector body renders links as styled spans (`gtk::TextTag` with foreground accent + underline) re-applied on every buffer change. Click gesture resolves the iter at the click position to a link → invokes a navigate callback → window resolves UUID → task id (new `task_id_for_uuid` read helper) → calls `open_inspector_for(id)`; stale links no-op silently. Builder Mode pane gains a "Link…" header-suffix button on the Notes group → popover with search-as-you-type ListBox (substring filter, capped at 50 rows) → click inserts `[[id:UUID][title]]` at the cursor. Both modes get the click-to-navigate; the picker is Builder-only. Zero schema. *Source: cmdln.org-2024, Karl Voit's UOMF: Linking Headings, org-roam ecosystem.*

- [x] **Body inline checkboxes (`- [ ]` / `- [X]` with state)** *(v0.15.0)*. New `atrium_core::checkbox` module recognises `- [ ]` / `- [X]` / `- [-]` lines (plus `+` and `*` bullet variants) in note bodies; verbatim round-trip preserved via the existing `OrgTask.body` field. Inspector pane (Builder) and Inspector dialog (Simple Mode) render a "Subtasks" group above Notes that lists each checkbox as a `gtk::CheckButton`; toggling rewrites the body via `toggle_body_checkbox` and dispatches the worker update (Builder's auto-save) or surfaces through Apply (Simple's transactional dialog). Cookie counter folds body-checkbox done/total into parent counts so a task with both subtasks + a body checklist gets one unified `[N/M]`. Zero schema. *Source: Karl Voit, Jeff Bradberry's lists-and-checklists, Christian Tietze.*

- [x] **Time-of-day on `scheduled_for`** *(v0.19.0)*. Migration `0011_task_scheduled_time.sql` adds `task.scheduled_time TEXT NULL` (HH:MM; `user_version` 10 → 11). The companion-column shape (rather than upgrading `ScheduledFor` to a sum type) keeps existing date sorting / SQL semantics intact and avoids the `ScheduledFor::Date | DateAt` API ripple. Org parser learns to capture the time portion of a SCHEDULED active timestamp into `OrgTask.scheduled_time`; emitter writes it back in canonical `<DATE Day HH:MM +Nx -Md>` order. Importer + watcher round-trip the column. Inspector pane (Builder) shows a Time entry beneath Schedule, visible only when scheduled is a Date; CLI `--time HH:MM` flag. Forecast + Calendar Month View prefix task titles with `HH:MM` when present. Closes the Todoist mapper's `DroppedTimeOfDay` lossy entry. *Source: Todoist mapper's lossy report, every "Org vs Todoist" comparison thread.*

**Phase 18.5 is now complete.** All five Tier-1 items + both Tier-2 items shipped across v0.14.0 → v0.19.0.

### Deliberately dropped (research validated skipping)

- **Habit grid (Org `STYLE: habit`).** Mixed signals across the research — Sacha Chua advocates, cmdln.org-2024 doesn't mention it, Bernt Hansen has the agenda view but doesn't emphasise it; the existence of `org-habit-stats` / `org-habit-plus` packages is itself a tell that the built-in is awkward. Atrium's existing repeating tasks (`+1d` Basic mode) cover the 80% case; a small **streak counter** in the Inspector for tasks with `repeat_rule IS NOT NULL` (computed from completion history once the LOGBOOK lands) is enough. Skip the consistency-grid widget entirely.
- **Custom property-drawer keys as an interactive feature.** Research surfaces almost zero workflows that *depend* on arbitrary properties beyond the four Atrium already handles — Karl Voit explicitly says he uses only `CREATED`, `ID`, and link-related properties. The case for surfacing them in the Inspector is weak. **But — lossless passthrough is a real gap** (`documented_limit_org_importer_drops_custom_property_keys` test pins it). Split out as an adjacent v0.14.x patch: add a `task_property` side table (or `extra_properties JSON` column), preserve unknown keys verbatim across round-trip, no UI. Spec §7.3.3 rule 1 alignment without overbuilding.
- **Active vs inactive timestamps in body content.** The Org distinction matters because active timestamps drive `org-agenda`; Atrium's agenda is driven by `scheduled_for` / `deadline` columns, not by scanning bodies. Either form already round-trips verbatim in body text. Nothing to do.
- **LOGBOOK as a separate first-class feature.** Subsumed by the CLOCK item — the LOGBOOK drawer is the natural projection target for clock entries. State-change events (`- State "DONE" from "TODO" [2026-05-09]`) are a smaller add on top once the drawer-emit machinery exists; defer to a CLOCK follow-up patch if real users ask.

### Schema impact summary

Two additive migrations across the Tier-1 set: `task_clock_entry` table (CLOCK), `task.deadline_warn_days INTEGER NULL` (warning windows). Tier-2's time-of-day adds `task.scheduled_time TIME NULL`. Tier-1's TODO sequences live in the sidecar — zero migrations. Statistics cookies, Org links, body checkboxes are all UI/emit/parse work — zero migrations. Per the schema rule, every migration here is append-only; no shipped migration gets rewritten.

### What's deliberately not in Phase 18.5

- **Drawers other than `:PROPERTIES:` / `:LOGBOOK:`** (`:NOTES:`, `:LINKS:`, etc.). Round-trip already preserved verbatim via `unknown_lines`; surfacing them as collapsible UI sections is over-design until users ask for it.
- **`org-attach` (file attachments).** Phase 19.5's drag-drop external files item covers this with simpler UX (a link in the note). Adding a per-task `attachments/` directory introduces vault-layout complexity that's not earned.
- **Org's column view / spreadsheet display.** Niche power-user feature; Atrium's Kanban + Perspectives already fill the "alternate views over the same data" slot.
- **Custom agenda commands.** Atrium's Perspectives are this feature under a different name — saved filter expressions surfaced as sidebar entries.
- **Refile (`C-c C-w`).** Already shipped under a different name — Atrium's "move to project" UI + drag-and-drop project picker covers what `org-refile` does. The Emacs UX shape (completion picker on a multi-file scope) is keyboard-first; Atrium's affordance is mouse-and-keyboard but the data operation is identical.

## Phase 19: Plain-text + CalDAV imports
*Round out the import surface for users coming from formats Atrium doesn't speak natively yet. One pass per source, sharing parser scaffolding. VTODO export ships here too. Scope is Linux + Org-mode-adjacent sources; macOS-only formats (TaskPaper, OmniFocus's `.ofocus` bundle) are out — the realistic audience for a Linux-native todo app doesn't have those files lying around. Atrium's schema remains the OmniFocus superset by spec commitment regardless; the importer was a Mac-to-Linux migration aid that didn't earn its weight against the more common Linux importers.*

- [x] **VTODO (RFC 5545) import:** `.ics` parser; covers the standard properties; covers Endeavour, Errands, Nextcloud Tasks, Planify (CalDAV-side). Shipped v0.25.0.
- [x] **VTODO export:** one-way `.ics` for hand-off to CalDAV apps. *Atrium does not become a CalDAV client.* Shipped v0.25.0.
- [x] **Taskwarrior:** `task export` JSON; UDA fields → tags / notes / drop per `--uda-as` flag. Shipped v0.26.0.
- [x] **todo.txt:** plain text with `(A)` priority, `+project`, `@context`, `due:` extension. Shipped v0.27.0.
- [x] **Unified import dialog** *(shipped v0.34.0):* picks source (Org / Todoist / VTODO / Taskwarrior / todo.txt), runs the parser + mapper through the worker, shows a dry-run preview report, and imports. Enabled by extracting the non-Org importers from `atrium-cli` into the `atrium-import` library crate so the GTK binary can reach them.
- [ ] **Dependency checks:** evaluate `ical` / `rustical` crates for VTODO; flag for sign-off if added.

### Cut from Phase 19 scope

- **TaskPaper.** macOS-only source app (Hog Bay Software). The format is portable plain text but the only realistic audience is Mac → Linux migrants — too narrow for a Linux-first roadmap.
- **OmniFocus (`.ofocus` bundle).** macOS / iOS only. Same logic that retired the Things 3 importer at v0.6.19. Atrium's schema remains the OmniFocus superset by spec commitment (§4); that's a *schema* decision and unaffected by dropping the *importer*. If a Mac → Linux migration story matters again later, OmniFocus users can export to OPML / VTODO and route through those.

## Phase 19.5: Productivity essentials (post-research gap-fill, v0.6.19)
*The gap analysis Brandon commissioned at v0.6.19 found nine items that competing native-Linux todo apps + Things 3 / OmniFocus / Todoist all expose, that Atrium doesn't yet. Most are pre-1.0 blockers — a productivity app without time-based reminders is hard to defend as "1.0 quality." Sources credited per item below; the analysis is in v0.6.19's patchnote.*

- [x] **System notifications / time-based reminders.** *Shipped v0.20.0.* Things 3 / OmniFocus / Planify all push reminders via the system notification daemon. New `reminder_at TIMESTAMP` column on `task` (migration `0012_task_reminder_at.sql`, `user_version` 11 → 12; partial index on open future reminders). A `gio::Notification` with the task title fires when `reminder_at <= now()` AND the task is open. A single tokio task on the GLib MainContext polls `next_pending_reminder`, sleeps until the soonest fire (or wakes via `Notify` when TaskChanges arrive); cap of one hour as a defensive re-query against clock jumps. Master toggle (`notifications-enabled`) gates the actual fire; default action `app.show-task::ID` opens the inspector. CLI: `add --reminder "YYYY-MM-DD HH:MM"`. *Sources: Things 3, OmniFocus, Planify.*
- [x] **Subtasks UI exposure** *(shipped v0.23.0).* `parent_id` (in the schema since `0001_initial.sql`) is now exposed: a Builder-only Inspector "Subtasks" group (per spec §5.1), indented list nesting, Shift-drag reparent, and `atrium-cli add/edit --parent`. The v0.15.0 body-checkbox group was renamed "Checklist" to free the "Subtasks" label. *Source: schema TODO. UX reference: Errands, Todoist, Things 3 checklists.*
- [ ] **Evolution Data Server (EDS) calendar overlay — read-only.** Atrium is a GNOME-native client running on a desktop that already has a calendar service: `evolution-data-server` is the GNOME-wide calendar / contacts / tasks backend, and GNOME Calendar (`gnome-calendar`, the default in GNOME 50) is its primary consumer. The user has already configured their accounts there (Google, Nextcloud, local, etc.); we read whatever EDS exposes via D-Bus and overlay events onto the Forecast / Today views as read-only "calendar context." Endeavour does the same shape for *tasks* — Atrium does it for *calendars* without becoming a calendar client itself. *No `.ics` file plumbing — that would duplicate what EDS already does properly.* Dependency check: `libecal` / `libedataserver` bindings or hand-rolled `zbus` D-Bus client. *Source: GNOME Calendar / Evolution Data Server. Conceptual mirror: Endeavour's task-side EDS integration.*
- [x] **`AdwPreferencesWindow`.** *Shipped v0.20.0 as `AdwPreferencesDialog` — the libadwaita 1.6+ replacement; the Window variant is deprecated.* Three pages: General (default mode, theme override, high-legibility font, vault path with folder picker), Capture (Quick Entry shortcut), Notifications (master toggle gating the v0.20.0 reminder service). All keys write through to the live GSettings backend; the dialog is a thin presentation layer. Wired to `app.preferences` (`Ctrl+Comma`) and the primary menu's "Preferences…" entry. Calendar feed paths deferred to the EDS overlay item below; Backups page deferred to that item. *Sources: every native GTK app.*
- [ ] **Task dependencies (`blocked_by`).** Taskwarrior treats this as fundamental. New `task_dependency` table (`task_id`, `blocks_task_id`); a task with any unfinished prerequisites surfaces with a "blocked" pill in the row. Atrium's `is:available` predicate already has the right shape for sequential projects; extend to dependency-blocked tasks too. *Source: Taskwarrior.*
- [x] **Drag external files / URLs to capture** *(shipped v0.30.0).* A window-level `gtk::DropTarget` accepts `gdk::FileList` (file-manager drags) + `String` (URLs / text); a drop opens Quick Entry pre-filled (file → base name, URL / text → verbatim) so the capture is reviewable, not silent. Quick Entry's `open` gained an `initial_text` param; the drop-payload parsing is a pure, unit-tested `capture_prefill_from_drop` helper. *Sources: standard Linux desktop pattern; explicit in Errands / Planify.*
- [x] **Task templates** *(shipped v0.33.0).* A reusable project shape (a named, optionally-nested set of tasks with per-item tags + estimates) instantiated into a fresh project. Migration 0017 adds `task_template` + `task_template_item` (index-based `parent_index` for nesting); distinct from the single-line Quick Entry `quick_entry_template`. Worker `instantiate_template` walks the item tree resolving parents + ensuring tags. CLI `atrium-cli task-template list/create/instantiate/delete`; GUI "New from Template…" picker. *Source: Todoist; Org-mode capture templates as conceptual reference.*
- [x] **First-run / onboarding** *(shipped v0.31.0).* A pristine database (no tasks, no projects, no areas) paints a welcoming `AdwStatusPage` with three CTAs (create your first project, capture a task, set up an Org vault) as a named page in `content_stack`; it clears itself the moment the user creates anything. No seeding, no GSetting. A cached `db_empty` flag (recomputed on each task / library change, short-circuiting on the first task) gates `refresh_active_list`. *Source: standard commercial app pattern; Brandon's v0.6.x cleanup arc already improved empty-state copy on canonical lists.*
- [x] **Backup / restore UI** *(shipped v0.32.0).* A Backups page in Preferences: "Back up now" (a `VACUUM INTO` snapshot to `$XDG_DATA_HOME/atrium/backups/`, keeping the newest 10), "Restore from backup…" (a file picker that queues the chosen snapshot to be copied over the live DB on next launch, via the `.restore-pending` marker), and a default-off `backup-weekly` GSetting (snapshot at launch when the newest is over a week old). Core helpers `backup::{backup_now, prune, latest_backup}` are CLI-exposed via `atrium-cli backup [--dir PATH]`. *Source: gap surfaced by Brandon's v0.6.19 research.*
- [x] **Inline editing on row edit (`atrium-inline`)** *(shipped incrementally; formalised when the Tier 3 run reached it — the v0.31.0 slot was repurposed for onboarding since this was already done).* When a task row enters edit mode (the double-click path that v0.1.13 → v0.1.16 shipped), the active editor parses `#tag` / `@date` / `!priority` syntax inline as the user types — markers convert into structured fields on commit, like Todoist or Fantastical. The parser already exists at `atrium-core::quick_entry`; this surfaces it on row edit instead of only inside the Quick Entry modal. If the parser grows past Quick Entry needs (tab-completion on existing tags, inline date suggestions, fuzzy completion across names), spin it out as a sibling crate `atrium-inline` so both row-edit and Quick Entry share one source of truth. *Sources: Todoist, Fantastical, Things 3 inline date parsing.*

## Phase 20: 1.0 — Polish, Localisation, Flathub
*The release that says Atrium is finished, not just shipped. Shipped as one minor per workstream (v0.35.0 →), then the `v1.0.0` tag.*

- [x] **Accessibility audit (round 2)** *(shipped v0.35.0).* Re-audited the Builder + Tier 2/3 surfaces (Inspector pane, Forecast, Review, Perspectives, kanban, Agenda, Calendar, plus Blocked-by / onboarding / import dialog / template picker / Backups); added explicit accessible labels to icon-only buttons (tooltip is only a description, not a name); confirmed the row pills carry text not colour-only; `docs/accessibility.md` updated. Full assistive-tech pass is Brandon's (needs a display).
- [x] **Performance regression suite** *(shipped v0.36.0):* `scripts/perf.sh` generates the 50K / 100K fixtures, times generation + a full read-path load, captures peak RSS, and asserts the headless budgets (50K data-layer < 80 MB idle; cold-start floor < 250 ms). Opt-in `--heaptrack` arm. Separate from the per-commit gate; `docs/perf-baseline.md` refreshed with the numbers (50K: ~220 ms / ~55 MB).
- [x] **Documentation site** *(shipped v0.37.0):* `mdbook` site under `book/` — short Guide chapters (modes, Quick Entry, search, import, Org vault) as on-ramps to `spec.md`, plus Reference chapters that `{{#include}}` the canonical `docs/*.md` (keymap, schema, accessibility, performance, gtd-patterns) verbatim so nothing forks. Built output git-ignored; hosting (Pages) is a follow-on.
- [ ] **Localisation scaffolding** *(deferred to a sandbox session):* `gettext-rs`, `po/` + meson i18n, marked Rust strings, `atrium.pot`, ship `en`. Scaffolding-only (no pilot translations at 1.0). Deferred out of the v0.37.0 slot — the meson MO-build wiring only verifies under a `meson` / `flatpak-builder` build (Brandon's environment).
- [ ] **Flathub readiness** *(deferred to a sandbox session):* offline cargo-sources (drop the network share) + `<screenshots>` metainfo scaffold. The offline build verification, screenshot capture, and the Flathub PR are all sandbox / account work.
- [ ] **AppStream screenshots refresh** — Simple and Builder both featured (Brandon captures; the XML scaffold lands with Flathub readiness).
- [ ] **Final icon and brand pass.**
- [ ] **`v1.0.0` tag, release notes, retrospective** — major-bump maintenance pass + the annotated tag (after l10n + Flathub readiness land).

**Deferred out of Phase 20 (post-1.0):**

- **Capture daemon (`atriumd`):** small binary under user systemd handling the global Quick Entry shortcut when the app is closed; IPC via D-Bus or local socket; global shortcut via the XDG GlobalShortcuts portal. Deferred from the 1.0 endgame — its own plan + a portal/dependency conversation. Closes the Phase 6c zero-launch carryover when it lands.

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
