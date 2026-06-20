# Atrium — Application Specification

**Version:** 0.38.0 (Phases 0–18.5 complete plus Phase 19.5 foundations and Phase 19 slices 1 + 2 + 3; schema version 17. Phase 20 (the 1.0 endgame) underway: v0.35.0 shipped the accessibility round-2 pass, v0.36.0 the `scripts/perf.sh` regression suite, v0.37.0 the `mdbook` documentation site (`book/`); v0.37.1 through v0.37.3 were documentation and test-suite maintenance (README + metainfo + book prose refresh, CLI parser-test consolidation, version/test-count header reconciliation). Phase 18.5's Org-mode power features shipped across v0.14.0 → v0.19.0: DEADLINE warning windows, statistics cookies, body inline checkboxes, custom TODO sequences, CLOCK time tracking, Quick Entry templates, inter-task `[[id:UUID]]` links, and time-of-day on SCHEDULED. v0.20.0 opened Phase 19.5 with an `AdwPreferencesDialog` and system-notification reminders (`reminder_at`). v0.21.0 was a behaviour-neutral maintenance pass: helper-method extraction, partial `read.rs` / `cli/main.rs` splits, and test-coverage gap fill. The seven-crate workspace, single-writer SQLite worker, two-way Org vault, and Calibre-style search grammar described below are all current. Phase 19.5 productivity essentials are underway: v0.20.0 shipped system-notification reminders + the preferences dialog, v0.23.0 added subtasks (Builder Inspector "Subtasks" group, indented list nesting, Shift-drag reparent, CLI `--parent`), and v0.24.0 closed the Org property-drawer round-trip gap (custom `:KEY: value` entries survive verbatim via the new `task.extra_properties` JSON column). v0.25.0 opens Phase 19 with VTODO (RFC 5545) import + export, the CalDAV-side `.ics` bridge to Endeavour, Errands, Nextcloud Tasks, and Planify; the parser + emitter + mapper are hand-rolled stdlib, matching the Org + Todoist precedents. v0.26.0 adds the Taskwarrior `task export` JSON importer with a configurable UDA policy (`--uda-as tag|note|drop`); RFC 4122 UUIDs round-trip directly into `task.uuid`. v0.27.0 closes the Phase 19 plain-text importer arc with todo.txt support: one task per line, `(A/B/C)` priorities map to `priority-N` tags, `@context` becomes a tag, `+project` is dropped (lossy) since `--into` wins, and `due:` / `t:` key-value extensions thread to typed columns. v0.28.0 adds per-area review schedules: a new nullable `area.default_review_interval_days` column (migration 0015, schema version 15) that the Review query falls back to when a project leaves its own `review_interval_days` unset. v0.29.0 adds task dependencies (`blocked_by`): a `task_dependency` join table (migration 0016, schema version 16) plus the `is:blocked` / `is:available` predicates, a Builder Inspector "Blocked by" picker, a row pill, and `atrium-cli depend`. v0.30.0 opens the Tier 3 polish run: drag external files / URLs onto the window to capture (a window-level `gtk::DropTarget` opens Quick Entry pre-filled). v0.31.0 adds first-run onboarding (a self-clearing `AdwStatusPage` for a pristine library). v0.32.0 adds a backup / restore UI (`VACUUM INTO` snapshots under `$XDG_DATA_HOME/atrium/backups/`, keep-10, optional weekly, restore-on-next-launch via a marker file) with core helpers exposed through `atrium-cli backup`. v0.33.0 adds task templates: migration 0017's `task_template` + `task_template_item` (schema version 17), `atrium-cli task-template`, and a GUI "New from Template…" picker. v0.34.0 closes the arc: the non-Org importers move into a new `atrium-import` library crate (so the GUI can reach them) and a unified import dialog ("Import…" menu) drives all five sources through the worker with a dry-run preview. v0.38.0 adds a second kanban grouping axis: status-axis boards (§4.6) group by Org TODO-sequence keyword and dragging a card changes real task state (keyword + completion) instead of rewriting synthetic tags, plus an `atrium-cli edit --keyword` flag so the status board is fully shell-driveable. See `patchnotes.md` for the full arc.)
**Target:** GNOME 50+, GTK4 ≥ 4.16, libadwaita ≥ 1.7
**Language:** Rust (2024 Edition)
**Build System:** Cargo / Meson wrapper for Flatpak packaging
**License:** MIT

---

## 1. Mission Statement

Atrium is a native GNOME task manager that synthesises four traditions into one application: **Org-mode's data discipline** (UUIDs everywhere, plain-text round-trip, three repeater semantics, contexts-as-tags, full bidirectional vault mirror), **Things 3's calm** (six canonical lists, deliberate omission, the `When`/`Deadline` distinction), **OmniFocus's depth** (defer dates, sequential projects, forecast, review queues, perspectives), and **Calibre's search vocabulary** (boolean expression grammar, regex match modifiers, `is:` predicates, sort modifiers, date keywords). The synthesis isn't a clone of any one of them — it's the first GNOME-native productivity app that lets a user keep all four conventions on tap from the same data store.

Two surfaces over one store. **Simple Mode** for *what am I doing right now* — Things calm, no defer dates, no review queue, six canonical lists, the visible features chosen for attention discipline rather than feature-completeness. **Builder Mode** for the days the system needs to do the work — Forecast, Review, Perspectives, repeating tasks, sequential projects, the always-visible Inspector pane, the full Org-mode bidirectional mirror. Same schema, same rows; mode is a UI-layer flip that never touches the database.

