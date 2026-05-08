# Atrium — Application Specification

**Version:** 0.3.0 (Simple + Builder Mode shipped; visual polish pass complete)
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

v0.1 through v0.3 is a single GTK application binary. Phase 20 (v1.0) introduces an optional capture daemon (`atriumd`) running under user systemd that handles the global Quick Entry shortcut even when the main app is closed and IPCs the captured task in. Until that ships, Quick Entry works only when Atrium is running.

### 3.4 Debug-First Architecture

Testing and debugging tooling is part of the binary, not a separate harness. A `--debug` CLI flag opens a debug surface inside the running application that exposes:

- **Stress generators** — synthesize 10K / 50K / 100K-task fixture databases on demand so the §8 perf budget can be exercised without manual seeding.
- **Edge-case fixtures** — pre-canned weird states reachable from a debug menu: empty projects, deeply nested hierarchies, recurring rules at DST boundaries, malformed imports, clock-skewed timestamps, unicode-hostile titles.
- **IO instrumentation** — every SQLite statement (text, params, duration) and every file read/write logged through `tracing` spans into a debug pane.
- **Memory watch** — periodic RSS / heap sampling surfaced live, with a "drop caches" affordance to expose retained allocations and leaks.

Release builds carry the same code paths; the heavy generators and the debug pane are gated on `--debug` so end users never see them. The integration test suite reuses the same fixtures — there is no separate test-only fork. The skeleton lands in Phase 0 and grows phase by phase (see `roadmap.md`); no extra crates are required, since `tracing` / `tracing-subscriber` are already in the v0.1 dependency set.

### 3.5 Org Vault as Projection

Atrium ships an optional **two-way Org-mode mirror**: a user-designated directory (the "vault", default `~/Tasks/`) holding `.org` files that reflect the database state and accept edits back from Emacs / Doom / vim-orgmode / any Org-aware tool.

The discipline is **DB canonical, vault projected**:

- The SQLite database is the single source of truth for application state. Atrium runs cleanly with no vault configured.
- When a vault is configured, the worker emits a vault-write job after each commit to mirror the change into the right `.org` file.
- The vault is watched via `inotify`. External edits (Emacs, etc.) are parsed, diffed against the DB by `:ID:` property, and applied through the worker as TaskChanges deltas — the same plumbing as a local UI edit.
- Conflicts (simultaneous edits, malformed files) follow a **never silently lose data** policy: the loser is preserved at `<file>.atrium.bak.<timestamp>` with a UI toast surfaced.

The §8 perf budget is met by the database (indexed SELECTs, FTS5), while the user keeps a useful plain-text representation of their tasks they can edit in any Org tool. **The vault is not a sync client** — there is no remote, no merge protocol, just file-watching and best-effort round-tripping.

Vault layout: `<vault>/<Area>/<Project>.org`, with `inbox.org` at the vault root and an Atrium-only sidecar at `<vault>/.atrium/config.toml`. Full mapping in §7.3.

Three documented limitations:

1. **Complex `repeat_rule` values** beyond Org's repeater syntax (`+1w` / `++1w` / `.+1w`) are stored verbatim in `:RRULE:` and rendered as a best-effort approximation in the SCHEDULED cookie. Editing a complex repeat in Emacs may lose precision; simple repeats round-trip cleanly.
2. **Atrium-only metadata** (tag colors, saved Perspectives, mode preference) lives in the sidecar. Other Org tools ignore it.
3. **Unknown Org constructs** (custom keywords, drawers Atrium doesn't model, body content Atrium doesn't render) are preserved verbatim — never destroyed on round-trip.

