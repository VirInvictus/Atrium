# Atrium — Application Specification

**Version:** 0.0.0 (pre-implementation)
**Target:** GNOME 50+, GTK4 ≥ 4.16, libadwaita ≥ 1.7
**Language:** Rust (2024 Edition)
**Build System:** Cargo / Meson wrapper for Flatpak packaging
**License:** MIT

---

## 1. Mission Statement

Atrium is a native GNOME task manager that fuses Things 3's clarity with OmniFocus's depth into a single application via a **mode switch over a shared data store**. Users pick the cognitive load that matches their day — Simple Mode for *what am I doing right now*, Builder Mode for full GTD review, deferral, sequential projects, and forecast — without migrating data.

Design philosophy: **One Store, Two Surfaces.** Tasks created in Simple Mode are real tasks with empty Builder fields. Builder Mode picks them up without conversion. The user can flip back without losing work. The app is local-first, no sync, no cloud, no telemetry.

The two source apps fail in opposite ways: Things 3 makes you outgrow it, OmniFocus makes you procrastinate by adjusting fields instead of doing tasks. Atrium's pitch is that a user can grow into Builder Mode when their system demands it without abandoning the calmer Simple Mode for the days when their system doesn't.

---

## 2. Core Mandates

- **Local-first.** SQLite at `$XDG_DATA_HOME/atrium/atrium.db`. No remote backend.
- **Native GNOME.** GTK4 + libadwaita 1.7+. No web tech in the UI surface.
- **Performance.** 10,000 tasks render at the same speed as 100. Single-writer SQLite worker; UI thread never blocks on I/O.
- **Mode-as-view.** Mode is a per-app preference. Schema and data are universal. Builder fields exist on every task; Simple Mode hides them.
- **Quick Entry sacred.** Capture is one shortcut, one keystroke. Quick Entry is identical in both modes.
- **No data loss on mode switch.** Round-trip Simple → Builder → Simple preserves everything Builder set.
- **Plain-text interop.** Org-mode is a first-class import *and* export target — Atrium does not silo your data.

---

## 3. Architecture

### 3.1 Mode-as-View

The Simple/Builder decision is a UI-layer toggle that adjusts:

- Which fields the task editor exposes
- Which navigation views are visible (Forecast and Review are Builder-only)
- Which menu items appear
- The default density of list rows

It does **not**:

- Affect schema
- Affect what the data layer reads or writes
- Migrate, transform, or hide rows
- Constrain Quick Entry behaviour

A Simple Mode user who never opens Builder Mode never sees defer dates, sequential projects, review intervals, or perspectives. Their data nonetheless populates those columns with NULL/false/sane defaults, so a future flip to Builder Mode is trivially supported.

**Design risk acknowledged up front:** Simple Mode must feel like *Things*, not *Builder with the advanced fields hidden*. Things isn't simple because it has fewer features — it's simple because every visible feature respects the user's attention. Simple Mode is a complete, opinionated experience; it is not a feature-flag-disabled subset.

### 3.2 Single-Writer SQLite Worker

A dedicated `tokio` task owns the writable `rusqlite::Connection`. The GTK thread holds an `mpsc::Sender<Command>` and never touches the writable connection directly. Reads use a separate read-only connection pool that the worker does not own. WAL mode is mandatory.

This mirrors Viaduct's `DatabaseQueue` analog. The pattern eliminates an entire class of UI-thread-blocking and write-conflict bugs.

```text
GTK main thread ──Command──▶ Writer task (tokio) ──▶ SQLite (rusqlite, WAL)
       ▲                            │
       └──────TaskChanges───────────┘   (via glib::MainContext::channel)

GTK main thread ──direct read──▶ SQLite read-only connection pool (separate handles)
```

`TaskChanges` is a coalescing batch type containing `created`, `updated`, `deleted`, and `status_changed` sets. UI updates apply as deltas, never full reloads.

### 3.3 Process Topology

v0.1 is a single GTK application binary. v0.2 introduces an optional capture daemon (`atriumd`) running under user systemd that handles the global Quick Entry shortcut even when the main app is closed and IPCs the captured task. Until v0.2 lands, Quick Entry works only when Atrium is running.

