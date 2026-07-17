// SPDX-License-Identifier: MIT
//! Builder Mode Calendar Month View (Phase 12.5).
//!
//! The third lens over the same task data that Forecast (30-day
//! strip) and Agenda (chronological bands) already cover. This is
//! the paper-calendar idiom: a 7-column month grid with a count
//! badge + a few inline task titles + "+N more" overflow popover
//! per day. Re-engaged at v0.11 after the v0.6.x roadmap revision
//! tentatively marked it "subsumed by Agenda" — a calendar lens
//! is a different mental model than the chronological bands or
//! the 30-day strip; the user wants paper-calendar paging.
//!
//! Like Agenda and Forecast this is a canonical page (not a
//! Perspective renderer) — same shape: scoped sidebar entry,
//! always shows all tasks, mode-gated to Builder. The pure date-
//! math + grouping helpers below are testable in isolation; the
//! GTK widget wiring sits below them.

use std::collections::HashMap;

use adw::prelude::*;
use atrium_core::{ScheduledFor, Task, TaskUpdate, WorkerHandle};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use gtk::gdk;
use gtk::glib;
use tracing::error;

use crate::i18n::{gettext, gettext_f, ngettext_f};

/// One day in the rendered month grid. `tasks` holds open tasks
/// whose `scheduled_for` is this date (deadline-only tasks are
/// surfaced via Forecast; the calendar uses the When-axis only,
/// matching the paper-calendar idiom). `is_today` paints the day
/// with the today emphasis; `in_view_month` distinguishes leading
/// / trailing days from neighbour months that share the same
/// week (those days appear muted).
#[derive(Debug, Clone)]
pub struct DayCell {
    pub date: NaiveDate,
    pub in_view_month: bool,
    pub is_today: bool,
    pub tasks: Vec<Task>,
}

/// The 7-column × N-row grid of `DayCell`s for a given calendar
/// month. The grid always starts on a Monday and runs full weeks,
/// so leading days from the previous month and trailing days from
/// the next month fill out incomplete weeks. Most months produce
/// 5 rows; a month whose 1st falls on a Sunday (and the month has
/// 31 days) needs 6.
#[derive(Debug, Clone)]
pub struct MonthGrid {
    pub year: i32,
    pub month: u32,
    pub weeks: Vec<[DayCell; 7]>,
}

/// First day of the calendar month containing `date`. Used as the
/// canonical "viewed month" marker when navigating prev / next /
/// today: round any input date to its month's first day.
pub fn first_of_month(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).expect("year/month/1 always valid")
}

/// First Monday on or before the 1st of the rendered month — i.e.
/// the leading-edge of the calendar grid. May land in the previous
/// month.
pub fn grid_anchor(year: i32, month: u32) -> NaiveDate {
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("invalid year/month");
    let offset = first.weekday().num_days_from_monday() as i64;
    first - Duration::days(offset)
}

/// Last Sunday on or after the last day of the rendered month —
/// i.e. the trailing edge of the calendar grid. May land in the
/// next month.
pub fn grid_end(year: i32, month: u32) -> NaiveDate {
    let last = last_day_of_month(year, month);
    let weekday = last.weekday();
    let offset = match weekday {
        Weekday::Sun => 0,
        other => 7 - other.num_days_from_monday() as i64 - 1,
    };
    last + Duration::days(offset)
}

/// The last calendar day of `year/month`. Robust to short months
/// (Feb 28/29) without a `chrono::Months` import — we walk to the
/// 1st of the next month and step back a day.
pub fn last_day_of_month(year: i32, month: u32) -> NaiveDate {
    let (next_y, next_m) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_y, next_m, 1).expect("year/month always valid") - Duration::days(1)
}

/// Number of week rows the grid for `year/month` needs. 5 for
/// most months; 6 when the month starts on a Sunday and runs
/// 30+ days (Jan 2026, Aug 2026, …). 4 is impossible for a
/// Mon-start grid — even February 2026 (28 days starting Sun)
/// produces 5 rows because the leading Mon-Sat are from
/// January.
pub fn week_rows(year: i32, month: u32) -> usize {
    let anchor = grid_anchor(year, month);
    let end = grid_end(year, month);
    let span = (end - anchor).num_days() + 1;
    debug_assert!(span % 7 == 0, "grid span {span} not week-aligned");
    (span / 7) as usize
}

