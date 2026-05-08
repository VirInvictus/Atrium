// SPDX-License-Identifier: MIT
//! Atrium CLI — headless access to the search engine + data layer.
//!
//! The data layer (`atrium-core`) and search engine (`atrium-search`)
//! are GUI-free by design. This binary makes them exercisable from
//! the shell so every feature aside from the GTK rendering itself is
//! testable, scriptable, grep-able, and ready to be reused by the
//! 2.0-era TUI / atriumd capture daemon.
//!
//! ## Usage
//!
//! ```text
//! atrium-cli [GLOBAL FLAGS] <SUBCOMMAND> [ARGS]
//!
//! Global flags:
//!   --db PATH        override the database path
//!                    (default: $XDG_DATA_HOME/atrium/atrium.db,
//!                     or ATRIUM_DB_PATH env if set)
//!   --json           output as JSON (one object per task)
//!   --tsv            output as TSV (default; columns: id, status,
//!                    title, scheduled, deadline, tags)
//!   --human          pretty-printed columns
//!   -h, --help       print this message and exit
//!   -V, --version    print version and exit
//!
//! Subcommands:
//!   search EXPR      run an Atrium search expression and print matches.
//!                    EXPR follows spec.md §4.3 (e.g. `tag:work AND is:overdue`).
//!   list NAME        print a canonical list. NAME ∈ inbox | today |
//!                    upcoming | anytime | someday | logbook | all.
//!   info ID          print full details of a single task.
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use atrium_core::db::read;
use atrium_core::domain::{ScheduledFor, Task};
use atrium_search::{EvalContext, evaluate};
use chrono::{Local, NaiveDate};
use rusqlite::{Connection, OpenFlags};

mod args;
mod output;

#[cfg(test)]
mod tests;

use args::{Format, Subcommand};
use output::{Row, format_row, format_rows, format_task_detail};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match args::parse(&raw) {
        Ok(args) => run(args),
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!();
            eprintln!("{}", args::USAGE);
            ExitCode::from(2)
        }
    }
}

fn run(args: args::Args) -> ExitCode {
    if args.show_help {
        println!("{}", args::USAGE);
        return ExitCode::SUCCESS;
    }
    if args.show_version {
        println!("atrium-cli {VERSION}");
        return ExitCode::SUCCESS;
    }
    let Some(sub) = args.subcommand else {
        eprintln!("error: no subcommand specified");
        eprintln!();
        eprintln!("{}", args::USAGE);
        return ExitCode::from(2);
    };

    let db_path = resolve_db_path(args.db_path.as_ref());
    let conn = match open_db(&db_path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("error: opening database at {}: {err}", db_path.display());
            return ExitCode::from(1);
        }
    };

    match sub {
        Subcommand::Search { expression } => {
            run_search(&conn, &expression, args.format).unwrap_or_exit_code()
        }
        Subcommand::List { name } => run_list(&conn, &name, args.format).unwrap_or_exit_code(),
        Subcommand::Info { id } => run_info(&conn, id, args.format).unwrap_or_exit_code(),
    }
}

/// Resolve the database path: CLI flag → `ATRIUM_DB_PATH` env →
/// XDG default. The CLI flag takes precedence so test scripts can
/// point at a fixture without polluting the user's environment.
fn resolve_db_path(override_path: Option<&PathBuf>) -> PathBuf {
    if let Some(p) = override_path {
        return p.clone();
    }
    if let Ok(env) = std::env::var("ATRIUM_DB_PATH")
        && !env.is_empty()
    {
        return PathBuf::from(env);
    }
    atrium_core::paths::db_path()
}

/// Open the database read-only. atrium-cli never writes; using
/// `OpenFlags::SQLITE_OPEN_READ_ONLY` makes that a process-level
/// guarantee — even a buggy query attempting an INSERT errors at
/// the engine, so we can never corrupt a user's database from a
/// CLI invocation.
fn open_db(path: &PathBuf) -> rusqlite::Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    Connection::open_with_flags(path, flags)
}

