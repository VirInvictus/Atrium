// SPDX-License-Identifier: MIT
//! Output formatters — TSV (default), JSON, and human-readable.

use atrium_core::Task;
use serde::Serialize;

/// One output row, derived from a Task plus the cached metadata
/// (project / area title resolution, tag-name aggregation).
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub id: i64,
    pub status: String,
    pub title: String,
    pub scheduled: String,
    pub deadline: String,
    pub project: String,
    pub area: String,
    /// Comma-separated tag names — keeps the TSV column count
    /// fixed so downstream `cut`/`awk` doesn't have to count.
    pub tags: String,
}

/// TSV column header — emitted once at the top of the row stream so
/// `head -n1` describes the schema.
const TSV_HEADER: &str = "id\tstatus\ttitle\tscheduled\tdeadline\tproject\tarea\ttags";

/// Format a vec of rows as TSV, leading with a header row.
pub fn format_rows(rows: &[Row]) -> String {
    let mut out = String::with_capacity(64 + rows.len() * 64);
    out.push_str(TSV_HEADER);
    out.push('\n');
    for row in rows {
        out.push_str(&format_row(row));
        out.push('\n');
    }
    out
}

/// Format a single row as TSV (no header, no trailing newline).
/// Used by the `info` subcommand, which prints exactly one record.
pub fn format_row(row: &Row) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        row.id,
        row.status,
        sanitize_tsv(&row.title),
        row.scheduled,
        row.deadline,
        sanitize_tsv(&row.project),
        sanitize_tsv(&row.area),
        sanitize_tsv(&row.tags),
    )
}

/// JSON-array output — one object per row, suitable for `jq`.
pub fn rows_to_json(rows: &[Row]) -> String {
    serde_json::to_string(rows).unwrap_or_else(|_| "[]".into())
}

/// Single-row JSON object — for the `info` subcommand.
pub fn row_to_json(row: &Row) -> String {
    serde_json::to_string(row).unwrap_or_else(|_| "{}".into())
}

/// Human-readable column layout — for terminal viewing rather than
/// piping. Columns: id (right-aligned), status, title, deadline.
/// Truncates titles at 60 chars.
pub fn format_rows_human(rows: &[Row]) -> String {
    if rows.is_empty() {
        return "(no matches)\n".to_string();
    }
    let id_width = rows
        .iter()
        .map(|r| r.id.to_string().len())
        .max()
        .unwrap_or(2)
        .max(2);
    let status_width = rows
        .iter()
        .map(|r| r.status.len())
        .max()
        .unwrap_or(4)
        .max(6);
    let mut out = String::new();
    for row in rows {
        let title = truncate(&row.title, 60);
        let suffix = match (row.deadline.is_empty(), row.scheduled.is_empty()) {
            (true, true) => String::new(),
            (false, true) => format!("  due {}", row.deadline),
            (true, false) => format!("  ({})", row.scheduled),
            (false, false) => format!("  ({})  due {}", row.scheduled, row.deadline),
        };
        let tag_suffix = if row.tags.is_empty() {
            String::new()
        } else {
            format!("  [{}]", row.tags)
        };
        out.push_str(&format!(
            "{:>id_w$}  {:<st_w$}  {}{}{}\n",
            row.id,
            row.status,
            title,
            tag_suffix,
            suffix,
            id_w = id_width,
            st_w = status_width
        ));
    }
    out
}

/// Pretty-print a single task's full detail. Used by the `info`
/// subcommand's --human output. The Row already has the resolved
/// project / area / tags; the Task carries the note + uuid + dates
/// not surfaced in the row.
pub fn format_task_detail(task: &Task, row: &Row) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {} ({})\n", row.title, row.status));
    out.push_str(&format!("id    {}\n", row.id));
    out.push_str(&format!("uuid  {}\n", task.uuid));
    if !row.area.is_empty() {
        out.push_str(&format!("area  {}\n", row.area));
    }
    if !row.project.is_empty() {
        out.push_str(&format!("proj  {}\n", row.project));
    }
    if !row.scheduled.is_empty() {
        out.push_str(&format!("when  {}\n", row.scheduled));
    }
    if !row.deadline.is_empty() {
        out.push_str(&format!("due   {}\n", row.deadline));
    }
    if let Some(d) = task.defer_until {
        out.push_str(&format!("defer {d}\n"));
    }
    if let Some(min) = task.estimated_minutes {
        out.push_str(&format!("est   {min} minutes\n"));
    }
    if let Some(rule) = &task.repeat_rule {
        out.push_str(&format!("rule  {rule}\n"));
    }
    if !row.tags.is_empty() {
        out.push_str(&format!("tags  {}\n", row.tags));
    }
    if !task.note.is_empty() {
        out.push('\n');
        out.push_str(&task.note);
        out.push('\n');
    }
    out
}

/// Replace tab and newline characters in a TSV cell with spaces so
/// the row stays parseable. Tasks rarely have these in titles, but
/// the note field can be multi-line — atrium-cli doesn't surface
/// the note in the TSV row, but tag names and project titles still
/// need defensive cleaning.
fn sanitize_tsv(s: &str) -> String {
    s.replace(['\t', '\n'], " ")
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max - 1).collect();
        format!("{head}…")
    }
}