/// Step `viewed.first_of_month()` to the previous month, clamping
/// to chrono's representable range. Wraps year on January.
pub fn previous_month(viewed: NaiveDate) -> NaiveDate {
    let first = first_of_month(viewed);
    let (y, m) = if first.month() == 1 {
        (first.year() - 1, 12)
    } else {
        (first.year(), first.month() - 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).expect("valid prev month")
}

/// Step `viewed.first_of_month()` to the following month.
pub fn next_month(viewed: NaiveDate) -> NaiveDate {
    let first = first_of_month(viewed);
    let (y, m) = if first.month() == 12 {
        (first.year() + 1, 1)
    } else {
        (first.year(), first.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).expect("valid next month")
}

/// Build the typed grid for the calendar month containing
/// `viewed`. `today` drives the today-cell emphasis; `tasks` are
/// bucketed by their `scheduled_for` date. Tasks without a
/// scheduled date (deadline-only, Inbox, Someday) don't appear on
/// the calendar — that's the paper-calendar idiom.
pub fn build_month_grid(viewed: NaiveDate, today: NaiveDate, tasks: &[Task]) -> MonthGrid {
    let first = first_of_month(viewed);
    let year = first.year();
    let month = first.month();
    let anchor = grid_anchor(year, month);

    // Bucket tasks by scheduled date; multiple tasks per date land
    // in input order (caller pre-sorts by relevance / position).
    let mut by_date: HashMap<NaiveDate, Vec<Task>> = HashMap::new();
    for task in tasks {
        if task.completed_at.is_some() {
            continue;
        }
        if let Some(ScheduledFor::Date(d)) = task.scheduled_for {
            by_date.entry(d).or_default().push(task.clone());
        }
    }

    let rows = week_rows(year, month);
    let mut weeks: Vec<[DayCell; 7]> = Vec::with_capacity(rows);
    for w in 0..rows {
        let mut week: [DayCell; 7] = std::array::from_fn(|d| {
            let date = anchor + Duration::days((w * 7 + d) as i64);
            let day_tasks = by_date.remove(&date).unwrap_or_default();
            DayCell {
                date,
                in_view_month: date.month() == month && date.year() == year,
                is_today: date == today,
                tasks: day_tasks,
            }
        });
        // The std::array::from_fn closure can't return early to
        // match the `move` semantics neatly; the assignment above
        // consumes by_date entries on the fly. Touch the array to
        // satisfy the linter.
        let _ = &mut week;
        weeks.push(week);
    }
    MonthGrid { year, month, weeks }
}

/// Month name for header rendering. Month names come from the
/// translation catalogue (Phase 20 localisation); untranslated
/// locales fall back to the English msgids.
pub fn month_name(month: u32) -> String {
    match (month as usize - 1).min(11) {
        0 => gettext("January"),
        1 => gettext("February"),
        2 => gettext("March"),
        3 => gettext("April"),
        4 => gettext("May"),
        5 => gettext("June"),
        6 => gettext("July"),
        7 => gettext("August"),
        8 => gettext("September"),
        9 => gettext("October"),
        10 => gettext("November"),
        _ => gettext("December"),
    }
}

// ── GTK widget rendering ─────────────────────────────────────

/// Up to this many task titles render inline in a day cell; the
/// rest collapse into a "+N more" link that opens a popover with
/// the full list. Matches OmniFocus's default cell density.
const INLINE_PER_CELL: usize = 3;

/// Width in CSS pixels at which the month grid collapses to a
/// vertical week strip. Tuned for phone-portrait sizes — desktop
/// / tablet windows always show the full month. Public so the
/// window can use the same threshold when watching its own
/// allocation.
pub const COMPACT_WIDTH_THRESHOLD: i32 = 600;

/// Pick the focal week for the compact strip layout. If the
/// viewed month contains today, the week containing today wins;
/// otherwise we anchor on the first week of the viewed month so
/// the user sees real days rather than leading-edge previous-
/// month padding.
fn focal_week_anchor(grid: &MonthGrid, today: NaiveDate) -> NaiveDate {
    let in_view = grid.weeks.iter().flatten().any(|c| c.date == today);
    if in_view {
        // Round today back to its Monday.
        let mon_offset = today.weekday().num_days_from_monday() as i64;
        today - Duration::days(mon_offset)
    } else {
        grid.weeks.first().map_or_else(
            || {
                let first =
                    NaiveDate::from_ymd_opt(grid.year, grid.month, 1).expect("year/month/1");
                let off = first.weekday().num_days_from_monday() as i64;
                first - Duration::days(off)
            },
            |w| w[0].date,
        )
    }
}

/// Callback bundle for the calendar page. `on_prev` / `on_next` /
/// `on_today` drive the nav header; `on_pick_month(year, month)`
/// opens the month picker; `on_row_click(task_id)` opens a task in
/// the inspector when clicked inside an overflow popover; and
/// `on_day_drill(date)` fires on a double-click anywhere in a
/// cell so the caller can swap the content pane to a date-scoped
/// view. Bundled so [`build_page`]'s argument list stays under
/// clippy's `too_many_arguments` threshold.
pub struct CalendarCallbacks<PrevFn, NextFn, TodayFn, PickFn, RowFn, DrillFn>
where
    PrevFn: Fn() + 'static + Clone,
    NextFn: Fn() + 'static + Clone,
    TodayFn: Fn() + 'static,
    PickFn: Fn(i32, u32) + 'static,
    RowFn: Fn(i64) + 'static + Clone,
    DrillFn: Fn(NaiveDate) + 'static + Clone,
{
    pub on_prev: PrevFn,
    pub on_next: NextFn,
    pub on_today: TodayFn,
    pub on_pick_month: PickFn,
    pub on_row_click: RowFn,
    pub on_day_drill: DrillFn,
}

/// Build the calendar page widget for the month containing
/// `viewed`. `today` drives the today-cell emphasis; `tasks` are
/// bucketed by `scheduled_for`. Callbacks come bundled in
/// [`CalendarCallbacks`]. `worker`, when present, enables drag-
/// to-reschedule between days; `None` keeps the page read-only
/// (used by tests / future read-only contexts). `compact` swaps
/// the 7×N month grid for a vertical week strip — the window
/// flips this on under phone-shaped portrait widths
/// ([`COMPACT_WIDTH_THRESHOLD`]).
pub fn build_page<PrevFn, NextFn, TodayFn, PickFn, RowFn, DrillFn>(
    viewed: NaiveDate,
    today: NaiveDate,
    tasks: &[Task],
    worker: Option<WorkerHandle>,
    compact: bool,
    cb: CalendarCallbacks<PrevFn, NextFn, TodayFn, PickFn, RowFn, DrillFn>,
) -> gtk::Widget
where
    PrevFn: Fn() + 'static + Clone,
    NextFn: Fn() + 'static + Clone,
    TodayFn: Fn() + 'static,
    PickFn: Fn(i32, u32) + 'static,
    RowFn: Fn(i64) + 'static + Clone,
    DrillFn: Fn(NaiveDate) + 'static + Clone,
{
    let CalendarCallbacks {
        on_prev,
        on_next,
        on_today,
        on_pick_month,
        on_row_click,
        on_day_drill,
    } = cb;
    let grid = build_month_grid(viewed, today, tasks);

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(16)
        .build();

    body.append(&build_header(
        grid.year,
        grid.month,
        on_prev.clone(),
        on_next.clone(),
        on_today,
        on_pick_month,
    ));
    if compact {
        // Vertical week strip — 7 day cards stacked. Drops the
        // weekday column header (each card carries its own day
        // label) and squeezes through phone-portrait widths.
        let anchor = focal_week_anchor(&grid, today);
        body.append(&build_week_strip(
            anchor,
            today,
            tasks,
            worker,
            on_row_click,
            on_day_drill,
        ));
    } else {
        body.append(&build_weekday_strip());
        body.append(&build_grid(&grid, worker, on_row_click, on_day_drill));
    }

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&body)
        .build();
    install_page_navigation_shortcuts(&scroller, on_prev, on_next);
    scroller.upcast()
}

/// Wire Page Up / Page Down to step the month back / forward.
/// Scoped to the calendar widget itself (gtk::ShortcutScope::Local)
/// so the keys stay free for other surfaces.
fn install_page_navigation_shortcuts<PrevFn, NextFn>(
    target: &gtk::ScrolledWindow,
    on_prev: PrevFn,
    on_next: NextFn,
) where
    PrevFn: Fn() + 'static + Clone,
    NextFn: Fn() + 'static + Clone,
{
    let controller = gtk::ShortcutController::new();
    controller.set_scope(gtk::ShortcutScope::Local);
    let prev = on_prev.clone();
    let prev_action = gtk::CallbackAction::new(move |_, _| {
        prev();
        glib::Propagation::Stop
    });
    controller.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Page_Up"),
        Some(prev_action),
    ));
    let next = on_next.clone();
    let next_action = gtk::CallbackAction::new(move |_, _| {
        next();
        glib::Propagation::Stop
    });
    controller.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Page_Down"),
        Some(next_action),
    ));
    target.add_controller(controller);
    // Make focusable so the controller has somewhere to attach.
    target.set_focusable(true);
}