---

## 4. Data Model

OmniFocus superset. Every Builder column lives in v0.1 schema; only some are exposed in Simple Mode.

### 4.1 Tables (sketch)

**`task`**
| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | rowid |
| `uuid` | TEXT NOT NULL UNIQUE | for round-trippable export/import |
| `title` | TEXT NOT NULL | |
| `note` | TEXT NOT NULL DEFAULT '' | |
| `project_id` | INTEGER NULL FK → project | NULL = Inbox |
| `parent_id` | INTEGER NULL FK → task | subtasks; Builder-only UI in v0.1 |
| `scheduled_for` | TEXT NULL | ISO date; *When* in Simple |
| `deadline` | TEXT NULL | ISO date |
| `defer_until` | TEXT NULL | Builder-only; hidden in Simple |
| `estimated_minutes` | INTEGER NULL | Builder-only |
| `completed_at` | TEXT NULL | ISO datetime; NULL = not done |
| `repeat_rule` | TEXT NULL | RFC 5545 RRULE; impl Phase 15 |
| `created_at` | TEXT NOT NULL | ISO datetime |
| `modified_at` | TEXT NOT NULL | ISO datetime, trigger-maintained |
| `position` | REAL NOT NULL | for ordering within parent |

**`project`**
| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `uuid` | TEXT NOT NULL UNIQUE | |
| `title` | TEXT NOT NULL | |
| `note` | TEXT NOT NULL DEFAULT '' | |
| `area_id` | INTEGER NULL FK → area | |
| `sequential` | INTEGER NOT NULL DEFAULT 0 | Builder-only; only first incomplete child task is "available" |
| `review_interval_days` | INTEGER NULL | Builder-only |
| `last_reviewed_at` | TEXT NULL | Builder-only |
| `archived_at` | TEXT NULL | Logbook semantics for completed projects |
| `created_at`, `modified_at`, `position` | | |

**`area`** — top-level grouping (`id`, `uuid`, `title`, `position`, timestamps).
**`tag`** — (`id`, `uuid`, `name UNIQUE`, `color`, timestamps).
**`task_tag`** — many-to-many join (`task_id`, `tag_id`).
**`heading`** — project subdivisions (`id`, `uuid`, `project_id`, `title`, `position`); Builder UI exposes editing in v0.1, Simple displays them inline as section breaks within a project.

### 4.2 Derived views

Things-style lists are SELECTs, not stored rows:

- **Inbox:** `task WHERE project_id IS NULL AND completed_at IS NULL`
- **Today:** `task WHERE completed_at IS NULL AND (scheduled_for ≤ today OR deadline ≤ today) AND (defer_until IS NULL OR defer_until ≤ today)`
- **Anytime:** `task WHERE completed_at IS NULL AND scheduled_for IS NULL AND (defer_until IS NULL OR defer_until ≤ today)`
- **Someday:** `task WHERE completed_at IS NULL AND scheduled_for = '__someday__'` (sentinel)
- **Upcoming:** `task WHERE completed_at IS NULL AND scheduled_for > today`
- **Logbook:** `task WHERE completed_at IS NOT NULL`
- **Forecast (Builder):** Today + Upcoming windowed to 30 days, grouped by date axis

### 4.3 FTS5

`task_fts` virtual table indexes `title` + `note`. Triggers keep it in sync on INSERT/UPDATE/DELETE. Search is `Ctrl+F` debounced 200 ms, ranks by recency × relevance.

### 4.4 Migrations

Schema versioned via SQLite `user_version` PRAGMA. Migrations live in `src/db/migrations/<NNNN>_*.sql`. v0.1 ships with `0001_initial.sql` containing the full superset. **No mid-v0.1 schema changes:** any v0.1 schema change is a breaking dev change. Backwards-compat begins at v0.2.

---

## 5. User Interface

### 5.1 Simple Mode (v0.1)

The default mode for new installations. Layout cribs Things 3's three-pane:

