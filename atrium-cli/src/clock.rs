// SPDX-License-Identifier: MIT
//! `atrium-cli clock` subcommands (Phase 18.5 Tier-1, v0.17.0).
//! Extracted from `main.rs` in the v0.21.0 maintenance pass.
//!
//! - `clock` (or `clock status`) — show currently-running entry
//! - `clock log <task_id>` — full per-task entry log with totals
//! - `clock in <task_id> [--note]` — open a new entry (auto-closes
//!   any other running clock per the single-active-clock invariant)
//! - `clock out <task_id>` — close the open entry on a task

use rusqlite::Connection;

use crate::args::Format;
use crate::{CliError, CliResult};

pub fn run_clock_status(conn: &Connection, format: Format) -> CliResult<()> {
    let active = atrium_core::db::read::active_clock(conn).map_err(CliError::from)?;
    let Some((task_id, started_at)) = active else {
        if matches!(format, Format::Human | Format::Tsv) {
            println!("(no clock running)");
        } else {
            println!("null");
        }
        return Ok(());
    };
    // Resolve the task title for human / TSV output.
    let title = atrium_core::db::read::task_by_id(conn, task_id)
        .map_err(CliError::from)?
        .map(|t| t.title)
        .unwrap_or_default();
    match format {
        Format::Human => {
            let started_local = started_at.with_timezone(&chrono::Local);
            println!(
                "running: {title} (id {task_id}, since {})",
                started_local.format("%Y-%m-%d %H:%M")
            );
        }
        Format::Tsv => {
            println!("task_id\ttitle\tstarted_at");
            println!(
                "{task_id}\t{title}\t{}",
                started_at.format("%Y-%m-%dT%H:%M:%SZ")
            );
        }
        Format::Json => {
            println!(
                "{{\"task_id\":{task_id},\"title\":\"{}\",\"started_at\":\"{}\"}}",
                title.replace('\\', "\\\\").replace('"', "\\\""),
                started_at.format("%Y-%m-%dT%H:%M:%SZ")
            );
        }
    }
    Ok(())
}

