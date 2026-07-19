// SPDX-License-Identifier: MIT
//! Slice D1 GUI — kanban board page.
//!
//! Renders a saved Perspective whose `renderer = "board"` as a
//! horizontal column layout: one column per configured tag, plus a
//! trailing "Other" column for everything that didn't match. Each
//! column is a vertical task list.
//!
//! v0.6.0 shipped a minimal read-only board (checkbox + title only,
//! no metadata). v0.6.1 fills the row out:
//!
//! - **Interactive checkbox.** Clicking the checkbox toggles the
//!   task's completion via the worker, same as the regular list
//!   view. The board re-renders on the next `apply_task_changes`.
//! - **Metadata line.** Project name, scheduled date or deadline,
//!   and tag pills (using the same Pango-coloured markup the
//!   regular task list uses) appear underneath the title when any
//!   of them are set.
//! - **Click any row** still opens the Inspector via the supplied
//!   callback (mirroring `win.edit-details-for(i64)`).
//!
//! v0.43.0 enriches the card: a statistics cookie ([done/total],
//! folding subtasks with body checkboxes via the shared
//! `build_cookie_resolver`) and an amber "Blocked" pill (from
//! `read::blocked_task_ids`) now sit on the title line, matching what
//! the regular list rows already show.
//!
//! v0.44.0 adds per-column WIP limits (count/limit header, over-limit
//! flagged red) and v0.45.0 a per-column "Add card" entry that creates a
//! task stamped with the column's tag or status.
//!
//! The grouping logic lives in `atrium_core::render::group_into_board`
//! — the GUI is a thin adapter on top of the same engine the
//! `atrium-cli kanban` subcommand uses.

use std::collections::{BTreeMap, HashMap, HashSet};

use atrium_core::{Column, ScheduledFor, Task, WorkerHandle};
use gtk::gdk;
use gtk::glib;
use gtk::glib::clone;
use gtk::pango;
use gtk::prelude::*;
use tracing::error;

use super::task_list::{TagPillMap, format_tag_names};
use crate::i18n::{gettext, gettext_f, ngettext_f};

/// Drop destination — either one of the configured columns (carry the
/// column's tag name verbatim so the move helper can use it as a
/// case-preserved tag) or the trailing `Other` bucket. Public so the
/// window-side callback closure can pattern-match on it.
#[derive(Debug, Clone)]
pub enum DropDestination {
    Column(String),
    Other,
}

/// Build the board page widget. Returns a horizontally-scrolling
/// container with one column per configured kanban column plus the
/// trailing `Other` bucket. The window mounts this into the
/// `board_host` host box in the content stack.
///
/// `tag_pills` and `project_titles` are read-only references the
/// rows borrow when building their secondary metadata line. `worker`
/// drives the interactive completion checkbox; `None` falls back to
/// a read-only state cue (same shape as v0.6.0).
///
/// `on_row_click` is the per-row click callback (open in Inspector).
// Eight params: the board page legitimately needs the data refs plus
// three callbacks (row click, drop, configure). Splitting into a
// struct would only move the same fields around.
#[allow(clippy::too_many_arguments)]
pub fn build_page<F, D, C, A>(
    perspective_name: &str,
    columns: &[Column<'_>],
    tag_pills: &TagPillMap,
    project_titles: &HashMap<i64, String>,
    subtask_cookies: &HashMap<i64, String>,
    blocked_ids: &HashSet<i64>,
    limits: &BTreeMap<String, u32>,
    worker: Option<WorkerHandle>,
    on_row_click: F,
    on_drop: D,
    on_configure: C,
    on_add: A,
) -> gtk::Widget
where
    F: Fn(i64) + 'static + Clone,
    D: Fn(i64, DropDestination, Option<i64>) + 'static + Clone,
    C: Fn() + 'static,
    A: Fn(DropDestination, String) + 'static + Clone,
{
    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(16)
        .build();

    // Page heading — perspective name + a Configure button. The board's
    // columns / axis are otherwise reachable only from the perspective
    // row's right-click menu, which the audit flagged as buried; the
    // button activates the same `win.configure-renderer` action.
    let total: usize = columns.iter().map(|c| c.tasks.len()).sum();
    let heading = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let title = gtk::Label::builder()
        .label(perspective_name)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build();
    title.add_css_class("title-2");
    heading.append(&title);

    let configure = gtk::Button::builder()
        .label(gettext("Configure\u{2026}"))
        .valign(gtk::Align::Center)
        .build();
    configure.add_css_class("flat");
    configure.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Configure board columns",
    ))]);
    configure.connect_clicked(move |_| on_configure());
    heading.append(&configure);
    outer.append(&heading);

    // Two independent plurals can't share one ngettext call, so the
    // count fragments translate separately and compose afterwards.
    let tasks_part = ngettext_f(
        "{n} task",
        "{n} tasks",
        total as u32,
        &[("n", &total.to_string())],
    );
    let columns_part = ngettext_f(
        "{n} column",
        "{n} columns",
        columns.len() as u32,
        &[("n", &columns.len().to_string())],
    );
    // Translators: {tasks} and {columns} are already-translated
    // fragments like "3 tasks" and "2 columns".
    let total_label = gtk::Label::builder()
        .label(gettext_f(
            "{tasks} across {columns}",
            &[("tasks", &tasks_part), ("columns", &columns_part)],
        ))
        .halign(gtk::Align::Start)
        .build();
    total_label.add_css_class("dim-label");
    outer.append(&total_label);

    // Horizontal column row.
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(4)
        .build();

    for col in columns {
        // Case-insensitive limit lookup, matching the core's `limit_for`.
        let limit = limits
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(&col.label))
            .map(|(_, v)| *v);
        row.append(&build_column(
            col,
            tag_pills,
            project_titles,
            subtask_cookies,
            blocked_ids,
            limit,
            worker.clone(),
            on_row_click.clone(),
            on_drop.clone(),
            on_add.clone(),
        ));
    }

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(false)
        .child(&row)
        .build();
    scroller.add_css_class("atrium-board-row-scroll");

    outer.append(&scroller);
    outer.upcast()
}

