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
mod clock;
mod import;
mod output;
mod template;

#[cfg(test)]
mod tests;

use args::{
    AddArgs, ClockSub, EditArgs, EditIcon, EditParent, EditProject, ExportSource, Format,
    ImportSource, PerspectiveArgs, PerspectiveSub, Subcommand, TargetSpec, TemplateSub,
    VaultSequencesOp,
};
use clock::{run_clock_in, run_clock_log, run_clock_out, run_clock_status};
use output::{Row, format_row, format_rows, format_task_detail};
use template::{run_template_add, run_template_edit, run_template_list, run_template_remove};

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
        Subcommand::Kanban { name } => {
            with_readonly(&db_path, |conn| run_kanban(conn, &name, args.format))
        }
        Subcommand::Perspective(sub) => with_writer(&db_path, |rt, handle, conn| {
            run_perspective(rt, handle, conn, sub, args.format)
        }),
        Subcommand::Import {
            source,
            path,
            dry_run,
        } => with_writer(&db_path, |rt, handle, _conn| {
            run_import(rt, handle, source, &path, dry_run, args.format)
        }),
        Subcommand::Export {
            source,
            path,
            dry_run,
        } => with_readonly(&db_path, |conn| {
            run_export(conn, source, &path, dry_run, args.format)
        }),
        Subcommand::VaultSequences { op, vault } => {
            // No DB needed — sidecar lives on disk and the
            // sub-subcommand operates on the file directly. Skip
            // both the readonly-open and writer-spawn paths.
            run_vault_sequences(&vault, op, args.format).unwrap_or_exit_code()
        }
        Subcommand::Clock(sub) => match sub {
            // Status / Log are read-only; In / Out + DeleteEntry
            // need the writer. Branch up front.
            ClockSub::Status => with_readonly(&db_path, |conn| run_clock_status(conn, args.format)),
            ClockSub::Log { task_id } => {
                with_readonly(&db_path, |conn| run_clock_log(conn, task_id, args.format))
            }
            ClockSub::In { task_id, note } => with_writer(&db_path, |rt, handle, _conn| {
                run_clock_in(rt, handle, task_id, note, args.format)
            }),
            ClockSub::Out { task_id } => with_writer(&db_path, |rt, handle, _conn| {
                run_clock_out(rt, handle, task_id, args.format)
            }),
        },
        Subcommand::Template(sub) => match sub {
            TemplateSub::List => {
                with_readonly(&db_path, |conn| run_template_list(conn, args.format))
            }
            TemplateSub::Add(template_args) => with_writer(&db_path, |rt, handle, conn| {
                run_template_add(rt, handle, conn, template_args, args.format)
            }),
            TemplateSub::Edit { name, args: ta } => with_writer(&db_path, |rt, handle, conn| {
                run_template_edit(rt, handle, conn, &name, ta, args.format)
            }),
            TemplateSub::Remove { name } => with_writer(&db_path, |rt, handle, conn| {
                run_template_remove(rt, handle, conn, &name)
            }),
        },
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
    let mut tasks = filtered_tasks(conn, &parsed.expr, today, &ctx)?;
    if !parsed.sorts.is_empty() {
        sort_tasks(&mut tasks, &parsed.sorts, &ctx_data);
    } else {
        // Bare-text fast-path: when the user typed freeform words and
        // didn't pin a sort order, rank by FTS5 bm25 blended with
        // recency so the result list reads "most relevant first"
        // instead of "first by task position." Falls through to
        // position order when no bare text is present (or none of
        // the matching rows happens to be in the FTS5 hit set).
        rank_by_bm25_and_recency(conn, &parsed.expr, &mut tasks, today)?;
    }
    print_tasks(&tasks, &ctx_data, format);
    Ok(())
}

/// Filter the full task set against `expr`. Uses the SQL-translation
/// fast-path when `atrium_search::try_translate` succeeds (every
/// node maps to SQL); otherwise falls back to loading every row
/// and running the in-memory evaluator. Both paths return the
/// same set — verified by an integration test pair in atrium-core.
fn filtered_tasks(
    conn: &Connection,
    expr: &atrium_search::Expr,
    today: NaiveDate,
    ctx: &EvalContext<'_>,
) -> CliResult<Vec<Task>> {
    if let Some(clause) = atrium_search::try_translate(expr, today) {
        let params: Vec<atrium_core::SqlBindValue> = clause.params.iter().map(Into::into).collect();
        return read::list_tasks_matching(conn, &clause.sql, &params).map_err(CliError::from);
    }
    let mut tasks = read::list_all_tasks(conn).map_err(CliError::from)?;
    tasks.retain(|t| evaluate(expr, t, ctx));
    Ok(tasks)
}