fn build_header<PrevFn, NextFn, TodayFn, PickFn>(
    year: i32,
    month: u32,
    on_prev: PrevFn,
    on_next: NextFn,
    on_today: TodayFn,
    on_pick_month: PickFn,
) -> gtk::Widget
where
    PrevFn: Fn() + 'static + Clone,
    NextFn: Fn() + 'static + Clone,
    TodayFn: Fn() + 'static,
    PickFn: Fn(i32, u32) + 'static,
{
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_bottom(4)
        .build();

    let prev_btn = gtk::Button::builder()
        .icon_name("go-previous-symbolic")
        .tooltip_text(gettext("Previous month (Page Up)"))
        .build();
    prev_btn.update_property(&[gtk::accessible::Property::Label(&gettext("Previous month"))]);
    {
        let cb = on_prev.clone();
        prev_btn.connect_clicked(move |_| cb());
    }
    header.append(&prev_btn);

    // Translators: calendar header, e.g. "May 2026"; reorder the
    // placeholders as the locale requires.
    let title_btn = gtk::MenuButton::builder()
        .label(gettext_f(
            "{month} {year}",
            &[("month", &month_name(month)), ("year", &year.to_string())],
        ))
        .css_classes(["flat", "title-2"])
        .tooltip_text(gettext("Pick a month"))
        .build();
    let popover = build_month_picker(year, month, on_pick_month);
    title_btn.set_popover(Some(&popover));
    title_btn.set_hexpand(true);
    title_btn.set_halign(gtk::Align::Start);
    header.append(&title_btn);

    let today_btn = gtk::Button::builder()
        .label(gettext("Today"))
        .tooltip_text(gettext("Jump to the current month"))
        .build();
    today_btn.connect_clicked(move |_| on_today());
    header.append(&today_btn);

    let next_btn = gtk::Button::builder()
        .icon_name("go-next-symbolic")
        .tooltip_text(gettext("Next month (Page Down)"))
        .build();
    next_btn.update_property(&[gtk::accessible::Property::Label(&gettext("Next month"))]);
    next_btn.connect_clicked(move |_| on_next());
    header.append(&next_btn);

    header.upcast()
}