Design philosophy: **One Store, Many Surfaces.** Tasks created in Simple Mode are real tasks with empty Builder fields. Builder Mode picks them up without conversion. An Org vault, when configured, is a downstream projection — readable in stock `org-agenda`, Doom, vim-orgmode — that round-trips bidirectionally without losing data Atrium doesn't surface. The CLI (`atrium-cli`) is a fourth surface; the post-1.0 TUI (`atrium-tui`) will be a fifth. The app is local-first, no sync, no cloud, no telemetry.

The four source traditions fail in opposite ways. Things makes you outgrow it (no defer dates, no sequential projects, no forecast). OmniFocus makes you procrastinate by adjusting fields instead of doing tasks. Org-mode makes you live in Emacs. Calibre's search vocabulary doesn't apply outside e-book libraries. Atrium's pitch: each of these four is at its best when complementing the others. A user grows into Builder Mode when the system demands it, falls back to Simple Mode when it doesn't, opens an Org vault for plain-text discipline when they want it, and types `tag:work AND is:overdue sort:-due` when they need to find something — all without abandoning their data, their app, or their attention.

---

## 2. Core Mandates

- **Local-first.** SQLite at `$XDG_DATA_HOME/atrium/atrium.db`. No remote backend, no cloud sync, no telemetry, no accounts.
- **Native GNOME.** GTK4 + libadwaita 1.7+. No web tech in the UI surface.
- **Performance.** 10,000 tasks render at the same speed as 100. Single-writer SQLite worker; UI thread never blocks on I/O.
- **Mode-as-view.** Mode is a per-app preference. Schema and data are universal. Builder fields exist on every task; Simple Mode hides them.
- **Headless surfaces stay scriptable.** The data layer (`atrium-core`), search engine (`atrium-search`), and Org projection (`atrium-org`) are GUI-free. `atrium-cli` exposes them; the post-1.0 TUI (`atrium-tui`) and Phase 20 capture daemon (`atriumd`) reuse the same crates without dragging GTK along.
- **Quick Entry sacred.** Capture is one shortcut, one keystroke. Quick Entry is identical in both modes.
- **No data loss on mode switch.** Round-trip Simple → Builder → Simple preserves everything Builder set.
- **Plain-text interop is bidirectional.** Org-mode is a first-class *peer* — import, export, and live two-way vault sync. Atrium does not silo your data, and edits made in Emacs against the vault flow back into the SQLite store.
- **Search expressivity matches Calibre.** The full boolean grammar is available everywhere search runs — saved Perspectives, the search bar, the CLI, the SQL fast-path. Power users get power; casual users see a search box.

---

## 3. Architecture

### 3.1 Mode-as-View

The Simple/Builder decision is a UI-layer toggle that adjusts:

