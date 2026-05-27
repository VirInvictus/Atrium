// SPDX-License-Identifier: MIT
//! stdlib argv parser for atrium-cli. No clap — keeps the dep graph
//! tight (nothing in atrium-cli's tree pulls in proc-macros for arg
//! handling) and matches the project's existing style (atrium's
//! `--debug` flag is also stdlib-parsed).

use std::path::PathBuf;

pub const USAGE: &str = "\
atrium-cli — headless access to the Atrium database and search engine

USAGE:
    atrium-cli [GLOBAL FLAGS] <SUBCOMMAND> [ARGS]

GLOBAL FLAGS:
    --db PATH         override the database path
                      (default: $XDG_DATA_HOME/atrium/atrium.db,
                       or $ATRIUM_DB_PATH if set)
    --json            output as JSON
    --tsv             output as TSV (default; columns: id, status,
                      title, scheduled, deadline, project, area, tags)
    --human           output as pretty-printed columns
    -h, --help        print this message and exit
    -V, --version     print version and exit

READ SUBCOMMANDS:
    search EXPR       run a search expression (spec.md §4.3) and
                      print matching tasks. Multiple words become
                      a single expression (no need to quote unless
                      the shell would split them).
    list NAME         print a canonical list. NAME ∈ task lists
                      (inbox, today, upcoming, anytime, someday,
                      logbook, all) or metadata lists (areas,
                      projects, tags, perspectives).
    info ID           print full details of a single task

WRITE SUBCOMMANDS:
    add TITLE [FLAGS]
                      create a new task. Flags:
                        --note TEXT
                        --project NAME      attach to a project (by
                                            unique title prefix)
                        --parent ID         nest under task ID as a
                                            subtask (inherits the
                                            parent's project)
                        --tag NAME          attach a tag (repeatable;
                                            tag is created if missing)
                        --scheduled DATE    YYYY-MM-DD, today,
                                            tomorrow, or `someday`
                        --due DATE          YYYY-MM-DD, today, tomorrow
                        --defer DATE        YYYY-MM-DD, today, tomorrow
                        --estimated MINUTES integer minutes
    capture LINE      Quick-Entry-style one-shot capture. Parses the
                      line for inline `#tag` / `@today` / `@tomorrow`
                      / `@someday` / `@yyyy-mm-dd` / `@deadline ...`
                      syntax exactly like the GUI's bottom-of-list
                      entry and Quick Entry modal. Drops to Inbox.
    edit ID [FLAGS]   modify an existing task. Flags accept the same
                      vocabulary as `add`; pass `none` to clear a
                      field (`--due none`, `--scheduled none`, etc.).
                      Use `--inbox` to move a task back to Inbox.
                      `--parent ID` reparents as a subtask; `--parent
                      none` promotes back to top level.
                      Tag flags are additive — `--tag X` ensures the
                      tag is attached, `--remove-tag X` (alias
                      `--untag`) removes it, `--clear-tags` empties
                      the set. Compose freely:
                      `--clear-tags --tag work` replaces the set.
                      Field semantics are diff-only — only the flags
                      you pass change.
    complete ID       toggle a task's completion (same as the GTK
                      checkbox; calling twice un-completes).
    complete --where EXPR
                      toggle completion for every task matching the
                      search expression. Prints each affected row.
    delete ID         delete a task. Prints the row before deletion
                      so the deletion is auditable in pipelines.
    delete --where EXPR --force
                      delete every task matching the search
                      expression. Requires --force to actually
                      delete; without it, prints the would-be-
                      deleted rows and exits with status 2 so a
                      script can review-then-confirm.
    kanban NAME       render a saved Perspective as a kanban board
                      (Slice D, v0.5.4). NAME is matched case-
                      insensitively against perspective.name; the
                      perspective's renderer must be 'board'.
    perspective <SUB> NAME [FLAGS]
                      saved-perspective write side. SUB is one of:
                        create     --filter EXPR [--icon NAME]
                                   [--renderer list|board] [--columns 'a,b,c']
                        edit       [--rename NEW] [--filter EXPR]
                                   [--icon NAME|none]
                                   [--renderer list|board] [--columns 'a,b,c']
                        delete     (case-insensitive exact-name match
                                   for safety; substring is read-only).
                      The columns flag is comma-separated tag names.

EXAMPLES:
    atrium-cli list today
    atrium-cli search 'tag:work AND is:overdue sort:-due'
    atrium-cli --json search 'is:repeating' | jq '.[] | .title'
    atrium-cli info 42 --human
    atrium-cli add 'Buy milk' --tag errand --due tomorrow
    atrium-cli add 'Q3 retrospective notes' --project 'Q3 plans' --scheduled today
    atrium-cli capture 'Buy milk #errand @today'
    atrium-cli capture 'File taxes #urgent @deadline 2026-04-15'
    atrium-cli edit 42 --due tomorrow
    atrium-cli edit 42 --due none --scheduled today
    atrium-cli edit 42 --project 'Q3 plans'
    atrium-cli edit 42 --inbox            # move back to Inbox
    atrium-cli edit 42 --tag urgent --remove-tag stale
    atrium-cli edit 42 --clear-tags --tag work    # replace whole set
    atrium-cli complete 42
    atrium-cli complete --where 'is:overdue AND tag:work'
    atrium-cli delete --where 'is:done AND completed:<lastmonth'   # dry run
    atrium-cli delete --where 'is:done AND completed:<lastmonth' --force
    atrium-cli list tags --json | jq '.[] | .name'
    atrium-cli perspective create 'Q3 plans' --filter 'project:\"Q3 plans\"' --icon view-grid-symbolic
    atrium-cli perspective edit 'Q3 plans' --renderer board --columns 'todo,doing,done'
    atrium-cli perspective edit 'Q3 plans' --renderer list   # back to flat
    atrium-cli perspective delete 'Q3 plans'
    atrium-cli import org ~/Tasks/Errands.org
    atrium-cli import org ~/Tasks               # vault directory walk
    atrium-cli import todoist Home.csv --into 'Weekly chores'
    atrium-cli export org ~/Tasks
    atrium-cli export json snapshot.json
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub db_path: Option<PathBuf>,
    pub format: Format,
    pub show_help: bool,
    pub show_version: bool,
    pub subcommand: Option<Subcommand>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Tsv,
    Json,
    Human,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subcommand {
    Search {
        expression: String,
    },
    List {
        name: String,
    },
    Info {
        id: i64,
    },
    Add(AddArgs),
    /// `capture LINE` — Quick-Entry-style one-shot capture.
    /// LINE is a single string parsed for `#tag` / `@date` /
    /// `@deadline ...` inline syntax via atrium_inline (the parser
    /// extracted from atrium-core::quick_entry at v0.13.0).
    Capture {
        line: String,
    },
    Edit {
        id: i64,
        edit: EditArgs,
    },
    /// Toggle completion. `target` is either a single task id or a
    /// search expression; bulk-complete walks every match and
    /// toggles each in turn (matching the GUI's multi-select bulk
    /// shape).
    Complete {
        target: TargetSpec,
    },
    /// Delete one or many. `force` only applies to bulk deletes —
    /// without it, `--where` runs in dry-run mode (prints what
    /// would be deleted, exits status 2).
    Delete {
        target: TargetSpec,
        force: bool,
    },
    /// `kanban NAME` — render a saved Perspective as a kanban board
    /// (Slice D1, v0.5.4). NAME is matched case-insensitively against
    /// `perspective.name`. The perspective's `renderer` must be
    /// `"board"`; otherwise the subcommand errors.
    Kanban {
        name: String,
    },
    /// `perspective SUBCOMMAND ...` — write side for saved
    /// perspectives. Read-side is `list perspectives`. Sub-subcommands:
    /// `create` / `edit` / `delete`. v0.6.5.
    Perspective(PerspectiveSub),
    /// `import SOURCE PATH [--dry-run]` — read a vault file or
    /// other supported source into the DB. Phase 16, v0.7.9.
    /// Currently only `org` is supported.
    Import {
        source: ImportSource,
        path: String,
        dry_run: bool,
    },
    /// `export SOURCE PATH [--dry-run]` — write the DB to a
    /// vault directory. Phase 16, v0.7.10. Currently only `org`
    /// is supported. Each project becomes a `<PATH>/<area>/
    /// <project>.org` file (or `<PATH>/<project>.org` for
    /// unfiled). Atomic writes per spec §7.3.3 rule 6.
    Export {
        source: ExportSource,
        path: String,
        dry_run: bool,
    },
    /// v0.16.0 — `vault sequences SUBCOMMAND ...` — manage the
    /// vault sidecar's `[[todo_sequences]]` (Phase 18.5 Tier-1).
    /// Operates on `<vault>/.atrium/config.toml` directly via the
    /// sidecar helpers; no DB round-trip needed. Vault root is
    /// resolved from a required `--vault PATH` flag (atrium-cli
    /// is process-isolated from the GTK GSettings store, so we
    /// can't reuse the GUI's vault-path key).
    VaultSequences {
        op: VaultSequencesOp,
        vault: String,
    },
    /// v0.17.0 — `clock SUBCOMMAND [ID]` — Phase 18.5 Tier-1
    /// CLOCK time tracking. `clock in <id> [--note TEXT]` opens
    /// an entry. `clock out <id>` closes it. `clock log <id>`
    /// prints the entries (TSV / JSON / human). Bare `clock`
    /// shows the currently-running entry (single-active-clock
    /// invariant — at most one entry across the table is open).
    Clock(ClockSub),
    /// v0.18.0 — `template SUBCOMMAND [...]` — Phase 18.5 Tier-1
    /// Quick Entry templates. CRUD over the
    /// `quick_entry_template` table; the GUI's modal renders
    /// the configured templates as a picker bar above the entry.
    Template(TemplateSub),
}

