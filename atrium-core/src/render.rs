// SPDX-License-Identifier: MIT
//! Perspective rendering — Slice D foundation (v0.5.4).
//!
//! `perspective.renderer` (TEXT) + `perspective.renderer_config`
//! (TEXT, JSON) shipped at v0.5.0. Until now, the only renderer the
//! GUI knew about was `"list"` (the default); this module is the
//! pure-Rust foundation for the second one, `"board"` (kanban).
//!
//! Two responsibilities:
//!
//! 1. **Parse** the `renderer_config` JSON into a typed [`Renderer`]
//!    enum. Reject bad shapes early so the GUI never has to guard
//!    against malformed config.
//! 2. **Group** a task vector into kanban columns per the parsed
//!    config. Returns a [`Vec<Column>`]; the GUI then renders each
//!    column as a vertical task list inside a horizontal scroll view.
//!
//! The grouping rules (locked at v0.5.4):
//!
//! - **Leftmost match wins.** A task with multiple matching tags
//!   appears in only the leftmost matching column. This matches the
//!   kanban-as-state mental model — a task is in *one* state at a
//!   time, even if its tag set names several.
//! - **"Other" column.** Tasks that don't match any of the listed
//!   columns go to a trailing `"Other"` column. Keeps the kanban
//!   honest about coverage; users who want a tighter view tighten
//!   the perspective filter (e.g., `is:open AND tag:true`).
//! - **Case-insensitive tag matching.** `Todo` and `todo` are the
//!   same column; mirrors the rest of the search engine's tag rules.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::domain::Task;

/// Top-level renderer kind. Wraps the parsed `renderer_config`. The
/// caller looks up `perspective.renderer` to know which arm to expect
/// — this enum is the typed shape downstream of that string.
#[derive(Debug, Clone, PartialEq)]
pub enum Renderer {
    /// `renderer = "list"` — the default flat list view. No config.
    List,
    /// `renderer = "board"` — kanban columns.
    Board(BoardConfig),
}

/// Parsed `"board"` config. The `renderer_config` JSON shape is
///
/// ```json
/// { "axis": "tag", "columns": ["todo", "doing", "done"] }
/// ```
///
/// for a tag-axis board, and for a status-axis board (v0.38.0):
///
/// ```json
/// { "axis": "status",
///   "columns": ["TODO", "NEXT", "WAITING", "DONE", "CANCELLED"],
///   "done_columns": ["DONE", "CANCELLED"] }
/// ```
///
/// `axis = "tag"` buckets by tag name; `axis = "status"` buckets by
/// the task's Org TODO-sequence keyword (`task.orig_keyword`, falling
/// back to canonical `TODO`/`DONE`). Rejecting unknown axes at parse
/// time keeps the GUI from silently doing the wrong thing on a config
/// it doesn't understand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoardConfig {
    pub axis: BoardAxis,
    /// Column values in display order. For `axis = "tag"`, each entry
    /// is a tag name (case-insensitive). For `axis = "status"`, each
    /// entry is a TODO-sequence keyword. Trailing whitespace is
    /// stripped at parse time; empty strings are rejected.
    pub columns: Vec<String>,
    /// Status-axis only: which of `columns` represent a *completed*
    /// state (right of the Org `#+TODO:` pipe). Dropping a card on
    /// one of these completes the task. Empty for tag-axis boards;
    /// the canonical `DONE` keyword is always treated as done even
    /// when absent here. `skip_serializing_if` keeps existing
    /// tag-board JSON byte-identical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub done_columns: Vec<String>,
}

impl BoardConfig {
    /// Serialise to the JSON shape the schema stores in
    /// `perspective.renderer_config`. Centralising this here keeps
    /// the GUI / CLI from having to import `serde_json` or hand-roll
    /// JSON; the round-trip with `Renderer::from_columns` stays
    /// pinned by the parsing tests above.
    pub fn to_json(&self) -> Result<String, RendererError> {
        serde_json::to_string(self).map_err(|e| RendererError::InvalidJson(e.to_string()))
    }

    /// Parse from the same JSON shape `to_json` produces. Skips
    /// the validation step that `Renderer::from_columns` runs
    /// (empty-columns rejection); use this when you want the raw
    /// config to populate an editing dialog and the validation is
    /// the dialog's job.
    pub fn from_json(s: &str) -> Result<Self, RendererError> {
        serde_json::from_str(s).map_err(|e| RendererError::InvalidJson(e.to_string()))
    }
}

