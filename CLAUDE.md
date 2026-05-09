# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

**Simple Mode shipped (v0.1.0, May 2026).** Phases 0–9 complete. Atrium runs end-to-end: workspace scaffolding, schema + single-writer worker, application shell, all six canonical lists (Inbox / Today / Upcoming / Anytime / Someday / Logbook), areas + projects + tags + multi-tag, Quick Entry, FTS5 search + filter expressions, multi-select + undo, Inspector + tag editor dialogs, sidebar find-as-you-type, full keyboard map, typography + accessibility (Atkinson Hyperlegible), debug-pane Memory Watch, ship-gate regression script. Three Phase 9 follow-ups remain on Brandon's plate (the actual `v0.1.0` git tag, the Flatpak publish, the public announcement on `VirInvictus.github.io`); two Phase 8 carryovers also outstanding (README screenshots, Flatpak font verification under sandbox).

**Builder Mode shipped (v0.2.0, May 2026).** Phases 10–15 complete. Mode toggle + Inspector pane + project Sequential / Review extras (Phase 10), defer dates + sequential rendering (Phase 11), Forecast 30-day calendar-axis page (Phase 12), Review queue with Mark Reviewed (Phase 13), saved Perspectives in their own sidebar section (Phase 14, v0.1.17), and Repeating Tasks with full RFC 5545 RRULE support + three Org-mode completion semantics (Phase 15, v0.2.0). v0.2.0 ends the v0.1 schema freeze: `ALTER TABLE` migrations are now allowed and v0.2.0 ships the first one (`0003_repeat_mode.sql` adds `task.repeat_mode`).

**v0.2.x patches + v0.3.0 visual polish landed (May 2026).** v0.2.1 fixed the tag-pill update path and shipped the *Area › Project* row context chip. v0.2.2 was an audit-pass patch — filter-typo toast warnings, sidebar zero-state hint, screen-reader badge labels, Inbox chip fallback. v0.3.0 was a focused visual-polish minor: tag colours wired end-to-end (six-swatch picker, sidebar dots, Pango-coloured pills), row hover states, completion micro-animation, per-list empty-state warmth, sidebar section dividers, header-bar `Area › Project` breadcrumb, Inspector-pane card treatment.

**Phase 15.5 shipped at v0.4.0 (May 2026).** Calibre-powered search: the search bar's flat filter language grew into a full expression grammar — boolean operators (AND / OR / NOT, `NOT > AND > OR` precedence), parens grouping, comparison + range operators on date and numeric fields, date keywords (`thisweek`, `5daysago`, etc.), state predicates (`is:overdue`, `is:repeating`, etc.), and Calibre-style match modifiers (`tag:x` substring, `tag:=x` exact, `tag:~regex`, `tag:true`/false). Saved Perspectives inherit the new grammar for free since they store filter expressions verbatim. Full reference in `spec.md` §4.3.

**v0.5.0 shipped (May 2026) — atrium-cli, search engine evolution, Phase 15.75 Slices A + B.** Fifteen post-v0.4.0 patches rolled into one minor. Closed the Phase 15.5 deferred-list (state predicates, `sort:` modifier, ↑/↓ history, `?` operator-reference popover, fuzzy match). Extracted the search engine to `atrium-search` and shipped a complete headless CLI as `atrium-cli` (full task CRUD: search / list / info / add / capture / edit / complete / delete; metadata reads for areas / projects / tags / perspectives; TSV / JSON / human output). Phase 15.75: Slice A foundation (`area.color` + `perspective.{renderer, renderer_config}` migrations), Slice B visual rhythm + per-area accent + canonical-list icon tinting + tag-icon fix + About-dialog icon resolution. Schema version: 5.

**v0.5.x patch arc (May 2026).** Four follow-ons after v0.5.0: `v0.5.1` shipped the `atrium-cli` runtime-nesting fix + the broken-pipe SIGPIPE reset + the missing AppStream entries; `v0.5.2` added FTS5 `bm25` + recency ranking on bare-text searches (search results sort by relevance instead of `task.position`); `v0.5.3` shipped the SQL-translation evaluator (`atrium_search::try_translate` — the search engine pushes most expressions to SQLite at query time with an in-memory fallback for regex / fuzzy / composite predicates); `v0.5.4` shipped the Slice D1 foundation (`atrium_core::render` module + `atrium-cli kanban NAME` subcommand).

