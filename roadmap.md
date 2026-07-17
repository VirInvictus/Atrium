# Atrium — Roadmap

What's done, what's next, what's deferred. Atrium is sequenced as a clean Simple Mode v0.1, a Builder Mode v0.2 expansion, and a 1.0 with broad import/export across the Linux task-app ecosystem. **Current release: v0.47.0.**

Phases 0 through 19.5 have shipped: Simple and Builder modes, the two-way Org vault, Calibre-style search, the full importer set (Org, Todoist, VTODO, Taskwarrior, todo.txt), and the Phase 18.5 / 19.5 power features. **Phase 20 (the 1.0 endgame) is in flight**; localisation scaffolding shipped at v0.47.0. **Sequencing change (Brandon, 2026-07-17): Phase 22 (de-adwaita + Kanagawa) is pulled in front of the `v1.0.0` tag**, because the remaining 1.0 assets (final icon, AppStream screenshots, Flathub metadata) are all invalidated by the toolkit swap and would have to be redone a release later. So the pre-1.0 order is now: the de-adwaita ladder → the icon/screenshots/Flathub asset tail → the `v1.0.0` tag. The pilot gate is satisfied (Colophon's Phase 6 de-adwaita shipped at v2.0.0; Conservatory's Phase 26 completed at v0.3.8). The one open Phase 19.5 item is the read-only Evolution Data Server calendar overlay, gated on a `libecal` / `zbus` dependency sign-off.

The version-by-version release history lives in `patchnotes.md`; this document tracks the phase plan, the prioritised pre-1.0 order, and what's deferred.

---

## North Star

Twenty phases mapping the journey from empty repo to 1.0.