Read-only sync (DB → vault) plus one-shot import from existing Org libraries ships in Phase 17. Full two-way sync (vault → DB via inotify) ships in Phase 17.5.

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
| `repeat_rule` | TEXT NULL | RFC 5545 RRULE; impl Phase 15 (v0.2.0) |
| `repeat_mode` | TEXT NULL | Org repeater cookie — `BASIC` / `CUMULATIVE` / `NEXT`; NULL falls back to default (CUMULATIVE). Phase 15 (v0.2.0) — column added via `0003_repeat_mode.sql` |
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
**`tag`** — (`id`, `uuid`, `name UNIQUE`, `color`, timestamps). The `color` column is wired end-to-end as of v0.3.0 — six-swatch picker in the editor, coloured dot in the sidebar tag row, coloured Pango pill on every task row.
**`task_tag`** — many-to-many join (`task_id`, `tag_id`).
**`heading`** — project subdivisions (`id`, `uuid`, `project_id`, `title`, `position`); Builder UI exposes editing in v0.1, Simple displays them inline as section breaks within a project.
**`perspective`** — (`id`, `uuid`, `name`, `icon`, `filter_expr`, `sort_order`, `grouping`, `position`, timestamps). Saved filter expressions surfaced as Builder-only sidebar entries. Phase 14 (v0.1.17) — added via `0002_perspectives.sql`. The `sort_order` and `grouping` columns ship now and stay unused by the v0.3 UI; they exist to absorb future per-perspective sort / grouping without another migration.

### 4.2 Derived views

Things-style lists are SELECTs, not stored rows:

- **Inbox:** `task WHERE project_id IS NULL AND completed_at IS NULL`
- **Today:** `task WHERE completed_at IS NULL AND (scheduled_for ≤ today OR deadline ≤ today + N) AND (defer_until IS NULL OR defer_until ≤ today)`, where `N = TODAY_DEADLINE_WINDOW_DAYS` (default `7`). The deadline window is the Things-3 "deadlines approaching" heads-up — a future-deadlined task surfaces in Today before it's actually due, so the user isn't blindsided. The window is a single constant in v0.1; turning it into a per-user GSettings key is a Phase 8d preferences task.
- **Anytime:** `task WHERE completed_at IS NULL AND scheduled_for IS NULL AND (defer_until IS NULL OR defer_until ≤ today)`
- **Someday:** `task WHERE completed_at IS NULL AND scheduled_for = '__someday__'` (sentinel)
- **Upcoming:** `task WHERE completed_at IS NULL AND scheduled_for > today`
- **Logbook:** `task WHERE completed_at IS NOT NULL`
- **Forecast (Builder):** Today + Upcoming windowed to 30 days, grouped by date axis

### 4.3 Search Expression Language

Phase 15.5 (v0.4.0) replaced the v0.1 flat filter parser with a Calibre-shaped expression grammar in `atrium-core/src/search/`. The language is the contract for the search bar, saved Perspectives (which store filter expressions verbatim), and any future scripting / import surface that wants to express a task query.

#### 4.3.1 Grammar

```text
expr      = or_expr
or_expr   = and_expr ( "OR" and_expr )*
and_expr  = not_expr ( ("AND")? not_expr )*       (implicit AND)
not_expr  = ( "NOT" | "!" ) not_expr | primary
primary   = "(" or_expr ")"
          | field ":" value_or_match
          | "is:" state
          | bareword                                (freeform text)
          | quoted_string                           (freeform text)
```

Precedence: `NOT > AND > OR` — standard boolean, matches Calibre, SQL, Python. `tag:work AND !done OR tag:home` parses as `(tag:work AND (NOT done)) OR tag:home`.

#### 4.3.2 Field operators

| Field | Aliases | Type | Example |
|---|---|---|---|
| `tag:` | `tags:` | text | `tag:errand` |
| `area:` | | text | `area:Personal` |
| `project:` | | text | `project:"Q3 plans"` |
| `title:` | | text | `title:milk` |
| `note:` | `notes:` | text | `note:"shopping list"` |
| `due:` | `deadline:` | date | `due:tomorrow` |
| `scheduled:` | `when:` | date | `scheduled:thisweek` |
| `defer:` | `defer_until:`, `deferred:` | date | `defer:>today` |
| `created:` | | date | `created:<lastweek` |
| `modified:` | `updated:` | date | `modified:thismonth` |
| `completed:` | `done:` | date | `completed:today` |
| `estimated:` | `est:`, `effort:` | number | `estimated:>30` |
| `repeats:` | `repeating:` | boolean | `repeats:true` |

#### 4.3.3 Match modifiers

Calibre's full match grammar applies on every text-shaped field. The default is substring (case-insensitive); explicit modifiers tighten or change the match shape.