/// Reorder `tasks` in-place by FTS5 bm25 + recency when the parsed
/// expression contains at least one bare-text term and the call site
/// hasn't already applied an explicit `sort:` modifier. Tasks that
/// don't appear in the bm25 result map keep their existing relative
/// order (stable sort) at the bottom of the list.
fn rank_by_bm25_and_recency(
    conn: &Connection,
    expr: &atrium_search::Expr,
    tasks: &mut [Task],
    today: NaiveDate,
) -> CliResult<()> {
    let terms = atrium_search::collect_text_terms(expr);
    if terms.is_empty() {
        return Ok(());
    }
    let scores = read::bm25_for_terms(conn, &terms).map_err(CliError::from)?;
    if scores.is_empty() {
        return Ok(());
    }
    // Half-life of 30 days mirrors the "freshly-touched edges out
    // lukewarm matches over the last month" intuition. Tunable; for
    // now there's no setting, just a sensible default.
    const HALF_LIFE_DAYS: f64 = 30.0;
    tasks.sort_by(|a, b| {
        let score_a = blended_score(a, &scores, today, HALF_LIFE_DAYS);
        let score_b = blended_score(b, &scores, today, HALF_LIFE_DAYS);
        // Higher score sorts first.
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(())
}

fn blended_score(task: &Task, scores: &HashMap<i64, f64>, today: NaiveDate, half_life: f64) -> f64 {
    // Tasks not in the FTS5 hit set receive a relevance of 0; their
    // recency contribution alone keeps them ordered consistently
    // among themselves.
    let bm25 = scores.get(&task.id).copied().unwrap_or(0.0);
    let days = (today - task.modified_at.date_naive()).num_days();
    atrium_search::blend_relevance(bm25, days, half_life)
}

/// Slice D1 (v0.5.4) — render a saved Perspective whose
/// `renderer = "board"` as a kanban. Resolves the perspective by
/// case-insensitive name match, parses `renderer_config`, runs the
/// stored filter expression to load matching tasks, and prints
/// each grouped column. Errors with a clear message when the named
/// perspective is missing or its renderer is `"list"`.
fn run_kanban(conn: &Connection, name: &str, format: Format) -> CliResult<()> {
    let perspectives = read::list_perspectives(conn).map_err(CliError::from)?;
    let needle = name.trim().to_ascii_lowercase();
    let perspective = perspectives
        .iter()
        .find(|p| p.name.to_ascii_lowercase() == needle)
        .or_else(|| {
            perspectives
                .iter()
                .find(|p| p.name.to_ascii_lowercase().contains(&needle))
        })
        .ok_or_else(|| CliError::Args(format!("no perspective matches: {name}")))?;
    let renderer = atrium_core::Renderer::from_columns(
        &perspective.renderer,
        perspective.renderer_config.as_deref(),
    )
    .map_err(|e| CliError::Args(format!("perspective `{}`: {e}", perspective.name)))?;
    let cfg = match renderer {
        atrium_core::Renderer::Board(cfg) => cfg,
        atrium_core::Renderer::List => {
            return Err(CliError::Args(format!(
                "perspective `{}` is a list, not a board (set renderer=\"board\")",
                perspective.name
            )));
        }
    };
    // Run the stored filter expression to get the candidate task
    // set. Same code path as the search subcommand — uses the SQL
    // fast-path when translatable, falls back to in-memory eval.
    let parsed = atrium_search::parse(&perspective.filter_expr)
        .map_err(|e| CliError::Search(format!("{e:?}")))?;
    if !parsed.warnings.is_empty() {
        for w in &parsed.warnings {
            eprintln!("warning: unrecognised token: {w}");
        }
    }
    let today = Local::now().date_naive();
    let ctx_data = ContextData::load(conn)?;
    let ctx = ctx_data.eval_context(today);
    let mut tasks = filtered_tasks(conn, &parsed.expr, today, &ctx)?;
    if !parsed.sorts.is_empty() {
        sort_tasks(&mut tasks, &parsed.sorts, &ctx_data);
    }
    let tag_names = read::tag_names_per_task(conn).unwrap_or_default();
    let columns = atrium_core::group_into_board(&tasks, &cfg, &tag_names);
    print_board(&perspective.name, &columns, &ctx_data, format);
    Ok(())
}

/// Phase 16/v0.7.9 + Phase 18/v0.12.0 — `atrium-cli import
/// <SOURCE> PATH [--dry-run]`.
///
/// Sources:
///
/// - `org` — read a single `.org` file or walk a vault directory
///   via `atrium_org::org::import_org_file` /
///   `import_org_directory`. Output mirrors the
///   `atrium_org::org::ImportSummary` struct.
///
/// - `todoist` — parse a Todoist CSV export and apply its rows
///   through the v0.12.0 mapper, creating a project named via
///   `--into PROJECT_NAME` plus headings (sections), tasks,
///   tags (`@labels` + `priority-N`), recurring rules, etc.
///   Output mirrors the
///   `import::todoist::mapper::ImportSummary` struct.
fn run_import(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    source: ImportSource,
    path: &str,
    dry_run: bool,
    format: Format,
) -> CliResult<()> {
    match source {
        ImportSource::Org => {
            let path = std::path::PathBuf::from(path);
            let metadata = std::fs::metadata(&path).map_err(|e| {
                CliError::Args(format!("import: cannot stat {}: {e}", path.display()))
            })?;
            // v0.7.14 — directory paths trigger the multi-file
            // vault walker; file paths take the existing single-
            // file fast path.
            if metadata.is_dir() {
                let summaries = runtime
                    .block_on(async {
                        atrium_org::org::import_org_directory(handle, &path, dry_run).await
                    })
                    .map_err(|e| CliError::Args(format!("import failed: {e}")))?;
                print_import_directory_summary(&summaries, dry_run, format);
                return Ok(());
            }

            let summary = runtime
                .block_on(async { atrium_org::org::import_org_file(handle, &path, dry_run).await })
                .map_err(|e| CliError::Args(format!("import failed: {e}")))?;

            print_import_summary(&summary, dry_run, format);
            Ok(())
        }
        ImportSource::Todoist { project_name } => {
            let path_buf = std::path::PathBuf::from(path);
            let csv = std::fs::read_to_string(&path_buf).map_err(|e| {
                CliError::Args(format!(
                    "import todoist: cannot read {}: {e}",
                    path_buf.display()
                ))
            })?;
            let rows = import::todoist::parser::parse_csv(&csv)
                .map_err(|e| CliError::Args(format!("todoist parse error: {e}")))?;
            let today = Local::now().date_naive();
            let summary = runtime
                .block_on(async {
                    import::todoist::mapper::import_todoist(
                        handle,
                        &rows,
                        &project_name,
                        today,
                        dry_run,
                    )
                    .await
                })
                .map_err(|e| CliError::Args(format!("import todoist failed: {e}")))?;
            print_todoist_summary(&summary, dry_run, format);
            Ok(())
        }
    }
}

/// Render the Todoist mapper's summary. Mirrors the Org importer
/// formatter shape so script consumers see a familiar layout.
fn print_todoist_summary(
    summary: &import::todoist::mapper::ImportSummary,
    dry_run: bool,
    format: Format,
) {
    let prefix = if dry_run { "DRY-RUN " } else { "" };
    match format {
        Format::Json => {
            let mut s = String::new();
            s.push_str("{\n");
            s.push_str(&format!("  \"dry_run\": {dry_run},\n"));
            s.push_str(&format!(
                "  \"project_title\": {},\n",
                json_string(&summary.project_title)
            ));
            s.push_str(&format!(
                "  \"project_id\": {},\n",
                summary
                    .project_id
                    .map_or_else(|| "null".to_string(), |n| n.to_string())
            ));
            s.push_str(&format!(
                "  \"headings_created\": {},\n",
                summary.headings_created
            ));
            s.push_str(&format!(
                "  \"tasks_created\": {},\n",
                summary.tasks_created
            ));
            s.push_str(&format!("  \"tags_created\": {},\n", summary.tags_created));
            s.push_str("  \"meta_entries\": [");
            for (i, m) in summary.meta_entries.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&json_string(m));
            }
            s.push_str("],\n  \"lossy\": [");
            for (i, note) in summary.lossy.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&json_string(&format!(
                    "{:?}: {} ({})",
                    note.kind,
                    note.task_title.as_deref().unwrap_or("?"),
                    note.raw
                )));
            }
            s.push_str("]\n}\n");
            print!("{s}");
        }
        _ => {
            println!(
                "{prefix}Imported project “{}”: {} headings, {} tasks, {} tags.",
                summary.project_title,
                summary.headings_created,
                summary.tasks_created,
                summary.tags_created,
            );
            if !summary.meta_entries.is_empty() {
                println!("  meta entries:");
                for m in &summary.meta_entries {
                    println!("    {m}");
                }
            }
            for note in &summary.lossy {
                println!(
                    "  lossy ({:?}): {} — {}",
                    note.kind,
                    note.task_title.as_deref().unwrap_or("?"),
                    note.raw,
                );
            }
        }
    }
}

