# Atrium — Application Specification

**Version:** 0.7.7 (Phase 15.75 complete; v0.6.x screenshot-cleanup arc complete; v0.6.18 efficiency pass; v0.6.19 roadmap revision retiring the Things 3 import phase, promoting Org-mode + Todoist to first-class import slots, and adding Phase 19.5 — productivity essentials surfaced by the gap-analysis pass against Errands / Planify / Endeavour / Things 3 / OmniFocus / Taskwarrior / Todoist; v0.7.0 visual-fusion + whitespace polish pass; v0.7.1 surface continuity follow-up; v0.7.2 Review canonical page absorbs the Weekly Review perspective + sidebar "Lists" auto-title retired; v0.7.3 inspector check-off + perspective editor dialog; v0.7.4 task-level Mark Reviewed via migration 0006 + per-row button on the weekly walk + 7-day exclusion; v0.7.5 visual refinement pass; v0.7.6 Phase 16 foundation — sync module + atomic-write helper + vault-path GSettings, hand-rolled Org parser instead of a third-party dep; v0.7.7 hand-rolled Org parser lands — atrium-core::sync::org parses headlines + TODO/DONE/CANCELLED keywords + SCHEDULED/DEADLINE/CLOSED cookies + repeater suffixes + headline tags + :PROPERTIES: drawer + body + nested subtasks; v0.7.8 adds the matching emitter so the parse → emit → re-parse round-trip is stable for every spec §7.3 construct, with `emit_org_file` going through the v0.7.6 atomic-write helper; v0.7.9 ships the one-shot importer — `atrium-cli import org PATH [--dry-run]` reads a single .org file, treats it as one Project, maps every TODO-keyworded headline to a Task with the spec §7.3.2 field mapping, preserves :ID: as task uuid (NewTask.uuid additive field), and surfaces lossy fields in the summary; v0.7.10 ships the inverse — `atrium-cli export org PATH [--dry-run]` writes every project in the DB to a vault directory at PATH, atomic per spec §7.3.3 rule 6, with the field mapping reversing the importer; v0.7.11 adds the lossless JSON snapshot export — `atrium-cli export json PATH` writes the entire DB state (areas, projects, headings, tasks, tags, task_tags, perspectives) into a single versioned JSON file via atrium-core::sync::json::Snapshot; v0.7.12 closes spec §7.3.3 rule 1's custom-keyword round-trip via migration 0007's task.orig_keyword column — the Org importer stashes non-canonical TODO keywords (WAITING, BLOCKED, IN-PROGRESS) and the writer restores them on emit, so headlines round-trip through Atrium without losing their label; v0.7.13 closes the project-level half — file-level metadata (`#+TITLE:` directive + a top-level `:PROPERTIES:` block carrying `:ID:` / `:SEQUENTIAL:` / `:REVIEW_INTERVAL:` / `:LAST_REVIEWED:` / `:ARCHIVED:`) round-trips through `parse_org_text_with_meta` / `emit_org_text_with_meta` plus the OrgFile struct + NewProject's new last_reviewed_at / archived_at fields; v0.7.14 ships the multi-file vault walk — `atrium-cli import org PATH` now accepts a directory and walks `<vault>/<area>/<project>.org` mapping subdirectories onto Atrium areas via the new `WorkerHandle::ensure_area` idempotent-create-by-name helper; v0.7.15 adds the post-write Org integrity check — every `emit_org_file_with_meta` write re-reads the file and verifies it parses cleanly through Atrium's own reader, propagating any failure as an `io::Error` per spec §7.3.3 / Phase 16 roadmap; v0.7.16 lands the auto-debounced worker write hook — `WorkerHandle::spawn_with_vault(conn, VaultConfig { root, read_pool })` spawns a background `VaultWriter` task that receives `ProjectDirty(project_id)` notifications from every Task / Project / Tag write the worker dispatches, debounces ~100ms, and rewrites the project's `.org` file via the v0.7.10 vault writer + v0.7.15 integrity check; v0.7.17 adds the round-trip test fixture (5 complicated `.org` files round-tripped through the full importer + writer + parser, asserting AST equality between source and regenerated trees) and tightens two real importer gaps surfaced by the suite — NewTask gains an additive `completed_at: Option<DateTime>` so the importer threads source CLOSED cookies through verbatim instead of stamping `now()`, and CANCELLED keywords now stash in `orig_keyword` so they survive the round-trip; v0.7.18 wires the GUI vault integration — the GTK binary now reads the `vault-path` GSettings key on boot and, when non-empty, calls `spawn_worker_with_vault` so every DB write auto-flushes to the vault)
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

The workspace ships four crates as of v0.5.0:

- **`atrium-core`** — headless data layer (domain types, SQLite worker, paths, repeat-rule wrapper). GUI-free; the foundation every other crate builds on.
- **`atrium-search`** — Calibre-style search expression language (lex / parse / ast / eval). Extracted from atrium-core in v0.4.2 so the engine can be exercised, fuzzed, and reused independently.
- **`atrium-cli`** — headless binary that exposes the search engine and full task CRUD (search / list / info / add / capture / edit / complete / delete) from the shell. TSV by default for shell pipelines, `--json` for jq, `--human` for terminal viewing. Read commands open the database read-only as a process-level safety guarantee; write commands spin up the worker on a current-thread tokio runtime, send commands via WorkerHandle, and shut down cleanly.
- **`atrium`** — the GTK4 binary. Depends on all three above.

The architectural commitment: every non-GUI surface stays CLI-testable. The 2.0-era TUI (`atrium-tui`) is the same shape — another headless consumer of atrium-core + atrium-search. Phase 20 (v1.0) introduces an optional capture daemon (`atriumd`) running under user systemd that handles the global Quick Entry shortcut even when the main app is closed and IPCs the captured task in. Until that ships, Quick Entry works only when Atrium is running.

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
| `tag:?value` | fuzzy | Damerau-Levenshtein within a length-aware threshold (≤4 chars → 1, 5–7 → 2, ≥8 → 3); transpositions count as a single edit so `tag:?wrok` matches `work`. In-memory only. |
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

`task_fts` virtual table indexes `title` + `note`. Triggers keep it in sync on INSERT/UPDATE/DELETE. Search is `Ctrl+F` debounced 200 ms.

**Bm25 + recency ranking (v0.5.2).** When the search expression contains bare-text terms (`Expr::Text` nodes) and the user hasn't pinned a `sort:` modifier, results rank by FTS5's `bm25` blended with a 30-day half-life recency factor:

```text
score = (|bm25| / (1 + |bm25|)) + 0.25 · 2^(-Δd / 30)
```

The relevance term is the saturating mapping `|bm25| / (1 + |bm25|)` — keeps relevance on a stable [0, 1) scale regardless of FTS5's per-DB magnitudes. The recency factor is a quarter-weight tiebreaker so freshly-touched matches edge out lukewarm older ones without dominating the ranking. Both `atrium_search::collect_text_terms` and `blend_relevance` are pure helpers; `atrium-core::db::read::bm25_for_terms` is the DB-side query; `atrium/ui/filter::rank_by_bm25_recency` and `atrium-cli::run_search` are the consumers.

### 4.5 SQL-translation evaluator

The Calibre-style search expression engine has two execution paths:

- **In-memory path.** `atrium_search::evaluate(&Expr, &Task, &EvalContext)` walks the AST against an already-loaded `Vec<Task>`. Handles every operator the grammar exposes — including the SQL-incompatible ones (regex, fuzzy, sequential-project state).
- **SQL path (v0.5.3).** `atrium_search::try_translate(&Expr, today) -> Option<SqlClause>` walks the same AST and emits a SQL `WHERE` fragment + parameter list when *every* node maps cleanly. Returns `None` for any subtree the translator can't safely express; the call site falls back to the in-memory path.

The "all-or-nothing" rule keeps semantics in lockstep — there's no shape where SQL and in-memory paths could disagree silently. An in-tree parity-test battery (`atrium-cli/src/tests.rs::sql_parity`, 21 cases) seeds a mixed fixture and asserts both paths return the same id set across every operator class the translator covers, plus negative tests confirming `try_translate` correctly rejects regex / fuzzy / `is:today`.

Coverage as of v0.5.3: boolean composition (AND / OR / NOT / Pass), bare text → `LOWER(title|note) LIKE ?`, state predicates (`open`, `done`, `overdue`, `scheduled`, `deadline`, `deferred`, `repeating`, `inproject`, `tagged`), field-scoped substring/exact on `title:` / `note:` / `tag:`, `repeats:true|false`, date comparisons + ranges on `due` / `scheduled` / `defer` / `created` / `modified` / `completed`, numeric comparison on `estimated:`. Falls back: regex, fuzzy, `Available` / `Queued`, the composite `is:today / is:inbox / is:upcoming / is:anytime / is:someday`, `Field::Project|Area`. The fall-back set is doable in future patches; the "all-or-nothing" guarantee is the current backstop.

### 4.6 Perspective renderers (Slice D)

`perspective.renderer` (TEXT, default `'list'`) and `perspective.renderer_config` (TEXT, JSON, NULL by default) shipped at v0.5.0 (Slice A). v0.5.4 → v0.6.6 wired up the second renderer, `'board'` (kanban):

```json
// renderer = "board"
{ "axis": "tag", "columns": ["todo", "doing", "done"] }
```

**Locked rules (`atrium-core::render`, v0.5.4):**

- **Leftmost-match-wins.** A task with multiple column-matching tags shows up only in the leftmost matching column. Kanban is a state view — a task is in *one* state at a time.
- **"Other" trailing column.** Tasks that don't match any configured column always appear in a final `"Other"` bucket. Keeps the kanban honest about coverage; users tighten the perspective filter if they want a tighter view.
- **Case-insensitive tag matching** mirrors the rest of the search engine.
- **Drag-drop tag rewrite (`move_to_column`).** Dragging a task to a different column removes the leftmost configured-column tag from the task's tag set and adds the destination column's tag. Non-column tags pass through unchanged.

The schema field is plain TEXT — future renderers (`'agenda'`, `'matrix'`, etc.) can land without a column-type migration. The Phase 10 Mode-as-View commitment (§3.1) holds: switching renderers never touches stored task data; it only changes how the existing rows are shown.

`atrium-cli kanban NAME` and `atrium-cli perspective <create|edit|delete>` provide the matching shell surface. `BoardConfig::{to_json,from_json}` round-trips the config without forcing the GUI binary to take a direct serde_json dependency.

### 4.7 Migrations

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
- **Perspectives** — saved filter expressions stored as `perspective` rows, surfaced in the sidebar above Areas. *Save Search as Perspective…* in the primary menu captures the current search bar query (Phase 14, v0.1.17). v0.6.7 reorganisation moved the Perspectives section out from under a "Builder" header to its current spot between the top-tier group and Areas. v0.6.2 added a *Configure renderer…* dialog on the Perspective row's right-click menu — switches a perspective between the default `'list'` renderer and the `'board'` (kanban) renderer (§4.6).
- **Kanban board renderer (Slice D1, v0.5.4 → v0.6.6).** When a saved Perspective has `renderer = 'board'`, it shows as a horizontal column layout instead of a flat list — one column per configured tag, plus a trailing "Other" bucket for non-matching tasks. Drag-drop between columns rewrites the task's tag set (`atrium_core::move_to_column`). Per-column scroll for tall lists; horizontal scroll across the board when wider than viewport. Click any row → opens in Inspector. Interactive completion checkbox.
- **Inspector pane** — right-side `AdwOverlaySplitView` companion, autosaves every field on focus-out / Enter (Phase 10).
- **Defer dates + sequential projects** — `defer_until` excludes from Today/Anytime; sequential rendering dims rows past the first incomplete one (Phase 11).
- **Repeat rules** — full RFC 5545 RRULE with three Org-style completion modes (Cumulative default, Next-from-completion, Basic). Editor in the Inspector pane; worker regenerates the next instance on completion. Schema-side, `repeat_mode` was added via `0003_repeat_mode.sql` — the first migration to alter an existing table, allowed because v0.2.0 ends the v0.1 freeze (Phase 15).

**Mode-agnostic additions (Slice D2 + v0.6.7 reorganisation):**

- **Agenda (v0.6.4).** Org-mode-style "everything you should think about right now" canonical page. Five chronological sections — Overdue / Today / Tomorrow / This Week / Next Week — that classify open tasks by their most-imminent date. Tasks without a time anchor or scheduled past Next Week don't appear. Surfaces in *both* Simple and Builder modes (it's a pure read view with no Builder-only concepts). Sidebar entry sits in the top tier alongside Inbox / Today / etc. with a warning-red accent on its alarm-clock icon.

