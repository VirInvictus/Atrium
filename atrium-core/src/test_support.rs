// SPDX-License-Identifier: MIT
//! Test-helper constructors for downstream test crates.
//!
//! Gated behind the `test-support` feature so production builds of
//! `atrium-core` don't carry the helpers. The `atrium` binary's
//! `[dev-dependencies]` opt in via:
//!
//! ```toml
//! [dev-dependencies]
//! atrium-core = { path = "../atrium-core", features = ["test-support"] }
//! ```
//!
//! Adding a new column on a domain struct is then a one-line edit
//! here instead of a sweep across every dummy-task literal.

use chrono::{NaiveDate, Utc};

use crate::domain::{ScheduledFor, Task};

/// Build a placeholder `Task` for tests. All fields default to
/// "open, no schedule, no project". `id` and a derived
/// `format!("u{id}")` uuid keep multiple dummies in the same test
/// distinguishable.
pub fn dummy_task(id: i64) -> Task {
    Task {
        id,
        uuid: format!("u{id}"),
        title: format!("t{id}"),
        note: String::new(),
        project_id: None,
        parent_id: None,
        scheduled_for: None,
        deadline: None,
        defer_until: None,
        estimated_minutes: None,
        completed_at: None,
        repeat_rule: None,
        repeat_mode: None,
        last_reviewed_at: None,
        orig_keyword: None,
        deadline_warn_days: None,
        scheduled_time: None,
        reminder_at: None,
        position: id as f64,
        created_at: Utc::now(),
        modified_at: Utc::now(),
    }
}

/// Build a placeholder `Task` with a specific deadline + completion
/// state. Used by filter / forecast tests that exercise date-based
/// list membership.
pub fn dummy_task_with(id: i64, deadline: Option<NaiveDate>, completed: bool) -> Task {
    Task {
        completed_at: completed.then(Utc::now),
        deadline,
        ..dummy_task(id)
    }
}

/// Build a placeholder `Task` with both a scheduled date and a
/// deadline. Forecast tests need the cross-product of these to
/// validate that a task with both fields surfaces on each day.
pub fn dummy_task_dated(
    id: i64,
    scheduled: Option<NaiveDate>,
    deadline: Option<NaiveDate>,
) -> Task {
    Task {
        scheduled_for: scheduled.map(ScheduledFor::Date),
        deadline,
        ..dummy_task(id)
    }
}
