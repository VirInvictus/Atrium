// SPDX-License-Identifier: MIT
//! Builder Mode Forecast view (Phase 12).
//!
//! 30-day calendar-axis layout. A scrollable column of day cards;
//! each card shows the open tasks that touch that day via
//! `scheduled_for`, `deadline`, or `defer_until`. An Overdue
//! pseudo-block at the top surfaces past-due open tasks so they
//! don't disappear off the bottom of the world.
//!
//! Phase 12 is purely UI on top of two new read queries
//! (`list_forecast`, `list_overdue`); the schema is unchanged.
//! Drag-to-reschedule writes `scheduled_for` via the worker — the
//! same column the Inspector touches.

use adw::prelude::*;
use atrium_core::{ScheduledFor, Task, TaskUpdate, WorkerHandle};
use chrono::{Datelike, NaiveDate};
use gtk::glib;
use gtk::{gdk, pango};
use tracing::error;

/// How many days the Forecast surface shows from today inclusive.
/// Spec §4.2 documents the 30-day window; OmniFocus uses ~14 days
/// by default but our spec settled on 30. Tunable later via
/// GSettings.
pub const FORECAST_WINDOW_DAYS: i64 = 30;

/// Reason a task appears in a given day card. A task with both a
/// schedule and a deadline in the window shows up under each day
/// it touches with the matching reason — e.g., "Scheduled" today
/// + "Deadline" three days out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Scheduled,
    Deadline,
    DeferEnds,
}

impl Reason {
    fn label(self) -> &'static str {
        match self {
            Self::Scheduled => "Scheduled",
            Self::Deadline => "Deadline",
            Self::DeferEnds => "Defer ends",
        }
    }

    fn css(self) -> &'static str {
        match self {
            Self::Scheduled => "atrium-forecast-reason-scheduled",
            Self::Deadline => "atrium-forecast-reason-deadline",
            Self::DeferEnds => "atrium-forecast-reason-defer",
        }
    }
}

/// One row in a day card (or in the Overdue block). A single task
/// can produce multiple `DayEntry`s — one per reason it touches a
/// date.
#[derive(Debug, Clone)]
pub struct DayEntry {
    pub task: Task,
    pub reason: Reason,
}

/// Group `tasks` by the date(s) they touch in the
/// `[today, today + days]` window. Each task appears under every
/// matching reason (scheduled / deadline / defer ends). The result
/// is a vector aligned with `today..=today + days`, one entry per
/// day. Days with no tasks are present as empty vectors so the UI
/// can render the full window grid.
pub fn group_by_date(
    tasks: &[Task],
    today: NaiveDate,
    days: i64,
) -> Vec<(NaiveDate, Vec<DayEntry>)> {
    let mut out: Vec<(NaiveDate, Vec<DayEntry>)> = (0..=days)
        .map(|i| (today + chrono::Duration::days(i), Vec::new()))
        .collect();

    for task in tasks {
        // Scheduled date (skip Someday sentinel — Someday is a
        // state, not a date, and shouldn't reach Forecast input
        // anyway since list_forecast filters it out).
        if let Some(ScheduledFor::Date(d)) = task.scheduled_for
            && let Some(slot) = out.iter_mut().find(|(date, _)| *date == d)
        {
            slot.1.push(DayEntry {
                task: task.clone(),
                reason: Reason::Scheduled,
            });
        }
        if let Some(d) = task.deadline
            && let Some(slot) = out.iter_mut().find(|(date, _)| *date == d)
        {
            slot.1.push(DayEntry {
                task: task.clone(),
                reason: Reason::Deadline,
            });
        }
        if let Some(d) = task.defer_until
            && let Some(slot) = out.iter_mut().find(|(date, _)| *date == d)
        {
            slot.1.push(DayEntry {
                task: task.clone(),
                reason: Reason::DeferEnds,
            });
        }
    }

    out
}

/// Build the Forecast page widget. Returns a scrollable container
/// that the window mounts into the content stack's "forecast"
/// page. Drag-to-reschedule on each day card writes
/// `scheduled_for` via the supplied `worker`.
pub fn build_page(
    today: NaiveDate,
    forecast_tasks: &[Task],
    overdue_tasks: &[Task],
    worker: Option<WorkerHandle>,
) -> gtk::Widget {
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(16)
        .build();

    body.append(&build_overdue_block(overdue_tasks));

    let groups = group_by_date(forecast_tasks, today, FORECAST_WINDOW_DAYS);
    for (date, entries) in groups {
        body.append(&build_day_card(date, &entries, today, worker.clone()));
    }

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&body)
        .build();

    scroller.upcast()
}