The widget tree is the same; Inspector and Forecast / Agenda / Review are added as sibling content pages, the sidebar gains a Perspectives section + the kanban board page where applicable, and the task editor's collapsed/expanded fieldset grows. **No DB work happens on mode switch.** Verified by `tests/mode_flip_snapshot.rs`.

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

v0.6.19 retired the Things 3 import phase — `.things` JSON requires a macOS export step the typical Linux user doesn't have access to ("how many people using GNOME are gonna be Things 3 users?"). OmniFocus's `.ofocus` bundle has the same access problem; it survives in the long-tail Phase 19 batch rather than its own phase. Org-mode and Todoist promote to first-class slots — Org-mode because it's the plain-text covenant Atrium was built around, Todoist because it's the cross-platform productivity app a Linux user is most likely actually leaving behind.

| Source | Format | Phase | Notes |
|---|---|---|---|
| **Org-mode** | `.org` plain text | 16 | First-class. Two-way mirror at Phase 17. TODO/DONE keywords, SCHEDULED/DEADLINE/CLOSED, headline tags, properties drawer. Stock `org-agenda` reads Atrium's vault directly. |
| **Todoist** | CSV via Todoist's official export tool | 18 | Project hierarchy; sub-tasks; sections → headings; labels → tags; comments → notes |
| **VTODO** (RFC 5545) | `.ics` | 19 | Covers Endeavour, Errands, Apple Reminders, Nextcloud Tasks, Planify (CalDAV-side) |
| **Taskwarrior** | `task export` JSON | 19 | Well-documented; UDA fields → tags or notes per user choice |
| **todo.txt** | plain text | 19 | `(A)` priority, `+project`, `@context`, `due:` |
| **TaskPaper** | plain text | 19 | Headlines + `@tags`, `@done` |
| **OmniFocus** | `.ofocus` bundle XML | 19 | macOS-export only; small Linux audience but kept open for the GTD-lineage migrant |

