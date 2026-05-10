# CLAUDE.md

Project guidance for Claude Code working on Atrium.

## Status

**Current release: v0.19.0** (May 2026). **Schema version: 11.** **All workspace tests green; `bash scripts/regression.sh` ships clean.** **Phase 18.5 is now complete** — all five Tier-1 items + both Tier-2 items shipped across v0.14.0 → v0.19.0. Phase 19.5 productivity essentials (AdwPreferencesWindow, system notifications, EDS calendar overlay, etc.) are next.

Five workspace crates: `atrium-core` (data layer), `atrium-search` (Calibre-style search expression language), `atrium-org` (Org-mode projection), `atrium-inline` (inline-syntax parser, extracted v0.13.0), `atrium-cli` (headless CLI), and the `atrium` GTK4 binary.

For shipped-phase history, see `patchnotes.md` (newest at top); the next-up plan lives in `roadmap.md`. **Phase 18.5** (Org-mode power features) and **Phase 19.5** (productivity essentials) are next.

**Architectural commitment: every non-GUI surface stays CLI-testable.** The data layer, search engine, and import/export pipelines all run through `atrium-cli` (or future siblings like `atriumd`, the post-1.0 `atrium-tui`). Don't add functionality to the GTK binary that can't be reached from the shell.

## Authoritative documents

- **`spec.md`** — the contract. Architecture (§3), schema (§4), UI deltas (§5), Quick Entry (§6), import/export mapping (§7), perf budget (§8). Read it before changing semantics. If a request conflicts with the spec, surface that — don't quietly drift.
- **`roadmap.md`** — the 20-phase plan plus four sub-phases (12.5, 15.5, 15.75, 19.5). Don't skip phases or pull work forward without explicit go-ahead.
- **`patchnotes.md`** — newest at top.

## Architectural commitments (don't drift)

These five decisions are load-bearing. Code that contradicts them is wrong even if it compiles and passes tests.

### 1. Mode-as-View

Mode (Simple / Builder) is a **GSettings flag plus UI-layer rendering choices** — nothing more. It does not affect schema, does not migrate data, does not hide rows, does not constrain Quick Entry. The schema is the **OmniFocus superset** on day one; every Builder column (`defer_until`, `estimated_minutes`, `sequential`, `review_interval_days`, `last_reviewed_at`, `repeat_rule`, `parent_id`) exists from `0001_initial.sql`. Simple Mode hides those fields in the editor and in derived views; it does not lack them.

The Phase 10 acceptance test (`atrium-core/tests/mode_flip_snapshot.rs`) enforces this — flipping mode must not touch the DB.

### 2. Single-writer SQLite worker

A dedicated `tokio` task owns the writable `rusqlite::Connection`. The GTK thread holds an `mpsc::Sender<Command>` and **never** touches the writable connection. Reads use a separate read-only connection pool (`PRAGMA query_only = ON` per connection). WAL mode is mandatory. UI updates arrive as `TaskChanges { created, updated, deleted, status_changed }` and `LibraryChanges` deltas via a `glib::MainContext` channel — **never as full reloads**.

Pattern lifted directly from Viaduct's `DatabaseQueue` (`~/.gitrepos/Viaduct/`). When designing data-layer changes, look there for the pattern's shape.

### 3. Local-first, no network sync