/// v0.17.0 — `atrium-cli clock log <task_id>` — print all clock
/// entries for a task. Newest-first per `list_clock_entries`.
pub fn run_clock_log(conn: &Connection, task_id: i64, format: Format) -> CliResult<()> {
    let entries =
        atrium_core::db::read::list_clock_entries(conn, task_id).map_err(CliError::from)?;
    match format {
        Format::Tsv => {
            println!("id\ttask_id\tstarted_at\tended_at\tduration_minutes\tnote");
            for e in &entries {
                let ended = e
                    .ended_at
                    .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                    .unwrap_or_default();
                let duration = e
                    .duration_minutes()
                    .map(|m| m.to_string())
                    .unwrap_or_default();
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    e.id,
                    e.task_id,
                    e.started_at.format("%Y-%m-%dT%H:%M:%SZ"),
                    ended,
                    duration,
                    e.note.replace(['\t', '\n'], " ")
                );
            }
        }
        Format::Json => {
            print!("[");
            for (i, e) in entries.iter().enumerate() {
                if i > 0 {
                    print!(",");
                }
                let ended = match e.ended_at {
                    Some(t) => format!("\"{}\"", t.format("%Y-%m-%dT%H:%M:%SZ")),
                    None => "null".to_string(),
                };
                print!(
                    "{{\"id\":{},\"task_id\":{},\"started_at\":\"{}\",\"ended_at\":{},\"note\":\"{}\"}}",
                    e.id,
                    e.task_id,
                    e.started_at.format("%Y-%m-%dT%H:%M:%SZ"),
                    ended,
                    e.note.replace('\\', "\\\\").replace('"', "\\\"")
                );
            }
            println!("]");
        }
        Format::Human => {
            if entries.is_empty() {
                println!("(no clock entries)");
                return Ok(());
            }
            let total: i64 = entries
                .iter()
                .filter_map(atrium_core::TaskClockEntry::duration_minutes)
                .sum();
            let h = total / 60;
            let m = total % 60;
            println!("# total: {h}:{m:02}");
            for e in &entries {
                let started_local = e.started_at.with_timezone(&chrono::Local);
                match e.duration_minutes() {
                    Some(d) => {
                        let h = d / 60;
                        let m = d % 60;
                        let note = if e.note.is_empty() {
                            String::new()
                        } else {
                            format!(" — {}", e.note)
                        };
                        println!(
                            "  {h}:{m:02}  {}{note}",
                            started_local.format("%a %b %-d %H:%M")
                        );
                    }
                    None => {
                        let note = if e.note.is_empty() {
                            String::new()
                        } else {
                            format!(" — {}", e.note)
                        };
                        println!(
                            "  running  started {}{note}",
                            started_local.format("%a %b %-d %H:%M")
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// v0.17.0 — `atrium-cli clock in <task_id> [--note TEXT]`.
/// Single-active-clock invariant — opens auto-close any other
/// running clock first.
pub fn run_clock_in(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    task_id: i64,
    note: String,
    format: Format,
) -> CliResult<()> {
    let entry = runtime
        .block_on(async { handle.clock_in(task_id, note).await })
        .map_err(CliError::from)?;
    match format {
        Format::Human => {
            let started_local = entry.started_at.with_timezone(&chrono::Local);
            println!(
                "clocked in on task {task_id} at {}",
                started_local.format("%H:%M")
            );
        }
        Format::Tsv | Format::Json => {
            // Reuse log printer for one entry.
            print_one_entry(&entry, format);
        }
    }
    Ok(())
}

/// v0.17.0 — `atrium-cli clock out <task_id>`. Soft no-op when
/// the task has no running clock.
pub fn run_clock_out(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    task_id: i64,
    format: Format,
) -> CliResult<()> {
    let result = runtime
        .block_on(async { handle.clock_out(task_id).await })
        .map_err(CliError::from)?;
    match (result, format) {
        (None, Format::Human) => println!("(no clock was running on task {task_id})"),
        (None, Format::Tsv | Format::Json) => println!(),
        (Some(entry), Format::Human) => {
            let mins = entry.duration_minutes().unwrap_or(0);
            let h = mins / 60;
            let m = mins % 60;
            println!("clocked out task {task_id} after {h}:{m:02}");
        }
        (Some(entry), fmt) => print_one_entry(&entry, fmt),
    }
    Ok(())
}

fn print_one_entry(entry: &atrium_core::TaskClockEntry, format: Format) {
    match format {
        Format::Tsv => {
            let ended = entry
                .ended_at
                .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_default();
            let duration = entry
                .duration_minutes()
                .map(|m| m.to_string())
                .unwrap_or_default();
            println!("id\ttask_id\tstarted_at\tended_at\tduration_minutes\tnote");
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                entry.id,
                entry.task_id,
                entry.started_at.format("%Y-%m-%dT%H:%M:%SZ"),
                ended,
                duration,
                entry.note.replace(['\t', '\n'], " ")
            );
        }
        Format::Json => {
            let ended = match entry.ended_at {
                Some(t) => format!("\"{}\"", t.format("%Y-%m-%dT%H:%M:%SZ")),
                None => "null".to_string(),
            };
            println!(
                "{{\"id\":{},\"task_id\":{},\"started_at\":\"{}\",\"ended_at\":{},\"note\":\"{}\"}}",
                entry.id,
                entry.task_id,
                entry.started_at.format("%Y-%m-%dT%H:%M:%SZ"),
                ended,
                entry.note.replace('\\', "\\\\").replace('"', "\\\"")
            );
        }
        Format::Human => {
            // The caller prints a friendlier line; this is the
            // fallback for direct callers.
            println!("{entry:?}");
        }
    }
}
