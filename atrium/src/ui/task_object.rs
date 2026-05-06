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

    /// Build an `AtriumTask` with the tag names already populated.
    /// Phase 6b's row factory consumes the `tag_names_csv` property
    /// to render inline tag pills.
    pub fn from_task_with_tags(task: &Task, tag_names: &[String]) -> Self {
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
        obj.set_tag_names_csv(format_tag_names(tag_names));
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
    }
}

fn format_tag_names(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("#{n}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_schedule(s: &Option<ScheduledFor>) -> String {
    match s {
        None => String::new(),
        Some(ScheduledFor::Someday) => "Someday".to_string(),
        Some(ScheduledFor::Date(d)) => d.format("%b %-d").to_string(),
    }
}

fn format_deadline(d: Option<chrono::NaiveDate>) -> String {
    d.map(|d| format!("⏰ {}", d.format("%b %-d")))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};

    fn dummy_task(id: i64) -> Task {
        Task {
            id,
            uuid: format!("uuid-{id}"),
            title: format!("Task {id}"),
            note: String::new(),
            project_id: None,
            parent_id: None,
            scheduled_for: None,
            deadline: None,
            defer_until: None,
            estimated_minutes: None,
            completed_at: None,
            repeat_rule: None,
            position: id as f64,
            created_at: Utc::now(),
            modified_at: Utc::now(),
        }
    }

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
        assert_eq!(obj.title(), "Task 7");

        let mut t2 = t.clone();
        t2.title = "renamed".into();
        t2.completed_at = Some(Utc::now());
        obj.refresh_from(&t2);
        assert_eq!(obj.title(), "renamed");
        assert!(obj.completed());
    }
}