/// Grouping axis for a board renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoardAxis {
    /// Bucket by tag name. Drag rewrites the task's tag set.
    Tag,
    /// Bucket by Org TODO-sequence keyword (`task.orig_keyword`,
    /// falling back to canonical `TODO`/`DONE`). Drag changes real
    /// state — completing the task when the destination is a
    /// done-column. (v0.38.0)
    Status,
}

/// Errors surfaced when parsing `renderer_config` JSON or building
/// a `Renderer` from a `(renderer_name, config_json)` pair.
#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    #[error("unknown renderer kind: {0:?}")]
    UnknownKind(String),
    #[error("renderer `{kind}` requires renderer_config but got NULL")]
    MissingConfig { kind: String },
    #[error("renderer_config is not valid JSON: {0}")]
    InvalidJson(String),
    #[error("renderer_config: {0}")]
    InvalidShape(String),
}

impl Renderer {
    /// Build a `Renderer` from a perspective's `(renderer, renderer_config)`
    /// pair. The renderer name is matched case-insensitively (the
    /// schema column is plain TEXT, so we don't want a typo'd
    /// `"Board"` to silently fall through to `"list"`).
    pub fn from_columns(renderer: &str, config_json: Option<&str>) -> Result<Self, RendererError> {
        match renderer.trim().to_ascii_lowercase().as_str() {
            "list" => Ok(Renderer::List),
            "board" => {
                let raw = config_json.ok_or_else(|| RendererError::MissingConfig {
                    kind: "board".into(),
                })?;
                let cfg: BoardConfig = serde_json::from_str(raw)
                    .map_err(|e| RendererError::InvalidJson(e.to_string()))?;
                validate_board_config(&cfg)?;
                Ok(Renderer::Board(cfg))
            }
            other => Err(RendererError::UnknownKind(other.into())),
        }
    }
}

fn validate_board_config(cfg: &BoardConfig) -> Result<(), RendererError> {
    if cfg.columns.is_empty() {
        return Err(RendererError::InvalidShape(
            "`columns` must contain at least one entry".into(),
        ));
    }
    for (i, c) in cfg.columns.iter().enumerate() {
        if c.trim().is_empty() {
            return Err(RendererError::InvalidShape(format!(
                "`columns[{i}]` is blank"
            )));
        }
    }
    Ok(())
}

/// One rendered kanban column. Borrows the underlying tasks so the
/// caller can decide on lifetimes — the GUI clones into AtriumTask
/// glib objects, the CLI prints fields directly.
#[derive(Debug, Clone, PartialEq)]
pub struct Column<'a> {
    /// The column's display label. For tag-axis boards, the
    /// configured tag name verbatim (case preserved as the user
    /// configured it). For the trailing "Other" bucket, the literal
    /// string "Other".
    pub label: String,
    /// Tasks landing in this column, in input order. The caller
    /// already ran whatever sort modifiers / bm25 ranking it
    /// wanted; the grouper only buckets, it never reorders.
    pub tasks: Vec<&'a Task>,
}

/// Column label used for the trailing "everything that didn't fit"
/// bucket. Public so the GUI can pattern-match on it (e.g., for an
/// "uncategorized" tint in the column header).
pub const OTHER_COLUMN_LABEL: &str = "Other";

/// Compute the new tag list when a task is dragged from its current
/// kanban column to a destination. The "current column" is the
/// task's leftmost-matching tag against `cfg.columns` (the same
/// rule [`group_into_board`] uses to bucket); that's the tag we
/// remove. The destination column's tag is appended when not
/// already in the list.
///
/// `destination = Some(name)` drops into a configured column;
/// `destination = None` drops into the trailing "Other" bucket
/// (just remove the source column tag, don't add anything).
///
/// Non-column tags pass through unchanged. If the task has no
/// column-matching tags (it was in "Other"), nothing is removed.
/// If the destination is the same column the task was already in,
/// the function is a no-op (case-insensitive).
///
/// Returns the new tag list. Does not mutate `current_tags`.
pub fn move_to_column(
    current_tags: &[String],
    cfg: &BoardConfig,
    destination: Option<&str>,
) -> Vec<String> {
    let lc_columns: Vec<String> = cfg.columns.iter().map(|c| c.to_ascii_lowercase()).collect();
    let lc_current: Vec<String> = current_tags
        .iter()
        .map(|t| t.to_ascii_lowercase())
        .collect();
    // Find the leftmost configured column whose name appears in the
    // task's current tag set — that's the "source" tag we strip.
    let source_lc: Option<String> = lc_columns
        .iter()
        .find(|col| lc_current.iter().any(|t| &t == col))
        .cloned();

    let mut result: Vec<String> = current_tags
        .iter()
        .filter(|t| {
            source_lc
                .as_ref()
                .is_none_or(|src| &t.to_ascii_lowercase() != src)
        })
        .cloned()
        .collect();

    if let Some(dest) = destination {
        let dest_lc = dest.to_ascii_lowercase();
        if !result.iter().any(|t| t.to_ascii_lowercase() == dest_lc) {
            result.push(dest.to_string());
        }
    }

    result
}

