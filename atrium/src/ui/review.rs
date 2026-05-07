// SPDX-License-Identifier: MIT
//! Builder Mode Review queue (Phase 13).
//!
//! Projects with a `review_interval_days` set surface here when
//! their last review is older than the interval allows. Each card
//! shows the project's title, area (when filed), how stale the
//! review is, and a *Mark Reviewed* button that stamps
//! `last_reviewed_at = now()` and drops the row out of the queue.
//!
//! Phase 13 is purely UI on top of two new data-layer pieces:
//! `read::list_review_queue` (the SELECT) and `Worker::mark_reviewed`
//! (the UPDATE). Schema unchanged — `review_interval_days` and
//! `last_reviewed_at` have lived in the project table since
//! Phase 1's superset migration.

use std::collections::HashMap;

use adw::prelude::*;
use atrium_core::{Project, WorkerHandle};
use chrono::NaiveDate;
use gtk::glib;
use tracing::error;

/// Build the Review page widget. Returns a scrollable container
/// the window mounts into the content stack's `review` page.
/// Empty queue gets an `AdwStatusPage` "No projects due for
/// review" placeholder.
pub fn build_page(
    today: NaiveDate,
    queue: &[Project],
    area_titles: &HashMap<i64, String>,
    worker: Option<WorkerHandle>,
) -> gtk::Widget {
    if queue.is_empty() {
        let status = adw::StatusPage::builder()
            .icon_name("checkmark-symbolic")
            .title("All caught up")
            .description("No projects are due for review right now.")
            .build();
        return status.upcast();
    }

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(16)
        .build();

    for project in queue {
        body.append(&build_project_card(
            project,
            today,
            area_titles,
            worker.clone(),
        ));
    }

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&body)
        .build();
    scroller.upcast()
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
        .label("Mark Reviewed")
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
        Some(area) => format!("{area} · {review_chunk}"),
        None => review_chunk,
    }
}

/// "Last reviewed N days ago" / "Never reviewed" / "Last reviewed
/// today" formatter. Days are calendar-day diff between
/// `last_reviewed_at`'s date portion and `today`.
pub fn format_last_reviewed(project: &Project, today: NaiveDate) -> String {
    match project.last_reviewed_at {
        None => "Never reviewed".to_string(),
        Some(last) => {
            let last_date = last.date_naive();
            let days = (today - last_date).num_days();
            match days {
                d if d < 0 => "Last reviewed in the future".to_string(),
                0 => "Last reviewed today".to_string(),
                1 => "Last reviewed 1 day ago".to_string(),
                d => format!("Last reviewed {d} days ago"),
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
