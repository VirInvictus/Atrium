<p align="center">
  <img src="logo.svg" alt="Atrium" width="240">
</p>

<p align="center">
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Language-Rust-blue" alt="Language: Rust"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/GNOME-50%2B-4a86cf" alt="GNOME 50+">
  <img src="https://img.shields.io/badge/status-pre--implementation-lightgrey" alt="Status: pre-implementation">
</p>

---

# Atrium

**The native GNOME task manager you grow into, not out of.**

Atrium fuses Things 3's clarity with OmniFocus's depth into one application via a mode switch over a shared data store. Pick **Simple Mode** for *what am I doing right now*. Switch to **Builder Mode** when you want full GTD review, deferral, sequential projects, and forecast. Same data, two surfaces, no migration.

**Author's Note:** I'm a broke college student in my late thirties with no professional industry experience yet — Atrium is one in a string of native Linux desktop apps I'm building to learn the craft and assemble a portfolio. I came from Things 3 and OmniFocus on macOS / iOS, and Linux has nothing in their lane that isn't an Electron wrapper or a CalDAV form over a webview. Atrium is the answer I wanted to exist. I work on Fedora 44 on a ThinkPad T14s AMD Gen 6; that's the environment it'll be tested against. I welcome contributions but can only honestly support my own setup.

## Why this exists

The two apps that taught GTD to a generation fail in opposite ways:

- **Things 3** is calm and beautiful, and so deliberate about what it omits that power users eventually outgrow it — no defer dates, no review queue, no forecast, no sequential projects. You leave because the tool can't keep up with your system.
- **OmniFocus** is the opposite — every GTD knob exposed, every facet editable. Its failure mode is *fiddling with fields instead of doing tasks*. The tool keeps up so well it becomes the work.

Atrium's pitch: a user grows into Builder Mode when their system demands it, and falls back to Simple Mode on the days when their system doesn't, **without abandoning their data or their app**. The schema is the OmniFocus superset on day one. Simple Mode hides the Builder fields; it doesn't lack them. Switching modes is a UI flip, never a migration.

The other thing nobody on Linux is doing well: **plain-text interop**. Atrium's first-class export targets are its own JSON backup and **Org-mode** — `.org` files with TODO/DONE keywords, `SCHEDULED:` / `DEADLINE:` cookies, headline tags, and `:ID:` properties for round-trippable UUIDs. Your data stays portable to Emacs, to Logseq, to a pile of `.org` files in a git repo. The app is local-first. Nothing leaves the machine unless you tell it to.

## What it is

### Simple Mode (v0.1 — first release)

A direct Things 3 analogue for GNOME.

| | |
|---|---|
| **Lists** | Inbox · Today · Upcoming · Anytime · Someday · Logbook |
| **Hierarchy** | Areas → Projects → Tasks (with project Headings) |
| **Tags** | Multi-tag, orthogonal to areas/projects, with their own pages |
| **Dates** | Distinct *When* (scheduled-for) and *Deadline* — the Things 3 detail most clones get wrong |
| **Quick Entry** | Global GTK shortcut → small modal → drops to Inbox without stealing focus |
| **Search** | FTS5-backed, debounced, ranks by recency × relevance |
| **Keyboard-first** | Every common op bindable; mouse optional |
| **Storage** | One SQLite file at `$XDG_DATA_HOME/atrium/atrium.db`. Single-writer worker thread; UI never blocks on I/O |

### Builder Mode (v0.2)

Same schema. Same data. New surfaces:

| | |
|---|---|
| **Defer dates** | Tasks invisible in Today/Anytime until their `defer_until` passes |
| **Sequential projects** | Only the next incomplete task is "available" — the rest dim |
| **Forecast** | Calendar-axis layout of the next 30 days; drag to reschedule |
| **Review** | Projects with stale `last_reviewed_at` surface in a queue, oldest first |
| **Perspectives** | Saved filter expressions as first-class sidebar entries |
| **Inspector pane** | Right-side overlay exposing every Builder field |
| **Repeating tasks** | RFC 5545 RRULE-driven; respects Org's `+` / `++` / `.+` semantics |