- **Phases 0–9:** Simple Mode → **v0.1**
- **Phases 10–15:** Builder Mode → **v0.2**. Phase 12.5 adds a Calendar Month View alongside Forecast (same data, different lens).
- **Phases 16–19:** Org-mode + Todoist + plain-text + VTODO interop. Phase 16 (one-shot import + DB → vault writer) shipped at v0.8.0; Phase 17 closes the loop with `inotify`-driven vault → DB sync. Phase 18.5 mines Org-mode's interaction patterns (CLOCK time tracking, LOGBOOK drawer, custom `:PROPERTIES:`, habit grid, statistics cookies, deadline warning windows, active/inactive timestamps) for Builder Mode's Inspector pane — features neither Things nor OmniFocus expose.
- **Phase 20:** Polish, localisation, Flathub → **v1.0**. Phase 22 (de-adwaita, below) now runs inside this endgame, before the tag; its number is later than Phase 21 but its execution is earlier (Phase 21's post-1.0 audit stays where it is).

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

### UI/UX audit follow-ups (2026-07)

A UI/UX audit benchmarked Atrium against the open-source todo/kanban ecosystem (Planify, Errands, Endeavour, GTG, Vikunja, Focalboard, Super Productivity). Most audited "gaps" turned out already shipped; the two real ones were bulk editing and kanban depth.

- [x] **Bulk editing** *(shipped v0.42.0)*: the multi-select selection bar gained Move… / Tag… / Schedule with coalesced undo, plus multi-id `atrium-cli edit`. No schema change. See §5.2.
- **Kanban maturity mini-phase** *(complete, v0.43.0 → v0.46.0)*: kept the projection column model (columns stay tag/status-derived for clean Org round-trip; no first-class buckets). Four sub-slices:
  - [x] 2a richer board cards *(v0.43.0)*: `[done/total]` cookie + amber "Blocked" pill, reusing the list-row logic; priority already shows as a `priority-N` pill.
  - [x] 2b per-column WIP limits *(v0.44.0)*: `name:limit` column suffix → `BoardConfig.limits` (`skip_serializing_if` keeps old configs byte-identical); header shows `count/limit`, over-limit flagged red. Advisory, never blocks a drop.
  - [x] 2c in-place "+ Add card" per column *(v0.45.0)*: per-column entry → `create_card_in_column` stamps the tag (tag axis) or keyword/completion (status axis, via `status_move`); reuses the inline parser.
  - [x] 2d persisted intra-column ordering *(v0.46.0)*: additive `board_card_position(perspective_id, column_key, task_id, position)` side table (migration 0019, schema 18 → 19), integer positions renumbered per reorder; per-card drop targets (drop-above) + column append; applies in GUI + `atrium-cli kanban`; set via `atrium-cli perspective reorder`.

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
- [x] **Localisation scaffolding** *(shipped v0.47.0):* `gettext-rs` (`gettext-system` feature; sign-off pre-granted here), `po/` + meson `i18n.gettext` (glib preset), a full marking sweep of the GTK binary's user-facing strings through the `atrium/src/i18n.rs` helpers, `atrium.pot` checked in, `en` shipped as the first catalogue. Scaffolding-only (no pilot translations at 1.0). The desktop entry + metainfo now install via `i18n.merge_file`; the shortcuts XML moved to `data/shortcuts.ui` so xgettext can see it; meson's `project()` version now reads the `VERSION` file. Convention going forward: every new metainfo `<release><description>` carries `translate="no"`; new `.rs` files with user-facing strings join `po/POTFILES`. See spec §3.6.
**Asset tail — runs after the Phase 22 de-adwaita ladder (below), so every asset is produced against the real 1.0 look, not the GNOME one:**

- [ ] **Final icon and brand pass.** Refine the placeholder "A"-as-building to the house grid (128 canvas, 12px safe margin, `rx≈22` card) in the Kanagawa Dragon palette, add a hand-redrawn 16×16 symbolic sibling, install both from meson, re-sample the metainfo `<branding>` colours. Independent of the stylesheet, so it may land at any point in the ladder; drawing it in Kanagawa now avoids a redraw at Phase 22.
- [ ] **Flathub readiness** *(offline build verifiable locally; the PR is account work):* vendor `flatpak-cargo-generator.py` (run via `uv run --with aiohttp --with toml`), generate a checked-in `cargo-sources.json` from `Cargo.lock`, drop `build-args: [--share=network]`, switch the module source `type: dir` → `type: git` + tag for submission, and add the `<screenshots>` metainfo scaffold. The screenshot capture and the Flathub PR itself stay Brandon's (display + account).
- [ ] **AppStream screenshots refresh** — Simple and Builder both featured (Brandon captures; the XML scaffold lands with Flathub readiness).
- [ ] **`v1.0.0` tag, release notes, retrospective** — major-bump maintenance pass + the annotated tag (after the de-adwaita ladder and the asset tail land).

**Deferred out of Phase 20 (post-1.0):**

- **Capture daemon (`atriumd`):** small binary under user systemd handling the global Quick Entry shortcut when the app is closed; IPC via D-Bus or local socket; global shortcut via the XDG GlobalShortcuts portal. Deferred from the 1.0 endgame — its own plan + a portal/dependency conversation. Closes the Phase 6c zero-launch carryover when it lands.

---

## Phase 21: Hyprland-Leaning Design — Tiling-First Polish (post-1.0)
*Brandon moved his desktop from GNOME Shell to Hyprland (a Wayland tiling compositor). This phase makes Atrium stop assuming GNOME Shell is the only compositor it will ever run under. Every item is additive: nothing here may regress Simple or Builder Mode for GNOME Shell users, no GNOME-specific integration is removed, and CSD stays the window-chrome model within this phase. This is a design audit informed by reading the actual GTK code, not a generic tiling-WM checklist; each item below cites the file it's grounded in.*

*Direction note (Brandon, 2026-07-09): the portfolio goal has since moved past "runs politely under Hyprland" to "fully belongs on Hyprland," which means dropping libadwaita while keeping GTK4. That work is Phase 22, gated on the Colophon pilot. This phase's audit items stay valid regardless (they are toolkit-agnostic geometry, keyboard, and portal work) and become part of Phase 22's verification tail if the two phases end up running together; only this phase's keep-adwaita guardrails are superseded.*

### Tiling-first geometry

- [ ] **Stop treating saved window geometry as load-bearing under tiling.** `AtriumWindow::bind_window_state` / `save_window_state` (`atrium/src/ui/window/mod.rs:402-420`) read `window-width` / `window-height` / `window-maximized` from GSettings and call `set_default_size` + `maximize()` on boot. Under a tiling compositor the compositor decides the window's actual size the moment it's tiled, so the restored size only matters for the brief pre-tile moment (or for a window a user deliberately floats). Audit: confirm nothing downstream (first paint, `AdwClamp` layout, sidebar fractions) assumes `default_width()` reflects a stable, user-chosen size rather than a transient or compositor-imposed one. No behaviour change for GNOME Shell users, who still get restore-on-launch as today.
- [ ] **Audit the two split-view minimum widths for arbitrary tile sizes.** `data/window.ui`: the Lists `AdwNavigationSplitView` sets `min-sidebar-width=220` / `max-sidebar-width=320` (line 57-58); the Builder-only Inspector `AdwOverlaySplitView` sets `min-sidebar-width=320` / `max-sidebar-width=480` / `sidebar-width-fraction=0.32` (lines 41-45). With both sidebars visible (Builder Mode, a task selected) the floor is roughly 220 + content + 320 px before either split view even starts collapsing. Verify by hand at common tile sizes (quarter of a 1920×1080 output is 960×540; a third of a 1366-wide laptop panel is ~455 px wide) that the content column doesn't get squeezed to an unreadable sliver, and that `AdwOverlaySplitView`'s own collapse-to-overlay behaviour actually kicks in rather than just shrinking the sidebar to its floor.
- [ ] **Give the kanban board a real narrow-tile story.** `atrium/src/ui/board.rs` gives each column a fixed `width_request(280)` (line ~210) inside a horizontally-scrolling container; there is no per-tile-width column count adjustment (contrast with Calendar Month View's `COMPACT_WIDTH_THRESHOLD` collapse, `atrium/src/ui/calendar.rs:213`). On a half-or-narrower tile, more than one column is rarely visible at once. Decide and implement a stance: either accept horizontal-scroll-only (document it) or add a one-column-at-a-time swipeable mode below a width threshold, mirroring the Calendar page's existing `default-width`-notify pattern (`atrium/src/ui/window/shell.rs:202-207`, `atrium/src/ui/window/views.rs:450-451`).
- [ ] **Extend the one hand-rolled breakpoint (`COMPACT_WIDTH_THRESHOLD = 600`, Calendar-only) to the surfaces that still lack any adaptive collapse.** There is no `AdwBreakpoint` anywhere in the codebase (`rg -w Breakpoint atrium/src` turns up nothing but comments); Calendar Month View is the only page with width-driven adaptive behaviour, wired by hand via `notify::default-width`. Audit the Inspector pane, the bulk-selection action bar (`data/window.ui`'s `selection_revealer` toolbar, ~10 buttons in a row with no wrap), and the project-extras toolbar (`project_extras_revealer`) for what happens at a genuinely narrow tile width, and either add real `AdwBreakpoint`s or extend the existing manual-threshold pattern consistently rather than leaving Calendar as the only page that adapts.
- [ ] **Check dialog content widths against small-tile parents.** None of the fixed `content_width` dialogs were paired with an `AdwBreakpoint` when surveyed: `atrium/src/ui/preferences.rs:38` (`AdwPreferencesDialog`, 620 px), `atrium/src/ui/inspector.rs:59` (Simple Mode Inspector, 560 px), `atrium/src/ui/import_dialog.rs:33` (540 px), `atrium/src/ui/tag_editor.rs:40` (380 px). `AdwDialog` in libadwaita 1.6+ has its own adaptive floor-behaviour, but that hasn't been verified against a parent window narrower than the dialog's requested width (a realistic case when Atrium itself is tiled into a narrow column). Verify each dialog degrades to a bottom-sheet or scrollable form rather than overflowing or clipping.

### Keyboard-first operation

- [ ] **Add a keyboard path to move a kanban card between columns.** Confirmed: card movement in `atrium/src/ui/board.rs` is wired exclusively through `gtk::DragSource` (line ~465) and two `gtk::DropTarget`s (inter-column at line ~332, intra-column reorder at line ~480); there is no action, accelerator, or menu entry that moves a card without a pointer drag (checked `install_accels` in `atrium/src/main.rs:711-774` and found nothing kanban-related). This is the single largest keyboard-first gap in the app for a tiling/keyboard-driven user: give the focused card a context menu or a `win.move-card-to-column` action reachable via `Menu`/`Shift+F10` and an accelerator, at minimum matching the existing `<Alt>Up` / `<Alt>Down` (`win.move-task-up` / `win.move-task-down`, list-only) precedent for regular list rows.
- [ ] **Audit whether the Builder Inspector pane can be dismissed or focused purely from the keyboard.** `Ctrl+I` (`win.edit-details-focused`, wired at `atrium/src/main.rs:769`, documented at `atrium/src/ui/shortcuts.rs:143-145`) opens the Inspector, but no accelerator or `GtkShortcutsShortcut` entry was found for closing or toggling the Inspector pane specifically (as distinct from the Simple Mode dialog, which already closes via `Escape`). Since the Inspector pane is an always-visible, non-modal `AdwOverlaySplitView` sidebar rather than a dialog, its dismiss/return-focus-to-list path needs its own keyboard story, not a borrowed `Escape` binding.
- [ ] **Verify the full daily-driver keyboard map end-to-end with the mouse unplugged.** The existing accelerator set is a good baseline (`Ctrl+N` new task, `Ctrl+Alt+Space` Quick Entry, `Ctrl+F` search, `Ctrl+1..6` canonical lists, `Ctrl+Shift+M` Calendar, `Ctrl+Z` undo, `Alt+Up`/`Alt+Down` reorder, `F2` rename, `Ctrl+L` focus sidebar filter, `Ctrl+T` / `Ctrl+I` tag/details on the focused row, `Ctrl+Shift+A`/`Ctrl+Shift+N`/`Ctrl+Shift+T` new area/project/tag; all in `atrium/src/main.rs:711-774` and mirrored in `atrium/src/ui/shortcuts.rs`). What's untested is whether focus order through the sidebar filter → list → Inspector pane → bulk-action toolbar is sane when tabbing rather than clicking; do a focus-order pass specifically for Builder Mode's three-pane layout (Lists sidebar, content, Inspector).

### Scratchpad-style quick capture

- [ ] **Document and lean into the Quick Entry modal's already-good scratchpad shape.** `atrium/src/quickentry/modal.rs:55-63` already builds Quick Entry as a small, non-modal `adw::Window` (`default_width(480)`, `default_height(120)`, `resizable(false)`, `modal(false)`, `transient_for(main)`) with a stable, static title of `"Quick Entry"`. That's structurally exactly what a Hyprland `windowrulev2 = float, ...` scratchpad target wants. Add a documented (README or `docs/`) example Hyprland window rule.
- [ ] **Resolve the app_id collision that blocks a class-based Hyprland window rule.** `atrium/src/main.rs:77` builds one `adw::Application::builder().application_id(APP_ID)` for the whole process, and no toplevel (main window, Quick Entry, Memory Watch) sets a per-window override; grep confirms there is exactly one `application_id` call in the binary. Every window therefore reports the same Wayland `app_id` (`io.github.virinvictus.atrium`), so a Hyprland rule keyed on `class:^(io.github.virinvictus.atrium)$` matches the main window too, not just Quick Entry. Since GTK4 has no per-toplevel app_id override, the practical fix is title-based matching (`title:^(Quick Entry)$`, exploiting the static title above); document that explicitly rather than leaving users to discover the collision by trial and error. Do not change the shared app_id itself; `StartupWMClass` in `data/io.github.virinvictus.atrium.desktop` already correctly matches it for the main window and must keep doing so.
- [ ] **Consider whether Quick Entry should raise/focus reliably when re-triggered while already open**, since a scratchpad workflow relies on the shortcut acting as a toggle (show-if-hidden, focus-if-shown) rather than only opening once. Check current behaviour of `atrium/src/quickentry/modal.rs::open` when invoked a second time while the window is already alive.
- [ ] **Note the existing `atrium-cli add` path as a stopgap capture surface, distinct from `atriumd`.** `atrium-cli`'s `add` subcommand (`atrium-cli/src/args.rs:749`, `:1827`) already does headless task creation against the live DB with no GUI involvement. It's not a substitute for the deferred `atriumd` capture daemon (Phase 20's "Deferred out of Phase 20" `atriumd` item stays the real zero-launch fix: `atrium-cli add` still needs Atrium's process/DB reachable and offers no inline parsing feedback), but it's usable today as a Hyprland-keybind-triggered capture command (`hyprctl` bind → terminal-less `atrium-cli add "..."`) and is worth a one-line callout in the docs so users don't wait for `atriumd` to get a keybind-driven capture flow.

### GNOME-session independence

- [ ] **Verify dark/light theme switching against the desktop portal, not GNOME Shell specifically.** `atrium/src/ui/preferences.rs:356-366` applies `adw::StyleManager`'s `ColorScheme` (`Default` / `ForceLight` / `ForceDark`) from the `theme` GSettings key (schema default `"auto"`, `data/io.github.virinvictus.atrium.gschema.xml`). `AdwStyleManager`'s `Default` scheme is documented to track `org.freedesktop.appearance.color-scheme` via the XDG Desktop Portal when no GNOME Shell `gsettings` key is present, but this has only ever been exercised under GNOME Shell. Confirm behaviour under Hyprland with `xdg-desktop-portal` + `xdg-desktop-portal-gtk` (or `-hyprland`) installed, and confirm the "auto" default degrades gracefully (falls to system default, not a hard failure) if no portal backend answers at all.
- [ ] **Verify `gtk::FileDialog` (the modern portal-routed chooser, used at `atrium/src/ui/preferences.rs:148`, `:301` and `atrium/src/ui/import_dialog.rs:100`) actually opens under a Hyprland + `xdg-desktop-portal-hyprland` (or `-gtk`) setup**, since it's never been exercised outside a GNOME session. This is already the right API choice (not the deprecated `GtkFileChooserNative`/`GtkFileChooserDialog`); the item is verification, not a code change.
- [ ] **Verify system notifications fire without GNOME Shell running.** `atrium/src/reminders.rs:169-175` builds a `gio::Notification` and calls `app.send_notification`, which talks to whatever implements the `org.freedesktop.Notifications` D-Bus name; confirmed no GNOME-Shell-specific or `libnotify`-direct code path exists. This should already work with `mako`, `dunst`, or `swaync` (common Hyprland notification daemons), but has not been exercised against any of them; a real confirmation pass closes this out rather than resting on "should work" from reading the code.
- [ ] **Don't assume Cantarell or a GNOME font-settings service.** Confirm the bundled-fonts path (Inter / Source Serif 4 / JetBrains Mono / Atkinson Hyperlegible under `data/fonts/`, installed via the Phase 8 fontconfig install-on-first-run) doesn't have any GNOME-Shell-only fallback assumption baked in; it shouldn't, since the whole point of bundling was machine-independence, but this phase is the natural point to double-check against a non-GNOME session.

### Wayland / Hyprland integration

- [ ] **Confirm `StartupWMClass` / app_id / `.desktop` basename stay in lockstep.** Already correct as read: `data/io.github.virinvictus.atrium.desktop` sets `StartupWMClass=io.github.virinvictus.atrium`, matching `Icon=` and the `application_id` built in `atrium/src/main.rs:77` (`atrium_core::APP_ID`). This is exactly what Hyprland needs for a default `windowrulev2 = ..., class:^(io.github.virinvictus.atrium)$` to match the main window. No change needed; this item is a guard so a future rename doesn't silently break it.
- [ ] **Audit for fractional-scaling sanity.** No GTK4 app needs bespoke fractional-scale code (GTK4's Wayland backend handles `wp-fractional-scale` natively), but the hand-rolled Cairo/GTK drawing surfaces in this codebase (Calendar Month View's `GtkGrid` cells, the kanban board's manually-sized columns) are worth a visual pass at a non-integer scale factor (e.g. 1.25x, 1.5x) since Hyprland setups commonly run fractional scaling where GNOME Shell historically nudged users toward integer scales.
- [ ] **Confirm the About dialog and Preferences use `present()` correctly for xdg-activation-style focus handoff** rather than any raise/focus assumption that depended on GNOME Shell's window-activation behaviour specifically. Spot-check `atrium/src/ui/preferences.rs` and the About dialog invocation path.

### CSD posture

- [ ] **Keep client-side decorations; audit headerbar weight rather than dropping CSD.** No change to the dialog-primitives convention documented in `CLAUDE.md` ("Dialog primitives, standardised v0.0.37"): Inspector/tag editor stay `adw::Dialog`, Quick Entry/Memory Watch stay non-modal `adw::Window`. The audit is narrower: confirm the app behaves sanely if a user sets `gtk-decoration-layout` to hide window buttons (a common Hyprland-adjacent tiling-WM preference, since the compositor's own bindings replace the close/maximize/minimize buttons); nothing in `data/window.ui`'s `AdwHeaderBar` children should assume the window-control buttons are always present, and no functionality (e.g. moving the window) may rely on chrome that a user has hidden.
- [ ] **Confirm nothing relies on server-side decorations (SSD) as a fallback.** Should already be a non-issue (libadwaita is CSD-only by design), but worth an explicit pass since Hyprland doesn't provide SSD by default the way some other compositors' fallback paths do.

### Schema impact

Zero. This entire phase is UI/window-management/desktop-integration work; no migration, no new table or column, no change to `atrium-core`.

### What's deliberately not in this phase

- **No Hyprland-specific config file shipped** (no bundled `hyprland.conf` snippet as an installed asset). A documented example window rule (Quick Entry scratchpad) is fine; owning a slice of Brandon's compositor config is not this project's job.
- **No compositor-detection branching in the code.** Every fix here (breakpoints, keyboard coverage, portal-routed dialogs, notification-daemon-agnostic notifications) is a correctness improvement that benefits GNOME Shell users too; there is no `if running_under_hyprland` code path anywhere, and there shouldn't be.
- **No SSD support, no dropping CSD within this phase.** Covered above; restated here because it's the one item most likely to get proposed and rejected if someone reads "tiling WM" as "remove decorations." Phase 22 revisits the decoration posture wholesale as part of the de-adwaita move; until that phase runs, this guardrail holds.
- **No global-shortcut / capture-daemon work.** That's the already-deferred `atriumd` item under Phase 20; this phase documents `atrium-cli add` as a stopgap, it doesn't build the daemon.

---

## Phase 22: De-adwaita — Hyprland-Native Design System (pre-1.0; pilot gate satisfied)

*Portfolio direction change (Brandon, 2026-07-09): the goal moved from "runs politely under Hyprland" (Phase 21's frame) to "fully belongs on Hyprland." Concretely: drop libadwaita, keep GTK4. GTK4 is Wayland-native and stays; libadwaita (the GNOME stylesheet, the adaptive widgets, the GNOME design language) is replaced with plain GTK4 widgets and an application stylesheet Atrium owns outright, styled flat and tiling-first rather than GNOME HIG. Colophon piloted the move (its roadmap, Phase 6) because it is the smallest shipped GTK app in the portfolio; Atrium follows now that the pilot's patterns (widget replacements, generated owned stylesheet, portal-based dark/light without `adw::StyleManager`) are proven.*

*Sequencing (Brandon, 2026-07-17): this phase is pulled in front of the `v1.0.0` tag rather than run post-1.0. The pilot gate is satisfied — Colophon's Phase 6 shipped at v2.0.0 (now v2.1.0; sheet generated from `colophon/src/theme.rs`, zero `.css` files), and Conservatory's Phase 26 completed at v0.3.8 ("zero libadwaita symbols in the workspace") with a 12-release sub-phase ladder (26b→26m) that maps almost 1:1 onto Atrium's surface. The reason to move it early: the remaining 1.0 assets (icon, screenshots, Flathub metadata) are all invalidated by the toolkit swap, so shipping 1.0 on GNOME's look means redoing them at Phase 22 anyway. Conservatory's `conservatory/src/theme.rs` and its ladder are the template. "Never break userspace" binds in full: no feature regression in either mode, and the app keeps working under GNOME; the look stops being GNOME's, not the compatibility.*

- [ ] **Go/no-go and sequencing against the pilot.** Review what Colophon's Phase 6 actually proved (and what it cost) before committing Atrium's much larger surface. Atrium is the portfolio's heaviest adwaita consumer: the standardised dialog primitives (`adw::Dialog` for Inspector/tag editor, non-modal `adw::Window` for Quick Entry/Memory Watch, per CLAUDE.md), two split views (`AdwNavigationSplitView` Lists, `AdwOverlaySplitView` Inspector), toasts, banners, the `AdwPreferencesDialog` family, and `AdwStyleManager` theming. First deliverable is a full `adw::` inventory with a mapped replacement per type, the Colophon migration table as the template.
- [ ] **Design decisions land in spec first.** Decoration posture (headerbar cargo, whether window buttons render at all), split-view replacements with tiling-honest panes instead of adaptive collapse, the dialog-primitive convention's plain-GTK successor, and dark/light via a direct `org.freedesktop.portal.Settings` read (gio D-Bus, no new dependency) instead of `adw::StyleManager`.
- [ ] **The owned stylesheet.** Author Atrium's application sheet (flat, square, hard borders, denser spacing; Kanagawa Dragon and the bundled-font typography unchanged), replacing the adwaita stylesheet classes in use. Reuse Colophon's generated-sheet machinery where it transfers. **Scope the keyboard-focus ring to the discrete interactive controls (`button:focus-visible, entry:focus-visible, switch:focus-visible, scale:focus-visible`, …); do NOT copy Colophon's universal `*:focus-visible { outline: accent }`** — that rings every widget in the focus chain when a bare modifier press (e.g. a tiling-WM workspace-switch chord) flips GTK into keyboard-focus mode, flashing the accent across the whole window (found in Conservatory 2026-07-12, fixed in its v0.3.7; Colophon carries the same bug pending its own fix). Atrium's current supplementary sheet already scopes its focus ring to sidebar rows, so the pattern to preserve is there.
- [ ] **Packaging follow-through.** Evaluate the Flatpak runtime move (GNOME runtime → freedesktop) once libadwaita is gone; Flathub metadata/screenshots refresh rides along. App-id and `.desktop` lockstep (Phase 21's verified invariant) must survive untouched.
- [ ] **Verification tail.** Phase 21's geometry/keyboard/portal audit items re-run against the migrated shell; they are the acceptance criteria for calling this phase done.

### Sub-phase ladder (one minor per step)

Adapted from Conservatory's 26b→26m. Each step is container-only where it can be (handler bodies unchanged), ships green through `scripts/regression.sh`, and regresses neither Simple nor Builder Mode. Owned widgets live in new `atrium/src/ui/` modules mirroring Conservatory's `ui/rows.rs`, `ui/dialogs.rs`, `ui/status_page.rs`, and `theme.rs`. C1, C9, and C10 are the fixed anchors (foundations first, visual flip mid, toolkit cut last); the middle steps may be reordered or merged as reality dictates, and each earns its own go-ahead before coding.

- [ ] **C1 — Foundations + priority fix.** About dialog → `gtk::AboutDialog`; keyboard-shortcuts window → hand-rolled plain-GTK. Fold in the CSS-provider-priority bug fix: the sheet loads at `PRIORITY_APPLICATION` (`atrium/src/ui/typography.rs`), below `PRIORITY_USER`, so a system-wide Kanagawa `~/.config/gtk-4.0/gtk.css` half-overrides Atrium today; move to `STYLE_PROVIDER_PRIORITY_USER + 1` (the Colophon/Conservatory fix). No visual change yet.
- [ ] **C2 — StatusPage.** Owned empty-state composite replacing the `adw::StatusPage` sites (onboarding, empty lists, filtered-no-match). Same copy, same runtime title/description swaps.
- [ ] **C3 — Toasts.** Owned crossfade auto-hide revealer replacing `adw::ToastOverlay` / `adw::Toast`. Same newest-wins 4 s behaviour.
- [ ] **C4 — AlertDialog.** Owned modal alert (`ui/dialogs.rs`) replacing the `adw::AlertDialog` sites, including the tag-colour-picker subclass (`prompt_for_tag`) with its swatch extra-child row. Named responses, suggested/destructive styling, Escape/close.
- [ ] **C5 — Rows family.** The big one: owned successors for `ActionRow`, `PreferencesGroup`, `ComboRow`, `EntryRow`, `SwitchRow`, `SpinRow`, `PreferencesPage`, `Bin`. Built incrementally as consumers migrate; the Inspector pane (Builder) and the Simple-Mode Inspector dialog are the heaviest consumers.
- [ ] **C6 — Split views.** Replace `AdwNavigationSplitView` (Lists) and `AdwOverlaySplitView` (Inspector) with `gtk::Paned` + a hand-rolled narrow-collapse width-watcher on the existing `COMPACT_WIDTH_THRESHOLD` pattern. Answers the Phase 21 split-view geometry audit items in place.
- [ ] **C7 — Preferences window.** Rebuild `AdwPreferencesDialog` as a plain modal window with a text page switcher over C5's owned groups/rows. Handler bodies (theme apply, vault-path `FileDialog`, notification toggles) unchanged.
- [ ] **C8 — Shell cut.** `AdwApplicationWindow` / `AdwToolbarView` / `AdwHeaderBar` → `gtk::ApplicationWindow` + real `gtk::HeaderBar`; rewrite `data/window.ui`. Decoration posture (window buttons kept vs. dropped) is decided in spec first, per the design-decisions bullet above. Quick Entry / Memory Watch `adw::Window` → `gtk::Window`, keeping the non-modal shape and the static "Quick Entry" title the Hyprland window rule keys on.
- [ ] **C9 — Visual flip + swatch migration.** Introduce `atrium/src/ui/theme.rs` (the generated Kanagawa sheet, `%TOKEN%` splice, the scoped-not-universal focus ring, the three-font-family test) replacing `data/style.css`'s reliance on adwaita named colours. **Migration 0020** rides here so the six tag/area swatch hexes recolour in lockstep with the CSS (see below).
- [ ] **C10 — Toolkit cut.** Drop `libadwaita` from the workspace + binary manifests; `adw::Application` → `gtk::Application`; retire `ColorScheme::ForceDark` for a direct portal read + `gtk-application-prefer-dark-theme`. meson/CI drop the libadwaita dev packages; align the gtk4 floor. `cargo tree` shows zero libadwaita. Spec / keymap / README sweep to the plain-GTK posture.

### Migration 0020 — swatch recolour (rides in C9)

The six swatch hexes are persisted user data: `tag.color` / `area.color` are `TEXT` hex strings (spec §4; `0001_initial.sql`, `0004_area_color.sql`) that round-trip into the Org sidecar as literal hexes (`atrium-org/src/sidecar.rs`). The class lookups (`swatch_class_for_hex`, `area_accent_class_for_hex`) are exact-string matches, so recolouring the palette without migrating the data silently degrades every existing tag/area to the grey fallback — a "never break userspace" tripwire. New `0020_swatch_kanagawa.sql` is UPDATE-only (append-only-safe): it rewrites exactly the six known Adwaita hexes to their Kanagawa counterparts and leaves off-palette values untouched (the picker only ever wrote those six). `user_version` bumps 19 → 20. `TAG_COLORS`, both reverse-map lookups, the CSS rules, fixtures, and the sidecar doc/test hexes move together. The sidecar round-trip needs verifying: confirm the DB migration stays authoritative and the next project write regenerates the sidecar from DB rather than an external stale-hex sidecar read writing the old value back.

### Schema impact

One migration (`0020`, swatch recolour, UPDATE-only). No new table, column, or constraint; no change to the OmniFocus-superset shape. Everything else in the phase is UI/toolkit/stylesheet work.

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
