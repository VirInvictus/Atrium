// SPDX-License-Identifier: MIT
//! Logbook page (Phase 15.75 Slice C2).
//!
//! Replaces the bare-list rendering of `ActiveList::Logbook` with a
//! day-band layout: completed tasks are grouped into Today /
//! Yesterday / Last 7 Days / Older sections. The bands match Things
//! 3's Logbook shape and answer the most common Logbook question
//! ("what did I get done recently?") without hand-scrolling through
//! a flat reverse-chrono list.
//!
//! Read-only by design — the rows display title, project context,
//! tags, and completion date but don't carry the regular row
//! factory's checkbox / inline edit / drag-reorder controls. To
//! re-open a completed task the user goes through the Inspector
//! or Ctrl+Z immediately after the toggle. A right-click "Reopen"
//! context menu is a follow-up patch.
//!
//! The grouping function lives at module level so it's pure-Rust
//! testable; the GTK build path is just the renderer on top.
//!
//! `build_page` mounts into the window's `logbook_host` AdwBin via
//! `refresh_logbook_page` in `window.rs`.

use std::collections::HashMap;

use adw::prelude::*;
use atrium_core::Task;
use chrono::{Duration, NaiveDate};
use gtk::pango;

use crate::i18n::{gettext, gettext_f};
use crate::ui::task_list::TagPillMap;

/// One band in the Logbook's date-grouped layout. Bands are
/// chronological in display order: Today (newest) → Older (oldest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DateBand {
    Today,
    Yesterday,
    LastSevenDays,
    Older,
}

impl DateBand {
    /// Display heading for the band — matches Things 3's vocabulary
    /// rather than ISO dates so the page reads like a journal.
    pub fn heading(self) -> String {
        match self {
            Self::Today => gettext("Today"),
            Self::Yesterday => gettext("Yesterday"),
            Self::LastSevenDays => gettext("Last 7 Days"),
            Self::Older => gettext("Older"),
        }
    }

    /// Display order — Today first, Older last. Used by the renderer
    /// to walk bands in a consistent sequence.
    pub fn ordered() -> [Self; 4] {
        [
            Self::Today,
            Self::Yesterday,
            Self::LastSevenDays,
            Self::Older,
        ]
    }
}

/// Classify a completion date relative to `today` into one of the
/// four bands. The boundaries are inclusive: a task completed
/// exactly 7 days ago lands in `LastSevenDays`, exactly 8 in
/// `Older`. Future dates (which shouldn't appear on completed tasks
/// in the wild but might in test fixtures) fold into `Today`.
pub fn classify_band(completed_on: NaiveDate, today: NaiveDate) -> DateBand {
    if completed_on >= today {
        return DateBand::Today;
    }
    let yesterday = today - Duration::days(1);
    if completed_on == yesterday {
        return DateBand::Yesterday;
    }
    let week_ago = today - Duration::days(7);
    if completed_on >= week_ago {
        return DateBand::LastSevenDays;
    }
    DateBand::Older
}

/// Group `tasks` (assumed completed; `completed_at` should be set)
/// by date band. Tasks without a `completed_at` are dropped — they
/// shouldn't reach Logbook in the first place. Within each band
/// tasks stay in input order; the caller is expected to pre-sort by
/// `completed_at` descending.
pub fn group_by_band(tasks: &[Task], today: NaiveDate) -> Vec<(DateBand, Vec<Task>)> {
    let mut buckets: HashMap<DateBand, Vec<Task>> = HashMap::new();
    for task in tasks {
        let Some(completed) = task.completed_at else {
            continue;
        };
        let band = classify_band(completed.date_naive(), today);
        buckets.entry(band).or_default().push(task.clone());
    }
    DateBand::ordered()
        .into_iter()
        .filter_map(|band| {
            let rows = buckets.remove(&band)?;
            (!rows.is_empty()).then_some((band, rows))
        })
        .collect()
}