/// The status-axis keyword a task currently lives under. Uses the
/// non-canonical Org keyword when present (`WAITING`, `NEXT`, …);
/// otherwise canonical `DONE` for a completed task, `TODO` for an
/// open one. Mirrors the Org writer's "orig_keyword first, else
/// TODO/DONE" lookup so the board and the vault agree.
pub fn status_keyword(task: &Task) -> String {
    match task.orig_keyword.as_deref() {
        Some(k) if !k.trim().is_empty() => k.to_string(),
        _ => {
            if task.completed_at.is_some() {
                "DONE".to_string()
            } else {
                "TODO".to_string()
            }
        }
    }
}

/// True when `col` is a completed-state column for `cfg` — either
/// listed in `done_columns` (right of the Org `#+TODO:` pipe) or the
/// canonical `DONE` keyword, which is always a done-column.
fn is_done_column(cfg: &BoardConfig, col: &str) -> bool {
    col.eq_ignore_ascii_case("DONE") || cfg.done_columns.iter().any(|d| d.eq_ignore_ascii_case(col))
}

/// Collapse a destination keyword to the value Atrium should store in
/// `task.orig_keyword`. The canonical `TODO`/`DONE` keywords carry no
/// custom label, so they map to `None` (the schema default); any
/// other keyword is stored verbatim.
fn keyword_for_storage(col: &str) -> Option<String> {
    if col.eq_ignore_ascii_case("TODO") || col.eq_ignore_ascii_case("DONE") {
        None
    } else {
        Some(col.to_string())
    }
}

/// The state change a status-axis drag implies. The GUI/CLI translate
/// this into worker calls: set `orig_keyword` via `update_task`, and
/// flip completion via `toggle_complete` when `completed` differs from
/// the task's current state (so a recurring task rolls forward exactly
/// as it would on a checkbox tick).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusChange {
    /// New `orig_keyword` value (`None` = canonical `TODO`/`DONE`).
    pub orig_keyword: Option<String>,
    /// Desired completion state after the move.
    pub completed: bool,
}

/// Compute the [`StatusChange`] for dragging a task to `destination`
/// on a status-axis board. `destination = Some(keyword)` drops into a
/// configured column; `None` drops into the trailing "Other" bucket.
///
/// - **Open column:** set the keyword, leave the task open.
/// - **Done column:** set the keyword (or clear it for canonical
///   `DONE`) and complete the task.
/// - **Other:** clear `orig_keyword` to canonical; completion is left
///   unchanged (there's no meaningful "no status" completion flip).
///
/// `current_completed` is the task's present completion state, used
/// only for the Other case.
pub fn status_move(
    cfg: &BoardConfig,
    destination: Option<&str>,
    current_completed: bool,
) -> StatusChange {
    match destination {
        None => StatusChange {
            orig_keyword: None,
            completed: current_completed,
        },
        Some(col) => StatusChange {
            orig_keyword: keyword_for_storage(col),
            completed: is_done_column(cfg, col),
        },
    }
}

/// Parse a status-axis column spec written in the Org `#+TODO:` pipe
/// convention — `TODO, NEXT, WAITING | DONE, CANCELLED`. Columns are
/// comma-separated within each side; surrounding whitespace is
/// trimmed and empty entries dropped. Everything right of the first
/// `|` is a done-column. No pipe → every column is open. Returns
/// `(columns, done_columns)` ready to drop into a [`BoardConfig`].
pub fn parse_status_columns(input: &str) -> (Vec<String>, Vec<String>) {
    let (open_part, done_part) = match input.split_once('|') {
        Some((l, r)) => (l, r),
        None => (input, ""),
    };
    let split = |s: &str| -> Vec<String> {
        s.split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    };
    let open = split(open_part);
    let done = split(done_part);
    let mut columns = open;
    columns.extend(done.iter().cloned());
    (columns, done)
}