```text
AdwApplicationWindow
└── AdwNavigationSplitView
    ├── [sidebar] AdwNavigationPage "Lists"
    │   └── GtkListView (TreeListModel)
    │       ├── Inbox (count badge)
    │       ├── Today (count badge)
    │       ├── Upcoming
    │       ├── Anytime
    │       ├── Someday
    │       ├── Logbook
    │       ├── ── Areas ──
    │       │   ├── <Area>
    │       │   │   └── <Project> (count badge)
    │       │   └── ...
    │       └── ── Tags ── (collapsible)
    └── [content] AdwNavigationPage "<active list>"
        └── GtkListView of tasks
```

**Visible task fields:** title, note, scheduled (When), deadline, tags, completion checkbox.
**Hidden task fields:** `defer_until`, `estimated_minutes`, `repeat_rule` editor, `parent_id` (no subtask UI in Simple).
**Hidden views:** Forecast, Review, Perspectives.
**Hidden project fields:** `sequential`, `review_interval_days`.

### 5.2 Builder Mode (v0.2+)

Adds:

- **Forecast** — calendar-axis layout of next 30 days
- **Review** — projects with stale `last_reviewed_at` surface here
- **Perspectives** — saved filter expressions, sidebar section
- **Inspector pane** — right-side `AdwOverlaySplitView` exposing every Builder field
- Sequential project rendering, defer-date scheduling, repeat-rule editor, estimated-time stamps

The widget tree is the same; Inspector and Forecast are added as sibling content pages, the sidebar gains a Perspectives section, and the task editor's collapsed/expanded fieldset grows. **No DB work happens on mode switch.**

### 5.3 Mode Switch

`Settings → Mode → [Simple, Builder]`. Switching is instant — settings flag flip plus re-render of menus and editor. Persisted across launches in GSettings.

---

## 6. Quick Entry

A global GTK shortcut (default `Ctrl+Alt+Space`) opens a small modal that:

- Drops a new task into Inbox
- Accepts inline `#tag` syntax (creates tag on first use)
- Accepts inline `@today`, `@tomorrow`, `@yyyy-mm-dd`, `@deadline yyyy-mm-dd` syntax
- Closes on Enter (commit) or Esc (discard)
- Is identical in both modes
- Does not steal focus from the previously focused window

If Atrium is closed, in v0.1 the shortcut launches it minimised and posts the task. v0.2 introduces `atriumd` (user systemd) for true zero-launch capture.

---

## 7. Imports & Exports

Imports are best-effort: each source has a documented mapping table; lossy fields are surfaced in a post-import report. Each importer ships with a dry-run mode that shows what would be created without touching the DB.

### 7.1 Import sources

| Source | Format | Phase | Notes |
|---|---|---|---|
| **Things 3** | JSON via Things URL scheme on macOS | 16 | Brandon's source app — primary user migration |
| **Org-mode** | `.org` plain text | 17 | TODO/DONE keywords, SCHEDULED/DEADLINE/CLOSED, headline tags, properties drawer |
| **OmniFocus** | `.ofocus` bundle XML | 18 | Bundle of XML files; transactions to fold |
| **Taskwarrior** | `task export` JSON | 19 | Well-documented; UDA fields → tags or notes per user choice |
| **Todoist** | CSV via official export tool | 19 | Project hierarchy; comments → notes |
| **VTODO** (RFC 5545) | `.ics` | 19 | Covers Endeavour, Errands, Apple Reminders, Nextcloud Tasks, Planify (CalDAV) |
| **todo.txt** | plain text | 19 | `(A)` priority, `+project`, `@context`, `due:` |
| **TaskPaper** | plain text | 19 | Headlines + `@tags`, `@done` |

### 7.2 Export targets

| Target | Format | Phase | Notes |
|---|---|---|---|
| **Atrium native backup** | JSON, includes UUIDs and Builder fields | 17 | Universal lossless dump |
| **Org-mode** | `.org`, two-way-ready | 17 | First-class plain-text covenant |
| **VTODO** | `.ics` | 19 | One-way file dump for CalDAV apps |
| **Markdown** | per-list `.md` | nice-to-have, no phase | Human-readable archive |

Atrium does **not** act as a CalDAV client in v1.0. VTODO export is a one-way file dump intended for archival or hand-off to apps like Endeavour, Errands, or Planify.

