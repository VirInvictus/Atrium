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
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use atrium_core::db::read;
use atrium_core::domain::{NewTask, ScheduledFor, Task};
use atrium_search::{EvalContext, evaluate};
use chrono::{Local, NaiveDate};
use rusqlite::{Connection, OpenFlags};

mod args;
mod output;

#[cfg(test)]
mod tests;

use args::{AddArgs, Format, Subcommand};
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

    // Write subcommands need a writable connection + worker; read
    // subcommands open read-only as a process-level safety guarantee.
    match sub {
        Subcommand::Search { expression } => {
            with_readonly(&db_path, |conn| run_search(conn, &expression, args.format))
        }
        Subcommand::List { name } => {
            with_readonly(&db_path, |conn| run_list(conn, &name, args.format))
        }
        Subcommand::Info { id } => with_readonly(&db_path, |conn| run_info(conn, id, args.format)),
        Subcommand::Add(add) => with_writer(&db_path, |handle, conn| {
            run_add(handle, conn, add, args.format)
        }),
        Subcommand::Complete { id } => with_writer(&db_path, |handle, conn| {
            run_complete(handle, conn, id, args.format)
        }),
        Subcommand::Delete { id } => with_writer(&db_path, |handle, conn| {
            run_delete(handle, conn, id, args.format)
        }),
    }
}

/// Open the database read-only and run the closure with a single
/// connection. Used for read commands that bypass the worker.
fn with_readonly<F>(path: &Path, f: F) -> ExitCode
where
    F: FnOnce(&Connection) -> CliResult<()>,
{
    let conn = match open_db_readonly(path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!(
                "error: opening database at {} (read-only): {err}",
                path.display()
            );
            return ExitCode::from(1);
        }
    };
    f(&conn).unwrap_or_exit_code()
}

/// Open the database read-write (running migrations as needed),
/// spawn the worker on a current-thread tokio runtime, and run the
/// closure with the WorkerHandle + a read connection. The runtime
/// shuts down when the handle drops on closure exit.
fn with_writer<F>(path: &Path, f: F) -> ExitCode
where
    F: FnOnce(&atrium_core::WorkerHandle, &Connection) -> CliResult<()>,
{
    let conn = match atrium_core::db::open(path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("error: opening database at {}: {err}", path.display());
            return ExitCode::from(1);
        }
    };
    // Read-only connection for context loading. The worker takes
    // the writable connection by value, so we snapshot the metadata
    // we need before handing it over — alternatively we'd open a
    // second connection, but the CLI is short-lived and one shared
    // path is enough.
    let ctx_data = match ContextData::load(&conn) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("error: loading context: {err}");
            return ExitCode::from(1);
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("error: starting tokio runtime: {err}");
            return ExitCode::from(1);
        }
    };
    let result = runtime.block_on(async move {
        let _enter = tokio::runtime::Handle::current();
        let (handle, _changes_rx, _library_rx) = atrium_core::spawn_worker(conn);
        // Read-only connection for post-write reads (status display).
        let read_conn = match open_db_readonly(path) {
            Ok(c) => c,
            Err(err) => {
                return Err(CliError::Args(format!("opening read connection: {err}")));
            }
        };
        // Apply the closure — drops handle at the end, which lets the worker shut down.
        let outcome = f(&handle, &read_conn);
        // Stash ctx_data for downstream uses (currently unused — kept
        // because run_add resolves project name → id via direct SQL).
        let _ = &ctx_data;
        outcome
    });
    result.unwrap_or_exit_code()
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

