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
use atrium_core::domain::{NewTask, ScheduledFor, Task, TaskUpdate};
use atrium_search::{EvalContext, evaluate};
use chrono::{Local, NaiveDate};
use rusqlite::{Connection, OpenFlags};

mod args;
mod output;

#[cfg(test)]
mod tests;

use args::{AddArgs, EditArgs, EditProject, Format, Subcommand, TargetSpec};
use output::{Row, format_row, format_rows, format_task_detail};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    // Reset SIGPIPE to the OS default so a closed stdout (the pipe
    // into `head`, `head -c 1`, etc.) terminates us cleanly instead
    // of panicking on the next println. Rust installs SIG_IGN at
    // startup, which is why the default behaviour is the panic.
    // Done inline (without the libc crate) to keep our dependency
    // surface small — the symbol is part of every Unix libc.
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn signal(signum: i32, handler: usize) -> usize;
        }
        const SIGPIPE: i32 = 13;
        const SIG_DFL: usize = 0;
        // SAFETY: signal() is async-signal-safe and SIG_DFL is the
        // canonical default-handler sentinel. We set it once at
        // startup, before any other thread or signal can race.
        unsafe {
            signal(SIGPIPE, SIG_DFL);
        }
    }

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
        Subcommand::Add(add) => with_writer(&db_path, |rt, handle, conn| {
            run_add(rt, handle, conn, add, args.format)
        }),
        Subcommand::Capture { line } => with_writer(&db_path, |rt, handle, conn| {
            run_capture(rt, handle, conn, &line, args.format)
        }),
        Subcommand::Edit { id, edit } => with_writer(&db_path, |rt, handle, conn| {
            run_edit(rt, handle, conn, id, edit, args.format)
        }),
        Subcommand::Complete { target } => with_writer(&db_path, |rt, handle, conn| {
            run_complete(rt, handle, conn, target, args.format)
        }),
        Subcommand::Delete { target, force } => with_writer(&db_path, |rt, handle, conn| {
            run_delete(rt, handle, conn, target, force, args.format)
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
/// closure with the Runtime + WorkerHandle + a read connection.
///
/// We hand `f` the `Runtime` rather than entering `runtime.block_on`
/// around it on purpose: inside an outer `block_on`, a second
/// `Runtime::block_on` (or `Handle::block_on`) panics with "Cannot
/// start a runtime from within a runtime." Keeping `f` *outside*
/// the runtime lets it drive each async call individually via
/// `runtime.block_on(handle.foo().await)`. The worker stays alive
/// because the runtime owns the spawn — it just isn't actively
/// running until the next `block_on`.
fn with_writer<F>(path: &Path, f: F) -> ExitCode
where
    F: FnOnce(&tokio::runtime::Runtime, &atrium_core::WorkerHandle, &Connection) -> CliResult<()>,
{
    let conn = match atrium_core::db::open(path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("error: opening database at {}: {err}", path.display());
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
    // Spawn the worker inside the runtime, then exit block_on so
    // the closure runs in non-async context. The worker future is
    // alive on the runtime; subsequent `runtime.block_on(...)` calls
    // from `f` drive it forward to handle each command.
    let (handle, _changes_rx, _library_rx) =
        runtime.block_on(async move { atrium_core::spawn_worker(conn) });
    // Read-only connection for post-write reads (status display).
    let read_conn = match open_db_readonly(path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("error: opening read-only connection: {err}");
            return ExitCode::from(1);
        }
    };
    f(&runtime, &handle, &read_conn).unwrap_or_exit_code()
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
    runtime: &tokio::runtime::Runtime,
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

/// `capture` — single-string Quick Entry equivalent. Parses the
/// line through atrium_core::quick_entry (the same parser the GUI's
/// Quick Entry modal and bottom-of-list entry use) and creates a
/// task with the resolved title / tags / scheduled / deadline.
/// Drops to Inbox (no project) — matching the GUI's Quick Entry
/// behaviour per spec §6.
fn run_capture(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    line: &str,
    format: Format,
) -> CliResult<()> {
    let parsed = atrium_core::quick_entry::parse(line);
    if parsed.title.trim().is_empty() && parsed.tag_names.is_empty() {
        return Err(CliError::Args(
            "capture line is empty after parsing inline syntax".into(),
        ));
    }
    let new = NewTask {
        title: parsed.title.clone(),
        scheduled_for: parsed.scheduled_for,
        deadline: parsed.deadline,
        ..Default::default()
    };
    let task = runtime
        .block_on(async { handle.create_task(new).await })
        .map_err(CliError::from)?;
    if !parsed.tag_names.is_empty() {
        let mut tag_ids: Vec<i64> = Vec::with_capacity(parsed.tag_names.len());
        for name in &parsed.tag_names {
            let tag = runtime
                .block_on(async { handle.ensure_tag(name.clone()).await })
                .map_err(CliError::from)?;
            tag_ids.push(tag.id);
        }
        let _ = runtime
            .block_on(async { handle.set_task_tags(task.id, tag_ids).await })
            .map_err(CliError::from)?;
    }
    let task = read::task_by_id(read_conn, task.id)
        .map_err(CliError::from)?
        .ok_or(CliError::NotFound(task.id))?;
    let ctx_data = ContextData::load(read_conn)?;
    let row = build_row(&task, &ctx_data);
    print_single_row(&task, &row, format);
    Ok(())
}

/// `edit ID [FLAGS]` — diff-based modify. Each EditArgs field that's
/// `Some` becomes a TaskUpdate setter; the magic value `"none"`
/// clears a nullable field. `--inbox` (or `--project inbox`) maps
/// to project_id = NULL.
fn run_edit(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    id: i64,
    edit: EditArgs,
    format: Format,
) -> CliResult<()> {
    // Verify the task exists upfront — gives a cleaner error than
    // an opaque worker NotFound.
    let existing = read::task_by_id(read_conn, id)
        .map_err(CliError::from)?
        .ok_or(CliError::NotFound(id))?;

    let today = Local::now().date_naive();
    let mut update = TaskUpdate::new(id);

    if let Some(t) = edit.title.clone() {
        update = update.title(t);
    }
    if let Some(n) = edit.note.clone() {
        update = update.note(n);
    }
    if let Some(p) = edit.project.clone() {
        match p {
            EditProject::Inbox => update = update.project(None),
            EditProject::Named(needle) => {
                let pid = resolve_project_by_name(read_conn, &needle)?;
                update = update.project(Some(pid));
            }
        }
    }
    if let Some(s) = edit.scheduled.as_deref() {
        if s.eq_ignore_ascii_case("none") {
            update = update.schedule(None);
        } else {
            update = update.schedule(Some(parse_scheduled(s, today)?));
        }
    }
    if let Some(s) = edit.due.as_deref() {
        if s.eq_ignore_ascii_case("none") {
            update = update.deadline_value(None);
        } else {
            update = update.deadline_value(Some(parse_date(s, today)?));
        }
    }
    if let Some(s) = edit.defer.as_deref() {
        if s.eq_ignore_ascii_case("none") {
            update = update.defer_value(None);
        } else {
            update = update.defer_value(Some(parse_date(s, today)?));
        }
    }
    if let Some(s) = edit.estimated.as_deref() {
        if s.eq_ignore_ascii_case("none") {
            update = update.estimated_minutes_value(None);
        } else {
            // Already validated at parse time, but defensive.
            let n: i64 = s
                .parse()
                .map_err(|_| CliError::Args(format!("--estimated: not an integer: {s}")))?;
            update = update.estimated_minutes_value(Some(n));
        }
    }

    // Run the field-update first; tag diff is applied separately
    // because tags route through ensure_tag + set_task_tags rather
    // than TaskUpdate's column-level setters.
    let task = if update.is_noop() {
        existing
    } else {
        runtime
            .block_on(async { handle.update_task(update).await })
            .map_err(CliError::from)?
    };

    if edit.touches_tags() {
        apply_tag_diff(handle, read_conn, task.id, &edit, runtime)?;
    }

    // Re-read so the row reflects post-tag state, plus a fresh
    // ContextData — tags landed since the open.
    let task = read::task_by_id(read_conn, task.id)
        .map_err(CliError::from)?
        .ok_or(CliError::NotFound(task.id))?;
    let ctx_data = ContextData::load(read_conn)?;
    let row = build_row(&task, &ctx_data);
    print_single_row(&task, &row, format);
    Ok(())
}

/// Apply the user's tag-edit intent against the task's current
/// tag set. Resolves the diff in name-space (clear / remove / add),
/// then ensure_tag for the final names and set_task_tags for the
/// resulting id list. Quietly no-ops on remove-of-not-present.
fn apply_tag_diff(
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    task_id: i64,
    edit: &EditArgs,
    runtime: &tokio::runtime::Runtime,
) -> CliResult<()> {
    let current_names: Vec<String> = if edit.clear_tags {
        Vec::new()
    } else {
        // Pull the current tag set for this task. Going through
        // tag_names_per_task rather than per-task fetch keeps the
        // round-trip count fixed (one query for any task count).
        let map = read::tag_names_per_task(read_conn).map_err(CliError::from)?;
        map.get(&task_id).cloned().unwrap_or_default()
    };

    let mut final_names: Vec<String> = current_names;
    // Remove first so a remove+add of the same tag is a no-op
    // rather than dropping it from the final set.
    let remove_lower: std::collections::HashSet<String> = edit
        .tags_remove
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    final_names.retain(|n| !remove_lower.contains(&n.to_ascii_lowercase()));
    for name in &edit.tags_add {
        let lower = name.to_ascii_lowercase();
        if !final_names.iter().any(|n| n.to_ascii_lowercase() == lower) {
            final_names.push(name.clone());
        }
    }

    let mut ids: Vec<i64> = Vec::with_capacity(final_names.len());
    for name in &final_names {
        let tag = runtime
            .block_on(async { handle.ensure_tag(name.clone()).await })
            .map_err(CliError::from)?;
        ids.push(tag.id);
    }
    runtime
        .block_on(async { handle.set_task_tags(task_id, ids).await })
        .map_err(CliError::from)?;
    Ok(())
}

fn run_complete(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    target: TargetSpec,
    format: Format,
) -> CliResult<()> {
    match target {
        TargetSpec::Id(id) => run_complete_one(runtime, handle, read_conn, id, format),
        TargetSpec::Where(expr) => run_complete_bulk(runtime, handle, read_conn, &expr, format),
    }
}

fn run_complete_one(
    runtime: &tokio::runtime::Runtime,
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
    let task = runtime
        .block_on(async { handle.toggle_complete(id).await })
        .map_err(CliError::from)?;
    let ctx_data = ContextData::load(read_conn)?;
    let row = build_row(&task, &ctx_data);
    print_single_row(&task, &row, format);
    Ok(())
}

/// `complete --where EXPR` — toggle each task matching the search
/// expression. Same semantics as multi-select bulk-complete in the
/// GUI: each task is toggled independently, so a mix of open/done
/// rows would invert each. For "mark these done" specifically,
/// users compose `is:open AND ...` into the where clause.
fn run_complete_bulk(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    expr: &str,
    format: Format,
) -> CliResult<()> {
    let matched = resolve_matching_tasks(read_conn, expr)?;
    if matched.is_empty() {
        eprintln!("no tasks matched the expression");
        return Ok(());
    }
    let mut after: Vec<atrium_core::Task> = Vec::with_capacity(matched.len());
    for task in &matched {
        let toggled = runtime
            .block_on(async { handle.toggle_complete(task.id).await })
            .map_err(CliError::from)?;
        after.push(toggled);
    }
    let ctx_data = ContextData::load(read_conn)?;
    print_tasks(&after, &ctx_data, format);
    Ok(())
}

fn run_delete(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    target: TargetSpec,
    force: bool,
    format: Format,
) -> CliResult<()> {
    match target {
        TargetSpec::Id(id) => run_delete_one(runtime, handle, read_conn, id, format),
        TargetSpec::Where(expr) => {
            run_delete_bulk(runtime, handle, read_conn, &expr, force, format)
        }
    }
}

fn run_delete_one(
    runtime: &tokio::runtime::Runtime,
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
    runtime
        .block_on(async { handle.delete_task(id).await })
        .map_err(CliError::from)?;
    print_single_row(&task, &row, format);
    Ok(())
}

/// `delete --where EXPR [--force]` — destructive bulk. Without
/// `--force` we run in dry-run mode: print the rows that *would*
/// be deleted and exit with status 2 so a calling script can
/// review the output and re-run with `--force` to commit.
fn run_delete_bulk(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    expr: &str,
    force: bool,
    format: Format,
) -> CliResult<()> {
    let matched = resolve_matching_tasks(read_conn, expr)?;
    if matched.is_empty() {
        eprintln!("no tasks matched the expression");
        return Ok(());
    }
    let ctx_data = ContextData::load(read_conn)?;
    if !force {
        eprintln!(
            "would delete {} task(s); pass --force to actually delete:",
            matched.len()
        );
        print_tasks(&matched, &ctx_data, format);
        return Err(CliError::DryRun(matched.len()));
    }
    for task in &matched {
        runtime
            .block_on(async { handle.delete_task(task.id).await })
            .map_err(CliError::from)?;
    }
    print_tasks(&matched, &ctx_data, format);
    Ok(())
}

/// Run a search expression against the full task set and return
/// the matches. Shared by complete --where and delete --where.
fn resolve_matching_tasks(read_conn: &Connection, expr: &str) -> CliResult<Vec<Task>> {
    let parsed = atrium_search::parse(expr).map_err(|e| CliError::Search(format!("{e:?}")))?;
    if !parsed.warnings.is_empty() {
        for w in &parsed.warnings {
            eprintln!("warning: unrecognised token: {w}");
        }
    }
    let today = Local::now().date_naive();
    let ctx_data = ContextData::load(read_conn)?;
    let ctx = ctx_data.eval_context(today);
    let mut tasks = read::list_all_tasks(read_conn).map_err(CliError::from)?;
    tasks.retain(|t| atrium_search::evaluate(&parsed.expr, t, &ctx));
    Ok(tasks)
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
    /// `delete --where EXPR` ran without `--force`. Carries the
    /// match count so the message is concrete; UnwrapOrExitCode
    /// maps this to exit status 2 so a script can branch on it
    /// (vs status 1 for real failures).
    DryRun(usize),
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
            CliError::DryRun(n) => write!(f, "dry run: {n} task(s) would be deleted"),
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
            // DryRun is an explicit "no work was done because the
            // user didn't pass --force" — exit 2 so a script can
            // distinguish it from a real error (1) or success (0).
            // The diagnostic was already printed inline.
            Err(CliError::DryRun(_)) => ExitCode::from(2),
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(1)
            }
        }
    }
}