/// v0.7.14 — render the multi-file vault walk's vec of
/// summaries. Aggregates counts across files for the human-mode
/// banner; expands per-file detail underneath.
fn print_import_directory_summary(
    summaries: &[atrium_org::org::ImportSummary],
    dry_run: bool,
    format: Format,
) {
    let prefix = if dry_run { "DRY-RUN " } else { "" };
    let project_count = summaries
        .iter()
        .filter(|s| s.project_title.is_some())
        .count();
    let task_total: usize = summaries.iter().map(|s| s.tasks_created).sum();
    let tag_total: usize = summaries.iter().map(|s| s.tags_ensured).sum();
    let heading_total: usize = summaries.iter().map(|s| s.headings_skipped).sum();

    match format {
        Format::Json => {
            let mut s = String::new();
            s.push_str("{\n");
            s.push_str(&format!("  \"dry_run\": {dry_run},\n"));
            s.push_str(&format!("  \"project_count\": {project_count},\n"));
            s.push_str(&format!("  \"task_count\": {task_total},\n"));
            s.push_str(&format!("  \"tag_count\": {tag_total},\n"));
            s.push_str(&format!("  \"headings_skipped\": {heading_total},\n"));
            s.push_str("  \"projects\": [");
            for (i, sum) in summaries.iter().enumerate() {
                if sum.project_title.is_none() {
                    continue;
                }
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&json_string(sum.project_title.as_deref().unwrap_or("")));
            }
            s.push_str("]\n}\n");
            print!("{s}");
        }
        _ => {
            println!(
                "{prefix}Imported {} project{}: {} tasks, {} tags, {} headings skipped.",
                project_count,
                if project_count == 1 { "" } else { "s" },
                task_total,
                tag_total,
                heading_total
            );
            for sum in summaries {
                if let Some(title) = &sum.project_title {
                    println!(
                        "  {} ({} tasks, {} tags)",
                        title, sum.tasks_created, sum.tags_ensured
                    );
                }
                for note in &sum.lossy {
                    println!("    lossy: {note}");
                }
            }
        }
    }
}