/// Inverse of [`parse_status_columns`]: render a config's columns back
/// into the pipe convention for an editing dialog's text entry. Open
/// columns first, then ` | `, then the done-columns. Omits the pipe
/// when there are no done-columns.
pub fn format_status_columns(cfg: &BoardConfig) -> String {
    let done_lc: Vec<String> = cfg
        .done_columns
        .iter()
        .map(|d| d.to_ascii_lowercase())
        .collect();
    let open: Vec<&str> = cfg
        .columns
        .iter()
        .filter(|c| !done_lc.contains(&c.to_ascii_lowercase()))
        .map(|c| c.as_str())
        .collect();
    if cfg.done_columns.is_empty() {
        open.join(", ")
    } else {
        let done: Vec<&str> = cfg.done_columns.iter().map(|c| c.as_str()).collect();
        format!("{} | {}", open.join(", "), done.join(", "))
    }
}

/// Group `tasks` into kanban columns per `cfg`. The trailing
/// `"Other"` column holds anything that didn't match a configured
/// column; its presence is unconditional so the user always sees
/// the full task set.
///
/// `tag_names_per_task` maps task id → its tag name list (the same
/// HashMap shape `read::tag_names_per_task` returns). Tag names are
/// matched case-insensitively against the configured column names.
pub fn group_into_board<'a>(
    tasks: &'a [Task],
    cfg: &BoardConfig,
    tag_names_per_task: &HashMap<i64, Vec<String>>,
) -> Vec<Column<'a>> {
    let mut columns: Vec<Column<'a>> = cfg
        .columns
        .iter()
        .map(|c| Column {
            label: c.clone(),
            tasks: Vec::new(),
        })
        .collect();
    let mut other = Column {
        label: OTHER_COLUMN_LABEL.into(),
        tasks: Vec::new(),
    };

    // Pre-lowercase the configured column names so we don't redo
    // the work in the per-task hot loop.
    let lc_columns: Vec<String> = cfg.columns.iter().map(|c| c.to_ascii_lowercase()).collect();

    for task in tasks {
        let bucket = match cfg.axis {
            BoardAxis::Tag => {
                let task_tags: Vec<String> = tag_names_per_task
                    .get(&task.id)
                    .map(|tags| tags.iter().map(|t| t.to_ascii_lowercase()).collect())
                    .unwrap_or_default();
                // Leftmost match wins — iterate columns in order, drop
                // out the moment we find one of the task's tags.
                lc_columns
                    .iter()
                    .position(|col| task_tags.iter().any(|t| t == col))
            }
            BoardAxis::Status => {
                // A task always has exactly one status, so leftmost
                // matching is just "find the column equal to it".
                let status = status_keyword(task).to_ascii_lowercase();
                lc_columns.iter().position(|col| *col == status)
            }
        };
        match bucket {
            Some(idx) => columns[idx].tasks.push(task),
            None => other.tasks.push(task),
        }
    }
    columns.push(other);
    columns
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::dummy_task;

    fn cfg(columns: &[&str]) -> BoardConfig {
        BoardConfig {
            axis: BoardAxis::Tag,
            columns: columns.iter().map(|s| (*s).to_string()).collect(),
            done_columns: Vec::new(),
        }
    }

    /// Status-axis config helper: `columns` are keyword names,
    /// `done` are the ones that mean completed.
    fn status_cfg(columns: &[&str], done: &[&str]) -> BoardConfig {
        BoardConfig {
            axis: BoardAxis::Status,
            columns: columns.iter().map(|s| (*s).to_string()).collect(),
            done_columns: done.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn tag_map(entries: &[(i64, &[&str])]) -> HashMap<i64, Vec<String>> {
        entries
            .iter()
            .map(|(id, tags)| (*id, tags.iter().map(|t| (*t).to_string()).collect()))
            .collect()
    }

    // ── parsing ────────────────────────────────────────

    #[test]
    fn list_renderer_needs_no_config() {
        let r = Renderer::from_columns("list", None).unwrap();
        assert_eq!(r, Renderer::List);
    }

    #[test]
    fn list_renderer_ignores_config_when_present() {
        // Defensive — a stray config on a `list` perspective shouldn't
        // error out the row.
        let r = Renderer::from_columns("list", Some("{}")).unwrap();
        assert_eq!(r, Renderer::List);
    }

    #[test]
    fn board_renderer_parses_axis_and_columns() {
        let json = r#"{"axis":"tag","columns":["todo","doing","done"]}"#;
        let r = Renderer::from_columns("board", Some(json)).unwrap();
        match r {
            Renderer::Board(cfg) => {
                assert_eq!(cfg.axis, BoardAxis::Tag);
                assert_eq!(cfg.columns, vec!["todo", "doing", "done"]);
            }
            other => panic!("expected Board, got {other:?}"),
        }
    }

    #[test]
    fn board_renderer_is_case_insensitive_on_kind() {
        let json = r#"{"axis":"tag","columns":["x"]}"#;
        let r = Renderer::from_columns("BOARD", Some(json)).unwrap();
        assert!(matches!(r, Renderer::Board(_)));
    }

    #[test]
    fn board_renderer_requires_config() {
        let err = Renderer::from_columns("board", None).unwrap_err();
        assert!(matches!(err, RendererError::MissingConfig { .. }));
    }

    #[test]
    fn board_renderer_rejects_invalid_json() {
        let err = Renderer::from_columns("board", Some("{not json")).unwrap_err();
        assert!(matches!(err, RendererError::InvalidJson(_)));
    }

    #[test]
    fn board_renderer_rejects_unknown_axis() {
        let json = r#"{"axis":"project","columns":["x"]}"#;
        let err = Renderer::from_columns("board", Some(json)).unwrap_err();
        // serde rejects the unknown variant before we even get to
        // validation; surfaces as InvalidJson because serde wraps
        // it as a deserialization error.
        assert!(matches!(err, RendererError::InvalidJson(_)));
    }

    #[test]
    fn board_renderer_rejects_empty_columns() {
        let json = r#"{"axis":"tag","columns":[]}"#;
        let err = Renderer::from_columns("board", Some(json)).unwrap_err();
        assert!(matches!(err, RendererError::InvalidShape(_)));
    }

    #[test]
    fn board_renderer_rejects_blank_column_entry() {
        let json = r#"{"axis":"tag","columns":["todo","   "]}"#;
        let err = Renderer::from_columns("board", Some(json)).unwrap_err();
        assert!(matches!(err, RendererError::InvalidShape(_)));
    }

    #[test]
    fn unknown_renderer_kind_errors() {
        let err = Renderer::from_columns("waterfall", None).unwrap_err();
        assert!(matches!(err, RendererError::UnknownKind(_)));
    }

    // ── grouping ───────────────────────────────────────

    #[test]
    fn groups_tasks_into_configured_tag_columns() {
        let t1 = dummy_task(1);
        let t2 = dummy_task(2);
        let t3 = dummy_task(3);
        let tasks = vec![t1, t2, t3];
        let map = tag_map(&[(1, &["todo"]), (2, &["doing"]), (3, &["done"])]);
        let cols = group_into_board(&tasks, &cfg(&["todo", "doing", "done"]), &map);
        assert_eq!(cols.len(), 4); // three configured + Other
        assert_eq!(cols[0].label, "todo");
        assert_eq!(cols[0].tasks.len(), 1);
        assert_eq!(cols[0].tasks[0].id, 1);
        assert_eq!(cols[1].tasks[0].id, 2);
        assert_eq!(cols[2].tasks[0].id, 3);
        assert_eq!(cols[3].label, OTHER_COLUMN_LABEL);
        assert!(cols[3].tasks.is_empty());
    }

    #[test]
    fn untagged_tasks_land_in_other_column() {
        let tasks = vec![dummy_task(1), dummy_task(2)];
        let map = tag_map(&[(1, &["todo"])]); // task 2 has no tags
        let cols = group_into_board(&tasks, &cfg(&["todo"]), &map);
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].tasks.len(), 1);
        assert_eq!(cols[0].tasks[0].id, 1);
        assert_eq!(cols[1].label, OTHER_COLUMN_LABEL);
        assert_eq!(cols[1].tasks.len(), 1);
        assert_eq!(cols[1].tasks[0].id, 2);
    }

    #[test]
    fn task_with_unmatched_tag_lands_in_other() {
        // Task has a tag, just not one of the configured columns.
        let tasks = vec![dummy_task(1)];
        let map = tag_map(&[(1, &["urgent"])]);
        let cols = group_into_board(&tasks, &cfg(&["todo", "doing"]), &map);
        assert_eq!(cols[0].tasks.len(), 0);
        assert_eq!(cols[1].tasks.len(), 0);
        assert_eq!(cols[2].label, OTHER_COLUMN_LABEL);
        assert_eq!(cols[2].tasks.len(), 1);
    }

    #[test]
    fn leftmost_matching_tag_wins() {
        let tasks = vec![dummy_task(1)];
        // Task 1 is tagged with both "doing" and "done"; configured
        // columns are [todo, doing, done] — leftmost wins, "doing".
        let map = tag_map(&[(1, &["doing", "done"])]);
        let cols = group_into_board(&tasks, &cfg(&["todo", "doing", "done"]), &map);
        assert_eq!(cols[0].tasks.len(), 0); // todo
        assert_eq!(cols[1].tasks.len(), 1); // doing
        assert_eq!(cols[1].tasks[0].id, 1);
        assert_eq!(cols[2].tasks.len(), 0); // done — second match, ignored
    }

    #[test]
    fn tag_match_is_case_insensitive() {
        let tasks = vec![dummy_task(1)];
        // Configured column "Todo", task tagged "TODO".
        let map = tag_map(&[(1, &["TODO"])]);
        let cols = group_into_board(&tasks, &cfg(&["Todo"]), &map);
        assert_eq!(cols[0].tasks.len(), 1);
    }

    #[test]
    fn tasks_in_input_order_within_a_column() {
        // The grouper never reorders within a column — that's the
        // caller's job (sort modifiers, bm25, etc.). We pass three
        // tasks with the same tag in a specific order and verify
        // the column preserves it.
        let t1 = dummy_task(10);
        let t2 = dummy_task(20);
        let t3 = dummy_task(30);
        let tasks = vec![t1, t2, t3];
        let map = tag_map(&[(10, &["todo"]), (20, &["todo"]), (30, &["todo"])]);
        let cols = group_into_board(&tasks, &cfg(&["todo"]), &map);
        let ids: Vec<i64> = cols[0].tasks.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![10, 20, 30]);
    }

    #[test]
    fn board_config_to_json_round_trips_through_from_json() {
        let original = BoardConfig {
            axis: BoardAxis::Tag,
            columns: vec!["todo".into(), "doing".into(), "done".into()],
            done_columns: Vec::new(),
        };
        let json = original.to_json().unwrap();
        let parsed = BoardConfig::from_json(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn board_config_to_json_emits_compact_shape() {
        // The exact shape that the GUI dialog and atrium-cli kanban
        // both depend on; pinning it keeps a future serde derive
        // tweak from accidentally rewording the field names.
        let cfg = BoardConfig {
            axis: BoardAxis::Tag,
            columns: vec!["todo".into()],
            done_columns: Vec::new(),
        };
        let json = cfg.to_json().unwrap();
        assert_eq!(json, r#"{"axis":"tag","columns":["todo"]}"#);
    }

    // ── move_to_column ─────────────────────────────────

    fn names(s: &[&str]) -> Vec<String> {
        s.iter().map(|n| (*n).to_string()).collect()
    }

    #[test]
    fn move_to_real_column_removes_source_and_adds_destination() {
        // Task in "doing" → drop on "done". Result: `[done]`.
        let cur = names(&["doing"]);
        let out = move_to_column(&cur, &cfg(&["todo", "doing", "done"]), Some("done"));
        assert_eq!(out, vec!["done".to_string()]);
    }

    #[test]
    fn move_to_other_just_removes_source() {
        let cur = names(&["doing"]);
        let out = move_to_column(&cur, &cfg(&["todo", "doing", "done"]), None);
        assert!(out.is_empty());
    }

    #[test]
    fn move_to_same_column_is_a_noop_modulo_order() {
        // Task tagged "doing" + "extra" in column "doing", dropped
        // back on "doing". The source-removal then destination-add
        // round-trips the column tag; non-column tags pass through.
        let cur = names(&["doing", "extra"]);
        let out = move_to_column(&cur, &cfg(&["todo", "doing", "done"]), Some("doing"));
        // Order-insensitive equivalence: same multiset.
        let mut sorted_out = out.clone();
        sorted_out.sort();
        let mut expected = vec!["doing".to_string(), "extra".to_string()];
        expected.sort();
        assert_eq!(sorted_out, expected);
    }

    #[test]
    fn move_preserves_non_column_tags() {
        // Task with `[urgent, doing]` dragged from doing → done.
        // Result: `[urgent, done]` (urgent passes through).
        let cur = names(&["urgent", "doing"]);
        let out = move_to_column(&cur, &cfg(&["todo", "doing", "done"]), Some("done"));
        let mut sorted = out.clone();
        sorted.sort();
        let mut exp = vec!["urgent".to_string(), "done".to_string()];
        exp.sort();
        assert_eq!(sorted, exp);
    }

    #[test]
    fn move_with_no_source_just_adds_destination() {
        // Task previously in "Other" (no column tags) dragged to
        // "doing". Nothing to remove; just append.
        let cur = names(&["urgent"]);
        let out = move_to_column(&cur, &cfg(&["todo", "doing", "done"]), Some("doing"));
        let mut sorted = out.clone();
        sorted.sort();
        let mut exp = vec!["urgent".to_string(), "doing".to_string()];
        exp.sort();
        assert_eq!(sorted, exp);
    }

    #[test]
    fn move_is_case_insensitive() {
        // Configured columns are mixed-case; task tags are mixed-case;
        // both sides match without surface-form mattering.
        let cur = names(&["Doing"]);
        let out = move_to_column(&cur, &cfg(&["TODO", "DOING", "DONE"]), Some("done"));
        // Output preserves the destination string the user passed in.
        assert_eq!(out, vec!["done".to_string()]);
    }

    #[test]
    fn move_destination_already_present_does_not_duplicate() {
        // Task tagged `[doing, done]`. Drop on "done". Result:
        // `[done]` — `doing` removed (source), `done` already there.
        let cur = names(&["doing", "done"]);
        let out = move_to_column(&cur, &cfg(&["todo", "doing", "done"]), Some("done"));
        assert_eq!(out, vec!["done".to_string()]);
    }

    #[test]
    fn move_only_removes_leftmost_column_match() {
        // Task tagged `[doing, done]` in `[todo, doing, done]`
        // bucketed by `doing` (leftmost). Drop on Other: only
        // `doing` is removed; `done` stays.
        let cur = names(&["doing", "done"]);
        let out = move_to_column(&cur, &cfg(&["todo", "doing", "done"]), None);
        assert_eq!(out, vec!["done".to_string()]);
    }

    #[test]
    fn empty_task_set_produces_empty_columns() {
        let tasks: Vec<Task> = vec![];
        let map: HashMap<i64, Vec<String>> = HashMap::new();
        let cols = group_into_board(&tasks, &cfg(&["todo", "doing"]), &map);
        assert_eq!(cols.len(), 3);
        assert!(cols.iter().all(|c| c.tasks.is_empty()));
    }

    // ── status axis ────────────────────────────────────

    fn open_task(id: i64) -> Task {
        dummy_task(id)
    }

    fn completed_task(id: i64) -> Task {
        let mut t = dummy_task(id);
        t.completed_at = Some(chrono::Utc::now());
        t
    }

    fn keyworded_task(id: i64, keyword: &str) -> Task {
        let mut t = dummy_task(id);
        t.orig_keyword = Some(keyword.to_string());
        t
    }

    #[test]
    fn status_keyword_falls_back_to_canonical() {
        // Open task, no keyword → TODO.
        assert_eq!(status_keyword(&open_task(1)), "TODO");
        // Completed task, no keyword → DONE.
        assert_eq!(status_keyword(&completed_task(2)), "DONE");
        // Non-canonical keyword wins over completion-derived fallback.
        assert_eq!(status_keyword(&keyworded_task(3, "WAITING")), "WAITING");
    }

    #[test]
    fn status_keyword_ignores_blank_keyword() {
        let mut t = dummy_task(1);
        t.orig_keyword = Some("   ".into());
        assert_eq!(status_keyword(&t), "TODO");
    }

    #[test]
    fn status_axis_buckets_by_keyword() {
        let tasks = vec![
            keyworded_task(1, "NEXT"),
            keyworded_task(2, "WAITING"),
            open_task(3),      // no keyword, open → TODO
            completed_task(4), // no keyword, done → DONE
        ];
        let map = HashMap::new(); // status axis ignores the tag map
        let cfg = status_cfg(&["TODO", "NEXT", "WAITING", "DONE"], &["DONE"]);
        let cols = group_into_board(&tasks, &cfg, &map);
        assert_eq!(cols.len(), 5); // four configured + Other
        assert_eq!(cols[0].label, "TODO");
        assert_eq!(cols[0].tasks[0].id, 3);
        assert_eq!(cols[1].tasks[0].id, 1); // NEXT
        assert_eq!(cols[2].tasks[0].id, 2); // WAITING
        assert_eq!(cols[3].tasks[0].id, 4); // DONE
        assert!(cols[4].tasks.is_empty()); // Other
    }

    #[test]
    fn status_axis_unconfigured_keyword_lands_in_other() {
        // CANCELLED isn't a configured column.
        let tasks = vec![keyworded_task(1, "CANCELLED")];
        let cfg = status_cfg(&["TODO", "DONE"], &["DONE"]);
        let cols = group_into_board(&tasks, &cfg, &HashMap::new());
        assert_eq!(cols[0].tasks.len(), 0);
        assert_eq!(cols[1].tasks.len(), 0);
        assert_eq!(cols[2].label, OTHER_COLUMN_LABEL);
        assert_eq!(cols[2].tasks[0].id, 1);
    }

    #[test]
    fn status_axis_keyword_match_is_case_insensitive() {
        let tasks = vec![keyworded_task(1, "waiting")];
        let cfg = status_cfg(&["TODO", "WAITING"], &[]);
        let cols = group_into_board(&tasks, &cfg, &HashMap::new());
        assert_eq!(cols[1].tasks[0].id, 1);
    }

    #[test]
    fn status_move_to_open_column_sets_keyword_keeps_open() {
        let cfg = status_cfg(&["TODO", "WAITING", "DONE"], &["DONE"]);
        let change = status_move(&cfg, Some("WAITING"), false);
        assert_eq!(change.orig_keyword.as_deref(), Some("WAITING"));
        assert!(!change.completed);
    }

    #[test]
    fn status_move_to_done_column_completes() {
        let cfg = status_cfg(&["TODO", "DONE", "CANCELLED"], &["DONE", "CANCELLED"]);
        // Custom done keyword: stored verbatim, task completed.
        let change = status_move(&cfg, Some("CANCELLED"), false);
        assert_eq!(change.orig_keyword.as_deref(), Some("CANCELLED"));
        assert!(change.completed);
    }

    #[test]
    fn status_move_canonical_done_clears_keyword() {
        // Canonical DONE → no custom label, but still completes. It's
        // a done-column even though done_columns is empty.
        let cfg = status_cfg(&["TODO", "DOING", "DONE"], &[]);
        let change = status_move(&cfg, Some("DONE"), false);
        assert_eq!(change.orig_keyword, None);
        assert!(change.completed);
    }

    #[test]
    fn status_move_canonical_todo_clears_keyword_and_reopens() {
        let cfg = status_cfg(&["TODO", "DONE"], &["DONE"]);
        // Was completed; dragged back to TODO.
        let change = status_move(&cfg, Some("TODO"), true);
        assert_eq!(change.orig_keyword, None);
        assert!(!change.completed);
    }

    #[test]
    fn status_move_to_other_clears_keyword_keeps_completion() {
        let cfg = status_cfg(&["TODO", "DONE"], &["DONE"]);
        let change = status_move(&cfg, None, true);
        assert_eq!(change.orig_keyword, None);
        assert!(change.completed); // unchanged
    }

    #[test]
    fn parse_status_columns_splits_on_pipe() {
        let (cols, done) = parse_status_columns("TODO, NEXT, WAITING | DONE, CANCELLED");
        assert_eq!(cols, vec!["TODO", "NEXT", "WAITING", "DONE", "CANCELLED"]);
        assert_eq!(done, vec!["DONE", "CANCELLED"]);
    }

    #[test]
    fn parse_status_columns_without_pipe_has_no_done() {
        let (cols, done) = parse_status_columns("TODO, DOING, DONE");
        assert_eq!(cols, vec!["TODO", "DOING", "DONE"]);
        assert!(done.is_empty());
    }

    #[test]
    fn parse_status_columns_trims_and_drops_empties() {
        let (cols, done) = parse_status_columns("  TODO ,, DOING  |  DONE , ");
        assert_eq!(cols, vec!["TODO", "DOING", "DONE"]);
        assert_eq!(done, vec!["DONE"]);
    }

    #[test]
    fn format_status_columns_round_trips_through_parse() {
        let (cols, done) = parse_status_columns("TODO, NEXT | DONE, CANCELLED");
        let cfg = BoardConfig {
            axis: BoardAxis::Status,
            columns: cols,
            done_columns: done,
        };
        let text = format_status_columns(&cfg);
        assert_eq!(text, "TODO, NEXT | DONE, CANCELLED");
        // And the reparse matches.
        let (cols2, done2) = parse_status_columns(&text);
        assert_eq!(cols2, cfg.columns);
        assert_eq!(done2, cfg.done_columns);
    }

    #[test]
    fn format_status_columns_omits_pipe_when_no_done() {
        let cfg = status_cfg(&["TODO", "DOING", "DONE"], &[]);
        assert_eq!(format_status_columns(&cfg), "TODO, DOING, DONE");
    }

    #[test]
    fn status_board_config_round_trips_through_json() {
        let original = BoardConfig {
            axis: BoardAxis::Status,
            columns: vec!["TODO".into(), "DONE".into(), "CANCELLED".into()],
            done_columns: vec!["DONE".into(), "CANCELLED".into()],
        };
        let json = original.to_json().unwrap();
        assert_eq!(
            json,
            r#"{"axis":"status","columns":["TODO","DONE","CANCELLED"],"done_columns":["DONE","CANCELLED"]}"#
        );
        assert_eq!(BoardConfig::from_json(&json).unwrap(), original);
    }

    #[test]
    fn legacy_tag_config_without_done_columns_still_parses() {
        // Pre-v0.38.0 board configs have no `done_columns` key.
        let json = r#"{"axis":"tag","columns":["todo","doing"]}"#;
        let cfg = BoardConfig::from_json(json).unwrap();
        assert_eq!(cfg.axis, BoardAxis::Tag);
        assert!(cfg.done_columns.is_empty());
    }
}
