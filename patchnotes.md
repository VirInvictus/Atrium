# Atrium — Patch Notes

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

The search bar's filter language grew from the v0.1 flat shape (`tag:foo is:open due:today` — useful but limited) into a full Calibre-style expression grammar. Saved Perspectives inherit the new power for free since they store filter expressions verbatim. The full operator reference lives in `spec.md` §4.3; the highlights:

### What's new

**Boolean composition with grouping.**

```text
tag:work AND is:overdue            implicit-AND default still works
tag:work OR tag:home               OR keyword (case-insensitive)
NOT tag:archived                   NOT prefix; ! is the shorthand
(tag:work OR tag:home) AND !done   parens override precedence
```

Standard precedence: **`NOT > AND > OR`** (matches Calibre, SQL, Python). `tag:work AND !done OR tag:home` parses as `(tag:work AND (NOT done)) OR tag:home`.

**Calibre match modifiers on every text field.**

```text
tag:work        substring (default, case-insensitive) — matches "worker", "homework"
tag:"work focus"  quoted substring for values with spaces
tag:=work       exact match (case-insensitive)
tag:"=work focus"  quoted exact for exact values with spaces
tag:~mystery.*  regex match (RE2 syntax via the regex crate)
tag:true        boolean existence — task has at least one tag
tag:false       boolean none — task has no tags
```

**Comparison + range on date and numeric fields.**

```text
due:>today                    deadline strictly in the future
due:<=lastweek                deadline anywhere through end of last week
estimated:>=30                tasks needing 30+ minutes
due:2026-05-01..2026-05-31    inclusive date range
```

