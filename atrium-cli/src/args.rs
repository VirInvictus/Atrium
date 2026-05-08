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
                        --tag NAME          attach a tag (repeatable;
                                            tag is created if missing)
                        --scheduled DATE    YYYY-MM-DD, today,
                                            tomorrow, or `someday`
                        --due DATE          YYYY-MM-DD, today, tomorrow
                        --defer DATE        YYYY-MM-DD, today, tomorrow
                        --estimated MINUTES integer minutes
    complete ID       toggle a task's completion (same as the GTK
                      checkbox; calling twice un-completes).
    delete ID         delete a task. Prints the row before deletion
                      so the deletion is auditable in pipelines.

EXAMPLES:
    atrium-cli list today
    atrium-cli search 'tag:work AND is:overdue sort:-due'
    atrium-cli --json search 'is:repeating' | jq '.[] | .title'
    atrium-cli info 42 --human
    atrium-cli add 'Buy milk' --tag errand --due tomorrow
    atrium-cli add 'Q3 retrospective notes' --project 'Q3 plans' --scheduled today
    atrium-cli complete 42
    atrium-cli list tags --json | jq '.[] | .name'
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
    Search { expression: String },
    List { name: String },
    Info { id: i64 },
    Add(AddArgs),
    Complete { id: i64 },
    Delete { id: i64 },
}

/// Fields populated from the `add` subcommand's flag soup. Resolved
/// to a NewTask + project lookup + tag attachments at command time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AddArgs {
    pub title: String,
    pub note: Option<String>,
    pub project: Option<String>,
    pub tags: Vec<String>,
    /// Raw text — `today`, `tomorrow`, `someday`, or `YYYY-MM-DD`.
    /// Resolved against `Local::now()` when the command runs.
    pub scheduled: Option<String>,
    pub due: Option<String>,
    pub defer: Option<String>,
    pub estimated_minutes: Option<i64>,
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
        "complete" | "done" | "toggle" => {
            let id_str = raw.get(i).ok_or("complete requires a task id")?;
            i += 1;
            let id: i64 = id_str
                .parse()
                .map_err(|_| format!("invalid task id: {id_str}"))?;
            apply_trailing_flags(&raw[i..], &mut args)?;
            Subcommand::Complete { id }
        }
        "delete" | "rm" => {
            let id_str = raw.get(i).ok_or("delete requires a task id")?;
            i += 1;
            let id: i64 = id_str
                .parse()
                .map_err(|_| format!("invalid task id: {id_str}"))?;
            apply_trailing_flags(&raw[i..], &mut args)?;
            Subcommand::Delete { id }
        }
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
