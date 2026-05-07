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
/// generates `uuid` and computes `position` (last-in-sibling-list).
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

    /// `true` when no field will change. The worker treats no-op
    /// updates as a read of the current row.
    pub fn is_noop(&self) -> bool {
        self.title.is_none()
            && self.note.is_none()
            && self.position.is_none()
            && self.project_id.is_none()
            && self.scheduled_for.is_none()
            && self.deadline.is_none()
    }
}

// ── Areas ────────────────────────────────────────────────────────

/// Input for creating a new area. Position is computed by the worker
/// (last-in-list).
#[derive(Debug, Clone, Default)]
pub struct NewArea {
    pub title: String,
}

#[derive(Debug, Clone, Default)]
pub struct AreaUpdate {
    pub id: i64,
    pub title: Option<String>,
    pub position: Option<f64>,
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

    pub fn is_noop(&self) -> bool {
        self.title.is_none() && self.position.is_none()
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