**v0.6.0 → v0.6.5 — Slice D end-to-end (May 2026).** Saved Perspectives whose `renderer = "board"` render as kanban columns. v0.6.0 read-only board page; v0.6.1 row metadata + interactive checkbox; v0.6.2 in-GUI renderer-config dialog; v0.6.3 drag-drop between columns (`move_to_column`); v0.6.4 Slice D2 — Agenda canonical page (Overdue / Today / Tomorrow / This Week / Next Week); v0.6.5 `atrium-cli perspective` write side (create / edit / delete from the shell).

**v0.6.6 → v0.6.10 — perf, sidebar reorg, soft-accent pass.** v0.6.6 mitigated CPU spikes during kanban drag (dropped a CSS hover transition + wired the SQL fast-path on board refresh); v0.6.7 reorganised the sidebar so Agenda / Forecast / Review join the top tier alongside the canonical lists; v0.6.8 was a docs catch-up pass (spec / roadmap / README aligned with what shipped); v0.6.9 fixed the Memory Watch theme-parser warning + the search-bar `connect_entry` warning; v0.6.10 layered a soft-accent pass across six surfaces (sidebar gradient, header bar leading edge, page title weight, sidebar count badges, section headers, hover tones).

**v0.6.11 → v0.6.16 — screenshot-driven cleanup arc.** Four patches off Brandon's screenshot review: v0.6.11 (eight quick wins — Inspector copy, Inbox chip suppression, window title reflects view, fixture colours, AdwClamp 720→960), v0.6.12 (state-aware row treatment — overdue red / today amber / upcoming accent on checkbox + date pills), v0.6.13 (Inspector Notes placeholder), v0.6.14 (visible row separators + derived recurrence icon for `repeat_rule != NULL`). v0.6.15 fixed the Memory Watch background + the Generate Fixtures debug action. v0.6.16 reorganised the sidebar so Logbook bookends the top tier (was sitting between Someday and Agenda).

**v0.6.17 → v0.6.20 — interaction polish + roadmap revision.** v0.6.17 wired click-to-open on Forecast rows (drag-to-reschedule was working; click was a dead-end). v0.6.18 was the efficiency pass (SQL fast-path on the list-renderer perspective + search-bar paths; eliminated a duplicate tag-map fetch). v0.6.19 + v0.6.20 were the roadmap revision triggered by Brandon's gap-analysis prompt — Things 3 import retired, Org-mode promoted to Phase 16/17 as the must-ship two-way mirror, Todoist promoted to its own Phase 18, new Phase 19.5 (productivity essentials) with the calendar item targeting Evolution Data Server (not iCal-file feeds — Atrium reads what GNOME Calendar already consumes).

**Test counts as of v0.6.20:** 119 atrium (binary) + 174 atrium-core (lib + 1 mode-flip integration) + 106 atrium-search + 106 atrium-cli = 505 tests. All green; ship-gate runs in under 2 seconds.

**Architectural commitment: every non-GUI surface stays CLI-testable.** The data layer, search engine, and import/export pipelines are all designed so they can be exercised through `atrium-cli` (or future siblings like `atrium-import`, `atrium-export`). The 2.0-era TUI (`atrium-tui`) is the same shape: another headless consumer of `atrium-core` + `atrium-search`. Don't add functionality to the GTK binary that can't be reached from the shell.

