// SPDX-License-Identifier: MIT
//! `AtriumTask` — a `glib::Object` wrapper around `atrium_core::Task`.
//!
//! `gio::ListStore` and `gtk::SignalListItemFactory` traffic in
//! `glib::Object`s, so each row in a list is an `AtriumTask`. Property
//! bindings let widgets sync bidirectionally without manual signal
//! plumbing — `bind_property("title", &editable_label, "text")` keeps
//! both sides in step.

use std::cell::{Cell, RefCell};

use atrium_core::{ScheduledFor, Task};
use gtk::glib;
use gtk::glib::Properties;
use gtk::glib::subclass::prelude::*;
use gtk::prelude::*;

mod imp {
    use super::*;

    #[derive(Debug, Default, Properties)]
    #[properties(wrapper_type = super::AtriumTask)]
    pub struct AtriumTaskInner {
        #[property(get, set, construct_only)]
        pub id: Cell<i64>,
        #[property(get, set, construct_only)]
        pub uuid: RefCell<String>,
        #[property(get, set)]
        pub title: RefCell<String>,
        #[property(get, set)]
        pub note: RefCell<String>,
        #[property(get, set)]
        pub completed: Cell<bool>,
        #[property(get, set)]
        pub schedule_label: RefCell<String>,
        #[property(get, set)]
        pub deadline_label: RefCell<String>,
        #[property(get, set)]
        pub position: Cell<f64>,
        /// Space-separated `#tag` names, e.g. `"#errand #urgent"`.
        /// Empty when the task has no tags.
        #[property(get, set)]
        pub tag_names_csv: RefCell<String>,
        /// Cross-list context chip: `"Area › Project"` when the task
        /// has both, just `"Project"` when it's unfiled or the area
        /// is empty, empty string when the task has no project at
        /// all. Window callers populate this before the row binds;
        /// it stays blank for project- or area-scoped views where
        /// the chip would just echo the heading.
        #[property(get, set)]
        pub context_label: RefCell<String>,
        /// Phase 11 — `true` when the task is in a sequential
        /// project AND not the first incomplete task. The factory
        /// applies the `.queued` CSS class based on this; it never
        /// changes membership. Always `false` outside sequential
        /// project views.
        #[property(get, set)]
        pub queued: Cell<bool>,
        /// v0.5.0 (Slice B2) — hex colour of the area the task's
        /// project belongs to (e.g., `"#3584e4"`), or empty when
        /// the task is unfiled, the project has no area, or the
        /// area has no colour. The row factory reads this to apply
        /// the matching `.atrium-area-accent-{color}` CSS class for
        /// the 3 px left-border stripe.
        #[property(get, set)]
        pub area_color: RefCell<String>,
        /// State string for the v0.6.12 state-aware row treatment.
        /// One of overdue / today / upcoming, or empty for neutral
        /// and completed rows; the factory translates the value
        /// into a CSS class. See `classify_row_state`.
        #[property(get, set)]
        pub row_state: RefCell<String>,
        /// True when the underlying task has a non-NULL repeat_rule.
        /// The row factory shows a small ⟳ icon to the right of the
        /// title for repeating tasks (v0.6.14, Patch D polish).
        #[property(get, set)]
        pub repeating: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AtriumTaskInner {
        const NAME: &'static str = "AtriumTask";
        type Type = super::AtriumTask;
    }

    #[glib::derived_properties]
    impl ObjectImpl for AtriumTaskInner {}
}

glib::wrapper! {
    pub struct AtriumTask(ObjectSubclass<imp::AtriumTaskInner>);
}

impl AtriumTask {
    pub fn from_task(task: &Task) -> Self {
        Self::from_task_with_tags(task, &[])
    }

    /// Build an `AtriumTask` with the tag pills already populated.
    /// Phase 6b's row factory consumes the `tag_names_csv` property
    /// to render inline tag pills; v0.3.0 expanded the input shape
    /// to `(name, optional hex color)` so per-pill colours can land
    /// in the rendered Pango markup.
    pub fn from_task_with_tags(task: &Task, pills: &[(String, Option<String>)]) -> Self {
        let obj: Self = glib::Object::builder()
            .property("id", task.id)
            .property("uuid", task.uuid.clone())
            .build();
        obj.set_title(task.title.clone());
        obj.set_note(task.note.clone());
        obj.set_completed(task.completed_at.is_some());
        obj.set_schedule_label(format_schedule(&task.scheduled_for));
        obj.set_deadline_label(format_deadline(task.deadline));
        obj.set_position(task.position);
        obj.set_tag_names_csv(format_tag_names(pills));
        obj.set_row_state(classify_row_state(task));
        obj.set_repeating(task.repeat_rule.is_some());
        obj
    }

