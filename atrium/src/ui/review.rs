// SPDX-License-Identifier: MIT
//! Builder Mode Review queue (Phase 13) + canonical Weekly Walk (v0.7.2).
//!
//! The Review page renders two sections in one surface:
//!
//! 1. **Projects to review** — projects whose `review_interval_days`
//!    has elapsed since `last_reviewed_at`. Each card shows the
//!    project's title, area (when filed), how stale the review is,
//!    and a *Mark Reviewed* button that stamps `last_reviewed_at =
//!    now()` and drops the row out of the queue.
//!
//! 2. **This week** — open tasks matching the
//!    `REVIEW_WEEKLY_WALK_FILTER` expression: anything overdue,
//!    anything scheduled this week, deadlines reaching next week,
//!    or tasks just freed from a defer. Compact rows reuse the
//!    `agenda::build_row` treatment; clicking a row opens the
//!    Inspector for that task.
//!
//! v0.7.2 merged what used to be a separately-seeded "Weekly
//! Review" Perspective into this canonical surface, killing the
//! confusion of having "Review" and "Weekly Review" both present
//! and showing different things.

use std::collections::HashMap;

use atrium_core::{Project, Task, WorkerHandle};
use chrono::NaiveDate;
use gtk::glib;
use gtk::prelude::*;
use tracing::error;

use crate::i18n::{gettext, gettext_f, ngettext_f};
use crate::ui::agenda;
use crate::ui::task_list::TagPillMap;

/// Build the Review page widget. Returns a scrollable container
/// the window mounts into the content stack's `review` page.
///
/// The page renders up to two sections — Projects to review
/// (the `queue` argument; Phase 13's list_review_queue) and This
/// week (the `weekly_tasks` argument; REVIEW_WEEKLY_WALK_FILTER,
/// excluding tasks marked reviewed in the last 7 days). If both
/// are empty, an owned status-page "All caught up" placeholder
/// shows instead.
#[allow(clippy::too_many_arguments)]
pub fn build_page<F, G>(
    today: NaiveDate,
    queue: &[Project],
    weekly_tasks: &[Task],
    project_titles: &HashMap<i64, String>,
    area_titles: &HashMap<i64, String>,
    tag_pills: &TagPillMap,
    worker: Option<WorkerHandle>,
    on_row_click: F,
    on_mark_reviewed: G,
) -> gtk::Widget
where
    F: Fn(i64) + 'static + Clone,
    G: Fn(i64) + 'static + Clone,
{
    if queue.is_empty() && weekly_tasks.is_empty() {
        return crate::ui::status_page::status_page(
            Some("checkmark-symbolic"),
            &gettext("All caught up"),
            Some(&gettext(
                "No projects need review and nothing is pressing this week.",
            )),
        )
        .widget()
        .clone();
    }

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(16)
        .build();

    // Section 1 — Projects to review.
    body.append(&build_queue_section(queue, today, area_titles, worker));

    // Section 2 — This week (Weekly Walk).
    body.append(&build_weekly_section(
        weekly_tasks,
        project_titles,
        tag_pills,
        on_row_click,
        on_mark_reviewed,
    ));

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&body)
        .build();
    scroller.upcast()
}

/// "Projects to review" section. Renders the existing project
/// cards when the queue has rows, or a quiet inline note when it
/// doesn't. The note shape is intentional — it tells the user
/// nothing actionable is here right now without pretending the
/// section doesn't exist.
fn build_queue_section(
    queue: &[Project],
    today: NaiveDate,
    area_titles: &HashMap<i64, String>,
    worker: Option<WorkerHandle>,
) -> gtk::Widget {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    section.append(&build_section_header(&gettext("Projects to review")));

    if queue.is_empty() {
        section.append(&build_inline_note(&gettext(
            "No projects are due for review right now.",
        )));
    } else {
        for project in queue {
            section.append(&build_project_card(
                project,
                today,
                area_titles,
                worker.clone(),
            ));
        }
    }
    section.upcast()
}