**Phase 16 (Org-mode import + read-only vault sync) is what's next** — primary plain-text covenant. Phase 17 follows with full two-way `inotify` sync. Brandon's "MUST" interop direction; the agenda-parity acceptance test (Atrium's Agenda page and stock `org-agenda` over the same vault produce semantically equivalent buckets) gates Phase 17. v0.6.19 retired the Things 3 import phase (`.things` JSON is macOS-only — vanishingly small GNOME audience); promoted Todoist to its own Phase 18 as the first-class cross-platform on-ramp; added Phase 19.5 covering the nine productivity essentials surfaced by the gap-analysis pass (system notifications, subtasks UI, **GNOME Calendar / Evolution Data Server overlay**, AdwPreferencesWindow, task dependencies, drag-drop external capture, templates, onboarding, backup UI). See `roadmap.md` and `spec.md` §7 for the full revision.

## Authoritative documents

- **`spec.md`** is the contract. Architecture (§3), schema (§4), UI deltas (§5), Quick Entry (§6), import/export mapping (§7), and the perf budget (§8) all live there. **Read it before changing semantics.** If a request conflicts with the spec, surface that conflict — don't quietly drift.
- **`roadmap.md`** is the 20-phase plan plus four sub-phases (12.5, 15.5, 15.75, 19.5). Phases 0–9 → v0.1 (Simple Mode). Phases 10–15 → v0.2 (Builder Mode). Phase 15.5 → Calibre-powered search. Phase 15.75 → visual rhythm + GTD audit + kanban + Agenda. Phases 16–19 → import/export (Org first, then Todoist, then long-tail). Phase 19.5 → productivity essentials gap-pass. Phase 20 → v1.0. **Don't skip phases or pull work forward** without explicit go-ahead — Brandon sequenced these deliberately to keep each release shippable.
- **`patchnotes.md`** — newest at top. v0.1.0 was the first user-facing tag; v0.6.20 is the most recent release. The v0.5.x and v0.6.x arcs covered atrium-search extraction, atrium-cli, FTS5 ranking, SQL-translation evaluator, kanban Perspectives, the Agenda page, and the screenshot-driven cleanup pass.

## Architectural commitments (don't drift from these)

These five decisions are load-bearing. Any code that contradicts them is wrong even if it compiles and passes tests.

### 1. Mode-as-View

Mode (Simple / Builder) is a **GSettings flag plus UI-layer rendering choices** — nothing more. It does not affect schema, does not migrate data, does not hide rows, does not constrain Quick Entry. The schema is the **OmniFocus superset** on day one; every Builder column (`defer_until`, `estimated_minutes`, `sequential`, `review_interval_days`, `last_reviewed_at`, `repeat_rule`, `parent_id`) exists from migration `0001_initial.sql`. Simple Mode hides those fields in the editor and in derived views; it does not lack them.

The Phase 10 acceptance test (mode-flip snapshot) exists to enforce this — flipping mode must not touch the DB.

### 2. Single-writer SQLite worker

A dedicated `tokio` task owns the writable `rusqlite::Connection`. The GTK thread holds an `mpsc::Sender<Command>` and **never** touches the writable connection. Reads use a separate read-only connection pool. WAL mode is mandatory. UI updates arrive as `TaskChanges { created, updated, deleted, status_changed }` deltas via a `glib::MainContext` channel — **never as full reloads**.

This pattern is lifted directly from Viaduct's `DatabaseQueue` (sibling repo at `~/.gitrepos/Viaduct/`). When implementing the data layer, look there for the pattern's shape rather than reinventing it.

### 3. Local-first, no network sync

SQLite at `$XDG_DATA_HOME/atrium/atrium.db`. No CalDAV client, no cloud, no telemetry, no network calls in v1.0. VTODO export (Phase 19) is a one-way file dump — explicitly **not** a CalDAV client. Local file mirroring (the Org vault, see commitment #5) is fine — that's filesystem IO, not network sync. If a feature request implies *network* sync, push back; it's out of scope through 1.0.

### 4. Debug-first architecture

Testing and debugging tooling is **built into the binary**, not bolted on after the fact. A `--debug` CLI flag opens a debug mode that surfaces special functions for stress-testing, edge-case rehearsal, and runtime introspection:

- **Stress generators** — synthesize 10K / 50K / 100K-task fixture databases on demand so the perf budget (spec.md §8) can be validated without hand-seeding.
- **Edge-case fixtures** — pre-canned weird states reachable from a debug menu: empty projects, deeply nested hierarchies, recurring rules at DST boundaries, malformed imports, clock-skewed timestamps, unicode-hostile titles.
- **IO instrumentation** — every SQLite statement (text, params, duration) and every file read/write logged via `tracing` spans into a debug pane. No new crates; this rides on the dependencies already in Phase 0.
- **Memory watch** — periodic RSS / heap sampling surfaced in the debug pane, with a "drop caches" affordance to expose retained allocations and leaks.

The skeleton lands in Phase 0 alongside the Cargo scaffolding so later phases grow the harness instead of inventing it. Release builds ship the same code paths — heavy generators are gated on `--debug` so end users never see them, but the wiring is always present. Tests reuse the same fixtures; don't fork a separate "test-only" path.

### 5. Vault projection, not alternative store

When configured, an Org vault (default `~/Tasks/`) mirrors task state to `.org` files for editing in Emacs / Doom / vim-orgmode / any Org-aware tool. The discipline is **DB canonical, vault projected** — SQLite is the source of truth, the vault is downstream. Atrium runs cleanly without a vault; the vault never runs without the DB.

Read-only sync (DB → vault, plus one-shot import on setup) ships in Phase 17. Two-way sync (vault → DB via `inotify`) ships in Phase 17.5. Both directions follow the round-trip rules in spec §7.3.3: never destroy data, `:ID:` is the round-trip anchor, conflicts are surfaced not silenced (losers preserved at `<file>.atrium.bak.<timestamp>`), atomic writes (`write-temp + fsync + rename`).

Don't pivot to "vault is the storage." The §8 perf budget assumes SQLite indexes for Forecast and Review queries; Org-as-store can't hit those targets at 10K-task scale (org-roam itself uses a SQLite cache for the same reason). The vault is interop, not architecture below it.

## Dependency discipline

**No third-party crates without prior sign-off.** This is hard. The full v0.1 dependency set is locked in `roadmap.md` Phase 0:

> `gtk4`, `libadwaita`, `tokio`, `rusqlite` (`bundled`, `chrono` features), `serde`, `serde_json`, `chrono`, `anyhow`, `thiserror`, `tracing`, `tracing-subscriber`

Sign-off granted in subsequent phases:
- `uuid` (Phase 1) — UUID v4 generation for `:ID:` round-trip.
- `rrule` (Phase 15, v0.2.0) — RFC 5545 RRULE parsing + iteration for repeating tasks.
- `regex` (Phase 15.5, v0.4.0) — `tag:~regex` match modifier in the search expression language. Already transitively in the dep graph via `tracing-subscriber`; promoted to a direct dependency for `atrium-core`.

Pending dependency checks: `ical` / `rustical` (Phase 19).

Resolved against (won't be added):
- `orgize` / `starsector` (Phase 16, v0.7.6 dep-research pass) — both crates surveyed and rejected. orgize's last stable was 0.9.0 in 2021 with the active line in alpha since 2023; starsector's last release was October 2022 and pulls orgize-alpha as a transitive anyway. Phase 16 hand-rolls the Org subset (atrium-core/src/sync/org/), fitting the CalibreQuarry stdlib-only ethos. The "preserve unknown constructs verbatim" rule (spec §7.3.3 rule 1) is satisfied by capturing every unrecognised line into the task's `unknown_lines` field and re-emitting verbatim on write — easier in a focused passthrough parser than fighting either crate's AST.

If a task pushes you toward a crate that isn't already in `Cargo.toml`, **stop and ask** — don't add it speculatively, and don't hand-roll a wide subset to dodge the conversation.

## Spec discipline

The contract docs are the single most valuable artifact in this repo right now. When editing them:

- **Match the existing voice and structure.** `spec.md` uses numbered sections with short paragraphs and small tables; `roadmap.md` is a flat checkbox list grouped by phase with one italic tagline per phase. Don't reformat or restructure unprompted.
- **Cross-reference, don't duplicate.** If a fact is in `spec.md` §4, refer to it from `roadmap.md` rather than restating it. They drift if both contain the same claim.
- **Update sibling docs when one changes.** A schema change in `spec.md` §4 likely needs a Phase 1 roadmap update and a `patchnotes.md` entry. The README's "Architecture (in one paragraph)" and "Stack" sections must stay aligned with `spec.md` §3 and §8.
- **`VERSION` is the single source of truth.** `Cargo.toml` (once it exists) and the AppStream metainfo must match. Bumping a version means updating all three.

## Release discipline

Versioning and the documentation set move together. No silent changes, no deferred bookkeeping.

- **Every change earns a logical version bump.** Patch for fixes-only, minor for additive features that don't break the spec, major for spec-changing or breaking work. The `VERSION` bump rides with the change that earns it — never "we'll bump it later".
- **Every minor or major change updates all four docs.** `spec.md`, `roadmap.md`, `patchnotes.md`, and `VERSION` move in the same commit (or stacked commits within the same change). If you can't write the `patchnotes.md` line, the change isn't done. If `spec.md` semantics shifted, the matching `roadmap.md` phase item gets updated too — the cross-reference rule in "Spec discipline" still applies.
- **Patch releases still update `patchnotes.md` and `VERSION`.** They're allowed to skip `spec.md` / `roadmap.md` only when the fix doesn't change documented behavior or the plan.
- **Every major bump includes a maintenance pass.** Majors are the sanctioned moment to refactor what's gotten messy, clear deferred bugs, and prune dead code. Don't slip cleanup into minor releases as a side-quest, and don't let a major ship without it. Call out the maintenance work in `patchnotes.md` so it's visible.

## Schema rule (post-v0.2.0)

The v0.1 schema freeze ended with v0.2.0 — `ALTER TABLE` migrations are now allowed.

The discipline going forward: every migration is **append-only and backwards-compatible**. Never rewrite a shipped migration. Adding columns / tables / triggers / indexes is fine; renaming or dropping columns / tables is a major-bump-only operation (and even then, prefer a new column with a deprecation window over an in-place rename). Constraint changes that could fail on existing data — adding NOT NULL, changing FK targets, adding UNIQUE indexes — need a backfill step and explicit sign-off.

The v0.1 freeze's good instinct still applies: when a Builder-feature task seems to need a new column on an existing table, first check whether the column already exists in the v0.1 superset and the right move is just to expose it differently in the UI. The superset is rich; most "I need a column for this" instincts turn out not to need a migration.

## Build / test / lint (once code lands)

Phase 0 establishes the CI baseline; until then these commands have no targets to run against. From Phase 0 onward:

```bash
cargo test                      # all tests
cargo test <test_name>          # single test
cargo clippy -- -D warnings     # lint, warnings = errors
cargo fmt --check               # formatting check
```

CI runs all three on Linux. Tests are required from day one (Brandon's hard rule, repeated in `roadmap.md` Phase 0 and `spec.md` §10). Match the project's eventual test style; don't propose a separate one.

A Meson wrapper over Cargo lands in Phase 0 to make Flatpak packaging straightforward. Native development uses Cargo directly; Flatpak builds go through Meson.

## Application identifiers and paths

Lock these in early — they appear across `Cargo.toml`, the desktop entry, GSettings schema, AppStream metainfo, and the Flatpak manifest, and changing them later is painful:

- **App ID:** `io.github.virinvictus.atrium`
- **Database:** `$XDG_DATA_HOME/atrium/atrium.db`
- **Cache:** `$XDG_CACHE_HOME/atrium/`
- **Default Quick Entry shortcut:** `Ctrl+Alt+Space` (user-configurable via GSettings)

## Performance budget (spec.md §8)

Each phase ends with a `heaptrack`/`massif` checkpoint against:

- **Idle:** < 80 MB
- **Active:** < 200 MB on a 10K-task DB
- **Cold start:** < 250 ms on a 5K-task DB
- **Quick Entry latency:** < 50 ms shortcut → focused entry

Features that miss budget get gated or revised. If a proposed approach has obvious memory or latency risk, raise it before implementing.

## Sibling project context

When implementing the data layer, **`~/.gitrepos/Viaduct/`** is the reference for the single-writer SQLite worker pattern (Brandon ports the same discipline here, no WebKit). The README explicitly acknowledges this. Look at Viaduct's queue, command enum, and `TaskChanges`-equivalent delta type before designing Atrium's.

`~/.gitrepos/Hermitage/` and `~/.gitrepos/Framework/` are the other native GTK4/libadwaita apps in the portfolio — useful for cross-checking GTK idioms, Flatpak manifest shape, and AppStream metainfo conventions.

## Codebase map (current — v0.6.20)

Four workspace crates split by responsibility. The data layer (`atrium-core`), the search engine (`atrium-search`, extracted v0.4.2), and the headless CLI (`atrium-cli`, added v0.4.3) all stay GUI-free so the Phase 20 `atriumd` daemon and the post-1.0 TUI can reuse them without dragging GTK along.

```
atrium-search/                        ← Calibre-powered search engine (v0.4.2 — extracted from atrium-core)
├── src/lib.rs                        ← module root, re-exports (parse / evaluate / try_translate / Expr / …)
├── src/lex.rs                        ← Token enum + tokenizer
├── src/parse.rs                      ← recursive-descent parser → Expr AST + sort modifiers
├── src/ast.rs                        ← Expr + Field + State + MatchKind + Comparator + Value + DateKeyword + SortSpec
├── src/dates.rs                      ← (v0.5.3) date keyword + relative-day → concrete date resolution
├── src/eval.rs                       ← in-memory evaluator + EvalContext (lazy regex cache, Damerau-Levenshtein for fuzzy)
├── src/rank.rs                       ← (v0.5.2) FTS5 bm25 + recency factor for bare-text ranking
├── src/sql_translate.rs              ← (v0.5.3) Expr → SQL fast-path; SqlValue; in-memory fallback for regex / fuzzy / composite
└── src/tests.rs                      ← integration tests (parse + eval + translate round-trips)

atrium-cli/                           ← headless CLI (v0.4.3 → v0.6.5 — full task + perspective CRUD from the shell)
├── src/main.rs                       ← subcommand dispatch, DB open, EvalContext build, write paths
├── src/args.rs                       ← stdlib argv parser; Args / Format / Subcommand types
├── src/output.rs                     ← TSV / JSON / human-readable formatters (incl. kanban columns)
└── src/tests.rs                      ← argv parsing + output formatting tests

atrium-core/                          ← headless data layer
├── src/lib.rs                        ← re-exports (Task / WorkerHandle / RepeatRule / SqlBindValue / …)
├── src/paths.rs                      ← XDG path helpers, APP_ID
├── src/error.rs                      ← thiserror hierarchy (DbError::BadRepeatRule v0.2.0)
├── src/repeat.rs                     ← RFC 5545 RRULE wrapper, RepeatMode, CountStep (Phase 15)
├── src/quick_entry.rs                ← (v0.4.5) #tag / @today / @deadline parser, lifted from atrium binary so atrium-cli can reuse
├── src/render.rs                     ← (v0.5.4) Slice D foundation — kanban column projection from a saved Perspective
├── src/test_support.rs               ← dummy_task helpers behind `test-support` feature (v0.2.0 maintenance)
├── src/domain/                       ← Task / Project / Area / Tag / Perspective / ScheduledFor types
└── src/db/
    ├── worker.rs                     ← single-writer task; spawn_worker; regenerate-on-complete (Phase 15)
    ├── read_pool.rs                  ← read-only connection pool
    ├── read.rs                       ← list_inbox / list_today / list_forecast / list_review_queue / list_agenda / search / counts / tag_info_per_task
    ├── command.rs                    ← Command enum (Create/Update/Toggle/Delete/Perspective/MarkReviewed/MoveToColumn/…)
    ├── changes.rs                    ← TaskChanges, LibraryChanges deltas (perspectives_* added v0.1.17)
    ├── fixtures.rs                   ← --fixture stress generators
    └── migrations/
        ├── mod.rs                    ← migrate(&mut conn) runner; user_version PRAGMA (currently 5)
        ├── 0001_initial.sql          ← OmniFocus superset (Phase 1)
        ├── 0002_perspectives.sql     ← perspective table (Phase 14, v0.1.17, additive)
        ├── 0003_repeat_mode.sql      ← task.repeat_mode column (Phase 15, v0.2.0, first ALTER)
        ├── 0004_area_color.sql       ← (v0.5.0) area.color for per-area accent
        └── 0005_perspective_renderer.sql ← (v0.5.0) perspective.renderer + renderer_config (kanban / list)

atrium/                               ← GTK binary
├── build.rs                          ← compiles GSettings schema for cargo-only runs
├── src/main.rs                       ← Application, CLI flags, accels, action wiring, bridges
├── src/error.rs
├── src/ui/
│   ├── mod.rs
│   ├── window.rs                     ← AtriumWindow (composite template); ContextMode; build_context_resolver
│   ├── task_list.rs                  ← row factory, ActiveList, apply_changes_seq, TagPillMap (v0.3.0)
│   ├── task_object.rs                ← AtriumTask glib::Object wrapper (context_label, row_state)
│   ├── inspector.rs                  ← Simple-Mode modal Inspector (AdwDialog, Phase 7i)
│   ├── inspector_pane.rs             ← Builder-Mode side pane (Phase 10) + repeat editor (Phase 15)
│   ├── tag_editor.rs                 ← per-task tag editor (AdwDialog, Phase 7g)
│   ├── filter.rs                     ← thin window-side shim over atrium_search (v0.4.0); warnings + EvalContext glue + SQL fast-path bridge (v0.6.18)
│   ├── forecast.rs                   ← Phase 12 calendar-axis page; build_page + drag-to-reschedule + click-to-open (v0.6.17)
│   ├── review.rs                     ← Phase 13 project review queue
│   ├── logbook.rs                    ← (Slice C2) day-grouped Logbook page
│   ├── agenda.rs                     ← (v0.6.4 Slice D2) Agenda canonical page — Overdue / Today / Tomorrow / This Week / Next Week
│   ├── board.rs                      ← (v0.6.0–v0.6.3 Slice D1) kanban Perspective renderer + drag-drop column moves
│   ├── shortcuts.rs                  ← Ctrl+? / F1 dialog
│   ├── about.rs                      ← AdwAboutDialog
│   └── typography.rs                 ← bundled font install + CSS load
├── src/quickentry/
│   ├── mod.rs
│   └── modal.rs                      ← Quick Entry modal (adw::Window, fade-in); parser lives in atrium-core::quick_entry
└── src/debug/mod.rs                  ← Memory Watch + /proc/self/status sampler

data/                                 ← installed assets
├── window.ui                         ← composite template (sidebar_empty_hint added v0.2.2)
├── style.css                         ← typography + per-surface tweaks; v0.3.0 swatches; v0.6.10 soft-accent pass; v0.6.12 state-aware row treatment
├── fonts/                            ← Inter + Source Serif 4 + JetBrains Mono + Atkinson Hyperlegible
├── icons/hicolor/scalable/apps/io.github.virinvictus.atrium.svg
├── io.github.virinvictus.atrium.gschema.xml
├── io.github.virinvictus.atrium.desktop
├── io.github.virinvictus.atrium.metainfo.xml
└── io.github.virinvictus.atrium.yml  ← Flatpak manifest

docs/                                 ← long-form references
├── schema.md                         ← per-column rationale + ER diagram (covers migrations 0001–0005)
├── keymap.md                         ← canonical written shortcut map
├── accessibility.md                  ← Phase 8f audit findings + conventions
├── perf-baseline.md                  ← release-mode RSS baseline + cold-start measurements
├── regression.md                     ← ship-gate regression-script doc (incl. atrium-cli + kanban smoke)
└── gtd-patterns.md                   ← (Slice C) GTD patterns reference — Inbox flow, Weekly Review, contexts vs. tags

scripts/regression.sh                 ← ship-gate: fmt → clippy → test → atrium-cli smoke → kanban smoke

tests/                                ← integration tests
└── mode_flip_snapshot.rs             ← Phase 10 acceptance (mode flip never touches DB)
```

**Test counts as of v0.6.20:** 119 atrium (binary) + 173 atrium-core (lib) + 1 mode-flip integration + 106 atrium-search + 106 atrium-cli = **505 tests**. All green; `bash scripts/regression.sh` runs in under 2 seconds.

The dialog primitives standardised in the v0.0.37 bugsweep:

- **Inspector** (Simple Mode) + **Tag editor** are `adw::Dialog` (in-window modal overlay; `present(parent)` / `close()`).
- **Inspector pane** (Builder Mode) is an always-visible `AdwBin` mounted into the right-side `AdwOverlaySplitView` sidebar — non-modal, autosaves on focus-out.
- **Quick Entry** stays an `adw::Window` (`modal=false`, `transient_for(main)`, fade-in keyframe) — the spec wants it to *not* steal grab from previously-focused windows; AdwDialog always grabs.
- **Memory Watch** stays an `adw::Window` for the same reason (non-modal observer pane).
- **Confirmations** use `adw::AlertDialog`. The v0.3.0 tag-colour picker (`prompt_for_tag`) extends `AlertDialog` with a custom extra-child Box holding the swatch row.