fn print_import_summary(summary: &atrium_org::org::ImportSummary, dry_run: bool, format: Format) {
    let prefix = if dry_run { "DRY-RUN " } else { "" };
    match format {
        Format::Json => {
            // Render as a small JSON object so scripts can parse.
            let mut s = String::new();
            s.push_str("{\n");
            s.push_str(&format!("  \"dry_run\": {dry_run},\n"));
            s.push_str(&format!(
                "  \"project_title\": {},\n",
                json_string_or_null(summary.project_title.as_deref())
            ));
            s.push_str(&format!(
                "  \"project_id\": {},\n",
                summary
                    .project_id
                    .map_or_else(|| "null".to_string(), |n| n.to_string())
            ));
            s.push_str(&format!(
                "  \"tasks_created\": {},\n",
                summary.tasks_created
            ));
            s.push_str(&format!("  \"tags_ensured\": {},\n", summary.tags_ensured));
            s.push_str(&format!(
                "  \"headings_skipped\": {},\n",
                summary.headings_skipped
            ));
            s.push_str("  \"lossy\": [");
            let mut first = true;
            for note in &summary.lossy {
                if !first {
                    s.push_str(", ");
                }
                first = false;
                s.push_str(&json_string(note));
            }
            s.push_str("]\n}\n");
            print!("{s}");
        }
        _ => {
            println!(
                "{prefix}Imported project “{}”: {} tasks, {} tags, {} headings skipped.",
                summary.project_title.as_deref().unwrap_or("?"),
                summary.tasks_created,
                summary.tags_ensured,
                summary.headings_skipped,
            );
            for note in &summary.lossy {
                println!("  lossy: {note}");
            }
        }
    }
}

fn json_string(s: &str) -> String {
    let escaped: String = s
        .chars()
        .map(|c| match c {
            '"' => "\\\"".to_string(),
            '\\' => "\\\\".to_string(),
            '\n' => "\\n".to_string(),
            '\r' => "\\r".to_string(),
            '\t' => "\\t".to_string(),
            other if (other as u32) < 0x20 => format!("\\u{:04x}", other as u32),
            other => other.to_string(),
        })
        .collect();
    format!("\"{escaped}\"")
}

/// v0.16.0 — Phase 18.5 Tier-1 `atrium-cli vault sequences …`.
/// Manipulates the vault sidecar's `[[todo_sequences]]` slot
/// directly via the sidecar helpers; no DB round-trip required.
/// `list` prints in TSV / JSON / human format; `set` replaces
/// the configured sequence outright (single-sequence-per-vault
/// is the typical case); `clear` drops all configured sequences.
fn run_vault_sequences(vault: &str, op: VaultSequencesOp, format: Format) -> CliResult<()> {
    let root = std::path::PathBuf::from(vault);
    if !root.exists() {
        return Err(CliError::Args(format!(
            "vault path does not exist: {}",
            root.display()
        )));
    }
    let mut sidecar = atrium_org::sidecar::read_sidecar(&root)
        .map_err(|e| CliError::Args(format!("read sidecar: {e}")))?;

    match op {
        VaultSequencesOp::List => {
            print_todo_sequences(&sidecar.todo_sequences, format);
            Ok(())
        }
        VaultSequencesOp::Set {
            name,
            workflow,
            done,
        } => {
            // Replace outright. If the user wants multi-sequence
            // they re-run with a different --name and the parser
            // appends — but v0.16.0 ships single-sequence-only
            // because that's what every Org tutorial uses.
            sidecar.todo_sequences = vec![atrium_org::sidecar::TodoSequenceEntry {
                name: name.unwrap_or_else(|| "default".to_string()),
                workflow,
                done,
            }];
            atrium_org::sidecar::write_sidecar(&root, &sidecar)
                .map_err(|e| CliError::Args(format!("write sidecar: {e}")))?;
            print_todo_sequences(&sidecar.todo_sequences, format);
            Ok(())
        }
        VaultSequencesOp::Clear => {
            sidecar.todo_sequences.clear();
            atrium_org::sidecar::write_sidecar(&root, &sidecar)
                .map_err(|e| CliError::Args(format!("write sidecar: {e}")))?;
            println!("vault sequences cleared");
            Ok(())
        }
    }
}

