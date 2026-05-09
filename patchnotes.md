# Atrium — Patch Notes

## v0.10.0 (2026-05-09) — Phase 17 first slice: vault → DB sync

The DB → vault direction has been live since v0.7.16 / Phase 16. v0.10.0 closes the loop: edits made in Emacs / Doom / vim-orgmode against the configured vault flow back into the SQLite store within ~250 ms.

**The watcher.** New `atrium-org/src/vault_watcher.rs` hosts a tokio task that pairs with the existing `VaultWriter`. It uses the `notify` crate (sign-off granted; v8.x; the canonical Rust file-watcher used by watchexec / cargo-watch) to subscribe to `.org` create / modify / delete events under the vault root. Events debounce 200 ms keyed on file path (last-deadline-wins, matching the writer's pattern); after debounce the watcher parses the file through the existing `parse_org_file_with_meta`, computes a diff against current DB state, and submits writes through `WorkerHandle`.

**The self-write filter.** Without coordination, every write the writer emits would echo back through inotify and trigger a redundant read/diff cycle. New `atrium-org/src/self_write.rs` exposes `RecentWrites`, an `Arc<RwLock<>>`-shared set the writer pushes to and the watcher consults. The match is **mtime-based exact tuple equality** on `(path, mtime_just_written)`, not a TTL window on path alone. The first design used path+TTL and the integration tests immediately surfaced the failure mode: an external edit happening within the TTL window after Atrium's own write got swallowed because the writer's record was still "recent" when the watcher's debounce fired. mtime-based matching is exact — Linux ext4 stores nanosecond mtimes so two distinct writes never collide; Atrium-from-Atrium echoes match exactly; real external edits produce a different mtime and fall through. The TTL stays as a memory bound (2 seconds) but doesn't gate the match.

**The diff.** `vault_watcher::diff_and_apply` resolves the project by file-level `:ID:` (creating one if the file is new), snapshots current DB tasks for that project, and walks the parsed headline tree:

- Tasks present in parsed but missing in DB → `WorkerHandle::create_task`. Headlines parsed without `:ID:` get a freshly-minted UUIDv4; the worker's auto `notify_project_dirty` after the create triggers the writer to rewrite the file with the now-stable property, and the self-write filter swallows the resulting inotify event.
- Tasks present in DB but missing in parsed → `WorkerHandle::delete_task`.
- Tasks present in both → `WorkerHandle::update_task` for any field difference (title, schedule, deadline, completed_at) plus `WorkerHandle::set_task_tags` for tag-set differences.

**`TaskUpdate.completed_at`.** Atrium previously had only `toggle_complete` (which stamps `now()`) for state transitions. The vault watcher needs to round-trip `CLOSED: [2026-04-01 Wed 09:00]` cookies verbatim — the source timestamp must land in the DB. New `TaskUpdate.completed_at: Option<Option<DateTime<Utc>>>` field + builder method; the worker SQL builder gained the matching branch. `Some(None)` clears (re-opens), `Some(Some(when))` sets. Schema unchanged; no migration.

**The wiring.** New ergonomic builder `atrium_org::spawn_org_vault_with_watcher(root, pool, worker_handle)` spawns the writer + the watcher sharing one `RecentWrites` set, returning the `VaultConfig` ready to thread into `spawn_worker_with_vault`. The legacy `spawn_org_vault` (write-only — the v0.8.0 / v0.9.0 shape) stays available for callers that want write-only behaviour or just the writer half (tests).

**Three integration tests** at `atrium-org/tests/vault_watcher_integration.rs` pin the working slice end-to-end:

- `external_add_creates_db_task` — append a new TODO headline to a vault file via `fs::write`; assert the DB has the new task and the rewritten file gained an `:ID:` property.
- `external_edit_completes_db_task` — flip TODO → DONE in the file; assert `task.completed_at` lands.
- `external_delete_removes_db_task` — splice a headline out of the file; assert the matching DB row is gone.

**What's deferred to the v0.10.x patch arc** per the Phase 17 roadmap entry:

- v0.10.1: conflict detection (mtime race → loser preserved at `<file>.atrium.bak.<timestamp>`); GUI wiring (`spawn_vault_watcher` from the GTK boot path).
- v0.10.2: malformed-file pause/resume (parse error → pause that file, toast surfaced; auto-resume when it parses again).
- v0.10.3: RRULE divergence detection on read-back (per the canonicalisation contract spec §3.5 + roadmap Phase 17).
- v0.10.4: agenda-parity acceptance test gating the v0.10.x → v0.11.0 close.

**Test count: 590** (up 8 — three integration tests + four `RecentWrites` unit tests + one watcher diff test bundled into the integration suite). Schema unchanged at version 7. New direct dependency: `notify` v8 in `atrium-org` (sign-off granted in this patch). Ship-gate runs in under 2 seconds.

VERSION + Cargo.toml + spec + roadmap + patchnotes + README + CLAUDE.md + AppStream metainfo bumped to 0.10.0.

## v0.9.0 (2026-05-09) — `atrium-org` crate extraction

The Phase 16 Org projection — parser, emitter, importer, vault writer task — moves out of `atrium-core::sync` into its own workspace crate, `atrium-org`. atrium-core stays Org-agnostic; the worker hooks into the projection through a new `VaultDirtyNotifier` trait. Workspace is now five crates (atrium-core, atrium-search, atrium-org, atrium-cli, atrium). Pre-Phase-17 housekeeping; no behaviour change, no schema change, test count unchanged at 582.

**The split.** What moved into `atrium-org`:

- `atrium-core/src/sync/org/{parse,emit,import,write}.rs` → `atrium-org/src/org/*`. Same public API; the only path change for callers is `atrium_core::sync::org::*` → `atrium_org::org::*`.
- `atrium-core/src/sync/vault_writer.rs` → `atrium-org/src/vault_writer.rs`. Now uses an `OrgVaultNotifier` wrapper that impls `atrium_core::VaultDirtyNotifier`.
- `atrium-core/tests/org_roundtrip.rs` (+ the five fixture `.org` files) → `atrium-org/tests/`. The Org-related worker_tests entries (`import_org_file_*` / `import_org_directory_*` / `spawn_with_vault_writes_org_file_on_task_create`) moved to a new integration test `atrium-org/tests/worker_org_integration.rs`.

What stayed in `atrium-core`:

- `atrium-core/src/sync/atomic.rs` (write-temp + fsync + rename helper — generic, not Org-specific).
- `atrium-core/src/sync/json.rs` (lossless DB snapshot — works on any projection).

**The trait abstraction.** New `atrium-core/src/db/vault_hook.rs` exposes:

```rust
pub trait VaultDirtyNotifier: Send + Sync {
    fn notify_project_dirty(&self, project_id: i64);
}

pub struct VaultConfig {
    pub notifier: Arc<dyn VaultDirtyNotifier>,
}
```

The atrium-core worker holds an `Option<Arc<dyn VaultDirtyNotifier>>` instead of a concrete `mpsc::Sender<VaultWriteRequest>`. atrium-org's `OrgVaultNotifier` wraps the sender and provides the impl. Ergonomic helper `atrium_org::spawn_org_vault(root, pool)` returns a ready-to-use `VaultConfig` so the GUI / CLI boot paths stay one-call.

**Schema rule cleanup.** `atrium-core::db::migrations` was `pub(crate)`; promoted to `pub` so atrium-org's integration tests can reach in for fresh-DB setup without depending on `atrium_core::db::open` for every test fixture. Production code never calls migrations directly; `db::open` remains the public entry point.

**Why now?** Phase 17 (vault → DB `inotify` sync) is the next chunk of code, and it'll grow the projection layer further. Splitting the surface before that work starts keeps atrium-core's ~5K-line data layer focused on the worker / read pool / domain model, and gives atrium-org a clean home for the inotify watcher when it arrives.

The Phase 18 Todoist importer (when it lands) will follow the same shape: another sibling crate, depending on atrium-core, with its own write side. The architectural commitment that every non-GUI surface stays CLI-testable still holds — atrium-cli depends on atrium-org directly for the `import org` / `export org` / `export json` paths.

Workspace version bumped to **0.9.0** across `Cargo.toml`, `VERSION`, spec, roadmap, README, CLAUDE.md, AppStream metainfo. Schema version unchanged at 7. No new dependencies; atrium-org borrows from the same locked workspace set.

## v0.8.0 (2026-05-09) — Phase 16 stamp + maintenance pass

Phase 16 (Org-mode import + DB → vault writer) ships, capping the eleven-patch v0.7.6 → v0.7.18 build-out. The GTK binary, `atrium-cli`, and the hand-rolled `atrium-core::sync::org` parser/emitter let a user keep a vault at the configured path, edit tasks in Atrium, and have the `.org` files reflect the change inside ~150 ms — readable in stock `org-agenda`, Doom, or any other Org-aware tool. All Phase 16 roadmap bullets are now `[x]` except the deferred `<vault>/.atrium/config.toml` sidecar (Phase 17 follow-up).

The maintenance pass that release discipline requires of every major:

- **Worker test split.** `atrium-core/src/db/worker.rs` (2622 lines, half tests) split into `worker.rs` (1469 lines, source only) and `worker_tests.rs` (1161 lines) loaded via `#[cfg(test)] #[path = "worker_tests.rs"] mod tests;`. Same coverage; tractable file size.
- **Dead-code prune in the Org writer.** `build_org_tree` carried a `HashMap<i64, usize>` populated then discarded with `let _ = by_index;` — scaffolding from the v0.7.10 iteration. Removed.
- **Comment audit.** Bulk pass across `atrium-core/src/sync` and `atrium-core/src/db` reduced per-patch `// v0.7.X — …` markers from 74 → 26. The survivors flag load-bearing context (additive migrations, spec rules, schema columns); the rest were navigation noise.

Four-doc sweep landed on `spec.md`, `roadmap.md`, `patchnotes.md`, README, CLAUDE.md, and the AppStream metainfo. Schema unchanged at version 7; no new dependencies; 582 tests, all green.

Phase 17 (vault → DB `inotify` sync) is next.

## v0.7.18 (2026-05-09) — GUI vault integration

The GTK binary now reads the `vault-path` GSettings key on boot and routes through `spawn_worker_with_vault` when the key is non-empty, closing the loop opened by v0.7.16's auto-debounced worker write hook. Until v0.7.18, no GUI caller was passing a `VaultConfig` — every DB write needed `atrium-cli` to flush.

`boot_data_layer` builds the `ReadPool` first (the `VaultConfig` needs it), reads `vault-path` via `gio::Settings::new(APP_ID)`, and either passes `Some(VaultConfig)` (auto-creating the directory if missing) or `None` (DB-only mode, current behaviour). Misconfigured paths log a `tracing::warn!` and fall through to `None` so the app never refuses to start over a vault config error.

`atrium-core` re-exports `VaultConfig` + `spawn_with_vault as spawn_worker_with_vault` from the crate root so callers don't dive into the worker module path.

A graphical *Settings → Org Vault → Choose folder* UI to manage the key is deferred to Phase 19.5's `AdwPreferencesWindow`. Until then: `gsettings set io.github.virinvictus.atrium vault-path /path/to/vault`.

Pure additive change: no schema, no dependency changes, 582 tests still green.

## v0.7.17 (2026-05-09) — Round-trip test fixture + two importer fixes

The Phase 16 roadmap requirement: "import → export → diff = empty (modulo whitespace and section ordering)." `atrium-core/tests/org_roundtrip.rs` delivers it across five fixtures at `atrium-core/tests/fixtures/org/`:

- `kitchen_sink.org` — every spec §7.3 feature mixed (TODO/DONE/CANCELLED keywords, SCHEDULED/DEADLINE/CLOSED with repeaters, headline tags, `:PROPERTIES:` drawer, body with bullets, nested subtasks, file-level metadata).
- `custom_keywords.org` — WAITING / BLOCKED / IN-PROGRESS preservation via `orig_keyword`.
- `deep_nesting.org` — 4+ levels of subtask hierarchy.
- `project_metadata.org` — file-level `#+TITLE:` + `:PROPERTIES:` block with `:SEQUENTIAL:` / `:REVIEW_INTERVAL:` / `:LAST_REVIEWED:`.
- `unicode.org` — CJK, Cyrillic, emoji, accented Latin.

Each test imports the fixture through the worker, exports back to a fresh path, parses both source and regenerated, and asserts AST equality on a paired-normalised shape. Normalisation strips fields that intentionally don't preserve (`:CREATED:` / `:MODIFIED:` — schema-auto-stamped; round-trip-added `:ID:` per §7.3.3 rule 2; tag order — sets, not lists). Strict on title, keyword (incl. custom), tags content, cookie dates, property values, body, subtask hierarchy, and file-level metadata.

The fixture surfaced two real importer gaps:

1. **`NewTask.completed_at: Option<DateTime<Utc>>`** — previously the DONE/CANCELLED path called `toggle_complete` after create, stamping `now()` instead of the source CLOSED cookie's timestamp. The importer now threads `org.closed` directly into `NewTask.completed_at`. Toggle still fires when the source had a TODO/DONE/CANCELLED keyword but no CLOSED cookie. All `NewTask` call sites updated (atrium-cli `run_add`, the worker's repeating-task respawn, the GUI undo restore — undo now preserves the original completion timestamp too).

2. **CANCELLED via `orig_keyword`** — Atrium's domain has TODO/DONE only; `completed_at` doesn't distinguish "completed normally" from "cancelled." v0.7.12's `orig_keyword` for non-canonical keywords (WAITING etc.) now also stashes CANCELLED. The writer's orig-keyword-first lookup picks it up automatically and round-trip preserves the keyword exactly.

## v0.7.16 (2026-05-09) — Auto-debounced worker write hook (DB → vault)

Every Task / Project write through the SQLite worker now triggers a background rewrite of the affected project's `.org` file in the configured vault. Atrium and Emacs can run side-by-side against the same vault and stay in sync (DB → vault direction; vault → DB is Phase 17's `inotify` watcher).

**`atrium-core::sync::vault_writer`** — new module hosting the
background writer:

- `VaultWriteRequest::ProjectDirty(i64)` is the request type.
- `VaultWriter` owns the vault root + a `ReadPool` + a
  `pending: HashMap<i64, Instant>` keyed by project_id where
  the value is the deadline after which the project should be
  flushed.
- `run()` is a `tokio::select!` loop: receive requests +
  tick on a 50ms interval. Receiving extends a project's
  deadline by 100ms (last-deadline-wins coalescing); the tick
  flushes any project past its deadline.
- `spawn_vault_writer(root, pool)` spins up the task and
  returns the request sender.

**Latency:** ~150 ms (debounce 100ms + tick 50ms) from a DB
write landing to the corresponding `.org` file appearing.
Below human-perceptible threshold.

**Worker integration.** New `spawn_with_vault(conn, vault:
Option<VaultConfig>)` entry point alongside the existing
`spawn`. `VaultConfig { root: PathBuf, read_pool: ReadPool }`.
The worker stashes a `vault_tx: Option<mpsc::Sender<VaultWriteRequest>>`
internally; a `notify_project_dirty(project_id)` helper
non-blockingly `try_send`s through it (full channel → drop,
not block — under absurd load the worst case is one stale
vault file). `spawn(conn)` becomes a thin wrapper that
delegates with `vault: None`, so atrium-cli and tests stay
unchanged.

**Dispatch sites.** Every Worker command that mutates a
project's task set or project metadata now calls
`notify_project_dirty`:

- `CreateTask` / `UpdateTask` / `ToggleComplete` —
  `task.project_id`
- `DeleteTask` — captures the project_id BEFORE deleting
  (since the row goes away)
- `CreateProject` / `UpdateProject` / `ArchiveProject` /
  `MarkReviewed` — the project's id
- `MarkTaskReviewed` — `task.project_id`
- `SetTaskTags` — `task.project_id`

**Architecture choices documented in the module doc:** why a
separate task (single-writer SQLite discipline; vault writes
shouldn't block command processing on large projects); why
debounce inside the writer (keeps worker dispatch sites
trivial); why mpsc instead of broadcast (single consumer +
overflow tolerable).

**Tests:**

- `vault_writer_emits_project_file_on_dirty_request` — the
  isolated writer task: send a request, wait, verify file
  appears.
- `vault_writer_debounces_burst_into_one_write` — 5 rapid
  requests over 50ms collapse into one final write.
- `spawn_with_vault_writes_org_file_on_task_create` — the
  end-to-end story: spawn the worker with a vault, create a
  task, the file lands automatically.

**What's NOT in v0.7.16** (deferred to v0.8.0's maintenance
pass): GUI integration with the GSettings `vault-path` key
(the worker accepts a vault config but no caller passes one
yet — atrium-cli stays unchanged, the GTK binary still uses
the no-vault `spawn`); rollback to `.atrium.bak.<timestamp>`
on integrity failure (v0.7.15's Err return is the
detection layer; the recovery layer needs the v0.7.16 hook
to make decisions on, which it now has).

## v0.7.15 (2026-05-09) — Post-write Org integrity check

