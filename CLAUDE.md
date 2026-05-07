# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

**Simple Mode shipped (v0.1.0, May 2026).** Phases 0–9 complete. Atrium runs end-to-end: workspace scaffolding, schema + single-writer worker, application shell, all six canonical lists (Inbox / Today / Upcoming / Anytime / Someday / Logbook), areas + projects + tags + multi-tag, Quick Entry, FTS5 search + filter expressions, multi-select + undo, Inspector + tag editor dialogs, sidebar find-as-you-type, full keyboard map, typography + accessibility (Atkinson Hyperlegible), debug-pane Memory Watch, ship-gate regression script. Three Phase 9 follow-ups remain on Brandon's plate (the actual `v0.1.0` git tag, the Flatpak publish, the public announcement on `VirInvictus.github.io`); two Phase 8 carryovers also outstanding (README screenshots, Flatpak font verification under sandbox).

Phase 10 (Builder Mode UI shell) is what's next — mode toggle, `AdwOverlaySplitView` Inspector pane, Builder-only sidebar entries (Forecast / Review / Perspectives) as stubs, project page Sequential + Review interval pickers, mode-flip integration test. **No schema changes** — the Phase 1 superset already has every Builder column.

## Authoritative documents

- **`spec.md`** is the contract. Architecture (§3), schema (§4), UI deltas (§5), Quick Entry (§6), import/export mapping (§7), and the perf budget (§8) all live there. **Read it before changing semantics.** If a request conflicts with the spec, surface that conflict — don't quietly drift.
- **`roadmap.md`** is the 20-phase plan. Phases 0–9 → v0.1 (Simple Mode). Phases 10–15 → v0.2 (Builder Mode). Phases 16–19 → import/export. Phase 20 → v1.0. **Don't skip phases or pull work forward** without explicit go-ahead — Brandon sequenced these deliberately to keep each release shippable.
- **`patchnotes.md`** — newest at top. The first real release entry lands at the end of Phase 9 as v0.1.0.

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

Every later phase that wants to add a crate has an explicit "dependency check" item — e.g. `rrule` (Phase 15), `orgize` (Phase 17), `ical`/`rustical` (Phase 19). If a task pushes you toward a crate that isn't already in `Cargo.toml`, **stop and ask** — don't add it speculatively, and don't hand-roll a wide subset to dodge the conversation.

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

## Schema rule (once Phase 1 ships)

**No mid-v0.1 schema changes.** Migration `0001_initial.sql` ships the full superset; backwards-compatible migrations begin at v0.2. If a v0.1 task seems to need a schema change, that's a signal to re-examine — almost always the column already exists in the superset and the right move is to expose it differently in the UI.

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

## Codebase map (current)

The workspace split adopted in v0.0.3 mirrors the Phase 20 `atriumd` daemon plan and the post-1.0 TUI plan — the data layer must be reusable from multiple frontends.

```
atrium-core/                          ← headless library
├── src/lib.rs                        ← re-exports
├── src/paths.rs                      ← XDG path helpers, APP_ID
├── src/error.rs                      ← thiserror hierarchy
├── src/domain/                       ← Task / Project / Area / Tag / ScheduledFor types
└── src/db/
    ├── worker.rs                     ← single-writer task; spawn_worker
    ├── read_pool.rs                  ← read-only connection pool
    ├── read.rs                       ← list_inbox / list_today / search / counts
    ├── command.rs                    ← Command enum (Create/Update/Toggle/Delete/…)
    ├── changes.rs                    ← TaskChanges, LibraryChanges deltas
    ├── fixtures.rs                   ← --fixture stress generators
    └── migrations/0001_initial.sql   ← OmniFocus superset (locked through v0.1)

atrium/                               ← GTK binary
├── build.rs                          ← compiles GSettings schema for cargo-only runs
├── src/main.rs                       ← Application, CLI flags, accels, action wiring
├── src/error.rs
├── src/ui/
│   ├── mod.rs
│   ├── window.rs                     ← AtriumWindow (composite template)
│   ├── task_list.rs                  ← row factory, ActiveList, apply_changes
│   ├── task_object.rs                ← AtriumTask glib::Object wrapper
│   ├── inspector.rs                  ← per-task Inspector (AdwDialog, Phase 7i)
│   ├── tag_editor.rs                 ← per-task tag editor (AdwDialog, Phase 7g)
│   ├── filter.rs                     ← search-bar filter expressions (Phase 7d)
│   ├── shortcuts.rs                  ← Ctrl+? / F1 dialog
│   ├── about.rs                      ← AdwAboutDialog
│   └── typography.rs                 ← bundled font install + CSS load
├── src/quickentry/
│   ├── mod.rs
│   ├── modal.rs                      ← Quick Entry modal (adw::Window, fade-in)
│   └── parser.rs                     ← #tag / @today / @deadline parser
└── src/debug/mod.rs                  ← Memory Watch + /proc/self/status sampler

data/                                 ← installed assets
├── window.ui                         ← composite template
├── style.css                         ← typography + per-surface tweaks
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
```

The dialog primitives standardised in the v0.0.37 bugsweep:

- **Inspector** + **Tag editor** are `adw::Dialog` (in-window modal overlay; `present(parent)` / `close()`).
- **Quick Entry** stays an `adw::Window` (`modal=false`, `transient_for(main)`, fade-in keyframe) — the spec wants it to *not* steal grab from previously-focused windows; AdwDialog always grabs.
- **Memory Watch** stays an `adw::Window` for the same reason (non-modal observer pane).
- **Confirmations** use `adw::AlertDialog`.