fn print_todo_sequences(sequences: &[atrium_org::sidecar::TodoSequenceEntry], format: Format) {
    match format {
        Format::Tsv => {
            println!("name\tworkflow\tdone");
            for s in sequences {
                println!("{}\t{}\t{}", s.name, s.workflow.join(","), s.done.join(","));
            }
        }
        Format::Json => {
            // Hand-rolled to avoid pulling serde_json into
            // formatting; the shape is small + flat.
            print!("[");
            for (i, s) in sequences.iter().enumerate() {
                if i > 0 {
                    print!(",");
                }
                print!(
                    "{{\"name\":\"{name}\",\"workflow\":[{wf}],\"done\":[{dn}]}}",
                    name = json_escape(&s.name),
                    wf = s
                        .workflow
                        .iter()
                        .map(|k| format!("\"{}\"", json_escape(k)))
                        .collect::<Vec<_>>()
                        .join(","),
                    dn = s
                        .done
                        .iter()
                        .map(|k| format!("\"{}\"", json_escape(k)))
                        .collect::<Vec<_>>()
                        .join(",")
                );
            }
            println!("]");
        }
        Format::Human => {
            if sequences.is_empty() {
                println!("(no TODO sequences configured)");
                return;
            }
            for s in sequences {
                println!("# {}", s.name);
                println!("  workflow: {}", s.workflow.join(" "));
                println!("  done:     {}", s.done.join(" "));
            }
        }
    }
}

pub(crate) fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Phase 16, v0.7.10/v0.7.11 — `atrium-cli export <SOURCE> PATH [--dry-run]`.
///
/// Sources:
/// - `org` — write every project to a vault directory (one
///   `.org` file per project).
/// - `json` — write a single lossless JSON snapshot of the DB.
///
/// Dry-run walks what would be written without touching disk.
fn run_export(
    conn: &Connection,
    source: ExportSource,
    path: &str,
    dry_run: bool,
    format: Format,
) -> CliResult<()> {
    match source {
        ExportSource::Json => {
            let path = std::path::PathBuf::from(path);
            if dry_run {
                // For JSON we can't really "preview" the file
                // contents cheaply, but we can summarise what
                // would land. Build the snapshot in memory and
                // discard the file write.
                let snapshot =
                    atrium_core::sync::json::build_snapshot(conn).map_err(CliError::Db)?;
                print_json_export_summary(&snapshot, true, format, &path);
                return Ok(());
            }
            atrium_core::sync::json::export_db_to_json_file(conn, &path)
                .map_err(|e| CliError::Args(format!("export failed: {e}")))?;
            // Re-build for the summary count. Cheap; the same
            // function ran inside the file-export.
            let snapshot = atrium_core::sync::json::build_snapshot(conn).map_err(CliError::Db)?;
            print_json_export_summary(&snapshot, false, format, &path);
            Ok(())
        }
        ExportSource::Org => {
            let vault_root = std::path::PathBuf::from(path);
            if dry_run {
                // Mirror the writer's logic without writing: list
                // projects, count tasks each, build the
                // would-be-written paths.
                let projects = atrium_core::db::read::list_projects(conn).map_err(CliError::Db)?;
                let mut summaries: Vec<atrium_org::org::WriteSummary> = Vec::new();
                for project in projects {
                    let tasks = atrium_core::db::read::list_all_in_project(conn, project.id)
                        .map_err(CliError::Db)?;
                    summaries.push(atrium_org::org::WriteSummary {
                        project_id: project.id,
                        project_title: project.title.clone(),
                        task_count: tasks.len(),
                        file_path: dry_run_path(&vault_root, conn, &project)?,
                    });
                }
                print_export_summary(&summaries, true, format);
                return Ok(());
            }
            let summaries = atrium_org::org::write_all_projects_to_vault(conn, &vault_root)
                .map_err(|e| CliError::Args(format!("export failed: {e}")))?;
            print_export_summary(&summaries, false, format);
            Ok(())
        }
    }
}

/// Resolve the same path the real writer would use, without
/// performing any write. Used by `run_export`'s dry-run branch.
fn dry_run_path(
    vault_root: &Path,
    conn: &Connection,
    project: &atrium_core::Project,
) -> CliResult<std::path::PathBuf> {
    let area_title = match project.area_id {
        Some(aid) => atrium_core::db::read::area_by_id(conn, aid)
            .map_err(CliError::Db)?
            .map(|a| a.title),
        None => None,
    };
    let mut path = vault_root.to_path_buf();
    if let Some(area) = area_title {
        path.push(sanitize_filename_for_dry_run(&area));
    }
    path.push(format!(
        "{}.org",
        sanitize_filename_for_dry_run(&project.title)
    ));
    Ok(path)
}

/// Mirror of `sync::org::write::sanitize_filename`. Re-implemented
/// here rather than re-exported so the writer's helper stays
/// crate-private.
fn sanitize_filename_for_dry_run(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_was_underscore = false;
    for ch in s.chars() {
        let valid = ch.is_alphanumeric() || matches!(ch, ' ' | '-' | '_' | '.');
        if valid {
            out.push(ch);
            prev_was_underscore = ch == '_';
        } else if !prev_was_underscore {
            out.push('_');
            prev_was_underscore = true;
        }
    }
    let trimmed = out.trim_matches(|c: char| c == ' ' || c == '_').to_string();
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed
    }
}