With the importer + writer +
multi-file walk in place, every vault write now goes through a
post-write parse-back assertion: the file we just wrote must
re-read cleanly through Atrium's own parser. If it doesn't, the
emit returns an `io::Error::Other` describing the divergence,
and a `tracing::warn` lands in the log so the failure is visible
even when the caller swallows the error.

**`emit_org_file_with_meta` now calls `verify_emitted_file`
after the atomic rename.** The verification path:

1. Re-read the just-written file from disk.
2. Parse it via `parse_org_file_with_meta`.
3. On success → `Ok(())`. On any read or parse error →
   `Err(io::Error::Other)` with the underlying error
   wrapped + a `tracing::warn` event.

The hand-rolled parser is intentionally permissive — anything it
doesn't recognise lands in body or unknown_lines — so "rejects"
in practice means an `io::Error` from the read itself (e.g. the
file mysteriously vanished mid-write, or the user hit a
permission flip on the parent directory). It's the minimum bar
the spec calls for: "newly-written file parses cleanly with
Atrium's own reader."

**Rollback to `.atrium.bak.<timestamp>`** is the second half of
the spec rule (§7.3.3 rule 5: "Conflicts are surfaced, not
silenced"). It defers to v0.7.16+ alongside the auto-debounced
worker write hook, since both paths need to know how to recover
gracefully — preserving the previous file content before the
atomic rename + writing it back to a `.bak` on integrity
failure is a meaningful infrastructure piece on its own.
v0.7.15's Err return lets callers (the v0.7.16 worker hook) make
that decision.

## v0.7.14 (2026-05-09) — Multi-file vault walk + ensure_area

With v0.7.6 → v0.7.13 in
place, Atrium can round-trip a single `.org` file through the
DB. v0.7.14 lifts the importer to the vault-as-directory level
so users can pull an entire `~/Tasks/` into Atrium with one
command.

**`WorkerHandle::ensure_area` (mirror of ensure_tag).**
`Command::EnsureArea { name, responder }` + an idempotent
inner helper. Probes the area table for a row whose title
matches `name` case-insensitively (the `area.title` column
isn't NOCASE-collated, so the match runs at the query level
via `LOWER(title) = LOWER(?1)`); returns the existing row when
found, creates a new one otherwise. Used by the multi-file
importer to map vault subdirectories onto Atrium areas
without duplicating existing rows on re-import. Test covers
case-insensitive dedup + creation of distinct names.

**`import_org_file_with_area`.** v0.7.9's `import_org_file`
becomes a thin wrapper that delegates with `area_id = None`;
the new `_with_area` form accepts an `Option<i64>` so the
directory walker can file projects under their resolved area.

**`import_org_directory(handle, vault_root, dry_run) ->
Vec<ImportSummary>`.** Walks the vault root:

- Files at `<vault_root>/<project>.org` → unfiled Project.
- Files at `<vault_root>/<area>/<project>.org` → Project
  filed under Area `<area>` (created via ensure_area when
  absent).
- Skips dot-prefixed entries (`.atrium/`, `.git/`, hidden
  temp files) for safety.
- Skips non-`.org` files silently.
- Sub-directories nested deeper than one level get a
  warning and skip — spec §7.3.1 has exactly one level of
  areas.

Returns one `ImportSummary` per imported file plus a synthetic
trailing summary for stragglers when only-skipped warnings
need a home.

**atrium-cli routing.** `run_import` stats the path and routes
file → `import_org_file`, directory →
`import_org_directory`. New `print_import_directory_summary`
aggregates counts across files for the human banner +
expands per-project detail underneath. `--json` output for
scripts.

**End-to-end smoke** verified manually: a 3-file vault
(`Inbox.org` at root, `Personal/Errands.org`, `Work/Q3.org`)
imports into 3 projects, 2 areas (auto-created), 2 tags
(auto-created via ensure_tag); `atrium-cli list projects`
renders the hierarchy as `Personal › Errands` and `Work › Q3`.

## v0.7.13 (2026-05-09) — File-level Org metadata round-trip

v0.7.12 closed the per-task
half of the round-trip discipline; v0.7.13 closes the
per-project half. With both in place, an .org file's preamble
+ headlines + drawer entries all survive a vault → Atrium →
vault round-trip cleanly.

**Parser additions.** `parse.rs` gains an additive
`parse_org_text_with_meta` / `parse_org_file_with_meta` pair
that returns an `OrgFile { directives, file_properties,
headlines }` instead of just a `Vec<OrgTask>`. The legacy
`parse_org_text` / `parse_org_file` keep their shape (call the
with-meta path and discard the preamble) so existing callers
don't break. Directives keys are upper-cased on parse for
case-insensitive lookups (`#+title:` and `#+TITLE:` both
produce the key `"TITLE"`). The :PROPERTIES: state machine
now distinguishes file-level (no current headline) from
headline-attached drawers; the former lands in
`file_properties`, the latter stays on the OrgTask.

**Emitter additions.** `emit.rs` gains
`emit_org_text_with_meta` / `emit_org_file_with_meta` that
takes the OrgFile shape. Directives sorted before emit so
`HashMap` iteration order can't perturb round-trips. A blank
line separates preamble from the first headline only when both
exist.

**Importer threading.** `import_org_file` reads `#+TITLE:`
(falls back to the file stem) and the file-level :PROPERTIES:
drawer for `:ID:` / `:SEQUENTIAL:` / `:REVIEW_INTERVAL:` /
`:LAST_REVIEWED:` / `:ARCHIVED:`. NewProject grows additive
`last_reviewed_at` and `archived_at` fields (Option<DateTime>)
to receive the imported values. The worker's `create_project`
SQL extends to include the two columns.

**Writer threading.** `write_project_to_vault` now builds an
OrgFile with `#+TITLE:` directive + a file-level :PROPERTIES:
block carrying every project metadata field that's set,
emitted via `emit_org_file_with_meta`. Project-level fields
that are NULL / default don't emit, keeping clean projects'
preambles minimal.

**Round-trip test** (`project_metadata_round_trips_through_db`)
imports a vault file with full project metadata, verifies the
DB row carries the expected values, exports back, and asserts
the regenerated file's preamble matches the source's project-
level fields. With this in place, projects round-trip cleanly
without losing project-scope flags.

## v0.7.12 (2026-05-09) — Custom-keyword Org round-trip (migration 0007)

Closes the loop on spec
§7.3.3 rule 1 — "Custom keywords map to a sentinel state on
import; the original is stashed in :ORIG_KEYWORD: and restored
on export" — at the data-model level rather than as a generic
property string in the .org file.

**Migration `0007_task_orig_keyword.sql`** adds a `task.orig_keyword`
TEXT NULL column. user_version 6 → 7. Existing tasks default
NULL = "no custom keyword recorded." v0.7.11 binaries reading a
v0.7.12 DB ignore the column.

**Domain Task + NewTask gain `orig_keyword: Option<String>`.**
Threaded through the read mapper, the worker INSERT, and every
NewTask / Task literal site (test_support, worker.rs's repeating-
task respawn, atrium-cli's run_add, atrium/src/ui/window.rs's
undo restore). Repeating-task respawn carries the value forward
so a `WAITING` task that completes still respawns as `WAITING`.

**Importer maps `OrgKeyword::Custom(name)` → `orig_keyword =
Some(name)` + canonical TODO sentinel.** No more lossy note;
the original is preserved in the DB.

**Writer's `task_to_org` checks `orig_keyword` first** when
choosing the headline keyword. Falls back to canonical TODO /
DONE based on `completed_at` when the column is NULL. Atrium's
UI never surfaces the column — completion semantics still flow
through `completed_at` alone.

**Why a column instead of `:ORIG_KEYWORD:`?** Atrium's task
model already has typed columns for everything else (tags,
defer, repeat, etc.); a generic property bag would be
out-of-character. The column is purely a round-trip anchor; if
a user removes the source vault file, the original keyword
still survives in the DB. The downside — a non-vault user
sees `WAITING` tasks rendered as TODO in Atrium's UI — is
intentional: Atrium's three canonical states are the surface
contract; the orig_keyword is upstream interop.

End-to-end test (`custom_keyword_round_trips_through_db`)
imports a file with `WAITING`, `IN-PROGRESS`, and `TODO`
headlines; exports the resulting DB; the regenerated file's
keyword sequence matches the source. Without this column the
test would fail with three `TODO` headlines.

## v0.7.11 (2026-05-09) — JSON snapshot export

The Org vault projection (v0.7.6
→ v0.7.10) is interoperable with Emacs / vim-orgmode but lossy on
constructs Atrium doesn't fully model (custom keywords fold to
TODO; project sub-headings drop through the writer; etc.). The
roadmap explicitly calls for a complementary lossless format:
"Atrium native JSON export ships in this phase too — universal
lossless backup format." v0.7.11 delivers it.

**`atrium-core::sync::json`.** New module. Top-level
[`Snapshot`] struct holds a `Vec<T>` per domain table:
`areas` / `projects` / `headings` / `tasks` / `tags` /
`task_tags` (as `(task_id, tag_id)` pairs) / `perspectives`.
Plus metadata: `version` ("1" for the v0.7.11 schema),
`exported_at` UTC timestamp, `atrium_version` (CARGO_PKG_VERSION).
Every domain type already derives Serialize / Deserialize so
the serializer is mostly composition.

`build_snapshot(conn)` reads every relevant table; uses
`list_all_projects` (a new additive read primitive that
includes archived projects, unlike the active-only
`list_projects`) so the backup is complete. New read
primitives `list_headings` and `list_task_tags` cover the
remaining tables.

`export_db_to_json_text(conn)` returns pretty-printed JSON.
`export_db_to_json_file(conn, path)` goes through the v0.7.6
`write_atomic` helper so a crash mid-write leaves any
previous backup intact.

**`atrium-cli export json PATH [--dry-run]`.** New export
target. Mirrors the `export org` shape: dry-run reports the
snapshot dimensions (counts per table) without writing; real
mode writes a single `.json` file at PATH. Output: human
(default) or `--json` (machine-readable summary).

**Re-import is deferred** — the use case is restore-from-
backup, not a hot path. A snapshot → DB importer can land
when there's a concrete need (cross-version migration, etc.).

**`DbError::Sync(String)` variant added** for serialization-
layer failures. Currently only the JSON exporter touches it
(serde_json failures, vanishingly rare).

## v0.7.10 (2026-05-09) — Vault writer + atrium-cli export org

v0.7.9 gave us the importer
(Org → DB); v0.7.10 lands the writer (DB → Org) so users can
round-trip in both directions. With this patch, an Atrium DB
can be projected to a vault directory, edited with Emacs / vim-
orgmode / any Org tool, and re-imported — the round-trip
discipline holds for every spec §7.3 construct already covered
by the importer.

**`atrium-core::sync::org::write::write_project_to_vault`.**
Reads a project + every task in it (open + done) + tag names
through a read-only `Connection`, builds an `OrgTask` tree
mirroring spec §7.3.2's field mapping in reverse:

- Task title → headline text
- Task note → body verbatim
- Task tags → headline `:tag1:tag2:`
- completed_at present → DONE keyword + CLOSED cookie
- completed_at None → TODO keyword
- scheduled_for / deadline → SCHEDULED / DEADLINE cookies
- task.uuid → `:ID:` property
- repeat_rule → `:RRULE:` property
- estimated_minutes → `:EFFORT:` `H:MM`
- defer_until → `:DEFER_UNTIL:` `YYYY-MM-DD`
- parent_id chain → nested headlines (depth = parent.depth + 1)

The destination path is `<vault_root>/<area_title>/<project_title>.org`
(or `<vault_root>/<project_title>.org` for unfiled projects).
Filename sanitization replaces filesystem-hostile chars with
`_` and collapses runs; empty / all-bad titles default to
"untitled". Emit goes through the v0.7.8 `emit_org_file` →
v0.7.6 `write_atomic` so a crash mid-write leaves the previous
file intact (spec §7.3.3 rule 6).

**`write_all_projects_to_vault`** walks `list_projects` and
calls `write_project_to_vault` for each. Used by the new CLI.

**New read primitive `list_all_in_project`.** The existing
`list_project` filters `completed_at IS NULL`; for the writer we
need open + done so the projected file reflects the full
project state. Additive — doesn't change the existing read API.

**`atrium-cli export org PATH [--dry-run]`.** New subcommand
parsed via `args::parse_export`. Walks every project in the DB
and writes one `.org` file per project under PATH. Dry-run mode
walks the project list and prints what *would* be written
without touching disk. Output: human (default) or `--json`
(machine-readable summary with per-project counts + paths).

**Limitations consciously deferred to v0.7.11+:** Project
sub-headings (the `heading` table) aren't emitted yet — they
round-trip as the importer's `headings_skipped` count grows on
each cycle. Custom keywords (`WAITING`, etc.) round-trip back
to TODO; the `:ORIG_KEYWORD:` machinery follows. File-level
project metadata (`#+TITLE:`, `:SEQUENTIAL:`,
`:REVIEW_INTERVAL:`, `:LAST_REVIEWED:`, `:ARCHIVED:`) not yet
emitted. Auto-debounced worker write hook (Atrium → vault on
TaskChanges) lands as a separate patch.

## v0.7.9 (2026-05-08) — Org importer (`atrium-cli import org`)

v0.7.6–v0.7.8 gave us the
foundation, parser, and emitter; v0.7.9 lands the one-shot
importer that lets users pull an existing .org file into the DB
through `atrium-cli`.

