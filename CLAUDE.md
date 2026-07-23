# CLAUDE.md

Project guidance for Claude Code working on Atrium.

## Status

**Current release: v0.65.1** on `main` (July 2026). The `phase-22-de-adwaita` branch was merged back and deleted at v0.65.1; the vault-ledger fixes that shipped on the release line as v0.48.0 / v0.48.1 are recorded under their branch numbers, v0.60.0 / v0.60.1 (both lines independently minted a v0.48.0; the branch's is the de-adwaita re-sequence docs commit). **Schema version: 20** (migrations `0001` → `0020`; 0020 is the C9 swatch recolour). Full workspace suite green. **The Phase 22 de-adwaita ladder is complete (C1 → C10): Atrium is plain GTK4 with a self-contained owned Kanagawa Dragon stylesheet, zero libadwaita in the tree.** Display-verified and look approved by Brandon.

Phases 0 through 19.5 are complete: the full OmniFocus-superset data layer, dual Simple/Builder modes, Quick Entry, the Org vault two-way mirror, search, recurrence, subtasks, dependencies, templates, backup/restore, per-area review schedules, bulk editing, reminders with launch catch-up, and the non-Org importers (Todoist, Taskwarrior, todo.txt, VTODO, extracted into `atrium-import`). The kanban surface has matured through v0.46.0 (richer cards, per-column WIP limits, add-in-place, persisted intra-column order), preserving the projection column model (columns stay a projection of a tag or Org status; no first-class buckets, so boards still round-trip to Org). v0.46.1 / v0.46.2 were test-only fixes for a flaky CI: the `atrium-org` vault-watcher integration tests now poll for the expected end-state instead of waiting a fixed interval (v0.46.1), and are serialized via a file-level `tokio::sync::Mutex` so the harness can't run them in parallel and starve each other on a small runner (v0.46.2).

**Phase 20 (the 1.0 endgame) is in flight,** and its tail is re-sequenced as of v0.48.0. Localisation scaffolding shipped at v0.47.0 (gettext text domain `atrium`, `po/` + meson `i18n.gettext`, a full marking sweep of the GTK binary through `atrium/src/i18n.rs`, `en` as the first catalogue; spec §3.6). Two conventions started there: every new metainfo `<release><description>` carries `translate="no"`, and new `.rs` files with user-facing strings join `po/POTFILES`.

**Sequencing (Brandon, 2026-07-17): Phase 22 — the de-adwaita + Kanagawa Dragon re-theme — is pulled in front of the `v1.0.0` tag** rather than run post-1.0, because the remaining 1.0 assets (final icon, screenshots, Flathub metadata) are all invalidated by the toolkit swap. Pre-1.0 order is now: the de-adwaita sub-phase ladder (roadmap Phase 22, C1 foundations → C10 toolkit cut) → the icon/screenshots/Flathub asset tail → the `v1.0.0` tag. The pilot gate is satisfied (Colophon Phase 6 at v2.0.0; Conservatory Phase 26 at v0.3.8). Template: `~/.gitrepos/Conservatory/conservatory/src/theme.rs` + its 26b→26m ladder. Design language: spec §3.7. **Ladder complete: C1–C9 shipped (v0.50.0 → v0.62.0) and C10 (drop libadwaita) landed at v0.64.0; the branch merged back and was deleted at v0.65.1.** C8 and C9 were display-verified and the look approved. The one schema touch in the whole phase, migration `0020` (swatch recolour, UPDATE-only), shipped at C9. Phase 21 (the Hyprland audit) stays post-1.0; its number is lower than Phase 22 but its execution is later. Flathub readiness is now verifiable locally (flatpak-builder + GNOME Platform/Sdk 50 are installed); only the screenshot capture and the Flathub PR need Brandon's display/account.

**The per-release history lives in `patchnotes.md` (newest at top); do not restate it here.** When precision on a specific version matters, read that file, `roadmap.md`, and `VERSION`.

Seven workspace crates: `atrium-core` (data layer), `atrium-search` (Calibre-style search expression language), `atrium-org` (Org-mode projection), `atrium-inline` (inline-syntax parser, extracted v0.13.0), `atrium-import` (non-Org import/export formats, extracted v0.34.0), `atrium-cli` (headless CLI), and the `atrium` GTK4 binary.

The next-up plan lives in `roadmap.md`; the current front is the Phase 22 de-adwaita ladder (running inside the Phase 20 endgame, before the tag).

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
- **Hand-rolled stdlib parsers in `atrium-import`** (extracted from atrium-cli at v0.34.0). The Todoist importer (Phase 18) ships three stdlib-only parsers — CSV, NL recurrence, mapper. The VTODO importer (Phase 19, v0.25.0) adds a fourth at `atrium-import/src/vtodo/{parser,emit,mapper}.rs`: hand-rolled RFC 5545 line-unfolding + escape-decoding + property-tokenisation, no `ical` crate. The `ical` crate was evaluated at v0.25.0 and declined for consistency with the Org + Todoist precedents — the savings (tokenisation + line-folding + escape decoding) are bounded, and the Atrium-specific mapping layer is the bulk of the work regardless. No `csv` crate, no `regex` (pattern-matching by tokenised words for the small phrase set). Stay consistent.
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

That blockquote is the historical v0.1 lock, not the current tree: `libadwaita` was **removed** at Phase 22 C10 (v0.64.0) and nothing replaced it.

Sign-off granted in subsequent phases:

- `uuid` (Phase 1) — UUID v4 for `:ID:` round-trip; `v5` feature added v0.12.0 for deterministic Todoist UUIDs (pulls in sha1_smol).
- `rrule` (Phase 15) — RFC 5545 RRULE parsing + iteration.
- `regex` (Phase 15.5) — `tag:~regex` modifier; promoted to direct dep of `atrium-search`.
- `notify` (Phase 17) — cross-platform filesystem watcher; direct dep of `atrium-org`. Default features only — uses inotify on Linux.
- `gettext-rs` (Phase 20, added v0.47.0): localisation runtime. `gettext-system` feature only: it links glibc's built-in gettext rather than vendoring GNU gettext, which matters for CI and the Flatpak. Binary-only, never in the library crates.

Resolved against (won't be added): `orgize` / `starsector` (both dormant). The hand-rolled subset at `atrium-org/src/org/` is the answer. `ical` / `rustical` (evaluated at v0.25.0 and declined; the hand-rolled RFC 5545 parser at `atrium-import/src/vtodo/` is the answer, for the same reason).

Pending: `libecal` / `libedataserver` bindings, or a hand-rolled `zbus` client, for the read-only EDS calendar overlay. That is the one open Phase 19.5 item and the only dependency question still unanswered.

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
cargo test --workspace            # all tests (1007 at v0.38.3)
cargo test <test_name>            # single test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
bash scripts/regression.sh        # ship gate: fmt → clippy → test → atrium-cli + kanban smoke → cold-start sanity
bash scripts/perf.sh              # perf suite (v0.36.0): 50K/100K fixtures, read-path load + peak RSS, §8 budgets. Separate from the ship gate (heavy); run before tagging.
```

CI runs fmt + clippy + test on Linux. Tests are required from day one.

A Meson wrapper over Cargo lives at `meson.build` for Flatpak packaging. Native development uses Cargo directly; Flatpak builds go through Meson.

## Application identifiers and paths

- **App ID:** `io.github.virinvictus.atrium`
- **Database:** `$XDG_DATA_HOME/atrium/atrium.db`
- **Cache:** `$XDG_CACHE_HOME/atrium/`
- **Default Quick Entry shortcut:** `Ctrl+Alt+Space` (user-configurable via GSettings)
- **Default vault path:** unset (DB-only mode); set via `gsettings set io.github.virinvictus.atrium vault-path /path/to/vault`. A graphical Settings UI for this shipped in Phase 19.5 (v0.20.0 as `AdwPreferencesDialog`, replaced by a plain `gtk::Window` at Phase 22 C10).

## Performance budget (spec.md §8)

Each phase ends with a `heaptrack` / `massif` checkpoint against:

- **Idle:** < 80 MB
- **Active:** < 200 MB on a 10K-task DB
- **Cold start:** < 250 ms on a 5K-task DB
- **Quick Entry latency:** < 50 ms shortcut → focused entry

Features that miss budget get gated or revised. If a proposed approach has obvious memory or latency risk, raise it before implementing.

## Sibling project context

- **`~/.gitrepos/Viaduct/`** — the reference for the single-writer SQLite worker pattern. Look at the queue, command enum, and `TaskChanges`-equivalent delta shape before reinventing data-layer pieces.
- **`~/.gitrepos/Hermitage/` and `~/.gitrepos/Framework/`**: the other native GTK4 apps in the portfolio. Both dropped libadwaita too (Hermitage at v0.17.0, Framework at v0.80.0), so they are current cross-references for the plain-GTK4 idiom, not just for Flatpak manifest shape and AppStream metainfo conventions.

## Codebase map

Seven workspace crates split by responsibility. The data layer (`atrium-core`), search engine (`atrium-search`), Org projection (`atrium-org`), inline-syntax parser (`atrium-inline`), non-Org importers (`atrium-import`), and headless CLI (`atrium-cli`) all stay GUI-free so the Phase 20 `atriumd` daemon and the post-1.0 TUI can reuse them. atrium-core knows nothing about Org or inline syntax; both projections plug in through their own crates.

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

atrium-import/                        ← non-Org import/export formats (extracted from atrium-cli v0.34.0; consumed by the CLI + the GUI import dialog)
├── src/lib.rs                        ← `pub mod import; pub mod vtodo;` + re-export `UdaPolicy`
├── src/import/{todoist,taskwarrior,todotxt}/  ← CSV / JSON / plain-text importers (parser + mapper + round_trip_tests)
├── src/vtodo/{parser,emit,mapper}.rs ← VTODO `.ics` import + one-way export
└── tests/fixtures/                   ← moved here with the modules (round_trip_tests use CARGO_MANIFEST_DIR)

atrium-cli/                           ← headless CLI (full task + perspective CRUD + import/export)
├── src/main.rs                       ← subcommand dispatch; `use atrium_import::{import, vtodo};`
├── src/args.rs                       ← stdlib argv parser; re-exports `atrium_import::UdaPolicy`
├── src/output.rs                     ← TSV / JSON / human-readable formatters (incl. kanban columns)
└── src/export.rs                     ← `export org PATH` (vault writer) + `export json PATH` (snapshot)

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
    └── migrations/                   ← 0001 initial → 0019 board_card_position; user_version PRAGMA currently 19

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
├── src/ui/                           ← window/ + inspector_pane/ (module dirs, split v0.22.0), task list/object, inspector, tag editor, filter, forecast, review,
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
scripts/perf.sh                       ← perf regression suite (v0.36.0): 50K/100K fixtures, §8 budget assertions
```

## Dialog primitives (standardised v0.0.37; de-adwaita'd through Phase 22 C8)

The Phase 22 ladder replaced every adwaita dialog primitive with an owned or plain-GTK equivalent. Current state:

- **Inspector** (Simple Mode) + **Tag editor** are modal, transient `gtk::Window`s (C8; were `adw::Dialog`). An invisible `gtk::HeaderBar` titlebar suppresses GTK's default header; the in-content `gtk::HeaderBar` carries the buttons with `show-title-buttons=false`; `dialogs::close_on_escape` wires Escape-to-dismiss; `present()` / `close()`.
- **Inspector pane** (Builder Mode) is an always-visible `gtk::Box` host in the right-side `gtk::Paned` end child (C5d/C6; were `AdwBin` in an `AdwOverlaySplitView`) — non-modal, autosaves on focus-out.
- **Quick Entry** is a `gtk::Window` (`modal=false`, `transient_for(main)`; C8) — it must *not* steal grab from the previously-focused window (adwaita's dialogs always grabbed). Static "Quick Entry" title kept for the Hyprland window rule.
- **Memory Watch** is a `gtk::Window` for the same non-grab reason (C8; gained Escape-to-close).
- **Confirmations** use the owned `dialogs::Alert` (C4; was `adw::AlertDialog`) — named responses, per-response appearance, optional extra child, async `choose_future`. The tag-colour picker (`prompt_for_tag`) passes a swatch-row extra child.

There is no adwaita surface left: C10 (v0.64.0) dropped libadwaita entirely. `adw::Application` → `gtk::Application`; the `preferences.rs` theme apply sets GtkSettings' `gtk-application-prefer-dark-theme` (Atrium ships the dark Kanagawa sheet; a light Lotus palette is post-1.0).
