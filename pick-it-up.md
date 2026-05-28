# pick-it-up.md — Atrium Tiers 2 + 3 arc (v0.30.0 → v0.35.0)

**Last updated:** 2026-05-28. **Reason:** end-of-cut handoff after v0.29.0 shipped.
**Retire this file** once the v0.35.0 cut ships (precedent: v0.23.1 retired the prior `pick-it-up.md`).

## TL;DR

We're in a ten-cut arc closing Atrium's pre-1.0 polish backlog. v0.26.0 (Taskwarrior), v0.27.0 (todo.txt), v0.28.0 (per-area review schedules), and **v0.29.0 (task dependencies, `blocked_by`) all shipped.** **v0.30.0 (drag external files / URLs to capture) is next — not started.** Pick up by entering plan mode for v0.30.0, writing the per-cut design plan, and executing.

The master plan with the full ten-cut sequence and shared scaffolding patterns lives at `/home/bdkl/.claude/plans/foamy-churning-summit.md` (its "Current cut" section is now v0.30.0). Read that first.

## Current state

- **Repo HEAD:** v0.29.0 on `main` (task dependencies). Confirm with `git log --oneline -1`.
- **Workspace unit-test count:** 991 (sum of the `Running unittests` binaries; the four `tests/*.rs` integration binaries — mode_flip_snapshot, org_roundtrip, vault_watcher_integration, worker_org_integration — are counted separately and excluded from the headline figure).
- **Schema version:** 16 (`user_version` 16; next migration `0017_*` lands at v0.34.0 task templates — v0.30.0–v0.33.0 are schema-free).
- **Tier 1 closed at v0.24.0** (custom property-drawer passthrough).
- **Phase 19 importer arc closed at v0.27.0** (Org, Todoist, VTODO, Taskwarrior, todo.txt all shipped). Only the unified import dialog (v0.35.0) remains for Phase 19's GUI side.
- **EDS calendar overlay deferred** out of this arc (needs separate dep sign-off for `libecal-sys` vs hand-rolled `zbus`).

## Remaining cuts (in order)

Six minor bumps left. One feature per cut; each ships independently. See the master plan for full per-cut scope; abbreviated here:

| Cut | Headline | Migration | Surface |
|---|---|---|---|
| v0.30.0 | Drag external files / URLs to capture | none | Window-level `DropTarget` |
| v0.31.0 | Inline editing on row edit | none | Surface `atrium-inline` parser on row edit |
| v0.32.0 | First-run / onboarding | none | `AdwStatusPage` for empty DB |
| v0.33.0 | Backup / restore UI | none | New Backups page in Preferences |
| v0.34.0 | Task templates | `0017_task_template.sql` | New table + Inspector entry + CLI |
| v0.35.0 | Unified import dialog | none | `AdwDialog` for every importer |

After v0.35.0 the arc closes; EDS overlay, Phase 20 (1.0 endgame), README screenshots, and Flatpak font verification carry over.

## v0.29.0 — shipped (reference)

Task dependencies (`blocked_by`). Patterns worth knowing:

- Migration `0016_task_dependency.sql`: `task_dependency(task_id, blocked_by_id)` join table, FK CASCADE both ends, `UNIQUE(task_id, blocked_by_id)`, `created_at` DEFAULT (`user_version` 15 → 16). Row `(A, B)` = "A blocked by B" (B is a prerequisite of A). Added `task_dependency` to the `all_user_tables_exist` test list.
- Worker: `Command::{AddDependency,RemoveDependency}` + handle methods; `would_create_dependency_cycle` (walks prerequisites forward from the proposed blocker; mirror of `would_create_cycle`); `ON CONFLICT DO NOTHING` for dups; `emit_task_refresh(task_id)` after each so the row repaints. New `DomainError::DependencyCycle`.
- Search: `State::Blocked` added; `State::Available` made functional (was a stub). Both translate to EXISTS / NOT EXISTS over `task_dependency` (`sql_translate.rs`); the in-memory path reads `EvalContext.blocked_ids` (new field, defaults to a shared empty set via `with_blocked_ids` builder so the 5-arg `EvalContext::new` callers didn't churn).
- Read: `blocked_task_ids(conn) -> HashSet<i64>` (open tasks with ≥1 open prerequisite) + `list_prerequisites(conn, id) -> Vec<Task>`.
- GUI: `AtriumTask.blocked` flag (mirror `queued`); a "Blocked" pill appended **at the row tail** (next_sibling-chain discipline — don't insert mid-row) + `.blocked` row class; `filter::apply_with_blocked` wrapper (so `apply`'s many callers didn't churn); `replace_store_with_tags_seq` / `apply_changes_seq` gained a `blocked_ids` param + a post-diff `recompute_blocked_state` across the whole store. Builder Inspector "Blocked by" group (`inspector_pane/mod.rs`) with per-prereq remove/navigate rows + a search popover Add picker.
- CLI: `Subcommand::Depend { id, on, remove }`; `run_depend`; `info --human` "Blocked by" section.

## v0.30.0 — next (not started)

### What's known (from the master plan)

Drag external files / URLs to capture. No schema. A top-level `gtk::DropTarget` on `AtriumWindow` accepting `text/uri-list` + `text/plain`; a dropped file → Quick Entry pre-fill (title = filename, note = `file://…`), a dropped URL → pre-fill with the URL in the note/body. Mirrors Errands / Planify.

### Scope (confirm in plan mode)

- Lift the `gtk::DropTarget` idiom from `atrium/src/ui/{forecast,task_list,board,calendar}.rs` / `window/sidebar.rs`; add a window-level top target.
- Route the drop into the existing Quick Entry pre-fill path.

### Open decisions to surface

- Drop while the main window is minimised — capture, or require focus?

## Per-cut workflow (established, applies to every remaining cut)

1. **Plan mode** — enter, write a per-cut design plan into `~/.claude/plans/foamy-churning-summit.md` (replace the previous "Current cut" section). Run `ExitPlanMode` for sign-off.
2. **Implement.**
3. **Test gates** (every cut must pass):
   ```bash
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --all --check
   bash scripts/regression.sh
   appstreamcli validate data/io.github.virinvictus.atrium.metainfo.xml
   ```
4. **Manual sanity:** CLI cuts get a shell smoke against the fixture; GUI cuts launch the binary and exercise the feature.
5. **Release artifacts** — every minor bumps all of: VERSION, Cargo.toml, metainfo XML (new `<release>`), spec.md, roadmap.md, patchnotes.md, CLAUDE.md. The "all docs per minor" rule is in `CLAUDE.md` under "Release discipline".
6. **Commit + push** — one commit per cut (HEREDOC message, `Co-Authored-By` trailer). Push to `origin/main` only with Brandon's go-ahead.

## Established conventions (don't re-derive)

- **Hand-rolled stdlib parsers.** Every importer in `atrium-cli` is stdlib-only. No `csv` / `regex` / `ical` crates. The dep-discipline rule (no third-party crates without Brandon's sign-off) is the highest-priority rule in this codebase.
- **`BTreeMap<String, String>` for property bags.** v0.24.0 established `task.extra_properties` as the lossless catch-all. New columns of this shape mirror the `default_tags` JSON encode/decode at `atrium-core/src/db/worker.rs`.
- **`Option<Option<T>>` for nullable-clearable update fields.** `AreaUpdate.color` and `.default_review_interval_days`, `ProjectUpdate.review_interval_days`, etc. `Some(None)` clears, `Some(Some(v))` sets, `None` leaves untouched. The builder takes `Option<T>` and wraps in `Some`.
- **Migrations are append-only.** Every cut with a migration bumps the `MIGRATIONS` array, the `user_version` comment block, and the 4 pinned `assert_eq!(v, N)` sites in `atrium-core/src/db/mod.rs` (×3) + `read_pool.rs` (×1).
- **`#[cfg(test)] mod round_trip_tests;` pattern** for importer integration tests (atrium-cli is a binary crate). Worker/test-file splits use `#[path = "..._tests.rs"] mod tests;`.
- **Atomic writes** via `atrium_core::sync::atomic::write_atomic`. Every disk write goes through this.
- **No em-dashes in prose written for Brandon.** Global rule. Comments inside source are exempt. EN-dashes in numeric ranges and hyphens in compound modifiers are fine.

## Gotchas surfaced during this arc

- **Test-count methodology.** The headline test count is the sum of `Running unittests` binaries only. The four `tests/*.rs` integration binaries are excluded. v0.29.0 = 991 by this method (full `cargo test` sum including integration + doctests is 1027).
- **rustfmt-version churn.** `rustfmt 1.9.0` reorders `use` imports (lowercase items like `params` sort after type names). HEAD is already in that style. If a PostToolUse formatter hook or a stray `cargo fmt` regresses sibling files you didn't touch, `git checkout --` the untouched files and run `cargo fmt --all` once to normalise only your edits. Verify scope with `git diff --stat`.
- **Cargo.lock drift.** The v0.27.0 commit bumped `Cargo.toml` but didn't commit the regenerated `Cargo.lock`; the v0.28.0 build corrected the workspace-crate versions in the lockfile. Always include the regenerated `Cargo.lock` in the cut commit.
- **AppStream first-word capitalisation.** `appstreamcli validate` lints `description-first-word-not-capitalized` when any `<p>` inside `<description>` starts with a lowercase word. Recast so the first word is capitalised; don't open a `<p>` with `atrium-cli` / `todo.txt`.
- **`status:waiting` round-trip.** Taskwarrior + Org both have a WAITING keyword; the mapper stashes it as `task.orig_keyword = "WAITING"` (Org importer pattern at `atrium-org/src/org/import.rs`). Reuse for any future importer surfacing non-canonical keywords.
- **`+project` token semantics.** Inline project tokens in todo.txt / Taskwarrior are dropped, not mapped — `--into PROJECT` wins, surfaced as a `LossyKind` entry.
- **Diagnostics can be stale.** rust-analyzer "missing field" / "file not found for module" diagnostics often persist after a fix. If `cargo build` succeeds but the diagnostic remains, it's stale.

## Where to look for shared scaffolding

- **Importer pattern:** `atrium-cli/src/import/{todoist,taskwarrior,todotxt}/`, `atrium-cli/src/vtodo/`.
- **Cycle guard:** `atrium-core/src/db/worker.rs::would_create_cycle` (subtasks). v0.29.0 dependencies mirror this as `would_create_dependency_cycle`.
- **Search predicate scaffold:** `atrium-search/src/{ast,eval,parse,sql_translate}.rs`. `State::Available` exists; v0.29.0 extends both eval + SQL and adds `State::Blocked`.
- **Inspector picker idiom:** `atrium/src/ui/inspector_pane/fields.rs` Org-link picker. v0.29.0 dependency picker reuses the search-as-you-type ListBox shape.
- **`gtk::DropTarget`:** `atrium/src/ui/forecast.rs`, `task_list.rs`, `board.rs`, `calendar.rs`, `window/sidebar.rs`. v0.30.0 adds a window-level top target.
- **Area-property edit:** `atrium/src/ui/window/widgets.rs::prompt_for_named_color` + `actions.rs` callers + the `area_*` caches in `window/mod.rs` / `sidebar.rs` (see the v0.28.0 reference above).
- **Preferences extension:** `atrium/src/ui/preferences.rs`. v0.33.0 adds a Backups page; v0.32.0 may add an onboarding-disable toggle.

## Commit message archive (style reference)

The v0.24.0 → v0.28.0 commits set the voice and structure. Read them with `git log -p --grep "Co-Authored-By: Claude"` to crib the shape.

## When the arc closes

After v0.35.0 ships, retire this file (`git rm pick-it-up.md` + a small note in the v0.35.0 patchnote). The arc's carryover items:

- README screenshots (Simple + Builder) — manual capture pass for Brandon.
- Flatpak font verification under the sandbox — `flatpak-builder` run for Brandon.
- EDS calendar overlay — open the dep sign-off conversation; new planning session.
- Phase 20 (1.0 endgame): `atriumd`, localisation, `mdbook` docs site, AppStream screenshots, Flathub submission, 50K-task perf suite, accessibility round 2, `v1.0.0` tag.