fn build_month_picker<F: Fn(i32, u32) + 'static>(
    current_year: i32,
    current_month: u32,
    on_pick: F,
) -> gtk::Popover {
    let popover = gtk::Popover::builder().build();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(8)
        .margin_end(8)
        .margin_top(8)
        .margin_bottom(8)
        .build();

    // Year row — prev / current / next. Three buttons total,
    // keyed off `current_year`. Multi-year navigation lands in a
    // follow-up if users push for it.
    let year_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let year_label = gtk::Label::builder()
        .label(format!("{current_year}"))
        .css_classes(["heading"])
        .hexpand(true)
        .build();
    year_row.append(&year_label);
    body.append(&year_row);

    // Month grid — 4×3.
    let month_grid = gtk::Grid::builder()
        .row_spacing(4)
        .column_spacing(4)
        .build();
    let on_pick = std::rc::Rc::new(on_pick);
    let popover_weak = popover.downgrade();
    for m in 1..=12u32 {
        // Char-based (not byte) truncation — translated month names
        // may be multibyte, and a byte slice could panic mid-char.
        let btn = gtk::Button::builder()
            .label(month_name(m).chars().take(3).collect::<String>())
            .css_classes(if m == current_month {
                vec!["suggested-action"]
            } else {
                vec!["flat"]
            })
            .build();
        let cb = on_pick.clone();
        let pw = popover_weak.clone();
        btn.connect_clicked(move |_| {
            cb(current_year, m);
            if let Some(p) = pw.upgrade() {
                p.popdown();
            }
        });
        let col = ((m - 1) % 4) as i32;
        let row = ((m - 1) / 4) as i32;
        month_grid.attach(&btn, col, row, 1, 1);
    }
    body.append(&month_grid);

    popover.set_child(Some(&body));
    popover
}

fn build_weekday_strip() -> gtk::Widget {
    let strip = gtk::Grid::builder()
        .column_homogeneous(true)
        .column_spacing(4)
        .build();
    // Translators: abbreviated weekday column headers, Monday first.
    let names = [
        gettext("Mon"),
        gettext("Tue"),
        gettext("Wed"),
        gettext("Thu"),
        gettext("Fri"),
        gettext("Sat"),
        gettext("Sun"),
    ];
    for (i, name) in names.into_iter().enumerate() {
        let lbl = gtk::Label::builder()
            .label(name)
            .css_classes(["dim-label", "caption"])
            .halign(gtk::Align::Center)
            .build();
        strip.attach(&lbl, i as i32, 0, 1, 1);
    }
    strip.upcast()
}

/// Compact-mode renderer — vertical strip of 7 day cards starting
/// at `anchor` (a Monday). Each card shows the day's full task
/// list inline (no "+N more" overflow popover, since we have
/// vertical room). The card layout reuses [`build_cell`]'s shape
/// so drag / drop / single-click peek / double-click drill all
/// work the same as in month-grid mode.
fn build_week_strip<F, DrillFn>(
    anchor: NaiveDate,
    today: NaiveDate,
    tasks: &[Task],
    worker: Option<WorkerHandle>,
    on_row_click: F,
    on_day_drill: DrillFn,
) -> gtk::Widget
where
    F: Fn(i64) + 'static + Clone,
    DrillFn: Fn(NaiveDate) + 'static + Clone,
{
    let strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .vexpand(true)
        .build();
    strip.add_css_class("atrium-calendar-strip");

    let mut by_date: HashMap<NaiveDate, Vec<Task>> = HashMap::new();
    for task in tasks {
        if task.completed_at.is_some() {
            continue;
        }
        if let Some(ScheduledFor::Date(d)) = task.scheduled_for {
            by_date.entry(d).or_default().push(task.clone());
        }
    }
    let viewed_month = NaiveDate::from_ymd_opt(anchor.year(), anchor.month(), 1)
        .map_or((anchor.year(), anchor.month()), |d| (d.year(), d.month()));

    for d in 0..7 {
        let date = anchor + Duration::days(d);
        let day_tasks = by_date.remove(&date).unwrap_or_default();
        let cell = DayCell {
            date,
            in_view_month: date.month() == viewed_month.1 && date.year() == viewed_month.0,
            is_today: date == today,
            tasks: day_tasks,
        };
        strip.append(&build_strip_card(
            &cell,
            worker.clone(),
            on_row_click.clone(),
            on_day_drill.clone(),
        ));
    }
    strip.upcast()
}