/// v0.7.11 — `atrium-cli export json` summary printer. Reports
/// the snapshot dimensions (counts per table) so the user knows
/// what landed in the file (or what *would* land, in dry-run).
fn print_json_export_summary(
    snapshot: &atrium_core::sync::json::Snapshot,
    dry_run: bool,
    format: Format,
    path: &Path,
) {
    let prefix = if dry_run { "DRY-RUN " } else { "" };
    match format {
        Format::Json => {
            let mut s = String::new();
            s.push_str("{\n");
            s.push_str(&format!("  \"dry_run\": {dry_run},\n"));
            s.push_str(&format!(
                "  \"path\": {},\n",
                json_string(&path.to_string_lossy())
            ));
            s.push_str(&format!("  \"areas\": {},\n", snapshot.areas.len()));
            s.push_str(&format!("  \"projects\": {},\n", snapshot.projects.len()));
            s.push_str(&format!("  \"headings\": {},\n", snapshot.headings.len()));
            s.push_str(&format!("  \"tasks\": {},\n", snapshot.tasks.len()));
            s.push_str(&format!("  \"tags\": {},\n", snapshot.tags.len()));
            s.push_str(&format!("  \"task_tags\": {},\n", snapshot.task_tags.len()));
            s.push_str(&format!(
                "  \"perspectives\": {}\n",
                snapshot.perspectives.len()
            ));
            s.push_str("}\n");
            print!("{s}");
        }
        _ => {
            println!("{prefix}Exported snapshot to {}", path.display());
            println!(
                "  {} areas, {} projects, {} headings, {} tasks, {} tags, {} task-tag pairs, {} perspectives",
                snapshot.areas.len(),
                snapshot.projects.len(),
                snapshot.headings.len(),
                snapshot.tasks.len(),
                snapshot.tags.len(),
                snapshot.task_tags.len(),
                snapshot.perspectives.len(),
            );
        }
    }
}

fn print_export_summary(
    summaries: &[atrium_org::org::WriteSummary],
    dry_run: bool,
    format: Format,
) {
    let prefix = if dry_run { "DRY-RUN " } else { "" };
    match format {
        Format::Json => {
            let mut s = String::new();
            s.push_str("{\n");
            s.push_str(&format!("  \"dry_run\": {dry_run},\n"));
            s.push_str(&format!("  \"project_count\": {},\n", summaries.len()));
            s.push_str("  \"projects\": [\n");
            for (i, sum) in summaries.iter().enumerate() {
                s.push_str("    {\n");
                s.push_str(&format!("      \"id\": {},\n", sum.project_id));
                s.push_str(&format!(
                    "      \"title\": {},\n",
                    json_string(&sum.project_title)
                ));
                s.push_str(&format!("      \"task_count\": {},\n", sum.task_count));
                s.push_str(&format!(
                    "      \"path\": {}\n",
                    json_string(&sum.file_path.to_string_lossy())
                ));
                s.push_str("    }");
                if i + 1 < summaries.len() {
                    s.push(',');
                }
                s.push('\n');
            }
            s.push_str("  ]\n}\n");
            print!("{s}");
        }
        _ => {
            println!(
                "{prefix}Exported {} project{}.",
                summaries.len(),
                if summaries.len() == 1 { "" } else { "s" }
            );
            for sum in summaries {
                println!(
                    "  {} → {} ({} task{})",
                    sum.project_title,
                    sum.file_path.display(),
                    sum.task_count,
                    if sum.task_count == 1 { "" } else { "s" }
                );
            }
        }
    }
}

fn json_string_or_null(s: Option<&str>) -> String {
    match s {
        Some(value) => json_string(value),
        None => "null".to_string(),
    }
}

/// Slice D follow-up (v0.6.5) — `atrium-cli perspective <SUB> NAME`
/// write side. Dispatches to create / edit / delete.
fn run_perspective(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    sub: PerspectiveSub,
    format: Format,
) -> CliResult<()> {
    match sub {
        PerspectiveSub::Create { name, args } => {
            run_perspective_create(runtime, handle, &name, &args, format)
        }
        PerspectiveSub::Edit { name, args } => {
            run_perspective_edit(runtime, handle, read_conn, &name, &args, format)
        }
        PerspectiveSub::Delete { name } => {
            run_perspective_delete(runtime, handle, read_conn, &name, format)
        }
    }
}

fn run_perspective_create(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    name: &str,
    args: &PerspectiveArgs,
    format: Format,
) -> CliResult<()> {
    let filter = args
        .filter
        .clone()
        .ok_or_else(|| CliError::Args("perspective create requires --filter EXPR".into()))?;
    let icon = match &args.icon {
        Some(EditIcon::Set(s)) => Some(s.clone()),
        Some(EditIcon::Clear) => None,
        None => None,
    };
    // Renderer + columns are validated together — a board needs
    // columns; a list rejects them. Returns the renderer name and
    // the JSON config (or None for list).
    let (renderer, renderer_config) = build_renderer_config(args)?;
    let new = atrium_core::NewPerspective {
        name: name.to_string(),
        icon,
        filter_expr: filter,
        renderer: Some(renderer),
        renderer_config,
    };
    let p = runtime
        .block_on(async { handle.create_perspective(new).await })
        .map_err(CliError::from)?;
    print_perspective_after_write(&p, format);
    Ok(())
}

