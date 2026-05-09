# Atrium — Roadmap

What's done, what's next, what's deferred. Sequenced for a clean Simple Mode v0.1, a Builder Mode v0.2 expansion, and a 1.0 with broad import/export across the Linux task-app ecosystem. Current release: **v0.12.0** — Phase 18 (Todoist CSV import) shipped. Hand-rolled CSV parser + natural-language recurrence parser + mapper; new `WorkerHandle::ensure_heading` API; Org writer learned to emit project sub-headings as depth-1 keyword-less headlines with tasks interleaved by `position`; deterministic v5 UUID namespace for re-import stability; `Home.csv` butter test pins the Todoist → DB → vault → re-parse round-trip. Phase 17 (vault → DB two-way sync) closed at v0.10.3; Phase 12.5 (Calendar Month View) closed at v0.11.0. Phase 18.5 (Org-mode power features) and Phase 19.5 (productivity essentials) are next.

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

- **Phase 6c — Quick Entry cold-start** (Phase 20 / `atriumd`). The in-app shortcut only fires while Atrium has focus; true zero-launch capture is a daemon problem.
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

The acceptance test for the whole phase: a Builder Mode user can do at least one thing in Atrium that Things 3 + OmniFocus + Todoist users genuinely cannot, and that Org-mode-via-Emacs users will recognise as familiar. The seven features below clear that bar at different levels — pick the subset that ships per Brandon's prioritisation when the time comes.

