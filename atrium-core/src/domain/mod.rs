// SPDX-License-Identifier: MIT
//! Domain types — Atrium's data shape in Rust. Each top-level table
//! has a struct here; the `db::read` module pulls rows into them and
//! the worker writes them back.

mod scheduled;

pub use scheduled::ScheduledFor;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// A `task` row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub uuid: String,
    pub title: String,
    pub note: String,
    pub project_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub scheduled_for: Option<ScheduledFor>,
    pub deadline: Option<NaiveDate>,
    pub defer_until: Option<NaiveDate>,
    pub estimated_minutes: Option<i64>,
    pub completed_at: Option<DateTime<Utc>>,
    pub repeat_rule: Option<String>,
    /// Phase 15 — Org-style repeater semantics. One of `BASIC` /
    /// `CUMULATIVE` / `NEXT`; NULL means "use the default" (currently
    /// CUMULATIVE — matches Org's `++` and OmniFocus's "next instance
    /// after now"). Only meaningful when `repeat_rule` is set.
    pub repeat_mode: Option<String>,
    /// v0.7.4 — when the user last marked this task as reviewed
    /// (canonical Review page's task-level Mark Reviewed action).
    /// NULL means "never reviewed." The Review page hides tasks
    /// whose `last_reviewed_at` is within the last 7 days from
    /// the weekly walk; otherwise the column is unused.
    pub last_reviewed_at: Option<DateTime<Utc>>,
    /// v0.7.12 — Phase 16 round-trip anchor for non-canonical Org
    /// keywords (e.g. `WAITING`, `BLOCKED`, `IN-PROGRESS`). NULL
    /// when the task was created in Atrium or imported with a
    /// canonical TODO / DONE / CANCELLED. The Org writer consults
    /// this column when emitting so the original keyword survives
    /// a vault round-trip; otherwise it falls back to the
    /// canonical keyword implied by `completed_at`.
    pub orig_keyword: Option<String>,
    pub position: f64,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
}

impl Task {
    /// Returns true when the task has a non-NULL `completed_at`.
    pub fn is_completed(&self) -> bool {
        self.completed_at.is_some()
    }
}

/// Input for creating a new task. The DB assigns `id`; the worker
/// generates `uuid` (or honors a caller-provided one — see `uuid`
/// field below) and computes `position` (last-in-sibling-list).
/// Timestamps default via the schema.
#[derive(Debug, Clone, Default)]
pub struct NewTask {
    pub title: String,
    pub note: String,
    pub project_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub scheduled_for: Option<ScheduledFor>,
    pub deadline: Option<NaiveDate>,
    pub defer_until: Option<NaiveDate>,
    pub estimated_minutes: Option<i64>,
    pub repeat_rule: Option<String>,
    /// Phase 15 — repeater mode (`BASIC` / `CUMULATIVE` / `NEXT`).
    /// `None` means "use the default", which is CUMULATIVE.
    pub repeat_mode: Option<String>,
    /// v0.7.9 — caller-provided UUID. `None` means the worker
    /// generates a fresh v4 UUID (the historical behaviour). The
    /// Org importer uses this to preserve `:ID:` from the source
    /// vault file (spec §7.3.3 rule 2: ":ID: is the round-trip
    /// anchor"). Empty strings are rejected by the worker.
    pub uuid: Option<String>,
    /// v0.7.12 — non-canonical Org keyword to stash on the task.
    /// `None` for canonical TODO / DONE / CANCELLED (the worker
    /// stores NULL); `Some(name)` for things like `WAITING`,
    /// `BLOCKED`, `IN-PROGRESS`. The Org writer consults the
    /// resulting column when emitting.
    pub orig_keyword: Option<String>,
    /// v0.7.17 — caller-provided completion timestamp. Lets the
    /// Org importer preserve the source vault file's `CLOSED:`
    /// cookie verbatim instead of stamping `now()` on import via
    /// the toggle_complete path. `None` means "the task is open"
    /// (canonical NewTask behaviour). `Some(when)` means "this
    /// task is already done at that timestamp" — the worker
    /// inserts with completed_at set directly, no toggle needed.
    pub completed_at: Option<DateTime<Utc>>,
}

impl NewTask {
    /// Convenience for the common case: a freshly-captured Inbox task.
    pub fn inbox(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }
}