/// Build the Logbook page widget. Empty input gets an
/// `AdwStatusPage` "Nothing logged yet" placeholder that mirrors
/// the canonical empty-state copy used elsewhere.
pub fn build_page(
    today: NaiveDate,
    tasks: &[Task],
    project_titles: &HashMap<i64, String>,
    project_areas: &HashMap<i64, Option<i64>>,
    area_titles: &HashMap<i64, String>,
    tag_pills: &TagPillMap,
) -> gtk::Widget {
    if tasks.is_empty() {
        let status = adw::StatusPage::builder()
            .icon_name("document-open-recent-symbolic")
            .title(gettext("Nothing logged yet"))
            .description(gettext(
                "Completed tasks settle here, grouped by when you finished them.",
            ))
            .build();
        return status.upcast();
    }

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(16)
        .build();

    for (band, rows) in group_by_band(tasks, today) {
        body.append(&build_section(
            band,
            &rows,
            project_titles,
            project_areas,
            area_titles,
            tag_pills,
        ));
    }

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&body)
        .build();
    scroller.upcast()
}

fn build_section(
    band: DateBand,
    rows: &[Task],
    project_titles: &HashMap<i64, String>,
    project_areas: &HashMap<i64, Option<i64>>,
    area_titles: &HashMap<i64, String>,
    tag_pills: &TagPillMap,
) -> gtk::Widget {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();

    // Section heading — bold-ish, slightly larger than caption,
    // sits at the top of the band. Same `.atrium-sidebar-section`
    // CSS class would be too small here; lean on libadwaita's
    // `.title-4` instead for parity with Forecast's day headings.
    let heading_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let heading = gtk::Label::builder()
        .label(band.heading())
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build();
    heading.add_css_class("title-4");
    heading_box.append(&heading);

    // Per-band count badge — quick "I did 14 things yesterday"
    // signal without scanning the rows.
    let count = gtk::Label::builder()
        .label(rows.len().to_string())
        .halign(gtk::Align::End)
        .build();
    count.add_css_class("dim-label");
    count.add_css_class("numeric");
    heading_box.append(&count);
    section.append(&heading_box);

    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    separator.add_css_class("atrium-logbook-rule");
    section.append(&separator);

    for task in rows {
        section.append(&build_task_row(
            task,
            project_titles,
            project_areas,
            area_titles,
            tag_pills,
        ));
    }

    section.upcast()
}

fn build_task_row(
    task: &Task,
    project_titles: &HashMap<i64, String>,
    project_areas: &HashMap<i64, Option<i64>>,
    area_titles: &HashMap<i64, String>,
    tag_pills: &TagPillMap,
) -> gtk::Widget {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(2)
        .margin_bottom(2)
        .build();
    row.add_css_class("atrium-logbook-row");

    // Title with strikethrough — completed-task convention. Pango
    // markup keeps the strikethrough on a single label without
    // needing a separate widget.
    let title = gtk::Label::builder()
        .use_markup(true)
        .label(format!("<s>{}</s>", pango_escape(&task.title)))
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(pango::EllipsizeMode::End)
        .xalign(0.0)
        .build();
    title.add_css_class("atrium-logbook-title");
    title.add_css_class("dim-label");
    row.append(&title);

    // Tag pills (compact) — same shape as the main list rows so the
    // visual link between task lists is obvious. Coloured per-tag
    // when the tag carries a hex.
    if let Some(pills) = tag_pills.get(&task.id)
        && !pills.is_empty()
    {
        let tags = gtk::Label::builder()
            .use_markup(true)
            .label(crate::ui::task_list::format_tag_names(pills))
            .halign(gtk::Align::End)
            .build();
        tags.add_css_class("atrium-task-tags");
        tags.add_css_class("dim-label");
        row.append(&tags);
    }

    // Area › Project context chip on the right. Mirrors the
    // cross-list chip the regular task rows render. Quietly empty
    // when the task is unfiled (Inbox).
    let context = build_context_label(task, project_titles, project_areas, area_titles);
    if !context.is_empty() {
        let chip = gtk::Label::builder()
            .label(context)
            .halign(gtk::Align::End)
            .build();
        chip.add_css_class("atrium-task-context");
        chip.add_css_class("dim-label");
        row.append(&chip);
    }

    // Completion date — short ISO-ish format matches the task
    // schedule pill style.
    if let Some(when) = task.completed_at {
        let date = gtk::Label::builder()
            .label(when.date_naive().format("%b %-d").to_string())
            .halign(gtk::Align::End)
            .build();
        date.add_css_class("atrium-task-schedule");
        date.add_css_class("dim-label");
        row.append(&date);
    }

    row.upcast()
}