### 7.2 Export targets

| Target | Format | Phase | Notes |
|---|---|---|---|
| **Atrium native backup** | JSON, includes UUIDs and Builder fields | 16 | Universal lossless dump; ships with the Org vault writer |
| **Org-mode** | `.org`, two-way-ready | 16 / 17 | First-class plain-text covenant. Read-only DB→vault at 16; full two-way at 17. |
| **VTODO** | `.ics` | 19 | One-way file dump for CalDAV apps |
| **Markdown** | per-list `.md` | nice-to-have, no phase | Human-readable archive |

Atrium does **not** act as a CalDAV client in v1.0. VTODO export is a one-way file dump intended for archival or hand-off to apps like Endeavour, Errands, or Planify.

### 7.3 Org-mode mapping

When an Org vault is configured (see §3.5), Atrium projects the data model into a directory of `.org` files. The mapping below is the contract for one-shot import, ongoing read-only sync + writer (Phase 16, was 17), and the full two-way sync (Phase 17, was 17.5). v0.6.19 elevated this from a deferred two-stage plan to Atrium's primary interop direction; the agenda-parity acceptance test in Phase 17 pins it.

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
| **Things 3** | macOS-only native | proprietary | not pursued — `.things` JSON requires macOS to extract; the GNOME-using-Things-3-user audience is too narrow (retired at v0.6.19) |
| **OmniFocus** | macOS-only native | `.ofocus` bundle | direct (Phase 19, long-tail) |
| **Todoist** | proprietary cloud | CSV/JSON export | direct (Phase 18 — first-class) |
| **Vikunja** | self-hosted web | API | not yet — out of scope for v1.0 |
| **Super Productivity** | Electron | JSON export | not yet — assess in v1.1 |
| **Logseq / AppFlowy** | Electron block editors | JSON / Markdown | not yet — block-editor semantics differ enough to defer |

The strategic choice: support **VTODO/CalDAV interop** (Phase 19) and **Org-mode** (Phase 16/17 — primary covenant) as two complementary interop directions. VTODO covers the GNOME/CalDAV ecosystem broadly; Org covers the Emacs/plain-text crowd and is Atrium's must-ship two-way mirror. Together they reach almost every Linux task user without per-app importer sprawl. Todoist (Phase 18) is the first-class proprietary-app on-ramp because its install base on Linux is real and its CSV export is friction-free; Things 3 is intentionally absent (retired at v0.6.19 — `.things` JSON is macOS-export-only and the GNOME audience is vanishingly small).

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