#[allow(clippy::too_many_arguments)]
fn build_column<F, D, A>(
    col: &Column<'_>,
    tag_pills: &TagPillMap,
    project_titles: &HashMap<i64, String>,
    subtask_cookies: &HashMap<i64, String>,
    blocked_ids: &HashSet<i64>,
    limit: Option<u32>,
    worker: Option<WorkerHandle>,
    on_row_click: F,
    on_drop: D,
    on_add: A,
) -> gtk::Widget
where
    F: Fn(i64) + 'static + Clone,
    D: Fn(i64, DropDestination, Option<i64>) + 'static + Clone,
    A: Fn(DropDestination, String) + 'static + Clone,
{
    // This column's drop/add destination — a configured column carries
    // its label verbatim; the trailing bucket is `Other`. Shared by the
    // drop target and the per-column "Add card" entry.
    let destination = if col.label == atrium_core::OTHER_COLUMN_LABEL {
        DropDestination::Other
    } else {
        DropDestination::Column(col.label.clone())
    };
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .width_request(280)
        .build();
    card.add_css_class("atrium-board-column");
    card.add_css_class("card");

    // Header — column label + count.
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_start(12)
        .margin_end(12)
        .margin_top(10)
        .margin_bottom(2)
        .build();
    header.add_css_class("atrium-board-column-header");

    let label = gtk::Label::builder()
        .label(&col.label)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(pango::EllipsizeMode::End)
        .build();
    label.add_css_class("heading");
    header.append(&label);

    // v0.44.0 — WIP limit display. With a limit set, show `count/limit`
    // and flag the column when it's over its cap (advisory only; drops
    // onto a full column still work).
    let count_text = match limit {
        Some(l) => format!("{}/{}", col.tasks.len(), l),
        None => format!("{}", col.tasks.len()),
    };
    let count = gtk::Label::builder()
        .label(count_text)
        .halign(gtk::Align::End)
        .build();
    count.add_css_class("dim-label");
    count.add_css_class("numeric");
    if let Some(l) = limit
        && col.tasks.len() as u32 > l
    {
        count.remove_css_class("dim-label");
        count.add_css_class("atrium-board-over-limit");
    }
    header.append(&count);

    card.append(&header);

    // Body — list of task rows in a vertically-scrolling region so
    // a column with many tasks doesn't bloat the page height.
    let list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_start(6)
        .margin_end(6)
        .margin_bottom(10)
        .build();

    if col.tasks.is_empty() {
        // Translators: placeholder for a kanban column with no cards.
        let empty = gtk::Label::builder()
            .label(gettext("(empty)"))
            .halign(gtk::Align::Start)
            .margin_start(6)
            .margin_top(4)
            .margin_bottom(8)
            .build();
        empty.add_css_class("dim-label");
        list.append(&empty);
    } else {
        for t in &col.tasks {
            list.append(&build_row(
                t,
                tag_pills,
                project_titles,
                subtask_cookies,
                blocked_ids,
                worker.clone(),
                on_row_click.clone(),
                destination.clone(),
                on_drop.clone(),
            ));
        }
    }

    let body_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .height_request(420)
        .vexpand(true)
        .child(&list)
        .build();

    card.append(&body_scroller);

    // v0.45.0 — per-column "Add card" entry. Enter creates a task in
    // this column via the on_add callback (which stamps the column's
    // tag or status). Inline `#tag` / `@date` / `!N` syntax still works.
    let add_entry = gtk::Entry::builder()
        .placeholder_text(gettext("Add card\u{2026}"))
        .margin_start(6)
        .margin_end(6)
        .margin_bottom(8)
        .build();
    add_entry.add_css_class("atrium-board-add-card");
    let add_dest = destination.clone();
    let add_cb = on_add.clone();
    add_entry.connect_activate(move |e| {
        let text = e.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        add_cb(add_dest.clone(), text);
        e.set_text("");
    });
    card.append(&add_entry);

    // v0.6.3 — drop target for drag-drop between columns. Each card
    // accepts an i64 task id; the on_drop callback is responsible
    // for the tag-set rewrite via the worker.
    // Column-level drop = append to the end of this column (no specific
    // card underneath the pointer). Per-card drop targets (build_row)
    // handle "insert before this card" for intra-column ordering.
    let drop_target = gtk::DropTarget::new(i64::static_type(), gdk::DragAction::MOVE);
    let drop_cb = on_drop.clone();
    drop_target.connect_drop(move |_, value, _, _| {
        let Ok(task_id) = value.get::<i64>() else {
            return false;
        };
        drop_cb(task_id, destination.clone(), None);
        true
    });
    card.add_controller(drop_target);

    card.upcast()
}