/// Supported import sources. v0.7.9 ships `Org` (single-file
/// or vault-directory Org-mode importer); v0.12.0 adds
/// `Todoist` (CSV export). Things 3 was retired in v0.6.19.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSource {
    Org,
    /// `import todoist PATH --into PROJECT_NAME`. The Todoist
    /// CSV doesn't carry a project name (a single export is one
    /// project's contents), so the user provides it explicitly.
    Todoist {
        project_name: String,
    },
}

/// Supported export targets. v0.7.10 ships `Org` (vault
/// projection). v0.7.11 adds `Json` (lossless DB snapshot).
/// VTODO and other targets follow in later phases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportSource {
    Org,
    Json,
}

/// Sub-subcommand of `perspective`. Each variant carries its own
/// argument shape; parsing happens in `parse_perspective`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerspectiveSub {
    Create { name: String, args: PerspectiveArgs },
    Edit { name: String, args: PerspectiveArgs },
    Delete { name: String },
}

/// v0.16.0 — sub-subcommand of `vault sequences`. The set
/// operation replaces the configured sequence outright (single-
/// sequence-per-vault is the typical case; multi-sequence support
/// would land here when a real user asks). Clear removes all
/// configured sequences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultSequencesOp {
    /// `vault sequences list` — print the configured sequences
    /// in TSV / JSON / human format.
    List,
    /// `vault sequences set --workflow STATES --done STATES [--name NAME]`.
    /// `workflow` + `done` are comma-separated keyword lists.
    Set {
        name: Option<String>,
        workflow: Vec<String>,
        done: Vec<String>,
    },
    /// `vault sequences clear` — drop all configured sequences.
    Clear,
}

/// v0.17.0 — Phase 18.5 Tier-1 CLOCK time tracking sub-subcommand.
/// `In` opens a clock; `Out` closes it; `Log` prints entries
/// for a task; `Status` (the bare `clock` form) shows the
/// currently-running entry across the whole DB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockSub {
    Status,
    In { task_id: i64, note: String },
    Out { task_id: i64 },
    Log { task_id: i64 },
}

/// v0.18.0 — Phase 18.5 Tier-1 Quick Entry templates sub-subcommand.
/// `List` prints the configured templates. `Add` creates a
/// fresh template (matched at create-time by the worker's
/// uniqueness constraints on name + shortcut_key). `Edit`
/// updates by name. `Remove` deletes by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateSub {
    List,
    Add(TemplateArgs),
    Edit { name: String, args: TemplateArgs },
    Remove { name: String },
}

/// Flags shared by `template add` and `template edit`. On
/// `add`, `name` is required positional. On `edit`, the lookup
/// `name` is the positional and the `rename` flag is the new
/// name (if any). The other fields' `Option` semantics:
/// `None` = leave alone; `Some("none")` clears (where
/// applicable); `Some(value)` sets.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TemplateArgs {
    pub name: Option<String>,
    pub rename: Option<String>,
    pub shortcut: Option<String>,
    pub project: Option<String>,
    pub prefix: Option<String>,
    pub tags: Vec<String>,
}

/// Flags shared by `perspective create` and `perspective edit`. Each
/// `Option<...>` is `None` when the user didn't pass the flag.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PerspectiveArgs {
    /// `create`: required (the perspective's filter expression).
    /// `edit`:   optional new filter; `None` keeps the existing one.
    pub filter: Option<String>,
    /// `create`/`edit`: rename (only meaningful on `edit`).
    pub rename: Option<String>,
    /// `create`/`edit`: icon name. The literal `none` clears it
    /// (back to the default icon).
    pub icon: Option<EditIcon>,
    /// `create`/`edit`: `Some("list")` or `Some("board")`. Together
    /// with `columns`, drives the renderer config.
    pub renderer: Option<String>,
    /// `create`/`edit`: comma-separated column list. Only meaningful
    /// when `renderer == Some("board")` or when editing an existing
    /// board's columns. Empty string is rejected.
    pub columns: Option<String>,
}

/// Tri-state for the icon flag: `None` means leave alone (no flag
/// passed), `Set(name)` sets it, `Clear` clears it (the literal
/// argument `none`). Mirrors the `EditProject::Inbox` shape used
/// by the task `edit` subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditIcon {
    Set(String),
    Clear,
}

/// "What does this command operate on?" — either an explicit task
/// id or a saved-shape search expression. Mutually exclusive at
/// parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetSpec {
    Id(i64),
    Where(String),
}

/// Flag values for the `edit` subcommand. Each `Option<String>` is
/// `None` when the user didn't pass the flag (no change), `Some("none")`
/// for an explicit clear, and `Some(other)` for an explicit set.
/// run_edit (in main.rs) converts these into TaskUpdate field setters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditArgs {
    pub title: Option<String>,
    pub note: Option<String>,
    /// `None` = leave alone, `Some(EditProject::Inbox)` = unfile,
    /// `Some(EditProject::Named(s))` = move to the project matched
    /// by `s`.
    pub project: Option<EditProject>,
    /// Subtasks (Phase 19.5) — `None` = leave alone,
    /// `Some(EditParent::Task(id))` = reparent under `id`,
    /// `Some(EditParent::TopLevel)` = promote to top level
    /// (`--parent none`).
    pub parent: Option<EditParent>,
    pub scheduled: Option<String>,
    pub due: Option<String>,
    pub defer: Option<String>,
    /// `None` = leave alone, `Some("none")` = clear, otherwise the
    /// raw integer text validated at parse time.
    pub estimated: Option<String>,
    /// v0.14.0 — per-task DEADLINE warning window override.
    /// `None` = leave alone, `Some("none")` = clear back to the
    /// global default, otherwise the integer days as text.
    pub deadline_warn: Option<String>,
    /// Tag names to ensure are attached after the field update. Ran
    /// against the current tag set: anything in `tags_add` that
    /// isn't already attached is added; anything already attached
    /// stays. Created via WorkerHandle::ensure_tag if missing.
    pub tags_add: Vec<String>,
    /// Tag names to detach. Quietly no-ops on names that aren't
    /// attached, so scripts don't have to check first.
    pub tags_remove: Vec<String>,
    /// When true, the current tag set is dropped before
    /// `tags_add` applies — the net result is "replace with the
    /// add-set." Composes with `tags_remove` as a no-op since the
    /// set is empty by then.
    pub clear_tags: bool,
}