/// Partial update to an existing task. `None` = leave the field
/// unchanged; for the nullable columns, `Some(None)` clears the
/// value, `Some(Some(v))` sets it.
#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    pub id: i64,
    pub title: Option<String>,
    pub note: Option<String>,
    pub position: Option<f64>,
    /// `Some(Some(id))` moves the task to that project; `Some(None)`
    /// unfiles it (Inbox); `None` leaves the field alone.
    pub project_id: Option<Option<i64>>,
    /// Phase 7i — schedule (When). `Some(None)` clears the column
    /// (no schedule), `Some(Some(value))` sets to either a date or
    /// the Someday sentinel.
    pub scheduled_for: Option<Option<ScheduledFor>>,
    /// Phase 7i — deadline. `Some(None)` clears, `Some(Some(date))`
    /// sets.
    pub deadline: Option<Option<NaiveDate>>,
    /// Phase 11 — defer-until (Builder). The list-filter contract
    /// in spec §4.2 already excludes `defer_until > today` from
    /// Today / Anytime; this field is what the Inspector writes to
    /// enable that exclusion.
    pub defer_until: Option<Option<NaiveDate>>,
    /// Phase 11 — estimated minutes (Builder). Free-form integer
    /// minutes; `Some(None)` clears, `Some(Some(n))` sets.
    pub estimated_minutes: Option<Option<i64>>,
    /// Phase 15 — RFC 5545 RRULE text. `Some(None)` clears the rule
    /// (and the task stops repeating); `Some(Some(rule))` sets it.
    /// The rule is validated by the worker before insertion.
    pub repeat_rule: Option<Option<String>>,
    /// Phase 15 — Org repeater mode. `Some(None)` clears (falls back
    /// to default CUMULATIVE); `Some(Some("BASIC" / "CUMULATIVE" /
    /// "NEXT"))` sets.
    pub repeat_mode: Option<Option<String>>,
    /// Phase 17 (v0.10.0) — completion timestamp set directly,
    /// without going through `toggle_complete`. Lets the vault
    /// watcher round-trip Org's `CLOSED:` cookie verbatim instead
    /// of stamping `now()` when state flips externally.
    /// `Some(None)` clears (re-opens the task — equivalent to
    /// toggling on a completed row); `Some(Some(when))` sets.
    pub completed_at: Option<Option<DateTime<Utc>>>,
}

impl TaskUpdate {
    pub fn new(id: i64) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub fn position(mut self, position: f64) -> Self {
        self.position = Some(position);
        self
    }

    /// Move the task to a project (or to Inbox via `None`).
    pub fn project(mut self, project_id: Option<i64>) -> Self {
        self.project_id = Some(project_id);
        self
    }

    /// Phase 7i — set the schedule. Pass `None` to clear, or
    /// `Some(ScheduledFor::Date(d))` / `Some(ScheduledFor::Someday)`
    /// to set.
    pub fn schedule(mut self, value: Option<ScheduledFor>) -> Self {
        self.scheduled_for = Some(value);
        self
    }

    /// Phase 7i — set the deadline. Pass `None` to clear or
    /// `Some(date)` to set.
    pub fn deadline_value(mut self, value: Option<NaiveDate>) -> Self {
        self.deadline = Some(value);
        self
    }

    /// Phase 11 — set the defer-until. Pass `None` to clear or
    /// `Some(date)` to set. A future date excludes the task from
    /// Today and Anytime until the date crosses.
    pub fn defer_value(mut self, value: Option<NaiveDate>) -> Self {
        self.defer_until = Some(value);
        self
    }

    /// Phase 11 — set the estimated minutes. Pass `None` to clear
    /// or `Some(n)` to set.
    pub fn estimated_minutes_value(mut self, value: Option<i64>) -> Self {
        self.estimated_minutes = Some(value);
        self
    }

    /// Phase 15 — set or clear the repeat rule. Pass `None` to drop
    /// the rule entirely (the task stops repeating); pass
    /// `Some(rule)` to install a fresh RFC 5545 RRULE string. The
    /// worker validates the rule before persisting.
    pub fn repeat_rule_value(mut self, value: Option<String>) -> Self {
        self.repeat_rule = Some(value);
        self
    }

    /// Phase 15 — set or clear the Org repeater mode. Pass `None` to
    /// fall back to the default (CUMULATIVE); pass `Some("BASIC")` /
    /// `Some("CUMULATIVE")` / `Some("NEXT")` to override.
    pub fn repeat_mode_value(mut self, value: Option<String>) -> Self {
        self.repeat_mode = Some(value);
        self
    }

    /// Phase 17 (v0.10.0) — set or clear the completion timestamp
    /// directly. Distinct from `toggle_complete`, which always
    /// stamps `now()`. The vault watcher uses this so an Org
    /// `CLOSED: [2026-04-01 Wed]` cookie round-trips into the DB
    /// with the source date intact.
    pub fn completed_at(mut self, value: Option<DateTime<Utc>>) -> Self {
        self.completed_at = Some(value);
        self
    }