- Which fields the task editor exposes
- Which navigation views are visible (Review and Calendar are Builder-only; the Agenda view's Strip layout is Builder-only, v0.39.0)
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

The workspace ships seven crates (six as of v0.13.0; `atrium-import` added v0.34.0):

- **`atrium-core`** — headless data layer (domain types, SQLite worker, paths, repeat-rule wrapper). GUI-free; the foundation every other crate builds on.
- **`atrium-search`** — Calibre-style search expression language (lex / parse / ast / eval). Extracted from atrium-core in v0.4.2 so the engine can be exercised, fuzzed, and reused independently.
- **`atrium-org`**: Org-mode projection (parser, emitter, importer, vault writer + `inotify` watcher) plus the RRULE / Org-cookie helpers and the `.atrium/config.toml` sidecar. Extracted from `atrium-core::sync` at v0.9.0 so the data layer stays Org-agnostic behind the `VaultDirtyNotifier` trait.
- **`atrium-inline`**: inline-syntax parser (`#tag` / `@date` / `@<weekday>` / `!N` priority) shared by Quick Entry, the bottom-of-list entry, inline rename, and the CLI `capture` subcommand. Extracted at v0.13.0; `atrium-core` stays inline-syntax-agnostic.
- **`atrium-import`**: non-Org import/export formats — Todoist CSV, Taskwarrior `task export` JSON, todo.txt, and VTODO `.ics`. Hand-rolled stdlib parsers + mappers that drive the `atrium-core` worker. Extracted from `atrium-cli` at v0.34.0 so the GTK binary's import dialog and the CLI share one implementation (Org import/export stays in `atrium-org`).
- **`atrium-cli`** — headless binary that exposes the search engine and full task CRUD (search / list / info / add / capture / edit / complete / delete) from the shell. TSV by default for shell pipelines, `--json` for jq, `--human` for terminal viewing. Read commands open the database read-only as a process-level safety guarantee; write commands spin up the worker on a current-thread tokio runtime, send commands via WorkerHandle, and shut down cleanly.
- **`atrium`** — the GTK4 binary. Depends on all six above.

The architectural commitment: every non-GUI surface stays CLI-testable. The 2.0-era TUI (`atrium-tui`) is the same shape — another headless consumer of atrium-core + atrium-search. A **post-1.0** release introduces an optional capture daemon (`atriumd`) running under user systemd that handles the global Quick Entry shortcut even when the main app is closed and IPCs the captured task in (deferred out of the 1.0 endgame — the Wayland global-shortcut portal + systemd + IPC subsystem is its own effort). Until that ships, Quick Entry works only when Atrium is running.

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

1. **`task.repeat_rule` is canonical RFC 5545 RRULE.** Org's native repeater syntax (`+1w` / `++1w` / `.+1w`) only encodes interval — it can't express multi-weekday patterns (`BYDAY=MO,WE`) or month-day-of-month patterns (`BYMONTHDAY=1`) that Atrium and Todoist support. The vault writer therefore emits **both**: a best-fit Org cookie on the SCHEDULED line so stock `org-agenda` surfaces a sensible repeat, and the full canonical RRULE in the task's `:PROPERTIES:` drawer (`:RRULE: FREQ=WEEKLY;BYDAY=MO,WE`). On read-back, `:RRULE:` wins; if the SCHEDULED cookie diverged from `:RRULE:` (the user retuned the cookie in Emacs), the divergence is logged + toasted, the file is rewritten so the cookie matches the canonical RRULE, and DB stays canonical. Multi-weekday repeats display incorrectly in stock `org-agenda` but round-trip losslessly through Atrium.
2. **Atrium-only metadata** (tag colors, saved Perspectives, mode preference) lives in the sidecar. Other Org tools ignore it.
3. **Unknown Org constructs** (custom keywords, drawers Atrium doesn't model, body content Atrium doesn't render) are preserved verbatim — never destroyed on round-trip.

DB → vault writer + one-shot import from existing Org libraries shipped at Phase 16 / v0.8.0. Full two-way sync (vault → DB via `inotify`) ships in Phase 17. Phase 18.5 mines Org-mode's interaction patterns (CLOCK time tracking, LOGBOOK drawer, custom `:PROPERTIES:`, habit grid, statistics cookies, deadline warning windows, active/inactive timestamps) into Builder Mode's Inspector pane.

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
| `last_reviewed_at` | TEXT NULL | Task-level Mark Reviewed timestamp; Phase 13 (v0.7.4) — column added via `0006_task_last_reviewed_at.sql` |
| `orig_keyword` | TEXT NULL | Phase 16 round-trip anchor for non-canonical Org keywords (`WAITING`, `BLOCKED`, etc.); v0.7.12 — column added via `0007_task_orig_keyword.sql` |
| `deadline_warn_days` | INTEGER NULL | Per-task override on `TODAY_DEADLINE_WINDOW_DAYS`; round-trips to / from the Org `-Nd` warning suffix on the DEADLINE cookie. Phase 18.5 Tier-1 (v0.14.0) — column added via `0008_task_deadline_warn_days.sql` |
| `scheduled_time` | TEXT NULL | `HH:MM` companion to `scheduled_for`; only meaningful when scheduled is a Date (Someday + None ignore the column). Round-trips to / from the time portion of the Org SCHEDULED active timestamp (`<2026-05-15 Wed 14:00>`). Phase 18.5 Tier-2 (v0.19.0) — column added via `0011_task_scheduled_time.sql`. |
| `reminder_at` | TEXT NULL | RFC 3339 UTC timestamp; when present and `<= now()` and the task is open, the GUI's reminder service fires a `gio::Notification`. Companion partial index `idx_task_reminder_at_open` covers open future reminders only. Phase 19.5 (v0.20.0) — column added via `0012_task_reminder_at.sql`. |
| `extra_properties` | TEXT NULL | JSON object of unmodeled `:KEY: value` lines from Org `:PROPERTIES:` drawers. Modeled keys (`ID`, `CREATED`, `MODIFIED`, `DEFER_UNTIL`, `EFFORT`, `RRULE`, `ORIG_KEYWORD`) are never stashed here — they map to typed columns. NULL == no extras (the read boundary normalises to an empty `BTreeMap`); empty maps written back through `update_task` normalise to NULL. Post-v0.22.0 Tier 1 (v0.24.0) — column added via `0014_task_extra_properties.sql`. Closes the §7.3.3 rule 1 gap for property drawers. |

**`quick_entry_template`** (Phase 18.5 Tier-1, v0.18.0) — pre-filled capture recipes surfaced in the Quick Entry modal as a picker bar. Closes the gap between Atrium's single Quick Entry shape and Org-capture-template multiplicity.

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | rowid |
| `name` | TEXT NOT NULL UNIQUE | user-facing label, shown in the picker |
| `shortcut_key` | TEXT NULL UNIQUE | single ASCII alphanumeric character; typing `:c ` in the modal activates the template (Emacs `org-capture` convention) |
| `target_project_id` | INTEGER NULL FK → project ON DELETE SET NULL | where new captures route; NULL = Inbox |
| `prefix` | TEXT NOT NULL DEFAULT '' | text prepended to the entry's title before parsing |
| `default_tags` | TEXT NOT NULL DEFAULT '[]' | JSON array of tag names attached to every capture |
| `position` | REAL NOT NULL | display order in the picker |
| `created_at`, `modified_at` | | trigger-maintained, same `WHEN OLD = NEW` pattern as elsewhere |

**`task_clock_entry`** (Phase 18.5 Tier-1, v0.17.0) — actual-time tracking, distinct from `task.estimated_minutes` (intent). Round-trips to / from Org's `:LOGBOOK:` drawer.

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | rowid |
| `task_id` | INTEGER NOT NULL FK → task ON DELETE CASCADE | entries die with their task |
| `started_at` | TEXT NOT NULL | ISO datetime |
| `ended_at` | TEXT NULL | NULL = clock still running. The single-active-clock invariant — at most one row across the entire table has NULL `ended_at` at any time — is enforced by the worker, not by a partial unique index (the constraint can't be expressed cleanly in SQL without a check trigger we'd rather not maintain) |
| `note` | TEXT NOT NULL DEFAULT '' | per-session free-form text; matches Org's CLOCK trailing-text convention |
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
| `review_interval_days` | INTEGER NULL | Builder-only; when NULL, the project inherits its area's `default_review_interval_days` (v0.28.0) |
| `last_reviewed_at` | TEXT NULL | Builder-only |
| `archived_at` | TEXT NULL | Logbook semantics for completed projects |
| `created_at`, `modified_at`, `position` | | |

**`task_template`** + **`task_template_item`** (v0.33.0) — reusable project templates, distinct from the single-line `quick_entry_template`. `task_template` carries `name` (UNIQUE), `project_title_seed`, `note`, and a `tags_json` array applied to every instantiated task. `task_template_item` holds the tasks the template stamps out, with an index-based `parent_index` (into the template's `position`-ordered item list, NULL = top-level) the worker resolves to a real `parent_id` at instantiate time, plus per-item `estimated_minutes` and `default_tags_json`. FK CASCADE from item to template. The worker's `instantiate_template` creates a fresh project and walks the items. CLI: `task-template list/create/instantiate/delete`.

**`task_dependency`** (v0.29.0) — `blocked_by` task dependencies. A row `(task_id, blocked_by_id)` means `task_id` is blocked by `blocked_by_id` (the latter is a prerequisite of the former); `task_id` is unavailable while the prerequisite is open. FK CASCADE on both ends; `UNIQUE(task_id, blocked_by_id)` makes a re-added edge a no-op. The worker enforces no-self-dependency and no-cycles (not expressible cleanly in SQL). Powers `is:blocked` / `is:available` (via an EXISTS subquery in the SQL fast-path) and the row's "Blocked" pill. CLI: `depend ID --on ID [--remove]`.

**`area`** — top-level grouping (`id`, `uuid`, `title`, `color`, `default_review_interval_days`, `position`, timestamps). `default_review_interval_days` (INTEGER NULL, v0.28.0) is the default Review cadence for projects in the area that leave their own `review_interval_days` NULL; the Review query resolves `COALESCE(project.review_interval_days, area.default_review_interval_days)`, so a project's own value always wins and both-NULL keeps the project out of the queue.
**`tag`** — (`id`, `uuid`, `name UNIQUE`, `color`, timestamps). The `color` column is wired end-to-end as of v0.3.0 — six-swatch picker in the editor, coloured dot in the sidebar tag row, coloured Pango pill on every task row.
**`task_tag`** — many-to-many join (`task_id`, `tag_id`).
**`heading`** — project subdivisions (`id`, `uuid`, `project_id`, `title`, `position`); Builder UI exposes editing in v0.1, Simple displays them inline as section breaks within a project.
**`perspective`** — (`id`, `uuid`, `name`, `icon`, `filter_expr`, `sort_order`, `grouping`, `position`, timestamps). Saved filter expressions surfaced as Builder-only sidebar entries. Phase 14 (v0.1.17) — added via `0002_perspectives.sql`. The `sort_order` and `grouping` columns ship now and stay unused by the v0.3 UI; they exist to absorb future per-perspective sort / grouping without another migration.

### 4.2 Derived views

Things-style lists are SELECTs, not stored rows:

- **Inbox:** `task WHERE project_id IS NULL AND completed_at IS NULL`
- **Today:** `task WHERE completed_at IS NULL AND (scheduled_for ≤ today OR deadline ≤ today + COALESCE(deadline_warn_days, N)) AND (defer_until IS NULL OR defer_until ≤ today)`, where `N = TODAY_DEADLINE_WINDOW_DAYS` (default `7`). The deadline window is the Things-3 "deadlines approaching" heads-up — a future-deadlined task surfaces in Today before it's actually due, so the user isn't blindsided. The window started as a single global constant in v0.1; v0.14.0 (Phase 18.5 Tier-1) added the per-task `deadline_warn_days` override so a sensitive task can surface earlier than the global default. The Org round-trip is the `-Nd` warning suffix on the DEADLINE cookie (`DEADLINE: <2026-04-15 Wed -7d>`); both `-` and `--` prefixes parse to the same column value, and the writer canonicalises onto `-` since Atrium has no global-default-override concept. Turning the global default itself into a GSettings key remains a Phase 8d / 19.5 preferences task.
- **Anytime:** `task WHERE completed_at IS NULL AND scheduled_for IS NULL AND (defer_until IS NULL OR defer_until ≤ today)`
- **Someday:** `task WHERE completed_at IS NULL AND scheduled_for = '__someday__'` (sentinel)
- **Upcoming:** `task WHERE completed_at IS NULL AND scheduled_for > today`
- **Logbook:** `task WHERE completed_at IS NOT NULL`
- **Forecast (Builder):** Today + Upcoming windowed to 30 days, grouped by date axis

### 4.3 Search Expression Language

Phase 15.5 (v0.4.0) replaced the v0.1 flat filter parser with a Calibre-shaped expression grammar in what is now the `atrium-search` crate (extracted from `atrium-core` at v0.4.2). The language is the contract for the search bar, saved Perspectives (which store filter expressions verbatim), and any future scripting / import surface that wants to express a task query.

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
| `is:available` | open AND not blocked by any open prerequisite (v0.29.0; dependency-only — defer is `is:deferred`, sequential state is `is:queued`) |
| `is:blocked` | open AND blocked by at least one open prerequisite (v0.29.0; a completed task is never blocked) |
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

`perspective.renderer` (TEXT, default `'list'`) and `perspective.renderer_config` (TEXT, JSON, NULL by default) shipped at v0.5.0 (Slice A). v0.5.4 → v0.6.6 wired up the second renderer, `'board'` (kanban). A board groups by one of two axes:

```json
// renderer = "board", axis = "tag" (v0.5.4)
{ "axis": "tag", "columns": ["todo", "doing", "done"] }

// renderer = "board", axis = "status" (v0.38.0)
{ "axis": "status",
  "columns": ["TODO", "NEXT", "WAITING", "DONE", "CANCELLED"],
  "done_columns": ["DONE", "CANCELLED"] }
```

**Locked rules (`atrium-core::render`):**

- **Leftmost-match-wins.** A task with multiple column-matching tags shows up only in the leftmost matching column. Kanban is a state view — a task is in *one* state at a time. (For `axis = "status"` a task has exactly one status, so this degenerates to a direct match.)
- **"Other" trailing column.** Tasks that don't match any configured column always appear in a final `"Other"` bucket. Keeps the kanban honest about coverage; users tighten the perspective filter if they want a tighter view.
- **Case-insensitive matching** (tag names or status keywords) mirrors the rest of the search engine.
- **Tag axis — drag-drop tag rewrite (`move_to_column`).** Dragging a task to a different column removes the leftmost configured-column tag from the task's tag set and adds the destination column's tag. Non-column tags pass through unchanged.
- **Status axis — drag-drop real state change (`status_move`).** Columns are Org TODO-sequence keywords; a task's column is its `orig_keyword` (falling back to canonical `TODO`/`DONE`). Dragging to an open column sets that keyword; dragging to a done-column (`done_columns`, plus the canonical `DONE` which is always done) sets the keyword *and* completes the task through the normal completion path (so a repeating task rolls forward exactly as it would on a checkbox tick). Canonical `TODO`/`DONE` store as a NULL keyword; the "Other" bucket clears the keyword. `done_columns` is omitted from the JSON for tag boards, so pre-v0.38.0 configs are byte-identical and parse unchanged.

The schema field is plain TEXT — future renderers (`'agenda'`, `'matrix'`, etc.) can land without a column-type migration. The Phase 10 Mode-as-View commitment (§3.1) holds: switching renderers never touches stored task data; it only changes how the existing rows are shown. Status-axis drags do change task state (keyword + completion), but that's the same write any completion or keyword edit makes — not a renderer-driven migration.

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

- **Forecast** — calendar-axis layout of next 30 days, drag-to-reschedule (Phase 12). As of v0.39.0 this is no longer a standalone sidebar entry: it is the **Strip** layout of the merged Agenda view (see Agenda below), reachable via a Builder-only Bands/Strip toggle.
- **Calendar Month View** — paper-calendar grid (7×N) for users who think in calendar pages. Sibling lens to Forecast (linear strip) and Agenda (chronological bands) — same data, different mental model. Day cells show count badge + up to 3 inline task titles + "+N more" overflow popover; today highlighted; out-of-month leading / trailing days muted. Prev / Today / Next / month-picker nav; `Ctrl+Shift+M` opens; Page Up / Page Down step months. Drag-to-reschedule between days; single-click peeks the day's tasks in a popover; double-click drills into a `scheduled:YYYY-MM-DD` search. Below 600 px the grid collapses to a vertical week strip (Phase 12.5, v0.11.0).
- **Review** — projects with stale `last_reviewed_at` surface here, oldest first; per-card *Mark Reviewed* button stamps the timestamp (Phase 13).
- **Perspectives** — saved filter expressions stored as `perspective` rows, surfaced in the sidebar above Areas. *Save Search as Perspective…* in the primary menu captures the current search bar query (Phase 14, v0.1.17). v0.6.7 reorganisation moved the Perspectives section out from under a "Builder" header to its current spot between the top-tier group and Areas. v0.6.2 added a *Configure renderer…* dialog on the Perspective row's right-click menu — switches a perspective between the default `'list'` renderer and the `'board'` (kanban) renderer (§4.6).
- **Kanban board renderer (Slice D1, v0.5.4 → v0.6.6).** When a saved Perspective has `renderer = 'board'`, it shows as a horizontal column layout instead of a flat list — one column per configured value, plus a trailing "Other" bucket for non-matching tasks. Two grouping axes (§4.6): the **tag axis** (default) groups by tag, and dragging between columns rewrites the task's tag set (`atrium_core::move_to_column`); the **status axis** (v0.38.0) groups by Org TODO-sequence keyword, and dragging changes real state via `atrium_core::status_move` (set `orig_keyword`, complete the task on a done-column). The status-board renderer config is configured from the GUI's "Configure renderer…" / perspective-editor dialogs (a "Board — columns by status" radio, columns entered in the Org `#+TODO:` pipe convention) and from `atrium-cli perspective … --renderer board --axis status --columns 'TODO, NEXT | DONE'`. Per-column scroll for tall lists; horizontal scroll across the board when wider than viewport. Click any row → opens in Inspector. Interactive completion checkbox.
- **Inspector pane** — right-side `AdwOverlaySplitView` companion, autosaves every field on focus-out / Enter (Phase 10).
- **Defer dates + sequential projects** — `defer_until` excludes from Today/Anytime; sequential rendering dims rows past the first incomplete one (Phase 11).
- **Repeat rules** — full RFC 5545 RRULE with three Org-style completion modes (Cumulative default, Next-from-completion, Basic). Editor in the Inspector pane; worker regenerates the next instance on completion. Schema-side, `repeat_mode` was added via `0003_repeat_mode.sql` — the first migration to alter an existing table, allowed because v0.2.0 ends the v0.1 freeze (Phase 15).
- **Subtasks** (v0.23.0, Builder-only per §5.1): a "Subtasks" group in the Inspector pane lists a task's `parent_id` children with completion checkboxes, navigates to a child on click, and creates a child (inheriting the parent's project) via an "Add subtask" entry. List views render children indented under their parent; Shift+drop reparents a task (a plain drop still reorders). The worker enforces the same-project rule and rejects parent cycles. `parent_id` has been in the schema since `0001_initial.sql`; this exposes it (no schema change, Phase 19.5). The v0.15.0 body-checkbox group is renamed "Checklist" (both modes) to free the "Subtasks" label for real nested tasks.

**Mode-agnostic additions (Slice D2 + v0.6.7 reorganisation):**

- **Agenda (v0.6.4; merged time-view as of v0.39.0).** Org-mode-style "everything you should think about right now" canonical page. Five chronological sections — Overdue / Today / Tomorrow / This Week / Next Week — that classify open tasks by their most-imminent date. Tasks without a time anchor or scheduled past Next Week don't appear. Surfaces in *both* Simple and Builder modes (it's a pure read view with no Builder-only concepts). Sidebar entry sits in the top tier alongside Inbox / Today / etc. with a warning-red accent on its alarm-clock icon. **v0.39.0** absorbed the former standalone Forecast entry: in Builder Mode the Agenda page carries a centered **Bands / Strip** layout toggle, where Bands is this chronological view and Strip is the 30-day Forecast projection (drag-to-reschedule). The toggle switches `ActiveList` between `Agenda` and `Forecast` against the same data; Strip is Builder-only, so Simple Mode shows only the Bands layout with no toggle. This consolidates the four overlapping "when" surfaces (Upcoming, Agenda, Forecast, Calendar) down the sidebar to two time-entries (Agenda, Calendar) plus the Upcoming canonical list.