### 7.3 Org-mode mapping

| Atrium concept | Org concept |
|---|---|
| Project | Top-level headline tagged `:project:` |
| Heading | Sub-headline within project |
| Task | Headline with TODO/DONE/CANCELLED keyword |
| `scheduled_for` | `SCHEDULED:` cookie |
| `deadline` | `DEADLINE:` cookie |
| `completed_at` | `CLOSED:` cookie |
| `defer_until` | Custom `:DEFER:` property |
| `tags` | `:tag1:tag2:` headline tags |
| `note` | Headline body text |
| `repeat_rule` | `+1w` / `++1w` / `.+1w` cookies on SCHEDULED/DEADLINE |
| `uuid` | `:ID:` property drawer entry |
| `estimated_minutes` | `Effort` property (in minutes) |

UUIDs preserved in `:ID:` make round-trip (import → edit elsewhere → re-import) safe.

### 7.4 Linux productivity-app landscape

Apps Atrium will share users with, sorted by likely import demand:

| App | Stack | Storage | Importable in v1.0? |
|---|---|---|---|
| **Errands** (was List) | GTK4 / Vala | local + optional CalDAV | via VTODO (Phase 19) |
| **Planify** | GTK4 / Vala | local + Todoist / CalDAV sync | via Todoist CSV or VTODO (Phase 19) |
| **Endeavour** (formerly GNOME To Do) | GTK4 / C | Evolution Data Server | via VTODO (Phase 19) |
| **Getting Things GNOME (GTG)** | GTK / Python | XML files | not yet — XML format research deferred post-1.0 |
| **Taskwarrior** | TUI / C++ | JSON-on-disk | direct (Phase 19) |
| **Things 3** | macOS native | proprietary | direct (Phase 16) |
| **OmniFocus** | macOS native | `.ofocus` bundle | direct (Phase 18) |
| **Todoist** | proprietary cloud | CSV/JSON export | direct (Phase 19) |
| **Vikunja** | self-hosted web | API | not yet — out of scope for v1.0 |
| **Super Productivity** | Electron | JSON export | not yet — assess in v1.1 |
| **Logseq / AppFlowy** | Electron block editors | JSON / Markdown | not yet — block-editor semantics differ enough to defer |

The strategic choice: support **VTODO/CalDAV interop** (Phase 19) and **Org-mode** (Phase 17) as two complementary covenants. VTODO covers the GNOME/CalDAV ecosystem broadly; Org covers the Emacs/plain-text crowd. Together they reach almost every Linux task user without per-app importer sprawl.

---

## 8. Memory & Performance Targets

Atrium is lighter than Viaduct (no WebKit), but discipline still matters:

- **Idle:** < 80 MB after full library load
- **Active:** < 200 MB during heavy use (10K-task forecast view, search active)
- **Cold start:** < 250 ms to first interactive frame on a 5K-task DB
- **Quick Entry latency:** < 50 ms from shortcut to focused entry field

Each phase ends with a `heaptrack`/`massif` measurement note. Features that miss budget get gated or revised.

---

## 9. Out of Scope (for v1.0)

- Sync of any kind (CalDAV client, iCloud, Todoist, custom server)
- Mobile or web clients
- Team/shared task lists or multi-user accounts
- Time tracking (estimates yes; logging time spent no)
- Calendar event creation (deadlines are tasks, not calendar events)
- AI features in v1.0 — the mission is a fast, predictable task app

These are post-1.0 considerations and not roadmap items.

---

## 10. Project Conventions

Standard layout:

- `README.md`, `spec.md` (this file), `roadmap.md`, `patchnotes.md`, `CLAUDE.md`
- `VERSION` is the single source of truth; `Cargo.toml` matches
- `LICENSE` (MIT), `logo.svg`
- `data/` — `.ui` XML files, icons, GSettings schema, AppStream metainfo, Flatpak manifest
- `src/` — Rust source
- `tests/` — integration tests
- `docs/` — schema, keymap, perf notes, RRULE supported subset

CI matches Viaduct: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` on Linux. Tests required from day one.