SQLite at `$XDG_DATA_HOME/atrium/atrium.db`. No CalDAV client, no cloud, no telemetry, no network calls in v1.0. VTODO export (Phase 19) is a one-way file dump — explicitly **not** a CalDAV client. Local file mirroring (the Org vault, see commitment #5) is fine — that's filesystem IO, not network sync. Network-sync feature requests are out of scope through 1.0.

### 4. Debug-first architecture

Testing and debugging tooling is **built into the binary**, not bolted on. The `--debug` flag opens an in-app debug surface for stress generators (1K / 10K / 50K / 100K-task fixtures), edge-case fixtures, IO instrumentation (rusqlite's `trace` feature routes every SQL statement into a `tracing` span — no new crates), and a Memory Watch pane (`/proc/self/status` sampler).

Release builds ship the same code paths — heavy generators are gated on `--debug` so end users never see them, but the wiring is always present. Tests reuse the same fixtures; don't fork a separate "test-only" path.

### 5. Vault projection, not alternative store

When configured, an Org vault (default `~/Tasks/`, set via the `vault-path` GSettings key) mirrors task state to `.org` files for editing in any Org-aware tool. Discipline: **DB canonical, vault projected** — SQLite is the source of truth, the vault is downstream. Atrium runs cleanly without a vault; the vault never runs without the DB.

Both directions follow the round-trip rules in spec §7.3.3: never destroy data, `:ID:` is the round-trip anchor, conflicts are surfaced not silenced (losers preserved at `<file>.atrium.bak.<timestamp>`), atomic writes (`write-temp + fsync + rename`).

Don't pivot to "vault is the storage." The §8 perf budget assumes SQLite indexes for Forecast and Review queries; Org-as-store can't hit those targets at 10K-task scale (`org-roam` itself uses a SQLite cache for the same reason).

## Project tricks worth remembering

The non-obvious mechanics that aren't visible from the code alone:

- **Hand-rolled Org parser, not a crate.** `orgize` and `starsector` were both surveyed at Phase 16 and rejected as dormant. The hand-roll lives at `atrium-org/src/org/`. The "preserve unknown constructs verbatim" rule (spec §7.3.3 rule 1) is satisfied by capturing every unrecognised line into `unknown_lines` and re-emitting on write. Don't add an Org crate without explicit re-discussion.
- **Hand-rolled TOML, not the `toml` crate.** Same ethos as the Org parser. The vault sidecar (`atrium-org/src/sidecar.rs`) is small (top-level scalars + one level of `[section]` with string-string entries). If the schema ever needs arrays or nested tables, that's a re-discussion before adding `toml`.
- **Hand-rolled stdlib parsers in atrium-cli.** The Todoist importer (Phase 18) ships three stdlib-only parsers — CSV, NL recurrence, mapper. No `csv` crate, no `regex` (pattern-matching by tokenised words for the small phrase set). Stay consistent.
- **Test-file split pattern.** When a `#[cfg(test)] mod tests` body in a source file gets unwieldy, split it out via `#[cfg(test)] #[path = "<name>_tests.rs"] mod tests;` at the bottom of the source file. Same compilation, same coverage; halves the file size for editing. See `atrium-core/src/db/worker.rs` + `worker_tests.rs`.
- **VaultWriter debounce shape.** ~100 ms debounce window with a 50 ms tick. Receiving a `ProjectDirty(project_id)` extends that project's deadline (last-deadline-wins coalescing); the tick fires writes for projects past their deadline. Channel is `mpsc` (single consumer); under absurd load `try_send` drops rather than blocks.
- **VaultWatcher self-write filter is mtime-based, not path-TTL-based.** The first design recorded `(path, recorded_at)` and matched on path within a TTL — it lost external edits inside the TTL window. Fixed design: `RecentWrites` stores `(path, mtime_just_written)`; the watcher reads the file's actual mtime and matches on exact tuple equality. Linux ext4 stores nanosecond mtimes so two distinct writes never collide. **Don't revert to a path-only filter** — it's been tried; it loses external edits.
- **Atomic-write helper.** `atrium-core/src/sync/atomic.rs` does `write-temp + fsync + rename` for every vault write. Crash-safe; non-Org consumers (JSON snapshot) use it too. **Never** write a vault file without going through it.
- **Post-write integrity check.** Every `emit_org_file_with_meta` re-reads the file and verifies it parses cleanly through Atrium's own reader; failure propagates as `io::Error`. Catches emitter regressions immediately.
- **SQL-translation fast-path.** `atrium_search::sql_translate::try_translate(&Expr, today)` converts an `Expr` to a SQL `WHERE` fragment + bound params when every node maps cleanly. Returns `None` for `~regex`, fuzzy `?word`, `is:today`, and `Field::Project|Area` — the in-memory evaluator is the fallback. Both GUI and CLI use this; parity is pinned by integration tests in `atrium-search`.
- **`modified_at` triggers with `WHEN old = new`.** The triggers prevent recursion *and* let explicit writes survive — important for import-time timestamp preservation. Don't drop the `WHEN` clause.
- **`ScheduledFor` enum, not string.** Schema's "TEXT (ISO date OR `__someday__` sentinel)" maps to a Rust enum (`Someday | Date(NaiveDate)`) via custom `ToSql` / `FromSql`. Type-safe at the boundary; round-trip-clean. Don't reach for the raw string.
- **`NewTask.completed_at` is additive.** When the importer parses a source CLOSED cookie, it threads the timestamp directly into `NewTask.completed_at` instead of calling `toggle_complete` after create (which would stamp `now()`). All `NewTask` call sites need to set or default it; the GUI undo path also threads it.
- **`task.orig_keyword` (migration 0007) preserves non-canonical Org keywords.** Atrium's domain has TODO/DONE only; WAITING / BLOCKED / IN-PROGRESS / CANCELLED stash in `orig_keyword` so headlines round-trip without losing their label. The Org writer's lookup checks `orig_keyword` first, then falls back to TODO/DONE.
- **`spawn_vault_loop` is two-step.** The Phase 17 GUI builder can't be one call: the watcher needs a `WorkerHandle` to dispatch incoming changes through, and the worker needs a `VaultConfig` (containing the writer-side notifier) to install the projection. Shape: `spawn_vault_loop(root, pool)` builds the writer-side and shared `RecentWrites` up front, returns `(VaultConfig, VaultLoopHandle, events_rx)`. Caller passes `VaultConfig` into `spawn_worker_with_vault`, then feeds the resulting handle into `VaultLoopHandle::attach_watcher`. Don't try to collapse this back to one call.
- **Conflict-detection backup format is `<file>.atrium.bak.<YYYYMMDDTHHMMSSZ>`.** Filesystem-safe (no colons), UTC, sortable. Don't use RFC 3339 with colons — it works on Linux ext4 but is unreliable on FAT32 / SMB shares.
- **`:RRULE:` is canonical; the SCHEDULED cookie is best-fit projection.** Spec §7.3.3 rule 3. `task.repeat_rule` carries the full RFC 5545 RRULE; the Org cookie's `+1w` / `++1w` / `.+1w` is a lossy summary the writer projects from canonical. When the user edits ONLY the cookie in Emacs, divergence detection fires and the watcher rewrites the file from canonical. When the user edits ONLY `:RRULE:` (adding a BY-clause the cookie can't express), no divergence — the watcher syncs the new rule to DB. **Don't try to make the cookie carry BY-clause information** — Org cookies can only encode FREQ + INTERVAL.

## Dependency discipline

**No third-party crates without prior sign-off.** The full v0.1 dependency set is locked in `roadmap.md` Phase 0:

> `gtk4`, `libadwaita`, `tokio`, `rusqlite` (`bundled`, `chrono` features), `serde`, `serde_json`, `chrono`, `anyhow`, `thiserror`, `tracing`, `tracing-subscriber`

Sign-off granted in subsequent phases:

- `uuid` (Phase 1) — UUID v4 for `:ID:` round-trip; `v5` feature added v0.12.0 for deterministic Todoist UUIDs (pulls in sha1_smol).
- `rrule` (Phase 15) — RFC 5545 RRULE parsing + iteration.
- `regex` (Phase 15.5) — `tag:~regex` modifier; promoted to direct dep of `atrium-search`.
- `notify` (Phase 17) — cross-platform filesystem watcher; direct dep of `atrium-org`. Default features only — uses inotify on Linux.

Pending: `ical` / `rustical` (Phase 19).

Resolved against (won't be added): `orgize` / `starsector` (both dormant). The hand-rolled subset at `atrium-org/src/org/` is the answer.

If a task pushes you toward a crate that isn't already in `Cargo.toml`, **stop and ask** — don't add it speculatively, and don't hand-roll a wide subset to dodge the conversation.

## Spec discipline

The contract docs are the most valuable artifact in this repo. When editing them:

- **Match the existing voice and structure.** `spec.md` uses numbered sections with short paragraphs and small tables; `roadmap.md` is a flat checkbox list grouped by phase with one italic tagline per phase. Don't reformat or restructure unprompted.
- **Cross-reference, don't duplicate.** If a fact is in `spec.md` §4, refer to it from `roadmap.md` rather than restating it.
- **Update sibling docs when one changes.** A schema change in `spec.md` §4 likely needs a roadmap update and a `patchnotes.md` entry. The README's "Architecture (in one paragraph)" and "Stack" sections must stay aligned with `spec.md` §3 and §8.
- **`VERSION` is the single source of truth.** `Cargo.toml` and the AppStream metainfo must match. Bumping a version means updating all three.

## Release discipline

Versioning and the documentation set move together. No silent changes.

- **Every change earns a logical version bump.** Patch for fixes-only, minor for additive features that don't break the spec, major for spec-changing or breaking work. The `VERSION` bump rides with the change that earns it — never "we'll bump it later".
- **Every minor or major change updates all four docs.** `spec.md`, `roadmap.md`, `patchnotes.md`, and `VERSION` move in the same commit (or stacked commits within the same change). If you can't write the `patchnotes.md` line, the change isn't done.
- **Patch releases still update `patchnotes.md` and `VERSION`.** They can skip `spec.md` / `roadmap.md` only when the fix doesn't change documented behavior or the plan.
- **Every major bump includes a maintenance pass.** Majors are the sanctioned moment to refactor, clear deferred bugs, and prune dead code. Don't slip cleanup into minor releases as a side-quest, and don't let a major ship without it.

## Schema rule (post-v0.2.0)

The v0.1 schema freeze ended at v0.2.0 — backwards-compatible `ALTER TABLE` migrations are now allowed.

Discipline: every migration is **append-only and backwards-compatible**. Never rewrite a shipped migration. Adding columns / tables / triggers / indexes is fine; renaming or dropping is a major-bump-only operation (and even then, prefer a new column with a deprecation window over an in-place rename). Constraint changes that could fail on existing data — adding NOT NULL, changing FK targets, adding UNIQUE indexes — need a backfill step and explicit sign-off.

The v0.1 freeze's good instinct still applies: when a feature seems to need a new column, first check whether the column already exists in the OmniFocus superset and the right move is exposing it differently in the UI.

## Build / test / lint

```bash
cargo test --workspace            # all tests (817 at v0.13.0)
cargo test <test_name>            # single test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
bash scripts/regression.sh        # ship gate: fmt → clippy → test → atrium-cli + kanban smoke → cold-start sanity
```

CI runs fmt + clippy + test on Linux. Tests are required from day one.

A Meson wrapper over Cargo lives at `meson.build` for Flatpak packaging. Native development uses Cargo directly; Flatpak builds go through Meson.

## Application identifiers and paths

- **App ID:** `io.github.virinvictus.atrium`
- **Database:** `$XDG_DATA_HOME/atrium/atrium.db`
- **Cache:** `$XDG_CACHE_HOME/atrium/`
- **Default Quick Entry shortcut:** `Ctrl+Alt+Space` (user-configurable via GSettings)
- **Default vault path:** unset (DB-only mode); set via `gsettings set io.github.virinvictus.atrium vault-path /path/to/vault`. A graphical Settings UI for this lands in Phase 19.5's `AdwPreferencesWindow`.

## Performance budget (spec.md §8)

Each phase ends with a `heaptrack` / `massif` checkpoint against:

- **Idle:** < 80 MB
- **Active:** < 200 MB on a 10K-task DB
- **Cold start:** < 250 ms on a 5K-task DB
- **Quick Entry latency:** < 50 ms shortcut → focused entry

Features that miss budget get gated or revised. If a proposed approach has obvious memory or latency risk, raise it before implementing.

## Sibling project context

- **`~/.gitrepos/Viaduct/`** — the reference for the single-writer SQLite worker pattern. Look at the queue, command enum, and `TaskChanges`-equivalent delta shape before reinventing data-layer pieces.
- **`~/.gitrepos/Hermitage/` and `~/.gitrepos/Framework/`** — the other native GTK4 / libadwaita apps in the portfolio. Useful for cross-checking GTK idioms, Flatpak manifest shape, and AppStream metainfo conventions.

## Codebase map

Six workspace crates split by responsibility. The data layer (`atrium-core`), search engine (`atrium-search`), Org projection (`atrium-org`), inline-syntax parser (`atrium-inline`), and headless CLI (`atrium-cli`) all stay GUI-free so the Phase 20 `atriumd` daemon and the post-1.0 TUI can reuse them. atrium-core knows nothing about Org or inline syntax; both projections plug in through their own crates.

```
atrium-inline/                        ← inline-syntax parser shared by every capture surface (extracted v0.13.0)
├── src/lib.rs                        ← `parse_with_today` + `ParsedEntry` (`#tag` / `@today` / `@<weekday>` / `@deadline` / `!N`)
└── src/completions.rs                ← `CompletionContext` + `context_at` + `replace_token` + `matches` + `SCHEDULE_KEYWORDS` + `PRIORITY_LEVELS`

atrium-search/                        ← Calibre-powered search engine (extracted v0.4.2)
├── src/lex.rs                        ← Token enum + tokenizer
├── src/parse.rs                      ← recursive-descent parser → Expr AST + sort modifiers
├── src/ast.rs                        ← Expr + Field + State + MatchKind + Comparator + Value + DateKeyword + SortSpec
├── src/dates.rs                      ← date keyword + relative-day → concrete date resolution
├── src/eval.rs                       ← in-memory evaluator + EvalContext (lazy regex cache, Damerau-Levenshtein for fuzzy)
├── src/rank.rs                       ← FTS5 bm25 + recency factor
├── src/sql_translate.rs              ← Expr → SQL fast-path; in-memory fallback for regex / fuzzy / composite
└── src/tests.rs                      ← parse + eval + translate round-trips

atrium-cli/                           ← headless CLI (full task + perspective CRUD + Phase 16/18 import/export)
├── src/main.rs                       ← subcommand dispatch, DB open, EvalContext build, write paths
├── src/args.rs                       ← stdlib argv parser; ImportSource::Org | Todoist { project_name }
├── src/output.rs                     ← TSV / JSON / human-readable formatters (incl. kanban columns)
├── src/export.rs                     ← `export org PATH` (vault writer) + `export json PATH` (snapshot)
└── src/import/
    ├── mod.rs
    └── todoist/                      ← Phase 18 Todoist CSV importer (stdlib-only)
        ├── parser.rs                 ← CSV → `Vec<TodoistRow>` (Meta / Section / Task / Blank)
        ├── recurrence.rs             ← NL phrasing → RFC 5545 RRULE + scheduled anchor
        └── mapper.rs                 ← row stream → worker calls + `ImportSummary`

atrium-core/                          ← headless data layer
├── src/lib.rs                        ← re-exports (Task / WorkerHandle / VaultConfig / VaultDirtyNotifier / spawn_worker / spawn_worker_with_vault / RepeatRule / …)
├── src/paths.rs                      ← XDG path helpers, APP_ID
├── src/error.rs                      ← thiserror hierarchy
├── src/repeat.rs                     ← RFC 5545 RRULE wrapper, RepeatMode, CountStep
├── src/render.rs                     ← kanban column projection from a saved Perspective
├── src/test_support.rs               ← dummy_task helpers behind `test-support` feature
├── src/domain/                       ← Task / Project / Area / Tag / Heading / Perspective / ScheduledFor / NewTask
├── src/sync/
│   ├── atomic.rs                     ← write-temp + fsync + rename helper used by every vault write
│   └── json.rs                       ← `Snapshot` type + `export_json`; lossless versioned DB dump
└── src/db/
    ├── worker.rs                     ← single-writer task; spawn / spawn_with_vault; vault_notifier ping after every commit
    ├── worker_tests.rs               ← tests submodule loaded via #[path = "worker_tests.rs"] mod tests
    ├── vault_hook.rs                 ← `VaultDirtyNotifier` trait + thin `VaultConfig` — the projection contract
    ├── read_pool.rs                  ← read-only connection pool
    ├── read.rs                       ← list_inbox / list_today / list_forecast / list_review_queue / list_agenda / search / counts
    ├── command.rs                    ← Command enum
    ├── changes.rs                    ← TaskChanges, LibraryChanges deltas
    ├── fixtures.rs                   ← --fixture stress generators
    └── migrations/                   ← 0001 initial → 0007 task.orig_keyword; user_version PRAGMA currently 7

atrium-org/                           ← Phase 16 Org-mode projection + Phase 17 vault → DB sync
├── src/lib.rs                        ← VaultEvent + RecentWrites + sidecar re-exports; `spawn_org_vault` (write-only); `spawn_vault_loop` (two-way GUI builder)
├── src/vault_writer.rs               ← `VaultWriter` task — debounced project flushes; pre-flush conflict check copies external edits to <file>.atrium.bak.<UTC>; refreshes sidecar via `last_sidecar` cache
├── src/vault_watcher.rs              ← `VaultWatcher` task — `notify` v8 backend; debounces 200 ms; consults RecentWrites to suppress self-writes; reader→DB diff by `:ID:` (CREATE / UPDATE / DELETE); ParseFailed / ParseRecovered / FileRemoved / RruleDiverged events
├── src/self_write.rs                 ← `RecentWrites` — bounded TTL set of (path, mtime) keyed on exact tuple equality. Shared via Arc<RwLock<>> between writer + watcher
├── src/sidecar.rs                    ← `<vault>/.atrium/config.toml` — Sidecar struct + emit_text/parse_text + read/write helpers + build_from_db. Hand-rolled minimal TOML; tag colours round-tripped
├── src/rrule_cookie.rs               ← `rrule_to_org_cookie` / `rrule_to_org_repeater` / `org_repeater_to_rrule` / `cookie_matches_rrule`. Pure helpers — RRULE ↔ Org cookie projection
└── src/org/
    ├── mod.rs                        ← OrgFile / OrgHeadline / OrgKeyword / parse_org_file / emit_org_file + post-write integrity check
    ├── parse.rs                      ← hand-rolled headline / cookie / properties / body / nested-subtask parser
    ├── emit.rs                       ← inverse — emits stable, org-agenda-readable output
    ├── import.rs                     ← single-file + multi-file vault importer; uses WorkerHandle::ensure_area
    └── write.rs                      ← project → .org file writer; `build_project_tree` interleaves heading rows + tasks by `position`

atrium/                               ← GTK binary
├── build.rs                          ← compiles GSettings schema for cargo-only runs
├── src/main.rs                       ← Application; boot_data_layer reads vault-path GSettings → spawn_worker_with_vault
├── src/ui/                           ← window, task list/object, inspector + inspector_pane, tag editor, filter, forecast, review,
│                                       perspective_editor, logbook, agenda, calendar, board, inline_complete, shortcuts, about, typography
├── src/quickentry/modal.rs           ← Quick Entry modal (adw::Window, fade-in); parser lives in atrium-inline
└── src/debug/mod.rs                  ← Memory Watch + /proc/self/status sampler

data/                                 ← installed assets
├── window.ui                         ← composite template
├── style.css                         ← typography + per-surface tweaks
├── fonts/                            ← Inter + Source Serif 4 + JetBrains Mono + Atkinson Hyperlegible (SIL OFL)
├── icons/hicolor/scalable/apps/io.github.virinvictus.atrium.svg
├── io.github.virinvictus.atrium.gschema.xml ← includes vault-path key
├── io.github.virinvictus.atrium.desktop
├── io.github.virinvictus.atrium.metainfo.xml
└── io.github.virinvictus.atrium.yml  ← Flatpak manifest

docs/                                 ← long-form references (schema.md / keymap.md / accessibility.md / perf-baseline.md / regression.md / gtd-patterns.md / org-roundtrip.md)
demos/showcase/                       ← hand-crafted Org fixture: 3 projects / 42 tasks / every keyword + cookie + repeater + body construct + Unicode
scripts/regression.sh                 ← ship-gate
```

## Dialog primitives (standardised v0.0.37)

- **Inspector** (Simple Mode) + **Tag editor** are `adw::Dialog` (in-window modal overlay; `present(parent)` / `close()`).
- **Inspector pane** (Builder Mode) is an always-visible `AdwBin` in the right-side `AdwOverlaySplitView` sidebar — non-modal, autosaves on focus-out.
- **Quick Entry** is `adw::Window` (`modal=false`, `transient_for(main)`, fade-in keyframe) — the spec wants it to *not* steal grab from the previously-focused window; AdwDialog always grabs.
- **Memory Watch** is `adw::Window` for the same non-grab reason.
- **Confirmations** use `adw::AlertDialog`. The tag-colour picker (`prompt_for_tag`) extends `AlertDialog` with a custom extra-child Box for the swatch row.
