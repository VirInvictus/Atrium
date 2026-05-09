# CLAUDE.md

Project guidance for Claude Code working on Atrium.

## Status

**Current release: v0.10.3** (May 2026). **Phase 17 (vault → DB two-way sync) is closed at v0.10.3.** Phase 16 (Org-mode import + DB → vault writer) shipped at v0.8.0; v0.9.0 lifted the Org projection into its own `atrium-org` workspace crate; v0.10.0 → v0.10.3 closes Phase 17's vault → DB direction across four slices: v0.10.0 first slice (watcher + self-write filter + diff), v0.10.1 GUI wiring + conflict detection + sidecar, v0.10.2 reliability (malformed-file pause/resume + custom-keyword preservation + file-removal toast), v0.10.3 closer (RRULE canonicalisation + divergence detection + agenda-parity acceptance test). Phase 18 (Todoist CSV) opens at v0.11. Phase 12.5 (Calendar Month View) is re-engaged from its earlier "subsumed by Agenda" framing — slots after Phase 18 unless re-prioritised.

Where each phase landed:

- **Phases 0–9 → Simple Mode (v0.1.0).** Six canonical lists, areas + projects + tags, Quick Entry (`Ctrl+Alt+Space`), FTS5 search + filter expressions, multi-select + undo, Inspector + tag editor dialogs, sidebar find-as-you-type, full keyboard map, typography + accessibility, debug-pane Memory Watch, ship-gate regression script.
- **Phases 10–15 → Builder Mode (v0.2.0).** Mode toggle (GSettings, no schema impact), Inspector pane, defer dates, sequential projects, Forecast, Review queue, saved Perspectives, Repeating Tasks (RFC 5545 RRULE + three Org-mode completion semantics). v0.2.0 ended the v0.1 schema freeze; backwards-compatible `ALTER TABLE` migrations are now allowed.
- **Phase 15.5 → Calibre-powered search (v0.4.0).** Boolean expression grammar, Calibre match modifiers, comparison + range operators, date keywords, state predicates, regex matcher.
- **Phase 15.75 → Polish + extraction + Slice D (v0.5.0 → v0.6.5).** `atrium-search` and `atrium-cli` extracted as their own workspace crates. Per-area accent. Kanban Perspective renderer (`renderer = "board"` with drag-drop column moves). Agenda canonical page. FTS5 bm25 + recency ranking. SQL-translation evaluator with in-memory fallback.
- **v0.6.x → screenshot-driven cleanup + roadmap revision.** Soft-accent pass; state-aware row treatment; sidebar reorganisation; v0.6.19 retired Things 3 import (macOS-only), promoted Org-mode to Phase 16/17 as the must-ship two-way mirror, promoted Todoist to Phase 18, added Phase 19.5 productivity essentials.
- **v0.7.0 → v0.7.5 — visual fusion + Review absorbs Weekly Review.** Inspector check-off + perspective editor dialog. Task-level Mark Reviewed via migration 0006.
- **Phase 16 (v0.7.6 → v0.7.18, stamped at v0.8.0).** Hand-rolled Org parser/emitter (no third-party Org crate — see *Project tricks*). One-shot importer + vault writer + JSON snapshot. Custom-keyword round-trip via migration 0007. File-level `#+TITLE:` + `:PROPERTIES:` metadata. Multi-file vault walk + `WorkerHandle::ensure_area`. Post-write integrity check. Auto-debounced worker write hook (`spawn_worker_with_vault` + `VaultWriter` task). Round-trip test fixture across five complicated `.org` files. GUI vault integration via `vault-path` GSettings key.
- **v0.9.0 — `atrium-org` extraction.** The Phase 16 Org projection moved out of `atrium-core::sync` into its own crate. atrium-core gained a `VaultDirtyNotifier` trait + thinner `VaultConfig` (`Arc<dyn VaultDirtyNotifier>` instead of path + pool); atrium-org provides the impl via `OrgVaultNotifier` and an ergonomic `spawn_org_vault(root, pool) -> VaultConfig` helper. atrium-cli + the GTK binary depend on `atrium-org` directly. Pre-Phase-17 housekeeping; no behaviour change.
- **v0.10.0 — Phase 17 first slice.** Vault → DB sync end-to-end. New `atrium-org::vault_watcher::VaultWatcher` task spawned via `spawn_org_vault_with_watcher(root, pool, worker_handle)` (since superseded — see v0.10.1). mtime-based `RecentWrites` self-write filter (the path-only-TTL design got swallowed external edits inside the TTL window — see *Project tricks*). New `TaskUpdate.completed_at` field + builder method so `CLOSED:` cookies round-trip without `toggle_complete`'s `now()` stamping. New `notify` v8 dependency (sign-off granted). Three integration tests for external add / edit / delete.
- **v0.10.1 — Phase 17 next slice + cleanup pass.** GUI wiring + conflict detection + sidecar config:
  - **`spawn_vault_loop` builder** replaces the broken v0.10.0 `spawn_org_vault_with_watcher` (chicken-and-egg: it took a `WorkerHandle` that didn't exist at the natural call point). New shape: returns `(VaultConfig, VaultLoopHandle, mpsc::UnboundedReceiver<VaultEvent>)`. Caller passes the `VaultConfig` into `spawn_worker_with_vault`, then feeds the resulting `WorkerHandle` into `VaultLoopHandle::attach_watcher` to finish the wiring.
  - **`VaultEvent` channel** carries `ConflictBackup { source, backup }` and `ParseFailed { source, error }`. The GTK binary's `bridge_vault_events` routes these to `AtriumWindow::show_toast`.
  - **Writer-side conflict detection (spec §7.3.3 rule 5).** Before each atomic-overwrite, the writer stats the destination file. If the file's mtime isn't in `RecentWrites` (an external editor touched it), the current contents copy to `<file>.atrium.bak.<UTC-timestamp>` first. Format `%Y%m%dT%H%M%SZ` (no colons — filesystem-safe; UTC; sortable).
  - **Sidecar.** New `atrium-org/src/sidecar.rs` ships `<vault>/.atrium/config.toml`. Hand-rolled minimal TOML (no `toml` crate). Tag colours round-tripped today; mode + perspectives slots reserved (the file always emits the section headers). The writer's `flush_due` calls `refresh_sidecar_if_changed` after each project flush burst; a `last_sidecar` cache skips the IO when content is unchanged.
  - **Worker domain invariants.** `DomainError::ParentProjectMismatch` (subtask must live in the same project as its parent — checked in `create_task` and `update_task` project-move) + `DomainError::EmptyFilterExpr` (rejects blank perspective filters in create + update). `DbError` gains `#[from] DomainError` so domain rejections flow through the existing API.
  - **Error-type wiring.** `atrium/src/error.rs` lost its `#![allow(dead_code)]`. `UiError::VaultPathInvalid` is constructed in `read_vault_setup_from_settings` when the user's `vault-path` GSetting points at an uncreatable directory. `boot_data_layer` returns `Result<BootedDataLayer, AtriumError>` (was `anyhow::Result`); the `BootedDataLayer` struct bundles handle + receivers + pool + optional `vault_events_rx`.
  - **`flatten_one` fix.** v0.10.0's vault watcher silently dropped TODOs nested under non-keyword headings (`* Backlog / ** TODO Real`). The importer always handled this; the watcher now matches.
- **v0.10.2 — Phase 17 reliability slice.**
  - **Malformed-file pause/resume.** `VaultWatcher` gains a `paused: Arc<Mutex<HashSet<PathBuf>>>` set. `mark_paused` returns whether the path was already in the set so `ParseFailed` only fires on transitions; `clear_paused` returns `true` once when the file goes back to clean and `ParseRecovered` fires before the diff applies. Repeated bad saves no longer re-toast on every event.
  - **Custom-keyword fix (two real v0.10.0 bugs).** v0.10.0's `ParsedTask::to_new_task` only handled `OrgKeyword::Cancelled` — `Custom` variants fell through and a fresh `WAITING` headline landed in DB as plain TODO. `diff_from` didn't compare `orig_keyword` either, and `TaskUpdate` had no field for it. New `TaskUpdate.orig_keyword: Option<Option<String>>` + builder; the worker SQL builder threads it through. New private helper `org_keyword_to_orig` in `vault_watcher.rs` shared by create + diff paths so they stay in lockstep.
  - **File removal: toast + retain.** Per spec §3.5 (DB canonical, vault projected), `rm`ing a vault file no longer silently leaves stale rows. Watcher emits `VaultEvent::FileRemoved`; GUI toasts; the next project flush recreates the file from DB.
  - **New VaultEvent variants:** `ParseRecovered { source }`, `FileRemoved { source }`. `ParseFailed` now means "first failure on this file since the last clean parse" rather than "every parse failure ever."
  - **Test scenarios.** Three of the four roadmap §17 items: `concurrent_atrium_and_external_edit_preserves_user_content_as_bak` (writer-side conflict detection under simultaneous edits), `large_file_parses_under_budget` (1K headlines, 500 ms wall budget), `external_file_removal_preserves_tasks_and_toasts`. Multi-day RRULE round-trip lands at v0.10.3.
- **v0.10.3 — Phase 17 closer.**
  - **`rrule_cookie` helpers** (atrium-org/src/rrule_cookie.rs). Three pure functions: `rrule_to_org_cookie(rrule_text, mode) -> Option<String>` and the typed sibling `rrule_to_org_repeater` (RRULE → cookie, lossy on multi-weekday / BYMONTHDAY); `org_repeater_to_rrule(repeater) -> Option<String>` (cookie → RRULE, FREQ + INTERVAL only); `cookie_matches_rrule(repeater, rrule_text) -> bool` (the divergence equality check — BY-clauses don't count as divergence since cookies can't express them). Hand-rolled FREQ + INTERVAL parser, no `toml`-style dep.
  - **Writer wiring.** `scheduled_repeater_from_task` (the v0.7.10 None-returning placeholder) flips on. SCHEDULED for repeating tasks now lands as `<2026-05-11 Mon ++1w>`; `:RRULE:` in the property drawer stays canonical. Stock org-agenda renders the cookie; Atrium reads `:RRULE:` on read-back.
  - **Watcher fixes two related v0.10.0 → v0.10.2 gaps.** `to_new_task` reads `:RRULE:` on create; `diff_from` syncs it on update via `TaskUpdate.repeat_rule_value`. A user adding `BYDAY=MO,WE` to the property in Emacs now propagates to DB.
  - **Divergence detection.** `collect_rrule_divergences` walks parsed headlines and flags any task where `cookie_matches_rrule` returns false. New `VaultEvent::RruleDiverged` event surfaces the title + cookie + RRULE; the watcher synchronously calls `write_project_to_vault` to rewrite the file from canonical. RecentWrites swallows the resulting inotify echo.
  - **Agenda parity acceptance test** (`atrium/src/ui/agenda.rs::tests::agenda_parity_with_reference_org_agenda`). Synthesised vault with tasks across every bucket plus all the "shouldn't appear" edge cases; reference classifier mirrors stock org-agenda's day-window logic from the Org spec; both must agree on every task. Visual style differs between the two surfaces — semantic groupings agree.
  - **Multi-day RRULE round-trip fixture** (`atrium-org/tests/fixtures/org/rrule_patterns.org`). Four cases: weekly single-day, weekly multi-day, monthly day-of-month, daily INTERVAL=3. All round-trip through the existing fixture harness with `:RRULE:` preserved verbatim in the property drawer.

**Architectural commitment: every non-GUI surface stays CLI-testable.** The data layer, search engine, and import/export pipelines all run through `atrium-cli` (or future siblings like `atriumd`, the post-1.0 `atrium-tui`). Don't add functionality to the GTK binary that can't be reached from the shell.

**Test count: 637 across the workspace at v0.10.3**, all green. `bash scripts/regression.sh` runs in under 2 seconds. Schema version: 7.

## Authoritative documents

- **`spec.md`** — the contract. Architecture (§3), schema (§4), UI deltas (§5), Quick Entry (§6), import/export mapping (§7), perf budget (§8). Read it before changing semantics. If a request conflicts with the spec, surface that — don't quietly drift.
- **`roadmap.md`** — the 20-phase plan plus four sub-phases (12.5, 15.5, 15.75, 19.5). Shipped phases are condensed; open phases (17 onward) are fully detailed. Don't skip phases or pull work forward without explicit go-ahead.
- **`patchnotes.md`** — newest at top. v0.0.0 → v0.4.x is condensed at the bottom; v0.5.0 → v0.8.0 entries are kept full.

## Architectural commitments (don't drift)

These five decisions are load-bearing. Code that contradicts them is wrong even if it compiles and passes tests.

### 1. Mode-as-View

Mode (Simple / Builder) is a **GSettings flag plus UI-layer rendering choices** — nothing more. It does not affect schema, does not migrate data, does not hide rows, does not constrain Quick Entry. The schema is the **OmniFocus superset** on day one; every Builder column (`defer_until`, `estimated_minutes`, `sequential`, `review_interval_days`, `last_reviewed_at`, `repeat_rule`, `parent_id`) exists from `0001_initial.sql`. Simple Mode hides those fields in the editor and in derived views; it does not lack them.

The Phase 10 acceptance test (`atrium-core/tests/mode_flip_snapshot.rs`) enforces this — flipping mode must not touch the DB.

### 2. Single-writer SQLite worker

A dedicated `tokio` task owns the writable `rusqlite::Connection`. The GTK thread holds an `mpsc::Sender<Command>` and **never** touches the writable connection. Reads use a separate read-only connection pool (`PRAGMA query_only = ON` per connection — SQLite enforces read-only at the engine level). WAL mode is mandatory. UI updates arrive as `TaskChanges { created, updated, deleted, status_changed }` and `LibraryChanges` deltas via a `glib::MainContext` channel — **never as full reloads**.

Pattern lifted directly from Viaduct's `DatabaseQueue` (`~/.gitrepos/Viaduct/`). When designing data-layer changes, look there for the pattern's shape.

### 3. Local-first, no network sync

SQLite at `$XDG_DATA_HOME/atrium/atrium.db`. No CalDAV client, no cloud, no telemetry, no network calls in v1.0. VTODO export (Phase 19) is a one-way file dump — explicitly **not** a CalDAV client. Local file mirroring (the Org vault, see commitment #5) is fine — that's filesystem IO, not network sync. Network-sync feature requests are out of scope through 1.0.

### 4. Debug-first architecture

Testing and debugging tooling is **built into the binary**, not bolted on. The `--debug` flag opens an in-app debug surface for stress generators (1K / 10K / 50K / 100K-task fixtures), edge-case fixtures, IO instrumentation (rusqlite's `trace` feature routes every SQL statement into a `tracing` span — no new crates), and a Memory Watch pane (`/proc/self/status` sampler).

Release builds ship the same code paths — heavy generators are gated on `--debug` so end users never see them, but the wiring is always present. Tests reuse the same fixtures; don't fork a separate "test-only" path.

### 5. Vault projection, not alternative store

When configured, an Org vault (default `~/Tasks/`, set via the `vault-path` GSettings key) mirrors task state to `.org` files for editing in any Org-aware tool. Discipline: **DB canonical, vault projected** — SQLite is the source of truth, the vault is downstream. Atrium runs cleanly without a vault; the vault never runs without the DB.

DB → vault writer shipped at Phase 16 / v0.8.0. Vault → DB sync (`inotify` watcher) ships at Phase 17. Both directions follow the round-trip rules in spec §7.3.3: never destroy data, `:ID:` is the round-trip anchor, conflicts are surfaced not silenced (losers preserved at `<file>.atrium.bak.<timestamp>`), atomic writes (`write-temp + fsync + rename`).

Don't pivot to "vault is the storage." The §8 perf budget assumes SQLite indexes for Forecast and Review queries; Org-as-store can't hit those targets at 10K-task scale (`org-roam` itself uses a SQLite cache for the same reason).

## Project tricks worth remembering

The non-obvious mechanics that aren't visible from the code alone:

- **Hand-rolled Org parser, not a crate.** `orgize` and `starsector` were both surveyed at Phase 16 and rejected — orgize's last stable was 0.9.0 (2021), the active line in alpha since 2023; starsector's last release was October 2022 and pulls orgize-alpha as a transitive. The hand-roll lives at `atrium-core/src/sync/org/`. The "preserve unknown constructs verbatim" rule (spec §7.3.3 rule 1) is satisfied by capturing every unrecognised line into `unknown_lines` and re-emitting on write — easier in a focused passthrough parser than fighting either crate's AST. Don't add an Org crate without explicit re-discussion.
- **Test-file split pattern.** When a `#[cfg(test)] mod tests` body in a source file gets unwieldy (e.g., `worker.rs` at 2622 lines pre-v0.8.0), split it out via `#[cfg(test)] #[path = "<name>_tests.rs"] mod tests;` at the bottom of the source file. Same compilation, same coverage; halves the file size for editing. See `atrium-core/src/db/worker.rs` + `worker_tests.rs`.
- **VaultWriter debounce shape.** ~100 ms debounce window with a 50 ms tick. Receiving a `ProjectDirty(project_id)` extends that project's deadline (last-deadline-wins coalescing); the tick fires writes for projects past their deadline. Channel is `mpsc` (single consumer); under absurd load `try_send` drops rather than blocks (worst case: one stale vault file).
- **VaultWatcher self-write filter is mtime-based, not path-TTL-based.** The first design recorded `(path, recorded_at)` and matched on path within a TTL — the integration tests immediately surfaced the failure: an external edit happening within the TTL window after Atrium's own write got swallowed because the writer's record was still "recent" when the watcher's debounce fired. Fixed design: `RecentWrites` stores `(path, mtime_just_written)`; the watcher reads the file's actual mtime and matches on exact tuple equality. Linux ext4 stores nanosecond mtimes so two distinct writes never collide. The TTL stays as a memory bound (2 seconds) but doesn't gate the match. **Don't revert to a path-only filter** — it's been tried; it loses external edits.
- **Atomic-write helper.** `atrium-core/src/sync/atomic.rs` does `write-temp + fsync + rename` for every vault write. Crash-safe; non-Org consumers (JSON snapshot) use it too. **Never** write a vault file without going through it.
- **Post-write integrity check.** Every `emit_org_file_with_meta` re-reads the file and verifies it parses cleanly through Atrium's own reader; failure propagates as `io::Error`. Catches emitter regressions immediately instead of letting bad files sit on disk.
- **SQL-translation fast-path.** `atrium_search::sql_translate::try_translate(&Expr, today)` converts an `Expr` to a SQL `WHERE` fragment + bound params when every node maps cleanly. Returns `None` for `~regex`, fuzzy `?word`, `is:today`, and `Field::Project|Area` (deferred) — the in-memory evaluator is the fallback. Both GUI and CLI use this; parity is pinned by 21 integration tests in `atrium-search`.
- **`modified_at` triggers with `WHEN old = new`.** The triggers prevent recursion *and* let explicit writes survive — important for import-time timestamp preservation. Don't drop the `WHEN` clause if you ever modify these triggers.
- **`ScheduledFor` enum, not string.** Schema's "TEXT (ISO date OR `__someday__` sentinel)" maps to a Rust enum (`Someday | Date(NaiveDate)`) via custom `ToSql` / `FromSql`. Type-safe at the boundary; round-trip-clean. Don't reach for the raw string.
- **`NewTask.completed_at` is additive (v0.7.17).** When the importer parses a source CLOSED cookie, it threads the timestamp directly into `NewTask.completed_at` instead of calling `toggle_complete` after create (which would stamp `now()`). All `NewTask` call sites need to set or default it; the GUI undo path also threads it so undo preserves the original completion timestamp.
- **`task.orig_keyword` (migration 0007) preserves non-canonical Org keywords.** Atrium's domain has TODO/DONE only; WAITING / BLOCKED / IN-PROGRESS / CANCELLED stash in `orig_keyword` so headlines round-trip without losing their label. The Org writer's lookup checks `orig_keyword` first, then falls back to TODO/DONE.
- **Single-writer worker pattern lifted from Viaduct.** When implementing or modifying the data layer, look at `~/.gitrepos/Viaduct/` for the queue, command enum, and `TaskChanges`-equivalent delta shape before reinventing.
- **`spawn_vault_loop` is two-step.** The Phase 17 GUI builder can't be one call: the watcher needs a `WorkerHandle` to dispatch incoming changes through, and the worker needs a `VaultConfig` (containing the writer-side notifier) to install the projection. v0.10.0 tried `spawn_org_vault_with_watcher(root, pool, worker_handle)` and the doc-comment had to lie — there was no valid call site. v0.10.1's shape: `spawn_vault_loop(root, pool)` builds the writer-side and shared `RecentWrites` up front, returns `(VaultConfig, VaultLoopHandle, events_rx)`. Caller passes `VaultConfig` into `spawn_worker_with_vault`, then feeds the resulting handle into `VaultLoopHandle::attach_watcher`. Don't try to collapse this back to one call.
- **Vault sidecar is hand-rolled TOML.** Same ethos as the hand-rolled Org parser — `orgize`/`starsector` were rejected, the `toml` crate was rejected. The schema is small (top-level scalars + one level of `[section]` with string-string entries) and the emit/parse helpers in `atrium-org/src/sidecar.rs` round-trip deterministically (`BTreeMap` for emit order). If the schema ever needs arrays or nested tables, that's a re-discussion before adding `toml`.
- **Conflict-detection backup format is `<file>.atrium.bak.<YYYYMMDDTHHMMSSZ>`.** Filesystem-safe (no colons), UTC, sortable. Don't use RFC 3339 with colons — it works on Linux ext4 but is unreliable on FAT32 / SMB shares users might have their vault on.
- **`:RRULE:` is canonical; the SCHEDULED cookie is best-fit projection.** Spec §7.3.3 rule 3. `task.repeat_rule` carries the full RFC 5545 RRULE; the Org cookie's `+1w` / `++1w` / `.+1w` is a lossy summary the writer projects from canonical. When the user edits ONLY the cookie in Emacs (e.g. `+1w` → `+2w` without touching `:RRULE:`), divergence detection fires and the watcher rewrites the file from canonical. When the user edits ONLY `:RRULE:` (adding a BY-clause the cookie can't express), no divergence — the watcher syncs the new rule to DB and the next writer flush re-emits with consistent best-fit. **Don't try to make the cookie carry BY-clause information** — Org cookies can only encode FREQ + INTERVAL; that's the contract.

## Dependency discipline

**No third-party crates without prior sign-off.** This is hard. The full v0.1 dependency set is locked in `roadmap.md` Phase 0:

> `gtk4`, `libadwaita`, `tokio`, `rusqlite` (`bundled`, `chrono` features), `serde`, `serde_json`, `chrono`, `anyhow`, `thiserror`, `tracing`, `tracing-subscriber`

Sign-off granted in subsequent phases:

- `uuid` (Phase 1) — UUID v4 generation for `:ID:` round-trip.
- `rrule` (Phase 15, v0.2.0) — RFC 5545 RRULE parsing + iteration for repeating tasks.
- `regex` (Phase 15.5, v0.4.0) — `tag:~regex` match modifier in the search expression language. Already transitively in the dep graph via `tracing-subscriber`; promoted to a direct dependency for `atrium-core`.
- `notify` (Phase 17, v0.10.0) — cross-platform filesystem watcher for vault → DB sync. Direct dep of `atrium-org`. Canonical Rust file-watching crate (used by watchexec / cargo-watch). Default features only — uses inotify on Linux, which is what Atrium ships.

Pending dependency checks: `ical` / `rustical` (Phase 19).

Resolved against (won't be added):

- `orgize` / `starsector` (Phase 16, v0.7.6 dep-research pass) — both surveyed and rejected as dormant. The hand-rolled subset at `atrium-core/src/sync/org/` is the answer.

If a task pushes you toward a crate that isn't already in `Cargo.toml`, **stop and ask** — don't add it speculatively, and don't hand-roll a wide subset to dodge the conversation.

## Spec discipline

The contract docs are the most valuable artifact in this repo. When editing them:

- **Match the existing voice and structure.** `spec.md` uses numbered sections with short paragraphs and small tables; `roadmap.md` is a flat checkbox list grouped by phase with one italic tagline per phase. Don't reformat or restructure unprompted.
- **Cross-reference, don't duplicate.** If a fact is in `spec.md` §4, refer to it from `roadmap.md` rather than restating it. They drift if both contain the same claim.
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

The v0.1 freeze's good instinct still applies: when a feature seems to need a new column, first check whether the column already exists in the OmniFocus superset and the right move is exposing it differently in the UI. The superset is rich; most "I need a column for this" instincts turn out not to need a migration.

## Build / test / lint

```bash
cargo test --workspace            # all tests (582 at v0.8.0)
cargo test <test_name>            # single test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
bash scripts/regression.sh        # ship gate: fmt → clippy → test → atrium-cli + kanban smoke → cold-start sanity
```

CI runs fmt + clippy + test on Linux. Tests are required from day one (Brandon's hard rule, in `roadmap.md` Phase 0 and `spec.md` §10).

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

## Codebase map (current — v0.10.3)

Five workspace crates split by responsibility. The data layer (`atrium-core`), search engine (`atrium-search`, extracted v0.4.2), Org projection (`atrium-org`, extracted v0.9.0), and headless CLI (`atrium-cli`, added v0.4.3) all stay GUI-free so the Phase 20 `atriumd` daemon and the post-1.0 TUI can reuse them. atrium-core knows nothing about Org; the projection plugs in via the `VaultDirtyNotifier` trait so a future Markdown / TaskPaper / Todoist sibling can use the same hook.

```
atrium-search/                        ← Calibre-powered search engine (extracted v0.4.2)
├── src/lex.rs                        ← Token enum + tokenizer
├── src/parse.rs                      ← recursive-descent parser → Expr AST + sort modifiers
├── src/ast.rs                        ← Expr + Field + State + MatchKind + Comparator + Value + DateKeyword + SortSpec
├── src/dates.rs                      ← date keyword + relative-day → concrete date resolution
├── src/eval.rs                       ← in-memory evaluator + EvalContext (lazy regex cache, Damerau-Levenshtein for fuzzy)
├── src/rank.rs                       ← FTS5 bm25 + recency factor
├── src/sql_translate.rs              ← Expr → SQL fast-path; in-memory fallback for regex / fuzzy / composite
└── src/tests.rs                      ← parse + eval + translate round-trips

atrium-cli/                           ← headless CLI (full task + perspective CRUD + Phase 16 import/export)
├── src/main.rs                       ← subcommand dispatch, DB open, EvalContext build, write paths
├── src/args.rs                       ← stdlib argv parser
├── src/output.rs                     ← TSV / JSON / human-readable formatters (incl. kanban columns)
├── src/import.rs                     ← `import org PATH [--dry-run]` — single .org or vault directory
└── src/export.rs                     ← `export org PATH` (vault writer) + `export json PATH` (snapshot)

atrium-core/                          ← headless data layer
├── src/lib.rs                        ← re-exports (Task / WorkerHandle / VaultConfig / VaultDirtyNotifier / spawn_worker / spawn_worker_with_vault / RepeatRule / …)
├── src/paths.rs                      ← XDG path helpers, APP_ID
├── src/error.rs                      ← thiserror hierarchy
├── src/repeat.rs                     ← RFC 5545 RRULE wrapper, RepeatMode, CountStep
├── src/quick_entry.rs                ← #tag / @today / @deadline parser; shared between GUI + CLI
├── src/render.rs                     ← kanban column projection from a saved Perspective (Slice D foundation)
├── src/test_support.rs               ← dummy_task helpers behind `test-support` feature
├── src/domain/                       ← Task / Project / Area / Tag / Perspective / ScheduledFor / NewTask
├── src/sync/                         ← projection-agnostic sync helpers
│   ├── atomic.rs                     ← write-temp + fsync + rename helper used by every vault write
│   └── json.rs                       ← `Snapshot` type + `export_json`; lossless versioned DB dump
└── src/db/
    ├── worker.rs                     ← single-writer task; spawn / spawn_with_vault; vault_notifier ping after every commit
    ├── worker_tests.rs               ← tests submodule loaded via #[path = "worker_tests.rs"] mod tests
    ├── vault_hook.rs                 ← (v0.9.0) `VaultDirtyNotifier` trait + thin `VaultConfig` — the projection contract
    ├── read_pool.rs                  ← read-only connection pool
    ├── read.rs                       ← list_inbox / list_today / list_forecast / list_review_queue / list_agenda / search / counts
    ├── command.rs                    ← Command enum
    ├── changes.rs                    ← TaskChanges, LibraryChanges deltas
    ├── fixtures.rs                   ← --fixture stress generators
    └── migrations/                   ← 0001 initial → 0007 task.orig_keyword; user_version PRAGMA currently 7

atrium-org/                           ← Phase 16 Org-mode projection (v0.9.0); Phase 17 vault → DB sync (v0.10.0); GUI wiring + conflict detection + sidecar (v0.10.1)
├── src/lib.rs                        ← VaultEvent + RecentWrites + sidecar re-exports; `spawn_org_vault` (write-only); `spawn_vault_loop` (two-way GUI builder, v0.10.1)
├── src/vault_writer.rs               ← `VaultWriter` task — receives ProjectDirty over tokio mpsc, debounces ~100 ms (50 ms tick); records (path, mtime) into RecentWrites after every flush. v0.10.1: pre-flush conflict check copies external edits to <file>.atrium.bak.<UTC>; emits ConflictBackup events; refreshes sidecar via `last_sidecar` cache
├── src/vault_watcher.rs              ← `VaultWatcher` task — `notify` v8 backend; debounces 200 ms; consults RecentWrites to suppress self-writes; reader→DB diff by `:ID:` (CREATE / UPDATE / DELETE). v0.10.1: emits ParseFailed events; flatten_one recurses into children of non-keyword headings
├── src/self_write.rs                 ← `RecentWrites` — bounded TTL set of (path, mtime) keyed on exact tuple equality. Shared via Arc<RwLock<>> between writer + watcher.
├── src/sidecar.rs                    ← (v0.10.1) `<vault>/.atrium/config.toml` — Sidecar struct + emit_text/parse_text + read/write helpers + build_from_db. Hand-rolled minimal TOML; tag colours round-tripped.
├── src/rrule_cookie.rs               ← (v0.10.3) `rrule_to_org_cookie` / `rrule_to_org_repeater` / `org_repeater_to_rrule` / `cookie_matches_rrule`. Pure helpers — RRULE ↔ Org cookie projection.
└── src/org/
    ├── mod.rs                        ← OrgFile / OrgHeadline / OrgKeyword / parse_org_file / emit_org_file + post-write integrity check
    ├── parse.rs                      ← hand-rolled headline / cookie / properties / body / nested-subtask parser
    ├── emit.rs                       ← inverse — emits stable, org-agenda-readable output
    ├── import.rs                     ← single-file + multi-file vault importer; uses WorkerHandle::ensure_area
    └── write.rs                      ← project → .org file writer; build_org_tree fans Tasks back into nested OrgHeadlines

atrium/                               ← GTK binary
├── build.rs                          ← compiles GSettings schema for cargo-only runs
├── src/main.rs                       ← Application; boot_data_layer reads vault-path GSettings → spawn_worker_with_vault
├── src/ui/                           ← window, task list/object, inspector + inspector_pane, tag editor, filter, forecast, review,
│                                       perspective_editor, logbook, agenda, board, shortcuts, about, typography
├── src/quickentry/modal.rs           ← Quick Entry modal (adw::Window, fade-in); parser lives in atrium-core::quick_entry
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

docs/                                 ← long-form references (schema.md / keymap.md / accessibility.md / perf-baseline.md / regression.md / gtd-patterns.md)
scripts/regression.sh                 ← ship-gate

atrium-core/tests/                    ← integration tests
└── mode_flip_snapshot.rs             ← Phase 10 acceptance (mode flip never touches DB)

atrium-org/tests/                     ← integration tests crossing the core/org boundary
├── org_roundtrip.rs                  ← Phase 16 round-trip across five fixtures
├── worker_org_integration.rs         ← import_org_file / import_org_directory / spawn_org_vault end-to-end
├── vault_watcher_integration.rs     ← (v0.10.0) external add / external edit / external delete via fs::write → DB
└── fixtures/org/                     ← kitchen_sink / custom_keywords / deep_nesting / project_metadata / unicode .org files
```

## Dialog primitives (standardised v0.0.37)

- **Inspector** (Simple Mode) + **Tag editor** are `adw::Dialog` (in-window modal overlay; `present(parent)` / `close()`).
- **Inspector pane** (Builder Mode) is an always-visible `AdwBin` in the right-side `AdwOverlaySplitView` sidebar — non-modal, autosaves on focus-out.
- **Quick Entry** is `adw::Window` (`modal=false`, `transient_for(main)`, fade-in keyframe) — the spec wants it to *not* steal grab from the previously-focused window; AdwDialog always grabs.
- **Memory Watch** is `adw::Window` for the same non-grab reason.
- **Confirmations** use `adw::AlertDialog`. The v0.3.0 tag-colour picker (`prompt_for_tag`) extends `AlertDialog` with a custom extra-child Box for the swatch row.