/// "This week" section. Each row reuses `agenda::build_row` for
/// the title + breadcrumb + date layout, wrapped in a horizontal
/// box that adds a trailing **Mark Reviewed** button. Clicking
/// the button dispatches `worker.mark_task_reviewed`; the row
/// drops out via the TaskChanges-driven page rebuild and stays
/// hidden from the weekly walk for 7 days.
fn build_weekly_section<F, G>(
    weekly_tasks: &[Task],
    project_titles: &HashMap<i64, String>,
    tag_pills: &TagPillMap,
    on_row_click: F,
    on_mark_reviewed: G,
) -> gtk::Widget
where
    F: Fn(i64) + 'static + Clone,
    G: Fn(i64) + 'static + Clone,
{
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    section.append(&build_section_header(&gettext("This week")));
    section.append(&build_inline_note(&gettext(
        "Mark items reviewed to hide them for 7 days.",
    )));

    if weekly_tasks.is_empty() {
        section.append(&build_inline_note(&gettext("Nothing pressing this week.")));
    } else {
        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .build();
        for task in weekly_tasks {
            list.append(&build_review_task_row(
                task,
                project_titles,
                tag_pills,
                on_row_click.clone(),
                on_mark_reviewed.clone(),
            ));
        }
        section.append(&list);
    }
    section.upcast()
}

/// v0.7.4 — single weekly-walk row. Wraps an `agenda::build_row`
/// (the visual content) in a horizontal box with a trailing
/// `Mark Reviewed` button. Two independent click paths: the
/// agenda body fires `on_row_click(id)` (opens Inspector); the
/// button fires `on_mark_reviewed(id)`.
fn build_review_task_row<F, G>(
    task: &Task,
    project_titles: &HashMap<i64, String>,
    tag_pills: &TagPillMap,
    on_row_click: F,
    on_mark_reviewed: G,
) -> gtk::Widget
where
    F: Fn(i64) + 'static,
    G: Fn(i64) + 'static,
{
    let body = agenda::build_row(task, project_titles, tag_pills, on_row_click);
    body.set_hexpand(true);

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    row.append(&body);

    let mark_button = gtk::Button::builder()
        .label(gettext("Mark Reviewed"))
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("Hide from the weekly walk for 7 days"))
        .build();
    let task_id = task.id;
    mark_button.connect_clicked(move |_| {
        on_mark_reviewed(task_id);
    });
    row.append(&mark_button);

    row.upcast()
}

fn build_section_header(text: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .build();
    label.add_css_class("heading");
    label.add_css_class("atrium-review-section-header");
    label
}

fn build_inline_note(text: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .margin_start(6)
        .build();
    label.add_css_class("dim-label");
    label.add_css_class("caption");
    label
}

fn build_project_card(
    project: &Project,
    today: NaiveDate,
    area_titles: &HashMap<i64, String>,
    worker: Option<WorkerHandle>,
) -> gtk::Widget {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .build();
    card.add_css_class("atrium-review-card");
    card.add_css_class("card");

    // Left column: title + subtitle (area · due-by-N-days).
    let info = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();

    let title = gtk::Label::builder()
        .label(&project.title)
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .xalign(0.0)
        .build();
    title.add_css_class("heading");
    title.add_css_class("atrium-review-card-title");
    info.append(&title);

    let subtitle_text = format_subtitle(project, today, area_titles);
    let subtitle = gtk::Label::builder()
        .label(&subtitle_text)
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .build();
    subtitle.add_css_class("dim-label");
    subtitle.add_css_class("caption");
    info.append(&subtitle);

    card.append(&info);

    // Right column: Mark Reviewed button.
    let button = gtk::Button::builder()
        .label(gettext("Mark Reviewed"))
        .css_classes(["suggested-action"])
        .valign(gtk::Align::Center)
        .build();
    if let Some(worker) = worker {
        let project_id = project.id;
        button.connect_clicked(move |btn| {
            // Disable while in flight to avoid double-clicks
            // firing two MarkReviewed commands.
            btn.set_sensitive(false);
            let worker = worker.clone();
            let btn = btn.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker.mark_reviewed(project_id).await {
                    error!(?e, project_id, "mark_reviewed failed");
                    // Re-enable on failure so the user can retry.
                    btn.set_sensitive(true);
                }
                // On success, the LibraryChanges delta triggers a
                // page rebuild that drops this row entirely; no
                // need to re-enable.
            });
        });
    } else {
        button.set_sensitive(false);
    }
    card.append(&button);

    card.upcast()
}