impl EditArgs {
    /// `true` when the user passed any tag-affecting flag. Used by
    /// run_edit to skip the tag round-trip (read current set →
    /// ensure → set_task_tags) when nothing tag-shaped changed.
    pub fn touches_tags(&self) -> bool {
        self.clear_tags || !self.tags_add.is_empty() || !self.tags_remove.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditProject {
    Inbox,
    Named(String),
}

/// Subtasks (Phase 19.5) — `edit --parent` value. `TopLevel` clears
/// the parent (`--parent none` / `top` / `0`); `Task(id)` reparents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditParent {
    TopLevel,
    Task(i64),
}

/// Fields populated from the `add` subcommand's flag soup. Resolved
/// to a NewTask + project lookup + tag attachments at command time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AddArgs {
    pub title: String,
    pub note: Option<String>,
    pub project: Option<String>,
    /// Subtasks (Phase 19.5) — `--parent ID` nests the new task under
    /// task `ID`. When set without `--project`, run_add inherits the
    /// parent's project so the worker's same-project rule is satisfied.
    pub parent: Option<i64>,
    pub tags: Vec<String>,
    /// Raw text — `today`, `tomorrow`, `someday`, or `YYYY-MM-DD`.
    /// Resolved against `Local::now()` when the command runs.
    pub scheduled: Option<String>,
    pub due: Option<String>,
    pub defer: Option<String>,
    pub estimated_minutes: Option<i64>,
    /// v0.14.0 — per-task DEADLINE warning window. `None` falls
    /// through to the global default; `Some(n)` writes the
    /// override on create.
    pub deadline_warn: Option<i64>,
    /// v0.19.0 — Phase 18.5 Tier-2 time-of-day on schedule.
    /// `--time HH:MM` parses into this; only meaningful when
    /// scheduled_for is also a Date.
    pub scheduled_time: Option<chrono::NaiveTime>,
    /// v0.20.0 — Phase 19.5 reminder timestamp.
    /// `--reminder YYYY-MM-DD HH:MM` parses into this.
    pub reminder_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            db_path: None,
            format: Format::Tsv,
            show_help: false,
            show_version: false,
            subcommand: None,
        }
    }
}

pub fn parse(raw: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut i = 0;

    // ── Global flags (stop when we hit a non-flag → subcommand). ─
    while i < raw.len() {
        let tok = raw[i].as_str();
        match tok {
            "-h" | "--help" => {
                args.show_help = true;
                i += 1;
            }
            "-V" | "--version" => {
                args.show_version = true;
                i += 1;
            }
            "--json" => {
                args.format = Format::Json;
                i += 1;
            }
            "--tsv" => {
                args.format = Format::Tsv;
                i += 1;
            }
            "--human" => {
                args.format = Format::Human;
                i += 1;
            }
            "--db" => {
                i += 1;
                let path = raw.get(i).ok_or("--db requires a path argument")?;
                args.db_path = Some(PathBuf::from(path));
                i += 1;
            }
            // First non-flag terminates global parsing — subcommand follows.
            _ if !tok.starts_with('-') => break,
            other => return Err(format!("unknown flag: {other}")),
        }
    }

    if args.show_help || args.show_version {
        return Ok(args);
    }

    // ── Subcommand. ──────────────────────────────────────────────
    let Some(name) = raw.get(i) else {
        return Ok(args);
    };
    i += 1;

    args.subcommand = Some(match name.as_str() {
        "search" => {
            // Allow trailing global flags to interleave with the
            // expression (atrium-cli search foo --json) by joining
            // every remaining non-flag token.
            let (rest, trailing_flags) = collect_expression_and_flags(&raw[i..]);
            apply_trailing_flags(&trailing_flags, &mut args)?;
            if rest.trim().is_empty() {
                return Err("search expression required".into());
            }
            Subcommand::Search { expression: rest }
        }
        "list" => {
            let name = raw
                .get(i)
                .ok_or("list requires a name (inbox / today / …)")?
                .clone();
            i += 1;
            apply_trailing_flags(&raw[i..], &mut args)?;
            Subcommand::List { name }
        }
        "info" => {
            let id_str = raw.get(i).ok_or("info requires a task id")?;
            i += 1;
            let id: i64 = id_str
                .parse()
                .map_err(|_| format!("invalid task id: {id_str}"))?;
            apply_trailing_flags(&raw[i..], &mut args)?;
            Subcommand::Info { id }
        }
        "add" => parse_add(&raw[i..], &mut args)?,
        "capture" => {
            // `capture` joins the rest of argv into one line so the
            // user doesn't have to quote unless they want to embed a
            // literal newline. Trailing global flags are still
            // honoured (atrium-cli capture 'Buy milk #errand' --json).
            let (line, trailing) = collect_expression_and_flags(&raw[i..]);
            apply_trailing_flags(&trailing, &mut args)?;
            if line.trim().is_empty() {
                return Err("capture requires a line of text".into());
            }
            Subcommand::Capture { line }
        }
        "edit" | "modify" => {
            let id_str = raw.get(i).ok_or("edit requires a task id")?;
            i += 1;
            let id: i64 = id_str
                .parse()
                .map_err(|_| format!("invalid task id: {id_str}"))?;
            let edit = parse_edit(&raw[i..], &mut args)?;
            Subcommand::Edit { id, edit }
        }
        "complete" | "done" | "toggle" => {
            let (target, _force) = parse_target_and_flags(&raw[i..], false, &mut args)?;
            Subcommand::Complete { target }
        }
        "delete" | "rm" => {
            let (target, force) = parse_target_and_flags(&raw[i..], true, &mut args)?;
            Subcommand::Delete { target, force }
        }
        "kanban" | "board" => {
            // Same shape as `capture` — collect the rest of argv as
            // the perspective name (so multi-word names don't need
            // quoting) and honour any trailing format flags.
            let (name, trailing) = collect_expression_and_flags(&raw[i..]);
            apply_trailing_flags(&trailing, &mut args)?;
            if name.trim().is_empty() {
                return Err("kanban requires a perspective name".into());
            }
            Subcommand::Kanban { name }
        }
        "perspective" => parse_perspective(&raw[i..], &mut args)?,
        "import" => parse_import(&raw[i..], &mut args)?,
        "export" => parse_export(&raw[i..], &mut args)?,
        "vault" => parse_vault(&raw[i..], &mut args)?,
        "clock" => parse_clock(&raw[i..], &mut args)?,
        "template" => parse_template(&raw[i..], &mut args)?,
        other => return Err(format!("unknown subcommand: {other}")),
    });

    Ok(args)
}