fn build_overdue_block(tasks: &[Task]) -> gtk::Widget {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    card.add_css_class("atrium-forecast-overdue");
    card.add_css_class("card");

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(10)
        .margin_bottom(2)
        .build();

    let title = gtk::Label::builder()
        .label("Overdue")
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build();
    title.add_css_class("heading");
    header.append(&title);

    let count_label = gtk::Label::builder()
        .label(count_text(tasks.len()))
        .halign(gtk::Align::End)
        .build();
    count_label.add_css_class("dim-label");
    count_label.add_css_class("numeric");
    header.append(&count_label);

    card.append(&header);

    if tasks.is_empty() {
        let caught_up = gtk::Label::builder()
            .label("Caught up.")
            .halign(gtk::Align::Start)
            .margin_start(12)
            .margin_end(12)
            .margin_top(2)
            .margin_bottom(12)
            .build();
        caught_up.add_css_class("dim-label");
        card.append(&caught_up);
    } else {
        for t in tasks {
            // Pick the most-overdue reason: deadline trumps
            // scheduled when both are past; defer is always within
            // window (overdue list excludes deferred-future).
            let reason = if t.deadline.is_some() {
                Reason::Deadline
            } else {
                Reason::Scheduled
            };
            let entry = DayEntry {
                task: t.clone(),
                reason,
            };
            card.append(&build_entry_row(&entry));
        }
        // Bottom padding inside the card.
        let pad = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .height_request(8)
            .build();
        card.append(&pad);
    }

    card.upcast()
}

fn build_day_card(
    date: NaiveDate,
    entries: &[DayEntry],
    today: NaiveDate,
    worker: Option<WorkerHandle>,
) -> gtk::Widget {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    card.add_css_class("atrium-forecast-day");
    card.add_css_class("card");
    if date == today {
        card.add_css_class("today");
    }

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(10)
        .margin_bottom(2)
        .build();

    let title = gtk::Label::builder()
        .label(format_day_title(date, today))
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build();
    title.add_css_class("heading");
    header.append(&title);

    if !entries.is_empty() {
        let count = gtk::Label::builder()
            .label(entries.len().to_string())
            .halign(gtk::Align::End)
            .build();
        count.add_css_class("dim-label");
        count.add_css_class("numeric");
        header.append(&count);
    }
    card.append(&header);

    if entries.is_empty() {
        let blank = gtk::Label::builder()
            .label("—")
            .halign(gtk::Align::Start)
            .margin_start(12)
            .margin_end(12)
            .margin_top(2)
            .margin_bottom(12)
            .build();
        blank.add_css_class("dim-label");
        card.append(&blank);
    } else {
        for entry in entries {
            card.append(&build_entry_row(entry));
        }
        let pad = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .height_request(8)
            .build();
        card.append(&pad);
    }

    // Drop target — schedule the dragged task to this date.
    if let Some(worker) = worker {
        let drop_target = gtk::DropTarget::new(i64::static_type(), gdk::DragAction::MOVE);
        let target_date = date;
        drop_target.connect_drop(move |_, value, _, _| {
            let Ok(task_id) = value.get::<i64>() else {
                return false;
            };
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_task(
                        TaskUpdate::new(task_id).schedule(Some(ScheduledFor::Date(target_date))),
                    )
                    .await
                {
                    error!(?e, task_id, ?target_date, "forecast drop failed");
                }
            });
            true
        });
        card.add_controller(drop_target);
    }

    card.upcast()
}

/// Compact entry row for a forecast day card. Layout:
/// `[reason-chip] [title (ellipsised)] [tags? — future polish]`
fn build_entry_row(entry: &DayEntry) -> gtk::Widget {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_start(12)
        .margin_end(12)
        .margin_top(2)
        .margin_bottom(2)
        .build();
    row.add_css_class("atrium-forecast-row");

    let reason = gtk::Label::builder()
        .label(entry.reason.label())
        .halign(gtk::Align::Start)
        .build();
    reason.add_css_class("atrium-forecast-reason");
    reason.add_css_class(entry.reason.css());
    row.append(&reason);

    let title = gtk::Label::builder()
        .label(&entry.task.title)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(pango::EllipsizeMode::End)
        .xalign(0.0)
        .build();
    title.add_css_class("atrium-forecast-row-title");
    if entry.task.is_completed() {
        title.add_css_class("dim-label");
    }
    row.append(&title);

    // Drag source — carry the task id so a day card's drop target
    // can reschedule.
    let drag_source = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    let id = entry.task.id;
    drag_source
        .connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&id.to_value())));
    row.add_controller(drag_source);

    row.upcast()
}