fn build_context_label(
    task: &Task,
    project_titles: &HashMap<i64, String>,
    project_areas: &HashMap<i64, Option<i64>>,
    area_titles: &HashMap<i64, String>,
) -> String {
    let Some(pid) = task.project_id else {
        return String::new();
    };
    let project = project_titles.get(&pid).cloned().unwrap_or_default();
    let area = project_areas
        .get(&pid)
        .copied()
        .flatten()
        .and_then(|aid| area_titles.get(&aid).cloned())
        .unwrap_or_default();
    match (area.is_empty(), project.is_empty()) {
        (true, true) => String::new(),
        // Translators: hierarchy breadcrumb, e.g. "Work › Website";
        // keep the › separator unless the locale demands otherwise.
        (false, false) => gettext_f(
            "{area} › {project}",
            &[("area", &area), ("project", &project)],
        ),
        (false, true) => area,
        (true, false) => project,
    }
}

fn pango_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use atrium_core::test_support::dummy_task;
    use chrono::Utc;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn t() -> NaiveDate {
        d(2026, 5, 15)
    }

    fn completed(id: i64, date: NaiveDate) -> Task {
        let mut task = dummy_task(id);
        task.completed_at = Some(chrono::DateTime::<Utc>::from_naive_utc_and_offset(
            date.and_hms_opt(12, 0, 0).unwrap(),
            Utc,
        ));
        task
    }

    #[test]
    fn classify_today_for_today_completion() {
        assert_eq!(classify_band(t(), t()), DateBand::Today);
    }

    #[test]
    fn classify_today_for_future_dates() {
        // Defensive — a future date on a completed task shouldn't
        // happen in production but folds into Today rather than
        // crashing.
        assert_eq!(classify_band(t() + Duration::days(2), t()), DateBand::Today);
    }

    #[test]
    fn classify_yesterday_for_one_day_ago() {
        assert_eq!(
            classify_band(t() - Duration::days(1), t()),
            DateBand::Yesterday
        );
    }

    #[test]
    fn classify_last_seven_days_for_two_through_seven_days_ago() {
        for n in 2..=7 {
            let band = classify_band(t() - Duration::days(n), t());
            assert_eq!(
                band,
                DateBand::LastSevenDays,
                "n={n} should be LastSevenDays"
            );
        }
    }

    #[test]
    fn classify_older_for_eight_or_more_days_ago() {
        assert_eq!(classify_band(t() - Duration::days(8), t()), DateBand::Older);
        assert_eq!(
            classify_band(t() - Duration::days(60), t()),
            DateBand::Older
        );
    }

    #[test]
    fn group_drops_tasks_without_completed_at() {
        let mut open_task = dummy_task(1);
        open_task.completed_at = None;
        let done_today = completed(2, t());
        let groups = group_by_band(&[open_task, done_today], t());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, DateBand::Today);
        assert_eq!(groups[0].1.len(), 1);
        assert_eq!(groups[0].1[0].id, 2);
    }

    #[test]
    fn group_orders_today_first_older_last() {
        let groups = group_by_band(
            &[
                completed(1, t() - Duration::days(30)), // Older
                completed(2, t() - Duration::days(1)),  // Yesterday
                completed(3, t()),                      // Today
                completed(4, t() - Duration::days(4)),  // LastSevenDays
            ],
            t(),
        );
        let bands: Vec<DateBand> = groups.iter().map(|(b, _)| *b).collect();
        assert_eq!(
            bands,
            vec![
                DateBand::Today,
                DateBand::Yesterday,
                DateBand::LastSevenDays,
                DateBand::Older,
            ]
        );
    }

    #[test]
    fn group_skips_empty_bands() {
        // Only Today and Older entries — Yesterday + LastSevenDays
        // are absent from the output rather than appearing as empty.
        let groups = group_by_band(
            &[completed(1, t()), completed(2, t() - Duration::days(60))],
            t(),
        );
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, DateBand::Today);
        assert_eq!(groups[1].0, DateBand::Older);
    }

    #[test]
    fn band_headings_are_human() {
        assert_eq!(DateBand::Today.heading(), "Today");
        assert_eq!(DateBand::Yesterday.heading(), "Yesterday");
        assert_eq!(DateBand::LastSevenDays.heading(), "Last 7 Days");
        assert_eq!(DateBand::Older.heading(), "Older");
    }
}
