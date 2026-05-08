# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

**Simple Mode shipped (v0.1.0, May 2026).** Phases 0–9 complete. Atrium runs end-to-end: workspace scaffolding, schema + single-writer worker, application shell, all six canonical lists (Inbox / Today / Upcoming / Anytime / Someday / Logbook), areas + projects + tags + multi-tag, Quick Entry, FTS5 search + filter expressions, multi-select + undo, Inspector + tag editor dialogs, sidebar find-as-you-type, full keyboard map, typography + accessibility (Atkinson Hyperlegible), debug-pane Memory Watch, ship-gate regression script. Three Phase 9 follow-ups remain on Brandon's plate (the actual `v0.1.0` git tag, the Flatpak publish, the public announcement on `VirInvictus.github.io`); two Phase 8 carryovers also outstanding (README screenshots, Flatpak font verification under sandbox).

**Builder Mode shipped (v0.2.0, May 2026).** Phases 10–15 complete. Mode toggle + Inspector pane + project Sequential / Review extras (Phase 10), defer dates + sequential rendering (Phase 11), Forecast 30-day calendar-axis page (Phase 12), Review queue with Mark Reviewed (Phase 13), saved Perspectives in their own sidebar section (Phase 14, v0.1.17), and Repeating Tasks with full RFC 5545 RRULE support + three Org-mode completion semantics (Phase 15, v0.2.0). v0.2.0 ends the v0.1 schema freeze: `ALTER TABLE` migrations are now allowed and v0.2.0 ships the first one (`0003_repeat_mode.sql` adds `task.repeat_mode`).

**v0.2.x patches + v0.3.0 visual polish landed (May 2026).** v0.2.1 fixed the tag-pill update path and shipped the *Area › Project* row context chip. v0.2.2 was an audit-pass patch — filter-typo toast warnings, sidebar zero-state hint, screen-reader badge labels, Inbox chip fallback. v0.3.0 was a focused visual-polish minor: tag colours wired end-to-end (six-swatch picker, sidebar dots, Pango-coloured pills), row hover states, completion micro-animation, per-list empty-state warmth, sidebar section dividers, header-bar `Area › Project` breadcrumb, Inspector-pane card treatment.

**Phase 15.5 shipped at v0.4.0 (May 2026).** Calibre-powered search: the search bar's flat filter language grew into a full expression grammar — boolean operators (AND / OR / NOT, `NOT > AND > OR` precedence), parens grouping, comparison + range operators on date and numeric fields, date keywords (`thisweek`, `5daysago`, etc.), state predicates (`is:overdue`, `is:repeating`, etc.), and Calibre-style match modifiers (`tag:x` substring, `tag:=x` exact, `tag:~regex`, `tag:true`/false). Saved Perspectives inherit the new grammar for free since they store filter expressions verbatim. Full reference in `spec.md` §4.3.

**Search engine v0.4.x patch sequence in flight (May 2026).** v0.4.1 closed the Phase 15.5 deferred-list: canonical-list state predicates (`is:today` / `is:inbox` / `is:upcoming` / `is:anytime` / `is:someday`), `sort:KEY` / `sort:-KEY` modifier with primary→secondary composition, ↑/↓ search history (in-memory ring buffer, 20 entries), `?` operator-reference popover on the search bar, and the `tag:?word` fuzzy modifier (Damerau-Levenshtein with length-aware threshold). v0.4.2 extracted the engine into its own `atrium-search` workspace crate so the parser/evaluator live independently of the GTK binary and the SQLite layer. v0.4.3 (SQL pushdown for the predicates SQLite can express) and v0.4.4 (FTS5 bm25 + recency ranking on bare-text searches) remain.

**Phase 16 (Things 3 import) is what's next** — JSON via the `things:///add-json` URL scheme; importer module at `atrium-core/src/import/things3.rs`; mapping table per `spec.md` §7.

## Authoritative documents

- **`spec.md`** is the contract. Architecture (§3), schema (§4), UI deltas (§5), Quick Entry (§6), import/export mapping (§7), and the perf budget (§8) all live there. **Read it before changing semantics.** If a request conflicts with the spec, surface that conflict — don't quietly drift.
- **`roadmap.md`** is the 20-phase plan plus three sub-phases (12.5, 15.5, 17.5). Phases 0–9 → v0.1 (Simple Mode). Phases 10–15 → v0.2 (Builder Mode). Phase 15.5 → Calibre-powered search. Phases 16–19 → import/export. Phase 20 → v1.0. **Don't skip phases or pull work forward** without explicit go-ahead — Brandon sequenced these deliberately to keep each release shippable.
- **`patchnotes.md`** — newest at top. v0.1.0 was the first user-facing tag; v0.3.0 is the most recent release (visual polish minor).

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

Pending dependency checks: `orgize` (Phase 17), `ical` / `rustical` (Phase 19).

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

## Codebase map (current — v0.4.x)

Three workspace crates split by responsibility. The data layer (`atrium-core`) and the search engine (`atrium-search`, extracted v0.4.2) both stay GUI-free so the Phase 20 `atriumd` daemon and the post-1.0 TUI can reuse them without dragging GTK along.