fn run_search(conn: &Connection, expression: &str, format: Format) -> CliResult<()> {
    let parsed =
        atrium_search::parse(expression).map_err(|e| CliError::Search(format!("{e:?}")))?;
    if !parsed.warnings.is_empty() {
        for w in &parsed.warnings {
            eprintln!("warning: unrecognised token: {w}");
        }
    }
    let today = Local::now().date_naive();
    let ctx_data = ContextData::load(conn)?;
    let ctx = ctx_data.eval_context(today);
    let mut tasks = read::list_all_tasks(conn).map_err(CliError::from)?;
    tasks.retain(|t| evaluate(&parsed.expr, t, &ctx));
    if !parsed.sorts.is_empty() {
        sort_tasks(&mut tasks, &parsed.sorts, &ctx_data);
    }
    print_tasks(&tasks, &ctx_data, format);
    Ok(())
}

fn run_list(conn: &Connection, name: &str, format: Format) -> CliResult<()> {
    let today = Local::now().date_naive();
    let tasks = match name {
        "inbox" => read::list_inbox(conn),
        "today" => read::list_today(conn, today),
        "upcoming" => read::list_upcoming(conn, today),
        "anytime" => read::list_anytime(conn, today),
        "someday" => read::list_someday(conn),
        "logbook" => read::list_logbook(conn),
        "all" => read::list_all_tasks(conn),
        other => return Err(CliError::Args(format!("unknown list: {other}"))),
    }
    .map_err(CliError::from)?;
    let ctx_data = ContextData::load(conn)?;
    print_tasks(&tasks, &ctx_data, format);
    Ok(())
}

fn run_info(conn: &Connection, id: i64, format: Format) -> CliResult<()> {
    let Some(task) = read::task_by_id(conn, id).map_err(CliError::from)? else {
        return Err(CliError::NotFound(id));
    };
    let ctx_data = ContextData::load(conn)?;
    let row = build_row(&task, &ctx_data);
    match format {
        Format::Json => {
            // Same JSON shape as the row-list — one object — so
            // downstream `jq` paths work uniformly.
            let s = output::row_to_json(&row);
            println!("{s}");
        }
        Format::Tsv => {
            // Single TSV record, no header; matches the search /
            // list output shape so consumers can grep symmetrically.
            println!("{}", format_row(&row));
        }
        Format::Human => {
            println!("{}", format_task_detail(&task, &row));
        }
    }
    Ok(())
}

fn print_tasks(tasks: &[Task], ctx: &ContextData, format: Format) {
    let rows: Vec<Row> = tasks.iter().map(|t| build_row(t, ctx)).collect();
    match format {
        Format::Json => {
            let s = output::rows_to_json(&rows);
            println!("{s}");
        }
        Format::Tsv => {
            print!("{}", format_rows(&rows));
        }
        Format::Human => {
            print!("{}", output::format_rows_human(&rows));
        }
    }
}

fn build_row(task: &Task, ctx: &ContextData) -> Row {
    let tags = ctx
        .tag_names
        .get(&task.id)
        .cloned()
        .unwrap_or_default()
        .join(",");
    let project = task
        .project_id
        .and_then(|pid| ctx.project_titles.get(&pid).cloned())
        .unwrap_or_default();
    let area = task
        .project_id
        .and_then(|pid| ctx.project_areas.get(&pid).copied().flatten())
        .and_then(|aid| ctx.area_titles.get(&aid).cloned())
        .unwrap_or_default();
    Row {
        id: task.id,
        status: status_glyph(task),
        title: task.title.clone(),
        scheduled: scheduled_iso(&task.scheduled_for),
        deadline: task.deadline.map(|d| d.to_string()).unwrap_or_default(),
        tags,
        project,
        area,
    }
}

fn status_glyph(task: &Task) -> String {
    if task.completed_at.is_some() {
        "done".into()
    } else if let Some(deadline) = task.deadline
        && deadline < Local::now().date_naive()
    {
        "overdue".into()
    } else {
        "open".into()
    }
}

fn scheduled_iso(s: &Option<ScheduledFor>) -> String {
    match s {
        None => String::new(),
        Some(ScheduledFor::Someday) => "someday".into(),
        Some(ScheduledFor::Date(d)) => d.to_string(),
    }
}