/// Walk the rest of argv pulling out the `add` subcommand's flags
/// and the leading TITLE positional. Flags can interleave with
/// global format flags (atrium-cli add 'Buy milk' --tag errand --json),
/// matching the search-subcommand convention.
fn parse_add(rest: &[String], args: &mut Args) -> Result<Subcommand, String> {
    let mut add = AddArgs::default();
    let mut title_words: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let tok = rest[i].as_str();
        match tok {
            "--note" => {
                i += 1;
                let v = rest.get(i).ok_or("--note requires a value")?;
                add.note = Some(v.clone());
                i += 1;
            }
            "--project" => {
                i += 1;
                let v = rest.get(i).ok_or("--project requires a value")?;
                add.project = Some(v.clone());
                i += 1;
            }
            "--parent" => {
                i += 1;
                let v = rest.get(i).ok_or("--parent requires a task id")?;
                let id: i64 = v
                    .parse()
                    .map_err(|_| format!("--parent must be a task id (integer), got {v}"))?;
                add.parent = Some(id);
                i += 1;
            }
            "--tag" => {
                i += 1;
                let v = rest.get(i).ok_or("--tag requires a value")?;
                add.tags.push(v.clone());
                i += 1;
            }
            "--scheduled" | "--when" => {
                i += 1;
                let v = rest.get(i).ok_or("--scheduled requires a value")?;
                add.scheduled = Some(v.clone());
                i += 1;
            }
            "--due" | "--deadline" => {
                i += 1;
                let v = rest.get(i).ok_or("--due requires a value")?;
                add.due = Some(v.clone());
                i += 1;
            }
            "--defer" | "--defer-until" => {
                i += 1;
                let v = rest.get(i).ok_or("--defer requires a value")?;
                add.defer = Some(v.clone());
                i += 1;
            }
            "--estimated" | "--est" => {
                i += 1;
                let v = rest.get(i).ok_or("--estimated requires a value")?;
                let n: i64 = v
                    .parse()
                    .map_err(|_| format!("--estimated must be an integer, got {v}"))?;
                add.estimated_minutes = Some(n);
                i += 1;
            }
            "--deadline-warn" | "--warn" => {
                i += 1;
                let v = rest.get(i).ok_or("--deadline-warn requires a value")?;
                let n: i64 = v
                    .parse()
                    .map_err(|_| format!("--deadline-warn must be an integer, got {v}"))?;
                if n < 0 {
                    return Err("--deadline-warn must be a non-negative integer".into());
                }
                add.deadline_warn = Some(n);
                i += 1;
            }
            "--time" => {
                i += 1;
                let v = rest.get(i).ok_or("--time requires a value (HH:MM)")?;
                let t = chrono::NaiveTime::parse_from_str(v, "%H:%M")
                    .map_err(|_| format!("--time must be HH:MM, got {v}"))?;
                add.scheduled_time = Some(t);
                i += 1;
            }
            "--reminder" => {
                i += 1;
                let v = rest
                    .get(i)
                    .ok_or("--reminder requires a value (YYYY-MM-DD HH:MM)")?;
                use chrono::TimeZone;
                let naive = chrono::NaiveDateTime::parse_from_str(v, "%Y-%m-%d %H:%M")
                    .map_err(|_| format!("--reminder must be YYYY-MM-DD HH:MM, got {v}"))?;
                let local = chrono::Local
                    .from_local_datetime(&naive)
                    .single()
                    .ok_or("--reminder timestamp is ambiguous (DST gap)")?;
                add.reminder_at = Some(local.with_timezone(&chrono::Utc));
                i += 1;
            }
            // Global format flags can appear anywhere.
            "--json" => {
                args.format = Format::Json;
                i += 1;
            }
            "--tsv" => {
                args.format = Format::Tsv;
                i += 1;
            }
            "--human" => {
                args.format = Format::Human;
                i += 1;
            }
            "--db" => {
                i += 1;
                let path = rest.get(i).ok_or("--db requires a path argument")?;
                args.db_path = Some(PathBuf::from(path));
                i += 1;
            }
            other if other.starts_with("--") => return Err(format!("unknown flag: {other}")),
            // Anything else is a title word; multiple words join.
            _ => {
                title_words.push(tok);
                i += 1;
            }
        }
    }
    add.title = title_words.join(" ");
    if add.title.trim().is_empty() {
        return Err("add requires a title".into());
    }
    Ok(Subcommand::Add(add))
}

/// Parse the rest-of-argv for a `complete` or `delete` subcommand.
/// Returns the resolved `TargetSpec` (either an explicit task id
/// or a `--where EXPR` search expression) and the `force` boolean
/// (only meaningful for `delete`; ignored by `complete`).
///
/// The two forms are mutually exclusive: either pass an id as the
/// first positional, or pass `--where EXPR` (where EXPR can span
/// multiple non-flag tokens, like `search`). `--force` is only
/// recognised when `accept_force` is true.
fn parse_target_and_flags(
    rest: &[String],
    accept_force: bool,
    args: &mut Args,
) -> Result<(TargetSpec, bool), String> {
    let mut id: Option<i64> = None;
    let mut where_words: Vec<String> = Vec::new();
    let mut where_active = false;
    let mut force = false;
    let mut i = 0;
    while i < rest.len() {
        let tok = rest[i].as_str();
        match tok {
            "--where" | "--filter" => {
                i += 1;
                where_active = true;
            }
            "--force" | "--yes" if accept_force => {
                force = true;
                i += 1;
                where_active = false;
            }
            "--json" => {
                args.format = Format::Json;
                i += 1;
                where_active = false;
            }
            "--tsv" => {
                args.format = Format::Tsv;
                i += 1;
                where_active = false;
            }
            "--human" => {
                args.format = Format::Human;
                i += 1;
                where_active = false;
            }
            "--db" => {
                i += 1;
                let path = rest.get(i).ok_or("--db requires a path argument")?;
                args.db_path = Some(PathBuf::from(path));
                i += 1;
                where_active = false;
            }
            other if other.starts_with("--") => return Err(format!("unknown flag: {other}")),
            // Positional: first non-flag is the id when --where
            // hasn't been seen, otherwise it's a where-expression word.
            _ => {
                if where_active {
                    where_words.push(rest[i].clone());
                    i += 1;
                } else if id.is_none() {
                    id = Some(
                        rest[i]
                            .parse::<i64>()
                            .map_err(|_| format!("invalid task id: {}", rest[i]))?,
                    );
                    i += 1;
                } else {
                    return Err(format!("unexpected positional: {}", rest[i]));
                }
            }
        }
    }
    let target = match (id, where_words.is_empty()) {
        (Some(_), false) => return Err("pass either an id or --where EXPR, not both".into()),
        (Some(id), true) => TargetSpec::Id(id),
        (None, false) => TargetSpec::Where(where_words.join(" ")),
        (None, true) => return Err("requires a task id or --where EXPR".into()),
    };
    Ok((target, force))
}