```
atrium-search/                        ← Calibre-powered search engine (v0.4.2 — extracted from atrium-core)
├── src/lib.rs                        ← module root, re-exports (parse / evaluate / Expr / …)
├── src/lex.rs                        ← Token enum + tokenizer
├── src/parse.rs                      ← recursive-descent parser → Expr AST + sort modifiers
├── src/ast.rs                        ← Expr + Field + State + MatchKind + Comparator + Value + DateKeyword + SortSpec
├── src/eval.rs                       ← in-memory evaluator + EvalContext (lazy regex cache, Damerau-Levenshtein for fuzzy)
└── src/tests.rs                      ← integration tests (parse + eval round-trips)

atrium-core/                          ← headless data layer
├── src/lib.rs                        ← re-exports (Task / WorkerHandle / RepeatRule / …)
├── src/paths.rs                      ← XDG path helpers, APP_ID
├── src/error.rs                      ← thiserror hierarchy (DbError::BadRepeatRule v0.2.0)
├── src/repeat.rs                     ← RFC 5545 RRULE wrapper, RepeatMode, CountStep (Phase 15)
├── src/test_support.rs               ← dummy_task helpers behind `test-support` feature (v0.2.0 maintenance)
├── src/domain/                       ← Task / Project / Area / Tag / Perspective / ScheduledFor types
└── src/db/
    ├── worker.rs                     ← single-writer task; spawn_worker; regenerate-on-complete (Phase 15)
    ├── read_pool.rs                  ← read-only connection pool
    ├── read.rs                       ← list_inbox / list_today / list_forecast / list_review_queue / search / counts / tag_info_per_task (v0.3.0)
    ├── command.rs                    ← Command enum (Create/Update/Toggle/Delete/Perspective/MarkReviewed/…)
    ├── changes.rs                    ← TaskChanges, LibraryChanges deltas (perspectives_* added v0.1.17)
    ├── fixtures.rs                   ← --fixture stress generators
    └── migrations/
        ├── mod.rs                    ← migrate(&mut conn) runner; user_version PRAGMA
        ├── 0001_initial.sql          ← OmniFocus superset (Phase 1)
        ├── 0002_perspectives.sql     ← perspective table (Phase 14, v0.1.17, additive)
        └── 0003_repeat_mode.sql      ← task.repeat_mode column (Phase 15, v0.2.0, first ALTER)

atrium/                               ← GTK binary
├── build.rs                          ← compiles GSettings schema for cargo-only runs
├── src/main.rs                       ← Application, CLI flags, accels, action wiring, bridges
├── src/error.rs
├── src/ui/
│   ├── mod.rs
│   ├── window.rs                     ← AtriumWindow (composite template); ContextMode; build_context_resolver
│   ├── task_list.rs                  ← row factory, ActiveList, apply_changes_seq, TagPillMap (v0.3.0)
│   ├── task_object.rs                ← AtriumTask glib::Object wrapper (context_label v0.2.1)
│   ├── inspector.rs                  ← Simple-Mode modal Inspector (AdwDialog, Phase 7i)
│   ├── inspector_pane.rs             ← Builder-Mode side pane (Phase 10) + repeat editor (Phase 15)
│   ├── tag_editor.rs                 ← per-task tag editor (AdwDialog, Phase 7g)
│   ├── filter.rs                     ← thin window-side shim over atrium_core::search (v0.4.0); warnings + EvalContext glue
│   ├── forecast.rs                   ← Phase 12 calendar-axis page; build_page + drag-to-reschedule
│   ├── review.rs                     ← Phase 13 project review queue
│   ├── shortcuts.rs                  ← Ctrl+? / F1 dialog
│   ├── about.rs                      ← AdwAboutDialog
│   └── typography.rs                 ← bundled font install + CSS load
├── src/quickentry/
│   ├── mod.rs
│   ├── modal.rs                      ← Quick Entry modal (adw::Window, fade-in)
│   └── parser.rs                     ← #tag / @today / @deadline parser
└── src/debug/mod.rs                  ← Memory Watch + /proc/self/status sampler

data/                                 ← installed assets
├── window.ui                         ← composite template (sidebar_empty_hint added v0.2.2)
├── style.css                         ← typography + per-surface tweaks; v0.3.0 swatches, hover, completion-pop
├── fonts/                            ← Inter + Source Serif 4 + JetBrains Mono + Atkinson Hyperlegible
├── icons/hicolor/scalable/apps/io.github.virinvictus.atrium.svg
├── io.github.virinvictus.atrium.gschema.xml
├── io.github.virinvictus.atrium.desktop
├── io.github.virinvictus.atrium.metainfo.xml
└── io.github.virinvictus.atrium.yml  ← Flatpak manifest

docs/                                 ← long-form references
├── schema.md                         ← per-column rationale + ER diagram
├── keymap.md                         ← canonical written shortcut map
├── accessibility.md                  ← Phase 8f audit findings + conventions
├── perf-baseline.md                  ← release-mode RSS baseline (Phase 8g)
└── regression.md                     ← Phase 9a regression-script doc

scripts/regression.sh                 ← ship-gate: fmt → clippy → test → smoke

tests/                                ← integration tests
└── mode_flip_snapshot.rs             ← Phase 10 acceptance (mode flip never touches DB)
```

**Test counts as of v0.4.0:** 82 atrium (binary) + 165 atrium-core (lib, +41 from search module) + 1 mode-flip integration = 248 tests.

The dialog primitives standardised in the v0.0.37 bugsweep:

- **Inspector** (Simple Mode) + **Tag editor** are `adw::Dialog` (in-window modal overlay; `present(parent)` / `close()`).
- **Inspector pane** (Builder Mode) is an always-visible `AdwBin` mounted into the right-side `AdwOverlaySplitView` sidebar — non-modal, autosaves on focus-out.
- **Quick Entry** stays an `adw::Window` (`modal=false`, `transient_for(main)`, fade-in keyframe) — the spec wants it to *not* steal grab from previously-focused windows; AdwDialog always grabs.
- **Memory Watch** stays an `adw::Window` for the same reason (non-modal observer pane).
- **Confirmations** use `adw::AlertDialog`. The v0.3.0 tag-colour picker (`prompt_for_tag`) extends `AlertDialog` with a custom extra-child Box holding the swatch row.