/// Sort `tasks` in-place by the parsed sort modifiers. The CLI
/// reuses atrium-search's filter semantics for predicates, but the
/// sort path lives here because it needs `ContextData` for things
/// like project / area title comparisons (future use).
fn sort_tasks(tasks: &mut [Task], sorts: &[atrium_search::SortSpec], _ctx: &ContextData) {
    use atrium_search::SortKey;
    use std::cmp::Ordering;
    tasks.sort_by(|a, b| {
        for spec in sorts {
            let ord = match spec.key {
                SortKey::Due => cmp_opt(a.deadline, b.deadline, spec.direction),
                SortKey::Scheduled => cmp_opt(
                    scheduled_date(&a.scheduled_for),
                    scheduled_date(&b.scheduled_for),
                    spec.direction,
                ),
                SortKey::Defer => cmp_opt(a.defer_until, b.defer_until, spec.direction),
                SortKey::Created => cmp_dir(a.created_at, b.created_at, spec.direction),
                SortKey::Modified => cmp_dir(a.modified_at, b.modified_at, spec.direction),
                SortKey::Completed => cmp_opt(a.completed_at, b.completed_at, spec.direction),
                SortKey::Estimated => {
                    cmp_opt(a.estimated_minutes, b.estimated_minutes, spec.direction)
                }
                SortKey::Title => cmp_dir(a.title.as_str(), b.title.as_str(), spec.direction),
                SortKey::Position => match a.position.partial_cmp(&b.position) {
                    Some(o) => apply_dir(o, spec.direction),
                    None => Ordering::Equal,
                },
            };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        Ordering::Equal
    });
}

fn scheduled_date(s: &Option<ScheduledFor>) -> Option<NaiveDate> {
    match s {
        Some(ScheduledFor::Date(d)) => Some(*d),
        _ => None,
    }
}

fn cmp_opt<T: Ord>(
    a: Option<T>,
    b: Option<T>,
    dir: atrium_search::SortDirection,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(av), Some(bv)) => apply_dir(av.cmp(&bv), dir),
    }
}

fn cmp_dir<T: Ord>(a: T, b: T, dir: atrium_search::SortDirection) -> std::cmp::Ordering {
    apply_dir(a.cmp(&b), dir)
}

fn apply_dir(ord: std::cmp::Ordering, dir: atrium_search::SortDirection) -> std::cmp::Ordering {
    match dir {
        atrium_search::SortDirection::Asc => ord,
        atrium_search::SortDirection::Desc => ord.reverse(),
    }
}

/// Cached database read produced once per CLI invocation. The
/// EvalContext takes references into this struct.
struct ContextData {
    tag_names: HashMap<i64, Vec<String>>,
    project_titles: HashMap<i64, String>,
    project_areas: HashMap<i64, Option<i64>>,
    area_titles: HashMap<i64, String>,
}

impl ContextData {
    fn load(conn: &Connection) -> CliResult<Self> {
        let tag_names = read::tag_names_per_task(conn).map_err(CliError::from)?;
        let projects = read::list_projects(conn).map_err(CliError::from)?;
        let areas = read::list_areas(conn).map_err(CliError::from)?;
        let project_titles: HashMap<i64, String> =
            projects.iter().map(|p| (p.id, p.title.clone())).collect();
        let project_areas: HashMap<i64, Option<i64>> =
            projects.iter().map(|p| (p.id, p.area_id)).collect();
        let area_titles: HashMap<i64, String> =
            areas.iter().map(|a| (a.id, a.title.clone())).collect();
        Ok(Self {
            tag_names,
            project_titles,
            project_areas,
            area_titles,
        })
    }

    fn eval_context(&self, today: NaiveDate) -> EvalContext<'_> {
        EvalContext::new(
            today,
            &self.tag_names,
            &self.project_titles,
            &self.project_areas,
            &self.area_titles,
        )
    }
}

// ── error / result plumbing ────────────────────────────────────────

#[derive(Debug)]
enum CliError {
    Args(String),
    Search(String),
    Db(atrium_core::DbError),
    NotFound(i64),
}

impl From<atrium_core::DbError> for CliError {
    fn from(e: atrium_core::DbError) -> Self {
        Self::Db(e)
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Args(m) => write!(f, "{m}"),
            CliError::Search(m) => write!(f, "search expression: {m}"),
            CliError::Db(e) => write!(f, "database: {e}"),
            CliError::NotFound(id) => write!(f, "task {id} not found"),
        }
    }
}

type CliResult<T> = Result<T, CliError>;

trait UnwrapOrExitCode {
    fn unwrap_or_exit_code(self) -> ExitCode;
}

impl UnwrapOrExitCode for CliResult<()> {
    fn unwrap_or_exit_code(self) -> ExitCode {
        match self {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(1)
            }
        }
    }
}