    /// `true` when no field will change. The worker treats no-op
    /// updates as a read of the current row.
    pub fn is_noop(&self) -> bool {
        self.title.is_none()
            && self.note.is_none()
            && self.position.is_none()
            && self.project_id.is_none()
            && self.scheduled_for.is_none()
            && self.deadline.is_none()
            && self.defer_until.is_none()
            && self.estimated_minutes.is_none()
            && self.repeat_rule.is_none()
            && self.repeat_mode.is_none()
            && self.completed_at.is_none()
    }
}

// ── Areas ────────────────────────────────────────────────────────

/// Input for creating a new area. Position is computed by the worker
/// (last-in-list).
#[derive(Debug, Clone, Default)]
pub struct NewArea {
    pub title: String,
    /// Phase 15.75 — optional accent colour. Hex string (`"#3584e4"`)
    /// from the same six-swatch palette tags use; `None` for areas
    /// with no chosen accent.
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AreaUpdate {
    pub id: i64,
    pub title: Option<String>,
    pub position: Option<f64>,
    /// Phase 15.75 — `Some(Some(hex))` sets the accent, `Some(None)`
    /// clears it back to no accent.
    pub color: Option<Option<String>>,
}

impl AreaUpdate {
    pub fn new(id: i64) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn position(mut self, position: f64) -> Self {
        self.position = Some(position);
        self
    }

    /// Phase 15.75 — set or clear the area's accent colour. Pass
    /// `None` to clear (back to no accent), `Some(hex)` to set.
    pub fn color(mut self, color: Option<String>) -> Self {
        self.color = Some(color);
        self
    }

    pub fn is_noop(&self) -> bool {
        self.title.is_none() && self.position.is_none() && self.color.is_none()
    }
}

// ── Projects ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct NewProject {
    pub title: String,
    pub note: String,
    pub area_id: Option<i64>,
    pub sequential: bool,
    pub review_interval_days: Option<i64>,
    /// v0.7.9 — caller-provided UUID. `None` means the worker
    /// generates a fresh v4 UUID. The Org importer uses this to
    /// preserve project `:ID:` values from a source vault.
    pub uuid: Option<String>,
    /// v0.7.13 — caller-provided last-reviewed timestamp. The
    /// Org importer threads `:LAST_REVIEWED:` from the file-level
    /// properties drawer. None falls through to the schema's
    /// NULL default.
    pub last_reviewed_at: Option<DateTime<Utc>>,
    /// v0.7.13 — caller-provided archived timestamp. The Org
    /// importer threads `:ARCHIVED:`. None falls through to NULL.
    pub archived_at: Option<DateTime<Utc>>,
}