The widget tree is the same; Inspector and Forecast / Agenda / Review are added as sibling content pages, the sidebar gains a Perspectives section + the kanban board page where applicable, and the task editor's collapsed/expanded fieldset grows. **No DB work happens on mode switch.** Verified by `tests/mode_flip_snapshot.rs`.

### 5.3 Mode Switch

`Settings → Mode → [Simple, Builder]`. Switching is instant — settings flag flip plus re-render of menus and editor. Persisted across launches in GSettings.

---

## 6. Quick Entry

A global GTK shortcut (default `Ctrl+Alt+Space`) opens a small modal that:

- Drops a new task into Inbox
- Accepts the same inline-syntax vocabulary as the bottom-of-list entry, the inline-rename surface, and the CLI's `capture` subcommand:
  - `#tag` — attach (creates the tag on first use; case-insensitive)
  - `@today` / `@tomorrow` / `@someday` — set `scheduled_for`
  - `@yyyy-mm-dd` — set `scheduled_for` to a specific date
  - `@<weekday>` (`@mon` / `@monday`, all forms case-insensitive) — set `scheduled_for` to the next occurrence of that weekday on or after today (Slice 2, v0.13.0)
  - `@deadline yyyy-mm-dd` — set `deadline`
  - `!1` / `!2` / `!3` — set priority (single-valued, projected onto a `priority-N` tag until Phase 19.5's numeric column lands; Slice 2, v0.13.0). `!4` and beyond stay in the title verbatim — Todoist treats 4 as "no priority" and Atrium follows.
- Closes on Enter (commit) or Esc (discard)
- Is identical in both modes
- Does not steal focus from the previously focused window

The same parser (`atrium-core::quick_entry`) drives the inline-rename surface in the GTK task list — F2 / right-click → Rename / double-click into edit. Renames take a fast path identical to pre-v0.13 behaviour when the new title contains no inline-syntax tokens; when tokens are present the title's parsed scalars set in a single `update_task` and tag side effects merge into the task's existing set (rename never removes a free-form tag, but `!N` does swap one priority tag for another since priority is single-valued).

The task row's right-click menu carries *Edit Details…* (Inspector), *Edit Tags…*, and a **Schedule** submenu — Today / Tomorrow / This Weekend / Next Week / Someday / Clear — that reschedules in one pick via the `win.reschedule` action (target `(task_id, keyword)`) instead of an editor round-trip (v0.40.0, Tier D). The keyword-to-date mapping is the pure, unit-tested `parse_quick_schedule`.

If Atrium is closed, the shortcut launches it and posts the task. A post-1.0 `atriumd` (user systemd) will add true zero-launch capture.

---

## 7. Imports & Exports

Imports are best-effort: each source has a documented mapping table; lossy fields are surfaced in a post-import report. Each importer ships with a dry-run mode that shows what would be created without touching the DB.

### 7.1 Import sources

v0.6.19 retired the Things 3 import phase — `.things` JSON requires a macOS export step the typical Linux user doesn't have access to ("how many people using GNOME are gonna be Things 3 users?"). v0.20.0 retired TaskPaper and OmniFocus (`.ofocus` bundle) for the same reason: both are macOS-only source apps, so the Linux + Org user audience the rest of the import surface targets effectively can't supply input files. Atrium's schema remains the OmniFocus superset (§4) by spec commitment regardless — that's a *schema* decision and unaffected by dropping the *importer*. Org-mode and Todoist promote to first-class slots — Org-mode because it's the plain-text covenant Atrium was built around, Todoist because it's the cross-platform productivity app a Linux user is most likely actually leaving behind.

| Source | Format | Phase | Notes |
|---|---|---|---|
| **Org-mode** | `.org` plain text | 16 | First-class. Two-way mirror at Phase 17. TODO/DONE keywords, SCHEDULED/DEADLINE/CLOSED, headline tags, properties drawer. Stock `org-agenda` reads Atrium's vault directly. |
| **Todoist** | CSV via Todoist's official export tool | 18 (shipped v0.12.0) | Per-project CSV export. `section` → heading; INDENT chain → `parent_id`; inline `@label` → tag; PRIORITY 1-3 → `priority-N` tag; `DATE` natural-language → RRULE + `scheduled_for`. v5 UUIDs from `(project_name, title)` give re-import stability. Lossy fields (time-of-day, timezone, duration, deadline) surface in the per-row import report. |
| **VTODO** (RFC 5545) | `.ics` | 19 (shipped v0.25.0) | Covers Endeavour, Errands, Nextcloud Tasks, Planify (CalDAV-side). Hand-rolled stdlib parser; UID round-trip via the v0.24.0 `extra_properties` column. See §7.5. |
| **Taskwarrior** | `task export` JSON | 19 (shipped v0.26.0) | Hand-rolled stdlib parser via `serde_json`; status pending/waiting/completed/deleted/recurring handled; UDA fields routed via `--uda-as tag|note|drop`. RFC 4122 UUIDs round-trip directly. |
| **todo.txt** | plain text | 19 (shipped v0.27.0) | Hand-rolled stdlib parser. `(A/B/C)` priority → `priority-N` tag, `+project` dropped (lossy; `--into` wins), `@context` → tag, `due:YYYY-MM-DD` → deadline, `t:YYYY-MM-DD` → defer_until, completion marker `x` → `completed_at`. v5 UUIDs derived from `(project_name, title, creation_date)`. |

### 7.2 Export targets

| Target | Format | Phase | Notes |
|---|---|---|---|
| **Atrium native backup** | JSON, includes UUIDs and Builder fields | 16 | Universal lossless dump; ships with the Org vault writer |
| **Org-mode** | `.org`, two-way-ready | 16 / 17 | First-class plain-text covenant. Read-only DB→vault at 16; full two-way at 17. |
| **VTODO** | `.ics` | 19 (shipped v0.25.0) | One-way file dump for CalDAV apps. One VCALENDAR per file, one VTODO per task, UTC for all timestamps, no VTIMEZONE. Atomic write via `atrium_core::sync::atomic::write_atomic`. |
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
| `deadline_warn_days` | `-Nd` warning suffix on the DEADLINE cookie (`-` and `--` both parse; emit canonicalises onto `-`) |
| Statistics cookie on parent | `[done/total]` or `[N%]` between title and tags; recomputed at emit from DB state, source shape (counter vs percent) preserved across round-trip. Counts immediate child TODOs + body checkboxes (mirrors `org-checkbox-hierarchical-statistics`). v0.15.0 — Phase 18.5 Tier-1. |
| Body inline checkbox | `- [ ]` / `- [X]` / `- [-]` lines in the note body. Verbatim round-trip; the Inspector renders interactive toggles that rewrite the body string in place. v0.15.0 — Phase 18.5 Tier-2. |
| Custom TODO sequence | `#+TODO: STATE1 STATE2 \| DONE1 DONE2` preamble per project file, sourced from the vault sidecar's `[[todo_sequences]]` slot. The watcher maps sequence-configured done keywords to Atrium's DONE state (preserving the source label via `task.orig_keyword`); workflow keywords stay open with the same preservation. Out-of-set keywords surface a `VaultEvent::UnknownKeyword` toast and stash via the existing Custom path. v0.16.0 — Phase 18.5 Tier-1. |
| `task_clock_entry` rows | `:LOGBOOK:` ... `:END:` drawer with `CLOCK: [start]--[end] => HH:MM` lines. Closed entries round-trip; in-progress entries are deliberately suppressed by the writer to avoid file churn while the clock runs (the next clock-out flushes). Timestamps treated as UTC (matches the existing CLOSED-cookie convention; users in non-UTC zones see UTC clock times in the file). Custom drawer lines that aren't CLOCK round-trip verbatim via `OrgTask.logbook_unknown_lines`. v0.17.0 — Phase 18.5 Tier-1. |
| Inter-task link in body | `[[id:UUID]]` (label-less) or `[[id:UUID][label]]` (with display text). Bodies round-trip verbatim via the existing `OrgTask.body` field; the Inspector renders matching spans as clickable links that focus the linked task on click (stale UUIDs no-op silently). v0.19.0 — Phase 18.5 Tier-2. |
| `scheduled_time` | Time portion of the Org SCHEDULED active timestamp (`<DATE Day HH:MM>`); when present, slotted between the day name and any repeater / warning suffix in canonical order. v0.19.0 — Phase 18.5 Tier-2. |
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

1. **Never destroy data.** Anything Atrium doesn't model — custom TODO keywords (`WAITING`, etc.), unknown drawers, body content with constructs Atrium doesn't render, custom `:PROPERTIES:` drawer keys outside the modeled set — is preserved verbatim. Custom keywords map to a sentinel state on import; the original is stashed in `:ORIG_KEYWORD:` and restored on export. Unmodeled property-drawer keys (anything outside `ID` / `CREATED` / `MODIFIED` / `DEFER_UNTIL` / `EFFORT` / `RRULE` / `ORIG_KEYWORD`) stash into `task.extra_properties` (JSON column, v0.24.0) and the writer merges them back into the emitted drawer on every flush.
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
| **OmniFocus** | macOS-only native | `.ofocus` bundle | not pursued — same Mac-only access problem as Things 3; importer dropped at v0.20.0. Atrium's schema remains the OmniFocus superset by spec commitment (§4) — that's an architectural anchor, not an importer promise |
| **TaskPaper** | macOS-only native | plain text | not pursued — Mac-only source app; portable plain text but the realistic audience is Mac → Linux migrants. Dropped at v0.20.0 for the same reason as OmniFocus |
| **Todoist** | proprietary cloud | CSV/JSON export | direct (Phase 18 — first-class) |
| **Vikunja** | self-hosted web | API | not yet — out of scope for v1.0 |
| **Super Productivity** | Electron | JSON export | not yet — assess in v1.1 |
| **Logseq / AppFlowy** | Electron block editors | JSON / Markdown | not yet — block-editor semantics differ enough to defer |

The strategic choice: support **VTODO/CalDAV interop** (Phase 19) and **Org-mode** (Phase 16/17 — primary covenant) as two complementary interop directions. VTODO covers the GNOME/CalDAV ecosystem broadly; Org covers the Emacs/plain-text crowd and is Atrium's must-ship two-way mirror. Together they reach almost every Linux task user without per-app importer sprawl. Todoist (Phase 18) is the first-class proprietary-app on-ramp because its install base on Linux is real and its CSV export is friction-free; Things 3 is intentionally absent (retired at v0.6.19 — `.things` JSON is macOS-export-only and the GNOME audience is vanishingly small).

### 7.5 VTODO mapping (RFC 5545)

Phase 19 slice 1 (v0.25.0). One-shot file import + one-way file export for the CalDAV ecosystem's lingua franca. The parser, emitter, and mapper live in `atrium-import/src/vtodo/` (extracted from `atrium-cli` at v0.34.0) and are stdlib-only — no `ical` crate, matching the Org parser + Todoist importer precedents. Per §7.2, Atrium is **not** a CalDAV client.

#### 7.5.1 Field mapping

| VTODO | Atrium |
|---|---|
| `SUMMARY` | `task.title` |
| `DESCRIPTION` | `task.note` |
| `DUE` | `task.deadline` (date portion only; time-of-day truncates) |
| `DTSTART` | `task.scheduled_for` (date) and `task.scheduled_time` (time portion, v0.19.0 column) |
| `COMPLETED` | `task.completed_at` |
| `STATUS:COMPLETED` | sets `completed_at = now()` when no COMPLETED property |
| `STATUS:NEEDS-ACTION` / `IN-PROCESS` / `CANCELLED` | stashed in `task.orig_keyword` for round-trip parity |
| `PRIORITY` 1–4 | `priority-N` tag (matches Todoist's shape; 5–9 emits no tag) |
| `CATEGORIES` | `task.tag` rows via `ensure_tag` (idempotent dedupe) |
| `RRULE` | `task.repeat_rule` (verbatim — RFC 5545 is RFC 5545) |
| `UID` (UUID-shaped) | `task.uuid` directly |
| `UID` (free-form) | v5 UUID derived from frozen namespace; original stashed in `task.extra_properties["VTODO_UID"]` |
| `LOCATION` | `task.extra_properties["VTODO_LOCATION"]` (no typed column; lossless via v0.24.0) |
| `X-*` | `task.extra_properties[X-*]` (lossless via v0.24.0) |
| `CREATED` / `LAST-MODIFIED` / `DTSTAMP` / `SEQUENCE` | dropped; Atrium's auto-stamped `created_at` / `modified_at` carry the equivalent |
| Anything else | one lossy entry per occurrence |

#### 7.5.2 UID round-trip anchor

Atrium's `task.uuid` is UUID v4 by contract; a VTODO UID is free-form text (`task@nextcloud.example.com`, `1234`, anything). The v0.24.0 `extra_properties` column unlocks the clean round-trip:

- **UUID-shaped UID:** thread directly into `task.uuid`. Identity round-trip.
- **Free-form UID:** derive `task.uuid` as `UUIDv5(VTODO_NAMESPACE, original_uid)` and stash the original in `extra_properties["VTODO_UID"]`. The exporter prefers the stashed value on emit, so the receiving app sees its UID unchanged. The frozen v5 namespace makes re-imports of the same source land on the same row.

#### 7.5.3 Scope guardrails

- No CalDAV client. No HTTP, no auth, no sync.
- No VTIMEZONE generation. Export is UTC-only — receiving CalDAV apps universally accept UTC.
- Non-UTC timestamps on import drop the timezone; the parser flags the loss as `LossyKind::DroppedTimezone`.
- No VEVENT / VJOURNAL / VFREEBUSY handling. One lossy entry per top-level non-VTODO component.
- No VALARM round-trip. Atrium's reminders are separate (`task.reminder_at`); cross-mapping VALARM ↔ reminder is deferred. Count surfaces in the lossy report.
- No multi-file vault. `.ics` convention is one calendar per file; multi-file is the JSON snapshot's job.

#### 7.5.4 Lossy report

Mirror of the Todoist `LossyKind` shape. One entry per per-VTODO occurrence; the unified `ImportSummary` carries them all. Variants:

| Kind | Trigger |
|---|---|
| `UnsupportedComponent` | Top-level VEVENT / VJOURNAL / VFREEBUSY / VTIMEZONE / X-* |
| `DroppedAlarm` | One or more VALARM blocks inside a VTODO |
| `DroppedAttendee` | ATTENDEE or ORGANIZER properties |
| `DroppedGeo` | GEO property |
| `DroppedPercentComplete` | PERCENT-COMPLETE property |
| `DroppedDuration` | DURATION property without paired DTSTART/DUE |
| `DroppedTimezone` | DTSTART / DUE / COMPLETED carried a TZID parameter |
| `UnknownProperty` | A property name outside the modeled set and not X-* |

---

## 8. Memory & Performance Targets

Atrium is lighter than Viaduct (no WebKit), but discipline still matters:

- **Idle:** < 80 MB after full library load
- **Active:** < 200 MB during heavy use (10K-task forecast view, search active)
- **Cold start:** < 250 ms to first interactive frame on a 5K-task DB
- **Quick Entry latency:** < 50 ms from shortcut to focused entry field

Each phase ends with a `heaptrack`/`massif` measurement note. Features that miss budget get gated or revised. The repeatable headless check is `scripts/perf.sh` (v0.36.0) — 50K / 100K fixtures, read-path load + peak RSS, asserting the data-layer budgets; the GUI-surface budgets are measured via the in-app Memory Watch on a real display. See `docs/perf-baseline.md`.

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