fn run_perspective_edit(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    name: &str,
    args: &PerspectiveArgs,
    format: Format,
) -> CliResult<()> {
    let perspective = resolve_perspective_exact(read_conn, name)?;
    let mut update = atrium_core::PerspectiveUpdate::new(perspective.id);
    if let Some(new_name) = &args.rename {
        update = update.name(new_name.clone());
    }
    if let Some(filter) = &args.filter {
        update = update.filter_expr(filter.clone());
    }
    if let Some(icon) = &args.icon {
        update = match icon {
            EditIcon::Set(s) => update.icon(Some(s.clone())),
            EditIcon::Clear => update.icon(None),
        };
    }
    // Renderer / columns combo. If `--renderer` is set explicitly,
    // honour it. If only `--columns` is set, treat it as "update
    // existing board's columns" — error if the perspective isn't a
    // board.
    if args.renderer.is_some() || args.columns.is_some() {
        let synthesised = synthesise_renderer_for_edit(&perspective, args)?;
        update = update
            .renderer(synthesised.0)
            .renderer_config(synthesised.1);
    }
    if update.is_noop() {
        // Nothing to do — print the existing row so the user gets
        // a confirmation that they referred to the right one.
        print_perspective_after_write(&perspective, format);
        return Ok(());
    }
    let p = runtime
        .block_on(async { handle.update_perspective(update).await })
        .map_err(CliError::from)?;
    print_perspective_after_write(&p, format);
    Ok(())
}

fn run_perspective_delete(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    read_conn: &Connection,
    name: &str,
    format: Format,
) -> CliResult<()> {
    let perspective = resolve_perspective_exact(read_conn, name)?;
    runtime
        .block_on(async { handle.delete_perspective(perspective.id).await })
        .map_err(CliError::from)?;
    print_perspective_after_write(&perspective, format);
    Ok(())
}

/// Resolve a perspective by exact (case-insensitive) name. Used by
/// the destructive write paths so a typo doesn't accidentally edit
/// the wrong row. The read-only `kanban` subcommand uses substring
/// fallback; we deliberately don't here.
fn resolve_perspective_exact(conn: &Connection, name: &str) -> CliResult<atrium_core::Perspective> {
    let needle = name.trim().to_ascii_lowercase();
    let perspectives = read::list_perspectives(conn).map_err(CliError::from)?;
    perspectives
        .into_iter()
        .find(|p| p.name.to_ascii_lowercase() == needle)
        .ok_or_else(|| CliError::Args(format!("no perspective named: {name}")))
}

/// Build a renderer + renderer_config pair from the create flags.
fn build_renderer_config(args: &PerspectiveArgs) -> CliResult<(String, Option<String>)> {
    match args.renderer.as_deref() {
        Some("board") => {
            let columns = parse_columns(args.columns.as_deref())?;
            if columns.is_empty() {
                return Err(CliError::Args(
                    "--renderer board requires --columns 'a,b,c'".into(),
                ));
            }
            let cfg = atrium_core::BoardConfig {
                axis: atrium_core::BoardAxis::Tag,
                columns,
            };
            let json = cfg
                .to_json()
                .map_err(|e| CliError::Args(format!("renderer config serialisation: {e}")))?;
            Ok(("board".into(), Some(json)))
        }
        Some("list") | None => {
            // No renderer specified, or list — list takes no config.
            // Reject `--columns` without `--renderer board` so the
            // user doesn't think they configured something.
            if args.columns.is_some() {
                return Err(CliError::Args(
                    "--columns is only meaningful with --renderer board".into(),
                ));
            }
            Ok(("list".into(), None))
        }
        Some(other) => Err(CliError::Args(format!(
            "--renderer must be 'list' or 'board', got {other}"
        ))),
    }
}

/// Synthesise the renderer + renderer_config tuple for `edit`. If
/// `--renderer` is explicit, behave like `create`. If only
/// `--columns` is set, update the existing board's columns
/// in-place (error if the perspective isn't a board).
fn synthesise_renderer_for_edit(
    perspective: &atrium_core::Perspective,
    args: &PerspectiveArgs,
) -> CliResult<(String, Option<String>)> {
    if args.renderer.is_some() {
        // Explicit renderer flag — same logic as create.
        return build_renderer_config(args);
    }
    // No --renderer; --columns alone → must be editing a board.
    if !perspective.renderer.eq_ignore_ascii_case("board") {
        return Err(CliError::Args(format!(
            "perspective '{}' is renderer={}; pass --renderer board to convert it",
            perspective.name, perspective.renderer
        )));
    }
    let columns = parse_columns(args.columns.as_deref())?;
    if columns.is_empty() {
        return Err(CliError::Args(
            "--columns must contain at least one entry".into(),
        ));
    }
    let cfg = atrium_core::BoardConfig {
        axis: atrium_core::BoardAxis::Tag,
        columns,
    };
    let json = cfg
        .to_json()
        .map_err(|e| CliError::Args(format!("renderer config serialisation: {e}")))?;
    Ok(("board".into(), Some(json)))
}