| Syntax | Match kind | Example |
|---|---|---|
| `tag:x` | substring (default) | matches `worker`, `homework`, `Work` |
| `tag:"x y"` | quoted substring | for values with spaces |
| `tag:=x` | exact (case-insensitive) | matches `Work` only, not `worker` |
| `tag:"=x y"` | quoted exact | for exact values with spaces |
| `tag:~regex` | regex | full RE2 syntax via the `regex` crate; in-memory only — SQL translation falls back |
| `tag:true` | has at least one | task must have any tag |
| `tag:false` | has none | task must have no tags |

#### 4.3.4 Comparison operators

`=`, `!=`, `<`, `<=`, `>`, `>=` apply to date and numeric fields. Comparing against a date keyword that resolves to a range (`thisweek`, `thismonth`, etc.) uses the appropriate bound: `>thisweek` is "after the end of this week", `<thisweek` is "before the start of this week", `=thisweek` is "anywhere within this week".

#### 4.3.5 Range syntax

`field:lo..hi` (inclusive). `due:2026-05-01..2026-05-31`. The bounds may be literal dates or date keywords.

#### 4.3.5.1 Sort modifier (v0.4.1)

`sort:KEY` re-orders the result set after the predicate filter runs. Multiple sorts compose primary → secondary → tertiary in input order, so `sort:-due sort:title` orders by deadline descending and breaks ties alphabetically by title.

| Syntax | Meaning |
|---|---|
| `sort:KEY` | ascending order on `KEY` |
| `sort:-KEY` | descending order on `KEY` |

Recognised keys: `due` (alias `deadline`), `scheduled` (alias `when`), `defer` (alias `deferred`, `defer_until`), `created`, `modified` (alias `updated`), `completed` (alias `done`), `estimated` (alias `est`, `effort`), `title`, `position` (alias `manual`).

NULLs sort last regardless of direction (SQL's `NULLS LAST` convention) — a task with no `deadline` always lands at the bottom of `sort:due` and `sort:-due` alike. When no `sort:` modifier is present, the result keeps the source list's `position` order.

`sort:` is metadata on the result set, not a per-task predicate; the parser strips it from the AST and surfaces it on `ParseResult.sorts` so the evaluator never sees a sort modifier as a filter. Saved Perspectives written against v0.4.1's grammar inherit sort modifiers verbatim — `tag:work sort:-due` saves the work and the order both.

#### 4.3.6 Date keywords

Calibre's date-keyword vocabulary plus future-tense forms Atrium needs (Calibre's library is past-only).

| Keyword | Meaning |
|---|---|
| `today` | the current date |
| `yesterday`, `tomorrow` | ±1 day |
| `thisweek`, `lastweek`, `nextweek` | Mon–Sun ISO week |
| `thismonth`, `lastmonth`, `nextmonth` | calendar month |
| `thisyear` | calendar year |
| `Ndaysago` | N days before today |
| `Ndaysout` | N days after today |

#### 4.3.7 State predicates

`is:NAME` shortcuts that read directly off task fields without taking a value. Each pairs with `!is:NAME` (or `NOT is:NAME`) for the inverse.

| Predicate | Meaning |
|---|---|
| `is:open` | `completed_at IS NULL` |
| `is:done`, `is:logbook` | `completed_at IS NOT NULL` |
| `is:overdue` | open AND `deadline < today` |
| `is:scheduled` | has a `scheduled_for` |
| `is:deadline` | has a `deadline` |
| `is:deferred` | has a `defer_until > today` |
| `is:repeating` | has a `repeat_rule` |
| `is:archived` | belongs to a project with `archived_at IS NOT NULL` |
| `is:project` (or `is:in_project`) | has a `project_id` |
| `is:area` (or `is:in_area`) | belongs (transitively) to an area |
| `is:tagged` | has at least one tag |
| `is:queued` | sequential project, not the first incomplete task |
| `is:available` | first incomplete task in a sequential project, OR not in one and not deferred |
| `is:today` | mirrors the Today list (§4.2): open AND (Schedule ≤ today OR Deadline ≤ today + 7) AND defer-resolved |
| `is:inbox` | mirrors the Inbox list: open AND no project assignment |
| `is:upcoming` | mirrors the Upcoming list: open AND `scheduled_for` is a date strictly in the future |
| `is:anytime` | mirrors the Anytime list: open AND no `scheduled_for` AND defer-resolved |
| `is:someday` | mirrors the Someday list: open AND `scheduled_for` = Someday sentinel |