- [ ] **CLOCK time tracking (Org `org-clock-in` / `org-clock-out`).** Distinct from `estimated_minutes` (intent) — this records *actual* time spent, accumulated across multiple work sessions. New `task_clock` table (`task_id`, `started_at`, `ended_at` NULL when in-progress). Inspector pane gains a Start/Stop button + total time + per-session log. Round-trips to Org as `CLOCK: [start]--[end] => HH:MM` lines under a `:LOGBOOK:` drawer. *Source: Org-mode. Neither Things nor OF have native time tracking; OmniFocus Pro has estimates only.*
- [ ] **LOGBOOK drawer — structured per-task changelog.** Org records every state change automatically: `- State "DONE" from "TODO" [2026-05-09 Sat 10:23]`. Atrium has `modified_at` (last-write timestamp) but no event log. New `task_event` table (`task_id`, `kind`, `from_value`, `to_value`, `at`); the worker emits an event row on every state-changing command. Inspector pane gains a "History" disclosure with the event timeline. Round-trips to Org as `:LOGBOOK:` drawer entries. *Source: Org-mode `org-log-into-drawer`.*
- [ ] **Custom `:PROPERTIES:` per task.** Org's `:PROPERTIES:` drawer holds arbitrary key/value metadata per headline. Atrium already round-trips them via `unknown_lines` (per spec §7.3.3 rule 1) — they survive vault round-trips but aren't surfaced in the GUI. Inspector pane gains a "Custom fields" disclosure with editable key/value rows. Schema-free per task: any user can have project-specific metadata (`:URL:`, `:CLIENT:`, `:HOURLY_RATE:`) without a migration. *Source: Org-mode. No equivalent in Things, OF, or Todoist.*
- [ ] **Habit grid (Org `STYLE: habit`).** Repeating tasks gain a small consistency-tracking widget: a 14-day grid showing which days the task was completed, with streak counter. Distinct from regular repeats — emphasises the pattern, not the next instance. New `task.is_habit BOOLEAN` column (additive migration). Inspector surfaces the grid only for tasks where `repeat_rule IS NOT NULL AND is_habit = TRUE`. Round-trips to Org as `:STYLE: habit:` property + the existing `:LAST_REPEAT:` cookie support. *Source: Org-mode habits.*
- [ ] **DEADLINE warning windows (Org `-Nd`).** Org's deadline cookies accept a warning offset: `DEADLINE: <2026-05-15 Fri -3d>` means "surface this task in agenda starting 3 days before the deadline." Atrium has Forecast (which surfaces upcoming deadlines uniformly); per-task warning windows let users say "this one is sensitive — flag me a week ahead." New `task.deadline_warning_days INTEGER` column (additive); Forecast queries surface the task starting `deadline - warning_days`. Round-trips to Org via cookie suffix. *Source: Org-mode.*
- [ ] **Statistics cookies (`[2/5]` / `[40%]`).** Org auto-renders progress on parent headlines that have child tasks: `* TODO Build feature [2/5]` shows 2 of 5 subtasks complete. Atrium has `parent_id` already; this is pure UI. Task-row title gains an inline `[N/M]` or `[P%]` cookie when the row has children. Lands alongside Phase 19.5's Subtasks UI exposure. *Source: Org-mode. UX recognised by anyone who's used it.*
- [ ] **Active vs inactive timestamps.** Org distinguishes `<2026-05-09>` (active — drives agenda) from `[2026-05-09]` (inactive — record only, doesn't schedule). Atrium's `scheduled_for` is essentially active; there's no "completed by" or "started on" record that doesn't drive scheduling. New `task.dates_other JSON` column for typed timestamp records (`{"completed_by": "[2026-05-09]", "started_on": "[2026-04-12]"}`); Inspector surfaces an "Other dates" disclosure. Round-trips to Org as inactive-timestamp lines under the headline. *Source: Org-mode. Useful for retrospective-only date records that shouldn't pollute Forecast.*

**Schema impact summary:** four additive migrations for the schema-touching items (`task_clock` table, `task_event` table, `task.is_habit`, `task.deadline_warning_days`). The custom-properties surface is UI-only — `unknown_lines` already preserves the data. Statistics cookies are UI-only — `parent_id` already encodes the structure. Active/inactive timestamps would need `task.dates_other JSON` (additive). Per the schema rule, every migration here is append-only; no shipped migration gets rewritten.

**What's deliberately not in Phase 18.5:**

- **Org links (`[[id:UUID]]` task-to-task).** Useful but conceptually heavy — needs a link-handling UI (open-in-app vs follow-link) that's its own design surface. Slip if it earns its way back in.
- **Capture templates.** Phase 19.5 already has an item for task templates; Org-flavoured capture templates are a layer on top of that.
- **Drawers other than `:PROPERTIES:` / `:LOGBOOK:`** — `:NOTES:`, `:LINKS:`, etc. Round-trip already preserved verbatim via `unknown_lines`; surfacing them as collapsible UI sections is over-design until users ask for it.
- **`org-attach` (file attachments).** Phase 19.5's drag-drop external files item covers this with simpler UX (a link in the note).

## Phase 19: Plain-text + CalDAV imports + OmniFocus long-tail
*Round out the import surface for users coming from formats Atrium doesn't speak natively yet. One pass per source, sharing parser scaffolding. VTODO export ships here too. OmniFocus moves here from its own phase — `.ofocus` is macOS-only, so the audience is small (same logic that retired the Things 3 phase at v0.6.19), but the OF half of Atrium's GTD lineage is worth keeping a path open for.*

- [ ] **VTODO (RFC 5545) import:** `.ics` parser; cover the standard properties; covers Endeavour, Errands, Apple Reminders, Nextcloud Tasks, Planify (CalDAV-side).
- [ ] **VTODO export:** one-way `.ics` for hand-off to CalDAV apps. *Atrium does not become a CalDAV client.*
- [ ] **Taskwarrior:** `task export` JSON; UDA fields → tags or notes per user choice.
- [ ] **todo.txt:** plain text with `(A)` priority, `+project`, `@context`, `due:` extension.
- [ ] **TaskPaper:** plain text headlines, `@tags`, `@done` metadata.
- [ ] **OmniFocus:** `.ofocus` bundle XML; archive structure, transaction folding. Folders → areas, Projects → projects with `sequential` flag, Actions → tasks, Contexts/Tags → tags, Defer → `defer_until`, Due → `deadline`, Estimated → `estimated_minutes`, Repeat → `repeat_rule`. Perspective definitions imported as Atrium Perspectives where the filter language allows. Test fixture: sanitised sample `.ofocus` bundle in `tests/fixtures/omnifocus/`.
- [ ] **Unified import dialog:** picks source, runs parser in worker, shows pre-import report, commits in batch (Phase 2 coalescer earns its keep).
- [ ] **Dependency checks:** evaluate `ical` / `rustical` crates for VTODO; flag for sign-off if added.

## Phase 19.5: Productivity essentials (post-research gap-fill, v0.6.19)
*The gap analysis Brandon commissioned at v0.6.19 found nine items that competing native-Linux todo apps + Things 3 / OmniFocus / Todoist all expose, that Atrium doesn't yet. Most are pre-1.0 blockers — a productivity app without time-based reminders is hard to defend as "1.0 quality." Sources credited per item below; the analysis is in v0.6.19's patchnote.*

- [ ] **System notifications / time-based reminders.** Things 3 / OmniFocus / Planify all push reminders via the system notification daemon. New `reminder_at TIMESTAMP` column on `task` (additive migration, schema rule). A `gio::Notification` with the task title fires when `reminder_at <= now()` AND the task is open. Reminders survive app restarts via a small "next pending reminder" worker timer. *Sources: Things 3, OmniFocus, Planify.*
- [ ] **Subtasks UI exposure.** `parent_id` has been in the schema since `0001_initial.sql` ("Builder-only UI in v0.1 (schema supports any depth)") but the GUI doesn't render the hierarchy yet. Inspector pane gains a Subtasks group; the task list either indents children or surfaces them via a disclosure triangle. atrium-cli's `add` / `edit` gain `--parent ID` flag. *Source: schema TODO. UX reference: Errands, Todoist, Things 3 checklists.*
- [ ] **Evolution Data Server (EDS) calendar overlay — read-only.** Atrium is a GNOME-native client running on a desktop that already has a calendar service: `evolution-data-server` is the GNOME-wide calendar / contacts / tasks backend, and GNOME Calendar (`gnome-calendar`, the default in GNOME 50) is its primary consumer. The user has already configured their accounts there (Google, Nextcloud, local, etc.); we read whatever EDS exposes via D-Bus and overlay events onto the Forecast / Today views as read-only "calendar context." Endeavour does the same shape for *tasks* — Atrium does it for *calendars* without becoming a calendar client itself. *No `.ics` file plumbing — that would duplicate what EDS already does properly.* Dependency check: `libecal` / `libedataserver` bindings or hand-rolled `zbus` D-Bus client. *Source: GNOME Calendar / Evolution Data Server. Conceptual mirror: Endeavour's task-side EDS integration.*
- [ ] **`AdwPreferencesWindow`.** No app-level preferences dialog exists; GSettings keys are set programmatically. Build a real `AdwPreferencesWindow` covering: vault path, Quick Entry shortcut binding, mode default, notifications on/off, calendar feed paths, theme (auto / light / dark — already auto via Adwaita but expose the override). *Sources: every native GTK app.*
- [ ] **Task dependencies (`blocked_by`).** Taskwarrior treats this as fundamental. New `task_dependency` table (`task_id`, `blocks_task_id`); a task with any unfinished prerequisites surfaces with a "blocked" pill in the row. Atrium's `is:available` predicate already has the right shape for sequential projects; extend to dependency-blocked tasks too. *Source: Taskwarrior.*
- [ ] **Drag external files / URLs to capture.** Drop a PDF onto the window → quick-entry-style new task with the path stored as a link in the note. Drop a URL → task with the URL pre-filled. GTK4 `gtk::DropTarget` accepts MIME types directly. *Sources: standard Linux desktop pattern; explicit in Errands / Planify.*
- [ ] **Task templates.** A reusable shape (project + standard subtasks + tags + estimated times) instantiable as a fresh project. `template` table or `project.is_template` flag; "Create from template…" in the project menu. *Source: Todoist; Org-mode capture templates as conceptual reference.*
- [ ] **First-run / onboarding.** Empty database on first launch shows a welcoming `AdwStatusPage` with three suggested next-steps (create your first project, capture a task with Ctrl+N, set up an Org vault). Optional: seed a small "Welcome" project with three tasks. *Source: standard commercial app pattern; Brandon's v0.6.x cleanup arc already improved empty-state copy on canonical lists.*
- [ ] **Backup / restore UI.** SQLite file-copy is the existing escape hatch but no in-app affordance exposes it. *Settings → Backups* with "Back up now" (writes a timestamped copy alongside the DB) and a quarterly automatic backup (off by default; opt-in via the new preferences dialog). *Source: gap surfaced by Brandon's v0.6.19 research.*
- [ ] **Inline editing on row edit (`atrium-inline` candidate).** When a task row enters edit mode (the double-click path that v0.1.13 → v0.1.16 shipped), the active editor parses `#tag` / `@date` / `!priority` syntax inline as the user types — markers convert into structured fields on commit, like Todoist or Fantastical. The parser already exists at `atrium-core::quick_entry`; this surfaces it on row edit instead of only inside the Quick Entry modal. If the parser grows past Quick Entry needs (tab-completion on existing tags, inline date suggestions, fuzzy completion across names), spin it out as a sibling crate `atrium-inline` so both row-edit and Quick Entry share one source of truth. *Sources: Todoist, Fantastical, Things 3 inline date parsing.*

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