/// Parse a `--columns "a,b,c"` flag into a column-name list.
/// Empty entries (consecutive commas) are dropped; surrounding
/// whitespace is trimmed.
fn parse_columns(raw: Option<&str>) -> CliResult<Vec<String>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    Ok(raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Print a single perspective row after a write. Re-uses the
/// existing `print_perspectives` plumbing so the output schema
/// matches `list perspectives`.
fn print_perspective_after_write(p: &atrium_core::Perspective, format: Format) {
    output::print_perspectives(std::slice::from_ref(p), format);
}

/// Render a kanban board to stdout. JSON emits one object per
/// column with a nested rows array; TSV labels each column with
/// `# label` and prints rows underneath; human output column-headers
/// each block + indents the rows for skim-readability.
fn print_board(
    perspective_name: &str,
    columns: &[atrium_core::Column<'_>],
    ctx: &ContextData,
    format: Format,
) {
    match format {
        Format::Json => {
            let payload: Vec<serde_json::Value> = columns
                .iter()
                .map(|c| {
                    let rows: Vec<Row> = c.tasks.iter().map(|t| build_row(t, ctx)).collect();
                    serde_json::json!({
                        "label": c.label,
                        "tasks": rows,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "perspective": perspective_name,
                    "columns": payload,
                }))
                .unwrap_or_else(|_| "{}".into())
            );
        }
        Format::Tsv => {
            for col in columns {
                println!(
                    "# {}\t({} task{})",
                    col.label,
                    col.tasks.len(),
                    if col.tasks.len() == 1 { "" } else { "s" }
                );
                let rows: Vec<Row> = col.tasks.iter().map(|t| build_row(t, ctx)).collect();
                if !rows.is_empty() {
                    print!("{}", output::format_rows(&rows));
                }
                println!();
            }
        }
        Format::Human => {
            println!("Board: {perspective_name}");
            println!();
            for col in columns {
                println!(
                    "── {} ── ({} task{})",
                    col.label,
                    col.tasks.len(),
                    if col.tasks.len() == 1 { "" } else { "s" }
                );
                if col.tasks.is_empty() {
                    println!("    (empty)");
                } else {
                    let rows: Vec<Row> = col.tasks.iter().map(|t| build_row(t, ctx)).collect();
                    for row in &rows {
                        println!("    {}  {}", row.status, row.title);
                    }
                }
                println!();
            }
        }
    }
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
    let mut project_id = match add.project.as_deref() {
        Some(p) => Some(resolve_project_by_name(read_conn, p)?),
        None => None,
    };
    // Subtasks (Phase 19.5) — when --parent is given without an
    // explicit --project, inherit the parent's project so the worker's
    // same-project rule passes. Reading the parent here also surfaces a
    // clear "not found" before the create attempt.
    if let Some(pid) = add.parent {
        let parent = read::task_by_id(read_conn, pid)
            .map_err(CliError::from)?
            .ok_or_else(|| CliError::Args(format!("parent task {pid} not found")))?;
        if add.project.is_none() {
            project_id = parent.project_id;
        }
    }
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
        parent_id: add.parent,
        scheduled_for,
        deadline,
        defer_until,
        estimated_minutes: add.estimated_minutes,
        repeat_rule: None,
        repeat_mode: None,
        uuid: None,
        orig_keyword: None,
        completed_at: None,
        deadline_warn_days: add.deadline_warn,
        // v0.19.0 — populated by the --time flag wired in the
        // scheduled_time CLI task.
        scheduled_time: add.scheduled_time,
        // v0.20.0 — populated by the --reminder flag wired in
        // task #61.
        reminder_at: add.reminder_at,
        // v0.24.0 — `atrium-cli add` doesn't surface a
        // `--extra` flag; in-CLI captures land with no
        // extras. The Org importer is the only path that
        // populates this column today.
        extra_properties: std::collections::BTreeMap::new(),
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
/// line through atrium_inline (the same parser the GUI's
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
    let parsed = atrium_inline::parse(line);
    let projected_tags = parsed.projected_tag_names();
    if parsed.title.trim().is_empty() && projected_tags.is_empty() {
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
    if !projected_tags.is_empty() {
        let mut tag_ids: Vec<i64> = Vec::with_capacity(projected_tags.len());
        for name in &projected_tags {
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
    if let Some(p) = edit.parent.clone() {
        match p {
            EditParent::TopLevel => update = update.reparent(None),
            EditParent::Task(pid) => update = update.reparent(Some(pid)),
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
    if let Some(s) = edit.deadline_warn.as_deref() {
        if s.eq_ignore_ascii_case("none") {
            update = update.deadline_warn_days_value(None);
        } else {
            let n: i64 = s
                .parse()
                .map_err(|_| CliError::Args(format!("--deadline-warn: not an integer: {s}")))?;
            update = update.deadline_warn_days_value(Some(n));
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
    filtered_tasks(read_conn, &parsed.expr, today, &ctx)
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
            // Subtasks (Phase 19.5) — Human format only, so the TSV /
            // JSON one-record shape stays stable for grep / jq.
            let children = read::list_subtasks(conn, id).map_err(CliError::from)?;
            if !children.is_empty() {
                println!("\nSubtasks ({}):", children.len());
                for c in &children {
                    let mark = if c.is_completed() { "x" } else { " " };
                    println!("  [{mark}] {} (#{})", c.title, c.id);
                }
            }
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
pub(crate) enum CliError {
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

pub(crate) type CliResult<T> = Result<T, CliError>;

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