#### 4.3.8 Forgiving parser

Unknown field names and unknown `is:NAME` predicates are non-fatal warnings, not errors — the unrecognised token falls through to freeform text. The window-side surfaces a toast and a yellow tint on the search entry; the user keeps typing without losing what they had. This shape is what lets the spec add new operators in future minor releases without breaking existing saved Perspectives.

#### 4.3.9 Persistence

The expression text — exactly what the user typed — is what's stored in `perspective.filter_expr`. Re-parsing on every load means a saved Perspective written against v0.4.0's grammar inherits operator additions in v0.5.0+ for free.

### 4.4 FTS5

`task_fts` virtual table indexes `title` + `note`. Triggers keep it in sync on INSERT/UPDATE/DELETE. Search is `Ctrl+F` debounced 200 ms, ranks by recency × relevance.

### 4.5 Migrations

Schema versioned via SQLite `user_version` PRAGMA. Migrations live in `src/db/migrations/<NNNN>_*.sql`.

v0.1 shipped with `0001_initial.sql` (the full OmniFocus superset). During v0.1 the rule was **no breaking schema changes** — purely-additive migrations were allowed (`0002_perspectives.sql` at v0.1.17 added the `perspective` table; v0.2.0 marks the end of the v0.1 freeze).

Post-v0.2.0, the discipline is **append-only and backwards-compatible**. `ALTER TABLE … ADD COLUMN` is allowed. `0003_repeat_mode.sql` (v0.2.0) is the first migration to alter an existing table, adding `task.repeat_mode TEXT NULL` for Phase 15's repeater semantics. Renaming or dropping columns is a major-bump-only operation. Constraint changes that could fail on existing rows (adding `NOT NULL`, tightening a `UNIQUE`, retargeting an FK) need a backfill step and explicit sign-off.

Migrations are never rewritten once shipped — old databases must replay the same SQL the first version that introduced them ran. This means a fresh install at any version walks the full migration list from `0001` forward.

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

### 5.2 Builder Mode (v0.2.0 — shipping)

Adds, all wired end-to-end as of v0.2.0:

- **Forecast** — calendar-axis layout of next 30 days, drag-to-reschedule (Phase 12).
- **Review** — projects with stale `last_reviewed_at` surface here, oldest first; per-card *Mark Reviewed* button stamps the timestamp (Phase 13).
- **Perspectives** — saved filter expressions stored as `perspective` rows, surfaced in the sidebar's Builder section. *Save Search as Perspective…* in the primary menu captures the current search bar query (Phase 14, v0.1.17).
- **Inspector pane** — right-side `AdwOverlaySplitView` companion, autosaves every field on focus-out / Enter (Phase 10).
- **Defer dates + sequential projects** — `defer_until` excludes from Today/Anytime; sequential rendering dims rows past the first incomplete one (Phase 11).
- **Repeat rules** — full RFC 5545 RRULE with three Org-style completion modes (Cumulative default, Next-from-completion, Basic). Editor in the Inspector pane; worker regenerates the next instance on completion. Schema-side, `repeat_mode` was added via `0003_repeat_mode.sql` — the first migration to alter an existing table, allowed because v0.2.0 ends the v0.1 freeze (Phase 15).

The widget tree is the same; Inspector and Forecast are added as sibling content pages, the sidebar gains a Perspectives section, and the task editor's collapsed/expanded fieldset grows. **No DB work happens on mode switch.** Verified by `tests/mode_flip_snapshot.rs`.

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

When an Org vault is configured (see §3.5), Atrium projects the data model into a directory of `.org` files. The mapping below is the contract for one-shot import, ongoing read-only sync (Phase 17), and the full two-way sync (Phase 17.5).

#### 7.3.1 Vault layout

```
~/Tasks/                              ← vault root (configurable, default $HOME/Tasks)
├── inbox.org                         ← uncategorized tasks
├── .atrium/
│   └── config.toml                   ← Atrium-only metadata (tag colors, perspectives, mode pref)
├── Personal/                         ← Area = directory
│   ├── Errands.org                   ← Project = file
│   └── Reading.org
└── Work/
    ├── Q3.org
    └── Onboarding.org
```

