# Atrium — Patch Notes

## v0.0.0 (2026-05-05) — Pre-implementation

Repository established. Specification, roadmap, and project conventions in place. No code yet — Phase 0 begins after sign-off.

### What's there

- **`spec.md`** — full application specification, 10 sections covering mission, mandates, architecture (mode-as-view, single-writer SQLite worker), data model (OmniFocus-superset schema), Simple/Builder UI deltas, Quick Entry contract, imports/exports with the Linux productivity-app landscape, perf budget, scope boundaries.
- **`roadmap.md`** — 20-phase plan. Phases 0–9 land Simple Mode (v0.1). Phases 10–15 add Builder Mode (v0.2). Phases 16–19 cover imports across Things 3, OmniFocus, Org-mode, Taskwarrior, Todoist, VTODO, todo.txt, TaskPaper. Phase 20 closes 1.0.
- **`README.md`** — public-facing introduction.
- **`LICENSE`** — MIT.
- **`VERSION`** — single source of truth (`0.0.0`).
- **`logo.svg`** — placeholder mark.

### Confirmed for v0.1

- **Stack:** Rust 2024, GTK4 ≥ 4.16, libadwaita ≥ 1.7, single-writer SQLite worker (Viaduct's pattern).
- **Direct deps:** `gtk4`, `libadwaita`, `tokio`, `rusqlite`, `serde`/`serde_json`, `chrono`, `anyhow`, `thiserror`, `tracing`/`tracing-subscriber`. Anything else gets a per-phase sign-off.
- **License:** MIT.

The first real release entry will land at the end of Phase 9 as **v0.1.0 — Simple Mode**.