**Date keywords** (Calibre's set plus future-tense forms Atrium needs):

`today`, `yesterday`, `tomorrow`, `thisweek`, `lastweek`, `nextweek`, `thismonth`, `lastmonth`, `nextmonth`, `thisyear`, `Ndaysago`, `Ndaysout`.

**State predicates** as `is:NAME` shortcuts:

`is:open`, `is:done`, `is:overdue`, `is:scheduled`, `is:deadline`, `is:deferred`, `is:repeating`, `is:archived`, `is:logbook`, `is:project`, `is:area`, `is:tagged`, `is:queued`, `is:available`. Each pairs with `!is:NAME` for the inverse.

**New field operators.** `area:`, `project:`, `title:`, `note:`, `created:`, `modified:`, `completed:`, `estimated:`, `repeats:` join the existing `tag:` / `due:` / `scheduled:` / `defer:` / `is:` set.

### Architecture

`atrium-core/src/search/` (new module, ~1100 lines):

- `lex.rs` — tokenizer producing `Token` stream. Bare words, quoted strings with `\"` / `\\` escapes, single- and multi-byte operators (`<`, `<=`, `..`, `!=`).
- `ast.rs` — `Expr` enum + supporting types (`Field`, `MatchKind`, `State`, `Comparator`, `Value`, `DateKeyword`). Round-trip-shaped: `Display` impls re-render to canonical text that re-parses to the same AST.
- `parse.rs` — recursive-descent parser. `parse_or` → `parse_and` → `parse_not` → `parse_primary` → `parse_term`. Sense-corrects bare date keywords on date fields into `Compare(Eq, ...)` (so `due:today` means "due exactly today", not "due column contains the substring 'today'"). Unknown field names are non-fatal warnings — the token falls through to freeform text.
- `eval.rs` — single-pass in-memory evaluator. Lazy regex compilation cached per-query in an `EvalContext` (same query against many tasks reuses the compiled `Regex`). Date-keyword resolution into `(low, high)` ranges; comparison operators interpret keyword-ranges sensibly (`due:>thisweek` is "after the end of this week").

`atrium-core` adds a direct dependency on the `regex` crate (sign-off granted before implementation; already transitively in the dep graph via `tracing-subscriber`, so the artifact ships either way). In-memory only — SQLite has no built-in regex.

`atrium/src/ui/filter.rs` is now a thin shim over `atrium_core::search`. Window-side call sites (search bar, Perspective load) keep their old shape — `parse(query)` returns a `FilterQuery` carrying the AST + warnings; `apply` filters a task vector against the parsed expression.

### Visual feedback

Search entry gains a yellow libadwaita `.warning` accent when the parsed expression has unrecognised tokens (`tga:work`, `is:fnord`). Tooltip on the entry surfaces the unknown tokens. Cleared the moment the user fixes the typo. Combines with the existing toast notification from v0.2.2 — toast for the explicit "you typed something wrong" hit, accent for the persistent-while-broken visual cue.

### Numbers

- 248 tests pass total: 82 atrium (binary), 165 atrium-core (lib, +41 from v0.3.0's 124 — search module), 1 mode-flip integration.
- New crate dependency: `regex` 1.12 (MIT/Apache, already transitively in tree).
- Clippy clean, fmt clean, regression script green at v0.4.0.

### What's deferred to v0.4.x patches

Three Phase 15.5 line items intentionally cut from v0.4.0 to keep the release focused; each is a polish addition, not a correctness gap:

- **SQL-translation evaluator.** Translates the AST to a `WHERE` clause for views over the entire library so we don't load every task to filter. Pure perf optimization — the in-memory path handles 100K tasks within the §8 budget today; we'll add SQL translation when measurement says we need it.
- **Search history ring buffer.** `↑` / `↓` to cycle the last 20 searches. Useful but additive.
- **Operator reference popover.** `?` button at the right of the search bar showing the operator set inline. Until then, `spec.md` §4.3 is the authoritative reference, and the search-bar placeholder still hints at the basics.

### Spec discipline

`spec.md` §4.3 is the authoritative reference for the language. Keeping it in `spec.md` rather than a separate `docs/search.md` means schema changes that touch the search surface (a new field, a renamed column) edit one document not two.

§4 was renumbered: the new §4.3 (Search Expression Language) shifts FTS5 → §4.4 and Migrations → §4.5.

## v0.3.0 (2026-05-07) — Visual polish pass

A focused minor release dedicated to making Atrium feel less utilitarian. No new features in the strict-spec sense — every change is a UI/UX refinement on top of what v0.2.2 ships. Two tiers of improvements landed:

### Tier 1 — quick wins

**Tag colours wired in.** The `tag.color` column has been in the schema since Phase 1 (v0.1.0) but no UI wrote to it or rendered it. v0.3.0 closes the loop:

- New `prompt_for_tag` helper presents a six-swatch palette (Blue / Green / Yellow / Orange / Red / Purple, plus a "no colour" option) alongside the name entry. Both *New Tag* and *Rename Tag* go through this prompt; the rename flow pre-selects the tag's current colour.
- Sidebar tag rows: the leading icon swaps for a coloured dot when a colour is set. Tooltip surfaces the hex value for power-user verification.
- Task-row tag pills: each tag renders as a coloured `<span foreground="#hex">#name</span>` Pango span. Tags without a colour fall back to the existing dim-label accent treatment.
- Stored as hex strings (`#3584e4`, etc.) — same values used in the swatch CSS classes (`.atrium-swatch-blue`, …) so the picker, sidebar dot, and task-row pill all render the same colour for a given tag.

Data plumbing: new `read::tag_info_per_task` returns `HashMap<i64, Vec<(String, Option<String>)>>` (typed as `TagInfoMap`). Window-side caches a `tag_pills: TagPillMap` alongside the existing `tag_map: TagMap`; the renderer uses the rich form, the filter evaluator keeps the name-only form for substring matching.

`format_tag_names` migrated from `&[String]` → `&[(String, Option<String>)]`; both call sites (`replace_store_with_tags_seq` and `apply_changes_seq`) updated. The `tags` GtkLabel in the row factory now has `use-markup=true`.

**Row hover state.** `.atrium-task-row:hover` adds a subtle `alpha(@accent_bg_color, 0.08)` background tint with a 120ms transition. Selection highlight (libadwaita default) takes priority when both fire. The row also gets a 6px border-radius so the hover tint reads as a discrete chip rather than a flat band.

**Per-list empty-state warmth.** Every empty-list state got a copy refresh with warmer phrasing — "Inbox zero" instead of "Inbox is empty"; "Clear plate today" instead of "Nothing today"; "Open horizon" for Upcoming and Forecast (the same phrase intentionally — both speak to "future is open"). The Someday icon swapped from the bizarre `user-home-symbolic` (a house) to `weather-clear-night-symbolic` (a moon) which actually evokes "later, after dark."

**Search bar placeholder rephrased.** From the colon-delimited `Search · tag:errand · is:overdue · due:today` (which read like a config file) to `Find tasks — try tag:errand, due:today, or is:overdue` (which reads like an invitation).

### Tier 2 — substantive polish

**Completion micro-animation.** A 280ms keyframe (`atrium-task-check-pop`) gives the CheckButton a brief 1.0 → 1.25 → 0.95 → 1.0 scale pulse when the row's `.completed` class is added, plus a `@success_color` colour swap on the inner check glyph. The animation only fires going from open → done — toggling back to open just runs the existing opacity transition in reverse. Things-3-style satisfaction without confetti.

**Sidebar visual rhythm.** Section headers (Areas / Tags / Builder / Perspectives) gained the new `.atrium-sidebar-section` class:

- 600 weight, uppercased, 0.04em letter-spacing — reads as a label rather than a row title.
- 0.78em font size — tighter than rows, so they don't compete.
- 1px top border in `alpha(@borders, 0.4)` — visible separator between sections without a heavy rule.

**Header bar context breadcrumb.** When viewing a Project, the header bar title now reads `Area › Project` instead of just the bare project name (when the project has an area). Falls back to the bare name for unfiled projects. `title_for(ActiveList::Project(_))` consults the existing `project_meta` and `area_titles` caches; no new SQL.

**Inspector pane card treatment.** The Builder Mode side pane gets the new `.atrium-inspector-pane` class on its `AdwPreferencesPage`. Adds a 1px left border in `alpha(@borders, 0.4)` plus 12px padding around the prefs groups, so the pane reads as a separate sheet of paper rather than an extension of the main task list. Subtle but the form-vs-list distinction makes the editor more inviting.

### CSS additions

`data/style.css` grew several blocks:

- `.atrium-swatch{,-blue,-green,-yellow,-orange,-red,-purple}` — circular toggle buttons in the tag-colour picker, plus a `:checked` ring.
- `.atrium-tag-dot` — the sidebar tag-row colour dot.
- `.atrium-sidebar-section` — section header treatment.
- `.atrium-inspector-pane` — Inspector pane left-border + clamp padding.
- `.atrium-task-row:hover` — hover tint.
- `@keyframes atrium-task-check-pop` — completion pulse animation.

### Tests

- 215 tests pass (90 atrium + 124 atrium-core + 1 mode-flip integration). No new tests; this release is render-only.

### What's intentionally NOT in v0.3.0

Tier 3 from the audit triage stays open as future work:

- **Per-area colour theming** — would need a new `area.color` column (additive migration) and a per-area accent on row left-edges. Belongs with a future phase that thinks about hierarchical visual identity holistically.
- **Logbook day grouping** — "Today / Yesterday / Last 7 Days / Older" section headers. Transformative for retrospective scanning but a real bit of work; deserves its own follow-up rather than getting tucked into a polish release.

## v0.2.2 (2026-05-07) — Audit-pass bug fixes

A focused patch from the v0.2.x audit. Four bug fixes; no new features. The next minor (v0.3.0) will tackle the visual-polish pass to make the app feel less utilitarian; this release clears the rough edges first.

### Filter typos surface a toast

`atrium/src/ui/filter.rs::parse` now collects unrecognised `key:value` tokens into a `warnings: Vec<String>` field on `FilterQuery`. Window-side, `surface_filter_warnings` toasts the unknown tokens (capped at three previewed, with a "+N more" suffix when longer). Toasts deduplicate against `last_filter_warning: RefCell<Option<String>>` on the imp struct, so the same typo doesn't re-toast on every refresh tick.

Wired into:

- The search bar's `connect_search_changed` handler — typing `tga:errand` produces a toast immediately.
- The Perspective load path (`refresh_active_list` for `ActiveList::Perspective(_)`) — saved perspectives with malformed expressions surface their warnings on first load.

Four new filter-parse tests cover the warning collection: unknown prefix, unknown value under a known key, recognised filters producing zero warnings, and the freeform-text fallback that was already there.

### Sidebar zero-state hint

When areas, projects, *and* tags are all empty (true cold-start, or a fully-pruned library), a `GtkRevealer` slides up at the bottom of the sidebar with:

- Caption-heading: "No projects yet"
- Caption-dim: "Group tasks by what they're for. You can always add an area later to organise multiple projects."
- Pill-styled `suggested-action` button: "New Project" → `app.new-project`

The check fires from `rebuild_dynamic_sidebar` after the canonical / dynamic rows are built, so it stays in sync as the library fills out. Tags-only libraries (capture-by-tag workflow) and areas-without-projects (in-progress states) don't trigger the hint — only the genuinely-empty case.

`data/window.ui` adds `sidebar_empty_hint: GtkRevealer` to the sidebar's vertical Box, below the existing scrolled-window-with-listbox.

### Inbox chip fallback in the Area › Project context

The chip introduced in v0.2.1 rendered blank when a task had no project — leaving users to wonder what the missing chip meant. `build_context_resolver` now returns `"Inbox"` for `project_id IS NULL` tasks when the chip mode is `AreaAndProject`. `ProjectOnly` mode (Area views) keeps the empty render — the area heading already names the scope, and there's no project to label.

### Screen-reader badge labels

`apply_badge_label` now sets `accessible::Property::Label` on each count badge with the *meaning* of the number, not just the digit. A badge showing "5" exposes its accessible label as `"5 open tasks"` (or `"1 open task"` for the singular case), so screen readers announce "Today, 5 open tasks" instead of "Today, 5".

The visible label stays a bare number — sighted users see the existing tabular-nums column. Only the SR-announced text changes.

### Numbers

- 215 tests pass: 90 atrium (binary, +3 from v0.2.1's 87), 124 atrium-core (lib, unchanged), 1 mode-flip integration. New atrium tests are the four filter-warning round-trip cases (one was added in v0.2.1's path that's still active).
- Clippy clean, fmt clean, regression script green at v0.2.2.

### Out of scope (deferred)

The audit also flagged a few items that turned out not to be actual bugs on closer reading:

- **Perspective deletion fallback** — already correct in `apply_library_changes`; falls back to Today and selects the row.
- **Perspective view skipping `refresh_dynamic_badges`** — already called after `refresh_active_list` in the relevant arm.
- **Forecast drag-to-yesterday** — `group_by_date` only emits cards from `today` forward, so a past date isn't reachable through the UI.

Two real items were flagged but explicitly deferred to future work:

- **Filter expression v2** — the Calibre-style grammar (Phase 15.5) supersedes the warning-on-typo approach. The current toast is the right shape for Phase 7d's flat language; the v2 grammar will surface validation inline in the search bar.
- **Sidebar accessibility on dynamic rows** — the audit overstated this. Each row already has `Property::Label` set via `sidebar_row()`; only the count badges were missing semantic meaning, and that's what this release fixed.

## v0.2.1 (2026-05-07) — Tag pill update fix + Area › Project chip

Two task-row fixes that arrived together because they touch the same factory and diff applier.

### Tag pill never refreshed when added or removed via the per-task editor

The tag label widget in the row factory was created with `visible(false)` at setup time and never had its visibility updated when `tag_names_csv` changed. The label's *text* was bound (so the property carried the right string), but the widget stayed hidden. New rows with non-empty tags rendered no pills; existing rows with tags didn't lose the chip when tags were removed; rows that had a tag added via the Inspector tag editor stayed pill-less.

Fix in `atrium/src/ui/task_list.rs`:

- `connect_bind` now calls `tags.set_visible(!task.tag_names_csv().is_empty())` — initial visibility from the bound state.
- A new `connect_tag_names_csv_notify` handler updates visibility on every property change. The handler ID is stashed under `"atrium-tags-handler"` and disconnected in `connect_unbind` so recycled rows don't accumulate handlers across binds.

The schedule and deadline labels already had this exact treatment (one-shot `set_visible` in bind plus implicit refresh via `refresh_from`); the tags label was the only `_label`-style row widget without it.

### Area › Project chip on cross-list task rows

The roadmap had a long-implicit gap: Today / Inbox / Anytime / Logbook / Tag / Forecast / Perspective / Search rows didn't show *which project* a task belonged to. You'd see "Buy milk" with `#errand May 12` but no indication of whether it was filed under Personal › Groceries or floating in the Inbox. Things 3 surfaces this with a project subtitle; OmniFocus uses an inline chip; Atrium now uses a chip on the right of the row.

Layout (Things-3 inspired):

```
[✓] Buy milk    #errand   · Personal › Groceries ·  May 12  Due May 14
[ ] Write spec  #work     · Work › Atrium       ·  May 15
```

Implementation:

- `AtriumTask` gains a `context_label` glib property — the rendered string per row.
- The factory adds a new ellipsizing `gtk::Label` between the tags chip and the schedule pill, with CSS class `.atrium-task-context` and a `.dim-label` style hand-off. `bind_property("context-label", &context, "label")` keeps it in sync; a notify hook flips visibility.
- `replace_store_with_tags_seq` and `apply_changes_seq` now take a `context_for: F` closure parameter — the window builds the closure per refresh based on the active list, populating from the existing `project_titles`, `area_titles`, and `project_meta` caches. No new SQL.
- `AtriumWindow::build_context_resolver` selects one of three modes:
  - **Suppressed** on `Project(_)` views — the heading already names the project, the chip would echo it.
  - **ProjectOnly** on `Area(_)` views — the area is the heading, render only the project name as the inner scope.
  - **AreaAndProject** everywhere else (Today / Inbox / Anytime / Someday / Logbook / Tag / SearchResults / Forecast / Perspective / Upcoming) — the full `Area › Project` form.
- The chip ellipsizes at 40 characters max so long Area + Project combinations don't push the schedule and deadline pills off-screen on narrow windows.

CSS lands in `data/style.css` as `.atrium-task-context` — quieter than the tag chip (no background, smaller font, dim colour from the inherited `.dim-label`) so it reads as reference info, not metadata you actively engage with.

### Numbers

- 212 tests still pass — no test changes needed (pure UI plumbing; the diff applier semantics are unchanged).
- Clippy clean, fmt clean, regression script green at v0.2.1.

## v0.2.0 (2026-05-07) — Phase 15: Repeating Tasks (Builder Mode milestone)

The first major release. Phases 10–15 are done; Builder Mode is feature-complete for the v0.2 milestone. Atrium can now repeat tasks with full RFC 5545 RRULE support, three Org-style completion semantics, and end-to-end editor → worker → list integration.

### Repeating tasks

Set a repeat in the Inspector pane (Builder Mode → select a task → scroll to *Builder*):

- **Frequency** dropdown: None / Daily / Weekly / Monthly / Yearly / Custom. None clears the rule and the task stops repeating. Custom takes a raw RFC 5545 RRULE string (`FREQ=WEEKLY;BYDAY=MO,WE,FR`, `FREQ=MONTHLY;BYMONTHDAY=15`, anything the `rrule` crate parses).
- **Every N** spin: hidden for None / Custom; visible for the four presets.
- **After completion** dropdown — the Org-mode "cookie":
  - *After completion (Cumulative)* — Org's `++1w`. Skip ahead until the next occurrence is in the future. Spawn that one. Most chores want this; it's the default.
  - *From completion date (Next)* — Org's `.+1w`. Anchor on when you finished, ignore the previous schedule. Right for "every N after I last did this" (haircut, oil change).
  - *Always shift by interval (Basic)* — Org's `+1w`. Always shift exactly one rule increment from the previous anchor, even if the result lands in the past. Rare; included for round-trip fidelity with Org files.

When you complete a repeating task, the worker:

1. Marks the row done (`completed_at = now()`).
2. Reads the rule + mode + the earliest set date field (scheduled / deadline / defer).
3. Computes the next occurrence per the mode.
4. Inserts a fresh `task` row with: new uuid; same project / parent / title / note / tags / repeat config; date fields all shifted by the same delta; `completed_at = NULL`.
5. If the rule had `COUNT=N`, the spawned row's `COUNT` is decremented; when `COUNT` was already 1, the just-completed instance was the last and no spawn happens.

The completed instance stays in the Logbook as the historical record. Tags carry forward (one INSERT-FROM-SELECT on `task_tag`). Reopening a completed task is a pure reopen — never a regenerate.

### Schema

`atrium-core/src/db/migrations/0003_repeat_mode.sql` — backwards-compatible additive change:

- `task.repeat_mode TEXT NULL` — one of `BASIC` / `CUMULATIVE` / `NEXT`. NULL falls back to the default (CUMULATIVE — matches Org's `++` and OmniFocus's "next instance after now").

`PRAGMA user_version` advances 2 → 3. The migrations array in `atrium-core/src/db/migrations/mod.rs` registers the new version. **First migration to alter an existing table** — allowed because v0.2.0 ends the v0.1 schema freeze. Future minor releases can ship more `ALTER` migrations without further policy changes.

### Data layer

`atrium-core` adds the `rrule` crate (sign-off granted before Phase 15 implementation; the alternative was a hand-rolled RFC 5545 subset that would have to be replaced when Phase 17 needs full RRULE round-trip for Org export).

`atrium-core/src/repeat.rs` (new, ~330 lines including tests):

- `RepeatMode` enum (`Basic` / `Cumulative` / `Next`) with `from_column` / `as_column` / `org_cookie` round-trips. `#[derive(Default)]` annotates `Cumulative`.
- `RepeatRule { rule, mode }` with `parse(rule, mode)` validating against the rrule crate, `next_after(previous_anchor, completed_on)` computing the next occurrence per mode, and `count` / `rule_with_count_decremented` for COUNT termination.
- `CountStep { Unbounded, Decremented(String), Exhausted }` — the discriminated outcome of "should we spawn a follow-up, and with what rule text?"
- 18 unit tests covering parse / mode round-trip / cumulative-skip / basic-strict-next / next-anchors-on-completion / daily / monthly / monthly-end-of-month-skips / yearly / count-termination / org-cookie-emit / count-step-{unbounded,decremented,exhausted}.

`Task.repeat_mode: Option<String>` and `NewTask.repeat_mode: Option<String>` exposed on the domain types. `TaskUpdate.repeat_rule_value(Option<String>)` and `TaskUpdate.repeat_mode_value(Option<String>)` builder methods set/clear in the worker. `is_noop` updated.

`atrium-core/src/error.rs` adds `DbError::BadRepeatRule(String)` so the editor can surface validation failures without relying on the underlying rrule diagnostic shape.

`atrium-core/src/db/worker.rs`:

- `create_task` and `update_task` validate `repeat_rule` against `RepeatRule::parse` up front; malformed text is rejected before any DB write.
- `toggle_complete` returns `(Task, Option<Task>)` — the toggled instance and an optional spawned follow-up. The dispatch arm packages both into a single `TaskChanges` delta (`updated` + `created`) so the UI sees them atomically.
- `spawn_repeat_follow_up(completed)` does the full regeneration logic: anchor pick, mode-aware iteration, COUNT decrement, NewTask construction, tag carry-forward INSERT.
- 7 new worker tests: spawn-on-complete, project / note / repeat-config carry, no-spawn-for-non-repeating, COUNT termination, no-spawn-on-reopen, weekly-survives-1-year-horizon, monthly-skips-short-month-end-of-month. The 1-year horizon test exercises the loop 52 times and asserts each cycle's date.

The public `WorkerHandle::toggle_complete` API is unchanged — UI callers still see a single `Task` returned. The spawned follow-up surfaces only via the `TaskChanges` delta on the changes channel.

### UI

`atrium/src/ui/inspector_pane.rs::install_repeat_editor` — the four-row repeat editor described above, autosaving on every interaction. Local validation short-circuits malformed Custom RRULE text before dispatch (the worker still validates server-side as a backstop). 6 new unit tests cover preset recognition, interval round-trip, rule emission, and mode-index round-trip.

The previous "Editor lands in Phase 15" placeholder row is gone.

### v0.2.0 maintenance pass

Per CLAUDE.md release discipline, every major bump runs a maintenance pass:

- **Dead code removal**: `ActiveList::is_builder_stub()` always returned `false` post-Phase 14 and was only called from its own tests. Function and tests removed.
- **Test helper consolidation**: four duplicated `dummy_task` / `dummy` helpers across `db/changes.rs`, `ui/task_object.rs`, `ui/task_list.rs`, `ui/filter.rs`, `ui/forecast.rs` (each ~15-line `Task` literals that had to be touched on every domain-struct change) consolidated into a new `atrium-core::test_support` module. Gated behind a `test-support` Cargo feature; the `atrium` binary opts in via `[dev-dependencies]`. Future schema columns are now a one-line edit instead of a sweep.
- **Clippy & doc nits**: `RepeatMode` switched to `#[derive(Default)]` with `#[default]` attribute (clippy's `derivable_impls`); doc-list-without-indentation warning fixed in `repeat.rs`.

### Spec discipline

Spec §4.4 ("Migrations") amended: the v0.1 schema freeze is now formally **"no breaking mid-v0.1 schema changes"** — purely-additive new tables (the v0.1.17 precedent set with `0002_perspectives.sql`) are allowed when they don't disturb v0.1 code paths or shift the v0.2 plan. v0.2.0 ships the first migration to alter an existing table (`0003_repeat_mode.sql`), explicitly allowed because the v0.1 freeze ends with this release.

`CLAUDE.md`'s schema-rule section updated to match.

### Numbers

- **212 tests** pass total: 89 atrium (binary), 124 atrium-core (lib), 1 mode-flip integration. Up from 199 at v0.1.17 (+14: 7 worker regen tests, 4 repeat-rule round-trip tests, 6 inspector-pane preset tests, 4 CountStep tests; offset by removed builder-stub tests).
- One new dependency: `rrule` (crate v0.14, MIT/Apache).
- Schema version: 3.
- One new migration: `0003_repeat_mode.sql`.

### Phase 15 follow-ups

- **Per-area review schedules** (deferred from Phase 13) — still open. The `area` table can take a `default_review_interval_days` column now that v0.2.0 unlocks `ALTER TABLE`. Quality-of-life on top of the per-project interval that already works; not blocking.
- **Drag-to-reschedule in Forecast** picking up repeating-task semantics — the spawned follow-up should respect any reschedule the user makes mid-cycle. Belongs with Phase 17's vault-projection work since the same logic governs Org round-trip.

## v0.1.17 (2026-05-07) — Phase 14: Saved Perspectives

Builder Mode's last big sidebar feature lands: saved Perspectives. Type a filter expression in the search bar (`tag:work is:overdue`, `due:today is:open`, etc.), open the primary menu, pick **Save Search as Perspective…**, name it, and it lives forever in a dedicated *Perspectives* section in the Builder sidebar. Selecting a perspective re-runs its filter expression against the current task set every time, so the view stays current without any manual refresh.

Right-click a perspective row for **Rename** / **Delete** (same shape as the area / project / tag context menus). Deleting only removes the saved view — the underlying tasks are untouched.

### Schema

`atrium-core/src/db/migrations/0002_perspectives.sql` — backwards-compatible add:

- `perspective` table: `id`, `uuid`, `name`, `icon`, `filter_expr`, `sort_order`, `grouping`, `position`, `created_at`, `modified_at`. The `sort_order` and `grouping` columns are populated by the schema but not yet consumed by the UI — they exist now so a future minor release can add per-perspective sort / grouping without another migration.
- `perspective_modified_at` trigger keeps `modified_at` in sync on every UPDATE, mirroring the trigger pattern on tasks / projects.

`PRAGMA user_version` advances from 1 → 2; the migrations array in `atrium-core/src/db/migrations/mod.rs` registers the new version. Existing v0.1 databases pick the migration up on next launch via the established `migrate(&mut conn)` path — no schema rule was broken (the freeze rule is "no breaking changes mid-v0.1"; this is purely additive).

### Data layer

`atrium-core/src/domain` — `Perspective`, `NewPerspective`, `PerspectiveUpdate` types with serde derives and the same builder-pattern shape as `ProjectUpdate` / `TagUpdate`.

`atrium-core/src/db/read.rs` — `list_perspectives(conn) -> Vec<Perspective>` (ordered by `position, name`) and `perspective_by_id(conn, id) -> Option<Perspective>` plus a `PERSPECTIVE_COLUMNS` const and a `perspective_from_row` row-mapper. No FTS5 or join — perspectives are a flat lookup table.

`atrium-core/src/db/command.rs` — three new variants: `Command::CreatePerspective { perspective, responder }`, `Command::UpdatePerspective { update, responder }`, `Command::DeletePerspective { id, responder }`. `variant_name()` arms updated.

`atrium-core/src/db/worker.rs`:

- `create_perspective(perspective)` — generates a fresh UUID, picks the next position via `next_perspective_position` (max + 1.0, mirroring the existing area / project pattern), inserts, returns the row.
- `update_perspective(update)` — partial update with the same dirty-field tracker as `update_project` / `update_tag`; emits `LibraryChanges { perspectives_updated }`.
- `delete_perspective(id)` — straight DELETE; emits `LibraryChanges { perspectives_deleted }`.
- `WorkerHandle::create_perspective` / `update_perspective` / `delete_perspective` async APIs.

`atrium-core/src/db/changes.rs` — `LibraryChanges` extended with `perspectives_created` / `perspectives_updated` / `perspectives_deleted` Vecs. `merge` and `is_empty` updated.

Three new worker tests round-trip create / update / delete and assert the matching `LibraryChanges` payload lands on the channel.

### UI

`atrium/src/ui/task_list.rs::ActiveList`:

- `Perspectives` (no-arg stub) replaced with `Perspective(i64)`.
- `is_builder_stub()` now always returns `false` (Forecast / Review / Perspective each drive concrete content).
- `task_matches` returns `false` for `Perspective(_)` — the diff applier would need filter-expression visibility to make a sensible call, so refresh-on-update covers it (cheap, FTS5-backed).

`atrium/src/ui/window.rs`:

- New imp fields `perspective_titles: HashMap<i64, String>` and `perspective_meta: HashMap<i64, atrium_core::Perspective>`. The titles cache resolves the content-pane heading; the meta cache lets `refresh_active_list` re-parse the saved filter expression without a read-pool round trip and powers rename-prefill / delete-confirmation prompts.
- `rebuild_dynamic_sidebar` appends a `Perspectives` section header in Builder Mode (always — even with zero perspectives, so the user knows where new ones land) followed by one row per saved perspective. Each row gets a context-menu gesture via `install_perspective_context_menu`.
- `refresh_active_list` routes `ActiveList::Perspective(id)` through `crate::ui::filter::parse` + `crate::ui::filter::apply` — exactly the same pipeline the search bar uses, so saved expressions and live ones behave identically.
- `apply_task_changes` re-runs `refresh_active_list` on every delta when the active view is a perspective (filter-expression visibility problem; same reasoning as `task_matches`).
- `apply_library_changes` falls back to Today when the active perspective is in `perspectives_deleted`.
- `prompt_rename_active` / `prompt_delete_active` gain `Perspective` arms with copy that mentions filter expressions explicitly so users understand delete only removes the saved view.
- `prompt_save_perspective` (new) drives the *Save Search as Perspective…* primary menu item: only fires on a `SearchResults` view with a non-empty trimmed query; switches the active list to the new perspective on success so the user sees the saved view immediately.

`build_primary_menu` adds the *Save Search as Perspective…* item to the library section.

### Tests

96 atrium-core tests pass (3 new perspective worker tests; 4 existing schema tests updated for the new `perspective` table + `user_version = 2`).

Five existing schema-introspection tests updated:

- `migration_applies_cleanly` and `migration_is_idempotent` now expect `user_version = 2`.
- `all_user_tables_exist` now expects `["area", "heading", "perspective", "project", "tag", "task", "task_tag"]`.
- `open_creates_parent_dir_and_migrates` and `acquire_release_round_trips` updated to expect version 2.
- `task_list::tests::builder_stub_titles_render` updated for the renamed variant.
- `task_list::tests::builder_stubs_report_themselves` and friends updated to reflect that `is_builder_stub()` is now always `false`.

`mode_flip_does_not_touch_db` integration test still passes (mode-as-view stays a UI-layer flag; no schema or row mutations on flip).

### Phase 14 follow-up deferred

JSON export / import of saved perspectives is deferred to Phase 16 (Export discipline) where it can ride on the file-format work for the rest of the workspace. The schema and worker plumbing are in place; only the I/O is pending.

## v0.1.16 (2026-05-07) — Phase 13: Review queue

Builder Mode's GTD discipline lands. Projects with a `review_interval_days` set surface in the Review sidebar entry when their last review is older than the interval allows. Each card shows the project's title, area, "Last reviewed N days ago" subtitle, and a Mark Reviewed button that stamps `last_reviewed_at = now()` and drops the row out of the queue.

The Phase 10 Review stub ("lands in Phase 13") is gone — selecting Review now lands on the real page.

### Data layer

`atrium-core/src/db/read.rs::list_review_queue(conn, today)` — SELECT with three filters:

- `review_interval_days IS NOT NULL` (the user opted in to reviewing this project),
- `archived_at IS NULL` (don't review archived projects),
- `last_reviewed_at IS NULL OR date(last_reviewed_at, '+' || review_interval_days || ' days') <= ?today`.

Order: `CASE WHEN last_reviewed_at IS NULL THEN 0 ELSE 1 END, last_reviewed_at ASC, position`. Never-reviewed projects sort first (highest priority — they've been waiting since creation); then oldest review next; manual `position` ordering breaks ties.

`atrium-core/src/db/command.rs` — new `Command::MarkReviewed { id }` variant.

`atrium-core/src/db/worker.rs::mark_reviewed(id)` — `UPDATE project SET last_reviewed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1`. Emits `LibraryChanges{projects_updated}` so the UI rebuilds the review page (and the row drops out).

`atrium-core::WorkerHandle::mark_reviewed(id) -> Result<Project, DbError>` — async API.

Three new SQL tests cover never-reviewed inclusion, overdue inclusion, ordering. Two new worker tests round-trip the timestamp + verify NotFound on unknown id.

### UI

New module `atrium/src/ui/review.rs` (~250 lines):

- `pub fn build_page(today, queue, area_titles, worker)` returns a scrollable column of project cards. Empty queue swaps to an `AdwStatusPage` "All caught up" placeholder.
- Each card: project title (heading), subtitle (`"<Area> · Last reviewed N days ago"` or `"Never reviewed"`), Mark Reviewed `suggested-action` button.
- `format_last_reviewed` helper handles all the day-diff cases: `today`, `1 day ago`, `N days ago`, `Never reviewed`. Six new unit tests cover the formatter and subtitle composition.
- The Mark Reviewed button disables itself while the worker call is in flight to prevent double-fires.

### Window integration

- `data/window.ui` — `content_stack` gains a `review` `GtkStackPage` with an `AdwBin id="review_host"` that the window mounts the freshly-built page into.
- `atrium/src/ui/window.rs::refresh_review_page` — pulls `list_review_queue` via the read pool, builds the page, mounts it.
- `refresh_active_list` special-cases `ActiveList::Review` to call `refresh_review_page` and switch the stack to `"review"` instead of falling through to the empty placeholder.
- `apply_library_changes` already calls `refresh_active_list` on every library delta, so a Mark Reviewed click triggers a page rebuild that drops the row visibly. No additional wiring needed.

### CSS

```css
.atrium-review-card {
    border: 1px solid alpha(@accent_bg_color, 0.25);
}
.atrium-review-card-title {
    font-size: 1.0em;
}
```

Subtle accent border draws attention to each card without being loud. The Mark Reviewed button uses libadwaita's `suggested-action` styling so its prominence matches the card's.

### Out of Phase 13 (deferred)

The roadmap's *per-area review schedules* item — having an area default a review interval that new projects inherit — adds a column to the `area` table. Per spec §4.4 the v0.1 schema is frozen; backwards-compatible migrations begin at v0.2. Since the per-project interval (Phase 11's SpinButton) already gives users full control, this is a quality-of-life nicety we can land cleanly when we tag v0.2.0 and unlock the migration path. Roadmap entry stays open with the explanation.

### Numbers

- **180 tests** pass (was 168). +6 review.rs unit tests, +3 list_review_queue SQL tests, +2 mark_reviewed worker tests, +1 carry-over.
- `cargo fmt` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 180 tests

### Try it

```bash
cargo run -p atrium

# Mode → Builder.
# Pick a project, set its Review interval (days) via the project-page
#   SpinButton — say 7.
# Sidebar → Review.
# The project shows up in the queue.
# Click Mark Reviewed. The card disappears (next review fires in 7 days).
# Set another project's interval to 0 — it surfaces every time you
#   open Review.
```

### What didn't change

- Schema (every column was already in `0001_initial.sql`'s superset).
- Single-writer worker, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Simple Mode unchanged — Review hides when `mode = simple`.
- Quick Entry, FTS5, multi-select, undo, Inspector, Forecast — every Phase 4–12 surface unchanged.

### What's next

Phase 14 — Perspectives (saved filter expressions). Filter expressions become first-class objects users can name and save (e.g., "Q3 work overdue" = `tag:work due:overdue`). The Phase 10 Perspectives sidebar stub becomes the real surface.

`VERSION`: 0.1.15 → 0.1.16 (patch — Phase 13 Review queue; one new SELECT, one new Command, ~250 lines of UI).

## v0.1.15 (2026-05-07) — Listen to GtkListView::activate for fast double-clicks

Brandon's v0.1.14 trace showed slow double-clicks working but **fast** ones registering only as a single click:

> "When I double click really fast, it's only show one response in the output log."

The trace confirmed only one `n_press=1` released event reached our row-level gesture for fast doubles. Triple-clicks and slower doubles fired multiple released events fine.

### Root cause

GtkListView wires its own internal `GtkGestureClick` to fire the `activate` signal on double-click (with `single-click-activate=false`). On a fast double-click — clicks within `gtk-double-click-time` — that internal gesture **claims the event sequence**, which prevents our row-level gesture from seeing the second click's release. GTK's gesture-claim mechanism is the documented way for nested gestures to coordinate, and ListView always wins for double-click activation.

The per-row gesture's Capture phase fires correctly *during* the first click; the second click's release never reaches us because by then ListView's gesture has claimed the sequence.

### Fix

`atrium/src/ui/window.rs::init_list_view` adds:

```rust
self.imp().task_list_view.connect_activate(move |_lv, _pos| {
    let Some(win) = win_weak.upgrade() else { return };
    glib::idle_add_local_once(move || {
        win.start_edit_focused_row();
    });
});
```

This listens to *exactly* the signal GtkListView fires after claiming a fast double-click. After GTK's selection logic settles (idle defer, same pattern as the row gesture), we grab focus on the entry via `start_edit_focused_row` — which uses the row that GTK has already focused for us as part of its activate flow.

### Two paths now cover the spectrum

- **Fast doubles** (clicks within `gtk-double-click-time` ~250–400 ms): handled by `GtkListView::activate`. GTK claims the sequence; we listen to its activate signal.
- **Slow doubles** (clicks outside the GTK threshold but within our 800 ms time-window): handled by the per-row Capture-phase gesture. GTK's internal gesture sees them as separate single clicks, doesn't claim, our row-level handler matches them.

### Why the per-row gesture stays

Some users (Brandon included) genuinely double-click slowly enough that GTK's window expires between clicks. Our 800 ms window catches those. Without it, slow doubles wouldn't fire at all.

### Tracing

Both paths emit debug logs (`row activate-gesture released …` and `list_view activate signal`), so the next bug report tells us which path failed.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests unchanged.

`VERSION`: 0.1.14 → 0.1.15 (patch — fast double-click fix via GtkListView::activate).

## v0.1.14 (2026-05-07) — Defer edit-start to idle so the editor stays open

The v0.1.13 trace was the smoking gun:

```
row activate-gesture released n_press=1 is_double_click=true already_editing=false
start_edit_on_row has_class=true has_stack=true has_label=true has_entry=true
row activate-gesture: start_edit_on_row returned did_edit=true
... 1.1s later ...
row activate-gesture released n_press=1 is_double_click=false already_editing=false
```

`already_editing=false` on the next click — meaning the editor opened (`did_edit=true`) but had already closed by the time the next click arrived. **The editor was opening then immediately closing.**

The cause: GtkListView's internal click handler grabs focus on the activated row's `GtkListItemWidget` *after* our gesture's released callback runs. So our flow was:

1. Our gesture fires; `start_edit_on_row` switches the stack to `edit`, calls `entry.grab_focus()`.
2. The click event continues propagating.
3. GtkListView's selection click handler runs; grabs focus on the row.
4. The entry's `EventControllerFocus::connect_leave` fires (it just lost focus).
5. The leave handler sees the stack is on `edit`, commits the (unchanged) text, switches stack back to `display`.
6. User sees nothing.

### Fix

`atrium/src/ui/task_list.rs` — the activate gesture's edit-start now runs via `glib::idle_add_local_once` instead of inline:

```rust
glib::idle_add_local_once(move || {
    crate::ui::window::start_edit_on_row(&widget);
});
```

The idle callback runs after the current event finishes propagating. By the time we grab focus on the entry, GtkListView has already finished its focus dance and isn't going to steal back. Our `grab_focus` is the last focus operation; the entry stays focused; the editor stays open.

### Tracing

`focus-leave` on the title entry now logs `task_id` and `visible_child` — so the next time someone reports an editor that opens-then-closes, the trace tells us whether the leave fired and what the stack saw.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests unchanged.

`VERSION`: 0.1.13 → 0.1.14 (patch — defer edit-start to idle so GtkListView's focus dance finishes first).

## v0.1.13 (2026-05-07) — Double-click hardening

Brandon's v0.1.12 trace showed the time-window match working — every is_double_click=true case landed cleanly with did_edit=true. But he reported "twice in succession acted as one click." Two likely causes weren't visible in that trace:

### Re-entry while already editing

After a successful double-click opens the title editor, the GTK Entry has focus and accepts text. But the row's Capture-phase gesture is still listening to clicks anywhere within the row's hit-area — including clicks on the Entry itself. A stray follow-up click within the time window would re-fire `start_edit_on_row`, reset the Entry to the original title, and bounce the cursor — looking like the editor "didn't open" because the user's typing got blown away.

Fix: gate the gesture's match on the title stack's current page. If it's already showing the `edit` page, skip the match entirely.

### Window slightly too tight

The 700 ms window was based on the trace data (clicks in the 614–559–434 ms range), but Brandon's natural cadence sometimes pushes past 700 ms. Bumped to 800 ms — generous for any user, still well below the threshold where two distinct single-click intents would accidentally collapse into a double.

### What changed

`atrium/src/ui/task_list.rs` — the activate gesture's released handler:

- Time window: 700 ms → 800 ms.
- New `already_editing` check reads `atrium-title-stack`'s `visible_child_name`. If `"edit"`, skip the match.
- Tracing now also logs `already_editing` so the next bug report has even more data.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests unchanged.

`VERSION`: 0.1.12 → 0.1.13 (patch — double-click hardening; window widened, re-entry guarded).

## v0.1.12 (2026-05-07) — Double-click really fires this time

The v0.1.11 trace was the gold:

```
row activate-gesture released n_press=1
row activate-gesture released n_press=1     ← 720 ms after click 1
row activate-gesture released n_press=3     ← rapid clicks finally counted
row activate-gesture released n_press=4
...
```

GTK's `n_press` increments only when consecutive clicks fall within `gtk-double-click-time` (default ~400 ms). Brandon's natural double-click cadence on his ThinkPad trackpad is ~700 ms — well outside that window. So the system was treating each of his "double-clicks" as two independent single-clicks (each `n_press = 1`), and our `n_press == 2` check never matched.

### Fix

`atrium/src/ui/task_list.rs` — the activate gesture now ignores GTK's `n_press` and runs its own time-window detection:

```rust
let last_release: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
activate_gesture.connect_released(move |gesture, _, _, _| {
    let now = Instant::now();
    let prev = last_release.replace(Some(now));
    let is_double_click = prev.is_some_and(|p|
        now.duration_since(p) <= Duration::from_millis(700)
    );
    if is_double_click { … start_edit_on_row … }
});
```

700 ms is generous — every legitimate double-click matches comfortably; single clicks never accidentally count as doubles. Reset on match so a third click within the window doesn't re-trigger.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests unchanged.

The diagnostic tracing from v0.1.11 stays in place; if a future user reports the same issue we can confirm the timing immediately.

`VERSION`: 0.1.11 → 0.1.12 (patch — double-click detection now matches user cadence, not GTK's tight default).

## v0.1.11 (2026-05-07) — Double-click capture + diagnostics

Brandon reported the v0.1.10 double-click → inline-edit change still wasn't firing. Two changes here to chase it:

### Capture-phase gesture

`atrium/src/ui/task_list.rs` — the per-row activate gesture now sets `propagation_phase = PropagationPhase::Capture`. The Bubble default fires on the way up, *after* ancestor widgets have processed the event. The parent `GtkListItemWidget` has its own `GtkGestureClick` for selection handling, and on this configuration it appears to be consuming the second click of a double-click before our gesture sees it. Capture fires on the way *down*, so our handler runs first and the parent's selection logic still gets to run after.

### Diagnostic tracing

Both the gesture handler and `start_edit_on_row` now emit `tracing::debug!` lines:

- `row activate-gesture released n_press=N` — fires every time the gesture's release handler runs. If we see this with `n_press=2`, the gesture fired correctly. If we see it only with `n_press=1` (or never), the gesture isn't reaching the double-click case.
- `start_edit_on_row has_class=… has_stack=… has_label=… has_entry=…` — fires every time `start_edit_on_row` is called. If `has_class=false`, the widget passed in isn't actually the row Box. If any of the data slots are missing, the bind path didn't stash them correctly.
- `row activate-gesture: start_edit_on_row returned did_edit=true|false` — confirms whether the start-edit succeeded.

To get the trace, run with `RUST_LOG=atrium=debug` and double-click a task. Paste the output into the next bug report and we'll know exactly which branch is misbehaving.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests unchanged.

`VERSION`: 0.1.10 → 0.1.11 (patch — capture-phase gesture + diagnostic tracing).

## v0.1.10 (2026-05-07) — Double-click → inline title edit

Brandon flagged that the v0.1.9 fix didn't help (double-click was still broken in Simple Mode), and pivoted to a different model entirely: double-click should rename the task in place; the Inspector stays a separate affordance.

That's the Things-3 idiom — double-click renames inline; the full editor needs an explicit "i" or right-click. Lighter, less modal, and dodges the GSettings-mode-read race entirely (the new path doesn't ask which mode we're in — F2 and double-click both just flip the title stack into edit mode).

### Interaction model now

| Gesture | Action |
|---|---|
| Single click | Select row + hold focus (`gtk::MultiSelection`). |
| Double click | Enter inline title edit (same as F2). |
| F2 | Enter inline title edit (existing). |
| Ctrl+I | Open Inspector (modal in Simple, side-pane focus in Builder). |
| Right-click → *Edit Details…* | Open Inspector. |
| Right-click → *Edit Tags…* | Open tag editor. |
| Space | Toggle complete on the focused row. |
| Delete | Delete the focused task. |

### What changed

`atrium/src/ui/task_list.rs` — the per-row primary-button double-click gesture stops calling `widget.activate_action("win.edit-details-for", …)` and starts calling `crate::ui::window::start_edit_on_row(&widget)` directly. Same code path F2 uses; same row-data stash (`atrium-title-stack` / `-label` / `-entry`).

`atrium/src/ui/window.rs::start_edit_on_row` — visibility bumped from private to `pub` so `task_list` can call it. The function flips the title `GtkStack` to its "edit" page, populates the `GtkEntry` from the bound display label, grabs focus + select-all.

`docs/keymap.md` — *List actions* section gains the explicit `Double-click` row pointing at inline title edit.

### Bonus side effect

The double-click path no longer reads the GSettings `mode` (because it doesn't decide between modal and side-pane anymore — both modes just edit inline). So even if every other mode-aware code path is broken on Brandon's GSettings backend, double-click will still work.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests unchanged.

`VERSION`: 0.1.9 → 0.1.10 (patch — interaction-model refinement).

## v0.1.9 (2026-05-07) — Double-click opens the Inspector in Simple Mode

Brandon caught a hangover from v0.1.6: double-clicking a task in Simple Mode did nothing. Same root cause as the v0.1.6 selection-pane bug — `open_inspector_for` was reading `self.settings().string("mode") == "builder"` to decide between the modal dialog (Simple) and the side-pane focus (Builder). On the dconf-wrapper-cross issue we documented in v0.1.6/v0.1.7, that read returned a stale "builder" even after the user flipped back to Simple. The dialog branch never fired; the side-pane branch tried to populate the (hidden) pane and grab focus on its (hidden) title row. Visually nothing happened.

The v0.1.6 patchnotes acknowledged the other call sites kept using GSettings reads and flagged it as a follow-up cleanup. Brandon's report turned that into a real bug — same race, different surface.

### What changed

`atrium/src/ui/window.rs` — every same-frame mode read now consults `current_mode_is_builder.get()` instead of `self.settings().string("mode")`:

- `refresh_dynamic_badges` (sidebar count formatter — sequential project shows available count vs open count).
- `rebuild_dynamic_sidebar` (decides whether to append the Builder section header + Forecast/Review/Perspectives entries).
- `set_active_list` (project extras revealer visibility on Project view selection).
- `open_inspector_for` (dialog vs side-pane routing for Ctrl+I, double-click, right-click → Edit Details).

The two remaining GSettings reads are the canonical ones — `attach_data_layer` reads the initial persisted mode at startup, and `install_mode_observer`'s callback reads from the signal-emitting Settings instance when an external write fires. Both flow into `apply_mode`, which writes the Cell. The Cell is the only same-frame source of truth.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests unchanged.

`VERSION`: 0.1.8 → 0.1.9 (patch — UI bug fix; the rest of the same-frame mode reads migrated to the Cell).

## v0.1.8 (2026-05-07) — Selection bar only for true bulk

The bulk-action toolbar that previously appeared the moment a row was clicked is now gated on `n >= 2`. Single-row selection had four redundant ways to do the same thing:

- The row's own checkbox to toggle complete.
- The `Space` key to toggle complete.
- The `Delete` key to delete the focused task.
- The toolbar's Complete + Delete buttons (the redundant ones).

The toolbar earns its keep when bulk ops are actually available — selecting two rows or more, where the per-row checkbox can't operate on the whole set. So that's when it shows now.

### What changed

`atrium/src/ui/window.rs::update_selection_bar`:

- Reveals the `selection_revealer` when `n >= 2`. Hides it for `n == 0` (no selection) and `n == 1` (single-row selection covered by the per-row affordances).
- Doc comment cites the rationale inline so this isn't re-discovered as a regression next time someone audits the chord map.

### What didn't change

- `Ctrl+A` still selects every row in the active list. After it, the toolbar reveals (since `n >= 2` for any non-trivial list).
- `Esc` still clears multi-selection via the `win.bulk-clear` action wired to the task list's shortcut controller. The toolbar's `×` button (still visible at `n >= 2`) targets the same action.
- The bulk worker handlers (`bulk_complete_selection`, `bulk_delete_selection`) are unchanged. They iterate over `selected_task_ids()` regardless of count; the toolbar just isn't the path to them at `n == 1`.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests unchanged.

`VERSION`: 0.1.7 → 0.1.8 (patch — UI cleanup; selection bar reveal threshold).

## v0.1.7 (2026-05-07) — Mode flip lands synchronously

The v0.1.6 trace Brandon ran nailed the actual root cause of the v0.1.5/v0.1.6 flap:

```
INFO atrium: mode switched mode=builder              ← action handler ran
(no apply_mode log here)                             ← observer never fired
DEBUG atrium::ui::window: refresh_inspector_pane: simple mode → clear
```

The action handler in `main.rs` was writing the GSetting (`settings.set_string("mode", "builder")`), but the window-side `connect_changed` handler that should have been routing the change into `apply_mode` **didn't fire**. So the Cell tracker stayed at `false`, the OverlaySplitView stayed hidden, and `refresh_inspector_pane` correctly observed Simple Mode and bailed.

Two `gio::Settings::new(APP_ID)` wrappers were involved — one in the action handler, one in the observer. `gio::Settings` instances for the same schema/path *should* share a backend and propagate the changed signal across all instances, but on this dconf backend the same-process write wasn't crossing wrapper boundaries reliably.

### Fix

`atrium/src/main.rs::install_mode_action`:

- After `settings.set_string("mode", &value)` succeeds, the action handler now calls `window.apply_mode(&value)` directly via `app.active_window().and_downcast::<AtriumWindow>()`. The UI rerender lands synchronously on the same frame as the menu click — no GSettings round-trip in the path.

`atrium/src/ui/window.rs::install_mode_observer`:

- Stays in place as a *safety net* for external GSettings writes (dconf-editor, another process, automated tooling). When external writes happen, the observer fires and routes through `apply_mode`. Same-process writes from the menu now bypass the observer entirely. `apply_mode` is idempotent — a duplicate call from the observer (if the dconf backend ever does cross the signal across wrappers) is harmless.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests unchanged.

The v0.1.5+v0.1.6 belt-and-suspenders work isn't reverted — it stays as defense in depth. The synchronous `apply_mode` call from the action handler is the new primary path; the Cell tracker is still synchronously written; the explicit `set_visible` + `pane.clear()` in `apply_mode` still hold. Three layers of redundancy now resolve mode flips correctly.

`VERSION`: 0.1.6 → 0.1.7 (patch — UI bug fix; mode action wires apply_mode directly).

## v0.1.6 (2026-05-07) — Inspector pane populates reliably in Builder

Brandon caught the inverse of v0.1.5: now the side pane stays empty (`No task selected`) when a row is actually selected in Builder Mode. The v0.1.5 fix added a mode gate at the top of `refresh_inspector_pane`:

```rust
if self.settings().string("mode") != "builder" {
    pane.clear();
    return;
}
```

That GSettings round-trip was sometimes returning a stale value on the same frame as a mode flip — the Cell- vs- read-through-backend timing isn't quite synchronous from the perspective of a callback that fires while the menu action is still in flight. The pane host's visibility was getting set to true (via `apply_mode`'s explicit `set_visible(builder)` belt added in v0.1.5), but the immediately-following selection-changed fire was reading "simple" from GSettings and bailing with `pane.clear()` — leaving the visible host stuck on the empty-state placeholder.

### What changed

`atrium/src/ui/window.rs::imp::AtriumWindow`:

- New `current_mode_is_builder: Cell<bool>`. Synchronous mode tracker; `apply_mode` is the single writer.

`atrium/src/ui/window.rs::apply_mode`:

- Writes `current_mode_is_builder.set(builder)` first thing — before any of the visibility setters or sidebar rebuilds run. Any callback that races into this method's body while it's still executing observes the new mode.

`atrium/src/ui/window.rs::refresh_inspector_pane`:

- Reads from `current_mode_is_builder.get()` instead of `self.settings().string("mode")`. Sidesteps the same-frame staleness entirely.
- Adds `tracing::debug!` at every branch — pane-missing, simple-mode, no-selection, multi-selection, same-task-noop, set-task-fired. The next mode-flip behaviour report comes with data: run with `RUST_LOG=atrium=debug` and the trace tells you exactly which branch each call lands in.

### Other reads of GSettings `mode`

The other call sites (`refresh_dynamic_badges`, `set_active_list`, etc.) keep reading from `self.settings().string("mode")` rather than the Cell. They run on user-initiated events (menu open, list switch) that don't race with `apply_mode` itself, so the round-trip is safe there. We could migrate them to the Cell for consistency, but that's a "no behaviour change" cleanup; flagging it as a follow-up rather than landing it speculatively.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests unchanged.

### Try it

```bash
RUST_LOG=atrium=debug cargo run -p atrium

# Mode → Builder. Click a task. The pane populates.
# Console shows: refresh_inspector_pane: set_task id=...
# Mode → Simple. Pane hides. Click a task in Simple Mode.
# Console shows: refresh_inspector_pane: simple mode → clear
# Mode → Builder. Click a task. The pane populates again.
```

`VERSION`: 0.1.5 → 0.1.6 (patch — UI bug fix; mode-flip race in the pane refresh).

## v0.1.5 (2026-05-07) — Inspector pane hides on Simple Mode flip

Brandon caught a follow-on bug after v0.1.4: switching from Builder Mode back to Simple left the Inspector side pane visible on the right, including its "Builder" group with the subtitle *"Fields exposed only in Builder Mode."* Visually inconsistent — Simple Mode users shouldn't see the pane at all, much less one that announces it's a Builder feature.

Two reasons it survived:

1. **`AdwOverlaySplitView::set_show_sidebar(false)` alone wasn't sufficient** to fully hide the pane host on every code path. The property toggles the visual state, but in some cases the AdwBin in the sidebar slot remained visible.
2. **`refresh_inspector_pane`** (the selection-changed handler that mirrors the active row into the pane) didn't gate on mode. So even if `apply_mode("simple")` had cleared the pane, the next single-row selection in Simple Mode repopulated it — invisible mostly, but very visible if reason 1 also kicked in.

### What changed

`atrium/src/ui/window.rs::apply_mode`:

- Adds `inspector_pane_host.set_visible(builder)` alongside the existing `overlay_split.set_show_sidebar(builder)`. Belt-and-suspenders — even if the OverlaySplitView's show-sidebar somehow doesn't propagate (which v0.1.4 user testing surfaced as a real failure mode), the host AdwBin is hidden directly.
- Calls `pane.clear()` on the InspectorPane when entering Simple Mode. Drops the per-task editor body so there's nothing visible inside even if the host happened to be visible.
- Adds a `tracing::debug!(mode, builder, "apply_mode")` log so the next time someone hits a "Builder Mode is sticky" report, the trace tells you whether `apply_mode` actually fired or whether the GSettings change observer dropped the event.

`atrium/src/ui/window.rs::refresh_inspector_pane`:

- Bails out early with `pane.clear()` when `mode != "builder"`. The pane stays empty in Simple Mode regardless of selection changes; on a flip back to Builder, the next selection repopulates it correctly.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests unchanged.

`VERSION`: 0.1.4 → 0.1.5 (patch — UI bug fix; mode-flip pane visibility).

## v0.1.4 (2026-05-07) — Builder-Mode Inspector chord fix

Brandon hit a real bug: pressing `Ctrl+I` on a task in Builder Mode did nothing. Tracing it back, the v0.1.1 Phase 10 implementation had `open_inspector_focused` short-circuit when `mode = builder`, on the rationale "the side pane already shows the editor; opening another one would be redundant." That was a wrong design call.

The user's mental model of Ctrl+I is *"get me into the editor for this task"* — not *"open a redundant second editor on top of the first."* When the chord does nothing, Builder Mode feels broken. The fix routes Ctrl+I (and the matching double-click and right-click → *Edit Details…* gestures) through to the side pane in Builder Mode and *focuses the title row* so the user can immediately type. Simple Mode keeps opening the modal dialog as before — its analogue is the `title_row.grab_focus()` call at the bottom of `inspector::open`.

### What changed

`atrium/src/ui/inspector_pane.rs`:

- `InspectorPane` gains a `current_title_row: RefCell<Option<adw::EntryRow>>` field.
- `build_editor` returns `(gtk::Widget, adw::EntryRow)` — body plus the title row — so `set_task` can stash the title for later focus.
- `set_task` populates `current_title_row` on every rebuild; `clear()` resets it.
- New `pub fn focus_title(&self)` grabs focus on the current editor's title `EntryRow` and selects-all on its delegate (matching the modal Inspector's grab + select pattern). No-ops when no task is currently displayed.

`atrium/src/ui/window.rs::open_inspector_for(task_id)`:

- Now mode-aware. Builder Mode: re-populates the side pane if the requested task isn't the one currently shown (e.g., a right-click on a row outside the current selection), then calls `pane.focus_title()`. Simple Mode: opens the modal dialog as before.
- All three editor entry points fan in here:
  - **`Ctrl+I`** via `open_inspector_focused` — now a thin wrapper over `open_inspector_for(focused_id)`.
  - **Double-click** via the per-row gesture in `task_list::build_factory` → `win.edit-details-for(i64)` action → `open_inspector_for(id)`.
  - **Right-click → *Edit Details…*** via the same `win.edit-details-for` action target.
- All three behave consistently in Builder Mode now: focus the side pane title row, ready to edit.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests unchanged.

### What didn't change

Schema, single-writer worker, every other Phase 4–12 surface. The fix is scoped to the editor-focus routing.

`VERSION`: 0.1.3 → 0.1.4 (patch — bug fix; retracts the v0.1.1 "Ctrl+I is a no-op in Builder" design call).

## v0.1.3 (2026-05-07) — Phase 12: Forecast view

OmniFocus's signature view, ported. Builder Mode now has a 30-day calendar-axis layout — a scrollable column of day cards that surfaces every task touching the window via `scheduled_for`, `deadline`, or `defer_until`. The Phase 10 Forecast stub ("lands in Phase 12") falls away; selecting Forecast in the sidebar now lands you on the real page.

### Data layer

- `atrium-core/src/db/read.rs::list_forecast(conn, today, days)` — selects every open task whose scheduled / deadline / defer_until lands in `[today, today + days]`. Someday excluded.
- `atrium-core/src/db/read.rs::list_overdue(conn, today)` — selects open tasks with scheduled or deadline strictly before today, excluding tasks deferred to a future date (those aren't actionable yet).
- Both queries return `Vec<Task>`; the UI groups by date in Rust via the pure helper `forecast::group_by_date(tasks, today, days)`.
- Four new tests cover the boundaries: forecast picks up scheduled / deadline / defer-ends in window; forecast excludes overdue; overdue picks up late scheduled / deadline; overdue excludes deferred-future. Plus five new tests for `group_by_date` and the day-title formatter.

### UI

- New module `atrium/src/ui/forecast.rs` (~400 lines). Public surface:
  - `pub const FORECAST_WINDOW_DAYS: i64 = 30` — single source of truth for the window length.
  - `pub fn build_page(today, forecast_tasks, overdue_tasks, worker) -> gtk::Widget` — assembles the entire scrollable column.
  - `pub fn group_by_date(tasks, today, days) -> Vec<(NaiveDate, Vec<DayEntry>)>` — pure function, the unit of testing.
- Each day card is an `card`-classed `GtkBox` with a `heading`-styled title and one row per `DayEntry`. The header line promotes `Today · Wed May 7` and `Tomorrow · Thu May 8` for the first two days; later days show weekday + date.
- Each row inside a card is `[reason chip] [title (ellipsised)]`. The reason chip uses one of three colour-shifted styles — `atrium-forecast-reason-scheduled` (accent), `atrium-forecast-reason-deadline` (destructive), `atrium-forecast-reason-defer` (warning) — so a quick scan tells you why the row is there.
- The Overdue pseudo-block sits above the day cards. Counts overdue tasks; when zero, a "Caught up." subtitle replaces the row list.

### Window integration

- `data/window.ui` — `content_stack` gains a third `GtkStackPage "forecast"` hosting an `AdwBin id="forecast_host"`. The window builds a fresh forecast widget every refresh and parents it into the bin.
- `atrium/src/ui/window.rs::refresh_forecast_page` — pulls forecast + overdue tasks from the read pool, calls `forecast::build_page`, mounts the result.
- `refresh_active_list` now special-cases `ActiveList::Forecast` to call `refresh_forecast_page` and switch the stack to `"forecast"` instead of running a list query. Review and Perspectives still hit the empty-state path.
- `apply_task_changes` re-renders the forecast page on every `TaskChanges` when the active view is Forecast — the page rebuilds in full rather than diff-applying, since day-card layout depends on date grouping that's cheaper to recompute than to track in place.

### Drag-to-reschedule

- Every forecast row carries a `GtkDragSource` carrying the task id (i64).
- Every day card carries a `GtkDropTarget` that on drop fires `worker.update_task(TaskUpdate::new(id).schedule(Some(ScheduledFor::Date(target))))` against the card's date.
- The worker write returns a `TaskChanges` delta, which the bridge applies via `apply_task_changes` → `refresh_forecast_page`, and the row visibly moves to the destination card.
- Dropping on the Overdue block is intentionally a no-op (overdue is a consequence of dates, not a target date).

### Today indicator + overdue surfacing

- `.atrium-forecast-day.today` — accent-coloured 1px border + accent-coloured heading. The user's anchor in time.
- `.atrium-forecast-overdue` — destructive-accent border + destructive-coloured heading. Reads as "this needs attention" without being noisy when empty.

### CSS

`data/style.css` gains:

```css
.atrium-forecast-day.today { border: 1px solid alpha(@accent_bg_color, 0.6); }
.atrium-forecast-day.today > box > label.heading { color: @accent_color; }
.atrium-forecast-overdue { border: 1px solid alpha(@destructive_bg_color, 0.45); }
.atrium-forecast-overdue > box > label.heading { color: @destructive_color; }
.atrium-forecast-reason { ...chip shape, font-size 0.85em, radius 6px... }
.atrium-forecast-reason.atrium-forecast-reason-scheduled { accent palette }
.atrium-forecast-reason.atrium-forecast-reason-deadline { destructive palette }
.atrium-forecast-reason.atrium-forecast-reason-defer { warning palette }
```

All accent colours pull from libadwaita's variables; light / dark / `prefer-contrast: more` all follow the platform.

### What didn't ship

The roadmap's Phase 12 *compact / expanded toggles* item is deferred. The dense layout is the Phase 12 default; per-card compact / expanded as a user-toggleable preference needs per-card state plus a header control, and is worth its own follow-up. Roadmap entry kept open with a note to that effect.

### Numbers

- **168 tests** pass (was 158). +5 forecast UI helper tests, +4 atrium-core integration / unit tests for the new SQL.
- `cargo fmt` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 168 tests

### Try it

```bash
cargo run -p atrium -- --debug --fixture medium

# Mode → Builder.
# Sidebar → Forecast.
# Scroll the day cards. Today's gets an accent border; the Overdue
#   block at the top shows everything past-due.
# Drag a row from one day to another — it moves, the row's
#   scheduled_for updates, the badge counts shift.
# Click any task's reason chip to see the original row in the
#   sidebar list (future polish: chip → focus row).
```

### What didn't change

- Schema (`0001_initial.sql` — Phase 1 superset is the contract; v0.1.3 only added two new SELECTs).
- Single-writer worker, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Simple Mode is identical — Forecast is hidden when mode = simple. The mode-flip snapshot test still passes.
- Quick Entry, FTS5 search, multi-select, undo, Inspector, Builder side pane — every Phase 4–11 surface unchanged.

### What's next

Phase 13 — Review queue. Projects whose `last_reviewed_at + review_interval_days ≤ today` surface in a Review list, oldest first. The Phase 11 review-interval picker on the project page already writes the column; Phase 13 turns it into a queue.

`VERSION`: 0.1.2 → 0.1.3 (patch — Phase 12; new view on top of two new SELECTs, no schema changes).

## v0.1.2 (2026-05-07) — Phase 11: defer dates + sequential project rendering

The OmniFocus mechanics that turn the Builder Mode shell from a stage set into something you can actually run. v0.1.1 wired the side pane and surfaced the Builder fields; v0.1.2 makes `defer_until` and `sequential` matter.

### Defer-until editor

- `atrium-core/src/domain/mod.rs::TaskUpdate` gains `defer_until: Option<Option<NaiveDate>>` and a `defer_value(Option<NaiveDate>)` builder method. Same `Some(None)` / `Some(Some(date))` semantics as the existing schedule + deadline fields.
- The worker's `update_task` SQL builder picks up the new field — one extra `if let Some(defer_until) = update.defer_until { sets.push("defer_until = ?"); … }` arm.
- Both Inspectors get the editor:
  - **Modal `inspector.rs`** — adds a Defer-until `AdwActionRow` with the same date popover used by Schedule and Deadline (`build_date_button` + the new `format_defer_label` helper that says *"Available now"* when the field is null instead of *"No deadline"*). Apply diffs include `defer_until` alongside the other columns.
  - **Side pane `inspector_pane.rs`** — the disabled "Editor lands in Phase 11" placeholder is gone; a real `MenuButton` popover replaces it. Auto-save on popover commit, mirroring how Schedule and Deadline work.
- Two new core tests: `update_task_sets_and_clears_defer_until` (set / clear round-trip) and `update_task_sets_and_clears_estimated_minutes` (the carryover from Phase 10).

### List-filter logic — already in place

The Today and Anytime SQL queries have filtered `defer_until > today` since Phase 4 (`atrium-core/src/db/read.rs::list_today` + `list_anytime`). The predicate was correct; it just had no editor wired to surface the column. With Phase 11 the editor's live, and the existing tests (`today_excludes_deferred_to_future`, `anytime_excludes_future_deferred`, `today_includes_deferred_now_active`) finally have a real user-facing path that exercises them. No SQL changes.

### `estimated_minutes` — wired

`TaskUpdate` gains `estimated_minutes: Option<Option<i64>>` and an `estimated_minutes_value(Option<i64>)` builder. The Phase 10 inspector-pane SpinRow that lived but didn't dispatch now commits via `worker.update_task(TaskUpdate::new(id).estimated_minutes_value(_))` on every value-changed event. 0 clears the column; any positive integer sets it.

### Sequential project rendering

- `AtriumTask` (`atrium/src/ui/task_object.rs`) gains a `queued` `glib::Property`. The factory mirrors it to a `.queued` CSS class on the row and observes `connect_queued_notify` so already-bound rows update when the head row gets completed and the next one is promoted to "available".
- `data/style.css` adds:
  ```css
  .atrium-task-row.queued {
      opacity: 0.45;
  }
  .atrium-task-row.queued .atrium-task-title {
      font-style: italic;
  }
  ```
  Plus a doc comment explaining why we don't disable the CheckButton (toggling the head row is how you advance through a sequential project — completing the head promotes the next).
- `task_list::compute_queued_state(tasks, sequential)` is the pure helper: given a task list and a sequential bool, returns one bool per task. The first incomplete task is unqueued; the rest are queued; completed tasks are never queued (the `.completed` fade already dims them).
- `replace_store_with_tags_seq(store, tasks, tag_map, sequential)` replaces the prior `replace_store_with_tags`. Window calls it with `sequential = matches!(active, ActiveList::Project(id)) && project_meta.get(id).is_some_and(|p| p.sequential)`. Other views (Today, Inbox, Area aggregates) pass `false` and never dim rows.
- `apply_changes_seq` (the new diff applier) calls `recompute_queued_state` after the delta lands so toggling the head row demotes it and promotes the next in the same frame.
- Four new tests cover the helper: empty when not sequential, first-open-unqueued / rest-queued, skip-completed-for-first-open, all-completed-no-queue.

### Available-task count badges

- `available_count(open, sequential)` — pure helper in `window.rs`. Sequential projects clamp to 0 or 1 (head only); parallel projects show their open count.
- `refresh_dynamic_badges` reads the project metadata cache and applies the available-count translation when `mode == "builder"`. Simple Mode keeps showing open count (Simple Mode hides the Sequential toggle, so available ≡ open there anyway).
- Two new unit tests: `available_parallel_project_shows_open_count` and `available_sequential_project_caps_at_one`.

### Numbers

- **158 tests** pass (was 150). +6 binary tests (4 queued-state, 2 available-count badge math); +2 atrium-core tests (defer / estimated round-trip).
- `cargo fmt` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 158 tests

### Try it

```bash
cargo run -p atrium

# Mode → Builder.
# Click any task → side pane shows Defer-until row → click → Calendar → pick
#   a date 3 days out → row falls out of Today (and Anytime) until then.
# Pick a project in the sidebar → flip Sequential ON → the project's tasks
#   past the first incomplete one dim and italicise; the sidebar badge shrinks
#   from N to 1.
# Complete the head task → next row brightens and the badge stays at 1.
# Set Estimated Minutes to 30 → reopen the row → the SpinRow shows 30.
```

### What didn't change

- Schema (`0001_initial.sql` — Phase 1 superset is the contract; `defer_until` and `estimated_minutes` were already columns. v0.1.2 only added editors).
- Single-writer worker, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Simple Mode is identical to v0.1.1 — every Builder-only surface still hides when mode = simple. The mode-flip snapshot test still passes.
- Quick Entry, FTS5 search, multi-select, undo, sidebar filter — every Phase 4–9 surface unchanged.

### What's next

Phase 12 — Forecast view. The 30-day calendar-axis layout that gives Builder Mode its OmniFocus flavor. Vertical day blocks; each shows scheduled, deadlined, and deferred tasks for that day. Drag-to-reschedule between days writes `scheduled_for`. Today indicator + overdue surfacing. The Phase 10 Forecast stub becomes the real page.

`VERSION`: 0.1.1 → 0.1.2 (patch — Phase 11; one new TaskUpdate field + a few hundred lines of UI on top of the v0.1.1 shell).

## v0.1.1 (2026-05-07) — Phase 10: Builder Mode UI shell

The mode switch becomes real. Until v0.1.1, "Builder Mode" was a GSettings string with no visible consequence — flipping it changed nothing on screen because nothing observed the key. This release wires the observer, lands the right-side Inspector pane, surfaces the Builder-only sidebar sections (Forecast / Review / Perspectives) as stubs pointing at the phases that fill them in, and adds project-level controls for the Builder fields (`sequential` and `review_interval_days`) that have lived in the schema since Phase 1 unused.

Per the roadmap Phase 10 tagline — *no new logic, just exposure*. The Builder fields exposed here are columns that already existed; the lists, queues, and forecast calendar that consume them ship in Phases 11–14. v0.1.1 ships the *shell* for them.

### Mode observer

- `atrium/src/ui/window.rs::install_mode_observer` — subscribes to `gsettings.connect_changed("mode", …)` and routes every change through a single `apply_mode(&str)` method. The `app.mode::simple` / `app.mode::builder` stateful action (wired since Phase 3) writes the GSetting; the GSetting handler does the actual UI rerender. One source of truth, one re-render path.
- `apply_mode` toggles four surfaces: the right-side `AdwOverlaySplitView::set_show_sidebar` (the Inspector pane), the Builder-only sidebar entries (via a `rebuild_dynamic_sidebar` pass that conditionally appends them), the project-extras revealer (when the active list is a Project view), and a fallback to Today if the user was on a Builder stub view and flipped to Simple. Idempotent.
- The doc comment on `apply_mode` cites the spec §3.1 / spec §5.3 / CLAUDE.md commitment #1 contract: *flipping mode is a GSetting write plus a UI re-render, never a migration, never a DB write.* The transitive call set lists every method `apply_mode` reaches and confirms none holds a `WorkerHandle`. The only DB path is `ReadPool`, which `PRAGMA query_only = ON` makes write-impossible.

### Inspector side pane

- `data/window.ui` — the existing `AdwNavigationSplitView` is now wrapped in an `AdwOverlaySplitView` (`sidebar-position: end`, default `show-sidebar=false`). Right pane holds an `AdwBin id="inspector_pane_host"` that the new `inspector_pane.rs` module mounts into during `attach_data_layer`.
- `atrium/src/ui/inspector_pane.rs` (new file, ~470 lines) — `InspectorPane` struct with `install` / `set_task` / `clear` / `current_task_id` API. Inside, a `GtkStack` swaps between an `AdwStatusPage` empty state ("No task selected — select a row to edit it here.") and a per-task editor body assembled at selection time.
- The editor body mirrors the Phase 7i dialog Inspector's groups — Title (`AdwEntryRow`), Schedule + Deadline + Project (`AdwActionRow`s with the same date popovers / project `AdwComboRow`), Tags (with Edit Tags… button hand-off to the existing tag editor dialog), Notes (`GtkTextView` in a card-styled `ScrolledWindow`) — but adds a fifth group titled **Builder** containing:
  - `estimated_minutes` — live `AdwSpinRow` (0–1440 minutes, step 5). The setter is wired but currently doesn't dispatch (TaskUpdate doesn't yet expose `estimated_minutes` as a builder method); flagged for a Phase 10.5 follow-up.
  - `defer_until` — disabled placeholder row pointing at Phase 11 (which adds the editor and the Today/Anytime exclusion logic).
  - `repeat_rule` — disabled placeholder row pointing at Phase 15.
- **Auto-save semantics**: every field commits on focus-out and on Enter. Title rejects empty values (bounces back to the previous value); Notes commits via an `EventControllerFocus::connect_leave`. Project changes fire on `connect_selected_notify`. Schedule + Deadline pickers fire on the popover commit. No Apply button — the pane is non-modal, there's nothing to dismiss. Ctrl+Z still reverses any commit via the existing undo cell.
- `Ctrl+I` and double-click behave differently per mode: in Simple Mode they open the modal Inspector dialog like before; in Builder Mode the side pane is already showing the row, so the chord becomes a no-op (the user is already editing it).

### Builder-only sidebar entries

- `ActiveList` enum gains `Forecast` / `Review` / `Perspectives` variants plus an `is_builder_stub() -> bool` predicate.
- When `mode = builder`, `rebuild_dynamic_sidebar` appends a `Builder` section header followed by Forecast (icon `x-office-calendar-symbolic`), Review (`object-select-symbolic`), and Perspectives (`view-grid-symbolic`).
- Selecting a Builder stub bypasses the read-pool query path entirely (`is_builder_stub()` short-circuits `refresh_active_list`) and renders an `AdwStatusPage` placeholder citing the phase that ships the actual content (Phase 12 for Forecast, 13 for Review, 14 for Perspectives).
- `task_matches` returns `false` for all three so the diff applier never appends rows to a stub view.

### Project page extras

- `data/window.ui` adds a `GtkRevealer id="project_extras_revealer"` above the task list, holding a `toolbar`-styled `GtkBox` with a Sequential `GtkSwitch` and a Review-interval `GtkSpinButton` (0–365 days, step 1, page 7).
- `wire_project_extras` connects `connect_active_notify` and `connect_value_changed` to dispatch `worker.update_project(ProjectUpdate::sequential(value))` and `worker.update_project(ProjectUpdate::review_interval_days(value))`. A `project_extras_syncing` flag suppresses echoes during programmatic population — `populate_project_extras(id)` reads the cached `Project` metadata and sets the controls without re-firing the handlers.
- `atrium-core/src/domain/mod.rs::ProjectUpdate` gains a `review_interval_days(Option<i64>)` builder method (sequential's was already there). `Some(None)` clears the column; `Some(Some(days))` sets it.
- The revealer's visibility is gated by `ActiveList::Project(_) && mode == builder`. Switching off Builder Mode hides it; switching projects re-populates from the project metadata cache.

### Phase 10 acceptance — mode-flip snapshot test

- New integration test `atrium-core/tests/mode_flip_snapshot.rs` enforces the *no DB writes on mode flip* contract.
- The test populates the Small fixture (1,000 tasks across 50 projects in 5 areas with 20 tags) into a temp database, takes a row-by-row snapshot of every user table (`area`, `project`, `task`, `tag`, `task_tag`, `heading`), then exercises the read traffic a mode flip triggers via the `ReadPool`: `list_areas` + `list_projects` + `list_tags` + `list_today` + `count_open_canonical`. It then attempts a `DELETE FROM task` through the pool and asserts the write fails (the contract's architectural guard). Reopens the DB, snapshots again, asserts byte-identical state.
- The UI side of the contract is enforced by code review of `apply_mode` — the doc comment lists the transitive call set and confirms it touches the worker through zero paths.

### Numbers

- **150 tests** pass (was 144). +5 binary tests for Builder ActiveList variants + tag-count formatter; +1 atrium-core integration test for the mode-flip snapshot.
- `cargo fmt` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 150 tests
- The mode-flip snapshot test passes against a 1K-task populated DB.

### Try it

```bash
cargo run -p atrium

# Open the primary menu → Mode → Builder.
# The Inspector pane slides in on the right.
# Click a task row — the pane populates with the editor; type into
#   the title / notes; commit by clicking elsewhere.
# Click a project in the sidebar — the Sequential toggle and Review
#   interval picker appear above the task list; flip them and watch
#   the worker writes flow.
# Click Forecast / Review / Perspectives in the new Builder sidebar
#   section — placeholder pages tell you which phase ships them.
# Flip Mode → Simple — every Builder surface disappears; the side
#   pane collapses; a Builder stub view falls back to Today.
```

### What didn't change

- Schema (`0001_initial.sql` — Phase 1 superset is the contract; mid-v0.1 changes are forbidden).
- Single-writer worker, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- The Phase 7i modal Inspector dialog still ships (Simple Mode's path); Builder Mode adds the pane *alongside* it.
- Quick Entry, FTS5 search, multi-select, undo, sidebar filter, every Phase 4–9 surface — unchanged.

### What's next

Phase 11 — Defer dates and sequential project rendering. The `defer_until` placeholder row in the Builder group becomes a real date picker; Today/Anytime exclude tasks deferred to the future; sequential-project rendering dims/disables tasks past the first incomplete one when the project's Sequential toggle is on.

`VERSION`: 0.1.0 → 0.1.1 (patch — Phase 10 UI shell; no schema or new behaviour, just exposure).

## v0.1.0 (2026-05-07) — Simple Mode ships

The first public release. Atrium starts here: a native GNOME task manager that fuses Things 3's clarity with OmniFocus's depth via a mode switch over a shared local-first data store. Simple Mode — the calm, opinionated surface for *what am I doing right now* — is feature-complete. Builder Mode (Forecast, Review, Perspectives, defer dates, sequential projects, repeat rules) ships in v0.2.

This release is the result of nine phases of sequenced work spelled out in `roadmap.md`. The version-bump entries between v0.0.0 and v0.0.38 are the development trail; this entry is the milestone framing for what v0.1.0 actually contains.

### What ships in v0.1.0

**Six canonical lists** (`spec.md` §4.2), each backed by an indexed SQLite SELECT and fed via the single-writer worker:

- **Inbox** — open tasks with no project assignment.
- **Today** — overdue + scheduled today + the next 7 days of deadlines (Things-3 "deadlines approaching" heads-up). Excludes deferred and Someday items.
- **Upcoming** — future-scheduled tasks (`scheduled_for > today`).
- **Anytime** — open tasks with no schedule, not currently deferred.
- **Someday** — tasks parked on the `__someday__` sentinel (a state, not a date).
- **Logbook** — completed tasks, newest first.

**Hierarchy.** Areas → Projects → Tasks, with multi-tag attachment via a `task_tag` join table. Drag a task onto any sidebar project (or onto Inbox) to file it. Drag tasks within a list to reorder. Areas and projects support full CRUD with right-click context menus + the F2 / Ctrl+Shift+Delete / Ctrl+Shift+N / Ctrl+Shift+A keyboard chords. Project archive is transactional — completes every still-open task in the project inside the same SQL transaction.

**Distinct When and Deadline.** Things 3's choice, kept. `scheduled_for` is when you intend to do the task; `deadline` is when it's actually due. They land in different lists — Today pulls both; Upcoming only watches schedule; the deadline heads-up window surfaces upcoming due dates as a Today reminder.

**Quick Entry.** `Ctrl+Alt+Space` opens a small, non-modal capture modal anchored to the main window. Type a task title with optional inline `#tag`, `@today`, `@tomorrow`, `@someday`, `@yyyy-mm-dd`, `@deadline yyyy-mm-dd` syntax. Enter commits to Inbox; Esc dismisses. The same parser drives the bottom-of-list `Add task…` entry, so the syntax works in two places. (Phase 20's `atriumd` will give true OS-global capture; v0.1's shortcut requires Atrium be the focused application.)

**FTS5-backed search.** `Ctrl+F` opens a search bar with 200 ms debounce. Queries can mix freeform text (passed to FTS5) with structured filter clauses applied in Rust — `tag:NAME`, `is:open`, `is:done`, `is:overdue`, `due:today`. AND semantics across filters. `Q3 tag:work is:overdue` is one query.

**Inspector + tag editor.** Double-click a task row, right-click → *Edit Details…*, or `Ctrl+I` opens a modal `AdwDialog` exposing every editable Simple Mode field: title, notes (multi-line), schedule, deadline, project assignment. Tags get their own dialog (right-click → *Edit Tags…* or `Ctrl+T`) with a checkbox-per-tag picker plus an inline "Add a new tag" field that creates tags on demand. Both apply diffs against the opened snapshot — only changed fields hit the worker.

**Multi-select + undo.** `Ctrl+Click` toggles, `Shift+Click` extends ranges, `Ctrl+A` selects all in the active list. A reveal toolbar above the list shows "N selected" with Complete and Delete (destructive-styled) buttons. Single-action toggle and delete are undoable via a 6-second `AdwToast` button or `Ctrl+Z`; whichever fires first consumes the callback.

**Find-as-you-type sidebar.** `Ctrl+L` focuses the sidebar's filter entry. Live substring match against area / project / tag titles; the canonical list rows always stay visible; section headers ("Areas", "Unfiled", "Tags") hide automatically when none of their children pass.

**Sidebar count badges.** Every canonical list, area, project, and tag shows an open-task count badge that hides at zero. Counts refresh on every `TaskChanges` / `LibraryChanges` delta.

**Keyboard map.** Full coverage in `docs/keymap.md` and the in-app `Ctrl+?` / `F1` dialog. Every common operation has a chord; `Space` toggles the focused task, `Delete` removes it, `F2` starts inline rename, `Ctrl+1`–`Ctrl+6` jump between canonical lists. List-conflicting chords (Space, Delete, Ctrl+A, Esc) are scoped to the task-list widget so text entries elsewhere keep their normal key behavior.

**Typography + accessibility.** Inter Variable for UI, Source Serif 4 Variable for note bodies, JetBrains Mono Variable for the debug pane — all SIL OFL 1.1, all bundled (no system-font dependency). Inter ships with `cv11` (curved-l) and `ss01` (single-storey-a) on, plus `tnum` tabular figures wherever digits would otherwise dance. Atkinson Hyperlegible (Braille Institute, SIL OFL 1.1) ships as a one-toggle a11y option for low-vision readers, surfaced in the primary menu under *Mode → Accessibility*.

**Debug surface.** The `--debug` CLI flag opens additional menu entries in the primary menu. `Debug → Generate Fixtures` synthesizes 1K / 10K / 50K / 100K-task realistic fixture databases against the active library. `Debug → Memory Watch` opens a live readout of VmRSS / VmHWM / VmData sampled from `/proc/self/status` once a second. SQL statements stream to `tracing` at TRACE level (`RUST_LOG=trace` or scoped `atrium_core::db=trace` reveals every query and its wall time).

### Architecture commitments honoured

The five load-bearing decisions from `CLAUDE.md` all hold:

1. **Mode-as-View.** The `mode` GSettings key flips between Simple and Builder; v0.1 only wires Simple. The schema in `atrium-core/src/db/migrations/0001_initial.sql` is the **OmniFocus superset** — every Builder column (`defer_until`, `estimated_minutes`, `sequential`, `review_interval_days`, `last_reviewed_at`, `repeat_rule`, `parent_id`, `archived_at`) exists from day one. Phase 10 just exposes them.
2. **Single-writer SQLite worker.** A `tokio` task owns the writable `rusqlite::Connection`. The GTK thread holds an `mpsc::Sender<Command>` and never touches the writable connection. Reads use a separate `ReadPool` with `PRAGMA query_only=ON`. WAL mode is mandatory. UI updates arrive as `TaskChanges` / `LibraryChanges` deltas via `glib::MainContext::default().spawn_local`.
3. **Local-first, no network sync.** Storage lives at `$XDG_DATA_HOME/atrium/atrium.db`. Zero network calls. No CalDAV client, no cloud, no telemetry. Org vault projection (Phase 17) and VTODO export (Phase 19) are filesystem IO, not sync.
4. **Debug-first architecture.** The debug surface ships in the binary, not in a separate test harness. Every Phase grew the harness: schema-aware fixtures (Phase 1), SQLite IO instrumentation (Phase 2), live RSS readout (Phase 8e). Tests use the same fixtures.
5. **Schema freeze through v0.1.** No mid-v0.1 migrations. Backwards-compatible migrations begin at v0.2. The v0.0.38 Today filter widening was a query change, not a schema change.

### Out of scope for v0.1

By design, not by oversight:

- **Builder Mode** (Phase 10–15) — Inspector pane, Forecast, Review, Perspectives, defer dates, sequential project rendering, repeating tasks. Stubs exist where the v0.1 work needed Builder hooks; the actual UI lands in v0.2.
- **Imports / exports** (Phase 16–19) — Things 3 JSON, Org-mode round-trip, OmniFocus `.ofocus`, Taskwarrior, Todoist, VTODO, todo.txt, TaskPaper. Atrium runs DB-only in v0.1.
- **Network sync of any kind.** Per spec §9.
- **Capture daemon (`atriumd`).** Phase 20. Quick Entry currently fires only when Atrium is focused; Phase 20 adds true OS-global capture.

### Numbers

- **144 tests** pass (62 binary + 82 core).
- `cargo fmt --all --check` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- Release-mode cold start ~25–33 ms in ~32 MB on the 5K-task baseline (`docs/perf-baseline.md`).
- Data-layer RSS flat with task count: 1K → 35 MB / 10K → 37 MB / 50K → 37 MB peak. All four §8 budgets met or trending well under.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 144 tests
- `scripts/regression.sh` ✓ — full ship gate

### Run it

```bash
git clone https://github.com/VirInvictus/Atrium
cd Atrium
cargo run -p atrium

# Or with the debug surface:
cargo run -p atrium -- --debug

# Or a populated fixture for a real-feel demo:
cargo run -p atrium -- --fixture medium  # 10K tasks
cargo run -p atrium                      # then run normally — fixture data is already in the DB
```

### What's next

Phase 10 — Builder Mode UI shell. Mode toggle in the primary menu, `AdwOverlaySplitView` Inspector pane, Builder-only sidebar entries (Forecast, Review, Perspectives) as stubs, Project page extras (Sequential toggle, Review interval picker). The mode-flip integration test proves no DB work happens on mode switch.

`VERSION`: 0.0.38 → 0.1.0 (minor — Simple Mode complete; first public release).

## v0.0.38 (2026-05-07) — Today: deadlines-approaching heads-up

Brandon flagged that Today wasn't matching the mental model — tasks with deadlines coming up in the next few days were buried in Anytime until the deadline date itself arrived, which is the opposite of what a "Today" view is supposed to do. Things 3 surfaces approaching deadlines in Today as a heads-up so you don't get blindsided; Atrium now matches that behaviour.

### Spec change

`spec.md` §4.2 — Today's derived-view definition gains a `today + N` deadline window:

> **Today:** `task WHERE completed_at IS NULL AND (scheduled_for ≤ today OR deadline ≤ today + N) AND (defer_until IS NULL OR defer_until ≤ today)`, where `N = TODAY_DEADLINE_WINDOW_DAYS` (default `7`).

Earlier the second clause was `deadline ≤ today` (today + overdue only). Now it's `deadline ≤ today + 7` (today + overdue + the next week). The window is one constant in v0.1; promoting it to a per-user GSettings key is a Phase 8d preferences task and noted as such in the doc.

`scheduled_for` semantics are unchanged. Future scheduled tasks still live in Upcoming; Today still treats `scheduled_for` as `≤ today` (today + overdue scheduled). The change is deadline-side only, which matches Things 3 — they distinguish "When" (scheduled) from "Deadline" the same way.

### Implementation

- `atrium-core/src/db/read.rs` — new `pub const TODAY_DEADLINE_WINDOW_DAYS: i64 = 7`. `list_today` and `count_open_canonical`'s today subquery both compute a horizon date (`today + window`) and use it in the deadline branch of the WHERE clause via a second bound parameter. Sidebar badge and content-pane list query share the same predicate, so they can't drift.
- `atrium/src/ui/task_list.rs` — `ActiveList::Today.task_matches` (the in-memory predicate the diff applier uses to decide whether a task belongs in the visible store) gets the same window logic. Imports `TODAY_DEADLINE_WINDOW_DAYS` from atrium-core so the constant has one home.

### Tests

Seven new tests cover the boundary cases:

- `today_includes_deadline_within_heads_up_window` — deadline 5 days out shows.
- `today_includes_deadline_at_window_edge` — deadline exactly `today + 7` shows.
- `today_excludes_deadline_past_window` — deadline 8 days out doesn't show.
- `today_count_matches_list_today_with_window` — sidebar badge count and Today-list rows agree (ensures the two SQL queries stay in sync).
- Three predicate-side analogues in `task_list.rs` for the same boundary cases.

The existing tests (`today_includes_overdue`, `today_includes_scheduled_for_today`, `today_excludes_future_scheduled`, `today_excludes_someday_sentinel`, `today_excludes_completed`, `today_excludes_deferred_to_future`, `today_includes_deferred_now_active`, `today_includes_deadline_only_due_today`) all still pass — the change is strictly additive on the deadline side.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **144 tests** (was 137; +4 in atrium-core, +3 in atrium-binary).

### Try it

```bash
cargo run -p atrium

# Open Today.
# Add a task in any list; set its deadline to 3 days from now via
#   the Inspector. The row appears in Today (was hiding in Anytime
#   until v0.0.38).
# Set a deadline 10 days out — the row stays in Anytime; it'll
#   migrate to Today automatically when the date crosses into the
#   7-day window.
```

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- `scheduled_for` Today semantics (still `≤ today`); Upcoming, Anytime, Someday, Inbox, Logbook all unchanged.
- The Inspector / tag editor / task row work from v0.0.37 — that's still the foundation of Today's display path; this change is the predicate underneath it.

`VERSION`: 0.0.37 → 0.0.38 (patch — Today filter alignment with spec change).

## v0.0.37 (2026-05-07) — Pre-v0.1 UI bugsweep

Brandon ran a full pass on the UI before tagging v0.1.0 and surfaced four real issues that v0.0.36's "ship the wiring" stance had papered over. The Inspector dialog appeared transparent — task rows visibly bled through the form because the window was floating without a clean modal background. Single-clicks on a task title hijacked the row's selection / Inspector-open semantics by entering edit mode. Pressing Escape inside the bottom-of-list new-task entry silently cleared the multi-selection. And every row in the basic list view rendered the date column as just "May", because the schedule + deadline labels were ellipsising under pressure from the title's hexpand.

This release fixes those four and clears a layer of code drift CLAUDE.md hadn't kept up with. No new features. No schema changes (the v0.1 freeze rule still holds). No new crates (the locked v0.1 dependency set still holds).

### Inspector + tag editor → AdwDialog

`atrium/src/ui/inspector.rs` and `atrium/src/ui/tag_editor.rs` previously built modal forms by constructing a fresh `adw::Window` with `transient_for + modal(true)` and turning off both sides of `show_*_title_buttons`. That gave the dialog a free-standing top-level surface with no chrome — and on tiling / floating compositors, the parent's content showed through wherever the form's rows didn't reach the dialog edges. The screenshot Brandon attached has the inspector form floating over visible task rows: that's what the bug looked like.

The libadwaita-correct primitive for this layout is `adw::Dialog`, which presents as an in-window modal overlay with a guaranteed solid background, automatic Esc-to-close, and the slide/fade animation libadwaita uses for every other modal in the platform. Both inspector + tag editor switch:

- `adw::Window::builder().transient_for + modal(true)` → `adw::Dialog::builder()` with `content_width` / `content_height`.
- `set_content` → `set_child`.
- `dialog.present()` → `dialog.present(Some(parent))`.
- The bespoke `gtk::EventControllerKey` for Esc dismissal is gone — AdwDialog consumes Esc itself and runs its own close path.
- `dialog.close()` returns `bool` instead of `()`, so the call sites bind it with `let _ = dialog.close();` or move it into a block expression.
- The dead `atrium-inspector` / `atrium-tag-editor` CSS classes (declared but never styled) are gone.
- The Inspector's Notes group drops its redundant "Plain text. Saved when you click Apply." description — the Apply button conveys the same thing.

Quick Entry (`atrium/src/quickentry/modal.rs`) and Memory Watch (`atrium/src/debug/mod.rs`) **stay** as `adw::Window` — both want non-modal, transient-for-main behaviour that AdwDialog can't give. Quick Entry per spec §6 ("Does not steal focus from the previously focused window") needs `modal=false`; Memory Watch is a passive observer pane. Their fade-in keyframe (`atrium-quickentry-window` CSS class) keeps working unchanged.

### Task row title: `GtkEditableLabel` → `GtkStack(Label / Entry)`

The bigger behavioral fix. v0.0.36's row layout used `gtk::EditableLabel` as the title widget — convenient because it bundles "render text" and "edit on click" in one widget. But `EditableLabel`'s built-in click gesture intercepts single + double clicks on the label's text region, which conflicted with two adjacent things:

1. The row's `MultiSelection` model wants single-clicks to select rows.
2. The per-row activate gesture (Phase 7j) wants double-clicks to open the Inspector.

The result was that **whether a click selected, edited, or activated depended on cursor pixel position** — the EditableLabel claimed clicks landing on the title text, the row's gesture got clicks landing on the padding around the title. That's the source of Brandon's "the application doesn't know if I'm inputting, just clicking, or running a shortcut" complaint.

The fix splits "render the title" from "rename the task" cleanly:

- `atrium/src/ui/task_list.rs` — title is now a `gtk::Stack` with two named pages: `display` (a `gtk::Label` with `ellipsize=End`) and `edit` (a `gtk::Entry`).
- The bound display page renders the title; the entry page is hidden until F2 (or right-click → Rename via the existing window action map) flips the stack to "edit", populates the entry from the label, focuses it, and select-alls.
- Enter on the entry commits via `on_rename(task_id, new_title)`; the stack flips back to "display".
- Focus-leave on the entry commits the same way (Things-3-style autosave).
- Esc on the entry reverts to the bound label text and flips back without writing.
- Single clicks on the title → fall through to the row's selection gesture.
- Double clicks on the title → fall through to the row's activate gesture (which opens the Inspector).
- Right-click on the title → fall through to the row's context menu (Edit Details… / Edit Tags…).

`atrium/src/ui/window.rs::start_edit_focused_row` (the F2 dispatcher) was updated to walk both upward (focus on a child of the row) and downward (focus on a parent like `GtkListItemWidget`) to find the row Box, then call a new helper `start_edit_on_row` that flips the stack. Per-row data stash keys changed too: `atrium-title` (the EditableLabel) is gone; in its place are `atrium-title-stack`, `atrium-title-label`, `atrium-title-entry`, `atrium-title-focus-ctrl`, `atrium-title-key-ctrl`.

The `atrium-task-title` CSS class still applies — to both the display Label and the edit Entry — so all the typography work from Phase 8a (Inter cv11/ss01, weight 450, letter-spacing −0.005em) still lands on titles regardless of mode.

### Escape: window-global → list-scoped

`atrium/src/main.rs` no longer registers `Escape` as a window-global accel for `win.bulk-clear`. The chord moved into `atrium/src/ui/window.rs::init_list_view`'s `gtk::ShortcutController` (`ShortcutScope::Managed`), the same controller that scopes Space / Delete / Ctrl+A as of Phase 7h. Esc now fires the bulk-clear only when the task list itself or one of its rows has focus — every entry on the surface (Quick Entry, search bar, sidebar filter, tag editor's add-tag entry, the bottom-of-list new-task entry) keeps its own Esc semantics.

### Date pills: drop ellipsize, drop the alarm-clock emoji

`atrium/src/ui/task_list.rs` — schedule + deadline labels no longer set `ellipsize=End`. The title's display Label is the one that ellipsises now (it has `ellipsize=End` and `hexpand=true`), so a long title shrinks instead of squeezing the date column to "May".

`atrium/src/ui/task_object.rs::format_deadline` swaps the leading `⏰` emoji for a `Due ` prefix. The emoji rendered inconsistently across systems — some show a glyph, some show a typographic box, some put it at the wrong baseline — and "Due May 15" reads the same everywhere.

### Code-smell sweep

- `atrium/src/main.rs::install_window_actions` was a documented empty stub kept "for symmetry". Inlined; the comment block + the `install_window_actions(app)` call site are gone.
- `atrium/src/ui/window.rs::prompt_create_project` had a dead `area_id` lookup branch (`win.imp().project_titles.borrow().get(&id).and(None)`) with an apologetic comment about not caching `area_id` on projects yet. Replaced with a `_` arm that returns `None` plus a comment explaining what's missing — same behaviour, no fake lookup.
- `data/window.ui` — the `AdwToastOverlay` block was indented two columns shy of its siblings, making the file painful to scan. Reformatted; structure unchanged.
- `docs/keymap.md` had a stale row in the "Builder Mode (sketched, not yet bound)" table reserving `Ctrl+I` for "Toggle Inspector pane" — the same chord Phase 7i (v0.0.35) bound to the Simple Mode "Open Inspector dialog". Dropped the duplicate row; added a one-sentence note explaining Builder Mode reuses the same chord for the side-pane variant.

### Documentation refresh

`CLAUDE.md` is the file that drifted hardest. It still claimed "Pre-implementation as of v0.0.0. No source code exists yet" — eight phases later, that's wrong in every clause. Rewrites:

- New "Status" block reflecting the v0.0.37 reality (Simple Mode shipping, Phase 9 release work outstanding, Phases 10+ not started).
- New "Codebase map (current)" section listing every module / file added since the original CLAUDE.md was written: `atrium-core/src/db/{worker,read_pool,read,command,changes,fixtures}.rs`, `atrium/src/ui/{window,task_list,task_object,inspector,tag_editor,filter,shortcuts,about,typography}.rs`, `atrium/src/quickentry/{mod,modal,parser}.rs`, `atrium/src/debug/mod.rs`, `data/fonts/`, `data/icons/`, `docs/{schema,keymap,accessibility,perf-baseline,regression}.md`, `scripts/regression.sh`, plus the new dialog primitive policy (which dialogs use AdwDialog vs AdwWindow).

The five architectural commitments (Mode-as-View, single-writer worker, local-first, debug-first, vault projection), the spec discipline rules, the release discipline rules, the dependency lock, the schema freeze, the perf budget, and the application identifiers all stay verbatim — those decisions don't move with a bugsweep.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **137 tests** (59 binary + 78 core), unchanged from v0.0.36.

### Try it

```bash
cargo run -p atrium

# Open the Inspector — solid background, no bleed-through.
# Click anywhere on a row, including the title text — the row
#   selects (multi-selection plumbing); no edit-mode hijack.
# Double-click anywhere on a row — Inspector opens.
# F2 with a row focused — title flips into an Entry; type the
#   new name; Enter or click-away commits, Esc reverts.
# Click into the bottom-of-list new-task entry, type a task,
#   hit Esc — the entry text stays put (was clearing the
#   multi-selection silently before).
# Look at any row's date column — full "May 7" / "Due May 15"
#   instead of a clipped "May".
```

### What didn't change

- Schema (`0001_initial.sql`), single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline, perf budget — all untouched.
- v0.1 dependency set unchanged. No new crates.
- Quick Entry and Memory Watch still use `adw::Window` (they want non-modal transient-for behaviour AdwDialog can't give).
- The Phase 7g tag editor's apply pipeline (`ensure_tag` for each new name → `set_task_tags`) is unchanged.
- The Phase 7i Inspector's apply pipeline (diff against snapshot → single `update_task`) is unchanged.
- Inline `#tag` / `@today` / `@deadline yyyy-mm-dd` syntax in Quick Entry and the bottom-of-list entry is unchanged.

`VERSION`: 0.0.36 → 0.0.37 (patch — UI bugsweep before v0.1.0).

## v0.0.36 (2026-05-06) — Phase 7j: Inspector layout pass + reliable double-click

The v0.0.35 Inspector shipped functional but visually broken — fields with no Adwaita chrome, labels misaligned with their values, footer floating in the middle of the screen. Brandon also caught that double-clicking a row didn't actually open the Inspector. Both were the kind of "ship the wiring, polish later" oversight that doesn't survive contact with the user. v0.0.36 rebuilds the Inspector body with proper libadwaita form widgets and adds a per-row double-click gesture that doesn't rely on `GtkListView::activate`.

### What was wrong

**Layout**: the v0.0.35 body was a hand-rolled `GtkBox` of `[caption_label] [field]` pairs. No card backgrounds, no group headers, no consistent column alignment, footer in the body. Looked like a debug form, not a GNOME app.

**Double-click**: `self.imp().task_list_view.connect_activate(...)` — the standard "row activated" hook — fires on `GtkListView`'s notion of activation, but the row's inner `GtkEditableLabel` (the title) traps double-clicks for its own enter-edit-mode behaviour first. The activate signal never fires for double-clicks that land near the title text, which is most of them.

### What shipped

**Inspector layout (`atrium/src/ui/inspector.rs`)** — full rebuild around libadwaita form widgets:

- **`adw::PreferencesPage`** as the body container — gives automatic padding, scrolling, and the standard Adwaita form background.
- **Group 1 — Title**: `adw::EntryRow` inside an `adw::PreferencesGroup`. Replaces the bare `GtkEntry` + `title-2` class hack with the proper Adwaita single-field form row.
- **Group 2 — Schedule / Deadline / Project**: a single `adw::PreferencesGroup` holding three rows. Schedule and Deadline are `adw::ActionRow`s with the existing `MenuButton`-with-popover pickers as suffix widgets (so the row title labels them and the pill sits on the right where the Adwaita pattern puts a "value or chevron" widget). Project is an `adw::ComboRow` — the proper Adwaita dropdown chrome with a chevron suffix and the right-aligned current-selection label.
- **Group 3 — Tags**: `adw::ActionRow` with title "Tags", subtitle showing the count ("3 tags" / "1 tag" / "No tags"), and an "Edit Tags…" `flat` button as suffix. Activating the row anywhere triggers the same hand-off to the Phase 7g tag editor.
- **Group 4 — Notes**: `adw::PreferencesGroup` titled "Notes" with description "Plain text. Saved when you click Apply." Holds a `GtkScrolledWindow` (`card` + `view` classes) wrapping a `GtkTextView` with proper internal padding. 180 px min height.
- **Header bar Cancel / Apply** — moved out of the body and into the `adw::HeaderBar` (`pack_start` for Cancel, `pack_end` for the suggested-action Apply). Matches GNOME convention; window-close-buttons hidden so the user follows the explicit flow.

**Double-click reliable (`atrium/src/ui/task_list.rs`)**:

- New per-row primary-button `gtk::GestureClick` with no special configuration — the default already accepts double-click events. The `connect_released` handler checks `n_press == 2` and fires `widget.activate_action("win.edit-details-for", Some(&task_id.to_variant()))`.
- Stashed under `atrium-activate-gesture` row data; unbind path tears it down so the factory recycle pool doesn't accumulate gestures.
- `gtk::Widget::activate_action` walks the action lookup chain to find `win.edit-details-for` on the `AtriumWindow` — same parameterized action the right-click menu uses, so the gesture and menu paths converge on `open_inspector_for(task_id)`.
- The (unreliable) `task_list_view.connect_activate` handler is gone. `Ctrl+I` covers the keyboard path.

**Why the gesture works where activate didn't**: `GestureClick` on the row Box runs in target-phase by default; clicks land on whatever child widget is under the pointer first, but pointer events bubble up to gestures attached at the row level after children get a shot. `GtkEditableLabel` consumes single and double clicks ON the title text (preserving inline-edit), but clicks on the row's padding, the checkbox area edges, the schedule/deadline pills' margins — anywhere else on the row Box — bubble up and trigger our gesture cleanly.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓ (one fix: `if … { if let Some … }` collapsed to `if … && let Some …`)
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **137 tests** unchanged.
- `scripts/regression.sh` — green at v0.0.35 → v0.0.36.

### Try it

```bash
cargo run -p atrium

# Double-click any task row → Inspector opens with the proper
# Adwaita form layout (groups, dividers, card-styled notes,
# header-bar Cancel/Apply).
# Double-click on the title text itself still starts inline title
# edit (EditableLabel intercepts there), which is the right
# behaviour — F2 works the same way.
# Click anywhere else on the row body → Inspector opens.
# Ctrl+I → opens Inspector for focused/selected task.
# Right-click → Edit Details… → opens Inspector.
```

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Apply / Cancel semantics, the date popovers, the project dropdown semantics — all the data-flow logic from v0.0.35 carries over identically.
- The hand-off to the Phase 7g tag editor.

`VERSION`: 0.0.35 → 0.0.36 (patch — Phase 7j Inspector polish + double-click hotfix).

## v0.0.35 (2026-05-06) — Phase 7i: per-task Inspector dialog

The biggest UX gap before v0.1.0 closes: there's now a real way to edit a task's full set of properties. Double-click a task, right-click → *Edit Details…*, or `Ctrl+I` opens a modal Inspector with title, notes, schedule (When), deadline, project, and a hand-off button to the tag editor. Notes had no UI before this slice; schedule and deadline could only be set via inline `@today` / `@deadline` syntax at create time; project assignment was drag-and-drop only. All four are now first-class.

### What shipped — data layer (atrium-core)

- **`TaskUpdate` extended** with `scheduled_for: Option<Option<ScheduledFor>>` and `deadline: Option<Option<NaiveDate>>`. The double-`Option` carries set/clear semantics: outer `Some(_)` = "include in update", inner `None` = "set the column to NULL", inner `Some(value)` = "set to value". Builder methods `schedule(value)` and `deadline_value(value)` for the call-site API.
- **`is_noop()`** updated to consider the two new fields.
- **`worker::update_task`** SQL builder grew two `if let Some(...) = update.{scheduled_for|deadline}` arms that push `scheduled_for = ?` / `deadline = ?` into the `UPDATE` statement with the value bound. `ScheduledFor` already implements `ToSql` from Phase 1; `NaiveDate` does too via the `chrono` rusqlite feature. Same transactional behaviour; same `TaskChanges` delta emission.
- **2 new core tests**: `update_task_sets_and_clears_schedule` (Date → Someday → cleared round-trip) and `update_task_sets_and_clears_deadline`.

### What shipped — UI (atrium binary)

- **`atrium/src/ui/inspector.rs`** — modal `adw::Window` (520×560, `transient_for(main)`, `modal(true)`). Layout is a vertical stack:
  - **Title** — `gtk::Entry` pre-filled, `title-2` style. Empty title on Apply rejected (entry shows the `error` style, focus restored).
  - **Notes** — `gtk::TextView` in a `card`-styled `ScrolledWindow`, 140 px min height. Wraps on word boundaries.
  - **Schedule** — `gtk::MenuButton` with current value as label. Popover has Today / Tomorrow / Someday / Clear preset buttons plus a `gtk::Calendar` for arbitrary dates.
  - **Deadline** — same pattern, no Someday option.
  - **Project** — `gtk::DropDown::from_strings` with "Inbox (no project)" at index 0 followed by every project in the library. Selection maps back to `Option<i64>` on apply.
  - **Tags** — read-only count label ("3 tags" / "1 tag" / "no tags") plus an *Edit Tags…* button that closes the Inspector and re-opens the Phase 7g tag editor against the same task id. Avoids two modal windows fighting for focus.
  - **Footer**: Cancel + suggested-action Apply.
- **Apply pipeline** — diffs the form state against the snapshot the dialog opened with and dispatches one `worker.update_task(...)` carrying only the changed fields. No-op submissions close without writing. Apply closes on success; on failure the dialog stays open so the user can retry.
- **Esc** dismisses (key controller).
- **4 new UI tests** for the label formatters (`format_schedule_label` covers None / Someday / Date; `format_deadline_label` covers None / Some).

### Wiring

- **`AtriumWindow::open_inspector_for(task_id)`** loads the task, project list, and tag count from the read pool, then hands off. **`open_inspector_focused()`** is the keyboard / shortcut entry point.
- **Three open paths**, all routing into the same method:
  1. `gtk::ListView::connect_activate` fires on double-click and on Enter when a row is focused.
  2. Right-click context menu on a row gains *Edit Details…* above the existing *Edit Tags…* entry. Both menu items use parameterized `win.edit-{details|tags}-for(i64)` actions so the right-click row doesn't need to be part of the current selection.
  3. `Ctrl+I` window-global accel → `win.edit-details-focused`.
- **Docs + dialog**: `docs/keymap.md` gains the `Ctrl+I` row in *List actions*; `atrium/src/ui/shortcuts.rs::SHORTCUTS_XML` gets the matching entry. The existing `Ctrl+T` tag-editor shortcut stays.

### Why a modal dialog instead of inline expansion

Things 3's "magic edit area" expands the row inline for editing. Doable in GTK with a `GtkStack`-per-row, but it's a much bigger refactor of the row factory and the recycle path. For Simple Mode v0.1, a modal Inspector ships every editable field today and matches the Phase 10 Inspector pattern that Builder Mode will deepen — no architectural drift.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓ (caught and fixed: `Option<ScheduledFor>::clone` on a `Copy` type, `as u32 as u32` redundant cast, unused parameters)
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **137 tests** (was 133; +2 core update_task date tests, +2 UI label-formatter tests).
- `scripts/regression.sh` — green.

### Try it

```bash
cargo run -p atrium

# Double-click any task → Inspector opens.
# Type into the notes field → Apply → notes saved.
# Click the Schedule pill → popover with Today / Tomorrow / Someday /
#   Clear / calendar → click → label updates → Apply.
# Same for Deadline.
# Change Project via the dropdown → Apply → task moves.
# Click Edit Tags… → Inspector closes, tag editor opens.
# Cancel / Esc → dialog dismisses with no writes.
```

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Inline `#tag` / `@today` / `@deadline` syntax in the bottom-of-list entry and Quick Entry stays as-is — fast-create path.
- The Phase 7g tag editor is unchanged; the Inspector reuses it via the *Edit Tags…* hand-off rather than reimplementing tag selection.

### Phase 9 — what's left for v0.1.0

The Inspector closes the last "no UI for this field" gap in Simple Mode. Remaining:

1. Capture screenshots → `docs/screenshots/` and update the README placeholder.
2. Bump VERSION → `0.1.0`, sync `Cargo.toml`, `meson.build`, add the `<release version="0.1.0">` entry to `data/io.github.virinvictus.atrium.metainfo.xml`.
3. Tag `v0.1.0`, push, build the Flatpak, publish.
4. Public release announcement on `VirInvictus.github.io`.

`VERSION`: 0.0.34 → 0.0.35 (patch — Phase 7i slice; closes the last "field has no UI" gap before v0.1.0).

## v0.0.34 (2026-05-06) — Phase 7h: stop eating spaces in entries

A real one — Brandon caught it the moment he tried to type a multi-word task: **the spacebar didn't work in any entry**. Three Phase 7c chords (`Space` toggle-complete, `Delete` delete-task, `Ctrl+A` select-all) were registered as window-global accels via `app.set_accels_for_action`, which meant they were intercepted before any focused GtkEntry could see the key. Typing a space ran the toggle handler, hitting Delete deleted the focused task, Ctrl+A selected every task in the active list. None of those are what the user wants while typing.

### What broke

```rust
// main.rs::install_accels
app.set_accels_for_action("win.delete-task", &["Delete"]);
app.set_accels_for_action("win.toggle-complete", &["space"]);
app.set_accels_for_action("win.select-all", &["<Primary>a"]);
```

GtkApplication accels are window-scoped at minimum; bound this way, they fire on any key press inside the window regardless of which widget has focus. For chords that conflict with text input — Space, Delete, Ctrl+A — that's wrong.

### What fixed it

The three list-action chords moved from app-level accels to a `gtk::ShortcutController` attached directly to `task_list_view`, scoped `ShortcutScope::Managed`:

```rust
// window.rs::init_list_view (Phase 7h)
let list_shortcuts = gtk::ShortcutController::new();
list_shortcuts.set_scope(gtk::ShortcutScope::Managed);
for (chord, action_name) in [
    ("space", "win.toggle-complete"),
    ("Delete", "win.delete-task"),
    ("<Primary>a", "win.select-all"),
] {
    if let Some(trigger) = gtk::ShortcutTrigger::parse_string(chord) {
        let action = gtk::NamedAction::new(action_name);
        list_shortcuts.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
    }
}
self.imp().task_list_view.add_controller(list_shortcuts);
```

`Managed` scope fires the shortcuts when *the controller's widget or one of its descendants* has focus — exactly the "task row is focused" condition. When focus is on a GtkEntry (Quick Entry, the bottom-of-list new-task entry, the search bar, the sidebar filter, the tag editor's "Add a new tag" entry), the controller doesn't fire and the entry handles the key normally.

### What didn't change

- **`Escape`** stays window-global (`win.bulk-clear`). Surfaces with their own Esc handlers — sidebar filter `stop-search`, Quick Entry's key controller, tag editor's key controller, alert dialogs — consume Esc before the global accel sees it. The global is the right fall-through for everywhere else.
- **`F2`, `Ctrl+T`, `Ctrl+L`, `Ctrl+F`, `Ctrl+1`–`Ctrl+6`, `Ctrl+N`, `Ctrl+Z`, `Ctrl+?`, `Ctrl+Q`, `Ctrl+Shift+N/A/T`, `Ctrl+Shift+Delete`** — none conflict with text input, so they stay globally bound where they were.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **133 tests** unchanged.
- `scripts/regression.sh` — green.

Manual test plan: open the bottom-of-list new task entry, type "buy milk and eggs" — the spaces survive. Same for Quick Entry, the search bar, the sidebar filter, and the tag editor's add-tag entry. Click a row, hit Space — it toggles. Click an entry, hit Delete to delete a character — it deletes the character; the task isn't deleted.

### Documentation

`docs/keymap.md` "List actions" section now notes the scoping explicitly so a future maintainer doesn't accidentally re-globalise these chords.

`VERSION`: 0.0.33 → 0.0.34 (patch — Phase 7h hotfix).

## v0.0.33 (2026-05-06) — Phase 7g: per-task tag editor + visible chips

The biggest gap on the road to v0.1.0 was tag UX: there was no way to adjust a task's tags except by retyping its title with `#tag` syntax. This slice fills that in — right-click any task row, pick *Edit Tags…*, get a proper dialog with checkboxes for every known tag plus an entry for new ones. `Ctrl+T` does the same from the keyboard. The existing inline tag display also picks up a chip-shaped CSS treatment so tags actually read as tags now instead of dim text.

### What shipped

- **`atrium/src/ui/tag_editor.rs`** — new module. `open(parent, worker, task_id, task_title, current_tag_ids, all_tags)` constructs an `adw::Window` (`transient_for(main)`, modal) with:
  - **Header**: "Editing: «truncated title»" with the full title in the tooltip.
  - **Body**: a `gtk::ListBox` styled `boxed-list`, one `gtk::CheckButton` per existing tag (currently-attached ones pre-checked). Toggling a row updates a shared `Rc<RefCell<HashSet<i64>>>` that the apply step harvests.
  - **Add row**: a `gtk::Entry` ("Add a new tag…") + a small `+` button. Pressing Enter or clicking + appends a new pre-checked, non-interactive row labelled `#name · new` and pushes the name into a separate buffer for `ensure_tag` on apply. Empty / case-insensitively-duplicate names are no-ops; if the name matches an existing tag, the existing row is checked instead of duplicated.
  - **Footer**: Cancel + suggested-action Apply.
- **Apply pipeline** — runs in a `glib::MainContext::default().spawn_local`. Snapshots the selected ids and the pending-name buffer first (so no RefCell ref is held across the `await` boundaries — clippy's `await_holding_refcell_ref` flagged the naive version, fixed). For each pending name, calls `worker.ensure_tag(name).await` and pushes the resolved tag id. Finally one `worker.set_task_tags(task_id, ids).await` writes the relationship table. Errors at either step keep the dialog open so the user can retry.
- **Right-click on a task row** opens a `gtk::PopoverMenu` with one entry — *Edit Tags…* — bound to a parameterized `win.edit-tags-for(i64)` action that takes the task id directly. Decoupled from selection state so right-clicking a row outside the current multi-selection still acts on the right task. The popover is `set_parent`-ed to the row's Box; `connect_unbind` `unparent()`s it (and removes the gesture controller) so the factory's row recycling pool doesn't leak phantom children — same Phase 8h cleanup pattern, applied at the row level.
- **`win.edit-tags-focused`** action (no parameter) targets `focused_task_id()` — bound to **`Ctrl+T`** in `install_accels`. Useful for keyboard-only flows where the user is already on a row.
- **`AtriumWindow::open_tag_editor_for(task_id)`** is the entry point both menu and accelerator funnel into. Loads `task_by_id`, `tag_ids_for_task`, and `list_tags` from the read pool, then hands off to `tag_editor::open`. Read-pool failures log via `tracing::error` and bail rather than opening a half-populated dialog.
- **CSS chip treatment** for the inline tag label:
  ```css
  .atrium-task-tags {
    font-size: 0.85em;
    padding: 1px 6px;
    border-radius: 6px;
    background-color: alpha(@accent_bg_color, 0.15);
    color: @accent_color;
  }
  .atrium-task-row.completed .atrium-task-tags {
    background-color: alpha(@accent_bg_color, 0.08);
  }
  ```
  Uses libadwaita's `@accent_bg_color` / `@accent_color` so the chips inherit theme colour and respect `prefer-contrast`. Per-tag distinct colours (a future polish slice) would mean splitting the single Label into multiple widgets — deferred so this slice stays focused.
- **Docs + dialog**: `docs/keymap.md` gains the `Ctrl+T` row in *List actions*; `atrium/src/ui/shortcuts.rs::SHORTCUTS_XML` gets the matching `GtkShortcutsShortcut`.
- **3 unit tests** for the local `truncate` helper (short strings unchanged, ellipsis at boundary, unicode-safe boundary). The dialog's GTK side is exercised manually because constructing it requires a GTK init.

### Why a dialog and not an inline popover

Two reasons:
1. **Discoverability.** A right-click menu lists "Edit Tags…" alongside the cluster of per-task ops; a pure popover triggered only by a chord wouldn't.
2. **Layout headroom.** A library with 30+ tags wants a scrollable list, an entry, and clear Cancel/Apply affordances. Cramming that into a popover anchored to a row makes the popover dominate the window. A proper transient `adw::Window` looks at home next to the existing Quick Entry surface and reuses libadwaita's modal close behaviour.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓ (caught and fixed: collapsible-if, await-holding-refcell-ref, let-and-return)
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **133 tests** (was 130; +3 `truncate` helper tests).
- `scripts/regression.sh` — green.

### Try it

```bash
cargo run -p atrium

# Right-click any task → Edit Tags… → check / uncheck / add new → Apply.
# Or: focus a task row → Ctrl+T → same dialog, no mouse.
# Inline `#tag` syntax in the bottom-of-list entry and Quick Entry
# still works — the editor is the *adjustment* path, the inline
# syntax remains the fast-create path.
```

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- The inline `#tag` parser and Quick Entry behaviour stay as they were.
- `worker.ensure_tag` and `worker.set_task_tags` are pre-existing — this slice is purely UI plumbing on top of the data layer that's been ready since Phase 6.

### Phase 9 — what's left for v0.1.0

Same list as v0.0.32, minus the tag-editor blocker Brandon flagged. Now genuinely:

1. Capture screenshots → `docs/screenshots/` and update the README placeholder.
2. Bump VERSION → `0.1.0`, sync `Cargo.toml`, `meson.build`, add the `<release version="0.1.0">` entry to `data/io.github.virinvictus.atrium.metainfo.xml`.
3. `scripts/regression.sh` clean.
4. Tag `v0.1.0`, push, build the Flatpak, publish.
5. Public release announcement on `VirInvictus.github.io`.

`VERSION`: 0.0.32 → 0.0.33 (patch — Phase 7g slice; closes the Phase 6 / Phase 8 deferred tag-editor item).

## v0.0.32 (2026-05-06) — Phase 9b: README finalisation

The README catches up to the actual shipped state. The "pre-implementation" badge is gone; the framing is "Simple Mode shipping / Builder Mode next." The feature table reflects what's actually in the binary today rather than the v0.0.0 contract.

### What shipped

- **Status badges** — "Simple Mode: shipping" + "Builder Mode: next" replace the pre-implementation grey.
- **Feature table for Simple Mode v0.1** rewritten to mention everything that actually landed across Phases 0-8: Quick Entry's inline `#tag` / `@today` / `@deadline` syntax (Phase 6b/c); FTS5 search (7a) + `tag:` / `is:` / `due:` filter expressions (7d); `Ctrl+L` find-as-you-type sidebar (7e); multi-select + bulk Complete / Delete with summary toast (7c); `Ctrl+Z` undo restoring tag attachments (7b/7f); drag-reorder + drag-to-file (Phase 5b); full keyboard map with `F2` inline edit fall-through (7f); Atkinson Hyperlegible accessibility toggle + AT-SPI labels (8c/8f); the `--debug` Memory Watch surface (8e).
- **Build and Run section** with copy-paste cargo commands, the regression gate one-liner, the four fixture sizes, the `--debug` flag, and the Flatpak invocation.
- **Screenshots placeholder section** — HTML comment lists the suggested set (Today view, project page with pills, sidebar filter, multi-select bar, search with filter expression, high-legibility toggle on). Screenshots get captured against the v0.1.0 tag when there's a polished build to point at.
- **`docs/` cross-links** added to the "Where things live" table — `keymap.md`, `accessibility.md`, `perf-baseline.md`, `regression.md` all referenced so a reader new to the repo lands on the right doc fast.
- **Bundled-fonts list** updated to include Atkinson Hyperlegible alongside Inter / Source Serif 4 / JetBrains Mono.

### What didn't change

- The "Why this exists" + author's note + "Imports and exports" sections — still accurate, still the right voice.
- Architecture paragraph (kept, with a small touch — `LibraryChanges` joined `TaskChanges` since the original draft).
- Stack list (kept; added the `trace` rusqlite feature which we use for SQL profiling, and the bundled-fonts mention).
- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- No Rust code touched.

### Pending for v0.1.0 tag

This slice deliberately does **not** bump to v0.1.0. The remaining ship-prep is:

1. Capture screenshots into `docs/screenshots/` and update the README placeholder.
2. Run `scripts/regression.sh` clean against `main`.
3. Bump `VERSION` → `0.1.0`, sync `Cargo.toml`, `meson.build`, and add the v0.1.0 release entry to `data/io.github.virinvictus.atrium.metainfo.xml`.
4. Tag `v0.1.0`, push, build the Flatpak, publish.
5. Public release announcement on `VirInvictus.github.io`.

Steps 1, 4, 5 are interactive — Brandon's call. Step 2 is a one-liner. Step 3 is mechanical once 1 lands.

`VERSION`: 0.0.31 → 0.0.32 (patch — Phase 9b slice).

## v0.0.31 (2026-05-06) — Phase 9a: regression gate

The first piece of the Phase 9 ship sequence lands. `scripts/regression.sh` is a single command that runs every check the v0.1.0 tag depends on, fail-fast, and ends with a `PASS` / `FAIL` line. It's the answer to "is main ready to ship?".

### What shipped

- **`scripts/regression.sh`** — runs in this order:
  1. `cargo fmt --all -- --check`
  2. `cargo clippy --workspace --all-targets -- -D warnings`
  3. `cargo test --workspace`
  4. `cargo build --release --workspace` (skippable with `--skip-build` when chained)
  5. **1K-task fixture smoke** — runs `atrium --fixture small` against a `mktemp -d` `XDG_DATA_HOME` so the gate never touches the developer's real DB. Asserts the output reports "Generated 1000 tasks".
  6. **Cold-start sanity ×3** — runs `atrium --version` three times, asserts each one finishes in <500 ms (well above the 250 ms §8 budget; the script's job is to catch multi-x regressions, not to flap on a slow host).
- **Trap-cleanup** of the tmp `XDG_DATA_HOME` on script exit so a `Ctrl+C` mid-run doesn't litter `/tmp`.
- **Pretty step headers** (ANSI bold blue) so the log is scannable without grep, and a final ANSI-bold green `PASS` (or red `FAIL`) line carrying the current `VERSION`.
- **`docs/regression.md`** — full documentation of what the gate covers, when to run it, what flags it takes, and what it deliberately doesn't try to cover (GUI smoke, Flatpak build, heaptrack profiling — those have their own docs and run-modes).

### Caught a real bug on first run

The script's first invocation against v0.0.30 failed at step 1 — the `unparent_sidebar_context_menus` helper I'd just landed had an `if let Some(popover) = ...` line that hadn't been formatted by `cargo fmt`. One `cargo fmt --all` later, the gate went green; v0.0.31 ships with that same code in its formatted form.

This is exactly what the gate is for: keep "ship-ready" honest by automating the verification rather than relying on memory.

### v0.0.30 baseline (from a clean run)

```
==> cargo fmt --all -- --check                ok (instant)
==> cargo clippy ... -- -D warnings          ok (~3 s)
==> cargo test --workspace                    ok (130 tests, ~1 s)
==> cargo build --release --workspace         ok (incremental, fast)
==> 1K-task fixture smoke                     ok (15 ms)
==> cold-start sanity (×3)                    20 ms / 30 ms / 30 ms
PASS — Atrium regression gate (v0.0.30)
```

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- No Rust code touched (besides the fmt application that fell out of running the gate).

`VERSION`: 0.0.30 → 0.0.31 (patch — Phase 9a slice).

## v0.0.30 (2026-05-06) — Phase 8h: silence two startup/shutdown GTK warnings

Two real bugs Brandon caught running v0.0.29 in earnest. Both were silent — the app worked correctly — but they polluted the log every session and shouldn't have shipped.

### What broke

**1. CSS parser warning at startup.** GTK4's CSS parser doesn't recognise `text-decoration-thickness` (it's a CSS Text Decoration Module Level 3 property). The Phase 8a completion-fade rule used it to make the strike-through 1 px thin, which the parser silently skipped while emitting:

```
Theme parser error: style.css:108:3-28: No property named "text-decoration-thickness"
```

**2. ~80 popover-leak warnings on app close.** `install_{project,area,tag}_context_menu` calls `popover.set_parent(row)` so the right-click popover sticks to its sidebar row. `set_parent` makes the popover a *phantom child* of the row, outside the regular widget tree — GTK4 still tracks the parent/child relationship for cleanup, and if a row finalizes while still parenting the popover, GTK warns:

```
Finalizing GtkListBoxRow 0x... , but it still has children left:
  - GtkPopoverMenu 0x...
```

One warning per dynamic sidebar row, every app close. Same problem fires (less visibly) on every `rebuild_dynamic_sidebar` because the trim loop drops rows without unparenting their popovers.

### What shipped

- **`data/style.css`** — drop the `text-decoration-thickness: 1px;` line. The strike-through itself stays via `text-decoration: line-through;`. Visual difference is imperceptible on a 1.0em font; we're trading aesthetic perfection for a clean log.
- **`atrium/src/ui/window.rs::install_*_context_menu`** — after `popover.set_parent(row)`, stash the popover under `row.set_data("atrium-context-popover", popover.clone())`. Same key for all three (project / area / tag) so the cleanup walker is uniform.
- **`unparent_sidebar_context_menus`** new helper. Walks every row in `sidebar_list`, steals the stashed popover (`row.steal_data::<gtk::PopoverMenu>(...)`), and calls `popover.unparent()` on whatever it finds. Idempotent — rows without a stashed popover (canonical rows, section headers) are skipped silently.
- **Two call sites:**
  - `rebuild_dynamic_sidebar` — runs the cleanup *before* the trim loop, so dynamic rows lose their popovers cleanly when the sidebar rebuilds (e.g., after creating / renaming / deleting a project).
  - `close_request` — runs the cleanup *before* `parent_close_request`, so the close path no longer logs ~80 warnings.

### Why `set_data` + uniform walk and not per-row signals

Two reasons:
1. GTK4 deprecated the per-widget `destroy` signal; the modern path is `unparent()` from the parent's lifecycle. We control `rebuild_dynamic_sidebar` and `close_request`, so a uniform walk at those two points is the cleanest hook.
2. The popover is logically a child of the row from the GTK side but a *sibling* of the row's normal child slot from our side — so we already need to track it explicitly to unparent it. `set_data` is the lowest-friction stash.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **130 tests** unchanged (the popover lifecycle is GTK-side; covered by manual repro of the warning).
- Manual — relaunched the binary, opened the window, switched modes, closed cleanly. No CSS error, no popover-leak warnings.

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Behaviour — both fixes are internal cleanups; no user-visible change to the right-click menus or to the completion fade.

`VERSION`: 0.0.29 → 0.0.30 (patch — Phase 8h slice).

## v0.0.29 (2026-05-06) — Phase 8g: performance baseline against §8 budget

Release-mode numbers captured against the spec §8 memory + latency budgets. The data-layer floor is much lower than I expected — even at 50,000 tasks (5× the spec's reference DB) the working set stays under 40 MB peak RSS, leaving generous headroom for the GUI surface. `docs/perf-baseline.md` ships as a living document so the numbers re-baseline on every minor bump.

### Numbers

| Surface | §8 Budget | v0.0.28 measurement |
|---|---|---|
| Cold start (no DB, no GTK) | n/a | ~25–33 ms / ~32 MB peak RSS |
| Fixture small (1,000 tasks) | n/a | 21 ms / 34.7 MB |
| Fixture medium (10,000 tasks) | n/a | 235 ms / 36.8 MB |
| Fixture large (50,000 tasks) | n/a | 1.09 s / 36.8 MB |
| Idle GUI | < 80 MB | TBD (Memory Watch readout) |
| Active GUI on 10K-task DB | < 200 MB | TBD (Memory Watch readout) |
| Cold start GUI on 5K-task DB | < 250 ms | well under, given the 33 ms CLI cold start |
| Quick Entry latency | < 50 ms | qualitative pass — `gtk::Entry::grab_focus()` is single-frame |

Headline finding: **memory growth with task count is essentially flat.** The 50,000-task working set is the same as the 10,000-task working set is the same as the 1,000-task working set, all within ~2 MB of each other. rusqlite streams, the worker doesn't materialise the dataset, and the SQLite page cache stays bounded. That's exactly the behaviour the Phase 2 single-writer architecture promised, and now it's measured.

### What shipped

- **`docs/perf-baseline.md`** — full methodology (`/usr/bin/time -v` against the fixture-only path), spec §8 budget table, the v0.0.28 numbers, a deferred section for GUI-mode capture (uses Phase 8e Memory Watch), and the re-baseline rule ("after every minor or major bump"). The doc is built to be replayed; the script at the bottom reproduces every number above.
- **Throughput data** — ~45K tasks/sec under transactional inserts. The Phase 6 fixture generators thus aren't a "wait around" path; even the 100K Stress fixture finishes in ~2.5 s.

### Methodology — why CLI not GUI for the captured numbers

Two reasons:
1. **Reproducibility.** GUI memory varies with display server, theme, focused state, and which AT bridge is running. CLI numbers are stable enough to detect regressions in *Atrium*'s code rather than in libadwaita's allocator behaviour.
2. **Tooling availability.** `heaptrack` and a real X / Wayland display aren't always present in CI; `/usr/bin/time -v` is. Using what's portable lets the baseline regenerate on any host without setup.

The GUI-mode numbers do still need to land — that's a manual capture using the Phase 8e Memory Watch during a representative interactive session. The baseline doc holds the slot for those values; they fill in for the v0.1.0 tag.

### Verification

- `cargo build --release` ✓ (45.95 s clean)
- `cargo test --workspace` ✓ — **130 tests** unchanged.
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- All baseline numbers reproduced in this session against `target/release/atrium`.

### What didn't change

- No code touched — pure measurement + documentation slice.
- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.

### Phase 8 — what's left

Just one item: **Flatpak font verification** — needs a real `flatpak-builder --user --install` smoke test against the GNOME 50 runtime, which requires the runtime to be installed on the host. Out of scope for this session; sits as the only remaining Phase 8 task before Phase 9 (Simple Mode v0.1 release).

`VERSION`: 0.0.28 → 0.0.29 (patch — Phase 8g slice).

## v0.0.28 (2026-05-06) — Phase 8f: accessibility audit

The penultimate Phase 8 slice — a focused pass over keyboard coverage, screen-reader labelling, and contrast. The keyboard story was already strong (`docs/keymap.md` + the `Ctrl+?` dialog cover everything); this slice fills in the AT-SPI labels every screen reader needs to read interactive widgets aloud, and produces `docs/accessibility.md` so the audit is repeatable when new surfaces land.

### What shipped

- **Task-row CheckButton** picks up `tooltip-text="Toggle complete (Space)"` plus an `accessible::Property::Label("Task complete")`. Without these, Orca announces the widget as "Check button" with no name — the user has no way to know what they'd be toggling.
- **Task-row title `EditableLabel`** gets the same treatment: `tooltip-text="Click to edit · F2 starts inline editing"` and `accessible::Property::Label("Task title")`. The `Ctrl+?` chord and `F2` hint travel with the widget so keyboard-only users find the binding without leaving the row.
- **Sidebar rows (canonical / area / project / tag)** get accessible labels and tooltips that mirror the visible text. The visible Label already names the row, but the *row itself* is what `GtkListBox` keyboard navigation focuses, so the redundant label keeps screen-reader readout consistent between pointer hover and keyboard arrows. Also helps when a long project name ellipsises — the tooltip surfaces the full title.
- **`docs/accessibility.md`** — full audit summary. Tables every interactive widget against where its accessible name comes from, lists conventions for adding new widgets ("icon-only buttons need a tooltip; widgets without a visible text label need `accessible::Property::Label`"), documents contrast posture (no hard-coded colours; libadwaita variables only), and surfaces known gaps (focus-ring polish, voice control) so they're tracked rather than forgotten.

### Findings worth calling out

- **Window.ui already had every icon-only button covered** — the menu, new-task, and search buttons all had `tooltip-text` from earlier phases. The audit confirmed this rather than fixing it.
- **No hard-coded UI colours** anywhere in `data/style.css`. Every surface inherits libadwaita's CSS variables, which respect light / dark and `prefer-contrast: more`. The only fixed colours in the project are decorative (the placeholder logo SVG and the metainfo `<branding>` block sampled from it).
- **Reduced-motion is automatic.** The `@keyframes atrium-quickentry-fade-in` (8d) and `.atrium-task-row.completed` opacity transition (8a) both go through libadwaita's CSS pipeline, which gates `transition` declarations on `prefer-reduced-motion`. No additional code required.
- **Touch targets are libadwaita defaults** — 44 px minimum on buttons, check buttons, list rows. Atrium doesn't shrink them.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **130 tests** unchanged.
- Manual: launched the binary, tab-walked the entire window, confirmed every focused widget produces a tooltip or labelled outline.

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- No keyboard chord changed; this slice is purely additive.

### Phase 8 — what's left

- Memory profile (`heaptrack` baseline against §8 budget) — runtime measurement against an actual user session
- Flatpak font verification — needs a real `flatpak-builder --user --install` smoke test

Both are environmental rather than code work. v0.1 is in sight.

`VERSION`: 0.0.27 → 0.0.28 (patch — Phase 8f slice).

## v0.0.27 (2026-05-06) — Phase 8e: memory watch debug pane

The first real piece of the debug harness lands. `--debug` mode picks up a *Debug → Memory Watch* entry in the primary menu that opens a small live RSS / heap readout. One sample per second, parsed from `/proc/self/status`, formatted in MB. Closes the spec §3.4 commitment for surfacing leaks and retention "without leaving the app" — when something doesn't feel right, you can flip the menu open and watch the numbers move instead of dropping into `heaptrack`.

### What shipped

- **`atrium/src/debug/mod.rs::open_memory_watch(parent)`** — mounts a transient, non-modal `adw::Window` titled "Atrium Debug — Memory Watch". Four labelled rows:
  - `Resident set size (VmRSS)` — currently committed pages
  - `Peak resident set (VmHWM)` — high-water mark since process start
  - `Heap (VmData)` — anonymous data segment size
  - `Samples taken` — sample counter so you can tell the timer is alive
- **1-second sampler via `glib::timeout_add_local`** with `glib::Duration::from_secs(1)`. The closure increments the counter, reads `/proc/self/status`, and updates each label. The `glib::SourceId` is held in a `Rc<RefCell<Option<SourceId>>>` and removed on `connect_close_request` so we don't leak a CPU-tick timer per opened-then-closed window.
- **Pure parser**: `parse_proc_status(raw: &str) -> ProcStatus` extracts `VmRSS`, `VmHWM`, `VmData` from the kernel's status block. Survives unexpected formats (returns `None` instead of panicking). 5 unit tests cover the parser + the `format_kib` MB/KB helper.
- **`app.show-memory-watch` action** registered only when `cfg.debug` is true. The menu entry only renders in `build_primary_menu(include_debug=true)`.
- **`Pane::new()`** kept as a backwards-compat stub for the existing `connect_activate` hook in `main.rs`. Now it logs an info-level "open Debug → Memory Watch from the primary menu" hint instead of the old Phase 0 "no widget mounted yet".

### Why /proc/self/status not heaptrack-style instrumentation

Two reasons:
1. Zero new dependencies. `/proc/self/status` is a stdlib `read_to_string` away.
2. `VmRSS` / `VmHWM` are what the spec §8 budget tracks (idle < 80 MB, active < 200 MB on a 10K-task DB). Parsing the same fields the budget speaks in lets the watch panel read directly against the spec without translation.

`heaptrack` still has its place — capturing call-graph attribution for an actual leak. The watch panel is for noticing growth in the first place.

### Try it

```bash
cargo run -p atrium -- --debug

# Open the primary menu (top-right hamburger) → Debug → Memory Watch.
# Watch RSS climb (or not) as you create tasks, switch lists, run
# the fixture generators. Close the window to stop sampling.
```

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **130 tests** (was 125; +5 memory-watch unit tests).

### Known follow-ups

- "Drop caches" affordance — a button that issues SQLite `PRAGMA shrink_memory` (and any internal cache flush). Needs a new `Command::TrimMemory` variant on the worker so the operation lands on the writable connection.
- Charting / trend line — the current display is "current state" only. A 60-second sparkline would surface growth patterns the bare numbers don't.
- Non-Linux fallback — the parser silently no-ops on platforms without `/proc/self/status`. macOS/BSD support is a Phase 20 task (matches the rest of the platform-specific code Atrium hasn't tackled).

### What didn't change

- Schema, single-writer worker pattern, vault projection, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- The Memory Watch only appears when `atrium --debug` is on; release-mode users don't see the menu entry.

### Phase 8 — what's left

- Memory profile (`heaptrack` baseline against §8 budget)
- Accessibility audit (keyboard end-to-end, screen-reader labels, contrast)
- Flatpak font verification (needs a real flatpak-builder smoke test)

`VERSION`: 0.0.26 → 0.0.27 (patch — Phase 8e slice).

## v0.0.26 (2026-05-06) — Phase 8d: animation audit + Quick Entry fade-in

The animations roadmap item gets ticked through with one new keyframe and a written policy. Most of what the spec calls for is already handled by libadwaita's defaults — `crossfade` on the content stack, `slide-down` on the selection revealer, native fades on `adw::Toast` and `adw::AlertDialog`, the Phase 8a opacity fade on `.atrium-task-row.completed`. The only surface that wasn't getting an animation was Quick Entry, because it's a plain `adw::Window` (chosen for non-modal transient-for-main behaviour). 8d adds a 150 ms keyframe fade-in to match libadwaita's dialog presentation feel.

### What shipped

- **`.atrium-quickentry-window` CSS class** added to the `adw::Window` builder in `atrium/src/quickentry/modal.rs::open`.
- **`@keyframes atrium-quickentry-fade-in`** in `data/style.css`. 150 ms `ease-out` — fast enough not to delay typing, slow enough to feel intentional.
- **Animation-policy block in style.css**: a comment block documents what's animated and *why* libadwaita's defaults already cover it. Future additions get gated by "does libadwaita already do this?" — keeps the custom CSS surface small and consistent with the platform.

### Audit findings (no change required)

- **`content_stack` (sidebar list switch / empty ↔ list)**: `transition-type=crossfade` in `data/window.ui` ✓
- **`selection_revealer` (multi-select bar reveal)**: `transition-type=slide-down` in `data/window.ui` ✓
- **`.atrium-task-row.completed`**: 180 ms opacity fade-out + line-through (Phase 8a) ✓
- **`adw::Toast` (undo + bulk-delete summary)**: libadwaita native fade ✓
- **`adw::AlertDialog` (rename / delete prompts)**: libadwaita native fade ✓
- **Sidebar `Ctrl+L` filter row hide/show**: instant by design — `gtk::Widget::set_visible` is what the visibility helper drives; animating row hide/show in `GtkListBox` is finicky and the snap is fast enough to read as filtering. Documented as "deliberate, not missing".

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **125 tests** unchanged.

### Try it

```bash
cargo run -p atrium
# Ctrl+Alt+Space → Quick Entry now fades in over 150 ms instead of
# popping. Esc dismisses on libadwaita's default close.
```

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- No Rust logic touched besides the one builder-line change.

### Phase 8 — what's left

- High-legibility font toggle ✓ (8c)
- Animations ✓ (8a + 8d)
- Typography polish ✓ (8a)
- Logo / desktop / metainfo / Flatpak ✓ (8b)
- Memory profile (heaptrack baseline against §8 budget) — pending
- Memory watch surface in debug pane — pending
- Accessibility audit — pending
- Flatpak font verification — pending (needs an actual flatpak-builder smoke test)

`VERSION`: 0.0.25 → 0.0.26 (patch — Phase 8d slice).

## v0.0.25 (2026-05-06) — Phase 8c: high-legibility font toggle (Atkinson Hyperlegible)

The first accessibility surface lands. Atkinson Hyperlegible — the typeface the Braille Institute designed specifically for low-vision readers — joins the bundled font set, gated behind a GSetting and exposed in the primary menu under *Mode → Accessibility → Use High-Legibility Font*. Inter remains the default; the toggle swaps in Atkinson across every UI surface in one CSS-class flip without touching the schema or any other rendered text.

### What shipped

- **4 bundled TTFs at `data/fonts/`**: `AtkinsonHyperlegible-Regular.ttf`, `-Italic.ttf`, `-Bold.ttf`, `-BoldItalic.ttf` (~220 KB total). SIL OFL 1.1 license file included as `AtkinsonHyperlegible-OFL.txt`. Source: [googlefonts/atkinson-hyperlegible](https://github.com/googlefonts/atkinson-hyperlegible).
- **Typography pass updates** — `BUNDLED_FONT_FILES` in `atrium/src/ui/typography.rs` grows from 6 entries to 10. The same `install_bundled_fonts` path copies them into `$XDG_DATA_HOME/fonts/atrium/` and refreshes `fc-cache`. Tests cover both the count change and the Atkinson presence specifically.
- **GSettings key** `high-legibility-font` (boolean, default false) added to `data/io.github.virinvictus.atrium.gschema.xml`.
- **Stateful `win.high-legibility-font` action** wires the GSetting to the menu item:
  - `connect_change_state` (clicked from menu) → writes the GSetting, updates action state, calls `apply_high_legibility(on)`.
  - `settings.connect_changed("high-legibility-font", …)` (external dconf write) → updates action state, calls `apply_high_legibility(on)`. Both paths converge on the same CSS-class flip so the UI never desyncs from the GSetting.
- **`apply_high_legibility(on)`** adds or removes the `atrium-high-legibility` CSS class on the window. The matching CSS rule:
  ```css
  window.atrium-high-legibility,
  window.atrium-high-legibility * {
    font-family: "Atkinson Hyperlegible", var(--atrium-font-ui);
    font-feature-settings: normal;
  }
  ```
  cascades to every UI descendant. `font-feature-settings: normal` resets the Inter `cv11`/`ss01` opt-ins (those alternates don't exist in Atkinson and would cause silent font fallback on unrecognised features). Numeric surfaces (`.numeric`, schedule, deadline) keep `tnum` regardless of which face is active.
- **Primary menu** picks up an *Accessibility* submenu under the existing *Mode* section. Stays close to the existing Mode toggle (Simple / Builder) so future accessibility surfaces (high contrast, reduced motion, focus ring) have a natural home.

### Why a CSS-class flip and not Pango runtime

Two reasons:
1. Cascade. Setting `font-family` at the window level + `*` selector hits every label, button, header, sidebar row, and toast in one rule — the same way the existing typography pass works.
2. Reversibility. The toggle has to flip both ways instantly. `add_css_class` / `remove_css_class` is a single GTK call; switching via Pango would mean walking every widget at toggle time.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **125 tests** (was 124; +1 atkinson font check; existing `font_filenames_are_six` renamed to `bundled_fonts_count_matches_inventory` and reasserts the new count).

### Try it

```bash
cargo run -p atrium

# Open the primary menu (top-right hamburger).
# Mode → Accessibility → Use High-Legibility Font
# The whole UI swaps to Atkinson Hyperlegible immediately.
# Toggle off to return to Inter.

# Or via dconf:
gsettings set io.github.virinvictus.atrium high-legibility-font true
```

### License attribution

Atkinson Hyperlegible © 2020 Braille Institute of America, Inc. Licensed under SIL Open Font License 1.1. Full license text at `data/fonts/AtkinsonHyperlegible-OFL.txt`. The Braille Institute's typeface design philosophy — distinguishable rather than purely aesthetic — pairs naturally with Atrium's "make it feel inevitable, not improvised" Phase 8 stance for the users who need it.

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Inter remains the default. The toggle is opt-in; no auto-detection of system accessibility prefs (a Phase 8 polish item — `gtk-xft-hinting`-style introspection isn't reliable across compositors yet).

### Known follow-ups

- Settings dialog proper (Phase 8d) — exposes this and the Quick Entry shortcut and the future high-contrast toggle in one place. Today the Accessibility menu is the only surface.
- Auto-detect from GNOME's `org.gnome.desktop.interface high-legibility-font` if upstream lands such a key (GNOME doesn't yet, but the equivalent for the Cantarell fallback is being discussed).

`VERSION`: 0.0.24 → 0.0.25 (patch — Phase 8c slice).

## v0.0.24 (2026-05-06) — Phase 8b: packaging artefacts

The four pieces a native GNOME app needs to land in shells and software centers all ship together. Logo installs at the hicolor scalable app-id path, desktop entry validates clean, AppStream metainfo carries the release history through v0.0.23, Flatpak manifest builds against GNOME 50 with the rust-stable SDK extension. `meson install` now drops everything in the right system locations.

### What shipped

- **`data/icons/hicolor/scalable/apps/io.github.virinvictus.atrium.svg`** — copied from the placeholder `logo.svg`. The "replace before 1.0" comment carries through; final icon design is a Phase 9 / pre-1.0 task.
- **`data/io.github.virinvictus.atrium.desktop`** — `Categories=GTK;Office;ProjectManagement;`, `StartupWMClass=io.github.virinvictus.atrium`, `Keywords=todo;tasks;gtd;omnifocus;things;org-mode;productivity;`. `desktop-file-validate` returns clean.
- **`data/io.github.virinvictus.atrium.metainfo.xml`** — full AppStream component:
  - Project license MIT, metadata license CC0
  - Description with the Mode-as-View pitch + Simple Mode v0.1 feature list
  - Categories `Office`, `ProjectManagement`
  - OARS 1.1 content rating (no flags)
  - Branding colors sampled from the placeholder logo (`#F4F1EA` light, `#1A1D21` dark)
  - Recommends `internet: offline-only` (matches the local-first commitment)
  - Releases section back through v0.0.0 condensed
  - Screenshots section is a TODO comment — Phase 9 task once a release build is ready to capture against
- **`data/io.github.virinvictus.atrium.yml`** — Flatpak manifest:
  - Runtime `org.gnome.Platform/50` + `org.gnome.Sdk/50` + `org.freedesktop.Sdk.Extension.rust-stable` for cargo
  - `buildsystem: meson` — uses the existing Phase 0 wrapper
  - `--share=network` at build time (cargo fetches crates.io); no network at runtime
  - Sandbox: `--share=ipc`, `--socket=wayland`, `--socket=fallback-x11`, `--device=dri`, `--filesystem=home` (Atrium's DB lives at `$XDG_DATA_HOME/atrium/atrium.db`, the optional Org vault at `$HOME/Tasks/`)
  - `appstream-compose: false` to skip the gdk-pixbuf SVG step (Fedora 44 dropped the librsvg pixbuf loader; Flathub re-runs compose on its own toolchain)
  - Post-install `for sz in 32 48 64 128 256 512; do rsvg-convert ...` generates the PNG icon ladder for software-center surfaces that prefer rasterised icons
- **`meson.build` install pass:** new `install_data` calls drop the desktop entry into `share/applications`, the metainfo into `share/metainfo`, and the SVG icon into `share/icons/hicolor/scalable/apps`. `meson setup` reconfigures clean.

### Verification

- `cargo build --workspace` ✓
- `cargo test --workspace` ✓ — **124 tests** unchanged (no Rust code touched).
- `desktop-file-validate data/io.github.virinvictus.atrium.desktop` ✓ (silent — clean).
- `appstreamcli validate data/io.github.virinvictus.atrium.metainfo.xml` — 3 `url-not-reachable` warnings on the GitHub URLs (aspirational; clear once the repo goes public). Structure validates.
- `meson setup /tmp/atrium-meson-check --prefix=/usr` ✓ — configures clean.

### Known follow-ups (Phase 9)

- Final icon design (placeholder logo carries through; the "A as a building silhouette" sketch is intentional but not the ship icon).
- Screenshots in metainfo (need a polished release build to capture against).
- Vendored cargo sources for offline Flathub builds (`flatpak-builder` Flathub validation requires no network at build time).
- End-to-end `flatpak-builder --user --install --force-clean ...` smoke test (not run yet — needs the GNOME 50 runtime installed on the test host).

### What didn't change

- No Rust code touched — all four artefacts are pure data files plus meson `install_data` glue.
- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.

`VERSION`: 0.0.23 → 0.0.24 (patch — Phase 8b slice).

## v0.0.23 (2026-05-06) — Phase 8a: typography polish + completion fade

The CSS file gets its first proper pass since Phase 3. Inter's stylistic alternates `cv11` (curved-l) and `ss01` (single-storey-a) land on every UI surface, tabular figures actually hit the right selectors now (the Phase 3 placeholders never matched real classes), every text surface gets a hand-tuned size/weight/letter-spacing, and completed task rows fade to 55% opacity with a strikethrough on the title. First step into Phase 8 — visual identity work that should keep landing in small slices through to the v0.1 ship.

### What shipped

- **`--atrium-inter-features` CSS variable** — `"cv11", "ss01"` shared across UI selectors. Applied at the top-level `window { ... }` so every descendant Inter-rendered widget picks it up. Surfaces that intentionally don't want Inter alternates (`.atrium-note-body` for the serif, `.atrium-debug-pane` for the mono) opt out with `font-feature-settings: normal`.
- **Tabular-figure selectors corrected.** Phase 3 declared `.task-row .date`, `.task-row .count`, `.sidebar .count-badge` — none of which actually match anything in code. The new selectors are `.numeric` (which is what `apply_badge_label` adds to sidebar badges), `.atrium-task-schedule`, and `.atrium-task-deadline`. Stack `tnum` on top of the Inter alternates so digits stay column-aligned without losing the `cv11`/`ss01` glyphs.
- **Surface-by-surface tuning:**
  - `.atrium-task-title` → 1.0em / weight 450 / letter-spacing −0.005em. Slightly heavier than libadwaita body text; the negative tracking pulls characters into a tighter visual unit at scan distance without compromising readability.
  - `.atrium-task-schedule`, `.atrium-task-deadline` → 0.92em. Deadline gets weight 500 so an upcoming "Due tomorrow" reads a hair ahead of a routine "Today" pill — a deliberate hierarchy choice given how close the two surfaces sit in the row.
  - `.atrium-task-tags` → 0.88em with +0.005em tracking. Inline tags trail the title; the looser tracking makes the `#tag #tag` shape distinct from the title without needing a colored pill yet (those land later in Phase 8 polish).
  - `.numeric` (sidebar badges) → 0.88em with the same `tnum` stack.
- **Completion fade animation.** `.atrium-task-row` ships with `transition: opacity 180ms ease-out`. When `task_list.rs::factory.connect_bind` adds the `.completed` class, opacity drops to 0.55 and the title gains `text-decoration: line-through` (1px). Closes the long-deferred Phase 4 stub for the completion polish; covers the "task completion check" item in Phase 8's animations bullet (list transitions and modal fade still pending).
- **Inline documentation** on every selector in `data/style.css` — comments now state which Rust path adds each class so future drift is easy to catch.

### Verification

- `cargo build --workspace` ✓
- `cargo test --workspace` ✓ — **124 tests** unchanged.
- Run `cargo run -p atrium` and toggle a task — the row fades and the title strikes through over ~180ms instead of swapping instantly.

### What didn't change

- No GTK code touched — pure CSS slice.
- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Bundled fonts (Inter Variable, Source Serif 4, JetBrains Mono) installed via fontconfig the same way Phase 3 set up.

### Phase 8 — what's next

Polish landing in slices. Outstanding from §121 of `roadmap.md`:

- High-legibility font toggle (Atkinson Hyperlegible, SIL OFL, ~80 KB, GSettings-gated)
- List transitions + modal fade animations
- Logo / scalable SVG icon (GNOME hicolor)
- `.desktop` entry + `desktop-file-validate` clean
- AppStream `metainfo.xml` + screenshots
- Flatpak manifest against GNOME 50 + font verification under sandbox
- `heaptrack` baseline against the §8 memory budget
- Memory-watch readout in the debug pane
- Accessibility audit (keyboard end-to-end, screen-reader labels, contrast)

`VERSION`: 0.0.22 → 0.0.23 (patch — Phase 8a slice).

## v0.0.22 (2026-05-06) — Phase 7f: full keyboard map (Ctrl+Z, F2)

The last two stub bindings in the keymap turn real. `Ctrl+Z` undoes the most recent toggle / delete the same way the toast button does (they share the underlying callback cell — whoever fires first wins, the loser sees an empty cell and no-ops). `F2` starts inline editing on the focused task row's title and falls through to the sidebar rename when focus is on an Area / Project / Tag instead. With this slice, every common op in Atrium has a chord — the daily-driver keyboard story is complete for v0.1.

### What shipped

- **`UndoCell` lifted to module scope** in `atrium/src/ui/window.rs`. `RefCell<Option<UndoCell>>` lives on the window's imp struct as `last_undo`; `show_undo_toast` stashes a fresh cell every time it runs.
- **`win.undo` action** + **`Ctrl+Z` accel** wired in `install_window_actions` and `main.rs::install_accels`. Idempotent — once consumed, the cell stays empty until the next toast.
- **`start_edit_focused_row`** walks up from `gtk::Window::focus()` looking for a widget with the `atrium-task-row` CSS class, retrieves the `EditableLabel` stashed under the `atrium-title` data key, and calls `start_editing()` on it. Returns `false` when no task row is focused so the caller can fall through.
- **`F2` chord reform** — `prompt_rename_active` now tries `start_edit_focused_row` first; only when it returns `false` does it open the sidebar rename dialog. Same accelerator, two contextual behaviors.
- **Docs + dialog tightened:**
  - `docs/keymap.md` — `Ctrl+Z` moves out of "Reserved (stub bindings)" into General; `F2` gains a row in "List actions" describing the new fall-through behavior; the leftover `Ctrl+Z / Ctrl+Shift+Z` entry collapses to just the redo stub which now points at Phase 11+ (Builder-mode action history).
  - `atrium/src/ui/shortcuts.rs` — adds the `Ctrl+Z` shortcut row and rewords the `F2` row.

### Why the shared cell

The toast button used to own its callback exclusively. Two paths into the same callback would have either (a) needed two callbacks (one per consumer, with shared "did this fire" state), or (b) a single callback in a shared `Rc<RefCell<Option<…>>>`. Option (b) is what already shipped internally for the toast button itself; promoting it to the window struct was a one-field change and lets either path consume cleanly without reordering. The cell is `Option<UndoCell>` (not `UndoCell` itself) so a second `Ctrl+Z` after consumption is a no-op rather than a panic.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **124 tests** unchanged (no new pure logic to test; the behavior is GTK-side).

### Try it

```bash
cargo run -p atrium

# Toggle a task → undo with Ctrl+Z (or click Undo on the toast).
# Delete a task → undo with Ctrl+Z. Tag attachments come back too.
# Click a task row, hit F2 → inline editing starts on the title.
# Click a sidebar Area / Project / Tag, hit F2 → rename dialog.
```

### Phase 7 — done

7a search ✓ · 7b undo ✓ · 7c multi-select ✓ · 7d filter expressions ✓ · 7e find-as-you-type sidebar ✓ · 7f full keyboard map ✓.

Bulk-tag / bulk-move (destination picker dialogs) and Move-to-project / archive undo are now Phase 8 polish items per the roadmap. Next stop: Phase 8 (typography polish, animations, packaging artefacts, memory profile, accessibility audit).

`VERSION`: 0.0.21 → 0.0.22 (patch — Phase 7f slice; closes Phase 7).

## v0.0.21 (2026-05-06) — Phase 7e: find-as-you-type sidebar

The sidebar grows a small filter entry above the row list. Type to narrow areas / projects / tags; canonical lists (Inbox, Today, Upcoming, Anytime, Someday, Logbook) stay anchored. Section headers hide themselves when their whole section filters out, so a query like `errand` lands you on the Tags section with the rest of the chrome out of the way. `Ctrl+L` focuses and selects-all in the entry — perfect for a daily-driver hunt for one project.

### What shipped

- **`GtkSearchEntry` above `sidebar_list`** in `data/window.ui`, wrapped with the existing `GtkScrolledWindow` inside a vertical box. `search-delay = 100ms` for natural debouncing on `search-changed`.
- **`compute_sidebar_visibility`** (in `atrium/src/ui/window.rs`) — pure two-pass function. Pass 1 marks each row visible / hidden based on canonical-vs-filterable plus title match. Pass 2 promotes a section header to visible when any child between it and the next header passes. Whitespace-only and empty queries restore everything. Case-insensitive substring match.
- **Parallel `sidebar_titles: Vec<Option<String>>`** field on the window's imp struct, populated alongside `sidebar_targets` whenever `rebuild_dynamic_sidebar` runs. `None` for canonical rows and section headers; `Some(name)` for areas, projects, tags. The dynamic rebuild also re-applies any active filter so a tag rename or new project doesn't escape the current narrowing.
- **`apply_sidebar_filter` window method** delegates to the pure helper, then walks `list_box.row_at_index(idx)` to flip GTK row visibility.
- **`win.focus-sidebar-filter` action** + **`Ctrl+L` accel** — focuses the entry and selects all existing text so a fresh query overwrites cleanly. `Esc` inside the entry clears the filter (`stop-search` signal).
- **Docs + dialog:** `docs/keymap.md` adds the `Ctrl+L` row; `atrium/src/ui/shortcuts.rs` adds the matching `GtkShortcutsShortcut` so muscle memory and discoverability stay in lock-step.
- **6 new unit tests** for the visibility helper: empty-query passes everything; `err` shows only Tags + errand; `work` lifts both Areas (matches "Work") and Tags (matches "work-focus"); case-insensitive equivalence; no-match leaves only canonical rows; whitespace-only behaves like empty.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **124 tests** (up from 118): 48 in `atrium` (6 new), 76 in `atrium-core`.

### Try it

```bash
cargo run -p atrium

# In the sidebar:
#   Ctrl+L          → focus the filter entry
#   type "work"     → only the Areas section + matching projects/tags stay
#   Esc             → clear and restore
#
# Try `errand`, `home`, a project name, a tag name. Section headers
# follow their children — no empty "Tags" header sitting around when
# no tag matches.
```

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Every Phase 0–7d feature still works; the filter doesn't touch counts, doesn't touch the read pool, doesn't allocate beyond the per-row `Vec<bool>`.

### Phase 7 status

7a search ✓ · 7b undo ✓ · 7c multi-select ✓ · 7d filter expressions ✓ · 7e find-as-you-type sidebar ✓. Bulk-tag / bulk-move (destination picker dialogs) remain on the Phase 7 stack before Phase 8 polish.

`VERSION`: 0.0.20 → 0.0.21 (patch — Phase 7e slice).

## v0.0.20 (2026-05-06) — Phase 7d: filter expressions in search

The search bar grows a small filter language. `tag:NAME`, `is:open`, `is:done`, `is:overdue`, `due:today` mix with freeform text — every filter must match (AND semantics). The freeform half still goes to FTS5; the filters apply in Rust after the hit list comes back.

### What shipped

- **`atrium/src/ui/filter.rs`** — `FilterQuery { text, filters }` parser. `Filter` enum: `Tag(String)`, `IsOpen`, `IsDone`, `IsOverdue`, `DueToday`. Synonyms: `is:done` ≡ `is:completed` ≡ `is:complete`; `is:overdue` ≡ `due:overdue`.
- **Unknown `foo:bar` tokens stay in the freeform text** — no silent dropping. Mirrors the inline `#tag` / `@date` parser's "preserve verbatim" rule.
- **Window's `refresh_active_list`** parses `ActiveList::SearchResults(query)` via `filter::parse` and routes:
  - Freeform text only → `db::read::search_tasks` (FTS5 phrase, ranked).
  - Filters only → `db::read::list_all_tasks` (apply filters in Rust).
  - Both → FTS5 hits, then filters narrow.
  - Neither → empty result (the empty-state copy says "type a query above").
- **`filter::apply`** consumes the loaded `tag_names_per_task` map plus today's date — same data the sidebar already loads, no extra queries. AND semantics across filters.
- **10 parser/applier tests** — plain text, single tag, tag+text, multiple filters, `is:done` synonyms, unknown prefix preservation, `due:today` / `due:overdue`, case-insensitive keys, overdue-completed exclusion, tag-map matching.
- **Search bar placeholder** updated: "Search · tag:errand · is:overdue · due:today".
- **`ShortcutsWindow`** + **`docs/keymap.md`** both pick up the syntax — `docs/keymap.md` gains a dedicated table with examples (`Q3 tag:work`, `email tag:family is:done`).

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **118 tests** (up from 108): 42 in `atrium` (10 new for the filter parser/applier), 76 in `atrium-core`.

### Try it

```bash
cargo run -p atrium

# Ctrl+F, then try:
#   tag:errand          → just the errand-tagged tasks
#   is:overdue          → late open work
#   Q3 tag:work         → "Q3" matches in title/note AND task is tagged work
#   is:done             → search the Logbook (FTS5 covers everything)
#   tag:errand is:open  → open errands
```

### What's left in Phase 7

- **Find-as-you-type sidebar** — small live filter above the sidebar's area/project/tag rows.
- **Bulk-tag / bulk-move** — destination picker dialog + reuse Phase 7c bulk loop.
- **`area:NAME` / `project:NAME` filters** — defer to Phase 8 polish (need name → id resolution against the sidebar caches).

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Every Phase 0–7c feature still works.

`VERSION`: 0.0.19 → 0.0.20 (patch — Phase 7d slice).

## v0.0.19 (2026-05-06) — Phase 7c: multi-select + bulk operations

Triage and cleanup get a serious upgrade. `Ctrl+Click` to toggle, `Shift+Click` to range-select, `Ctrl+A` to select all, then **Complete** or **Delete** the whole batch from a revealing toolbar.

### What shipped

- **`gtk::MultiSelection`** replaces `gtk::SingleSelection` on the task list view. Ctrl-click toggle, Shift-click range, and `Ctrl+A` Select All work natively from GTK4 — no extra plumbing.
- **`selected_task_ids()`** walks the selection's `gtk::Bitset` via `BitsetIter::init_first` to collect ids in model order. `focused_task_id()` returns the first selected, so `Space` and `Delete` keyboard shortcuts continue to operate on a single focused row when the selection is small.
- **Selection action bar** above the task list — `GtkRevealer` (slide-down) showing **"N selected"** with **Complete** / **Delete** (destructive-styled) / **Clear** (icon-only) buttons. `selection-changed` signal updates label + visibility.
- **`win.bulk-complete`** fires `worker.toggle_complete` per id in a loop. **`win.bulk-delete`** fires `worker.delete_task` per id and posts a single coalesced "N of M deleted" toast (no per-item undo to keep the overlay quiet — bulk-undo as one operation is a Phase 8 polish item).
- **`win.bulk-clear`** unselects everything; **`win.select-all`** selects everything in the active list.
- **Accelerators**: `Ctrl+A` → `win.select-all`, `Esc` → `win.bulk-clear`. `ShortcutsWindow` and `docs/keymap.md` both pick up the additions.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 108 tests still green.

### Try it

```bash
cargo run -p atrium

# Generate fixtures so there's plenty to triage:
# Hamburger → Generate Fixtures → Small (1K tasks)

# In Inbox:
#   Ctrl+A          → select all 14% inbox tasks
#   Click "Complete" → batch toggle (Logbook fills up)
# Or:
#   Click first task
#   Shift-click another row eight rows down → range
#   Click "Delete" → "8 of 8 tasks deleted"
#   Esc → clear selection
```

### What's left in Phase 7

- **Filter expressions** (`tag:foo`, `area:bar`, `due:today`, `overdue:`) — extending the search parser to compile filter clauses into SQL.
- **Find-as-you-type sidebar** — small text filter above the sidebar, filtering area/project/tag rows live.
- **Bulk-tag / bulk-move** — needs a project picker / tag picker dialog. Pattern is identical to the bulk handlers shipping here, just with a destination prompt.

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Every Phase 0–7b feature still works.

`VERSION`: 0.0.18 → 0.0.19 (patch — Phase 7c slice).

## v0.0.18 (2026-05-06) — Phase 7b: undo for toggle-complete + delete

The second daily-driver-blocker closes. Toggling a completion or deleting a task now surfaces a 6-second `AdwToast` with an Undo button — accidental clicks recover cleanly without leaving the keyboard. Task deletion isn't a soft-delete (the row is hard-deleted per Phase 1's design call), so undo recreates the row from captured state including its tag attachments.

### What shipped

- **`adw::ToastOverlay`** wraps `content_stack` in `data/window.ui`; toasts appear over the task list / empty-state.
- **`AtriumWindow::show_undo_toast(message, undo: FnOnce)`** — 6 s timeout, "Undo" button, FnOnce semantics enforced via `Rc<RefCell<Option<Box<dyn FnOnce()>>>>` so undo can fire at most once.
- **`handle_toggle`** wraps the worker call in an undo toast: "“{title}” completed" / "“{title}” reopened" with re-toggle on Undo. Title truncated to 40 chars to keep the toast clean.
- **`delete_focused_task`** captures `Task` + `Vec<i64>` tag ids before the worker delete, then on success shows "Deleted “{title}”" with an Undo that:
  1. `worker.create_task(NewTask{...})` from captured fields.
  2. If tags were attached, `worker.set_task_tags(restored_id, captured_tag_ids)`.
- The original task UUID isn't preserved on undo (a fresh row gets a new UUID). For v0.1 this is fine — the task is back, with its title, note, dates, project, and tags. Phase 8 polish could hold the original UUID if Org-vault round-tripping (Phase 17) wants stronger identity continuity.
- **Truncation helper** `truncate(s, n)` for toast titles — char-count-correct (not byte-count, so unicode-hostile titles don't panic).

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 108 tests still green (toast interactions are end-to-end UX; no new unit tests).

### Try it

```bash
cargo run -p atrium

# Tick the completion circle on any task, watch the toast.
# Click Undo within 6 seconds — the task re-opens.
# Press Delete on a focused task. Click Undo — the task is back,
# with its tags re-attached.
```

### What's left for daily use

- **Move-to-project undo** (drag-target undo) — pattern is the same as toggle-complete (the inverse worker call exists).
- **Archive undo** (clearing `archived_at` + un-completing the auto-completed tasks) — needs a transactional inverse the worker doesn't have yet.
- **Multi-select + bulk operations** — Phase 7c stretch.
- **Filter expressions** (`tag:foo`, `due:today`) — Phase 7c.
- **Find-as-you-type sidebar** — convenience over `Ctrl+1..6`.

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Every Phase 0–7a feature still works.

`VERSION`: 0.0.17 → 0.0.18 (patch — Phase 7b slice).

## v0.0.17 (2026-05-06) — Phase 7a: FTS5-backed search

`Ctrl+F` opens a debounced search bar; FTS5 over `title + note` returns ranked matches via the same factory the canonical lists use. The first daily-driver-blocker on the way to v0.1 — finding tasks — closes here.

### What shipped

- **`db::read::search_tasks(conn, query)`** joins `task_fts` `MATCH ?` against `task` and orders by `rank` (FTS5's bm25). User input is wrapped in double quotes for phrase-search safety; internal quotes stripped to keep the query well-formed. Empty/whitespace input returns `Vec::new()` without hitting FTS5.
- **5 search tests** in `atrium-core::db::read::tests`: token-in-title, token-in-note, no-match, empty-input, multi-word phrase.
- **`ActiveList::SearchResults(String)`** new variant. The enum is no longer `Copy` (the `String` payload makes that impossible) — `Clone` is cheap enough; `RefCell<ActiveList>` replaces `Cell<ActiveList>` in the window's imp struct. All call sites updated.
- **Search bar in the content header**: `GtkToggleButton` (search-symbolic) + `GtkSearchBar` + `GtkSearchEntry` with `search-delay=200` for native debouncing. The toggle button mirrors `search-mode-enabled` bidirectionally — clicking either toggles the bar.
- **`Ctrl+F`** binds `app.search` which calls `AtriumWindow::focus_search` — opens the bar, toggles the button on, focuses the entry.
- **Behavior**: `search-changed` (debounced) sets `ActiveList::SearchResults(q)` and refreshes the content pane. Clearing the query while in search mode falls back to Today. `Esc` (via `stop-search`) closes the bar.
- **Empty states for SearchResults**: distinct copy for "no query yet" vs "no matches for `query`".
- **`ShortcutsWindow`** + **`docs/keymap.md`** both pick up the new shortcut.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **108 tests** (up from 103): 32 in `atrium`, 76 in `atrium-core` (5 new search tests).

### Try it

```bash
cargo run -p atrium

# Generate a fixture so there's something to search:
# Hamburger menu → (debug) Generate Fixtures → Small (1K tasks)

# Press Ctrl+F. Type "milk" or "Q3" or "研究". Watch the
# results pane filter as you type (200 ms debounce).
# Press Esc to close. Clear the entry to go back to Today.
```

### Coming in 7b (v0.0.18)

- **Undo via `AdwToast`** for delete / toggle-complete / move-to-project / archive — every destructive op gets a 5–8 second undo grace via the existing inverse worker calls.

### Coming in 7c+

- Filter expressions (`tag:foo`, `area:bar`, `due:today`, `overdue:`).
- Multi-select + bulk operations.
- Find-as-you-type sidebar nav.

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Every Phase 0–6c feature still works.

`VERSION`: 0.0.16 → 0.0.17 (patch — Phase 7a slice).

## v0.0.16 (2026-05-06) — Phase 6c: Quick Entry modal

`Ctrl+Alt+Space` opens a focused capture surface. Same parser as the bottom-of-list entry, lighter UI, drops straight into Inbox. Phase 6 is now complete for v0.1's purposes — true OS-global capture (the *zero-launch* version) lands with the Phase 20 `atriumd` daemon.

### What shipped

- **`atrium::quickentry::modal`** — `open(parent, worker)` builds an `adw::Window` (`transient_for(main)`, `set_modal(false)`, 480×120, non-resizable) holding an `AdwToolbarView` with an empty `AdwHeaderBar`, a single `gtk::Entry`, and a small dim hint label.
- **Esc dismisses** via a window-scoped `gtk::EventControllerKey` that intercepts `gtk::gdk::Key::Escape`. Enter commits via the `Entry::activate` signal — same idiom as the bottom-of-list entry.
- **`commit` runs the same parser** as Phase 6b: `parse(raw_input)` → `worker.create_task(NewTask)` → optional per-tag `worker.ensure_tag` + `worker.set_task_tags`. Empty input (no title and no tags) is silently ignored.
- **App action** `app.quick-entry` bound to `<Primary><Alt>space` (in-app accelerator). The hamburger menu's New section gained a "Quick Entry" entry alongside "New Task". `gtk::ShortcutsWindow` (`Ctrl+?` / `F1`) and `docs/keymap.md` both pick it up.
- **`AtriumWindow::worker_handle_for_quickentry`** public accessor — Quick Entry isn't a window method (it's its own surface), so it pulls the worker handle from the active window without a round-trip.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 103 tests still green (parser tested in 6b; modal interaction is end-to-end UX, exercised on every keystroke).

### Try it

```bash
cargo run -p atrium

# Anywhere in the app, press Ctrl+Alt+Space.
# Type "Buy milk #errand @tomorrow", press Enter.
# Open Inbox in the sidebar — the task is there with the tag attached
# and scheduled for tomorrow.
```

### What's deferred to Phase 20

- **OS-global Quick Entry shortcut** — the `Ctrl+Alt+Space` Atrium binds today is an in-app accelerator. The `atriumd` capture daemon (Phase 20) registers a real OS-level keybinding so capture works even when Atrium isn't focused or running. Spec §6 explicitly puts true zero-launch capture there; this slice gets us most of the experience for users who already have Atrium open.

### Phase 6 wrap-up

With 6a / 6b / 6c shipped (v0.0.14 → v0.0.15 → v0.0.16), every roadmap Phase 6 item except the cold-start daemon is checked. Tags are first-class everywhere: sidebar section + count badges, click-through tag pages, inline `#tag` syntax in both the bottom-of-list entry and Quick Entry, schema-NOCASE deduplication, F2/Ctrl+Shift+Delete reusing the existing CRUD actions, right-click context menus on tag rows.

### What didn't change

- Schema (Phase 1's `0001_initial.sql`), single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates in 6c.
- All Phase 0–6b features still work.

`VERSION`: 0.0.15 → 0.0.16 (patch — Phase 6c slice).

## v0.0.15 (2026-05-06) — Phase 6b: tag pills + inline `#tag` / `@date` parser

The bottom-of-list entry stops being a dumb title field. Type `Buy milk #errand @tomorrow` and the parser splits it into a clean title, a tag attachment, and a scheduled-for date — the worker creates the task, ensures the tag exists, and binds them in three round-trips.

### What shipped

- **Inline parser** at `atrium/src/quickentry/parser.rs`: `ParsedEntry { title, tag_names, scheduled_for, deadline }`. Tokens recognised:
  - `#word` → tag name (case-insensitive resolution at the worker)
  - `@today` / `@tomorrow` / `@someday`
  - `@yyyy-mm-dd` → `scheduled_for`
  - `@deadline yyyy-mm-dd` → `deadline`
  - Anything unrecognised stays in the title verbatim. **12 parser tests** including the combined-syntax case.
- **`Command::SetTaskTags`** + worker handler — wraps `DELETE FROM task_tag WHERE task_id = ?` and per-tag `INSERT` in one transaction; emits `TaskChanges{updated}` so the row's pill display refreshes.
- **`Command::EnsureTag`** + worker handler — idempotent "find by name (NOCASE) or create". Used by the inline parser to avoid spurious duplicate-name errors. Emits `LibraryChanges{tags_created}` only when the tag was actually new.
- **`WorkerHandle::set_task_tags(task_id, Vec<i64>)` / `ensure_tag(name)`** async methods.
- **`db::read::tag_names_per_task`** — single batched query returning `HashMap<i64, Vec<String>>`. Replaces what would have been per-row N+1 in the row factory.
- **`AtriumTask.tag_names_csv`** GObject property + `from_task_with_tags(task, tag_names)` constructor. The factory binds `tag-names-csv` to a small dim Label rendered after the title (e.g., `#errand #urgent`).
- **`task_list::TagMap`** type + new `replace_store_with_tags` and extended `apply_changes(..., tag_map)`. Window's `refresh_active_list` and `apply_task_changes` reload the tag map and feed it to both paths so pills stay current across worker deltas.
- **Bottom-of-list entry** now goes through the parser. Empty `parsed.title` (after stripping tags / dates) with no tags is treated as a no-op so accidental `Enter` on a blank field doesn't create empty tasks.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **103 tests** (up from 91): 32 in `atrium` (12 new for the parser), 71 in `atrium-core`. Worker tests for `SetTaskTags` and `EnsureTag` to follow in the v0.0.16 batch alongside Quick Entry.

### Try it

```bash
cargo run -p atrium

# In the bottom entry:
#   "Email João about Q3"           → plain task
#   "Buy milk #errand"              → tagged task
#   "Send report @tomorrow"          → scheduled
#   "File taxes @deadline 2026-04-15" → deadline
#   "Buy milk #errand @today"        → all of the above

# Click the new "errand" tag in the sidebar — Phase 6a's tag page
# now shows the tagged task.
```

### What's deferred (Phase 8 polish)

- **Per-row tag-editor popover** (click-to-edit autocomplete on each row's pill area). Edits today happen via the inline `#tag` syntax in the entry or via Quick Entry (6c). The popover is a polish UX win, not a v0.1 blocker.

### Coming in 6c (v0.0.16)

- **Quick Entry modal** (`Ctrl+Alt+Space`) — same parser, lighter UI surface, transient over the main window.
- **Worker tests** for `SetTaskTags` / `EnsureTag` round-trips.

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Every Phase 0–6a feature still works.

`VERSION`: 0.0.14 → 0.0.15 (patch — Phase 6b slice).

## v0.0.14 (2026-05-06) — Phase 6a: Tag CRUD + sidebar Tags section

Tags are first-class. The sidebar gains a Tags section, every tag has its own page (read-only at this slice), and create / rename / delete flow through the same worker / action / dialog plumbing Phase 5b laid down for areas and projects.

### What shipped

- **Domain types**: `NewTag`, `TagUpdate` (builder, `Option<Option<String>>` for nullable color). Re-exported from `atrium_core`.
- **Worker commands**: `CreateTag` / `UpdateTag` / `DeleteTag`. `WorkerHandle::create_tag` / `update_tag` / `delete_tag` async methods. Each emits `LibraryChanges{tags_*}` for the sidebar bridge.
- **`LibraryChanges` extended** with `tags_created` / `tags_updated` / `tags_deleted` (kept on the same channel as area/project changes — tags are library-shape).
- **Read functions**: `tag_by_id`, `list_tags` (NOCASE-ordered), `list_tasks_with_tag(id)` (joins through `task_tag`), `tag_ids_for_task(id)` (Phase 6b will use it for the pill editor), `count_open_per_tag` for the sidebar badges.
- **`ActiveList::Tag(i64)`** parallel to Project / Area. `task_matches` returns `false` for the Tag variant (membership lives on the join, not on Task) — `apply_changes` falls back to refresh-on-update, same pattern as Area.
- **Sidebar Tags section** populated from `list_tags` after the read pool attaches. Right-click context menu on each tag row (Rename / Delete) with destructive-action confirmation. The Phase 5 placeholder ("Tags · lands in Phase 6") is gone.
- **Tag count badges** in the sidebar (open-task count per tag, hidden when zero — same idiom as projects/areas).
- **`ActiveList::Tag(id)` content pane**: title renders as `#tagname`; empty state copy "{} is empty / No open tasks bear this tag."
- **Actions + accels**: `app.new-tag` triggers `prompt_create_tag`. `Ctrl+Shift+T` accelerator. Hamburger menu's New section gained "New Tag". `win.rename-active` / `win.delete-active` already-installed actions extended their match arms to handle `ActiveList::Tag(_)`.
- **Schema's NOCASE uniqueness** surfaces as a friendly behaviour: creating a tag with the same case-insensitive name as an existing one returns `DbError::Sqlite` (the constraint violation), which the UI maps to a console warning today and a toast in the Phase 8 polish pass.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **91 tests** (up from 87): 20 in `atrium`, 71 in `atrium-core`. **4 new** worker tests cover tag create / rename / delete (with library delta) and the NOCASE-unique constraint rejection.

### Try it

```bash
cargo run -p atrium

# Hamburger menu → New Tag → "errand" → Enter
# Click "errand" in the sidebar to see its tagged tasks (none yet —
# Phase 6b ships the pill editor that attaches tags to tasks).
# Right-click → Rename → "Errands" → F2 also works.
```

### Coming in 6b (v0.0.15)

- **Multi-tag pill editor** on task rows. Pills appear after the title; click opens a popover with autocomplete over existing tags. Worker gains `SetTaskTags(task_id, Vec<i64>)`.
- **Inline `#tag` syntax** in the bottom-of-list entry: typing `Buy milk #errand` creates the task and attaches the tag (creating the tag if needed).

### Coming in 6c (v0.0.16)

- **Quick Entry modal** (`Ctrl+Alt+Space` in-app — true OS-global shortcut deferred to Phase 20 daemon).
- **Inline parser** for `#tag` and `@today` / `@tomorrow` / `@yyyy-mm-dd` / `@deadline yyyy-mm-dd` inside Quick Entry.

### What didn't change

- Schema (Phase 1's `0001_initial.sql`), single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged.
- Every Phase 0–5.5 feature still works.

`VERSION`: 0.0.13 → 0.0.14 (patch — Phase 6a slice).

## v0.0.13 (2026-05-06) — Phase 5.5 polish: right-click context menus + sidebar selection

Two small UX wins. The first stretch items from Phase 5 close out before Phase 6 begins.

### What shipped

- **Right-click context menus** on sidebar rows:
  - Project rows: **Rename** / **Archive** / **Delete**.
  - Area rows: **Rename** / **Delete** (areas don't archive).
  - Implementation: `gtk::GestureClick::set_button(BUTTON_SECONDARY)` per row + a `gtk::PopoverMenu::from_model(&gio::Menu)`. The menu items target the existing `win.rename-active` / `win.archive-active-project` / `win.delete-active` actions; the gesture sets `active_list` to the right-clicked row's project / area before popping the menu, so the actions operate on the right entity.
- **Sidebar selection preserved across `LibraryChanges`**:
  - `apply_library_changes` now remembers the active list before rebuild, calls `select_sidebar_row_for(active)` after, and only falls back to Today when the active entity was actually deleted.
  - `select_sidebar_row_for(active)` walks the freshly-built `sidebar_targets` for the matching `Some(active)` and restores the highlight. No more "selection bounces to top of sidebar after every rename" flicker.

### What's deferred

- **Heading CRUD** is not in this patch. Schema-side it's been there since Phase 1 (`heading` table); display as section breaks within a project page is spec §5.1 territory. We're slipping it to **Phase 10** where the Builder Mode Inspector pane provides the natural editing surface — Simple Mode users don't need a Heading editor at v0.1, and a half-implemented one in Phase 5.5 would be more confusing than useful.
- **Smarter sidebar diff applier** that preserves scroll position (not just selection) — left for Phase 8 polish if perf demands. Current full-rebuild is sub-millisecond on a 100-area / 500-project tree.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 87 tests still green (no new tests added; right-click and selection-preserve are interactive UX, not unit-testable without a display).

### What didn't change

- Schema, single-writer worker, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- All Phase 0–5c features still work.

`VERSION`: 0.0.12 → 0.0.13 (patch — Phase 5.5 polish, no contract change).

## v0.0.12 (2026-05-06) — Phase 5c: count badges + drag-to-project

The sidebar gets numbers, and tasks move between lists by drag. Phase 5 closes here for the Simple Mode hierarchy work — Phase 6 is next, with tags and Quick Entry.

### What shipped

- **Count read functions** in `atrium-core::db::read`:
  - `CanonicalCounts` struct + `count_open_canonical(today)` — six SELECTs in one call, returning open-task counts for Inbox / Today / Upcoming / Anytime / Someday / Logbook.
  - `count_open_per_project()` — `HashMap<i64, i64>` from a single `GROUP BY` query.
  - `count_open_per_area()` — `HashMap<i64, i64>` aggregating across the area's projects via the `task` ↔ `project` join.
- **Sidebar count badges**:
  - Every sidebar row (canonical, area, project) now renders an optional integer badge on the right. Hidden when the count is zero per the Phase 5 design call — visual calm over OmniFocus-style always-visible.
  - Badges use the `numeric` CSS class — tabular figures (set up in Phase 3 typography) keep digits from dancing.
  - `apply_badge_label(label, count)` flips visibility based on count; `refresh_canonical_badges` / `refresh_dynamic_badges` walk the stored label refs (no full sidebar rebuild on every TaskChanges).
  - The window's imp struct gained `canonical_counts` / `project_counts` / `area_counts` (data) and `canonical_badges` / `project_badges` / `area_badges` (widget refs). Three small `RefCell`s on top of Phase 5b's caches.
- **Drag-to-project**: every project sidebar row is now a `GtkDropTarget` accepting `i64` (the task id provided by Phase 4.5's per-row `GtkDragSource`). On drop the window calls `worker.update_task(TaskUpdate::new(task_id).project(Some(project_id)))` and the `TaskChanges{updated}` delta drops the task from the source list and the `LibraryChanges` re-emit isn't needed (no library mutation). The Inbox row is also a drop target — dropping a task there sets `project_id = NULL` to unfile it.
- **Live updates**: `apply_task_changes` and `apply_library_changes` both call `refresh_counts() + refresh_canonical_badges() + refresh_dynamic_badges()` — every mutation that could move a count refreshes the badges. The library bridge already triggered a sidebar rebuild; that path now picks up fresh counts on the new rows automatically.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **87 tests** (up from 83): 20 in `atrium`, 67 in `atrium-core`. **4 new** read tests cover `count_open_canonical` distribution (with the spec-correct expectation that scheduled-but-unfiled tasks count in Inbox AND Today), per-project grouping, and per-area aggregation.

### Try it

```bash
cargo run -p atrium

# Generate fixtures via the --debug menu, then:
# - Watch sidebar badges populate as you toggle complete on tasks.
# - Drag any Inbox task onto a sidebar project — it moves there
#   and the badge ticks up.
# - Drag any project task onto Inbox — it unfiles back to Inbox.
```

### Phase 5 wrap-up

With 5a / 5b / 5c shipped, Phase 5 of the roadmap is complete. The Simple Mode hierarchy is live: Areas + Projects nested in the sidebar with badges, every canonical list reads from real data, area / project / heading-pending CRUD via menu and keyboard, drag-to-project, archive-with-cascade, FK-aware delta emission. Phase 6 (tags + Quick Entry capture modal) is next.

### Coming in Phase 5.5 patch

- Right-click context menus on sidebar rows (Rename / Archive / Delete).
- Heading CRUD + sectioned project pages (skipped from Phase 5 to keep slices tight).
- Smarter sidebar diff applier that preserves selection / scroll across `LibraryChanges`.

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- Every Phase 0–5b feature still works.

`VERSION`: 0.0.11 → 0.0.12 (patch — Phase 5c slice). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.11 (2026-05-06) — Phase 5b: Area / Project CRUD + LibraryChanges

The hierarchy stops being read-only. Areas and projects can be created, renamed, archived, and deleted from the menu and keyboard, with confirmations on destructive operations. The worker grew a parallel `LibraryChanges` channel so the sidebar updates immediately on every mutation, and `TaskUpdate` gained `project_id` so tasks can be moved between projects.

### What shipped

- **New domain types** (`atrium-core::domain`): `NewArea`, `AreaUpdate` (builder); `NewProject` (with `unfiled` / `in_area` constructors), `ProjectUpdate` (builder with `Option<Option<i64>>` for nullable `area_id` and `review_interval_days`). `TaskUpdate` extended with `project_id: Option<Option<i64>>` + `.project(Option<i64>)` builder method, so `update_task` can move a task to a project (or back to Inbox via `Some(None)`).
- **`LibraryChanges`** (`atrium-core::db::changes`): parallel delta type carrying `areas_created` / `areas_updated` / `areas_deleted` / `projects_created` / `projects_updated` / `projects_deleted`. `merge` for coalescing matches `TaskChanges`. Sidebar listens here; the task list keeps listening on `TaskChanges` — separate channels keep subscribers focused.
- **Worker commands** (Phase 5b set): `CreateArea`, `UpdateArea`, `DeleteArea`, `CreateProject`, `UpdateProject`, `ArchiveProject`, `DeleteProject`. Each carries its own `oneshot::Sender` and gets a `WorkerHandle` async method. `spawn_worker` now returns `(WorkerHandle, mpsc::UnboundedReceiver<TaskChanges>, mpsc::UnboundedReceiver<LibraryChanges>)`.
- **Cascade-aware delta emission**:
  - `DeleteArea` reads the area's projects before the SQL fires, then emits `LibraryChanges{areas_deleted, projects_updated}` so the sidebar reflects the FK-driven `area_id = NULL` unfiling.
  - `DeleteProject` reads the project's tasks before deletion, then emits both `LibraryChanges{projects_deleted}` and `TaskChanges{deleted}` so list views drop the cascade-deleted rows.
  - `ArchiveProject` runs `archived_at = now` *and* `completed_at = now` on open tasks inside a single transaction (per design call — Things-3 behaviour), then emits both deltas with the right `status_changed` set.
- **Window plumbing**:
  - `bridge_library_changes` consumes the new receiver via `glib::MainContext::spawn_local` and dispatches to `window.apply_library_changes`.
  - `apply_library_changes` rebuilds the dynamic sidebar from scratch (small enough for v0.1) and falls back to Today if the active list referenced a deleted project / area.
  - `prompt_create_area` / `prompt_create_project` / `prompt_rename_active` / `prompt_delete_active` / `archive_active_project` methods. Each opens an `adw::AlertDialog` (`prompt_for_text` for entry, `prompt_confirm_destructive` for confirms with `ResponseAppearance::Destructive`).
  - New project defaults to the active area when one is selected.
- **Actions + accels** (full keymap reference: `docs/keymap.md`):
  - `app.new-area` — `Ctrl+Shift+A`
  - `app.new-project` — `Ctrl+Shift+N`
  - `win.rename-active` — `F2`
  - `win.delete-active` — `Ctrl+Shift+Delete`
  - `win.archive-active-project` — menu only (destructive enough that we don't bind a default accel)
- **Hamburger menu** gained a Library section ("Rename Active", "Archive Project", "Delete Active") and the New section grew "New Project" / "New Area".
- **`gtk::ShortcutsWindow`** (`Ctrl+?` / `F1`) gained a Library group surfacing the new shortcuts.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **83 tests** (up from 76): 20 in `atrium`, 63 in `atrium-core`. **7 new** worker tests cover area create/rename/delete (with project-unfile cascade), project create, archive (with auto-complete-open-tasks), delete (with cascade-task-delete), and `update_task(project)` for move-to-project.

### Try it

```bash
cargo run -p atrium

# Hamburger menu → New Area → "Personal" → Enter
# Click "Personal" in the sidebar
# Hamburger menu → New Project → "Errands" → Enter
# (creates Errands inside Personal)
# F2 to rename, Ctrl+Shift+Delete to delete
# Hamburger menu → Archive Project to archive
```

### Coming in 5c (v0.0.12)

- Sidebar count badges (open task count per list/project/area, hidden when zero per design call).
- Drag tasks onto sidebar projects to move them.
- Right-click context menus on sidebar rows (rename / delete / move).

### Coming in Phase 5.5 patch

- Heading CRUD + sectioned project pages.
- Smarter sidebar diff applier (preserve scroll/selection across `LibraryChanges`).

### What didn't change

- Schema, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- All Phase 0–5a features still work — every list, sidebar navigation, drag-reorder, bottom-of-list entry, completion toggle, inline edit, the keymap, ShortcutsWindow.

`VERSION`: 0.0.10 → 0.0.11 (patch — Phase 5b slice). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.10 (2026-05-06) — Phase 5a: sidebar hierarchy + remaining lists

The first slice of Phase 5. Sidebar grows beyond the six canonical lists into a real Areas → Projects hierarchy, and the four lists Phase 4 stubbed (Upcoming / Anytime / Someday / Logbook) all render real data now. Every list is one read function in `atrium-core::db::read`, all built on the same `gio::ListStore<AtriumTask>` machinery from Phase 4.

### What shipped (Phase 5a)

- **`atrium-core::db::read` additions** — `list_anytime(today)`, `list_someday`, `list_upcoming(today)`, `list_logbook`, `list_project(id)`, `list_area(id)` (joins `task` with `project` to aggregate across an area's projects), `list_areas`, `list_projects`. Each one a small, indexed query; `list_logbook` orders by `completed_at DESC`. **11 new tests** cover Someday-sentinel exclusion, deferred-task handling, archived-project exclusion, area aggregation across projects, NULL/non-NULL area_id grouping. Total core tests: **56** (up from 45).
- **`ActiveList::Project(i64)` and `ActiveList::Area(i64)`** added to the existing enum. `task_matches` extends to all variants — Inbox / Today / Upcoming / Anytime / Someday / Logbook fully predicate-checked; Project matches by `project_id`; Area returns `false` (lookup needs project→area mapping that isn't on `Task`, so the diff applier falls back to refresh-on-update for that case). The old `implemented_in_phase_4` gate is gone — every variant is implemented.
- **`AtriumWindow` sidebar rewrite** — `build_sidebar` ships canonical rows on construction (Phase 4 behaviour); `rebuild_dynamic_sidebar` runs from `attach_data_layer` and appends Areas + Projects + Unfiled + Tags-placeholder sections from the read pool. Non-selectable header rows separate sections without breaking `GtkListBox` arrow-key navigation. Project rows indent under their area. The window holds `sidebar_targets: Vec<Option<ActiveList>>` aligned with row indices, plus `project_titles` and `area_titles` `HashMap<i64, String>` caches for content-pane title resolution.
- **`refresh_active_list` dispatches all variants** — every list type maps to its `db::read::*` function. The content pane title now flows through `title_for(active)` which consults the caches for Project/Area; the canonical lists return their static label.
- **Empty states for every list** — distinct copy per variant ("Inbox is empty / Press Ctrl+N", "Nothing today", "No anytime tasks", "Logbook is empty / Completed tasks accumulate here, newest first", project-named for `Project(_)` etc).
- **`Ctrl+1..6` still works** as in Phase 4 — limited to the canonical lists. Project / area shortcuts are reserved for Phase 5b's CRUD pass.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **76 tests** (up from 65): 20 in `atrium`, 56 in `atrium-core`.
- **Live launch confirmed**: `cargo run -p atrium` opens the window, sidebar reads areas/projects from the DB, click switches the content pane.

### Try it

```bash
# Generate a fixture DB to see real areas/projects
rm -rf ~/.local/share/atrium/atrium.db*
cargo run -p atrium -- --fixture small  # 5 areas, 50 projects, 1000 tasks

# Open the window — every list works now
cargo run -p atrium

# Click any area in the sidebar to see aggregated tasks across its projects.
# Click any project to see that project's open tasks.
# Click Upcoming/Anytime/Someday/Logbook — all populated.
```

### Coming in 5b (v0.0.11)

- `Command::CreateArea` / `RenameArea` / `DeleteArea` and same for Project, Heading.
- New keyboard shortcuts: `Ctrl+Shift+N` for new project, right-click context menus.
- Hamburger menu items for "New project" / "New area".
- `LibraryChanges` parallel delta type for live sidebar refresh.

### Coming in 5c (v0.0.12)

- Sidebar count badges (open task count, hide when zero per design call).
- Project completion → archive workflow with auto-complete-tasks-with-toast-cancel.
- Drag tasks onto sidebar projects to move them.

### What didn't change

- Schema, single-writer worker, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- All Phase 0–4.5 features still work — Inbox, Today, drag-reorder, bottom-of-list entry, completion toggle, inline edit, the keymap, ShortcutsWindow, the menu, fixture generator, etc.

`VERSION`: 0.0.9 → 0.0.10 (patch — Phase 5a slice). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.9 (2026-05-06) — Phase 4.5 patch: drag-to-reorder + bottom-of-list entry

The two stretch items v0.0.8 explicitly slipped land here. Pure UI work; no schema or contract changes.

### What shipped

- **Bottom-of-list inline-create entry** (`data/window.ui`, `atrium/src/ui/window.rs`): a `GtkEntry` ("Add task…") sits below the `GtkListView`. `Ctrl+N` (and the `+` toolbar button) now focuses this entry instead of immediately spawning a "New task" placeholder. Enter commits → `worker.create_task(NewTask)`; the entry clears so rapid capture stays fluid. This is the Things-3 idiom — type the title, hit Enter.
- **Drag-to-reorder within Inbox** (`atrium/src/ui/task_list.rs`, `window.rs`):
  - `build_factory` gained an `on_reorder` callback parameter. Every row now carries a `gtk::DragSource` (provides the task id as `i64` content) and a `gtk::DropTarget` (accepts `i64`, calls `on_reorder(src_id, dest_id)` on drop).
  - `window::handle_reorder` snapshots the active store's positions, finds source and destination, computes a midpoint (`(dest.pos + neighbour.pos) / 2.0`) so the source lands adjacent to the destination, and fires one `worker.update_task(TaskUpdate::new(id).position(new))`. Inbox-only — the other lists return early since they auto-sort by date.
  - `task_list::sort_by_position` re-sorts the `gio::ListStore` after `apply_changes`, so the reorder becomes visible as soon as the worker's `TaskChanges` delta applies. Same sort runs after every full-list reload.
- **Roadmap Phase 4 boxes updated**: drag-to-reorder and inline-create rows now check off, with the implementation details captured.

### Caveats / known limitations

- **No drag visual feedback yet** — the drop target accepts the drop but there's no highlight on hover or insertion line. Works functionally; polish (cursor change, drop-position indicator, animated row movement) lands with the broader Phase 8 polish pass.
- **Drag-reorder respects active-list semantics**: dragging in Today / Logbook / etc. is silently ignored (those lists auto-sort by date or completion time, not user-driven position).

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — 65 tests still green (unchanged from v0.0.8; the new code path is exercised end-to-end at runtime via the window).
- **Live launch confirmed**: `Ctrl+N` focuses the entry; typing + Enter creates a task; dragging an Inbox row onto another reorders it.

### What didn't change

- Schema, single-writer worker, vault projection, debug-first, dependency discipline, release discipline.
- v0.1 dependency set unchanged. No new crates.
- All Phase 0–4 features still work.

`VERSION`: 0.0.8 → 0.0.9 (patch — Phase 4.5 stretch landings, no contract changes). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.8 (2026-05-06) — Phase 4: Simple Mode — Inbox & Today + Calendar Month View on roadmap

The first phase Atrium becomes *usable*. `cargo run -p atrium` opens the window, real tasks render in the sidebar's Inbox / Today views, completion toggles, inline title edits, and `Ctrl+N` task creation all flow through the single-writer worker and reach the UI via the `TaskChanges` bridge that landed in Phase 3.

Plus a roadmap addition: **Phase 12.5 — Calendar Month View (Builder)** — the traditional month-grid view that complements Forecast's day-block layout.

### What shipped (Phase 4)

- **`db::read::list_today(today)`** in `atrium-core` per spec §4.2 — open tasks scheduled-or-deadline ≤ today, not deferred, Someday sentinel explicitly excluded (the lexical-sort bug the comment in `read.rs` calls out). 8 new tests cover scheduled / overdue / Someday / completed / deferred / deadline-only edge cases.
- **`AtriumTask`** GObject (`atrium/src/ui/task_object.rs`): `id` / `uuid` / `title` / `note` / `completed` / `schedule_label` / `deadline_label` / `position` exposed as `glib::Properties` for bidirectional widget binding. `from_task` / `refresh_from` keep it in sync with `atrium_core::Task`.
- **`task_list` module** (`atrium/src/ui/task_list.rs`): `ActiveList` enum (Inbox / Today / Upcoming / Anytime / Someday / Logbook) with `task_matches(task, today)` predicate mirroring the spec's filter rules. `build_factory(on_toggle, on_rename)` produces a `gtk::SignalListItemFactory` that builds rows imperatively (checkbox + `GtkEditableLabel` title + schedule pill + deadline pill). `replace_store` for full reloads on list switch; `apply_changes` for in-place TaskChanges diff (created / updated / deleted / status_changed handled per active-list membership).
- **Window subclass rewrite** (`atrium/src/ui/window.rs`): sidebar built programmatically with click + selection-changed handlers; `AdwNavigationSplitView` content pane hosts a `gtk::Stack` between an `AdwStatusPage` empty state and the `gtk::ListView`. `attach_data_layer(worker, pool)` plugs in after `boot_data_layer` succeeds; `apply_task_changes` runs the diff applier on the active store.
- **TaskChanges bridge wired to the window**: `glib::MainContext::default().spawn_local` consumes the worker's `mpsc::UnboundedReceiver<TaskChanges>` and calls `window.apply_task_changes` on the GTK thread. Window weak-ref keeps the bridge alive only as long as the window exists.
- **CRUD plumbing**: row toggle → `worker.toggle_complete`; inline title edit → `worker.update_task(TaskUpdate::title)`; `Ctrl+N` → `worker.create_task(NewTask)`; `Delete` → `worker.delete_task` on focused row; `Space` → `worker.toggle_complete` on focused row. All async, dispatched through `spawn_local` on the GTK thread.
- **Comprehensive keymap** centralised in `main.rs::install_accels`: `Ctrl+N` (new), `Ctrl+1..6` (jump to lists), `Ctrl+Q` (quit), `Ctrl+?` / `F1` (shortcuts dialog), `Space` / `Delete` (focused-row actions). Stub bindings reserved for `Ctrl+Z` / `Ctrl+F` / `Ctrl+,` (undo / search / preferences — wired in Phase 7+).
- **`gtk::ShortcutsWindow`** (`atrium/src/ui/shortcuts.rs`) loaded from inline XML; opens via `Ctrl+?` / `F1` / hamburger menu. Three sections: General / Navigation / List actions.
- **`docs/keymap.md`** — written reference for the keymap, Builder Mode growth sketched (`Ctrl+I`, `Ctrl+Shift+F`, `Ctrl+Shift+M`, etc.), discovery rules, and the four-edit checklist for adding a shortcut.
- **Empty states**: per-list `AdwStatusPage` swapped via `gtk::Stack` — "Inbox is empty / Press Ctrl+N", "Nothing today", placeholder for Phase 5+ lists.

### What shipped (roadmap addition)

- **Phase 12.5 — Calendar Month View (Builder)** added to `roadmap.md` between Forecast (Phase 12) and Review (Phase 13). 8-bullet item: month-grid widget with task-count badges, drag-to-reschedule between days, click-day-to-filter, today indicator, month nav with `Ctrl+Shift+M`, narrow-window collapse to week strip, Builder-only sidebar entry, tests for date-filter correctness across month boundaries / DST / leap day. Doesn't disturb the v0.2 phase numbering — sub-phase under Phase 12's calendar-view domain.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **65 tests** (up from 49): 20 in `atrium` (CLI parsing × 6, debug pane × 1, typography × 3, window menu × 3, ScheduledFor × 0 [moved], task_object × 3, task_list × 4) + 45 in `atrium-core` (37 prior + 8 new for `list_today`).
- **Live launch confirmed**: `cargo run -p atrium` opens the window with the sidebar populated, Today selected, and (when fixture data exists in the DB) tasks rendering in the content pane.

### Deferred to Phase 4.5 patch

The original Phase 4 plan included two stretch items that didn't make it into v0.0.8 but are explicitly slipped (not dropped):

- **Drag-to-reorder within Inbox.** `update_task` already accepts a `position` field; the remaining work is binding `GtkDragSource` + `GtkDropTarget` on rows and computing midpoint positions. Lands in v0.0.9 (Phase 4.5 patch — pure UI work, no schema or contract impact).
- **Bottom-of-list inline-create entry.** Today, `Ctrl+N` creates a task titled "New task" that the user immediately renames via the existing inline editor — functional but not the Things-3 idiom. The dedicated entry widget that focuses on `+` lands in 4.5.

### What didn't change

- Phase 0–3 surfaces unchanged. `--debug`, `--fixture <scale>`, `--version`, `--help` all still work.
- v0.1 dependency set: `chrono` enabled in `atrium`'s `[dependencies]` (already locked in workspace deps from Phase 0). No new crates introduced.
- Schema (`0001_initial.sql`) unchanged.
- Mode-as-view, single-writer worker, vault projection, debug-first, dependency discipline, release discipline.

`VERSION`: 0.0.7 → 0.0.8 (patch — Phase 4 ship + roadmap addition). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.7 (2026-05-06) — Phase 3: Application Shell

The first phase Atrium becomes lookable. `cargo run -p atrium` opens a real `AdwApplicationWindow`, the bundled type system installs on first run, the worker plugs into a tokio runtime that coexists with glib's main loop, and `TaskChanges` deltas reach the UI thread via the canonical `spawn_local` bridge.

### What shipped

- **GTK + libadwaita application** (`atrium/src/main.rs` rewrite): `adw::Application` with `io.github.virinvictus.atrium` ID. Tokio multi-thread runtime built once via `OnceLock<Runtime>`, lives until exit. `connect_startup` installs fonts and CSS; `connect_activate` opens the DB, spawns the worker, bridges `TaskChanges` to the GTK main loop, and presents the window.
- **`AtriumWindow`** (`atrium/src/ui/window.rs` + `data/window.ui`): `AdwApplicationWindow` subclass via `gtk::CompositeTemplate`. `AdwToolbarView` + `AdwHeaderBar` + hamburger menu over `AdwNavigationSplitView`. Sidebar lists the six canonical Simple-Mode rows (Inbox / Today / Upcoming / Anytime / Someday / Logbook); content pane is a placeholder `AdwStatusPage` that Phase 4 replaces with real list views.
- **About dialog** (`atrium/src/ui/about.rs`): `adw::AboutDialog` with version, MIT, repo + issue URLs, designer/developer credits, an acknowledgement section (Things 3, OmniFocus, Org-mode, NetNewsWire), and a bundled-fonts legal section.
- **GSettings schema** (`data/io.github.virinvictus.atrium.gschema.xml`): `mode` enum (Simple/Builder), `window-width`/`window-height`/`window-maximized`, `sidebar-width`, `quick-entry-shortcut` (declared, bound in Phase 6).
- **`atrium/build.rs`**: runs `glib-compile-schemas` against `data/`, exports `ATRIUM_GSCHEMA_DIR` and `ATRIUM_DATADIR` via `cargo:rustc-env` so `cargo run` finds the compiled schema and the data tree without needing `meson install`.
- **Mode action**: stateful `gio::SimpleAction` `app.mode` (parameter `s`) writes to GSettings; state mirrors back. Builder Mode is wired but currently identical to Simple Mode visually — Inspector / Forecast / Review are Phase 10+.
- **Quit action** with `<Primary>q` accel.
- **Window state persistence**: width / height / maximized read from GSettings on construction, written on `close-request`. Verified: resize, close, reopen → same size.
- **Light/dark follow-system** via libadwaita's default style manager.
- **Typography foundation** (spec / roadmap landing): Inter Variable + Italic (UI), Source Serif 4 Variable Roman + Italic (note bodies), JetBrains Mono Variable + Italic (debug pane / monospace) — all SIL OFL 1.1, bundled at `data/fonts/` (~4 MB total). Installed to `$XDG_DATA_HOME/fonts/atrium/` on first run with `fc-cache` refresh (proven Viaduct pattern, idempotent). `data/style.css` loaded via `gtk::CssProvider`; tabular figures default-on for `.numeric` selectors. Fallback to system fonts if the bundled files are missing — non-fatal.
- **`--debug` integration**: when set, the hamburger menu gains a Debug section with a fixture-generator submenu (Small / Medium / Large / Stress). Activations route through `tokio::task::spawn_blocking` so the GTK thread isn't blocked.
- **`TaskChanges` UI bridge**: `glib::MainContext::default().spawn_local` consumes `mpsc::UnboundedReceiver<TaskChanges>` directly. tokio's mpsc receiver futures use runtime-agnostic wakers, so glib's executor drives them without `tokio-stream`, `async-channel`, or any other extra crate.
- **Worker handle stash**: spawned `WorkerHandle` lives on a thread-local on the GTK main thread (`thread_local!` `RefCell<Option<WorkerHandle>>`). Phase 4+ pulls it via accessor when the UI starts sending commands.
- **Meson updates**: installs the gschema XML to `$datadir/glib-2.0/schemas/` (with post-install `glib-compile-schemas`), `data/fonts/` to `$datadir/atrium/fonts/`, `data/style.css` to `$datadir/atrium/`. `ATRIUM_DATADIR` exported into the cargo build environment so the runtime resolver lands on the install path.

### Defaults captured (from Phase 3 plan)

- **tokio + glib coexistence**: glib owns the main thread, tokio runs in a separate multi-thread runtime — the canonical GTK4-rs pattern Viaduct uses.
- **CompositeTemplate `.ui` files** for window structure (sidebar list rows declarative); menu built imperatively in code so the `--debug` section can be conditional.
- **Fonts via fontconfig** (Viaduct's pattern) instead of in-process `pango::FontMap::add_font_file`. Simpler and proven.
- **`build.rs` for GSettings compile** so `cargo run` works without manual install.
- **`adw::AboutDialog`** with explicit acknowledgements section (portfolio-piece detail).

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **49 tests** (up from 37): 12 in `atrium` binary (CLI parsing × 6, debug pane × 1, typography × 3, window menu × 2) + 37 in `atrium-core` (unchanged from v0.0.6).
- **Live launch test**: `cargo run -p atrium` opens the window cleanly. Fonts install to `~/.local/share/fonts/atrium/`, `fc-cache` succeeds, stylesheet applies, DB opens at `~/.local/share/atrium/atrium.db`, worker starts. `--debug` adds the debug pane stub. `--fixture small` still bypasses GTK and exits with the summary.

### What's deferred

- **`heaptrack` baseline** (roadmap Phase 3 closing item): heaptrack isn't installed on the development machine right now. Will land as a `docs/perf.md` entry once Brandon installs it (`sudo dnf install heaptrack`); this is purely measurement, no code impact. Phase 3's idle binary opens a window with no task data — well below the §8 80 MB target by inspection, but the empirical number is missing.

### What didn't change

- Phase numbering, mode-as-view, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- Schema (`0001_initial.sql` from Phase 1) is unchanged.
- v0.1 dependency set: `gtk` / `adw` / `tokio` enabled in `atrium`'s `[dependencies]` (already locked in workspace deps from Phase 0). No new crates introduced.

`VERSION`: 0.0.6 → 0.0.7 (patch — Phase 3 ship). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.6 (2026-05-05) — Phase 2: Data Layer (Single-Writer Worker)

The architectural-commitment-2 pattern lands. Domain types, single-writer worker, read-only pool, IO instrumentation, `TaskChanges` deltas. UI doesn't exist yet but the headless data layer it'll plug into is real and tested.

### What shipped

- **Domain types** (`atrium-core::domain`): `Task` (full row), `NewTask` (insert input with `inbox()` helper), `TaskUpdate` (builder-style partial update), `Project`, `Area`, `Tag`, `Heading`. All `serde`-derived. `ScheduledFor` enum (`Someday | Date(NaiveDate)`) with custom `rusqlite::ToSql` / `FromSql` impls so the schema's "ISO date OR `__someday__` sentinel" is type-safe in Rust — `parse()` / `Display` round-trip cleanly.
- **`TaskChanges`** (`atrium-core::db::changes`): `{ created, updated, deleted, status_changed }` per spec §3.2. `merge()` folds deltas for the coalescer.
- **`Command`** enum (`atrium-core::db::command`): Phase 2 set is `CreateTask`, `UpdateTask`, `ToggleComplete`, `DeleteTask`. Each variant carries its own `oneshot::Sender` for the per-call result. Project / area / tag / heading commands follow naturally in Phase 5 with their UI.
- **Single-writer worker** (`atrium-core::db::worker`): a dedicated `tokio` task owns the writable connection. `WorkerHandle` is `Clone`; the worker shuts down when the last handle drops. Spawn returns `(WorkerHandle, mpsc::UnboundedReceiver<TaskChanges>)`. Position auto-computed as `MAX(position) + 1` per sibling list (parent → children, project → tasks, inbox).
- **Read-only connection pool** (`atrium-core::db::read_pool`): `Mutex<Vec<Connection>>` with lazy on-demand connection creation. `PRAGMA query_only = ON` per connection — SQLite enforces read-only at the engine level. `with(|conn| ...)` API. Soft cap on idle connections; excess dropped on release.
- **Read functions** (`atrium-core::db::read`): `task_by_id`, `list_inbox`, `list_all_tasks`, `count_tasks` — free functions taking `&Connection` so they compose with both worker and pool connections.
- **IO instrumentation** (spec §3.4): rusqlite `Connection::profile` callback routes every SQL statement (text + elapsed micros) through `tracing` at TRACE level. `RUST_LOG=trace` (or scoped `atrium_core::db=trace`) reveals each statement. Required adding rusqlite's `trace` feature — feature flip on an existing locked dep, no new crate.
- **`DbError::WorkerClosed`** for "command sent but channel closed" / "responder dropped"; **`DbError::NotFound`** for "no row matched."
- **`atrium-core` lib exports:** `TaskChanges`, `WorkerHandle`, `spawn_worker`, all domain types, all errors flow through the crate root.

### Defaults captured (from Phase 2 plan)

- **`ScheduledFor` as enum, not string** — schema's "TEXT (ISO date OR sentinel)" maps to a sum type in Rust. Type-safe at the boundary; round-trips through rusqlite via custom `ToSql`/`FromSql`.
- **Worker channels:** bounded mpsc (capacity 64) for commands so backpressure surfaces in `WorkerHandle::*` awaits; unbounded mpsc for `TaskChanges` so a slow UI subscriber never stalls writes.
- **Per-variant `oneshot::Sender`:** boilerplate-y `WorkerHandle` methods but each operation's response type is statically checked. No `CommandResult` super-enum.
- **Read pool: lazy on-demand**, soft cap on idle, no hard cap on total opens. Pragmatic for v0.1 single-user concurrency; bounded variant can land later if perf demands.
- **IO instrumentation always-on, gated by `RUST_LOG`.** Trace level so default INFO logging stays clean. Phase 3 will surface the stream visually in the debug pane.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **37 tests** (up from 27): 30 in `atrium-core` (paths × 3, db schema × 12, fixtures × 4, ScheduledFor × 4, ReadPool × 3, TaskChanges × 2, Worker × 8) + 7 in `atrium`. Worker tests cover create/update/toggle/delete round-trips, NotFound on missing id, position auto-increment, Someday round-trip, clean worker shutdown on handle drop.

### What didn't change

- Phase 0 binary surface: `--debug` and `--fixture <scale>` work exactly as before. The Phase 2 worker is library-only; Phase 3 will wire it into the GTK + tokio main loop.
- v0.1 dependency set: tokio enabled in `atrium-core`'s `[dependencies]` (already locked in workspace deps from Phase 0). `rusqlite` got the `trace` feature added — no new crate, feature flip only.
- Schema (Phase 1's `0001_initial.sql`) is the contract; no changes there per "no mid-v0.1 schema changes."

### Open / deferred

- **`glib::MainContext::channel` bridge** (roadmap Phase 2 item): explicitly slipped to Phase 3 since it requires GTK on the binary side. Phase 2 ships `mpsc::UnboundedReceiver<TaskChanges>`; Phase 3 spawns the bridging glue.

`VERSION`: 0.0.5 → 0.0.6 (patch — Phase 2 ship). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.5 (2026-05-05) — Roadmap addition: Beyond 1.0

Roadmap horizon extended past Phase 20.

### What changed

- **`roadmap.md` — new "Beyond 1.0" section** after Phase 20. Captures **Toward 2.0 — Full TUI mode (`atrium-tui`)** as the first post-1.0 horizon item: keyboard-first terminal frontend over the same headless core, three-pane layout, Simple / Builder mode reused, FTS5 search via `/`, dependency check on a TUI crate (likely `ratatui`) to land before adoption. Not committed to a phase number yet.
- The workspace split done in Phase 0 (`atrium-core` headless + `atrium` GTK binary) is the load-bearing decision that makes this cheap — `atriumd` (Phase 20) is already a second consumer; a TUI would be the third.
- Items still explicitly out of scope per spec §9 (network sync, mobile/web, multi-user, time tracking, calendar event creation, AI) remain out of scope and are *not* on the horizon either.

`VERSION`: 0.0.4 → 0.0.5 (patch — roadmap refinement, no code change).

## v0.0.4 (2026-05-05) — Phase 1: Schema Design

The OmniFocus superset lives in SQL. Migration `0001_initial.sql` ships once and stays — backwards-compatible migrations begin at v0.2 per CLAUDE.md commitment.

### What shipped

- **`atrium-core/src/db/migrations/0001_initial.sql`** — full schema per spec §4: `area`, `project`, `heading`, `task`, `tag`, `task_tag`. Every Builder-only column (`defer_until`, `estimated_minutes`, `sequential`, `review_interval_days`, `last_reviewed_at`, `repeat_rule`, `parent_id`) exists from day one. `task_fts` virtual table (FTS5, content='task', tokenize='unicode61') with insert/update/delete sync triggers. `modified_at` triggers on all five timestamped tables, with `WHEN old = new` clauses that prevent recursion *and* let explicit writes survive (import-time timestamp preservation).
- **`atrium-core::db::open(path)`** — opens (or creates) the database, ensures `$XDG_DATA_HOME/atrium/` exists, applies pragmas (`WAL`, `synchronous=NORMAL`, `temp_store=MEMORY`, `mmap_size=256 MB`, `foreign_keys=ON`), runs pending migrations.
- **`atrium-core::db::migrations::migrate`** — `PRAGMA user_version`-driven runner. Each migration runs inside a transaction; failed migrations roll back without leaving the schema half-applied. Idempotent.
- **`atrium-core::db::fixtures`** — stress generator at four scales (`Small` 1K, `Medium` 10K, `Large` 50K, `Stress` 100K). Realistic distribution (~20 tasks per project, ~14 % inbox-only, mix of scheduled / completed / Someday, ~30 % tagged, unicode-hostile titles). Wired into `--fixture <scale>` CLI flag.
- **CLI surface expanded:** `--fixture small|medium|large|stress` triggers fixture generation against `$XDG_DATA_HOME/atrium/atrium.db`; default behaviour now opens the DB and runs migrations on every invocation.
- **`DbError` fleshed out:** `Sqlite(rusqlite::Error)` via `From`, `Migration { version, source }` for nicer reporting.
- **`docs/schema.md`** — Mermaid ER diagram, per-table/column rationale, cross-referenced to spec §4 (contract) and `0001_initial.sql` (canonical SQL).

### Design calls captured here

- **`uuid` crate added** (sign-off granted in Phase 1 plan). Pure-SQL UUID v4 generation was the alternative; rejected on ergonomics — UUIDs would be opaque to Rust code without a roundtrip.
- **Hard-delete only** — no `deleted_at` columns. Logbook holds completed tasks; deleted tasks are gone forever. Soft-delete can land in v0.2 if it earns its keep.
- **`unicode61` tokenizer** for FTS5 — predictability beats fuzzy matching for short task titles. English stemming (`porter unicode61`) considered for v0.2 as an option.
- **Basic stress generator** now (vs skeleton + flesh-out later). Realistic-shape generator runs at 1K-100K scales without being elaborate.
- **Datetimes stay TEXT (ISO 8601)** — INTEGER unix can't represent the `'__someday__'` sentinel for `scheduled_for` (spec §4.2), conflates date vs datetime granularity, and forces conversion on every Org / VTODO interop call.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ — **20 / 20** (up from 7): 16 in `atrium-core` (paths × 3, db schema/triggers/FK/cascade × 9, fixtures × 4) + 7 in `atrium` (CLI parsing × 6, debug pane × 1). Includes `migration_is_idempotent`, `explicit_modified_at_survives_trigger`, `tag_name_is_case_insensitive_unique`, `project_cascade_deletes_tasks`, `area_set_null_on_delete`, FTS sync on insert/update/delete.
- **Perf smoke** (release build, T14s AMD Gen 6): `--fixture small` (1K) → 59 ms; `--fixture medium` (10K) → 203 ms. The 10K-task DB exists in well under the 250 ms cold-start budget for Phase 8's eventual application shell.

### What didn't change

- Phase numbering, mode-as-view, single-writer worker pattern, vault projection, debug-first, dependency discipline, release discipline.
- Phase 2 (single-writer worker) is the next phase. v0.1 dependency set still locked; tokio enters atrium-core's `[dependencies]` with the worker.

`VERSION`: 0.0.3 → 0.0.4 (patch — Phase 1 ship). `Cargo.toml` workspace + `meson.build` synchronized.

## v0.0.3 (2026-05-05) — Phase 0: Scaffolding

First code lands. Cargo workspace, module skeleton, error hierarchy, tracing, `--debug` flag, Meson wrapper, GitHub Actions CI. Binary builds clean and runs; no UI surface yet.

### What shipped

- **Cargo workspace** at the repo root: `atrium/` (binary) and `atrium-core/` (headless library). Workspace `Cargo.toml` locks the v0.1 dependency set per spec — `tokio`, `rusqlite` (`bundled`, `chrono` features), `serde`, `serde_json`, `chrono`, `anyhow`, `thiserror`, `tracing`, `tracing-subscriber`, `gtk4` (`v4_16`), `libadwaita` (`v1_7`). Each crate's `[dependencies]` lists only what its phase actually uses; later phases pull more from the workspace as they need them.
- **Module skeleton** with `SPDX-License-Identifier: MIT` on every Rust file:
  - `atrium-core/src/{lib,error,paths}.rs` + `db/`, `domain/` placeholders.
  - `atrium/src/{main,error}.rs` + `ui/`, `quickentry/`, `debug/` placeholders.
- **XDG paths** (`atrium-core::paths`): stdlib-only — no `directories` / `xdg` crate. Honours `XDG_DATA_HOME` / `XDG_CACHE_HOME`, falls back to `$HOME/.local/share` / `$HOME/.cache`. Exposes `data_dir()`, `cache_dir()`, `db_path()`, and the `APP_ID` const (`io.github.virinvictus.atrium`).
- **Error hierarchy** (`thiserror`): `DbError`, `DomainError`, `CoreError` in core; `UiError`, `AtriumError` in the binary. Phase 0 ships the scaffolding; concrete variants land with the data layer (Phase 1+) and the application shell (Phase 3).
- **`--debug` flag plumbing:** stdlib argv parser, `Config` struct, `debug::Pane` stub gated on the flag. The pane logs that it's active in Phase 0; the actual widget mounts in Phase 3 with the application shell.
- **`tracing-subscriber`** initialised with `EnvFilter` (default `info,atrium=debug,atrium_core=debug`), compact format, target on. Honours `RUST_LOG` overrides.
- **CLI surface:** `--debug`, `--version` / `-V`, `--help` / `-h`. Unknown args ignored (no `clap` until we need it).
- **Meson wrapper** (`meson.build`): mirrors Viaduct's pattern — thin `cargo build --release` orchestration, installs binary to `$bindir`. GSettings / desktop entry / AppStream metainfo / icons grow in with Phases 3 and 8. Verified via `meson setup builddir && meson compile -C builddir` against the local toolchain.
- **GitHub Actions CI** (`.github/workflows/ci.yml`): Ubuntu 24.04, apt installs `libgtk-4-dev` / `libadwaita-1-dev` / `libsqlite3-dev` / `pkg-config`, runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- **`.gitignore`:** standard Rust + Meson + editor patterns.

### Verification

- `cargo build --workspace` ✓
- `cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo fmt --all --check` ✓
- `cargo test --workspace` ✓ (7 tests: 3 in `atrium-core` covering `paths`, 4 in `atrium` covering `Config` parsing + `debug::Pane`)
- `cargo run -p atrium -- --debug` ✓ — logs version, debug_mode=true, app_id, pane init, exit
- `cargo run -p atrium -- --version` ✓ — prints `atrium 0.0.3`
- `meson setup builddir && meson compile -C builddir` ✓ — produces a 1.6 MB release ELF that runs cleanly

### Decisions captured here

- **Workspace over single-crate** (roadmap originally specced single-crate `src/{db,domain,ui,quickentry,debug,main.rs}`): workspace mirrors Viaduct's discipline and pre-empts the Phase 20 `atriumd` daemon split. Roadmap module-layout item updated to reflect.
- **Stdlib XDG / argv:** no `directories` / `clap` crate added. Phase 0 needs are small enough that hand-rolled is honest, and it keeps the locked dependency set true to spec. `clap` revisited if/when the CLI grows beyond a handful of flags.
- **Per-phase patch bumps** per the new release discipline: Phase 0 ships as v0.0.3. Phase 1 → v0.0.4, Phase 2 → v0.0.5, ..., Phase 9 → v0.1.0.

### Skipped intentionally

- **Heaptrack baseline:** Phase 0 binary does no allocation worth measuring; the §8 perf budget targets an active app on a 10K-task DB. First meaningful heaptrack lands at the end of Phase 3 when there's a GTK window to measure.

`VERSION`: 0.0.2 → 0.0.3 (patch — Phase 0 ship).

## v0.0.2 (2026-05-05) — Org vault projection + typography foundation

Pre-implementation. Two contract refinements: Org-mode integration grew into a first-class two-way mirror, and the typography foundation moved earlier so later UI phases develop into it.

### What changed

- **Org vault as projection** (`spec.md` §3.5, `CLAUDE.md` commitment #5): an optional two-way Org-mode mirror — SQLite stays canonical, a `.org` directory tree at `~/Tasks/` (configurable) reflects task state and accepts edits back from Emacs / Doom / vim-orgmode / any Org tool. Atrium runs cleanly with no vault configured; vault is downstream of the DB. The §7.3 mapping expanded to a full round-trip contract: vault layout (`<vault>/<Area>/<Project>.org`, `inbox.org` at root, `.atrium/config.toml` sidecar), every Atrium field's Org home, and six round-trip rules covering data preservation, `:ID:` anchoring, best-effort RRULE rendering, sidecar policy, conflict surfacing (no silent loss), and atomic file writes.
- **Roadmap split (Option B):** Phase 17 reworked into "Org-Mode Import & Read-Only Sync (DB → Vault)" — Atrium writes a clean vault any Org tool reads, plus one-shot import from existing Org libraries. Phase 17.5 added: "Two-Way Org Sync (Vault → DB)" — `inotify` watcher, `:ID:` allocation on read, conflict detection with `<file>.atrium.bak.<timestamp>` fallback, malformed-file recovery, RRULE divergence detection.
- **Typography foundation moved to Phase 3:** bundled font set lands with the Application Shell so Phases 4–7 develop into the type system instead of being re-skinned at Phase 8. Set: **Inter Variable** (UI), **Source Serif 4 Variable** (note bodies), **JetBrains Mono** (debug pane / monospace) — all SIL OFL or Apache 2.0, registered via `pango::FontMap::add_font_file`. Tabular figures (`tnum`) default-on for numeric contexts.
- **Typography polish (Phase 8) expanded** from one bullet to: Inter OpenType feature opt-ins (`cv11`, `ss01`), tabular-figures audit across every numeric column, optional Atkinson Hyperlegible accessibility toggle (SIL OFL, ~80 KB), surface-by-surface size/weight/leading pass, Flatpak font-load verification.
- **`CLAUDE.md` commitment #3 clarified:** "Local-first, no sync" → "Local-first, no *network* sync". Local file mirroring (the Org vault) is fine; CalDAV/cloud is still out.
- **`CLAUDE.md` commitment #5 added:** vault projection rule formalised — DB canonical, vault projected, don't pivot to vault-as-storage (perf budget would not survive at 10K-task scale).
- **`README.md` architecture paragraph** updated with the vault and `--debug` mentions; trimmed for length.

### What didn't change

- Single-writer SQLite worker (commitment #2) is unchanged. Org vault is downstream of the DB, not parallel to it.
- 20-phase brand intact; 17.5 is a sub-phase under Phase 17's Org-sync domain, not a renumbering.
- Mode-as-view, debug-first architecture, dependency discipline, and release discipline are unchanged.
- v0.1 dependency set unchanged. Phase 17 still flags `orgize` as a sign-off check before adoption.

`VERSION`: 0.0.1 → 0.0.2 (patch — contract refinement, no feature shipped).

## v0.0.1 (2026-05-05) — Contract refinement

Pre-implementation still. The contract gained a fourth architectural commitment and a written release discipline; no code shipped.

### What changed

- **Debug-first architecture** (`spec.md` §3.4, `CLAUDE.md` commitment #4): a `--debug` CLI flag opens an in-app debug surface for stress generators, pre-canned edge-case fixtures, SQLite/IO instrumentation through `tracing` spans, and live RSS/heap watch. Built into the binary, not bolted on. Skeleton in Phase 0; harness grows phase by phase.
- **Roadmap aligned:** Phase 0 lands the `--debug` skeleton and `debug::Pane` shell; module layout adds `src/debug/`. Phase 1 grows the stress fixture generator (10K / 50K / 100K presets + edge-case states). Phase 2 wires the IO instrumentation onto the single-writer worker. Phase 8 surfaces the live memory watch. North Star calls out the rhythm.
- **Release discipline** (`CLAUDE.md`): every minor or major change updates `spec.md`, `roadmap.md`, `patchnotes.md`, and `VERSION` together — if you can't write the patchnotes line, the change isn't done. Patch releases still bump `VERSION` and `patchnotes.md`. **Every major bump includes a maintenance pass** (refactor, deferred bugfixes, dead-code prune), called out in `patchnotes.md`.
- **Logical version bumps:** patch for fixes-only, minor for additive features that don't break the spec, major for spec-changing or breaking work. The bump rides with the change that earns it.

### What didn't change

- 20-phase sequence is intact: v0.1 (Simple Mode) ends at Phase 9, v0.2 (Builder Mode) at Phase 15, v1.0 at Phase 20.
- Dependency set is intact. The debug harness rides on `tracing` / `tracing-subscriber`, both already in Phase 0's locked set.
- Mode-as-view, single-writer SQLite worker, and local-first commitments are unchanged.

`VERSION`: 0.0.0 → 0.0.1 (patch — contract refinement, no feature shipped).

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