/// Open the database read-only. The read commands (search / list /
/// info) use this so a buggy query attempting an INSERT errors at
/// the engine — no CLI invocation can corrupt the user's database
/// through a read path.
fn open_db_readonly(path: &Path) -> rusqlite::Result<Connection> {
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
    // Metadata lists carry their own schemas — short-circuit before
    // the task-list match.
    match name {
        "areas" => return run_list_areas(conn, format),
        "projects" => return run_list_projects(conn, format),
        "tags" => return run_list_tags(conn, format),
        "perspectives" => return run_list_perspectives(conn, format),
        _ => {}
    }
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

fn run_list_areas(conn: &Connection, format: Format) -> CliResult<()> {
    let areas = read::list_areas(conn).map_err(CliError::from)?;
    output::print_areas(&areas, format);
    Ok(())
}

fn run_list_projects(conn: &Connection, format: Format) -> CliResult<()> {
    let projects = read::list_projects(conn).map_err(CliError::from)?;
    let area_titles: HashMap<i64, String> = read::list_areas(conn)
        .map_err(CliError::from)?
        .into_iter()
        .map(|a| (a.id, a.title))
        .collect();
    output::print_projects(&projects, &area_titles, format);
    Ok(())
}

fn run_list_tags(conn: &Connection, format: Format) -> CliResult<()> {
    let tags = read::list_tags(conn).map_err(CliError::from)?;
    output::print_tags(&tags, format);
    Ok(())
}

fn run_list_perspectives(conn: &Connection, format: Format) -> CliResult<()> {
    let perspectives = read::list_perspectives(conn).map_err(CliError::from)?;
    output::print_perspectives(&perspectives, format);
    Ok(())
}

// ── Write commands ──────────────────────────────────────────────

fn run_add(
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    add: AddArgs,
    format: Format,
) -> CliResult<()> {
    let today = Local::now().date_naive();
    // Resolve flags into NewTask + tag attachments.
    let project_id = match add.project.as_deref() {
        Some(p) => Some(resolve_project_by_name(read_conn, p)?),
        None => None,
    };
    let scheduled_for = match add.scheduled.as_deref() {
        Some(s) => Some(parse_scheduled(s, today)?),
        None => None,
    };
    let deadline = match add.due.as_deref() {
        Some(s) => Some(parse_date(s, today)?),
        None => None,
    };
    let defer_until = match add.defer.as_deref() {
        Some(s) => Some(parse_date(s, today)?),
        None => None,
    };
    let new = NewTask {
        title: add.title.clone(),
        note: add.note.unwrap_or_default(),
        project_id,
        parent_id: None,
        scheduled_for,
        deadline,
        defer_until,
        estimated_minutes: add.estimated_minutes,
        repeat_rule: None,
        repeat_mode: None,
    };

    let runtime = tokio::runtime::Handle::current();
    let task = runtime
        .block_on(async { handle.create_task(new).await })
        .map_err(CliError::from)?;

    // Tag attachments — ensure each, then set as the task's tag set.
    if !add.tags.is_empty() {
        let mut tag_ids: Vec<i64> = Vec::with_capacity(add.tags.len());
        for name in &add.tags {
            let tag = runtime
                .block_on(async { handle.ensure_tag(name.clone()).await })
                .map_err(CliError::from)?;
            tag_ids.push(tag.id);
        }
        let _ = runtime
            .block_on(async { handle.set_task_tags(task.id, tag_ids).await })
            .map_err(CliError::from)?;
    }

    // Re-read so the row reflects the post-tag state, plus a fresh
    // ContextData (tags landed since the open).
    let task = read::task_by_id(read_conn, task.id)
        .map_err(CliError::from)?
        .ok_or(CliError::NotFound(task.id))?;
    let ctx_data = ContextData::load(read_conn)?;
    let row = build_row(&task, &ctx_data);
    print_single_row(&task, &row, format);
    Ok(())
}

fn run_complete(
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    id: i64,
    format: Format,
) -> CliResult<()> {
    // Verify the id exists before sending the worker a toggle for
    // a missing row — gives a cleaner error than waiting for the
    // worker's NotFound.
    if read::task_by_id(read_conn, id)
        .map_err(CliError::from)?
        .is_none()
    {
        return Err(CliError::NotFound(id));
    }
    let runtime = tokio::runtime::Handle::current();
    let task = runtime
        .block_on(async { handle.toggle_complete(id).await })
        .map_err(CliError::from)?;
    let ctx_data = ContextData::load(read_conn)?;
    let row = build_row(&task, &ctx_data);
    print_single_row(&task, &row, format);
    Ok(())
}

fn run_delete(
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    id: i64,
    format: Format,
) -> CliResult<()> {
    // Snapshot the row before deletion so we can print exactly what
    // got removed — auditable in pipelines.
    let task = read::task_by_id(read_conn, id)
        .map_err(CliError::from)?
        .ok_or(CliError::NotFound(id))?;
    let ctx_data = ContextData::load(read_conn)?;
    let row = build_row(&task, &ctx_data);
    let runtime = tokio::runtime::Handle::current();
    runtime
        .block_on(async { handle.delete_task(id).await })
        .map_err(CliError::from)?;
    print_single_row(&task, &row, format);
    Ok(())
}

fn print_single_row(task: &Task, row: &Row, format: Format) {
    match format {
        Format::Json => println!("{}", output::row_to_json(row)),
        Format::Tsv => println!("{}", format_row(row)),
        Format::Human => println!("{}", format_task_detail(task, row)),
    }
}

/// Resolve a project name against the database. Accepts a unique
/// case-insensitive substring; ambiguous matches return an error
/// listing the candidates so the user can pick a more specific
/// fragment.
fn resolve_project_by_name(conn: &Connection, needle: &str) -> CliResult<i64> {
    let projects = read::list_projects(conn).map_err(CliError::from)?;
    let needle_lower = needle.to_ascii_lowercase();
    // Exact match wins.
    if let Some(p) = projects
        .iter()
        .find(|p| p.title.to_ascii_lowercase() == needle_lower)
    {
        return Ok(p.id);
    }
    let candidates: Vec<&atrium_core::Project> = projects
        .iter()
        .filter(|p| p.title.to_ascii_lowercase().contains(&needle_lower))
        .collect();
    match candidates.len() {
        0 => Err(CliError::Args(format!("no project matches \"{needle}\""))),
        1 => Ok(candidates[0].id),
        _ => {
            let titles: Vec<String> = candidates.iter().map(|p| p.title.clone()).collect();
            Err(CliError::Args(format!(
                "project \"{needle}\" is ambiguous; matches: {}",
                titles.join(", ")
            )))
        }
    }
}

/// Parse a date keyword or YYYY-MM-DD literal into a NaiveDate.
/// `today`, `yesterday`, `tomorrow` resolve against `today`. The
/// CLI doesn't accept `someday` here — that's only meaningful for
/// scheduled_for (which has its own enum branch).
fn parse_date(s: &str, today: NaiveDate) -> CliResult<NaiveDate> {
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "today" => Ok(today),
        "yesterday" => Ok(today - chrono::Duration::days(1)),
        "tomorrow" => Ok(today + chrono::Duration::days(1)),
        _ => NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
            CliError::Args(format!(
                "invalid date: {s} (expected YYYY-MM-DD or today/tomorrow/yesterday)"
            ))
        }),
    }
}

/// Parse a scheduled-for value: `someday` or anything `parse_date`
/// accepts. `someday` becomes the sentinel; everything else maps to
/// a Date variant.
fn parse_scheduled(s: &str, today: NaiveDate) -> CliResult<ScheduledFor> {
    if s.eq_ignore_ascii_case("someday") {
        return Ok(ScheduledFor::Someday);
    }
    Ok(ScheduledFor::Date(parse_date(s, today)?))
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