Each `.org` file is one project. The file's `#+TITLE:` line carries the project title (the file's first headline is the fallback). Headlines without TODO keywords inside a project file are project sub-headings (`heading` table); headlines with TODO/DONE/CANCELLED keywords are tasks. Subtasks are nested headlines under their parent task. Unfiled projects live as `.org` files at the vault root next to `inbox.org`.

#### 7.3.2 Field mapping

| Atrium concept | Org representation |
|---|---|
| Vault root | User-configurable path; default `~/Tasks/` |
| Area | Directory under vault root |
| Project | `.org` file inside an area directory (or vault root for unfiled) |
| Project sub-heading (`heading`) | Non-TODO headline within a project file |
| Task | Headline with TODO / DONE / CANCELLED keyword |
| Subtask (`parent_id`) | Nested headline under its parent task |
| `title` | Headline text |
| `note` | Headline body — preserved verbatim, including unmodeled constructs |
| `tags` | `:tag1:tag2:` headline tags |
| Status (open / done / cancelled) | TODO / DONE / CANCELLED keyword |
| `scheduled_for` | `SCHEDULED:` cookie |
| `deadline` | `DEADLINE:` cookie |
| `completed_at` | `CLOSED:` cookie |
| `defer_until` | `:DEFER_UNTIL:` property |
| `estimated_minutes` | `Effort` property (Org-standard) |
| `repeat_rule` (canonical) | `:RRULE:` property (verbatim RFC 5545) |
| `repeat_rule` (rendered) | `+1w` / `++1w` / `.+1w` cookie on SCHEDULED / DEADLINE, when expressible |
| `uuid` | `:ID:` property — the round-trip anchor |
| `created_at` | `:CREATED:` property |
| `modified_at` | `:MODIFIED:` property |
| `sequential` (project) | `:SEQUENTIAL: t` property in file's `:PROPERTIES:` block |
| `review_interval_days` (project) | `:REVIEW_INTERVAL:` property |
| `last_reviewed_at` (project) | `:LAST_REVIEWED:` property |
| `archived_at` (project) | `:ARCHIVED:` property + `ARCHIVE` tag |
| `position` (ordering) | Implicit by file order; reorder = file rewritten in new order |
| Tag color, perspectives, mode pref | Atrium-only sidecar (`<vault>/.atrium/config.toml`) |

#### 7.3.3 Round-trip rules

These rules govern Atrium's behaviour on every read and write of a vault file:

1. **Never destroy data.** Anything Atrium doesn't model — custom TODO keywords (`WAITING`, etc.), unknown drawers, body content with constructs Atrium doesn't render — is preserved verbatim. Custom keywords map to a sentinel state on import; the original is stashed in `:ORIG_KEYWORD:` and restored on export.
2. **`:ID:` is the round-trip anchor.** Tasks imported without `:ID:` receive one and the file is rewritten with the property added. Tasks edited in Emacs and saved must keep their `:ID:` for the next vault read to recognise them as the same row.
3. **Best-effort RRULE rendering.** Simple repeats (fixed-interval daily / weekly / monthly with the three Org completion semantics) render to `+1w` / `++1w` / `.+1w` and round-trip cleanly. Complex RRULEs (BYDAY filters, COUNT, EXDATE, etc.) are stored canonical in `:RRULE:` and approximated in the SCHEDULED cookie. Editing a complex repeat in Emacs may lose precision — Atrium surfaces this in the post-sync report.
4. **Sidecar metadata is Atrium-only.** `<vault>/.atrium/config.toml` holds tag colors, saved Perspectives, and the mode preference. Other Org tools ignore it. Deleting the sidecar loses Atrium-side state but never task data.
5. **Conflicts are surfaced, not silenced.** Simultaneous edits → last-writer-wins by mtime; the loser is preserved at `<file>.atrium.bak.<timestamp>` and surfaced in a UI toast. Malformed file → vault sync paused for that file with a toast; DB version preserved; auto-resume when the file parses again.
6. **Atomic file writes.** Every Atrium-side vault write is `write-temp + fsync + rename`, never partial. Crash mid-write leaves the previous version intact.

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