    /// Apply the latest fields from a refreshed `Task` row. Used by
    /// the diff applier when the worker emits an updated task.
    pub fn refresh_from(&self, task: &Task) {
        if self.title() != task.title {
            self.set_title(task.title.clone());
        }
        if self.note() != task.note {
            self.set_note(task.note.clone());
        }
        let new_completed = task.completed_at.is_some();
        if self.completed() != new_completed {
            self.set_completed(new_completed);
        }
        let new_schedule = format_schedule(&task.scheduled_for);
        if self.schedule_label() != new_schedule {
            self.set_schedule_label(new_schedule);
        }
        let new_deadline = format_deadline(task.deadline);
        if self.deadline_label() != new_deadline {
            self.set_deadline_label(new_deadline);
        }
        if (self.position() - task.position).abs() > f64::EPSILON {
            self.set_position(task.position);
        }
        let new_state = classify_row_state(task);
        if self.row_state() != new_state {
            self.set_row_state(new_state);
        }
        let new_repeating = task.repeat_rule.is_some();
        if self.repeating() != new_repeating {
            self.set_repeating(new_repeating);
        }
    }
}

/// Classify a task into a row-state string for the v0.6.12
/// state-aware row treatment. Returns `""` (neutral) for completed
/// tasks (the existing `.completed` CSS class already handles them)
/// and for tasks with no time anchor. Otherwise returns one of
/// `"overdue"`, `"today"`, or `"upcoming"`. The classifier reads
/// `chrono::Local` once per call — the row updates on every
/// `refresh_from`, which fires on the worker's task-change deltas.
///
/// Rules (mirrors the in-memory evaluator's state predicates and
/// the agenda's `classify`):
/// - Overdue: open AND deadline < today.
/// - Today: open AND most-imminent date == today (where most-
///   imminent = min(scheduled, deadline)).
/// - Upcoming: open AND most-imminent date > today (and within a
///   short window the eye reads as "soon").
/// - Neutral: completed, no time anchor, or scheduled-someday.
fn classify_row_state(task: &Task) -> String {
    use chrono::Local;
    if task.completed_at.is_some() {
        return String::new();
    }
    let today = Local::now().date_naive();
    if let Some(deadline) = task.deadline
        && deadline < today
    {
        return "overdue".into();
    }
    let scheduled_date = match &task.scheduled_for {
        Some(ScheduledFor::Date(d)) => Some(*d),
        _ => None,
    };
    let most_imminent = match (scheduled_date, task.deadline) {
        (Some(s), Some(d)) => Some(s.min(d)),
        (Some(s), None) => Some(s),
        (None, Some(d)) => Some(d),
        (None, None) => None,
    };
    let Some(date) = most_imminent else {
        return String::new();
    };
    if date == today {
        "today".into()
    } else if date > today {
        "upcoming".into()
    } else {
        // Past scheduled (no deadline) — treated as neutral; the
        // user already let it slide, no point flashing red on a
        // schedule that wasn't a hard commitment.
        String::new()
    }
}

/// Re-export of the task_list module's formatter so this module
/// doesn't duplicate the Pango-escape logic. Both call paths
/// (`from_task_with_tags` here, the diff-applier in `task_list`)
/// produce the same markup string.
fn format_tag_names(pills: &[(String, Option<String>)]) -> String {
    crate::ui::task_list::format_tag_names(pills)
}

fn format_schedule(s: &Option<ScheduledFor>) -> String {
    match s {
        None => String::new(),
        Some(ScheduledFor::Someday) => "Someday".to_string(),
        Some(ScheduledFor::Date(d)) => d.format("%b %-d").to_string(),
    }
}

fn format_deadline(d: Option<chrono::NaiveDate>) -> String {
    // The earlier alarm-clock emoji rendered inconsistently across
    // systems (some show it as a glyph, some as a typographic box,
    // some at the wrong baseline). A "Due " prefix reads the same
    // everywhere and lines up with the existing typography pass.
    d.map(|d| format!("Due {}", d.format("%b %-d")))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use atrium_core::test_support::dummy_task;
    use chrono::{NaiveDate, Utc};

    fn init() {
        // GObject initialisation is process-global; tests in this
        // module rely on it being live. `gtk::init` no-ops if already
        // initialised; a failure here means we're running headless.
        let _ = gtk::init();
    }

    #[test]
    fn from_task_round_trips_basic_fields() {
        init();
        let mut t = dummy_task(42);
        t.title = "buy milk".into();
        let obj = AtriumTask::from_task(&t);
        assert_eq!(obj.id(), 42);
        assert_eq!(obj.title(), "buy milk");
        assert!(!obj.completed());
        assert_eq!(obj.schedule_label(), "");
    }

    #[test]
    fn schedule_label_renders_someday_and_date() {
        init();
        let mut t = dummy_task(1);
        t.scheduled_for = Some(ScheduledFor::Someday);
        let obj = AtriumTask::from_task(&t);
        assert_eq!(obj.schedule_label(), "Someday");

        t.scheduled_for = Some(ScheduledFor::Date(
            NaiveDate::from_ymd_opt(2026, 5, 15).unwrap(),
        ));
        let obj = AtriumTask::from_task(&t);
        assert_eq!(obj.schedule_label(), "May 15");
    }

    #[test]
    fn refresh_from_updates_title_and_completed() {
        init();
        let t = dummy_task(7);
        let obj = AtriumTask::from_task(&t);
        assert_eq!(obj.title(), "t7");

        let mut t2 = t.clone();
        t2.title = "renamed".into();
        t2.completed_at = Some(Utc::now());
        obj.refresh_from(&t2);
        assert_eq!(obj.title(), "renamed");
        assert!(obj.completed());
    }
}