/// One card in the compact week strip. Wider than a month-grid
/// cell because it's the only thing in its row, so we can render
/// the full task list inline without an overflow popover.
fn build_strip_card<F, DrillFn>(
    cell: &DayCell,
    worker: Option<WorkerHandle>,
    on_row_click: F,
    on_day_drill: DrillFn,
) -> gtk::Widget
where
    F: Fn(i64) + 'static + Clone,
    DrillFn: Fn(NaiveDate) + 'static + Clone,
{
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_start(8)
        .margin_end(8)
        .margin_top(8)
        .margin_bottom(8)
        .build();
    card.add_css_class("atrium-calendar-strip-card");
    card.add_css_class("card");
    if cell.is_today {
        card.add_css_class("atrium-calendar-cell-today");
    }
    if !cell.in_view_month {
        card.add_css_class("atrium-calendar-cell-out-of-month");
    }

    // Header: full weekday + day-month label.
    let header_text = cell.date.format("%A %B %-d").to_string();
    let header = gtk::Label::builder()
        .label(&header_text)
        .halign(gtk::Align::Start)
        .build();
    if cell.is_today {
        header.add_css_class("heading");
    } else {
        header.add_css_class("dim-label");
    }
    card.append(&header);

    if cell.tasks.is_empty() {
        let empty = gtk::Label::builder()
            .label(gettext("Nothing scheduled"))
            .css_classes(["dim-label", "caption"])
            .halign(gtk::Align::Start)
            .build();
        card.append(&empty);
    } else {
        for task in &cell.tasks {
            // v0.19.0 — Phase 18.5 Tier-2: prefix with HH:MM
            // when present so the calendar's narrow-week-strip
            // surfaces the time inline.
            let label = match task.scheduled_time {
                Some(t) => format!("{}  {}", t.format("%H:%M"), task.title),
                None => task.title.clone(),
            };
            let row = gtk::Label::builder()
                .label(&label)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .halign(gtk::Align::Start)
                .css_classes(["caption"])
                .build();
            attach_task_drag_source(&row, task.id);
            // v0.39.x (Tier D) — a click on the task row opens *that*
            // task, consistent with Agenda / Forecast rows. Claiming
            // the event stops it bubbling to the card's day-peek
            // gesture, so a row click opens the task and an empty-area
            // click still peeks the day.
            let row_click = gtk::GestureClick::new();
            let task_id = task.id;
            let on_click = on_row_click.clone();
            row_click.connect_released(move |g, _, _, _| {
                g.set_state(gtk::EventSequenceState::Claimed);
                on_click(task_id);
            });
            row.add_controller(row_click);
            card.append(&row);
        }
    }

    if let Some(worker) = worker {
        attach_drop_target_for_date(&card, cell.date, worker);
    }
    attach_day_click_handlers(
        &card,
        cell.date,
        cell.tasks.clone(),
        on_row_click,
        on_day_drill,
    );

    card.upcast()
}

fn build_grid<F, DrillFn>(
    grid: &MonthGrid,
    worker: Option<WorkerHandle>,
    on_row_click: F,
    on_day_drill: DrillFn,
) -> gtk::Widget
where
    F: Fn(i64) + 'static + Clone,
    DrillFn: Fn(NaiveDate) + 'static + Clone,
{
    let g = gtk::Grid::builder()
        .row_homogeneous(true)
        .column_homogeneous(true)
        .row_spacing(4)
        .column_spacing(4)
        .vexpand(true)
        .build();
    g.add_css_class("atrium-calendar-grid");

    for (row_i, week) in grid.weeks.iter().enumerate() {
        for (col, cell) in week.iter().enumerate() {
            g.attach(
                &build_cell(
                    cell,
                    worker.clone(),
                    on_row_click.clone(),
                    on_day_drill.clone(),
                ),
                col as i32,
                row_i as i32,
                1,
                1,
            );
        }
    }
    g.upcast()
}