**`NewTask.uuid` / `NewProject.uuid` (additive).** Both creator
structs gain an `Option<String>` UUID field. `None` (and empty
strings) keep the historical "worker generates a fresh v4"
behaviour; `Some(s)` lets the importer preserve `:ID:` from the
source vault file (spec §7.3.3 rule 2: ":ID: is the round-trip
anchor"). All existing call sites updated. Three new worker
tests cover the round-trip + the empty-string fallback.

**`atrium-core::sync::org::import_org_file`.** Parses the file
through `parse_org_file`, derives the project title from the
file stem, and walks the headline tree creating tasks via the
worker. Field mapping per spec §7.3.2:

- Headline → Task.title
- Headline `:tags:` → Atrium tags via `ensure_tag` (idempotent),
  attached via `set_task_tags`
- Body → Task.note (verbatim)
- TODO / DONE / CANCELLED → keyword (DONE/CANCELLED toggled
  via `toggle_complete` after create)
- Custom keywords → folded to TODO with a lossy note
- SCHEDULED → `scheduled_for`, DEADLINE → `deadline`
- `:ID:` → `Task.uuid`
- `:RRULE:` → `Task.repeat_rule` (verbatim)
- `:EFFORT:` (`H:MM` or `Hh[Mm]` form) → `estimated_minutes`
- `:DEFER_UNTIL:` → `defer_until`
- Children → tasks with `parent_id` set

**Dry-run mode.** `import_org_file(handle, path, dry_run=true)`
walks the parse tree and tallies what *would* be created
without touching the DB. The atrium-cli surface is
`atrium-cli import org PATH --dry-run`.

**Limitations consciously deferred:** project sub-headings
(headlines without a TODO keyword) skipped and counted in
`headings_skipped` — heading-table writes follow in v0.7.10+.
DONE / CANCELLED tasks have `completed_at = now()`, not the
CLOSED cookie's timestamp — surfaced as a lossy note. Repeater
suffixes on SCHEDULED / DEADLINE recorded in the parsed tree
but not converted to RFC 5545 RRULE; use `:RRULE:` for canonical
round-trips. Multi-file vault walk lands in v0.7.10+. Re-imports
always create new rows; full bidirectional sync (Phase 17) adds
upsert-by-`:ID:`.

**`atrium-cli import org PATH [--dry-run]`.** New subcommand
parsed via `args::parse_import`, dispatched through the existing
worker-runtime helper. Output formats: human (default),
`--json` (machine-readable summary).

## v0.7.8 (2026-05-08) — Org-mode emitter (round-trip safe)

v0.7.6 + v0.7.7 gave us the
foundation + the parser; v0.7.8 lands the emitter that pairs
with it to satisfy spec §7.3.3's round-trip discipline. With
both halves in place, Atrium can now read an Org vault file
and write it back without losing or reordering the constructs
the spec §7.3 mapping pins down.

**`atrium-core::sync::org::emit_org_text`** takes a `&[OrgTask]`
and returns the Org text. Per-task layout:

- Headline: `*` × depth + `KEYWORD` (if any) + title + ` :tag1:tag2:` (if tags).
- Cookie line below the headline (only when at least one of
  scheduled / deadline / closed is set): SCHEDULED/DEADLINE
  rendered as active timestamps (`<YYYY-MM-DD Day [+repeater]>`)
  joined by single spaces; CLOSED rendered as inactive
  (`[YYYY-MM-DD Day HH:MM]`, with the time elided when it's the
  parser's noon-UTC default — matches Emacs's "date-only CLOSED"
  shape).
- `:PROPERTIES:` drawer (only when there are properties or
  unknown_lines): keys emitted in sorted order so `HashMap`
  iteration randomness can't perturb round-trips. Empty values
  emit as bare `:KEY:` per Org's canonical form.
- Body preserved verbatim from `OrgTask::body`; trailing newline
  added on emit (parser strips it on read).
- Children rendered recursively at depth+1 immediately after the
  parent's body.

**`atrium-core::sync::org::emit_org_file`** wraps the text emit
through the v0.7.6 `write_atomic` helper, satisfying spec
§7.3.3 rule 6. A crash mid-write leaves the previous version of
the file intact.

**Round-trip discipline.** 13 dedicated `roundtrip_*` tests
parse a representative input, emit it, re-parse, and assert the
two parsed trees are equal. Coverage spans every spec §7.3
construct: simple TODO, DONE+CLOSED, scheduled+deadline,
all three repeater modes (`+1d`, `++1w`, `.+2w`), headline
tags, properties drawer, body verbatim preservation, nested
subtasks, project sub-headings (no keyword), custom keywords
(WAITING), unknown-lines preservation inside the drawer, and a
kitchen-sink test combining everything in one document.

## v0.7.7 (2026-05-08) — Hand-rolled Org-mode parser

v0.7.6 laid the foundation
(sync module + atomic write + GSettings); v0.7.7 lands the
parser that everything from here on builds on. No third-party
dep — the v0.7.6 dep-research established that orgize and
starsector were both too dormant to take, and a focused
passthrough parser fits the use-case better anyway.

**`atrium-core::sync::org::parse_org_text` / `parse_org_file`.**
Reads Org text → `Vec<OrgTask>` tree. Coverage matches spec §7.3:

- Headlines `*+ [KEYWORD ]title [:tag1:tag2:]`. Stars give the
  depth; `KEYWORD` recognised as TODO / DONE / CANCELLED, with
  custom keywords (e.g. `WAITING`) preserved verbatim under
  `OrgKeyword::Custom`.
- Cookies on the line below a headline: SCHEDULED, DEADLINE
  (active timestamps `<...>`), and CLOSED (inactive `[...]`).
  All three can co-exist on one line.
- Repeater suffixes on SCHEDULED / DEADLINE: `+1w`, `++1w`,
  `.+1w` parsed into `OrgRepeater { mode, interval, unit }`.
- `:PROPERTIES:` drawer with `:KEY: value` entries until `:END:`.
  Keys preserve case. Garbage lines inside the drawer are
  preserved into the task's `unknown_lines` field for
  round-trip safety.
- Headline tags `:foo:bar:` validated for the canonical Org
  shape (rejects `:foo bar:` with whitespace inside).
- Nested subtasks: depth-based tree resolution. Headlines
  without a TODO keyword become project sub-headings per spec
  §7.3.1; deeper headlines nest under their nearest shallower
  ancestor.
- Headline body — everything between cookies/properties and the
  next headline — captured verbatim. Source blocks, tables,
  custom drawers, latex, links: all flow through unchanged so
  v0.7.8's emitter can re-emit them without loss.

**The "preserve unknown constructs verbatim" rule (spec §7.3.3
rule 1) is satisfied at two layers** — body content stays
verbatim; properties drawer entries that don't pattern-match
land in `OrgTask::unknown_lines` for explicit round-trip.

**Limitations consciously deferred to v0.7.8+:** multi-line
property values, active-timestamp time-of-day (date-only matches
Atrium's `scheduled_for`), file-level `#+TITLE:` capture (lands
when the importer needs the project title).

Pure additive change. No schema changes. No new dependencies.

## v0.7.6 (2026-05-08) — Phase 16 foundation (Org vault projection)

The roadmap calls for Org-mode
import + two-way vault sync, staged across v0.7.6 → v0.8.0 with
each patch shippable on its own. v0.7.6 lands the foundation
pieces that everything later builds on, plus the dep-research
decision that reverses the original plan.

**Org parser dep-research and the reversal.** CLAUDE.md listed
`orgize` as a pending dep for Phase 16. The v0.7.6 survey turned
up two practical issues: orgize's last stable release (`0.9.0`,
November 2021) is four years old; the active line has been in
alpha (`0.10.0-alpha.X`) since November 2023. The obvious
alternative — `starsector 1.0.1` — looked cleaner on paper
("structural parser/emitter with emphasis on avoiding edits
unrelated to changes") but its last release was October 2022 and
it pulls orgize-alpha as a transitive anyway. Conclusion:
hand-roll the Org subset Atrium needs, fitting the
CalibreQuarry stdlib-only ethos. The "preserve unknown
constructs verbatim" rule (spec §7.3.3 rule 1) is actually
*easier* in a focused passthrough parser: capture every
unrecognised line into the task's `unknown_lines` field and
re-emit verbatim on write. CLAUDE.md's dependency-discipline
section now records this decision so future passes don't
re-litigate it.

**Sync module skeleton.** `atrium-core/src/sync/mod.rs`
declares the module structure for the Phase 16 work coming in
v0.7.7+: `atomic` (write-temp + fsync + rename helper, lands
this patch) and `org` (hand-rolled parser + emitter, lands in
v0.7.7).

**Atomic write helper.** `atrium-core/src/sync/atomic.rs`
implements `write_atomic(path, contents) -> io::Result<()>` per
spec §7.3.3 rule 6: write to `<path>.atrium.tmp` in the same
directory, fsync, rename atomically. Best-effort cleanup of the
temp file on failure. Five tests cover the happy path,
overwrite, no-temp-file-leftover, and error cases (missing
parent dir, root path).

**Vault-path GSettings key.** New `vault-path` key in
`data/io.github.virinvictus.atrium.gschema.xml`, default empty
string (= "no vault configured"). Atrium runs DB-only when
unset, which is a valid configuration per spec §3.5. The proper
Settings → Org Vault → Choose folder UI lands in Phase 19.5;
v0.7.7+ patches will read the key directly via gio::Settings
when wiring the importer / writer / sync hook.

Test count: 119 + 174 + 1 + 106 + 106 = **506** (up 3 from
v0.7.5's 503 — the new atomic-write tests). Pure additive
change. No schema changes. No new dependencies.

## v0.7.5 (2026-05-08) — Visual refinement pass

The polish list deferred from v0.7.3 / v0.7.4 finally lands. Five
items, all aimed at reducing remaining boxiness on the rows and
panes after v0.7.0–v0.7.2 set the foundation.

**Tag pill softening.** The `.atrium-task-tags` chip retired the
visible bg-color (`alpha(@accent_bg_color, 0.15)`) in favour of
bare colored Pango spans. Each `<span foreground=HEX>#tag</span>`
still renders the per-tag colour for a row with multiple tags
the colors stack inline, reading as typography rather than as a
Bootstrap-style badge stuck onto the row. The `.completed`
override goes away (no more chip background to dim; the row's
existing opacity does the work).

**Inspector empty state.** The big AdwStatusPage with the
edit-symbolic icon and "No task selected" / "Select a row to
edit it here." was claiming the full pane during navigation;
v0.7.5 swaps it for a small centred caption ("Select a task to
edit it here.") near the top of the pane. The pane's
atmospheric tint signals the inspector's home; the caption is
just a hint.

**Sidebar filter ghost.** The "Filter lists…" search entry got
the same opacity-on-hover/focus treatment the v0.7.0 quick-add
introduced. New `.atrium-filter-ghost` class on the GtkSearchEntry,
mirroring `.atrium-quick-add` semantics — dim at rest, full on
:hover / :focus / :focus-within.

**Row separator fade.** The `.atrium-task-listview > row`
border-bottom alpha dropped 0.30 → 0.12. After the v0.7.0 row
margin bump (6 → 9 px) the separators were reading as ledger-
grid against the new whitespace; the lower alpha keeps a quiet
scan-tracking line without outshouting the spacing.

**Sidebar selection soft-fill.** The `:selected` state on
sidebar rows gained `border-radius: 8px`, an `outline: none`
override, and a 4 px horizontal margin so the rounded fill has
breathing room to bloom rather than clipping at the listbox
edge. Mirrors the v0.7.0 task-row selection treatment —
selection becomes a glow, not a flat-bottomed rectangle.

Pure CSS + small UI tweaks. No schema changes. No new
dependencies. 503 tests still green.

## v0.7.4 (2026-05-08) — Task-level Mark Reviewed (migration 0006)

The Review page's "This week" weekly walk shipped at v0.7.2 with
no way to acknowledge an item — clicking through it revealed the
gap. v0.7.4 closes it with a true Mark Reviewed action mirroring
the Phase 13 project-level pattern.

**Schema.** Migration `0006_task_last_reviewed_at.sql` adds an
additive `task.last_reviewed_at TEXT NULL` column. Mirror of
`project.last_reviewed_at` from migration 0001. Existing user
DBs migrate cleanly; user_version 5 → 6.

**Worker.** `Command::MarkTaskReviewed { id, responder }` +
`WorkerHandle::mark_task_reviewed(id)` mirror the project-side
wiring exactly. Handler runs `UPDATE task SET last_reviewed_at
= strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1`, fetches
the updated row, emits `TaskChanges{updated: vec![task]}`. Two
new tests cover the round-trip and the not-found case.

**UI.** Each row in the Review page's "This week" section now
carries a trailing flat **Mark Reviewed** button (the agenda
row treatment stays exactly the same — it's wrapped in a
horizontal Box with the button as a sibling). Clicking the
button dispatches `worker.mark_task_reviewed`; the row drops
out via the TaskChanges-driven page rebuild.
`apply_task_changes` now routes Review the same way it routes
Forecast / Logbook / Agenda / Perspective — full page rebuild
on any delta.

**Filter.** `refresh_review_page` now excludes tasks whose
`last_reviewed_at` is within the last 7 days from `today`.
After 7 days the row resurfaces if it still matches the
weekly-walk filter. A small inline note above the section
("Mark items reviewed to hide them for 7 days.") tells users
what the button does.

Test count: 119 + 171 + 1 + 106 + 106 = **503** (up 2 from
v0.7.3's 501 — the two MarkTaskReviewed worker tests).
docs/schema.md picked up migration 0006 + the new column entry.
Pure additive change. No spec semantics shifted; no new
dependencies.

## v0.7.3 (2026-05-08) — Inspector check-off + perspective editor

Two functional gaps Brandon caught after living with v0.7.2:
the inspector had no way to mark a task complete (he had to
bounce back to the row to click the checkbox), and there was
still no GUI path to add or edit a saved perspective (only the
shared rename/delete actions and the renderer-config dialog had
landed; creating a new perspective required `atrium-cli`).

**Inspector check-off.** A circular CheckButton now sits at the
leading edge of the inspector's title row, mirroring the row-
checkbox in the task list (same `.selection-mode` class). State
reflects `task.completed_at`; clicks dispatch through
`worker.toggle_complete(id)`. A `Cell<bool>` latches the
persisted state so the worker round-tripping the toggle doesn't
ping-pong with the user click. Reachable while the inspector is
open without leaving the pane.

**Perspective editor.** A new `prompt_edit_perspective` dialog
covers all four perspective fields in one place: name, filter
expression, renderer (List / Board radios), and columns
(comma-separated tag names; sensitive only when Board is
selected). Used in two flows:

- **Create.** A "+" affordance trailing the *Perspectives*
  sidebar section header opens the dialog in create mode. On
  Save, dispatches `worker.create_perspective(NewPerspective{
  name, filter_expr, renderer, renderer_config, .. })`.
- **Edit.** The right-click context menu on a perspective row
  collapsed from three items (Rename / Configure renderer /
  Delete) to two (**Edit…** / Delete). Edit opens the dialog
  pre-filled with the existing values; on Save, dispatches a
  full `worker.update_perspective(PerspectiveUpdate)` covering
  name + filter + renderer + renderer_config in one round-trip.

The previous Rename + Configure renderer flows still exist as
plumbing (the `win.rename-active` and `win.configure-renderer`
actions are unchanged), they just no longer appear in the
perspective context menu — Edit subsumes both. Other surfaces
that fire `win.rename-active` against a perspective (none
currently) would still work.

Pure code patch. No schema changes, no new dependencies. Test
count unchanged at 501. Ship-gate runs in under 2 seconds.
## v0.7.2 (2026-05-08) — Confusion-killer patch

Brandon's after-v0.7.1 review of the Review page surfaced two
problems we'd previously planned to fix in tier 3 of the v0.7
polish arc but hadn't gotten to: the canonical Review page and
the seeded "Weekly Review" perspective both lived in the sidebar
under almost the same name and showed completely different
content (Review page: "All caught up"; Weekly Review perspective:
a long list of tasks). And the upper-left corner still had the
centered "Lists" header from libadwaita's default sidebar
auto-title, which contradicted the magazine-spread treatment
v0.7.0 introduced for the right side.

**v0.7.2 fixes both:**

1. **Review = Weekly Review merge.** The canonical Review page
   now renders two sections in one surface — "Projects to
   review" (the existing Phase 13 review queue) followed by
   "This week" (the open-tasks-this-week filter that was
   formerly seeded as a saved perspective). Both sections show
   inline notes when empty; the page falls back to "All caught
   up" only when both are empty. Section 2 reuses
   `agenda::build_row` for visual consistency with the Agenda
   canonical page; clicking a row opens the Inspector for that
   task. The seeded "Weekly Review" perspective is retired (the
   `seed_initial_perspectives` helper, the
   `WEEKLY_REVIEW_NAME` constant, and the four
   `seed_weekly_review_*` tests removed; the filter constant
   survives as `REVIEW_WEEKLY_WALK_FILTER`, used by the GUI's
   refresh path). Existing user DBs keep their row (we don't
   delete data); fresh DBs and fixtures land clean.

2. **Drop the "Lists" centered title.** The sidebar's
   AdwHeaderBar now carries an empty AdwWindowTitle as its
   title-widget, suppressing the auto-rendered "Lists" label.
   The header becomes pure chrome (which is empty, since
   show-end-title-buttons=false), and the filter entry below
   acts as the sidebar's visual top. Mirrors the
   title-suppression on the content side from v0.7.0.

Pure code patch — no schema changes, no new dependencies, no
spec semantics shifted. Roadmap: this is the tier-3 functional
work from the v0.7 polish arc landing earlier than planned, at
Brandon's call. The visual refinement (tag pills, inspector
empty-state, filter ghost, row separators, sidebar selection
softening) ships next as v0.7.3.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to
**0.7.2**.

## v0.7.1 (2026-05-08) — Surface continuity (kill the colour breaking)

Brandon's first reaction to v0.7.0: the magazine-spread title
landed, but the upper-left corner now showed visible "colour
breaking" — distinct horizontal bands of tone where the headerbar,
filter entry, and listbox met. Three things were stacking
unhelpfully:

1. The v0.6.10 standalone `headerbar` accent gradient — painted a
   leading-edge accent on every headerbar in the app, including
   the inner sidebar + content headerbars.
2. The libadwaita-default headerbar background — the inner
   headerbars had their own elevated bg-color sitting on top of
   whatever surface I'd painted underneath.
3. The v0.7.0 surface gradients — applied only to the inner
   widgets (`.navigation-sidebar` listbox, `.atrium-inspector-pane`
   PreferencesPage), not to the headerbar / filter / scrolled-window
   above them. The atmospheric tint started mid-surface, leaving
   a visible band where it began.

v0.7.1 simplifies all three:

- **Drop the v0.6.10 standalone headerbar gradient.** Surface
  gradients do the atmospheric work now; the headerbar layer was
  redundant. Replaced with a scoped `.atrium-main-toolbar
  headerbar { background: transparent; box-shadow: none; }` rule
  so the surface flows continuously behind the headerbars.
- **Replace the v0.7.0 directional surface gradients with a flat
  per-pane tint.** The 160deg / -20deg gradients were painting
  banded tones across surfaces; the flat
  `background-color: alpha(@accent_color, 0.04)` paints a uniform
  warmer tone across the whole sidebar / inspector. No bands.
- **Move the tint from the inner widget to the whole pane.** Class
  `.atrium-sidebar-pane` on the sidebar's AdwToolbarView (so the
  tint covers the headerbar area, the filter entry, and the
  listbox in one continuous fill); the inner widgets are made
  transparent so the parent's tint shows through. Same for
  `.atrium-inspector-pane`.

Net effect: the upper-left corner is no longer three stacked
horizontal bands of slightly different tone. The sidebar reads as
one continuous warmer surface from the top of the window down.
Same for the inspector on the opposite side. The neutral content
area in between is the calm centre.

Sacrifice: the directional gradient (warm at the corners, fading
toward the centre) is gone. v0.7.0's "OF4-atmospheric" was
ambitious; the flat tint is more "Things-3-calm" — uniform tone
distinguishing the panes by hue rather than by gradient drama.
The visible-banding cost wasn't worth the directional warmth.

Pure CSS + window.ui patch. No code changes. All 505 tests still
green. VERSION / Cargo.toml / patchnotes / AppStream metainfo
bump to **0.7.1**.

## v0.7.0 (2026-05-08) — Visual fusion + whitespace pass

The first big polish minor of the v0.7 line. Addresses Brandon's
critical-eye review of the v0.6.21 screenshot: the app didn't feel
"living" yet — accents had hard boundaries, the three panes were
visually identical rectangles separated by 1 px verticals,
selection states read as outlines instead of glows, and Linux-app
disregard for whitespace had crept into the row rhythm and the
inspector. Two tiers:

**Tier 1 — Living surface (the fusion pass):**

- **Three-pane atmosphere.** The sidebar's existing soft-accent
  gradient bumps from 0.025 → 0.05 alpha; the inspector pane gains
  a mirrored gradient on its leading edge (`-20deg` so the warm
  corner is on the opposite side). The two side panels now flank a
  neutral content area; the eye reads three connected spaces
  instead of one rectangle bisected by hard verticals. `data/style.css`.
- **Selection state on task rows is no longer a rectangle.** The
  default libadwaita selection paints a strong accent fill plus a
  focus outline; combined with the area-stripe and the row
  separator, selected rows looked like 1 px orange bordered boxes.
  v0.7.0 ships a soft accent fill (alpha 0.14, no border, no
  outline, rounded corners) — selection becomes a glow, not a
  frame. `data/style.css`.
- **Area accent moved from a 3 px hard left stripe to a row-wide
  gradient bleed.** The stripe approach made each row read as
  "rectangle with stripe stuck on" — the eye saw the stripe as a
  separate decorative element. The gradient (alpha 0.10 fading to
  transparent at 40% width) makes the *row* read as area-tinted.
  Six per-color rules updated; the reserved 3 px left-border on
  `.atrium-task-row` retired. `data/style.css`.
- **Sidebar section headers softened.** The v0.3.0 treatment was
  uppercase + tight tracking + a top-border divider — read as a
  partition. v0.7.0 retires the all-caps and the divider for
  medium weight, mixed case, breathing room above and below. The
  headers introduce the rows that follow rather than separating
  them from above. `data/style.css`.
- **Quick-add entry as a ghost.** The "Add task…" row at the
  bottom of the list was always-visible and always-bordered. v0.7.0
  dims it to ~0.45 opacity by default; hover or focus inside the
  box brings it back to full presence with a 180 ms ease-out
  transition. `data/window.ui` + `data/style.css`.

**Tier 2 — Whitespace pass (Brandon's specific call-out):**

- **Task-row vertical rhythm.** Margin top + bottom 6 → 9 px on
  every row. Things 3 / OmniFocus leave real air between rows;
  Linux apps habitually do not. The change adds 6 px of total
  vertical breathing per row without touching density on the row
  content. `atrium/src/ui/task_list.rs`.
- **Inspector pane field clustering.** Was: Schedule + Deadline +
  Project in one group, Tags alone in its own one-row group (an
  orphan card the eye couldn't justify). Now: dates_group carries
  only the date fields, and Project + Tags collapse into a new
  Classify cluster — both fields answer the question "where does
  this task live?" so the eye groups them naturally. Five visual
  groups overall, none of them orphans. `atrium/src/ui/inspector_pane.rs`.
- **Magazine-spread page title.** "Today" (and every other view
  name) was centered in the AdwHeaderBar — read as a tabular UI
  heading, not a page title. v0.7.0 suppresses the auto-title in
  the header bar and adds a strip below carrying the view name as
  a large left-aligned heading + an optional supporting subtitle
  beneath. The subtitle ships for Today (today's date in long
  form), Upcoming ("Next 7 days"), and Forecast ("Next 30 days");
  hidden on views without a useful subhead. `data/window.ui` +
  `atrium/src/ui/window.rs` + `data/style.css`.

No schema changes. No new dependencies. All 505 tests still
green; ship-gate runs in under 2 seconds.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to
**0.7.0**.

## v0.6.21 (2026-05-08) — Documentation housekeeping pass

Pure docs patch — bringing references that hadn't been touched
in several minors back into alignment with the current state.
No code touched. Ship-gate green.

The post-v0.4.0 release arc landed a lot in a hurry: the
search-engine extraction (`atrium-search`), the headless CLI
(`atrium-cli`), FTS5 ranking, the SQL-translation evaluator,
two new migrations (`area.color` + `perspective.renderer` /
`renderer_config`), the kanban renderer, the Agenda page, the
v0.6.x screenshot-driven cleanup arc, and the v0.6.19 / v0.6.20
roadmap revision. Several reference docs lagged behind. This
patch pulls them current.

**`README.md`:**
- Version badge `0.5.0` → `0.6.20`.
- "Both modes ship at v0.5.0." paragraph rewritten to "Both
  modes shipped early." with the current release noted.

**`CLAUDE.md`:**
- Status section: collapsed the "v0.6.0 carryover" framing
  (carryover is all shipped) and replaced it with three
  consolidated paragraphs walking the v0.5.0 → v0.5.4
  search-engine arc, the v0.6.0 → v0.6.5 Slice D arc, the
  v0.6.6 → v0.6.10 perf / sidebar / soft-accent arc, and the
  v0.6.11 → v0.6.20 screenshot-cleanup + roadmap-revision arc.
- Authoritative documents: `roadmap.md` description updated
  (now four sub-phases — 12.5, 15.5, 15.75, 19.5 — not three);
  `patchnotes.md` description updated ("v0.3.0 is the most
  recent release" → "v0.6.20 is the most recent release").
- Codebase map: header `v0.4.x` → `v0.6.20`. Added the missing
  files: `atrium-search/{dates,rank,sql_translate}.rs`,
  `atrium-core/{quick_entry,render}.rs`, migrations
  `0004_area_color.sql` + `0005_perspective_renderer.sql`,
  `atrium/src/ui/{agenda,board,logbook}.rs`. Updated read.rs
  / command.rs descriptions to mention the surfaces added in
  v0.5.x and v0.6.x. Removed the lifted `quickentry/parser.rs`
  entry (parser moved to `atrium-core::quick_entry` at v0.4.5).
- Test counts: `82 + 165 + 1 = 248 tests as of v0.4.0` →
  `119 + 173 + 1 + 106 + 106 = 505 tests as of v0.6.20`.

**`docs/schema.md`:**
- Removed the "No mid-v0.1 schema changes" framing — the v0.1
  freeze ended at v0.2.0.
- Added a migration-history table covering 0001 → 0005,
  including the v0.5.0 additions (`area.color`,
  `perspective.renderer` / `renderer_config`).
- ER diagram: added `AREA.color`, `TASK.repeat_mode`, the
  full `PERSPECTIVE` entity (was missing entirely), and the
  saved-search relation.
- Per-table rationale: added `repeat_mode` to the task
  notes, added the missing `perspective` section, and added
  `color` to the area section.

**`docs/perf-baseline.md`:**
- Refreshed the v0.0.28 capture against current binaries.
  Cold start: 30–40 ms / ~34 MB (was 25–33 ms / ~32 MB).
  Fixture generation across small / medium / large scales
  remains under 39 MB peak RSS at 50K tasks; the data layer
  is still nowhere near the §8 budget. Numbers within noise
  of the original capture despite four major arcs of feature
  work intervening.
- Note added: search-engine evolution did not regress the
  data-layer budget.

**`docs/regression.md`:**
- Step table: added the 5.5 `atrium-cli` end-to-end smoke
  (added at v0.5.x, grown through v0.6.x), with notes on
  what it covers — read paths over every canonical list,
  write paths over every CRUD subcommand, the kanban smoke
  against the fixture-seeded "Fixture Board" perspective,
  and the v0.6.5 perspective write-side smoke.
- Cold-start observed numbers updated to match the refreshed
  perf baseline.

**`docs/keymap.md`:**
- Removed the "*(view lands Phase 5)*" suffixes — all six
  canonical lists shipped at v0.1.0.
- Added a note about Agenda / Forecast / Review joining the
  top-tier sidebar at v0.6.7 / v0.6.16 without dedicated
  number accels.
- Search-filter section rewritten — the flat AND-only
  grammar grew into a full expression language at v0.4.0
  / v0.5.0; pointed at `spec.md` §4.3 as the canonical
  reference and called out the `?` operator-reference
  popover.
- Builder Mode chord table reframed — Builder Mode shipped
  at v0.2.0 but the `Ctrl+Shift+F` / `Ctrl+P` / `Ctrl+D`
  chords are still aspirational slots (these features ship
  via the sidebar / Inspector today, not via accels).

**`docs/accessibility.md`:**
- Header note added: the Phase 8f findings cover the v0.1
  surface area; the Builder Mode side pane, Forecast,
  Review, Perspectives, kanban renderer, and Agenda all
  inherit the same widget primitives but owe a full re-audit
  at the next minor.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to
**0.6.21**.

## v0.6.20 (2026-05-08) — Phase 19.5 calendar item: iCal feed → Evolution Data Server

Brandon course-corrected the original "read-only iCal calendar
feed" item that landed in v0.6.19's Phase 19.5 list. The right
integration model for a GNOME-native client running on Fedora
isn't a `.ics` file feed — it's reading the system's calendar
service.

GNOME 50's default calendar app (`gnome-calendar`) doesn't store
its own calendar data; it consumes Evolution Data Server (EDS),
the GNOME-wide calendar/contacts/tasks backend. The user has
already configured their accounts (Google, Nextcloud, local,
exchange-web-services, …) in EDS via GNOME Online Accounts. An
iCal-file feed would either duplicate that work or sit awkwardly
alongside it.

Updated framing: Atrium reads EDS via D-Bus and overlays calendar
events onto the Forecast / Today views as read-only context.
Endeavour does the same shape for *tasks* — Atrium does it for
*calendars* without becoming a calendar client. Dependency check
deferred: either `libecal` / `libedataserver` bindings or a
hand-rolled `zbus` D-Bus client. No `.ics` file plumbing.

Files touched:
- `roadmap.md` — Phase 19.5 third item rewritten.
- `spec.md` — no change needed (it didn't reference the iCal
  framing; the calendar overlay isn't in the import / export
  table because it's not import / export — it's read-side
  display-only context).
- `CLAUDE.md` — "Phase 16 is what's next" paragraph item list
  updated.
- `README.md` — landing-paragraph item list updated.
- `data/io.github.virinvictus.atrium.metainfo.xml` — v0.6.19
  release description updated to match.

Pure docs change; no code touched. Ship-gate green.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to
0.6.20.

## v0.6.19 (2026-05-08) — roadmap revision: drop Things 3, elevate Org-mode + Todoist, add Phase 19.5 (productivity essentials)

Pure docs change. Brandon commissioned a feature-survey pass against
competing native-Linux + cross-platform todo apps to identify gaps
in Atrium's roadmap. The findings drove a four-part revision.

**1. Phase 16 (Things 3 Import) retired.** `.things` JSON requires
a macOS export step Linux users don't have access to. As Brandon
put it: "how many people using GNOME are gonna be Things 3 users?"
Things 3 stays in the inspiration paragraph (Simple Mode's calm
+ six-list shape comes from there) but the import phase goes
away. Same logic applied indirectly to OmniFocus — kept open as a
Phase 19 long-tail entry rather than its own phase, since
`.ofocus` has the same macOS-only access problem.

**2. Org-mode promoted to Phase 16 + 17 (was 17 + 17.5).** Brandon's
"MUST" interop direction. Atrium's vault is fully compatible with
Emacs / Doom / vim-orgmode out of the box: open the same
`~/Tasks/` directory in `org-agenda` and the result should look
like Atrium's Agenda canonical page. The two-stage plan (one-shot
import + DB→vault writer at Phase 16; full two-way `inotify` sync
at Phase 17) stays, but the framing tightens to a single must-ship
goal and a new acceptance test pins the agenda parity (with a
synthesised vault, both Atrium's Agenda and `M-x org-agenda`
should bucket tasks the same way).

**3. Todoist promoted to its own Phase 18.** Was bundled into the
Phase 19 long-tail. Brandon's gap-analysis prompt explicitly said
"Todoist would be a good one" — its install base on Linux is real
(web client + Linux Electron app) and CSV export is friction-free.
Now first-class with its own phase. Phase 19 becomes the long-tail
batch (Taskwarrior, VTODO, todo.txt, TaskPaper, OmniFocus).

**4. Phase 19.5 added — productivity essentials.** The gap-analysis
surfaced nine items competing apps have that Atrium doesn't:

- **System notifications / time-based reminders.** Things 3 /
  OmniFocus / Planify all push reminders via the system
  notification daemon. Atrium has zero notification code
  (`libnotify` / `gio::Notification` not imported anywhere).
  For a productivity app this is the biggest 1.0 blocker.
- **Subtasks UI exposure.** `parent_id` has been in the schema
  since `0001_initial.sql` but the GUI doesn't render the
  hierarchy. Schema-supported, UI-missing.
- **Evolution Data Server (EDS) calendar overlay — read-only.**
  Brandon course-corrected the original "iCal feed" framing:
  Atrium is a GNOME-native client on a desktop that already
  has a calendar service. EDS is the GNOME-wide
  calendar/contacts/tasks backend that GNOME Calendar
  (`gnome-calendar`, default in GNOME 50) consumes; the user
  has already configured their accounts there. Read whatever
  EDS exposes via D-Bus and overlay events onto Forecast /
  Today. No `.ics` file plumbing — that would duplicate what
  EDS already does properly. Endeavour does the same shape
  for *tasks*; Atrium does it for *calendars* without
  becoming a calendar client.
- **`AdwPreferencesWindow`.** No app-level preferences dialog
  exists; GSettings keys are set programmatically. Build one.
- **Task dependencies (`blocked_by`).** Taskwarrior treats this
  as fundamental. New `task_dependency` table; `is:available`
  predicate extends to dependency-blocked tasks too.
- **Drag external files / URLs to capture.** Standard Linux
  desktop pattern; explicit in Errands / Planify.
- **Task templates.** Reusable shapes (project + standard
  subtasks). Todoist; Org-mode capture templates as
  conceptual reference.
- **First-run / onboarding.** Sample tasks, welcome project,
  guided three-step intro. Standard commercial-app pattern.
- **Backup / restore UI.** SQLite file-copy is the existing
  escape hatch but no in-app affordance.

Each Phase 19.5 item names its source in `roadmap.md`.

**Sources** (read public README/docs/feature pages — no code
copied):

- Errands — GTK4 / Python — subtasks, drag-drop, accent colors,
  CalDAV / Nextcloud sync.
- Planify — GTK4 / Vala — Todoist + Nextcloud + CalDAV sync,
  multi-reminder, attachments, recurring patterns.
- Endeavour — GTK4 / C — GNOME Online Accounts integration.
- Things 3 — macOS native — Today / This Evening / Upcoming /
  Anytime / Someday / Logbook canonical lists, magic plus
  button, calendar integration, share extensions, Things URL
  scheme, Siri / Shortcuts.
- OmniFocus 4 — macOS native — sequential vs parallel projects,
  Mail Drop, Omni Automation, web access, weekly review, focus
  mode.
- Taskwarrior — CLI — real task dependencies, virtual tags,
  urgency formula, UDA fields, hooks API, named dates, snooze.
- Todoist — cross-platform — natural language input, sub-tasks,
  sections, comments, file attachments, custom filters,
  list/board/calendar view toggle, templates.
- Super Productivity blog comparison piece — open-source
  productivity-app survey.

Files touched: `roadmap.md` (full Phase 16-19.5 rewrite),
`spec.md` (§7.1 import sources table cleaned, §7.4 Linux
landscape table updated, version line bumped), `CLAUDE.md`
("Phase 16 is what's next" line updated), `README.md` (landing
paragraph + Imports section + new Acknowledgments section).
No code changes. No tests touched.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to
0.6.19.

## v0.6.18 (2026-05-08) — efficiency pass: SQL fast-path everywhere search runs

Brandon asked for a top-to-bottom efficiency pass. After surveying
the codebase the honest answer is: Atrium is already pretty efficient
by construction (single-writer worker, read pool, prepared statements
via `prepare_cached`, WAL + tuned pragmas, cold start consistently
20–30 ms, ship-gate runs in under 2 seconds). The clippy pedantic
pass surfaced 250+ items but they're cosmetic — `doc-markdown` nits,
`module-name-repetitions`, etc. — not real efficiency wins.

The actual hot-path wins came from finishing two earlier deferrals
plus eliminating one duplicate DB query:

- **List-renderer perspective path uses the SQL fast-path.** v0.5.3
  shipped the SQL translation evaluator and v0.6.6 wired it into the
  kanban refresh; the deferred case noted in the v0.5.3 patchnote
  was the regular *list*-renderer perspective path — saved
  Perspectives whose renderer is `"list"`. v0.6.18 wires the
  fast-path here too. Translatable filters (most: `is:open`,
  `tag:work`, `due:today`, …) load only matching rows from SQLite
  instead of pulling every task and filtering in Rust. At
  fixture scale (1k tasks) the win is measurable; at 10k+ it
  dominates. Untranslatable expressions (regex / fuzzy / composite
  `is:today` / etc.) keep the in-memory `filter::apply` path —
  no semantic change.

- **Search-bar (SearchResults) path uses the SQL fast-path.** Same
  shape. The bar fired `list_all_tasks` on every keystroke (after
  the 200ms debounce) when the parser successfully built an
  expression; now it fires `list_tasks_matching` with the
  translated `WHERE` clause instead. Same fallback behaviour for
  expressions the translator can't yet express.

- **Eliminate duplicate tag-map DB query on perspective + search
  refresh.** Both paths fetched `tag_names_per_task` *and*
  `tag_info_per_task` back-to-back — same JOIN with one extra
  column on the second query. New helper
  `crate::ui::task_list::tag_names_from_pills(&TagPillMap) ->
  TagMap` derives the name-only view from the colour-bearing pill
  map locally, so we fetch once and project twice. Saves one DB
  roundtrip per refresh.

What I deliberately *didn't* do:

- **Did not download other Rust to-do apps for inspiration.**
  Brandon authorised it but the time cost is high and the
  marginal value is low — Atrium's architecture already follows
  the canonical patterns (worker queue, read pool, GtkListView
  factories with property bindings, FTS5 + bm25 ranking). The
  three wins above came from our own deferred work, not from
  external patterns. If a specific external technique becomes
  relevant later we can attribute it then.

- **Did not chase the 250+ pedantic clippy warnings.** They're
  cosmetic — `doc-markdown`, `module-name-repetitions`,
  `must-use-candidate`, etc. The standard `cargo clippy
  --workspace --all-targets -- -D warnings` is and stays clean.

- **Did not refactor HashMap closure captures into `Rc<HashMap>`.**
  At our scale (typical user has < 200 areas + projects + tags
  combined) the per-refresh clones cost less than 1ms. The
  cleaner ownership model isn't worth the API churn until a real
  workload pushes back.

- **Did not chase `LIKE %x%` table scans.** Bare-text matches in
  the SQL translator emit `LOWER(t.title) LIKE %?% ESCAPE '\\'`,
  which can't use an index. At 100k tasks this would matter; at
  fixture scale (1k) it's ~5ms. The right answer is FTS5 for
  bare text — already used for bm25 ranking — but plumbing it
  through the translator is a bigger surgery best done when
  someone's actually feeling the pain.

## v0.6.17 (2026-05-08) — Forecast view: click-to-open

Brandon flagged that clicking a task in the Forecast view did
nothing. The forecast row had a `gtk::DragSource` (so drag-to-
reschedule worked) but no `gtk::GestureClick` — the row was a
visual dead-end for tap-to-open.

v0.6.17 adds the same `on_row_click` callback shape board and
agenda already use. Single-click on any forecast row (including
the trailing rows under the Overdue card) activates
`win.edit-details-for(id)` and opens the task in the Inspector.
GTK4's drag-threshold means the click + drag controllers
coexist cleanly: a press that doesn't drift past the threshold
fires as a click; a press-and-drag past the threshold initiates
the reschedule drag.

What's in the patch:

- **`atrium/src/ui/forecast.rs`.** `build_page` /
  `build_overdue_block` / `build_day_card` / `build_entry_row`
  all gain an `on_row_click: F` parameter (`F: Fn(i64) +
  'static + Clone`). The callback plumbs from `build_page`
  through the day cards down to each row's `GestureClick`.
- **`atrium/src/ui/window.rs::refresh_forecast_page`.** Builds
  the closure with a `downgrade`d window weak ref and routes
  through `WidgetExt::activate_action(window, "win.edit-details-for", id)`.
  Identical pattern to the board and agenda click handlers.

This closes the last "row doesn't open" gap I'm aware of —
list / kanban / agenda / forecast all open Inspector on
single-click now.

## v0.6.16 (2026-05-08) — sidebar order: Logbook bookends the top tier

Brandon flagged that Logbook in the middle of the top-tier set
(between Someday and Agenda) read as out of place — completed
work was interrupting the flow of active / future-facing lists.
v0.6.16 moves Logbook to the trailing slot so the past lives
where the past belongs.

New top-tier order (both modes):

```
Inbox       capture
Today       today's plate
Upcoming    future scheduled
Anytime     no time commitment
Someday     parked
Agenda      now-picture across days
Forecast    calendar projection (Builder-only)
Review      project review queue (Builder-only)
Logbook     completed past
```

The active/future-facing lists run unbroken from Inbox through
Agenda; Builder mode inserts Forecast + Review without
disturbing the bookends; Logbook closes the top tier so the
sidebar reads as "now → future → past" top to bottom.

What's in the patch:

- **`atrium/src/ui/window.rs`.** Logbook removed from
  `CANONICAL_LISTS` (now five entries: Inbox / Today / Upcoming
  / Anytime / Someday). `top_tier_extras` extended to always
  include Agenda + Logbook; Forecast + Review still gated on
  Builder mode and still sit between Agenda and Logbook.
- **`refresh_canonical_badges`** updated. Logbook's count
  badge moved from the canonical Vec to its own
  `logbook_badge: Option<gtk::Label>` cell; the refresher
  updates both. The `rebuild_dynamic_sidebar` loop captures the
  Logbook badge as it builds the top-tier rows so the count
  stays live across `TaskChanges`.
- **Three unit tests updated** to match the new shape (`CANONICAL_LISTS.len()` is 5, simple-mode extras are Agenda + Logbook, builder-mode extras are Agenda + Forecast + Review + Logbook).

CSS, behaviour, and badge tinting are unchanged — Logbook keeps
its `.atrium-canonical-logbook` purple-2 accent, just at a
different visual position.

## v0.6.15 (2026-05-08) — Memory Watch background + Debug → Generate Fixtures fix

Two real bugs Brandon surfaced testing v0.6.14:

- **Memory Watch dialog had no visible body / background.** Labels
  appeared to float against the system desktop. `adw::Window` with
  an `AdwToolbarView` content slot doesn't auto-paint a window
  background on every theme — the toolbar's content slot is
  transparent and the window underneath wasn't rendering its
  `@window_bg_color`. CSS fix: explicit
  `window.atrium-debug-pane { background-color: @window_bg_color }`.
  The class was already on the window for the monospace font; we
  reused it.

- **Debug menu's *Generate Fixtures* did nothing visible.** The
  action handler was opening a fresh writable connection, writing
  rows directly, but never told the GUI to re-read. The worker's
  read pool kept showing the old cached state, so the sidebar /
  list looked exactly the same after the user clicked. Brandon
  worked around it by running fixture generation via the CLI,
  which spins a fresh process and starts cold. The fix is to
  queue a `rebuild_dynamic_sidebar` + `refresh_active_list` after
  the spawn_blocking write completes — the read pool then
  re-queries the DB and the new rows appear.

Code:

- `data/style.css` — one CSS rule binds `@window_bg_color` to the
  Memory Watch window class.

- `atrium/src/ui/window.rs` — `rebuild_dynamic_sidebar` was
  private; promoted to `pub` so the binary's debug action handler
  can call it.

- `atrium/src/main.rs` — `install_fixture_action` rewritten. The
  DB write now runs via `gio::spawn_blocking` (off the main
  thread; ~30 ms small / ~150 ms medium so a UI freeze would be
  visible), and on completion the closure resumes on the GTK main
  thread to call the window's refresh methods. The previous code
  used `runtime().spawn` (tokio) and tried to capture the
  `adw::Application`, which isn't `Send` — the rewrite uses
  glib's main-context-local spawn which avoids the Send
  requirement entirely.

This closes the two bugs from the v0.6.14 screenshot. The
soft-accent + screenshot-cleanup arc is still done; this is just
the fixture/debug surface catching up.

## v0.6.14 (2026-05-08) — Patch D (reframed): visible row separators + recurrence icon

The original Patch D was "day-band grouping in the main task list."
Walking through the implementation surfaced a scope problem: the
Today list is a single-day view by definition (every row would
read "Today"), Logbook already has day-bands (Slice C2), and Agenda
is the explicit "everything across days" view. Day-band grouping
inside Today / Inbox / Anytime would duplicate Agenda; the only
sensible target was Upcoming, which is a single-view scope rather
than a main-list-wide change.

Reframed Patch D as two smaller polish wins that actually address
what the screenshot showed:

- **Visible row separators.** `GtkListView`'s `show-separators=true`
  was on (window.ui) but the default separator on dark themes was
  so faint that 20+ rows read as a wall of text. v0.6.14 adds a
  1px `@borders`-tinted bottom border to each task row (constrained
  by `:has(.atrium-task-row)` so kanban / agenda card rows don't
  inherit). The eye now has a clear stride between rows without the
  list looking like a heavy table.

- **Recurrence icon (#9b).** Tasks whose `repeat_rule` is set now
  show a small `view-refresh-symbolic` icon at the right edge of
  the row, with a tooltip "Repeating task." The icon is a derived
  state cue — the original screenshot bug was the *fixture* shoving
  emoji into title strings (#9a, fixed in Patch A); the icon now
  reads correctness from `repeat_rule` regardless of what the title
  says. New `repeating: bool` glib property on `AtriumTask`,
  computed at construction + on `refresh_from`. The row factory
  appends a `gtk::Image` after the deadline pill (preserves the
  existing `next_sibling` chain so other bind logic stays
  unchanged) and toggles its visibility via
  `connect_repeating_notify`. Handler stashed under
  `atrium-repeating-handler` and disconnected on unbind. The icon
  picks up the row-state tint when the task is overdue or today,
  matching the date-pill pattern from Patch B.

This closes the four-patch screenshot-cleanup arc:
- v0.6.11 Patch A — eight quick wins (eight files, low risk).
- v0.6.12 Patch B — state-aware row treatment (the biggest visual
  win; overdue red / today amber / upcoming accent).
- v0.6.13 Patch C — Inspector Notes placeholder.
- v0.6.14 Patch D — visible row separators + recurrence icon.

## v0.6.13 (2026-05-08) — Patch C: Inspector Notes placeholder

Small focused patch off the screenshot-cleanup arc. The Inspector
pane's Notes field used to be a blank dark rectangle — first-run
users had no way to know it was editable. v0.6.13 adds a
placeholder hint that disappears the moment the user types.

GtkTextView doesn't have a native placeholder property the way
GtkEntry does, so the implementation is the standard GTK4 idiom:
overlay a `GtkLabel` (set to `set_can_target(false)` so clicks
pass through to the underlying TextView) inside a `GtkOverlay`
that wraps the TextView. The label's visibility tracks the
buffer's character count — visible when zero, hidden otherwise —
via `connect_changed`. The TextBuffer's autosave-on-focus-out
behaviour is unchanged.

Placeholder text reads "What / why / next step — autosaves on
focus-out" so users who haven't read the docs (most of them, most
of the time) understand both *what kind of content* belongs in
the field and *when their input will be saved*.

The recurrence icon piece originally bundled with this patch
(#9b — derive an icon from `repeat_rule`) was deferred — issue
#9 was really about the fixture's emoji-prefixed titles, which
Patch A already fixed. The derived recurrence icon is a polish
"would be nice" rather than a screenshot-bug, so it can wait
for a real use case to push it.

Patch D (day-band grouping in the main task list — Today /
Tomorrow / This Week / Later headers between rows) is the last
one in the four-patch arc.

## v0.6.12 (2026-05-08) — Patch B: state-aware row treatment

The biggest visual win in the screenshot-cleanup arc. Each row now
classifies into one of three states based on its dates + completion
state, and the leading checkbox + the right-hand schedule / deadline
pills tint accordingly. The eye picks up "needs attention" without
reading the dates.

States (mirrors the in-memory evaluator + agenda classify rules):

- **Overdue** — open AND deadline < today. Strong red on checkbox
  + deadline pill. The eye doesn't get to look anywhere else.
- **Today** — open AND most-imminent date == today (where
  most-imminent = `min(scheduled_for, deadline)`). Warm amber.
  "What you said you'd do today."
- **Upcoming** — open AND most-imminent date > today. Theme accent
  (blue by default) at lower alpha so the cue reads as quiet "on
  the way" rather than competing with the urgent states above.
- **Neutral** — no time anchor, completed, or scheduled-someday.
  No special tint; rows look as they did pre-v0.6.12.

Completed tasks (the existing `.completed` class) override the
state tints — a finished task should read as settled regardless
of when its deadline used to be.

What's in the patch:

- **`atrium/src/ui/task_object.rs`.** New `row_state` glib property on `AtriumTask` (`""` / `"overdue"` / `"today"` / `"upcoming"`). New `classify_row_state(&Task) -> String` function that walks the same rules `agenda::classify` uses. Both `from_task_with_tags` and `refresh_from` call it so the property updates on every worker delta — a task whose deadline rolls past today flips state on the next refresh.
- **`atrium/src/ui/task_list.rs`.** Row factory `bind` adds the matching CSS class on initial bind, then a `connect_row_state_notify` keeps it in sync as the property mutates. Three classes (`atrium-task-row-overdue` / `atrium-task-row-today` / `atrium-task-row-upcoming`) are mutually exclusive — the factory drops all three before adding the current one. Handler stashed under `atrium-row-state-handler` and disconnected on unbind.
- **`data/style.css`.** Three CSS rules per state, targeting `checkbutton check` (the GtkCheckButton's checkmark) for the leading colour cue and `.atrium-task-deadline` / `.atrium-task-schedule` for the date-pill colour. A fourth rule resets the colours when the row also has `.completed` so the strike-through treatment isn't fighting the state colour.

Patch C (Notes placeholder + derived recurrence icon) and Patch D
(day-band grouping in the main task list) follow.

## v0.6.11 (2026-05-08) — screenshot-issue cleanup, Patch A (eight quick wins)

First patch off the screenshot-driven issue list logged in v0.6.10.
Eight tightly-scoped low-risk fixes that ship together because each
touches one file and the visual benefit is immediate. The harder
items (state-aware row treatment, Notes placeholder, day-band
grouping) follow in their own patches.

- **Inspector "Defer until: Available now" → "Not deferred."** "Available now" read as a status (every undeferred task is "available now"), not the date-shaped fact the row promises. The new copy treats the absence of a defer date as a date-shaped value.
- **Inspector "Builder" subsection rename.** The pane only renders in Builder Mode, so the "Fields exposed only in Builder Mode" subtitle was redundant noise. Title now reads *Schedule depth*; subtitle dropped.
- **"Inbox" project chip suppressed on the Inbox view.** Every row on that view is in Inbox by definition; the chip just duplicated what the page header said.
- **Window title reflects the active view** — `Atrium · Today` / `Atrium · Inbox` / `Atrium · Q3 plans`. The window-level title shows in window managers, alt-tab overlays, and screencast picker UIs; the bare `Atrium` was a brand sticker, not a context cue.
- **Fixture areas get colours from the six-swatch palette.** Per-area accent stripes (Slice B2, v0.5.0) were invisible in `--fixture small` because no fixture area had a colour set. Now they cycle through the palette, demonstrating the feature without manual setup.
- **Fixture tags get colours from the same palette** (staggered by one entry from areas). Pango-coloured tag pills (v0.3.0) had been monotone in screenshots because the fixture left every tag colour-less.
- **Fixture cleanup: drop emoji prefixes** on `Buy {item}` / `Reminder: …` titles. Those characters were title text masquerading as derived state; a real "this is a recurring reminder" cue should come from `repeat_rule`, not a literal emoji in the title. (The derived recurrence-icon bit lands in Patch C.)
- **`AdwClamp` max-content-size 720 → 960.** Slice B1's 720 px cap left a visible dead zone on wide windows when the inspector pane was visible flush-right (sidebar + main + inspector + the centered clamp's gap). 960 reclaims that space without losing the paper-list calm.

This is one focused commit per the four-patch screenshot-cleanup plan logged in v0.6.10. Patch B is state-aware row treatment (overdue red, today amber, upcoming accent), Patch C is the Notes placeholder + recurrence icon, Patch D is day-band grouping in the main task list.

## v0.6.10 (2026-05-08) — soft-accent pass: warmth without obnoxiousness

The default Adwaita dark theme reads as a uniform grey wall when an
app fills it edge-to-edge with content. v0.6.10 layers a thin
accent-warmth pass across six surfaces — barely perceptible per
rule, additive across the window — so the eye picks up structure
without any single surface screaming. Everything uses libadwaita's
named colour tokens (`@accent_color`, `@warning_color`,
`@success_color`, etc.), so light / dark / high-contrast themes
stay in lockstep.

What got tinted:

- **Sidebar background.** A diagonal accent-color gradient at 2.5%
  → 0% alpha. Almost invisible on its own, but it gives the
  sidebar a subtle directional cue that separates it from the
  main content without a hard divider.
- **Header bars.** Whisper of accent on the leading 35% (4% alpha
  fading to 0). The bar is otherwise a uniform grey strap; this
  hints at the accent without covering any controls.
- **Page title in the header bar.** "Today", "Inbox", "Agenda",
  etc. now render at weight 600 with a hair of letter-spacing.
  The page identity reads as a *headline* rather than just a
  label.
- **Sidebar count badges.** Those "131 / 75 / 178" numbers next to
  Inbox / Today / Upcoming are no longer plain grey — each picks
  up its row's canonical accent (Inbox → blue, Today → yellow,
  Upcoming → green, etc.) at the same alpha as the icon tint, so
  badge and icon read as a kindred set.
- **Sidebar section headers.** "AREAS" / "PERSPECTIVES" / "TAGS"
  pick up a hint of `@accent_color` so they nudge away from pure
  grey toward the accent's hue.
- **Sidebar selection.** Selected row uses a softer accent-tinged
  background (12% alpha) instead of the system's stark selected
  state. The canonical icon tint stays readable when the row is
  selected.
- **Inspector pane group headings.** "Title" / "Schedule" / "Tags"
  / "Notes" / "Builder" pick up an accent-warmth tint so the
  inspector reads as a curated detail panel rather than a cold
  form.
- **Task row hover.** Replaces v0.6.6's instant grey hover with an
  instant accent-tinged hover. Same speed (no transition — drag
  motion stays cheap), warmer hue.

This is a CSS-only patch. No code changes, no schema changes, no
tests touched. The "Brandon ran v0.6.9 and surfaced two warnings"
flow from the previous patch is unchanged — log is still quiet.

What's *not* in this patch (called out in the screenshot
analysis but deferred to follow-up patches):

- State-aware status circles (red for overdue, amber for today,
  etc.) — needs a code-side CSS class per row state.
- State-aware date column (the "May 1" / "May 2" text picking up
  red on past-due, accent on today). Same shape — code-side
  per-row class.
- Inspector "Defer until: Available now" rephrasing — the value
  reads as a status, not a date.
- "Inbox" project chip on no-project tasks — duplicates the
  canonical-list selection signal.
- AdwClamp-induced dead zone on wide windows — the inspector
  pane lives flush against the right edge while the main task
  column is centered with empty space on either side.

## v0.6.9 (2026-05-08) — fix two startup-log warnings

Brandon ran the v0.6.8 binary and surfaced two real warnings in
the log that were going unnoticed in CI:

- **CSS theme parser error at `style.css:488`.** A no-op
  placeholder rule from v0.6.1 used `:not(:last-child)::after`,
  which GTK4's CSS doesn't recognise (`:not()` and pseudo-element
  combinators differ from browser CSS). The rule never rendered
  anything anyway — replaced with a one-line comment explaining
  that visual separation between metadata segments comes from
  the parent box's spacing, not a pseudo-element.

- **Search bar warning on every keystroke.** GTK was emitting
  *"The search bar does not have an entry connected to it. Call
  `gtk_search_bar_connect_entry()` to connect one."* on every
  captured key event. The fix is a one-liner — `bar.connect_entry(&entry)`
  in `wire_search_bar`. This had been missing because the entry
  lives inside a wrapper Box (so the `?` help button can sit
  alongside it), and `GtkSearchBar` only auto-discovers an entry
  that's a direct child. Without the explicit connection, the
  bar's `key-capture-widget=task_list_view` had nowhere to route
  forwarded keystrokes — they fell through and the warning fired.

Both fixes are surgical and surfaced no other warnings in the
log Brandon shared.

## v0.6.8 (2026-05-08) — v0.6.x cleanup pass: docs catch-up + small code hygiene

End-of-session maintenance pass. Eleven v0.6.x releases shipped
since the v0.5.0 line (atrium-cli runtime fix → broken-pipe fix →
FTS5 bm25 → SQL-translation evaluator → Slice D foundation →
kanban GUI → kanban polish → renderer-config dialog → drag-drop →
Agenda canonical page → atrium-cli perspective write side →
kanban CPU mitigation → sidebar top-tier reorg). The contract
docs (`spec.md`, `roadmap.md`, `README.md`) lagged behind the
patches; this release brings them back into alignment per the
"Spec discipline" rule in `CLAUDE.md`.

What's in the patch:

- **`spec.md`** — version header bumped from 0.5.0 to 0.6.7 with a one-line summary of what 0.6.x delivered. Three new sections added without renumbering the existing tail: §4.4 (FTS5) gains a "Bm25 + recency ranking" subsection documenting the saturating-relevance + half-life math; §4.5 (SQL-translation evaluator) describes the all-or-nothing translation rule, the parity-test backstop, and the current coverage / fall-back set; §4.6 (Perspective renderers) documents the `'list'` / `'board'` axis and the Slice D locked rules (leftmost-match-wins, "Other" trailing column, case-insensitive matching, `move_to_column` drag-rewrite). The original §4.5 (Migrations) renumbers to §4.7. §5.2 (Builder Mode) gains a description of the kanban board renderer; new "Mode-agnostic additions" subsection covers Agenda + the v0.6.7 sidebar reorganisation.
- **`roadmap.md`** — Phase 15.75 rewritten to reflect what actually shipped. All seven previously-deferred items are now `[x]`-checked with their landing versions (Slice C v0.5.0 → v0.6.0, Slice D v0.5.4 → v0.6.5, FTS5 bm25 v0.5.2, SQL pushdown v0.5.3, sidebar reorg v0.6.7, CLI bulk operations v0.4.6, regression-script integration v0.5.x). Each line traces the actual code paths so the roadmap reads as a "what shipped where" map rather than a planning document.
- **`README.md`** — landing paragraph extended with a v0.6.x summary covering Slice D, FTS5 bm25, the SQL-translation evaluator, and the sidebar reorg. The detailed feature surface in the lower sections still describes v0.5.0 capabilities accurately, so a full README rewrite isn't due until the next major.
- **Code hygiene.** `print_perspective_after_write` had a dead `&Connection` parameter (introduced when refactoring perspective output); dropped it and the now-unused parameter through `run_perspective_create`. Two stale "Phase X will" promise comments updated — the SQL-translation comment in `window.rs::refresh_active_list` no longer claims "Stage 3 will add" (Stage 3 shipped at v0.5.3), and `task_list::ActiveList::task_matches`'s old "Phase 5c will revisit" promise is now an accurate description of the current behaviour.
- **Workspace clippy clean.** `cargo clippy --workspace --all-targets -- -D warnings` reports zero warnings.
- **Regression-script ship gate green at v0.6.8.**

What's *not* in this patch (deliberately deferred — these are larger surgeries that warrant their own changes):

- `atrium/src/ui/window.rs` is at ~5000 lines. A `ui::sidebar` extraction is the obvious next refactor target; the composite-template wiring couples a lot to it though, so it's a careful surgery not a quick cleanup.
- The list-renderer Perspective path in `refresh_active_list` doesn't yet use the SQL fast-path (only the board path does, as of v0.6.6). Adding it is the same shape but the sort-spec / bm25 plumbing needs to align.

## v0.6.7 (2026-05-08) — sidebar reorganisation: Agenda / Forecast / Review join the top tier

The "Builder" sidebar header is gone. Agenda / Forecast / Review
no longer hide at the bottom of the sidebar in Builder mode — they
now sit in the top tier alongside Inbox / Today / Upcoming /
Anytime / Someday / Logbook, with their own accent tints:

- **Agenda** appears in *both* Simple and Builder modes (the
  agenda is a pure read view with no Builder-only concepts;
  it makes sense to surface it everywhere). Accent: warning
  red on the alarm-clock icon, so urgency reads at a glance.
- **Forecast** + **Review** stay Builder-only but join the top
  tier in that mode. Accents: cool blue (calendar) and success
  green (checkmark).

Perspectives section moves up from the bottom of the sidebar to
right under the top-tier group — above Areas, below "the Inbox
grouping," exactly as the user wanted.

Final sidebar order:

- **Both modes:** Inbox, Today, Upcoming, Anytime, Someday,
  Logbook, Agenda
- **Builder mode adds:** Forecast, Review (still in the top
  tier), then a "Perspectives" section header + its rows
  underneath
- **Both modes continue with:** Areas (and nested projects),
  Unfiled projects, Tags

What's in the patch:

- **`atrium/src/ui/window.rs`.** New `top_tier_extras(builder)` helper returns the post-canonical rows that should appear in the current mode. `rebuild_dynamic_sidebar` now appends those rows + the Perspectives section *before* Areas, instead of the old "Builder" section header at the bottom. `canonical_accent_class` extended to cover Agenda / Forecast / Review.
- **`data/style.css`.** Three new accent rules (`.atrium-canonical-agenda` → `@warning_color`, `.atrium-canonical-forecast` → `@accent_color`, `.atrium-canonical-review` → `@success_color`). Same alpha treatment the canonical rows already use, so they sit alongside without screaming.
- **Three new unit tests** pin the top-tier shape (Simple = just Agenda; Builder = Agenda + Forecast + Review in that order) and the accent-class wiring so a future tweak can't quietly drop the tints.

## v0.6.6 (2026-05-08) — kanban drag-drop CPU mitigation

Two targeted optimisations to address the CPU spike Brandon
reported during kanban drag operations:

- **Drop the hover transition on board / agenda task rows.**
  v0.6.1 added a `transition: background-color 120ms ease-out`
  on `.atrium-board-task-row` (and Agenda inherited the same
  pattern). During a drag, the cursor crosses many rows in
  succession; each crossing fired a 120ms CSS animation
  producing continuous repaint work and a visible CPU spike.
  The hover background still applies — it's just instant now,
  so there's no per-frame paint cost.

- **SQL fast-path on board refresh.** v0.5.3 added the SQL
  translation evaluator to atrium-cli; v0.6.6 wires it into
  the GUI's `refresh_board_page`. When the perspective's
  filter expression translates cleanly to SQL (most do — the
  fixture's `is:open` does), we now load only the matching
  task rows from SQLite instead of pulling every row and
  filtering in Rust on every drop. At 1000-task scale that
  cuts the per-drop work meaningfully; at 10K+ it'll
  dominate. Falls back to the in-memory evaluator for
  expressions the translator doesn't yet cover (regex /
  fuzzy / composite is:today / etc.).

What's also in the patch:

- **`atrium_core::SqlBindValue` enum.** Pulled the binding
  conversion out of atrium-cli's local helper and into a
  proper public type on atrium-core. The atrium GUI binary
  now bridges to it without needing a direct rusqlite dep.
  `From<atrium_search::SqlValue> for atrium_core::SqlBindValue`
  lives in atrium-search so call sites just say `.into()`.
- **`filter::sort_tasks_by_specs`.** Tiny re-export of the
  sort-spec helper so the SQL fast-path in window.rs can
  apply explicit `sort:` modifiers without re-running the
  full `filter::apply` pipeline.

If the CPU spike persists after this patch, the next move is
either (a) profile with `tracing` spans around the rebuild
to find the dominant cost, or (b) coalesce/debounce
TaskChanges-driven refreshes so rapid drops only trigger one
rebuild at the end. Both are clean follow-ups for a fresh
session.

## v0.6.5 (2026-05-08) — atrium-cli perspective write side

Closes the gap that the only way to create or convert a saved
perspective from the shell was via direct SQL. Three new sub-
subcommands under `atrium-cli perspective`:

```bash
# Create a list-renderer perspective.
atrium-cli perspective create 'Q3 plans' --filter 'project:"Q3 plans"'

# Convert it to a kanban board.
atrium-cli perspective edit 'Q3 plans' --renderer board \
  --columns 'todo,doing,done'

# Update the column list in place (renderer stays as board).
atrium-cli perspective edit 'Q3 plans' --columns 'backlog,todo,doing,done'

# Rename + re-icon + retune the filter in one shot.
atrium-cli perspective edit 'Q3 plans' \
  --rename 'Q3 plans (rev 2)' \
  --icon view-grid-symbolic \
  --filter 'project:"Q3 plans" AND is:open'

# Back to a flat list.
atrium-cli perspective edit 'Q3 plans (rev 2)' --renderer list

# Tear it down.
atrium-cli perspective delete 'Q3 plans (rev 2)'
```

Locked semantics:
- **Name lookup is case-insensitive exact** for write paths
  (edit / delete) — substring fallback would risk editing the
  wrong perspective on a typo. Read-only `kanban NAME` keeps
  its substring fallback because there's no such risk.
- **`--renderer board` requires `--columns`** on create. On edit,
  `--columns` alone is allowed *if the perspective is already a
  board* — that's the in-place column-list update.
- **`--icon none`** clears the icon (back to the default); a
  bare value sets it.
- **`perspective edit` with no flags is a noop** — prints the
  existing row so the user gets a confirmation that they
  matched the right name.

What's in the patch:

- **`atrium-cli/src/args.rs`.** New `Subcommand::Perspective(PerspectiveSub)`; new `PerspectiveSub` enum (Create / Edit / Delete) and `PerspectiveArgs` flag bundle; new `EditIcon` tri-state for the `--icon` flag; new `parse_perspective` body parser that supports multi-word names + the full flag vocabulary. USAGE help text extended with the new shape.
- **`atrium-cli/src/main.rs`.** New `run_perspective` dispatcher + `run_perspective_create` / `run_perspective_edit` / `run_perspective_delete` handlers. Helper functions `build_renderer_config`, `synthesise_renderer_for_edit`, `parse_columns`, `resolve_perspective_exact` keep the renderer/columns logic in one place.
- **13 argv-parsing tests.** Cover create-minimum, missing --filter, board+columns, --rename rejection on create, invalid renderer, edit-with-all-flags, --icon none, edit-noop, delete-name-only, delete-rejects-body-flags, unknown sub, no-sub, multi-word names.
- **Regression-script smoke (step 5.5).** Now exercises the full create → edit (convert to board) → edit (update columns) → edit (back to list) → delete round-trip plus a `perspective edit … (no flags)` noop and a `--json list perspectives` post-condition assertion.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to 0.6.5.

## v0.6.4 (2026-05-08) — Slice D2: Agenda canonical page

Org-mode-style "everything you should think about right now" view.
A new canonical page (sidebar entry next to Forecast / Review) that
groups open tasks into five chronological sections:

- **Overdue** — open AND `deadline < today`. Surfaces past-due
  work first so it isn't buried under future scheduling.
  Heading is rendered in red to flag urgency at a glance.
- **Today** — most-imminent date == today. "Most-imminent" is
  `min(scheduled_for, deadline)`. Same rule the regular Today
  list uses, plus deadline-today.
- **Tomorrow** — most-imminent == today + 1.
- **This Week** — most-imminent within the rest of the current
  ISO Mon-start week (after Tomorrow). Empty on Sunday.
- **Next Week** — most-imminent within next ISO Mon-start week.
- Tasks farther out live in Forecast; tasks without a time
  anchor (no scheduled, no deadline) don't appear; completed
  and deferred-future tasks don't appear.

Each section is an Adwaita card with a heading + count and a
vertical task list. Rows show title + date chip + project name
+ tag pills. Click any row → opens in the Inspector. Empty
agenda gets an `AdwStatusPage` "Nothing on the agenda" banner.

What's in the patch:

- **`atrium/src/ui/agenda.rs`.** New module. `AgendaSection` enum, `classify(task, today)` (returns `None` when not on agenda), `group_by_section(tasks, today)` returning `Vec<(AgendaSection, Vec<Task>)>` in canonical order, `build_page(today, tasks, …)` returning the GTK widget. **14 unit tests** covering the classification rules: completed-skip, deferred-future-skip, no-anchor-skip, someday-skip, overdue precedence, scheduled-today / deadline-today / scheduled-tomorrow, this-week / next-week boundaries, beyond-next-week-skip, most-imminent-wins-when-both-dates-set, group_by_section ordering and filtering.
- **`ActiveList::Agenda` variant.** Added to `task_list::ActiveList`; matched everywhere ActiveList is exhaustive.
- **Sidebar entry.** Builder-mode sidebar gains an "Agenda" row between Forecast and Review (same group, same shape).
- **`refresh_agenda_page` + content stack page.** `data/window.ui` adds an `agenda_host` AdwBin in a new GtkStackPage `"agenda"`; `refresh_active_list` and `apply_task_changes` route `ActiveList::Agenda` through it.
- **CSS.** `.atrium-agenda-section` + `.atrium-agenda-overdue` (heading turns red) + `.atrium-agenda-row-meta` styling so the agenda reads as a focused composite view rather than another flat list.

The agenda is currently Builder-only (matches the pattern Forecast / Review / Perspectives use). A future polish pass could surface it in Simple Mode too — the underlying data is mode-agnostic.

## v0.6.3 (2026-05-08) — kanban drag-drop between columns

The kanban is no longer read-only. Drag a task row to a different
column → the task's tag set is rewritten so the kanban grouper
buckets it under the new column on the next refresh:

- The leftmost configured-column tag in the task's current set
  is removed (that was the source column).
- The destination column's tag is added if not already present.
- Non-column tags pass through unchanged.
- Dropping on the trailing "Other" column just removes the
  source column tag — the task lands in Other for not matching
  any configured column.

The tag-set-rewrite logic is `atrium_core::move_to_column` —
pure-Rust, no GUI dependencies, eight unit tests cover the
combinatorial cases (move-to-column, move-to-other, move-to-same,
non-column passthrough, no-source, case-insensitive,
no-duplicate-on-existing, leftmost-only-removal).

The GUI side is plain GTK4 DnD: each row registers a
`gtk::DragSource` carrying the task id, each column card a
`gtk::DropTarget` accepting `i64`. The drop callback walks the
task's current tag names through `move_to_column`, then
dispatches `worker.ensure_tag` for each new name and
`worker.set_task_tags` to install the result. No-op short-circuit
when the new tag list is set-equal (case-insensitive) to the old
one — covers the common "drop on the same column" case without a
worker round-trip.

## v0.6.2 (2026-05-08) — perspective renderer-config dialog

Closes the v0.6.0 gap that the only way to make a Perspective
render as a kanban was direct SQL or the test fixture. Right-
clicking a Perspective row in the sidebar now exposes a
"Configure renderer…" item that opens an `AdwAlertDialog`:

- Two radio toggles: **List** (default flat task list) /
  **Board** (kanban columns).
- When Board is selected, a comma-separated entry takes the
  column list — pre-populated with the existing columns when
  editing an already-configured board.
- Save → writes `perspective.renderer` and
  `perspective.renderer_config` via the worker.
  `apply_library_changes` re-renders the active perspective
  immediately, so the column layout appears without needing
  a sidebar refresh.

What's also in the patch:

- **`BoardConfig::to_json` / `BoardConfig::from_json`.** The
  GUI dialog uses these to round-trip the JSON shape without
  pulling `serde_json` into the GTK binary. Pinned by two
  unit tests — one for the round-trip, one for the exact
  emitted shape so a future serde derive tweak can't silently
  rename the JSON keys.

The CLI doesn't yet have a board-renderer setter (the v0.5.4
`atrium-cli kanban NAME` only renders an *existing* board). A
sibling patch will add `atrium-cli perspective …` for the
write side; for now, perspective creation/config from the
shell is "edit the DB directly or use the GUI dialog."

## v0.6.1 (2026-05-08) — kanban polish: row metadata + interactive checkbox

The first polish pass on the v0.6.0 kanban. Two gaps closed:

- **Row metadata line.** Project name, the most-relevant date
  (deadline trumps scheduled; Someday renders as the literal
  "Someday"), and tag pills (using the same Pango-coloured
  markup the regular task list uses) now appear under the title
  when any of them are set. Tasks with no metadata stay tight —
  the metadata row is suppressed entirely rather than rendering
  empty.
- **Interactive checkbox.** Clicking the checkbox toggles the
  task's completion via the worker, same as the regular list
  view. The board re-renders on the next `apply_task_changes`
  delta. Previously the checkbox was render-only.

Drag-drop between columns and a board-renderer editing UI are
still the next slices.

## v0.6.0 (2026-05-08) — Slice D1 GUI (read-only kanban board page)

The first GUI consumer for the v0.5.0 `perspective.renderer` /
`renderer_config` columns. A saved Perspective whose `renderer =
"board"` now renders as a horizontal column layout in the GTK
binary instead of a flat list. Each column is a tag — leftmost
match wins, "Other" trailing column for tasks that don't match
any configured column. Same engine the v0.5.4 `atrium-cli kanban`
subcommand uses (`atrium_core::render::group_into_board`).

What's interactive in v0.6.0:

- Click any task row → opens it in the Inspector (same
  `win.edit-details-for(i64)` action the regular list and
  keyboard shortcuts go through).
- Vertical scrolling per column for tall task lists.
- Horizontal scrolling across the whole board when the column
  set exceeds the viewport.

What's *not* interactive yet (deferred to a follow-up patch):

- Drag-drop between columns. Today, moving a task between
  columns is "edit the task's tags from the Inspector or via
  `atrium-cli edit ID --tag X --remove-tag Y`."
- The completion checkbox renders the state but isn't
  click-toggleable from the board view (use the regular task
  list or the Inspector).
- No board-renderer editing UI yet — to convert a Perspective
  to a board, edit `renderer` and `renderer_config` directly.
  An editing dialog ships in a future slice.

What's in the commit:

- **`atrium/src/ui/board.rs`.** New module. `build_page(name, columns, on_row_click)` returns a horizontally-scrolling `gtk::Box` with one card-styled column per `Column<'_>`. Per-column scrolling caps at 420px tall; per-row click activates the inspector via the supplied callback.
- **`data/window.ui`.** New `GtkStackPage` named `"board"` with an `AdwBin id="board_host"` host, mirroring the forecast/review/logbook pattern.
- **`atrium/src/ui/window.rs`.** Window struct gains a `board_host` template child. New `refresh_board_page(perspective)` method orchestrates load → filter → bm25 rank → group → mount. The `ActiveList::Perspective(id)` branch in the active-list refresh checks the perspective's renderer; `"board"` switches to the board stack page, anything else falls through to the existing list rendering.
- **`data/style.css`.** Adwaita-`card`-class kanban columns, subtle hover tint on rows, transparent scroller backgrounds so the board reads as one surface rather than nested boxes.

VERSION / Cargo.toml / patchnotes / AppStream metainfo bump to 0.6.0.

## v0.5.4 (2026-05-08) — Slice D1 foundation (kanban renderer + atrium-cli)

The first slice of Slice D — saved Perspectives can now render as
kanban boards. v0.5.4 ships the *headless* foundation: parser,
grouping engine, and a complete CLI consumer; v0.6.0 will land the
GUI rendering on top of these pieces.

The kanban contract is small and opinionated:

- **Schema reused.** `perspective.renderer = "board"` plus
  `perspective.renderer_config = '{"axis":"tag","columns":["…"]}'`.
  These columns shipped at v0.5.0 (Slice A); this is what they're
  *for*.
- **Leftmost match wins.** A task with multiple matching tags
  appears in only the leftmost matching column. Kanban is a state
  view — a task is in *one* state at a time.
- **"Other" trailing column.** Tasks that don't match any
  configured column always appear in a final `"Other"` bucket so
  the kanban stays honest about coverage. Users who want a
  tighter view tighten the perspective filter (e.g.,
  `is:open AND tag:true`).
- **Case-insensitive tag matching.** Mirrors the rest of the
  search-engine tag rules.

What landed:

- **`atrium-core::render` module.** New file. `Renderer::from_columns(renderer, config_json)` parses the `(renderer, renderer_config)` pair into a typed `Renderer` enum. `group_into_board(tasks, &cfg, &tag_names_per_task)` walks a task list and emits one `Column<'_>` per configured column plus the trailing `Other`. 17 unit tests cover parsing rejection (unknown axis, blank columns, missing config, unknown kind), grouping rules (untagged → Other, leftmost-wins, case-insensitive, input-order preservation, empty input).
- **`atrium-cli kanban NAME`.** New subcommand. Resolves a perspective by case-insensitive name (exact first, substring fallback), parses its renderer_config, runs the perspective's filter expression through the v0.5.3 SQL fast-path / in-memory eval, groups by tag, and prints columns. TSV / JSON / `--human` formats. Errors clearly when the perspective is missing or its renderer is `"list"` instead of `"board"`.
- **Fixture board perspective.** `--fixture small` seeds a `"Fixture Board"` perspective with three tag columns (`tag-0`, `urgent-3`, `home-4`) so the kanban subcommand has something to render in test contexts and the CLI smoke step can exercise it without seeding a perspective by hand.
- **Regression-script kanban smoke.** `scripts/regression.sh` step 5.5 now exercises `atrium-cli kanban Fixture Board` in TSV / JSON / human formats plus the negative case (`atrium-cli kanban Weekly Review` must error with `"is a list, not a board"` since the seeded Weekly Review is a list-renderer perspective).

The GUI rendering of board perspectives — switching from a flat list to a horizontal column layout, drag-drop between columns rewriting the underlying tag — lands in v0.6.0. The agenda/overview view (Slice D2) follows.

## v0.5.3 (2026-05-08) — SQL-translation evaluator (atrium-cli)

The fourth v0.6.x carryover. The Calibre-style search expression
language now executes at the SQLite layer instead of pulling every
row into memory and filtering in Rust — for queries that translate
cleanly. The translator's "all-or-nothing" rule keeps semantics
unchanged: anything that can't be expressed in SQL (regex match
modifiers, fuzzy matches, sequential-project state, the composite
`is:today` family) falls back to the in-memory evaluator. The two
paths are pinned to identical behaviour by 21 parity integration
tests in atrium-cli.

The win matters most at the 100K-task scale (spec §8 perf budget).
A search that previously loaded 100K rows + iterated them in Rust
now lets SQLite's query planner do the work using its existing
indexes. Wired into atrium-cli for v1; the GUI search-bar +
saved-Perspective wiring follows in a sibling patch.

- **`atrium-search::sql_translate`.** New module. `try_translate(&Expr, today) -> Option<SqlClause>` walks the parsed AST and emits a SQL `WHERE` fragment + parameter list when every node maps cleanly to SQL. Returns `None` for any subtree containing `MatchKind::Regex`, `MatchKind::Fuzzy`, `State::Available`/`Queued`, `State::Today`/`Inbox`/`Upcoming`/`Anytime`/`Someday` (composite list-membership), `State::InArea`/`Archived`, `Field::Project`/`Area` (deferred — would need joins), or any unsupported `Field`/`MatchKind` combination. 21 unit tests.
- **`atrium-search::dates`.** Extracted from `eval.rs` so the SQL translator and the in-memory evaluator share the same date-keyword arithmetic (`today`, `thisweek`, `5daysago`, …). Single source of truth — no drift possible between paths.
- **`atrium-core::db::read::list_tasks_matching`.** New helper that runs a pre-built SQL `WHERE` fragment + bound params against the `task` table and decodes the resulting rows. Plain `prepare` (not `prepare_cached`) since the WHERE clause varies per query — caching would unboundedly grow the per-connection statement cache.
- **`atrium-cli::filtered_tasks`.** New private helper consumed by `run_search` and `resolve_matching_tasks`. Calls `try_translate` first; on `Some`, executes via `list_tasks_matching`; on `None`, falls back to the existing `list_all_tasks` + in-memory `evaluate` path. Same input expression → same task ID set on both paths.
- **Parity tests.** 21 cross-validation tests in `atrium-cli/src/tests.rs::sql_parity` seed a small mixed-shape fixture (open + done + overdue + scheduled + deferred + repeating + tagged tasks), run a battery of expressions through both paths, and assert identical id sets. Includes negative tests confirming `try_translate` correctly rejects regex / fuzzy / `is:today`.

## v0.5.2 (2026-05-08) — FTS5 bm25 + recency ranking on bare-text searches

The third v0.6.x carryover off the deferred list. Bare-text searches
(`atrium-cli search milk`, the GUI search bar with a freeform word)
now rank by FTS5's `bm25` blended with a 30-day half-life recency
factor. Stronger matches and freshly-touched tasks rise to the top
instead of every result coming back in `task.position` order.

- **`atrium-search::rank` module.** Two pure helpers — `collect_text_terms` walks the parsed AST for `Expr::Text` nodes, `blend_relevance` maps `bm25` + `days_since_modified` → a single comparable score on a stable scale. Twelve unit tests cover the math (saturating relevance, recency half-life, clamped negative days, AND/OR/NOT walking, field-scoped exclusion).
- **`atrium-core::db::read::bm25_for_terms`.** Queries FTS5 with the term set unioned via `OR`, returns `HashMap<task_id, bm25>` for the matching rows. User input is double-quote-stripped + phrase-quoted so a stray `"` can't inject MATCH operators. Six tests cover the empty / blank / quote-injection edge cases plus a term-frequency rank check.
- **CLI wiring (`atrium-cli`).** `run_search` calls the rank helper after the in-memory evaluator, only when the query has bare text and no explicit `sort:` modifier. Skipped automatically when `sort:` is present so power users keep their explicit ordering.
- **GUI wiring (`atrium/ui/filter::rank_by_bm25_recency` + window.rs).** Same fast-path applied to both the search-bar's transient SearchResults list and saved Perspectives whose filter contains bare text. Four window-side unit tests cover the no-op / strong-match / recency-tiebreak / unscored-fallback cases.
- **No new dependencies.** Sits on the existing FTS5 `task_fts` virtual table that's been in place since migration `0001_initial.sql`.

## v0.5.1 (2026-05-08) — atrium-cli runtime fix + ship-gate smoke + broken-pipe fix

A focused patch with three small, coupled fixes that the v0.5.0 ship-gate hadn't been wide enough to catch.

- **atrium-cli runtime nesting fix.** `with_writer` previously called `Handle::current().block_on(...)` from inside an outer `runtime.block_on(...)`, which is a "Cannot start a runtime from within a runtime" panic the moment any write subcommand ran. Reshaped to spawn the worker inside `block_on` and exit, then pass `&Runtime` to each `run_X` so subsequent `block_on`s run outside async context. The worker future stays alive on the runtime; each `handle.foo()` awaits a single mpsc round-trip. No behavioural change at the user level — the panic was hit by every write path.
- **Ship-gate end-to-end smoke for atrium-cli.** `scripts/regression.sh` step 5.5 exercises every read subcommand, every search-operator class shipped at v0.5.0, the JSON formatter (now via `head -c 1` to also exercise the broken-pipe path), the add → info → search → edit → complete → delete write round-trip, and the bulk `delete --where` dry-run / `--force` flow. Closes the architectural commitment that every non-GUI surface stays CLI-testable — without this step, the runtime nesting panic would have shipped silently in v0.5.0.
- **Broken-pipe behaviour.** Rust's default-installed SIGPIPE handler is `SIG_IGN`, which means a `println!` to a closed stdout panics on the next write. Atrium-cli now resets SIGPIPE to `SIG_DFL` at startup (inline `unsafe extern fn signal` so we don't add a `libc` dep) — pipes into `head`, `head -c N`, `q`-pressed pagers, etc. now exit cleanly instead of dumping a Rust panic message onto the user's terminal.

## v0.5.0 (2026-05-08) — atrium-cli, search engine evolution, Phase 15.75 visual polish

A meaty minor — this release rolls together fifteen post-v0.4.0 patches into one shippable boundary. Three threads finished and one started:

1. **Phase 15.75 (partial) — visual polish + per-area accent.** Foundation migrations, beauty pass, and per-area colour rendering all landed. The board view (Slice D) and GTD-audit work (Slice C) remain for v0.6.0 / Phase 15.75 finish.
2. **Phase 15.5 deferred-list — closed.** Every search-engine line item the v0.4.0 release punted into "v0.4.x patch" territory shipped: state-predicate coverage, `sort:` modifier, ↑/↓ history, `?` operator-reference popover, fuzzy match, plus the SQL-translation evaluator and FTS5 ranking still pending for a future patch.
3. **Architectural extraction — atrium-search + atrium-cli.** The search engine and a full headless CLI both live as their own workspace crates. The GTK binary is no longer the gatekeeper for the search engine or the data layer.
4. **CLI-testable everything.** Every non-GUI surface is now exercisable from the shell. Foundation for the 2.0-era TUI / atriumd capture daemon.

### Phase 15.75 visual polish

- **Foundation (Slice A).** Two additive migrations — `0004_area_color.sql` (one new column on `area`) and `0005_perspective_renderer.sql` (two new columns on `perspective`: `renderer TEXT NOT NULL DEFAULT 'list'` and `renderer_config TEXT NULL`). Domain types and worker SQL grew alongside; user_version 3 → 5. No UI consumer yet for the perspective renderer columns — that's Slice D's board view, deferred to v0.6.0.
- **Visual rhythm (Slice B1).** `.atrium-task-row:hover` gains a subtle inset bottom border (`@card_shade_color` 1px) plus alpha bump 0.08 → 0.10 for a "lift" cue. `.atrium-sidebar-section` letter-spacing 0.04em → 0.06em — section headers read more clearly as labels. `.atrium-note-body` picks up `font-style: italic` + tighter line-height (1.55 → 1.6); both Inspector surfaces (Simple-mode dialog + Builder-mode pane) now attach the class to their notes TextView so the editable Notes field reads as a writing surface, not a clone of the row chrome. Task list wrapped in an `AdwClamp` (max 720 px) so rows don't stretch into runway on wide windows.
- **Per-area accent (Slice B2).** `prompt_for_tag` generalised to `prompt_for_named_color` with a `placeholder` parameter. Tag callers (3 sites) pass "Tag name"; new area callers (2 sites) pass "Area name". `prompt_create_area` and the Area arm of `prompt_rename_active` now both surface the six-swatch picker. `build_area_row` mirrors `build_tag_row`'s coloured-dot pattern when `area.color` is set. `AtriumTask` gains an `area_color` glib property; `apply_area_accent` toggles the matching `.atrium-area-accent-{color}` CSS class on bind + on every notify so a project move that shifts a task under a differently-coloured area updates the stripe in place. Six new CSS rules paint `border-left-color` at alpha 0.7 on each `.atrium-area-accent-{color}` class. `replace_store_with_tags_seq` + `apply_changes_seq` grow an `area_color_for: G` closure parameter alongside the existing `context_for`; three call sites in `window.rs` pass the new resolver via `build_area_color_resolver`.
- **About-dialog icon resolution.** `typography::register_icon_search_paths` walks three candidate paths (ATRIUM_DATADIR runtime env, compile-time install, `CARGO_MANIFEST_DIR`-relative dev fallback) and registers each existing one with `gtk::IconTheme::for_display`, so AdwAboutDialog's `application_icon(APP_ID)` lookup finds the bundled SVG during `cargo run` development. Installed builds were always fine.
- **Subtle warmth.** Each canonical sidebar list now carries a quiet accent on its leading symbolic icon — Things-3-style. Inbox `@blue_3`, Today `@yellow_5`, Upcoming `@green_4`, Anytime unchanged (intentional neutral beat), Someday `@purple_3`, Logbook `@purple_2` (faded). All wrapped in alpha 0.75–0.95 so accents read as personality, not signage. Also fixed the "cancel symbol" tag icons — `tag-outline-symbolic` isn't in the GNOME standard set; switched to `tag-symbolic`.

### Search engine evolution (Phase 15.5 deferred-list closure)

- **Canonical-list state predicates.** Five new `is:NAME` shortcuts mirroring the canonical sidebar lists per spec §4.2: `is:today`, `is:inbox`, `is:upcoming`, `is:anytime`, `is:someday`. Each pairs with `!is:NAME` for the inverse. Closes the user-mental-model gap that `due:today` (correctly exact-match on Deadline) doesn't surface tasks scheduled for today — `is:today` is the broader Today-list mirror.
- **`sort:` modifier.** `sort:KEY` (ascending) / `sort:-KEY` (descending) with primary → secondary composition. Recognised keys: `due` (alias `deadline`), `scheduled` (alias `when`), `defer`, `created`, `modified`, `completed`, `estimated`, `title`, `position`. NULLs sort last regardless of direction (SQL convention). Implemented as a parser-time AST extraction (the `Expr::Pass` placeholder + `ParseResult.sorts` metadata) so the evaluator never sees a sort modifier as a predicate.
- **Fuzzy `?` modifier.** `tag:?work` matches with Damerau-Levenshtein within a length-aware threshold (≤4 chars → 1, 5–7 → 2, ≥8 → 3). Damerau (vs plain Levenshtein) counts a transposition of adjacent characters as a single edit, so `tag:?wrok` matches `work` — the most common typing slip survives fuzzy without falling back to substring.
- **Search history (↑ / ↓).** 20-entry in-memory ring buffer of recent committed queries. ↑ steps back, ↓ moves toward newer entries; pressing ↓ off the most-recent entry returns to the live entry. Pure-Rust `push_history_entry` + `cycle_history_cursor` helpers keep the state-machine logic out of GTK glue and unit-testable.
- **Operator-reference popover (`?` button).** The search bar grew a `?` GtkMenuButton; clicking opens a structured quick-reference organised by section (Boolean, Fields, Modifiers, Comparison & range, Date keywords, State, Sort). Closes the discoverability gap — without this the search-engine power was invisible to anyone who hadn't read spec §4.3.

### atrium-search workspace crate (v0.4.2)

`atrium-core/src/search/` was lifted into its own sibling workspace crate `atrium-search`. Same code, same tests, no behaviour change — the move means the parser/evaluator can be fuzzed, benchmarked, and reused (atrium-cli + future TUI / atriumd / search server) without dragging the SQLite/worker layer along. atrium-core no longer depends on `regex`. The codebase map in `CLAUDE.md` documents the four-crate workspace.

### atrium-cli — headless data + search access

A whole new headless binary, sibling to the GTK app:

- **Read commands.** `search EXPR` (full search expression language, sort modifiers honoured), `list NAME` (canonical task lists: inbox, today, upcoming, anytime, someday, logbook, all; metadata lists: areas, projects, tags, perspectives), `info ID` (full task detail).
- **Write commands.** `add TITLE [flags]` (full NewTask flag soup with date keywords, project resolution by case-insensitive substring, tag attachment via ensure_tag), `capture LINE` (Quick-Entry-style one-shot capture using the same inline-syntax parser the GUI's bottom-of-list entry uses — lifted from `atrium/src/quickentry/parser.rs` to `atrium-core/src/quick_entry.rs` at v0.4.5), `edit ID [flags]` (diff-based field updates including additive tag flags `--tag X` / `--remove-tag X` / `--clear-tags`), `complete ID` (toggle), `delete ID`.
- **Output formats.** `--tsv` (default — header row + sanitised columns; `cut`/`grep`-friendly), `--json` (serde_json array; `jq`-friendly), `--human` (pretty columns with truncation; for terminal viewing).
- **Database resolution.** `--db PATH` flag → `ATRIUM_DB_PATH` env → XDG default. Read commands open `SQLITE_OPEN_READ_ONLY` so a buggy query attempting an INSERT errors at the engine — no CLI invocation can corrupt the user's database through a read path.

### Numbers

- **362 tests pass total** (89 atrium + 63 atrium-cli + 136 atrium-core + 73 atrium-search + 1 mode-flip integration). Up from 248 at v0.4.0 (+114).
- **Workspace shape:** four crates (`atrium-core`, `atrium-search`, `atrium-cli`, `atrium`).
- **Schema version:** 5 (was 3 at v0.4.0; +0004 area_color, +0005 perspective_renderer).
- **Migrations log:** `0001_initial.sql` (Phase 1) → `0005_perspective_renderer.sql` (v0.5.0 / Phase 15.75 Slice A).

### Spec discipline

- `spec.md` §3.3 Process Topology rewritten to reflect the four-crate workspace + the architectural commitment that every non-GUI surface stays CLI-testable.
- `spec.md` §4.3 search expression language updated with the new operators (state predicates, sort modifier, fuzzy match) and §4.5 migrations log records 0004 + 0005.
- `roadmap.md` Phase 15.75 records partial progress (Slices A + B done; C/D/E pending). Phase 15.5 deferred-list moves to "closed" with the line items shipped at v0.4.x.
- `CLAUDE.md` codebase map shows the four-crate layout and includes atrium-cli's structure.

### Phase 15.75 carryover into v0.6.0

Three slices remain on Phase 15.75's plan:
- **Slice C — GTD audit fixes.** Weekly-Review seed Perspective on first-run; Logbook day-grouping headers (Today / Yesterday / Last 7 Days / Older); `docs/gtd-patterns.md` documenting the `#waiting` user-tag idiom.
- **Slice D — Board view.** Saved Perspectives gain a `renderer = 'board'` option that renders the filter expression as a kanban with tag-axis columns. The schema columns shipped at v0.5.0 (Slice A); UI is Slice D.
- **Slice E — Documentation polish.** Already partly subsumed by this v0.5.0 release notes entry; what remains is the fuller spec / roadmap / patchnotes pass that goes with the next minor.

### Other deferred to v0.6.x

- **SQL-translation evaluator** for the search engine. Translates the AST to a SQL `WHERE` clause when expressible; falls back to in-memory eval for regex / complex tag predicates. Pure perf optimization — the in-memory path handles 100K tasks within budget today.
- **FTS5 bm25 + recency ranking** on bare-text searches. Currently search returns matches unranked.
- **CLI bulk operations.** `atrium-cli complete --where 'is:overdue'` to bulk-complete matched tasks. The pieces are all in place; just needs a flag-driven dispatcher.
- **Regression-script integration.** `scripts/regression.sh` should exercise atrium-cli end-to-end against a fixture DB so the architectural commitment is automatically verified at every release.

## v0.4.0 (2026-05-07) — Phase 15.5: Calibre-Powered Search

The search bar's filter language grew from a flat key:value shape into a full expression grammar. Saved Perspectives inherit it for free since they store filter expressions verbatim. Full reference in `spec.md` §4.3.

Boolean composition with grouping (`AND` / `OR` / `NOT` / `!`, parens, `NOT > AND > OR` precedence). Calibre match modifiers on every text field (`tag:work` substring, `tag:=work` exact, `tag:~regex.*` regex, `tag:true` / `tag:false` existence). Comparison + range on date and numeric fields (`due:>today`, `due:2026-05-01..2026-05-31`, `estimated:>=30`). Date keywords (`today`, `thisweek`, `Ndaysago`, `Ndaysout`, etc.). State predicates as `is:NAME` shortcuts (`is:overdue`, `is:scheduled`, `is:repeating`, etc.). New field operators: `area:`, `project:`, `title:`, `note:`, `created:`, `modified:`, `completed:`, `estimated:`, `repeats:`.

Implementation: new `atrium-core/src/search/` module — lexer (Token stream), AST (Expr enum + supporting types with round-trip-shaped Display impls), recursive-descent parser, single-pass in-memory evaluator with lazy regex compilation cached per-query. `regex` crate added as a direct dependency (sign-off granted; already transitively present via tracing-subscriber).

Yellow `.warning` accent on the search entry when the parsed expression has unrecognised tokens; tooltip surfaces the typos. Three line items deferred to v0.4.x patches: SQL-translation evaluator, `↑/↓` history ring buffer, `?` operator-reference popover — all polish, not correctness.

## v0.3.0 (2026-05-07) — Visual polish pass

Tag colours wired end-to-end (six-swatch picker, sidebar dots, Pango-coloured pills via the existing `markup` property). Row hover states. Completion micro-animation (200 ms fade on toggle). Per-list empty-state warmth — distinct copy per canonical list instead of a generic "Nothing here." Sidebar section dividers. Header-bar `Area › Project` breadcrumb that updates as selection changes. Inspector-pane card treatment.

`prompt_for_tag` extends `adw::AlertDialog` with a custom extra-child Box for the swatch row — first non-trivial AlertDialog use beyond plain confirmations. Fully reactive: dragging the colour onto a tag instantly updates every visible pill via the existing `LibraryChanges` channel.

## v0.2.2 (2026-05-07) — Audit-pass bug fixes

Filter-typo toast warnings (when an unknown field token is parsed away to freeform text, surface a toast so the user knows). Sidebar zero-state hint ("Add an area or project to get started"). Screen-reader badge labels (count badges in the sidebar gain `accessible-description` attributes). Inbox chip fallback on tasks lacking an explicit context.

## v0.2.1 (2026-05-07) — Tag pill update fix + Area › Project chip

Fixed: editing a tag's colour did not propagate to already-rendered pills until the row was re-laid-out (Pango markup re-render gap). Each `LibraryChanges::tag` update now triggers a per-task pill rebuild keyed on the tag id. `Area › Project` row context chip surfaces parent context inline so the eye doesn't have to track the sidebar.

## v0.2.0 (2026-05-07) — Phase 15: Repeating Tasks (Builder Mode milestone)

Closes Phases 10–15 → Builder Mode shipped. Full RFC 5545 RRULE support via the `rrule` crate (sign-off granted before implementation). Three Org-mode completion semantics: `+1d` (regenerate from completion date), `++1d` (regenerate from scheduled date), `.+1d` (regenerate from a "now" sentinel — only the days/weeks shift). Migration `0003_repeat_mode.sql` — first ALTER post-v0.1 (the v0.1 schema freeze ends here; backwards-compatible migrations are now allowed per the schema discipline).

Inspector-pane repeat editor: dropdown → human label, RRULE preview shown live as the user adjusts. Worker regenerates the next occurrence on `ToggleComplete` for repeating tasks; user sees the new row pop in via `TaskChanges` without a refresh.

## v0.1.17 (2026-05-07) — Phase 14: Saved Perspectives

Saved searches as first-class sidebar entries. `Save Search as Perspective…` in the primary menu captures the current search-bar expression + view metadata into the new `perspective` table (migration `0002_perspectives.sql`, additive). Renaming and deleting via the sidebar context menu. Perspectives inherit the full search expression language (Phase 15.5 will retroactively give them grammar improvements without schema changes).

## v0.1.10 → v0.1.16 — Builder polish + interaction fixes

Phase 12 Forecast (30-day calendar-axis, drag-to-reschedule) shipped at v0.1.3. Phase 13 Review queue at v0.1.16. Builder Mode UI shell at v0.1.1; defer dates + sequential-project rendering at v0.1.2. The v0.1.4 → v0.1.9 run resolved Inspector-pane edge cases (synchronous mode flip, Builder Inspector chord, Inspector hide-on-Simple-flip, populate-on-mount). The v0.1.10 → v0.1.15 run was the **double-click hardening arc** — getting double-click to open the Inspector / start inline edit reliably across `GtkColumnView::activate`, gesture interception, and edit-start race conditions. The fix that stuck: listen to `GtkListView::activate` (not `pressed`), defer edit-start to idle, and gate on the gesture-stream timing.

## v0.1.0 (2026-05-07) — Simple Mode ships

Closes Phases 0–9. Six canonical lists (Inbox / Today / Upcoming / Anytime / Someday / Logbook), areas + projects + tags + multi-tag, Quick Entry (Ctrl+Alt+Space), FTS5 search + flat filter expressions, multi-select + undo, Inspector + tag editor dialogs, sidebar find-as-you-type, full keyboard map, typography + accessibility, debug-pane Memory Watch, ship-gate regression script.

Three Phase 9 follow-ups carry to v0.1.x: the actual `v0.1.0` git tag, Flatpak publish, public announcement. Two Phase 8 carryovers: README screenshots, Flatpak font-load verification.

## v0.0.30 → v0.0.38 — Pre-v0.1 polish + bugsweep

The pre-1.0 cleanup arc. Phase 8h silenced two startup/shutdown GTK warnings. Phase 9a built the regression gate (`scripts/regression.sh`: fmt + clippy + test + cold-start sanity). Phase 9b finalised the README. v0.0.33 → v0.0.36 closed the Phase 7 follow-up surface (per-task tag editor, Inspector dialog, layout pass, double-click reliability, stop-eating-spaces in entries). v0.0.37 was the dialog primitives bugsweep: standardised on `adw::Dialog` for in-window modals (Inspector, tag editor); `adw::Window` for non-grab observers (Quick Entry, Memory Watch); `adw::AlertDialog` for confirmations. v0.0.38 added the deadlines-approaching heads-up to Today.

## v0.0.23 → v0.0.29 — Phase 8 (typography, accessibility, perf, debug)

Bundled-font typography polish (Inter cv11/ss01 features, tabular figures audit on every numeric column). Atkinson Hyperlegible accessibility toggle (~80 KB SIL OFL, runtime-swappable). Packaging artefacts (desktop entry, AppStream metainfo, gschema XML, Flatpak manifest). Animation audit + Quick Entry fade-in keyframe. Memory Watch debug pane (`/proc/self/status` sampler, surfaces RSS + heap with a "drop caches" affordance). Accessibility audit (semantic roles, focus rings, screen-reader labels). Performance baseline against `spec.md` §8 budget — release build hits all four targets on Brandon's T14s.

## v0.0.17 → v0.0.22 — Phase 7 (search, undo, multi-select, sidebar, keymap)

FTS5-backed search (Phase 7a). Undo for toggle-complete + delete via a per-action undo stack; toast surfaces the affordance (Phase 7b). Multi-select + bulk operations — bulk complete / move / tag (Phase 7c). Filter expressions in the search bar — flat key:value shape that Phase 15.5 grew into the full grammar (Phase 7d). Find-as-you-type sidebar filter (Phase 7e). Full keyboard map — Ctrl+Z, F2 to rename, etc. (Phase 7f); written reference at `docs/keymap.md`.

## v0.0.14 → v0.0.16 — Phase 6 (tags + Quick Entry)

Tag CRUD + sidebar Tags section (Phase 6a). Tag pills + inline `#tag` / `@date` parser — typing `#work @today` in any task entry creates the tag if absent and applies the date (Phase 6b). Quick Entry modal — Ctrl+Alt+Space anywhere on the desktop drops a tiny `adw::Window` for capture without grabbing focus from the prior application; same parser; closes on Enter (Phase 6c).

## v0.0.10 → v0.0.13 — Phase 5 (areas, projects, sidebar hierarchy)

Sidebar hierarchy + remaining canonical lists (Phase 5a). Area / Project CRUD + the `LibraryChanges` delta channel paralleling `TaskChanges` for area/project mutations (Phase 5b). Count badges + drag-to-project (Phase 5c). Right-click context menus + sidebar selection refinement (Phase 5.5).

## v0.0.6 → v0.0.9 — Phases 2–4 (data layer, application shell, lists)

Single-writer worker + read-only pool (Phase 2): `Command` enum, `TaskChanges` delta, `WorkerHandle`, IO instrumentation via rusqlite's `trace` feature routing every SQL statement into a `tracing` span. Application shell (Phase 3): GTK4 + libadwaita window, sidebar shell, GSettings schema, font-install-on-first-run via fontconfig. Phase 4 brought Inbox + Today + the Calendar Month View item onto the roadmap. Phase 4.5 patched in drag-to-reorder + bottom-of-list entry.

## v0.0.3 → v0.0.5 — Phases 0 + 1 + roadmap horizon

Phase 0 (v0.0.3): Cargo workspace (`atrium` binary + `atrium-core` library), v0.1 dependency set locked, `--debug` skeleton, Meson wrapper, GitHub Actions CI. Phase 1 (v0.0.4): OmniFocus-superset schema in migration `0001_initial.sql` (every Builder column present from day one), FTS5 virtual table + sync triggers, `modified_at` triggers with `WHEN old = new` clauses, stress-fixture generator at four scales. v0.0.5 added the "Beyond 1.0" roadmap section (post-1.0 horizon for `atrium-tui`).

## v0.0.0 → v0.0.2 — Pre-implementation contract refinement

Spec, roadmap, README, LICENSE, VERSION, logo. Org vault as a projection — SQLite canonical, `.org` files downstream — formalised in `spec.md` §3.5 + the §7.3 round-trip rules. Debug-first architecture (`spec.md` §3.4) — `--debug` opens an in-app debug surface for stress generators, edge-case fixtures, IO instrumentation, memory watch — built into the binary, not bolted on. Release discipline written down: every minor or major change touches `spec.md`, `roadmap.md`, `patchnotes.md`, and `VERSION` together; every major bump includes a maintenance pass.