#[allow(clippy::too_many_arguments)]
fn build_row<F, D>(
    task: &Task,
    tag_pills: &TagPillMap,
    project_titles: &HashMap<i64, String>,
    subtask_cookies: &HashMap<i64, String>,
    blocked_ids: &HashSet<i64>,
    worker: Option<WorkerHandle>,
    on_row_click: F,
    destination: DropDestination,
    on_drop: D,
) -> gtk::Widget
where
    F: Fn(i64) + 'static,
    D: Fn(i64, DropDestination, Option<i64>) + 'static,
{
    // Rows are drag *sources* only; the matching drop target lives
    // on the parent column card so the destination is unambiguous.
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_start(4)
        .margin_end(4)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    row.add_css_class("atrium-board-task-row");

    // Top line: checkbox + title.
    let top = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    let check = gtk::CheckButton::builder()
        .active(task.completed_at.is_some())
        .focusable(false)
        .build();
    check.set_sensitive(worker.is_some());
    if let Some(worker) = worker.clone() {
        let task_id = task.id;
        check.connect_toggled(move |_| {
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker.toggle_complete(task_id).await {
                    error!(?e, task_id, "kanban toggle_complete failed");
                }
            });
        });
    }
    top.append(&check);

    let title = gtk::Label::builder()
        .label(&task.title)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .lines(2)
        .ellipsize(pango::EllipsizeMode::End)
        .build();
    if task.completed_at.is_some() {
        title.add_css_class("dim-label");
    }
    top.append(&title);

    // v0.43.0 — statistics cookie [done/total], reusing the same
    // string the list rows show (subtask + body-checkbox counts folded
    // together upstream in `build_cookie_resolver`). Only shown when the
    // task actually has a non-empty cookie.
    if let Some(cookie) = subtask_cookies.get(&task.id).filter(|c| !c.is_empty()) {
        let cookie_label = gtk::Label::builder().label(cookie).build();
        cookie_label.add_css_class("atrium-task-cookie");
        cookie_label.add_css_class("dim-label");
        top.append(&cookie_label);
    }

    // v0.43.0 — "Blocked" pill for tasks with an open prerequisite,
    // reusing the list's amber `.atrium-task-blocked` style so the board
    // and list agree on what "blocked" looks like.
    if blocked_ids.contains(&task.id) {
        let blocked_pill = gtk::Label::builder().label(gettext("Blocked")).build();
        blocked_pill.add_css_class("atrium-task-blocked");
        top.append(&blocked_pill);
    }

    row.append(&top);

    // Metadata line — project, date chip, tag pills. Only built
    // when there's something to show; otherwise we skip the second
    // row entirely so all-empty tasks stay tight.
    let metadata = build_metadata_line(task, tag_pills, project_titles);
    if let Some(meta) = metadata {
        row.append(&meta);
    }

    // Whole-row click → open in Inspector. We attach the gesture to
    // the outer row Box so the user can click anywhere except the
    // checkbox to activate.
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_PRIMARY);
    let task_id = task.id;
    // The checkbox handles its own click via the toggled signal; we
    // don't want a click on the checkbox to also open the Inspector.
    // GTK lets the checkbox's controller fire first and consume the
    // event, so the row-level controller only sees clicks that
    // didn't land on a child handler — which is the behaviour we
    // want here.
    click.connect_pressed(move |_, n_press, _, _| {
        if n_press == 1 {
            on_row_click(task_id);
        }
    });
    row.add_controller(click);

    // v0.6.3 — drag source. Emits the task id so a column-level drop
    // target can route it through the move helper. We don't attach
    // a custom drag icon here (default uses the row widget's
    // snapshot) — same idiom as the forecast page's drag rows.
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::MOVE);
    drag.connect_prepare(clone!(
        #[strong]
        task_id,
        move |_, _, _| Some(gdk::ContentProvider::for_value(&task_id.to_value()))
    ));
    row.add_controller(drag);

    // v0.46.0 — per-card drop target. Dropping another card here inserts
    // it immediately before this card: intra-column reorder when the two
    // share a column, or a cross-column move landing at this position.
    // Dropping a card on itself is a no-op. Fires before the column-level
    // target (child controller wins), so a drop on empty column space
    // still appends via the column target.
    let card_drop = gtk::DropTarget::new(i64::static_type(), gdk::DragAction::MOVE);
    card_drop.connect_drop(move |_, value, _, _| {
        let Ok(dragged) = value.get::<i64>() else {
            return false;
        };
        if dragged == task_id {
            return true;
        }
        on_drop(dragged, destination.clone(), Some(task_id));
        true
    });
    row.add_controller(card_drop);

    row.upcast()
}