fn build_cell<F, DrillFn>(
    cell: &DayCell,
    worker: Option<WorkerHandle>,
    on_row_click: F,
    on_day_drill: DrillFn,
) -> gtk::Widget
where
    F: Fn(i64) + 'static + Clone,
    DrillFn: Fn(NaiveDate) + 'static + Clone,
{
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_start(4)
        .margin_end(4)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    card.add_css_class("atrium-calendar-cell");
    card.add_css_class("card");
    if !cell.in_view_month {
        card.add_css_class("atrium-calendar-cell-out-of-month");
    }
    if cell.is_today {
        card.add_css_class("atrium-calendar-cell-today");
    }

    // Header strip: day number + count badge.
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .build();
    let day_lbl = gtk::Label::builder()
        .label(format!("{}", cell.date.day()))
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build();
    if cell.is_today {
        day_lbl.add_css_class("heading");
        // Accessible marker (v0.38.x audit): today is otherwise cued by
        // border colour + a bold day number, which reads identically to
        // any other day for a screen reader. Name it explicitly.
        // Translators: accessible label for the current day's cell;
        // {day} is the day-of-month number.
        day_lbl.update_property(&[gtk::accessible::Property::Label(&gettext_f(
            "{day} (today)",
            &[("day", &cell.date.day().to_string())],
        ))]);
    }
    header.append(&day_lbl);
    if !cell.tasks.is_empty() {
        let badge = gtk::Label::builder()
            .label(format!("{}", cell.tasks.len()))
            .css_classes(["caption", "dim-label"])
            .build();
        header.append(&badge);
    }
    card.append(&header);

    // Inline titles up to INLINE_PER_CELL. Each title is its
    // own draggable widget so the user can drop it on another
    // day to reschedule (handler attaches at the cell level
    // below).
    for task in cell.tasks.iter().take(INLINE_PER_CELL) {
        // v0.19.0 — Phase 18.5 Tier-2 time-of-day prefix.
        let label = match task.scheduled_time {
            Some(t) => format!("{}  {}", t.format("%H:%M"), task.title),
            None => task.title.clone(),
        };
        let row = gtk::Label::builder()
            .label(&label)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .halign(gtk::Align::Start)
            .css_classes(["caption"])
            .build();
        attach_task_drag_source(&row, task.id);
        card.append(&row);
    }

    if cell.tasks.len() > INLINE_PER_CELL {
        let overflow = cell.tasks.len() - INLINE_PER_CELL;
        // Translators: overflow label on a crowded calendar day;
        // {n} is the number of hidden tasks.
        let more_btn = gtk::MenuButton::builder()
            .label(ngettext_f(
                "+{n} more",
                "+{n} more",
                overflow as u32,
                &[("n", &overflow.to_string())],
            ))
            .css_classes(["flat", "caption"])
            .halign(gtk::Align::Start)
            .build();
        let pop = build_overflow_popover(&cell.tasks[INLINE_PER_CELL..], on_row_click.clone());
        more_btn.set_popover(Some(&pop));
        card.append(&more_btn);
    }

    // Drop target — drop a task here to reschedule it to this
    // date. Mirrors Forecast's pattern. Out-of-month cells still
    // accept drops so a user can drag into the previous / next
    // month from the leading / trailing rows.
    if let Some(worker) = worker {
        attach_drop_target_for_date(&card, cell.date, worker);
    }

    // Click gestures — single click pops a "day's tasks"
    // overview popover (handy when there are 0..3 tasks and the
    // "+N more" affordance never appears); double click drills
    // into a date-scoped search so the user can edit the tasks
    // in the standard list view.
    attach_day_click_handlers(
        &card,
        cell.date,
        cell.tasks.clone(),
        on_row_click,
        on_day_drill,
    );

    card.upcast()
}

/// Single-click → popover with the day's tasks; double-click →
/// invoke `on_day_drill(date)` so the caller can swap the
/// content pane to a date-scoped view. The popover anchors to
/// the cell card, so successive clicks on the same cell stay
/// on screen until the user dismisses.
fn attach_day_click_handlers<F, DrillFn>(
    card: &gtk::Box,
    date: NaiveDate,
    tasks_for_day: Vec<Task>,
    on_row_click: F,
    on_day_drill: DrillFn,
) where
    F: Fn(i64) + 'static + Clone,
    DrillFn: Fn(NaiveDate) + 'static + Clone,
{
    let click = gtk::GestureClick::new();
    click.set_button(gdk::BUTTON_PRIMARY);
    let card_weak = card.downgrade();
    let on_row_click = on_row_click.clone();
    click.connect_pressed(move |_, n_press, _, _| {
        match n_press {
            2 => on_day_drill(date),
            1 => {
                // Single click: anchor a popover to the card
                // and show the day's tasks inline. Empty days
                // still pop a "Nothing scheduled" affordance —
                // confirms the user clicked a real day, not a
                // gutter.
                let Some(card) = card_weak.upgrade() else {
                    return;
                };
                let pop = build_day_popover(date, &tasks_for_day, on_row_click.clone());
                pop.set_parent(&card);
                pop.popup();
            }
            _ => {}
        }
    });
    card.add_controller(click);
}

fn build_day_popover<F>(date: NaiveDate, tasks: &[Task], on_row_click: F) -> gtk::Popover
where
    F: Fn(i64) + 'static + Clone,
{
    let pop = gtk::Popover::builder().build();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_start(8)
        .margin_end(8)
        .margin_top(8)
        .margin_bottom(8)
        .build();
    let header = gtk::Label::builder()
        .label(date.format("%A, %B %-d, %Y").to_string())
        .css_classes(["heading"])
        .halign(gtk::Align::Start)
        .build();
    body.append(&header);
    if tasks.is_empty() {
        let empty = gtk::Label::builder()
            .label(gettext("Nothing scheduled."))
            .css_classes(["dim-label", "caption"])
            .halign(gtk::Align::Start)
            .build();
        body.append(&empty);
    } else {
        for task in tasks {
            let id = task.id;
            // v0.19.0 — Phase 18.5 Tier-2 time-of-day prefix
            // in the day-peek popover.
            let label = match task.scheduled_time {
                Some(t) => format!("{}  {}", t.format("%H:%M"), task.title),
                None => task.title.clone(),
            };
            let btn = gtk::Button::builder()
                .label(&label)
                .css_classes(["flat"])
                .halign(gtk::Align::Start)
                .build();
            let cb = on_row_click.clone();
            let pop_weak = pop.downgrade();
            btn.connect_clicked(move |_| {
                cb(id);
                if let Some(p) = pop_weak.upgrade() {
                    p.popdown();
                }
            });
            body.append(&btn);
        }
    }
    pop.set_child(Some(&body));
    pop
}