/// Build the subtitle for a Review card. Format:
/// `<Area> · Last reviewed N days ago` or `<Area> · Never reviewed`.
/// Unfiled projects skip the leading area chunk.
pub fn format_subtitle(
    project: &Project,
    today: NaiveDate,
    area_titles: &HashMap<i64, String>,
) -> String {
    let area_chunk = project.area_id.and_then(|id| area_titles.get(&id)).cloned();
    let review_chunk = format_last_reviewed(project, today);
    match area_chunk {
        // Translators: review-card subtitle; {area} is an area name and
        // {reviewed} an already-translated "Last reviewed …" phrase.
        Some(area) => gettext_f(
            "{area} · {reviewed}",
            &[("area", &area), ("reviewed", &review_chunk)],
        ),
        None => review_chunk,
    }
}

/// "Last reviewed N days ago" / "Never reviewed" / "Last reviewed
/// today" formatter. Days are calendar-day diff between
/// `last_reviewed_at`'s date portion and `today`.
pub fn format_last_reviewed(project: &Project, today: NaiveDate) -> String {
    match project.last_reviewed_at {
        None => gettext("Never reviewed"),
        Some(last) => {
            let last_date = last.date_naive();
            let days = (today - last_date).num_days();
            match days {
                d if d < 0 => gettext("Last reviewed in the future"),
                0 => gettext("Last reviewed today"),
                d => ngettext_f(
                    "Last reviewed {n} day ago",
                    "Last reviewed {n} days ago",
                    d as u32,
                    &[("n", &d.to_string())],
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn project_with_review(last_reviewed_at: Option<DateTime<Utc>>) -> Project {
        Project {
            id: 1,
            uuid: "uuid".into(),
            title: "Project".into(),
            note: String::new(),
            area_id: None,
            sequential: false,
            review_interval_days: Some(7),
            last_reviewed_at,
            archived_at: None,
            position: 0.0,
            created_at: Utc::now(),
            modified_at: Utc::now(),
        }
    }

    #[test]
    fn last_reviewed_never() {
        let p = project_with_review(None);
        assert_eq!(format_last_reviewed(&p, d(2026, 5, 15)), "Never reviewed");
    }

    #[test]
    fn last_reviewed_today() {
        let p = project_with_review(Some(Utc.with_ymd_and_hms(2026, 5, 15, 8, 0, 0).unwrap()));
        assert_eq!(
            format_last_reviewed(&p, d(2026, 5, 15)),
            "Last reviewed today"
        );
    }

    #[test]
    fn last_reviewed_one_day_singular() {
        let p = project_with_review(Some(Utc.with_ymd_and_hms(2026, 5, 14, 8, 0, 0).unwrap()));
        assert_eq!(
            format_last_reviewed(&p, d(2026, 5, 15)),
            "Last reviewed 1 day ago"
        );
    }

    #[test]
    fn last_reviewed_n_days_plural() {
        let p = project_with_review(Some(Utc.with_ymd_and_hms(2026, 5, 1, 8, 0, 0).unwrap()));
        assert_eq!(
            format_last_reviewed(&p, d(2026, 5, 15)),
            "Last reviewed 14 days ago"
        );
    }

    #[test]
    fn subtitle_includes_area_when_filed() {
        let mut p = project_with_review(Some(Utc.with_ymd_and_hms(2026, 5, 1, 8, 0, 0).unwrap()));
        p.area_id = Some(42);
        let mut areas = HashMap::new();
        areas.insert(42_i64, "Work".to_string());
        let subtitle = format_subtitle(&p, d(2026, 5, 15), &areas);
        assert_eq!(subtitle, "Work · Last reviewed 14 days ago");
    }

    #[test]
    fn subtitle_skips_area_when_unfiled() {
        let p = project_with_review(None);
        let areas = HashMap::new();
        let subtitle = format_subtitle(&p, d(2026, 5, 15), &areas);
        assert_eq!(subtitle, "Never reviewed");
    }
}