### Imports and exports (toward 1.0)

Direct importers ship for the apps users actually migrate *from*:

- **Things 3** (JSON via the URL scheme on macOS)
- **OmniFocus** (`.ofocus` bundle)
- **Taskwarrior** (`task export` JSON)
- **Todoist** (CSV via the official export tool)
- **Org-mode** (two-way `.org` interop, with UUID round-trip via `:ID:`)
- **VTODO / RFC 5545** (`.ics`) — covers Endeavour, Errands, Apple Reminders, Nextcloud Tasks, Planify
- **todo.txt** and **TaskPaper** (plain text)

VTODO export is one-way — Atrium does not become a CalDAV client. The plan is to reach the Linux task ecosystem through two interop covenants — Org-mode and VTODO — rather than per-app importer sprawl.

## Status

**Pre-implementation.** As of v0.0.0, the repository contains the contract — `spec.md`, `roadmap.md`, this README, license, version, placeholder logo — and no code. Phase 0 (Cargo skeleton, CI, project scaffolding) begins after design sign-off.

The journey to v1.0 lives in [`roadmap.md`](roadmap.md), broken into 20 phases:

- **Phases 0–9** — Simple Mode → ships as **v0.1**
- **Phases 10–15** — Builder Mode → ships as **v0.2**
- **Phases 16–19** — imports and exports across the Linux productivity-app ecosystem
- **Phase 20** — l10n, accessibility round 2, capture daemon, Flathub → **v1.0**

## Architecture (in one paragraph)

A single GTK4 + libadwaita application written in Rust 2024. Storage is SQLite in WAL mode, with the schema modeled as the OmniFocus superset so Builder fields exist on every task from day one. A dedicated `tokio` worker task owns the writable connection; the UI thread holds an `mpsc::Sender<Command>` and reads through a separate read-only connection pool. UI updates arrive as `TaskChanges { created, updated, deleted, status_changed }` deltas via a `glib::MainContext` channel, never as full reloads. Mode (Simple / Builder) is a per-app GSettings flag — flipping it never touches the DB. See [`spec.md`](spec.md) §3 for the full architecture and §4 for the schema.

## Stack

- **Rust 2024 Edition**
- **GTK 4.16+** / **libadwaita 1.7+**
- **SQLite** via `rusqlite` (`bundled`, `chrono` features) — single-writer worker, WAL mode, FTS5 for search
- **`tokio`** runtime; **`chrono`** for dates; **`serde`** / **`serde_json`** for export formats; **`anyhow`** / **`thiserror`** for errors; **`tracing`** for diagnostics
- **Meson** wrapper over Cargo so Flatpak packaging is straightforward
- **Memory budget:** < 80 MB idle, < 200 MB active on a 10K-task DB, < 250 ms cold start on 5K tasks, < 50 ms Quick Entry latency

No third-party crate gets added without per-phase sign-off — see the dependency-check items in `roadmap.md`.

## Where things live

| File | What it is |
|---|---|
| [`spec.md`](spec.md) | The contract. Architecture, schema, UI deltas, import/export mapping, perf budget. |
| [`roadmap.md`](roadmap.md) | 20-phase plan from empty repo to 1.0. |
| [`patchnotes.md`](patchnotes.md) | Release notes, newest at top. |
| `CLAUDE.md` | Per-project guidance for AI-assisted development. |
| `data/` | `.ui` XML, icons, GSettings schema, AppStream metainfo, Flatpak manifest. |
| `src/` | Rust source. |
| `tests/` | Integration tests. |
| `docs/` | Schema reference, keymap, perf notes, RRULE supported subset. |

## Influences and acknowledgements

- **Things 3** (Cultured Code) — the calm-mode ideal Atrium opens with.
- **OmniFocus** (The Omni Group) — the depth-mode ideal Atrium grows into.
- **Org-mode** (Carsten Dominik, Bastien Guerry, et al.) — the plain-text covenant.
- **NetNewsWire** (Brent Simmons) — the single-writer SQLite worker discipline lifted from Viaduct.

## License

MIT. See [`LICENSE`](LICENSE).