fn format_day_title(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        return format!("Today · {}", date.format("%a %b %-d"));
    }
    if date == today + chrono::Duration::days(1) {
        return format!("Tomorrow · {}", date.format("%a %b %-d"));
    }
    // Within the same year: drop the year for less noise. Across
    // a year boundary: include it.
    if date.year() == today.year() {
        date.format("%A · %b %-d").to_string()
    } else {
        date.format("%A · %b %-d, %Y").to_string()
    }
}

fn count_text(n: usize) -> String {
    match n {
        0 => "0 overdue".to_string(),
        1 => "1 overdue".to_string(),
        n => format!("{n} overdue"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn task(id: i64, scheduled: Option<NaiveDate>, deadline: Option<NaiveDate>) -> Task {
        Task {
            id,
            uuid: format!("u{id}"),
            title: format!("t{id}"),
            note: String::new(),
            project_id: None,
            parent_id: None,
            scheduled_for: scheduled.map(ScheduledFor::Date),
            deadline,
            defer_until: None,
            estimated_minutes: None,
            completed_at: None,
            repeat_rule: None,
            position: id as f64,
            created_at: Utc::now(),
            modified_at: Utc::now(),
        }
    }

    #[test]
    fn group_by_date_buckets_per_day() {
        let today = d(2026, 5, 15);
        let tasks = vec![
            task(1, Some(d(2026, 5, 16)), None),
            task(2, None, Some(d(2026, 5, 16))),
            task(3, Some(d(2026, 5, 18)), None),
        ];
        let grouped = group_by_date(&tasks, today, 7);
        // 7-day window plus today inclusive → 8 slots.
        assert_eq!(grouped.len(), 8);
        // Day +1 has tasks 1 and 2; day +3 has task 3.
        let day_plus_1 = &grouped[1].1;
        assert_eq!(day_plus_1.len(), 2);
        let day_plus_3 = &grouped[3].1;
        assert_eq!(day_plus_3.len(), 1);
        assert_eq!(day_plus_3[0].task.id, 3);
    }

    #[test]
    fn task_with_both_schedule_and_deadline_appears_twice() {
        // Same task scheduled today and due in 3 days appears
        // under each day with the matching reason.
        let today = d(2026, 5, 15);
        let t = task(1, Some(today), Some(today + chrono::Duration::days(3)));
        let grouped = group_by_date(&[t], today, 7);
        let scheduled_entries = &grouped[0].1;
        let deadline_entries = &grouped[3].1;
        assert_eq!(scheduled_entries.len(), 1);
        assert_eq!(scheduled_entries[0].reason, Reason::Scheduled);
        assert_eq!(deadline_entries.len(), 1);
        assert_eq!(deadline_entries[0].reason, Reason::Deadline);
    }

    #[test]
    fn defer_ends_lands_under_target_date() {
        let today = d(2026, 5, 15);
        let mut t = task(1, None, None);
        t.defer_until = Some(today + chrono::Duration::days(2));
        let grouped = group_by_date(&[t], today, 7);
        let day = &grouped[2].1;
        assert_eq!(day.len(), 1);
        assert_eq!(day[0].reason, Reason::DeferEnds);
    }

    #[test]
    fn empty_window_is_full_of_empties() {
        let today = d(2026, 5, 15);
        let grouped = group_by_date(&[], today, 7);
        assert_eq!(grouped.len(), 8);
        for (_, entries) in grouped {
            assert!(entries.is_empty());
        }
    }

    #[test]
    fn day_titles_promote_today_and_tomorrow() {
        let today = d(2026, 5, 15);
        assert!(format_day_title(today, today).starts_with("Today"));
        assert!(format_day_title(today + chrono::Duration::days(1), today).starts_with("Tomorrow"));
        // Far future — uses weekday name.
        let later = d(2026, 5, 25);
        assert!(!format_day_title(later, today).starts_with("Today"));
        assert!(!format_day_title(later, today).starts_with("Tomorrow"));
    }

    #[test]
    fn count_text_pluralises() {
        assert_eq!(count_text(0), "0 overdue");
        assert_eq!(count_text(1), "1 overdue");
        assert_eq!(count_text(7), "7 overdue");
    }
}
