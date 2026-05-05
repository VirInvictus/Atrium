# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

**Pre-implementation as of v0.0.0.** No source code exists yet. The repository currently holds the contract (`spec.md`, `roadmap.md`), public framing (`README.md`), release notes (`patchnotes.md`), and metadata (`VERSION`, `LICENSE`, `logo.svg`). Phase 0 (Cargo skeleton, CI, scaffolding) begins after design sign-off.

Until code lands, the work in this directory is almost entirely **editing the contract**. Treat that with care — see "Spec discipline" below.

## Authoritative documents

- **`spec.md`** is the contract. Architecture (§3), schema (§4), UI deltas (§5), Quick Entry (§6), import/export mapping (§7), and the perf budget (§8) all live there. **Read it before changing semantics.** If a request conflicts with the spec, surface that conflict — don't quietly drift.
- **`roadmap.md`** is the 20-phase plan. Phases 0–9 → v0.1 (Simple Mode). Phases 10–15 → v0.2 (Builder Mode). Phases 16–19 → import/export. Phase 20 → v1.0. **Don't skip phases or pull work forward** without explicit go-ahead — Brandon sequenced these deliberately to keep each release shippable.
- **`patchnotes.md`** — newest at top. The first real release entry lands at the end of Phase 9 as v0.1.0.

## Architectural commitments (don't drift from these)

These three decisions are load-bearing. Any code that contradicts them is wrong even if it compiles and passes tests.

### 1. Mode-as-View

Mode (Simple / Builder) is a **GSettings flag plus UI-layer rendering choices** — nothing more. It does not affect schema, does not migrate data, does not hide rows, does not constrain Quick Entry. The schema is the **OmniFocus superset** on day one; every Builder column (`defer_until`, `estimated_minutes`, `sequential`, `review_interval_days`, `last_reviewed_at`, `repeat_rule`, `parent_id`) exists from migration `0001_initial.sql`. Simple Mode hides those fields in the editor and in derived views; it does not lack them.

The Phase 10 acceptance test (mode-flip snapshot) exists to enforce this — flipping mode must not touch the DB.

### 2. Single-writer SQLite worker

A dedicated `tokio` task owns the writable `rusqlite::Connection`. The GTK thread holds an `mpsc::Sender<Command>` and **never** touches the writable connection. Reads use a separate read-only connection pool. WAL mode is mandatory. UI updates arrive as `TaskChanges { created, updated, deleted, status_changed }` deltas via a `glib::MainContext` channel — **never as full reloads**.

This pattern is lifted directly from Viaduct's `DatabaseQueue` (sibling repo at `~/.gitrepos/Viaduct/`). When implementing the data layer, look there for the pattern's shape rather than reinventing it.

### 3. Local-first, no sync

SQLite at `$XDG_DATA_HOME/atrium/atrium.db`. No CalDAV client, no cloud, no telemetry, no network calls in v1.0. VTODO export (Phase 19) is a one-way file dump — explicitly **not** a CalDAV client. If a feature request implies sync, push back; it's out of scope through 1.0.

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
