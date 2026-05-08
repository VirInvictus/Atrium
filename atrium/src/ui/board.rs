// SPDX-License-Identifier: MIT
//! Slice D1 GUI — kanban board page (v0.6.0).
//!
//! Renders a saved Perspective whose `renderer = "board"` as a
//! horizontal column layout: one column per configured tag, plus a
//! trailing "Other" column for everything that didn't match. Each
//! column is a vertical task list.
//!
//! v0.6.0 ships a *read-only* board:
//!
//! - Click a row to open it in the Inspector (GTK `Tasks::Open`
//!   action; same as the regular task list's row activation).
//! - The completion checkbox is rendered for state visibility but
//!   isn't interactive yet — toggling completion goes through the
//!   regular list view.
//! - No drag-drop between columns. That's the next slice.
//!
//! The grouping logic lives in `atrium_core::render::group_into_board`
//! — the GUI is a thin adapter on top of the same engine the
//! `atrium-cli kanban` subcommand uses.

use adw::prelude::*;
use atrium_core::{Column, Task};
use gtk::pango;

/// Build the board page widget. Returns a horizontally-scrolling
/// container with one column per configured kanban column plus the
/// trailing `Other` bucket. The window mounts this into the
/// `board_host` AdwBin in the content stack.
///
/// `on_row_click` is a per-row click callback the caller wires to
/// the existing "open in Inspector" path; passing `task.id` so the
/// board can stay loosely coupled to the rest of the window state.
pub fn build_page<F: Fn(i64) + 'static + Clone>(
    perspective_name: &str,
    columns: &[Column<'_>],
    on_row_click: F,
) -> gtk::Widget {
    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(16)
        .build();

    // Page heading — perspective name + total task count.
    let total: usize = columns.iter().map(|c| c.tasks.len()).sum();
    let title = gtk::Label::builder()
        .label(perspective_name)
        .halign(gtk::Align::Start)
        .build();
    title.add_css_class("title-2");
    outer.append(&title);

    let total_label = gtk::Label::builder()
        .label(format!(
            "{} task{} across {} column{}",
            total,
            if total == 1 { "" } else { "s" },
            columns.len(),
            if columns.len() == 1 { "" } else { "s" },
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
        row.append(&build_column(col, on_row_click.clone()));
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

fn build_column<F: Fn(i64) + 'static + Clone>(col: &Column<'_>, on_row_click: F) -> gtk::Widget {
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

    let count = gtk::Label::builder()
        .label(format!("{}", col.tasks.len()))
        .halign(gtk::Align::End)
        .build();
    count.add_css_class("dim-label");
    count.add_css_class("numeric");
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
        let empty = gtk::Label::builder()
            .label("(empty)")
            .halign(gtk::Align::Start)
            .margin_start(6)
            .margin_top(4)
            .margin_bottom(8)
            .build();
        empty.add_css_class("dim-label");
        list.append(&empty);
    } else {
        for t in &col.tasks {
            list.append(&build_row(t, on_row_click.clone()));
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
    card.upcast()
}

fn build_row<F: Fn(i64) + 'static>(task: &Task, on_row_click: F) -> gtk::Widget {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_start(6)
        .margin_end(6)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    row.add_css_class("atrium-board-task-row");

    // Read-only completion indicator. Toggling lives in the regular
    // list view; this is just a state cue so the user can see at a
    // glance whether a task in the board is already done.
    let check = gtk::CheckButton::builder()
        .active(task.completed_at.is_some())
        .sensitive(false)
        .focusable(false)
        .build();
    row.append(&check);

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
    row.append(&title);

    // Click → open in Inspector. We wrap the row in a GestureClick
    // rather than using a Button so the row's visual stays lean.
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_PRIMARY);
    let task_id = task.id;
    click.connect_pressed(move |_, n_press, _, _| {
        // Single click activates — same idiom as the v0.1.15 list
        // view (ListView::activate for fast double-clicks); but the
        // board's click target is the whole row so we don't need
        // the double-click escape hatch.
        if n_press == 1 {
            on_row_click(task_id);
        }
    });
    row.add_controller(click);

    row.upcast()
}