/// Walk argv after `edit ID` pulling out per-field flags. Each flag
/// is recorded as Some-or-None on EditArgs; run_edit translates that
/// into TaskUpdate. Magic value `none` clears a nullable field.
fn parse_edit(rest: &[String], args: &mut Args) -> Result<EditArgs, String> {
    let mut edit = EditArgs::default();
    let mut i = 0;
    while i < rest.len() {
        let tok = rest[i].as_str();
        match tok {
            "--title" => {
                i += 1;
                let v = rest.get(i).ok_or("--title requires a value")?;
                edit.title = Some(v.clone());
                i += 1;
            }
            "--note" => {
                i += 1;
                let v = rest.get(i).ok_or("--note requires a value")?;
                edit.note = Some(v.clone());
                i += 1;
            }
            "--project" => {
                i += 1;
                let v = rest.get(i).ok_or("--project requires a value")?;
                if v.eq_ignore_ascii_case("inbox") {
                    edit.project = Some(EditProject::Inbox);
                } else {
                    edit.project = Some(EditProject::Named(v.clone()));
                }
                i += 1;
            }
            "--inbox" | "--unfile" => {
                edit.project = Some(EditProject::Inbox);
                i += 1;
            }
            "--parent" => {
                i += 1;
                let v = rest.get(i).ok_or("--parent requires a task id or 'none'")?;
                if v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("top") || v == "0" {
                    edit.parent = Some(EditParent::TopLevel);
                } else {
                    let id: i64 = v
                        .parse()
                        .map_err(|_| format!("--parent must be a task id or 'none', got {v}"))?;
                    edit.parent = Some(EditParent::Task(id));
                }
                i += 1;
            }
            "--scheduled" | "--when" => {
                i += 1;
                let v = rest.get(i).ok_or("--scheduled requires a value")?;
                edit.scheduled = Some(v.clone());
                i += 1;
            }
            "--due" | "--deadline" => {
                i += 1;
                let v = rest.get(i).ok_or("--due requires a value")?;
                edit.due = Some(v.clone());
                i += 1;
            }
            "--defer" | "--defer-until" => {
                i += 1;
                let v = rest.get(i).ok_or("--defer requires a value")?;
                edit.defer = Some(v.clone());
                i += 1;
            }
            "--estimated" | "--est" => {
                i += 1;
                let v = rest.get(i).ok_or("--estimated requires a value")?;
                if !v.eq_ignore_ascii_case("none") {
                    // Validate at parse time so the user sees a
                    // syntax error before we open the database.
                    v.parse::<i64>().map_err(|_| {
                        format!("--estimated must be an integer or 'none', got {v}")
                    })?;
                }
                edit.estimated = Some(v.clone());
                i += 1;
            }
            "--deadline-warn" | "--warn" => {
                i += 1;
                let v = rest.get(i).ok_or("--deadline-warn requires a value")?;
                if !v.eq_ignore_ascii_case("none") {
                    let n: i64 = v.parse().map_err(|_| {
                        format!("--deadline-warn must be an integer or 'none', got {v}")
                    })?;
                    if n < 0 {
                        return Err("--deadline-warn must be a non-negative integer".into());
                    }
                }
                edit.deadline_warn = Some(v.clone());
                i += 1;
            }
            "--tag" | "--add-tag" => {
                i += 1;
                let v = rest.get(i).ok_or("--tag requires a value")?;
                edit.tags_add.push(v.clone());
                i += 1;
            }
            "--remove-tag" | "--untag" => {
                i += 1;
                let v = rest.get(i).ok_or("--remove-tag requires a value")?;
                edit.tags_remove.push(v.clone());
                i += 1;
            }
            "--clear-tags" => {
                edit.clear_tags = true;
                i += 1;
            }
            // Global format flags can appear anywhere.
            "--json" => {
                args.format = Format::Json;
                i += 1;
            }
            "--tsv" => {
                args.format = Format::Tsv;
                i += 1;
            }
            "--human" => {
                args.format = Format::Human;
                i += 1;
            }
            "--db" => {
                i += 1;
                let path = rest.get(i).ok_or("--db requires a path argument")?;
                args.db_path = Some(PathBuf::from(path));
                i += 1;
            }
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    // edit with no flags is a no-op; we accept it (run_edit prints
    // the unchanged row) so users can use `edit ID` as a "show
    // single task in the list-row format" companion to `info`.
    Ok(edit)
}

/// Parse `export <source> <path> [--dry-run]`. v0.7.10 ships
/// only the `org` source (write every project to a vault
/// directory). Mirrors the `parse_import` shape.
fn parse_export(rest: &[String], args: &mut Args) -> Result<Subcommand, String> {
    let source_str = rest
        .first()
        .ok_or("export requires a source: org")?
        .as_str();
    let source = match source_str {
        "org" => ExportSource::Org,
        "json" => ExportSource::Json,
        other => return Err(format!("unknown export source: {other}")),
    };
    let body = &rest[1..];

    let mut path: Option<String> = None;
    let mut dry_run = false;
    let mut trailing: Vec<String> = Vec::new();
    let mut iter = body.iter();
    while let Some(tok) = iter.next() {
        match tok.as_str() {
            "--dry-run" => dry_run = true,
            "--json" | "--tsv" | "--human" => trailing.push(tok.clone()),
            "--db" => {
                trailing.push(tok.clone());
                let next = iter
                    .next()
                    .ok_or_else(|| "--db requires a path".to_string())?;
                trailing.push(next.clone());
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown flag: {other}"));
            }
            positional => {
                if path.is_some() {
                    return Err(format!(
                        "export takes a single PATH argument; extra positional: {positional}"
                    ));
                }
                path = Some(positional.to_string());
            }
        }
    }
    apply_trailing_flags(&trailing, args)?;

    let path = path.ok_or("export requires a path argument")?;
    Ok(Subcommand::Export {
        source,
        path,
        dry_run,
    })
}

/// Parse `import <source> <path> [...flags]`. Sources:
///
/// - `org` — single-file or vault-directory Org-mode import
///   (Phase 16, v0.7.9+).
/// - `todoist` — CSV export from Todoist's per-project export
///   (Phase 18, v0.12.0). Requires `--into PROJECT_NAME` because
///   the export doesn't carry a project name.
///
/// Trailing global format flags (`--json`, `--human`, `--db
/// PATH`) honour the standard apply_trailing_flags pass.
/// `--dry-run` skips DB writes and prints what *would* happen.
fn parse_import(rest: &[String], args: &mut Args) -> Result<Subcommand, String> {
    let source_str = rest
        .first()
        .ok_or("import requires a source: org | todoist")?
        .as_str();
    // Source-specific flags (e.g. todoist's `--into`) are parsed
    // alongside the common ones — we tag the source first, then
    // collect the project_name during the body walk.
    let source_kind = match source_str {
        "org" => SourceKind::Org,
        "todoist" => SourceKind::Todoist,
        other => return Err(format!("unknown import source: {other}")),
    };
    let body = &rest[1..];

    let mut path: Option<String> = None;
    let mut dry_run = false;
    let mut into_project: Option<String> = None;
    let mut trailing: Vec<String> = Vec::new();
    let mut iter = body.iter();
    while let Some(tok) = iter.next() {
        match tok.as_str() {
            "--dry-run" => dry_run = true,
            "--into" => {
                let next = iter
                    .next()
                    .ok_or_else(|| "--into requires a project name".to_string())?;
                into_project = Some(next.clone());
            }
            "--json" | "--tsv" | "--human" => trailing.push(tok.clone()),
            "--db" => {
                trailing.push(tok.clone());
                let next = iter
                    .next()
                    .ok_or_else(|| "--db requires a path".to_string())?;
                trailing.push(next.clone());
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown flag: {other}"));
            }
            positional => {
                if path.is_some() {
                    return Err(format!(
                        "import takes a single PATH argument; extra positional: {positional}"
                    ));
                }
                path = Some(positional.to_string());
            }
        }
    }
    apply_trailing_flags(&trailing, args)?;

    let path = path.ok_or("import requires a path argument")?;
    let source = match source_kind {
        SourceKind::Org => {
            if into_project.is_some() {
                return Err(
                    "import org doesn't accept --into; the project name comes from the file"
                        .to_string(),
                );
            }
            ImportSource::Org
        }
        SourceKind::Todoist => {
            let project_name = into_project.ok_or("import todoist requires --into PROJECT_NAME")?;
            ImportSource::Todoist { project_name }
        }
    };
    Ok(Subcommand::Import {
        source,
        path,
        dry_run,
    })
}

/// Discriminator captured during `parse_import`'s body walk so
/// source-specific flags (like todoist's `--into`) can mix with
/// the common ones in any order.
enum SourceKind {
    Org,
    Todoist,
}

/// Parse the rest-of-argv for `perspective <create|edit|delete>
/// NAME [...flags]`. Sub-subcommand kind is at rest[0]; the
/// perspective name is collected from non-flag tokens up to the
/// first flag (so multi-word names work without quoting). Flags
/// after the name follow the same vocabulary as the matching task
/// edit shape — `--filter EXPR`, `--icon NAME|none`,
/// `--rename NEW`, `--renderer list|board`, `--columns "a,b,c"`.
fn parse_perspective(rest: &[String], args: &mut Args) -> Result<Subcommand, String> {
    let kind = rest
        .first()
        .ok_or("perspective requires a sub-subcommand: create / edit / delete")?
        .as_str();
    let body = &rest[1..];
    match kind {
        "create" => {
            let (name, perspective_args) = parse_perspective_args(body, args, true)?;
            if perspective_args.filter.is_none() {
                return Err("perspective create requires --filter EXPR".into());
            }
            if perspective_args.rename.is_some() {
                return Err("perspective create does not accept --rename".into());
            }
            Ok(Subcommand::Perspective(PerspectiveSub::Create {
                name,
                args: perspective_args,
            }))
        }
        "edit" => {
            let (name, perspective_args) = parse_perspective_args(body, args, false)?;
            Ok(Subcommand::Perspective(PerspectiveSub::Edit {
                name,
                args: perspective_args,
            }))
        }
        "delete" | "rm" => {
            // `delete` doesn't take any flags beyond the name (and
            // global format flags). We share the parser to honour
            // `--db` and friends, but reject the body-shaped flags.
            let (name, perspective_args) = parse_perspective_args(body, args, false)?;
            if perspective_args.filter.is_some()
                || perspective_args.rename.is_some()
                || perspective_args.icon.is_some()
                || perspective_args.renderer.is_some()
                || perspective_args.columns.is_some()
            {
                return Err(
                    "perspective delete only takes a name (and global flags); did you mean edit?"
                        .into(),
                );
            }
            Ok(Subcommand::Perspective(PerspectiveSub::Delete { name }))
        }
        other => Err(format!(
            "perspective: unknown sub-subcommand: {other} (expected create / edit / delete)"
        )),
    }
}

/// Shared body parser for `perspective create` and `perspective
/// edit`. Returns the perspective name (multi-word OK, joined with
/// spaces) plus the parsed flag bundle. `expect_filter_required` is
/// a hint to the caller — we don't enforce it here so the
/// "did you forget --filter?" error message can stay specific.
fn parse_perspective_args(
    rest: &[String],
    args: &mut Args,
    _expect_filter_required: bool,
) -> Result<(String, PerspectiveArgs), String> {
    let mut name_words: Vec<&str> = Vec::new();
    let mut p = PerspectiveArgs::default();
    let mut i = 0;
    while i < rest.len() {
        let tok = rest[i].as_str();
        match tok {
            "--filter" => {
                i += 1;
                let v = rest.get(i).ok_or("--filter requires an expression")?;
                p.filter = Some(v.clone());
                i += 1;
            }
            "--rename" => {
                i += 1;
                let v = rest.get(i).ok_or("--rename requires a new name")?;
                p.rename = Some(v.clone());
                i += 1;
            }
            "--icon" => {
                i += 1;
                let v = rest.get(i).ok_or("--icon requires a value")?;
                p.icon = Some(if v.eq_ignore_ascii_case("none") {
                    EditIcon::Clear
                } else {
                    EditIcon::Set(v.clone())
                });
                i += 1;
            }
            "--renderer" => {
                i += 1;
                let v = rest.get(i).ok_or("--renderer requires list or board")?;
                let lower = v.to_ascii_lowercase();
                if lower != "list" && lower != "board" {
                    return Err(format!("--renderer must be 'list' or 'board', got {v}"));
                }
                p.renderer = Some(lower);
                i += 1;
            }
            "--columns" => {
                i += 1;
                let v = rest.get(i).ok_or("--columns requires a value")?;
                p.columns = Some(v.clone());
                i += 1;
            }
            // Global format/db flags can appear anywhere.
            "--json" => {
                args.format = Format::Json;
                i += 1;
            }
            "--tsv" => {
                args.format = Format::Tsv;
                i += 1;
            }
            "--human" => {
                args.format = Format::Human;
                i += 1;
            }
            "--db" => {
                i += 1;
                let path = rest.get(i).ok_or("--db requires a path argument")?;
                args.db_path = Some(PathBuf::from(path));
                i += 1;
            }
            other if other.starts_with("--") => return Err(format!("unknown flag: {other}")),
            _ => {
                name_words.push(tok);
                i += 1;
            }
        }
    }
    let name = name_words.join(" ");
    if name.trim().is_empty() {
        return Err("perspective requires a name".into());
    }
    Ok((name, p))
}

/// v0.16.0 — `vault SUBCOMMAND ARGS`. Currently dispatches only
/// `sequences`. Future work could add `vault tags` / `vault
/// perspectives` here, but for now the sidecar's tags/perspectives
/// are GUI-managed and the CLI stays narrow.
fn parse_vault(rest: &[String], args: &mut Args) -> Result<Subcommand, String> {
    let sub = rest
        .first()
        .ok_or("vault requires a sub-subcommand (sequences)")?;
    match sub.as_str() {
        "sequences" => parse_vault_sequences(&rest[1..], args),
        other => Err(format!("unknown vault sub-subcommand: {other}")),
    }
}

/// `vault sequences SUBCOMMAND [--vault PATH] [...]`. Vault path
/// is a required flag because atrium-cli is process-isolated from
/// the GTK GSettings store; without `--vault` the subcommand
/// can't know which sidecar to read or write.
fn parse_vault_sequences(rest: &[String], args: &mut Args) -> Result<Subcommand, String> {
    let sub = rest
        .first()
        .ok_or("vault sequences requires a sub-subcommand (list / set / clear)")?;
    let body = &rest[1..];

    let mut vault: Option<String> = None;
    let mut name: Option<String> = None;
    let mut workflow: Vec<String> = Vec::new();
    let mut done: Vec<String> = Vec::new();

    let mut i = 0;
    while i < body.len() {
        let tok = body[i].as_str();
        match tok {
            "--vault" => {
                i += 1;
                vault = Some(
                    body.get(i)
                        .ok_or("--vault requires a path argument")?
                        .clone(),
                );
                i += 1;
            }
            "--name" => {
                i += 1;
                name = Some(body.get(i).ok_or("--name requires a value")?.clone());
                i += 1;
            }
            "--workflow" => {
                i += 1;
                let v = body.get(i).ok_or("--workflow requires a value")?;
                workflow = split_keyword_list(v);
                i += 1;
            }
            "--done" => {
                i += 1;
                let v = body.get(i).ok_or("--done requires a value")?;
                done = split_keyword_list(v);
                i += 1;
            }
            "--json" => {
                args.format = Format::Json;
                i += 1;
            }
            "--tsv" => {
                args.format = Format::Tsv;
                i += 1;
            }
            "--human" => {
                args.format = Format::Human;
                i += 1;
            }
            other => return Err(format!("unknown flag: {other}")),
        }
    }

    let vault = vault.ok_or("vault sequences requires --vault PATH")?;

    let op = match sub.as_str() {
        "list" => VaultSequencesOp::List,
        "set" => {
            if workflow.is_empty() && done.is_empty() {
                return Err(
                    "vault sequences set requires --workflow and/or --done with at least one keyword".into(),
                );
            }
            VaultSequencesOp::Set {
                name,
                workflow,
                done,
            }
        }
        "clear" => VaultSequencesOp::Clear,
        other => return Err(format!("unknown vault sequences sub-subcommand: {other}")),
    };

    Ok(Subcommand::VaultSequences { op, vault })
}

/// v0.17.0 — `clock SUBCOMMAND [ID] [--note TEXT]`. Bare `clock`
/// (no SUBCOMMAND) is sugar for `clock status` — print the
/// currently-running entry, or "(no clock running)" when none.
fn parse_clock(rest: &[String], args: &mut Args) -> Result<Subcommand, String> {
    let Some(sub) = rest.first() else {
        // Bare `clock` → status.
        return Ok(Subcommand::Clock(ClockSub::Status));
    };
    let body = &rest[1..];

    let mut note = String::new();
    let mut positional: Option<i64> = None;
    let mut i = 0;
    while i < body.len() {
        let tok = body[i].as_str();
        match tok {
            "--note" => {
                i += 1;
                let v = body.get(i).ok_or("--note requires a value")?;
                note = v.clone();
                i += 1;
            }
            "--json" => {
                args.format = Format::Json;
                i += 1;
            }
            "--tsv" => {
                args.format = Format::Tsv;
                i += 1;
            }
            "--human" => {
                args.format = Format::Human;
                i += 1;
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown flag: {other}"));
            }
            // Positional task id.
            _ => {
                if positional.is_some() {
                    return Err(format!("unexpected positional argument: {tok}"));
                }
                positional = Some(tok.parse().map_err(|_| format!("invalid task id: {tok}"))?);
                i += 1;
            }
        }
    }

    let op = match sub.as_str() {
        "status" => ClockSub::Status,
        "in" | "start" => {
            let task_id = positional.ok_or("clock in requires a task id")?;
            ClockSub::In { task_id, note }
        }
        "out" | "stop" => {
            let task_id = positional.ok_or("clock out requires a task id")?;
            ClockSub::Out { task_id }
        }
        "log" => {
            let task_id = positional.ok_or("clock log requires a task id")?;
            ClockSub::Log { task_id }
        }
        other => return Err(format!("unknown clock sub-subcommand: {other}")),
    };
    Ok(Subcommand::Clock(op))
}

/// v0.18.0 — `template SUBCOMMAND ...`. Dispatches list / add
/// / edit / remove; flag soup is parsed via `parse_template_flags`.
fn parse_template(rest: &[String], args: &mut Args) -> Result<Subcommand, String> {
    let sub = rest
        .first()
        .ok_or("template requires a sub-subcommand (list / add / edit / remove)")?;
    match sub.as_str() {
        "list" => {
            apply_trailing_flags(&rest[1..], args)?;
            Ok(Subcommand::Template(TemplateSub::List))
        }
        "add" => {
            let body = &rest[1..];
            let template_args = parse_template_flags(body, args)?;
            let name = template_args
                .name
                .clone()
                .ok_or("template add requires a name (positional)")?;
            Ok(Subcommand::Template(TemplateSub::Add(TemplateArgs {
                name: Some(name),
                ..template_args
            })))
        }
        "edit" => {
            let body = &rest[1..];
            let template_args = parse_template_flags(body, args)?;
            let name = template_args
                .name
                .clone()
                .ok_or("template edit requires a name (positional)")?;
            Ok(Subcommand::Template(TemplateSub::Edit {
                name,
                args: TemplateArgs {
                    name: None,
                    ..template_args
                },
            }))
        }
        "remove" | "delete" => {
            let body = &rest[1..];
            let template_args = parse_template_flags(body, args)?;
            let name = template_args
                .name
                .ok_or("template remove requires a name (positional)")?;
            Ok(Subcommand::Template(TemplateSub::Remove { name }))
        }
        other => Err(format!("unknown template sub-subcommand: {other}")),
    }
}

/// Parse the `template add` / `template edit` / `template remove`
/// flag soup. Positional non-flag tokens become the template
/// name (concatenated with spaces so multi-word names don't
/// need quoting).
fn parse_template_flags(rest: &[String], args: &mut Args) -> Result<TemplateArgs, String> {
    let mut out = TemplateArgs::default();
    let mut name_words: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let tok = rest[i].as_str();
        match tok {
            "--rename" => {
                i += 1;
                let v = rest.get(i).ok_or("--rename requires a value")?;
                out.rename = Some(v.clone());
                i += 1;
            }
            "--shortcut" | "--key" => {
                i += 1;
                let v = rest.get(i).ok_or("--shortcut requires a value")?;
                out.shortcut = Some(v.clone());
                i += 1;
            }
            "--project" => {
                i += 1;
                let v = rest.get(i).ok_or("--project requires a value")?;
                out.project = Some(v.clone());
                i += 1;
            }
            "--prefix" => {
                i += 1;
                let v = rest.get(i).ok_or("--prefix requires a value")?;
                out.prefix = Some(v.clone());
                i += 1;
            }
            "--tag" => {
                i += 1;
                let v = rest.get(i).ok_or("--tag requires a value")?;
                out.tags.push(v.clone());
                i += 1;
            }
            "--json" => {
                args.format = Format::Json;
                i += 1;
            }
            "--tsv" => {
                args.format = Format::Tsv;
                i += 1;
            }
            "--human" => {
                args.format = Format::Human;
                i += 1;
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown flag: {other}"));
            }
            _ => {
                name_words.push(tok);
                i += 1;
            }
        }
    }
    if !name_words.is_empty() {
        out.name = Some(name_words.join(" "));
    }
    Ok(out)
}

/// Split a `--workflow TODO,NEXT,WAITING` argument into individual
/// keywords. Trims whitespace per element; drops empty entries.
fn split_keyword_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Pull non-flag tokens into a space-joined expression, leaving
/// flag-shaped tokens for the trailing-flag pass. This lets the
/// user write `atrium-cli search tag:work --json` without quoting
/// the expression.
fn collect_expression_and_flags(rest: &[String]) -> (String, Vec<String>) {
    let mut expression_words: Vec<&str> = Vec::new();
    let mut trailing: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let tok = rest[i].as_str();
        if tok == "--db" {
            // Two-token flag — consume the value too.
            trailing.push(rest[i].clone());
            if let Some(v) = rest.get(i + 1) {
                trailing.push(v.clone());
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if matches!(
            tok,
            "--json" | "--tsv" | "--human" | "--help" | "-h" | "-V" | "--version"
        ) {
            trailing.push(rest[i].clone());
            i += 1;
            continue;
        }
        expression_words.push(tok);
        i += 1;
    }
    (expression_words.join(" "), trailing)
}

/// Re-apply global flags that appeared after a subcommand (e.g.,
/// `atrium-cli list today --json`).
fn apply_trailing_flags(flags: &[String], args: &mut Args) -> Result<(), String> {
    let mut i = 0;
    while i < flags.len() {
        match flags[i].as_str() {
            "--json" => args.format = Format::Json,
            "--tsv" => args.format = Format::Tsv,
            "--human" => args.format = Format::Human,
            "-h" | "--help" => args.show_help = true,
            "-V" | "--version" => args.show_version = true,
            "--db" => {
                i += 1;
                let path = flags.get(i).ok_or("--db requires a path argument")?;
                args.db_path = Some(PathBuf::from(path));
            }
            other => return Err(format!("unexpected token: {other}")),
        }
        i += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! v0.21.0 — Phase 4 maintenance pass added these tests to
    //! cover the previously-untested argv parser. Focus is on the
    //! shapes most likely to regress: subcommand routing, the
    //! global format flags, and the higher-flag-density commands
    //! (`add`, `template`, `clock`).

    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_string()).collect()
    }

    // ── Global flags ──────────────────────────────────────────────

    #[test]
    fn parse_help_short_and_long() {
        let r = parse(&argv(&["-h"])).unwrap();
        assert!(r.show_help);
        let r = parse(&argv(&["--help"])).unwrap();
        assert!(r.show_help);
    }

    #[test]
    fn parse_version_short_and_long() {
        let r = parse(&argv(&["-V"])).unwrap();
        assert!(r.show_version);
        let r = parse(&argv(&["--version"])).unwrap();
        assert!(r.show_version);
    }

    #[test]
    fn parse_default_format_is_tsv() {
        let r = parse(&argv(&["list", "today"])).unwrap();
        assert_eq!(r.format, Format::Tsv);
    }

    #[test]
    fn parse_format_flags() {
        let r = parse(&argv(&["--json", "list", "today"])).unwrap();
        assert_eq!(r.format, Format::Json);
        let r = parse(&argv(&["--human", "list", "today"])).unwrap();
        assert_eq!(r.format, Format::Human);
        let r = parse(&argv(&["--tsv", "list", "today"])).unwrap();
        assert_eq!(r.format, Format::Tsv);
    }

    #[test]
    fn parse_db_path_flag() {
        let r = parse(&argv(&["--db", "/tmp/test.db", "list", "today"])).unwrap();
        assert_eq!(r.db_path, Some(PathBuf::from("/tmp/test.db")));
    }

    #[test]
    fn parse_db_flag_missing_path_errors() {
        let err = parse(&argv(&["--db"])).unwrap_err();
        assert!(err.contains("--db"), "got: {err}");
    }

    // ── Subcommand routing ────────────────────────────────────────

    #[test]
    fn parse_search_collects_expression_words() {
        let r = parse(&argv(&["search", "tag:work", "AND", "is:open"])).unwrap();
        match r.subcommand {
            Some(Subcommand::Search { expression }) => {
                assert_eq!(expression, "tag:work AND is:open");
            }
            other => panic!("expected Search, got {other:?}"),
        }
    }

    #[test]
    fn parse_list_with_name() {
        let r = parse(&argv(&["list", "today"])).unwrap();
        match r.subcommand {
            Some(Subcommand::List { name }) => assert_eq!(name, "today"),
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn parse_list_missing_name_errors() {
        let err = parse(&argv(&["list"])).unwrap_err();
        assert!(err.to_lowercase().contains("name"), "got: {err}");
    }

    #[test]
    fn parse_info_with_id() {
        let r = parse(&argv(&["info", "42"])).unwrap();
        match r.subcommand {
            Some(Subcommand::Info { id }) => assert_eq!(id, 42),
            other => panic!("expected Info, got {other:?}"),
        }
    }

    #[test]
    fn parse_info_invalid_id_errors() {
        assert!(parse(&argv(&["info", "abc"])).is_err());
    }

    #[test]
    fn parse_complete_no_target_errors() {
        let err = parse(&argv(&["complete"])).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn parse_complete_invalid_id_errors() {
        assert!(parse(&argv(&["complete", "not-an-id"])).is_err());
    }

    #[test]
    fn parse_complete_alias_done() {
        let r = parse(&argv(&["done", "7"])).unwrap();
        assert!(matches!(r.subcommand, Some(Subcommand::Complete { .. })));
    }

    #[test]
    fn parse_delete_alias_rm() {
        let r = parse(&argv(&["rm", "9"])).unwrap();
        assert!(matches!(r.subcommand, Some(Subcommand::Delete { .. })));
    }

    // ── add subcommand ────────────────────────────────────────────

    #[test]
    fn parse_add_minimal() {
        let r = parse(&argv(&["add", "Buy milk"])).unwrap();
        match r.subcommand {
            Some(Subcommand::Add(a)) => {
                assert_eq!(a.title, "Buy milk");
                assert!(a.tags.is_empty());
                assert!(a.project.is_none());
            }
            other => panic!("expected Add, got {other:?}"),
        }
    }

    #[test]
    fn parse_add_empty_title_errors() {
        assert!(parse(&argv(&["add"])).is_err());
    }

    #[test]
    fn parse_add_unknown_flag_errors() {
        let err = parse(&argv(&["add", "x", "--bogus"])).unwrap_err();
        assert!(
            err.contains("--bogus") || err.contains("bogus"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_add_with_flags() {
        let r = parse(&argv(&[
            "add",
            "Title",
            "--note",
            "Body",
            "--project",
            "Work",
            "--tag",
            "urgent",
            "--tag",
            "todo",
        ]))
        .unwrap();
        match r.subcommand {
            Some(Subcommand::Add(a)) => {
                assert_eq!(a.title, "Title");
                assert_eq!(a.note.as_deref(), Some("Body"));
                assert_eq!(a.project.as_deref(), Some("Work"));
                assert_eq!(a.tags, vec!["urgent", "todo"]);
            }
            other => panic!("expected Add, got {other:?}"),
        }
    }

    #[test]
    fn parse_add_with_deadline_warn() {
        let r = parse(&argv(&["add", "x", "--deadline-warn", "14"])).unwrap();
        match r.subcommand {
            Some(Subcommand::Add(a)) => assert_eq!(a.deadline_warn, Some(14)),
            other => panic!("expected Add, got {other:?}"),
        }
    }

    #[test]
    fn parse_add_with_warn_alias() {
        let r = parse(&argv(&["add", "x", "--warn", "3"])).unwrap();
        match r.subcommand {
            Some(Subcommand::Add(a)) => assert_eq!(a.deadline_warn, Some(3)),
            other => panic!("expected Add, got {other:?}"),
        }
    }

    #[test]
    fn parse_add_warn_negative_errors() {
        assert!(parse(&argv(&["add", "x", "--warn", "-1"])).is_err());
    }

    #[test]
    fn parse_add_with_parent() {
        let r = parse(&argv(&["add", "Child", "--parent", "42"])).unwrap();
        match r.subcommand {
            Some(Subcommand::Add(a)) => assert_eq!(a.parent, Some(42)),
            other => panic!("expected Add, got {other:?}"),
        }
    }

    #[test]
    fn parse_add_parent_non_integer_errors() {
        assert!(parse(&argv(&["add", "x", "--parent", "abc"])).is_err());
    }

    // ── edit subcommand ───────────────────────────────────────────

    #[test]
    fn parse_edit_invalid_id_errors() {
        assert!(parse(&argv(&["edit", "not-an-id"])).is_err());
    }

    #[test]
    fn parse_edit_with_parent_id() {
        let r = parse(&argv(&["edit", "7", "--parent", "3"])).unwrap();
        match r.subcommand {
            Some(Subcommand::Edit { edit, .. }) => {
                assert_eq!(edit.parent, Some(EditParent::Task(3)));
            }
            other => panic!("expected Edit, got {other:?}"),
        }
    }

    #[test]
    fn parse_edit_parent_none_promotes() {
        for word in ["none", "top", "0"] {
            let r = parse(&argv(&["edit", "7", "--parent", word])).unwrap();
            match r.subcommand {
                Some(Subcommand::Edit { edit, .. }) => {
                    assert_eq!(edit.parent, Some(EditParent::TopLevel), "for {word}");
                }
                other => panic!("expected Edit, got {other:?}"),
            }
        }
    }

    // ── capture subcommand ────────────────────────────────────────

    #[test]
    fn parse_capture_with_text() {
        let r = parse(&argv(&["capture", "Buy", "milk", "#errand"])).unwrap();
        match r.subcommand {
            Some(Subcommand::Capture { line }) => {
                assert_eq!(line, "Buy milk #errand");
            }
            other => panic!("expected Capture, got {other:?}"),
        }
    }

    #[test]
    fn parse_capture_empty_errors() {
        assert!(parse(&argv(&["capture"])).is_err());
    }

    // ── clock subcommand ──────────────────────────────────────────

    #[test]
    fn parse_clock_bare_status() {
        let r = parse(&argv(&["clock"])).unwrap();
        assert!(matches!(
            r.subcommand,
            Some(Subcommand::Clock(ClockSub::Status))
        ));
    }

    #[test]
    fn parse_clock_status_explicit() {
        let r = parse(&argv(&["clock", "status"])).unwrap();
        assert!(matches!(
            r.subcommand,
            Some(Subcommand::Clock(ClockSub::Status))
        ));
    }

    #[test]
    fn parse_clock_in_with_id() {
        let r = parse(&argv(&["clock", "in", "42"])).unwrap();
        match r.subcommand {
            Some(Subcommand::Clock(ClockSub::In { task_id, .. })) => assert_eq!(task_id, 42),
            other => panic!("expected Clock In, got {other:?}"),
        }
    }

    #[test]
    fn parse_clock_in_with_note() {
        let r = parse(&argv(&["clock", "in", "1", "--note", "deep work"])).unwrap();
        match r.subcommand {
            Some(Subcommand::Clock(ClockSub::In { note, .. })) => assert_eq!(note, "deep work"),
            other => panic!("expected Clock In, got {other:?}"),
        }
    }

    #[test]
    fn parse_clock_log_with_id() {
        let r = parse(&argv(&["clock", "log", "7"])).unwrap();
        match r.subcommand {
            Some(Subcommand::Clock(ClockSub::Log { task_id })) => assert_eq!(task_id, 7),
            other => panic!("expected Clock Log, got {other:?}"),
        }
    }

    // ── template subcommand ───────────────────────────────────────

    #[test]
    fn parse_template_list() {
        let r = parse(&argv(&["template", "list"])).unwrap();
        assert!(matches!(
            r.subcommand,
            Some(Subcommand::Template(TemplateSub::List))
        ));
    }

    #[test]
    fn parse_template_add_minimal() {
        let r = parse(&argv(&["template", "add", "Capture Errand"])).unwrap();
        match r.subcommand {
            Some(Subcommand::Template(TemplateSub::Add(t))) => {
                assert_eq!(t.name.as_deref(), Some("Capture Errand"));
            }
            other => panic!("expected Template Add, got {other:?}"),
        }
    }

    #[test]
    fn parse_template_add_with_shortcut() {
        let r = parse(&argv(&[
            "template",
            "add",
            "Errand",
            "--shortcut",
            "e",
            "--prefix",
            "Buy:",
        ]))
        .unwrap();
        match r.subcommand {
            Some(Subcommand::Template(TemplateSub::Add(t))) => {
                assert_eq!(t.shortcut.as_deref(), Some("e"));
                assert_eq!(t.prefix.as_deref(), Some("Buy:"));
            }
            other => panic!("expected Template Add, got {other:?}"),
        }
    }

    // ── unknown subcommand ────────────────────────────────────────

    #[test]
    fn parse_unknown_subcommand_errors() {
        let err = parse(&argv(&["bogus"])).unwrap_err();
        assert!(!err.is_empty());
    }
}