impl NewProject {
    pub fn unfiled(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    pub fn in_area(title: impl Into<String>, area_id: i64) -> Self {
        Self {
            title: title.into(),
            area_id: Some(area_id),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProjectUpdate {
    pub id: i64,
    pub title: Option<String>,
    pub note: Option<String>,
    pub area_id: Option<Option<i64>>,
    pub sequential: Option<bool>,
    pub review_interval_days: Option<Option<i64>>,
    pub position: Option<f64>,
}

impl ProjectUpdate {
    pub fn new(id: i64) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub fn area(mut self, area_id: Option<i64>) -> Self {
        self.area_id = Some(area_id);
        self
    }

    pub fn sequential(mut self, sequential: bool) -> Self {
        self.sequential = Some(sequential);
        self
    }

    /// Phase 10 — Review interval picker on the Builder Mode project
    /// page. `Some(None)` clears the column (project no longer
    /// reviewed); `Some(Some(days))` sets it.
    pub fn review_interval_days(mut self, days: Option<i64>) -> Self {
        self.review_interval_days = Some(days);
        self
    }

    pub fn position(mut self, position: f64) -> Self {
        self.position = Some(position);
        self
    }

    pub fn is_noop(&self) -> bool {
        self.title.is_none()
            && self.note.is_none()
            && self.area_id.is_none()
            && self.sequential.is_none()
            && self.review_interval_days.is_none()
            && self.position.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub uuid: String,
    pub title: String,
    pub note: String,
    pub area_id: Option<i64>,
    pub sequential: bool,
    pub review_interval_days: Option<i64>,
    pub last_reviewed_at: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub position: f64,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Area {
    pub id: i64,
    pub uuid: String,
    pub title: String,
    /// Phase 15.75 — optional accent colour as a hex string. `None`
    /// for areas with no chosen accent.
    pub color: Option<String>,
    pub position: f64,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub uuid: String,
    pub name: String,
    pub color: Option<String>,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
}

/// Input for creating a tag. The DB enforces NOCASE-unique `name`,
/// so duplicate creation surfaces as `DbError::Sqlite` (constraint
/// violation) and the UI maps it to a friendly toast.
#[derive(Debug, Clone, Default)]
pub struct NewTag {
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TagUpdate {
    pub id: i64,
    pub name: Option<String>,
    /// `Some(Some(rgb))` sets the colour; `Some(None)` clears it.
    pub color: Option<Option<String>>,
}

impl TagUpdate {
    pub fn new(id: i64) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn color(mut self, color: Option<String>) -> Self {
        self.color = Some(color);
        self
    }

    pub fn is_noop(&self) -> bool {
        self.name.is_none() && self.color.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Heading {
    pub id: i64,
    pub uuid: String,
    pub project_id: i64,
    pub title: String,
    pub position: f64,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
}

// ── Perspectives (Phase 14) ─────────────────────────────────────

/// A saved filter expression. Phase 14's first-class object —
/// users name a filter (Phase 7d's mini-language: `tag:NAME`,
/// `is:open`, `is:done`, `is:overdue`, `due:today`, plus freeform
/// FTS5 text), pin it to the sidebar, and it acts like a custom
/// derived view alongside Today / Inbox / etc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Perspective {
    pub id: i64,
    pub uuid: String,
    pub name: String,
    /// Symbolic icon name (e.g., `"starred-symbolic"`); `None`
    /// falls back to the default Perspective icon.
    pub icon: Option<String>,
    /// The Phase 7d filter expression, stored verbatim so future
    /// parser changes don't require migrations.
    pub filter_expr: String,
    /// Reserved for Phase 14.x — explicit sort spec.
    pub sort_order: Option<String>,
    /// Reserved for Phase 14.x — explicit grouping spec.
    pub grouping: Option<String>,
    /// Phase 15.75 — renderer name. `"list"` (default — same as
    /// v0.4.0) or `"board"` (Slice D kanban). Stored as a string so
    /// future renderers can append without a column-type migration.
    pub renderer: String,
    /// Phase 15.75 — renderer-specific configuration as JSON. NULL
    /// for `"list"` (no config needed). For `"board"` the shape is
    /// `{ "axis": "tag", "columns": [...], "wip_limits": {...} }`.
    pub renderer_config: Option<String>,
    pub position: f64,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
}

/// Input for creating a perspective. Position is computed by the
/// worker (last in the list). UUID is generated by the worker.
#[derive(Debug, Clone, Default)]
pub struct NewPerspective {
    pub name: String,
    pub icon: Option<String>,
    pub filter_expr: String,
    /// Phase 15.75 — renderer name. `None` falls back to `"list"`,
    /// matching the column DEFAULT.
    pub renderer: Option<String>,
    /// Phase 15.75 — renderer-specific JSON config. `None` for
    /// `"list"`; `Some` JSON for `"board"`.
    pub renderer_config: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PerspectiveUpdate {
    pub id: i64,
    pub name: Option<String>,
    /// `Some(None)` clears the icon (back to default), `Some(Some(...))` sets it.
    pub icon: Option<Option<String>>,
    pub filter_expr: Option<String>,
    pub position: Option<f64>,
    /// Phase 15.75 — `Some("list")` or `Some("board")` swaps the
    /// renderer. `None` leaves it unchanged.
    pub renderer: Option<String>,
    /// Phase 15.75 — `Some(Some(json))` sets the renderer config,
    /// `Some(None)` clears it (back to NULL — empty board).
    pub renderer_config: Option<Option<String>>,
}

impl PerspectiveUpdate {
    pub fn new(id: i64) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn icon(mut self, icon: Option<String>) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn filter_expr(mut self, expr: impl Into<String>) -> Self {
        self.filter_expr = Some(expr.into());
        self
    }

    pub fn position(mut self, position: f64) -> Self {
        self.position = Some(position);
        self
    }

    /// Phase 15.75 — set the renderer. `"list"` or `"board"`.
    pub fn renderer(mut self, renderer: impl Into<String>) -> Self {
        self.renderer = Some(renderer.into());
        self
    }

    /// Phase 15.75 — set or clear the renderer config JSON. Pass
    /// `None` to clear (board with no columns picked yet), or
    /// `Some(json)` to install.
    pub fn renderer_config(mut self, config: Option<String>) -> Self {
        self.renderer_config = Some(config);
        self
    }

    pub fn is_noop(&self) -> bool {
        self.name.is_none()
            && self.icon.is_none()
            && self.filter_expr.is_none()
            && self.position.is_none()
            && self.renderer.is_none()
            && self.renderer_config.is_none()
    }
}