/// Wire up a `gtk::DragSource` carrying the task id as an `i64`
/// content provider. Mirrors the forecast / list-view drag shape
/// so the same drop targets across the app accept calendar
/// drags. Action: MOVE — this isn't a copy, it's a reschedule.
fn attach_task_drag_source(widget: &gtk::Label, task_id: i64) {
    let drag_source = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    drag_source
        .connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&task_id.to_value())));
    widget.add_controller(drag_source);
}

/// Wire up a `gtk::DropTarget` accepting `i64` (task id) and
/// rescheduling the dropped task to `target_date`. Same shape as
/// forecast's drop handler; the worker handles the actual update
/// and emits the TaskChanges delta that triggers a calendar
/// refresh so the dropped task moves to its new cell on the next
/// tick.
fn attach_drop_target_for_date(card: &gtk::Box, target_date: NaiveDate, worker: WorkerHandle) {
    let drop_target = gtk::DropTarget::new(i64::static_type(), gdk::DragAction::MOVE);
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
                error!(?e, task_id, ?target_date, "calendar drop failed");
            }
        });
        true
    });
    card.add_controller(drop_target);
}

fn build_overflow_popover<F>(tasks: &[Task], on_row_click: F) -> gtk::Popover
where
    F: Fn(i64) + 'static + Clone,
{
    let pop = gtk::Popover::builder().build();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_start(8)
        .margin_end(8)
        .margin_top(8)
        .margin_bottom(8)
        .build();
    for task in tasks {
        let id = task.id;
        let label = match task.scheduled_time {
            Some(t) => format!("{}  {}", t.format("%H:%M"), task.title),
            None => task.title.clone(),
        };
        let btn = gtk::Button::builder()
            .label(&label)
            .css_classes(["flat"])
            .halign(gtk::Align::Start)
            .build();
        let cb = on_row_click.clone();
        let pop_weak = pop.downgrade();
        btn.connect_clicked(move |_| {
            cb(id);
            if let Some(p) = pop_weak.upgrade() {
                p.popdown();
            }
        });
        body.append(&btn);
    }
    pop.set_child(Some(&body));
    pop
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    // ── Date math ────────────────────────────────────────────

    #[test]
    fn first_of_month_normalises() {
        assert_eq!(first_of_month(d(2026, 5, 9)), d(2026, 5, 1));
        assert_eq!(first_of_month(d(2026, 5, 1)), d(2026, 5, 1));
        assert_eq!(first_of_month(d(2024, 2, 29)), d(2024, 2, 1));
    }

    #[test]
    fn last_day_handles_31_30_28_29() {
        assert_eq!(last_day_of_month(2026, 1), d(2026, 1, 31));
        assert_eq!(last_day_of_month(2026, 4), d(2026, 4, 30));
        assert_eq!(last_day_of_month(2026, 2), d(2026, 2, 28));
        assert_eq!(last_day_of_month(2024, 2), d(2024, 2, 29)); // leap
        assert_eq!(last_day_of_month(2026, 12), d(2026, 12, 31));
    }

    #[test]
    fn previous_and_next_month_wrap_year() {
        assert_eq!(previous_month(d(2026, 1, 15)), d(2025, 12, 1));
        assert_eq!(next_month(d(2026, 12, 15)), d(2027, 1, 1));
        assert_eq!(previous_month(d(2026, 5, 1)), d(2026, 4, 1));
        assert_eq!(next_month(d(2026, 5, 1)), d(2026, 6, 1));
    }

    #[test]
    fn grid_anchor_is_monday_on_or_before_first() {
        // May 2026: 1st is Friday → Monday before is April 27.
        assert_eq!(grid_anchor(2026, 5), d(2026, 4, 27));
        // February 2026: 1st is Sunday → Monday before is January 26.
        assert_eq!(grid_anchor(2026, 2), d(2026, 1, 26));
        // June 2026: 1st is Monday → grid_anchor == 1st.
        assert_eq!(grid_anchor(2026, 6), d(2026, 6, 1));
    }

    #[test]
    fn grid_end_is_sunday_on_or_after_last() {
        // May 2026 ends on Sunday May 31 → grid_end == May 31.
        assert_eq!(grid_end(2026, 5), d(2026, 5, 31));
        // April 2026 ends on Thursday April 30 → grid_end is the
        // next Sunday, May 3.
        assert_eq!(grid_end(2026, 4), d(2026, 5, 3));
    }

    #[test]
    fn week_rows_handles_short_and_long_months() {
        // February 2026: 28 days starting Sunday → grid_anchor
        // is Jan 26 (Mon), grid_end is Feb 28 (Sat) ... wait,
        // Feb 28 2026 is Saturday, so grid_end = Mar 1 (Sun).
        // Span Jan 26 → Mar 1 = 35 days = 5 rows.
        assert_eq!(week_rows(2026, 2), 5);
        // August 2026 starts Saturday and runs 31 days → 6 rows.
        assert_eq!(week_rows(2026, 8), 6);
    }

    #[test]
    fn leap_february_renders_29_days() {
        let grid = build_month_grid(d(2024, 2, 1), d(2024, 2, 1), &[]);
        let in_month_days: Vec<u32> = grid
            .weeks
            .iter()
            .flatten()
            .filter(|c| c.in_view_month)
            .map(|c| c.date.day())
            .collect();
        assert_eq!(in_month_days.len(), 29);
        assert!(in_month_days.contains(&29));
    }

    #[test]
    fn dst_transition_does_not_lose_a_day() {
        // March 2026: DST starts March 8 (US). The 7th must
        // produce 31 in-month days regardless.
        let grid = build_month_grid(d(2026, 3, 1), d(2026, 3, 1), &[]);
        let count = grid
            .weeks
            .iter()
            .flatten()
            .filter(|c| c.in_view_month)
            .count();
        assert_eq!(count, 31);
        // November 2026: DST ends November 1. 30 in-month days.
        let grid = build_month_grid(d(2026, 11, 1), d(2026, 11, 1), &[]);
        let count = grid
            .weeks
            .iter()
            .flatten()
            .filter(|c| c.in_view_month)
            .count();
        assert_eq!(count, 30);
    }

    // ── Task bucketing ───────────────────────────────────────

    use atrium_core::ScheduledFor;
    use atrium_core::test_support::dummy_task;

    fn task_scheduled(id: i64, scheduled: NaiveDate) -> Task {
        let mut t = dummy_task(id);
        t.scheduled_for = Some(ScheduledFor::Date(scheduled));
        t
    }

    #[test]
    fn tasks_bucket_by_scheduled_date() {
        let viewed = d(2026, 5, 1);
        let tasks = vec![
            task_scheduled(1, d(2026, 5, 9)),
            task_scheduled(2, d(2026, 5, 9)),
            task_scheduled(3, d(2026, 5, 15)),
        ];
        let grid = build_month_grid(viewed, viewed, &tasks);
        let may9 = grid
            .weeks
            .iter()
            .flatten()
            .find(|c| c.date == d(2026, 5, 9))
            .unwrap();
        let may15 = grid
            .weeks
            .iter()
            .flatten()
            .find(|c| c.date == d(2026, 5, 15))
            .unwrap();
        assert_eq!(may9.tasks.len(), 2);
        assert_eq!(may15.tasks.len(), 1);
    }

    #[test]
    fn completed_tasks_omitted_from_calendar() {
        let viewed = d(2026, 5, 1);
        let mut t = task_scheduled(1, d(2026, 5, 9));
        t.completed_at = Some(
            chrono::DateTime::parse_from_rfc3339("2026-05-09T08:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        );
        let grid = build_month_grid(viewed, viewed, &[t]);
        let may9 = grid
            .weeks
            .iter()
            .flatten()
            .find(|c| c.date == d(2026, 5, 9))
            .unwrap();
        assert!(may9.tasks.is_empty());
    }

    #[test]
    fn deadline_only_tasks_omitted_from_calendar() {
        // The calendar uses the When-axis only, matching the
        // paper-calendar idiom. Deadline-only tasks are surfaced
        // in Forecast / Agenda and don't pollute the grid.
        let viewed = d(2026, 5, 1);
        let mut t = dummy_task(1);
        t.deadline = Some(d(2026, 5, 9));
        let grid = build_month_grid(viewed, viewed, &[t]);
        let may9 = grid
            .weeks
            .iter()
            .flatten()
            .find(|c| c.date == d(2026, 5, 9))
            .unwrap();
        assert!(may9.tasks.is_empty());
    }

    #[test]
    fn today_cell_marked() {
        let viewed = d(2026, 5, 1);
        let today = d(2026, 5, 9);
        let grid = build_month_grid(viewed, today, &[]);
        let may9 = grid
            .weeks
            .iter()
            .flatten()
            .find(|c| c.date == today)
            .unwrap();
        assert!(may9.is_today);
        let may10 = grid
            .weeks
            .iter()
            .flatten()
            .find(|c| c.date == d(2026, 5, 10))
            .unwrap();
        assert!(!may10.is_today);
    }

    #[test]
    fn out_of_month_cells_flagged() {
        let grid = build_month_grid(d(2026, 5, 1), d(2026, 5, 1), &[]);
        // April 27 is the grid anchor; should be in the first week
        // and flagged out-of-view-month.
        let cell = grid
            .weeks
            .iter()
            .flatten()
            .find(|c| c.date == d(2026, 4, 27))
            .unwrap();
        assert!(!cell.in_view_month);
    }
}