/// Build the secondary metadata line shown under the title.
/// Returns `None` when the task has no project, no scheduled date,
/// no deadline, and no tags — keeps "naked" tasks visually compact.
fn build_metadata_line(
    task: &Task,
    tag_pills: &TagPillMap,
    project_titles: &HashMap<i64, String>,
) -> Option<gtk::Widget> {
    let project_name: Option<&String> = task.project_id.and_then(|pid| project_titles.get(&pid));
    let date_chip = format_date_chip(task);
    let pills = tag_pills.get(&task.id).cloned().unwrap_or_default();

    if project_name.is_none() && date_chip.is_none() && pills.is_empty() {
        return None;
    }

    let line = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .margin_start(28) // align with title (after the checkbox)
        .build();
    line.add_css_class("atrium-board-row-meta");
    line.add_css_class("dim-label");

    if let Some(name) = project_name {
        let label = gtk::Label::builder()
            .label(name)
            .ellipsize(pango::EllipsizeMode::End)
            .build();
        label.add_css_class("atrium-board-row-project");
        line.append(&label);
    }

    if let Some(chip) = date_chip {
        let label = gtk::Label::builder().label(&chip).build();
        label.add_css_class("atrium-board-row-date");
        line.append(&label);
    }

    if !pills.is_empty() {
        let tags_label = gtk::Label::builder()
            .use_markup(true)
            .ellipsize(pango::EllipsizeMode::End)
            .label(format_tag_names(&pills))
            .build();
        tags_label.add_css_class("atrium-board-row-tags");
        line.append(&tags_label);
    }

    Some(line.upcast())
}

/// Compose a single date chip showing the most-relevant date for
/// the task. Deadline trumps scheduled (a deadline is a harder
/// commitment); Someday renders as the literal "Someday" label.
fn format_date_chip(task: &Task) -> Option<String> {
    if let Some(deadline) = task.deadline {
        return Some(format!("⏰ {deadline}"));
    }
    match &task.scheduled_for {
        Some(ScheduledFor::Date(d)) => Some(format!("📅 {d}")),
        Some(ScheduledFor::Someday) => Some(gettext("Someday")),
        None => None,
    }
}
