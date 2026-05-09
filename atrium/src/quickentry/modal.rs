// SPDX-License-Identifier: MIT
//! Quick Entry capture modal.
//!
//! `Ctrl+Alt+Space` opens a small `adw::Window` with a single entry.
//! Type a task description (with optional inline `#tag` / `@today` /
//! `@yyyy-mm-dd` / `@deadline …` syntax — the same parser the
//! bottom-of-list entry uses), press Enter, and the task lands in
//! Inbox via `worker.create_task` + `set_task_tags`. Esc dismisses.
//!
//! Modal is `set_modal(false)` and `transient_for(main)` — closes
//! cleanly without GTK's strict modal grab. v1.0's `atriumd` daemon
//! will provide true OS-global capture (Phase 20); for now the
//! shortcut only fires while Atrium is the focused application.

use adw::prelude::*;
use atrium_core::db::read_pool::ReadPool;
use atrium_core::{NewTask, WorkerHandle};
use gtk::glib;
use gtk::glib::clone;
use tracing::{debug, error, warn};

use atrium_inline as parser;

/// Open the Quick Entry modal anchored to `parent`. Returns
/// immediately; commit/dismiss runs through the GTK event loop.
///
/// `tag_pool` is consulted by the v0.13 Slice 3 tab-completion
/// popover for `#tag` candidates — pass `None` to disable tag
/// completion (the `@`/`!` candidates work either way).
pub fn open(
    parent: &impl IsA<gtk::Window>,
    worker: Option<WorkerHandle>,
    tag_pool: Option<ReadPool>,
) {
    let dialog = adw::Window::builder()
        .title("Quick Entry")
        .transient_for(parent)
        .modal(false)
        .default_width(480)
        .default_height(120)
        .resizable(false)
        .css_classes(["atrium-quickentry-window"])
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(
        &adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build(),
    );

    let entry = gtk::Entry::builder()
        .placeholder_text("New task. Try #tag, @today, @deadline 2026-04-15…")
        .activates_default(true)
        .hexpand(true)
        .build();

    let hint = gtk::Label::builder()
        .label("Press Enter to drop into Inbox · Esc to dismiss")
        .halign(gtk::Align::Start)
        .build();
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(16)
        .margin_end(16)
        .margin_top(8)
        .margin_bottom(16)
        .build();
    body.append(&entry);
    body.append(&hint);
    toolbar.set_content(Some(&body));
    dialog.set_content(Some(&toolbar));

    // Esc dismisses.
    let key_controller = gtk::EventControllerKey::new();
    key_controller.connect_key_pressed(clone!(
        #[weak]
        dialog,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                dialog.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    ));
    dialog.add_controller(key_controller);

    // Enter commits.
    entry.connect_activate(clone!(
        #[weak]
        dialog,
        move |entry| {
            let raw = entry.text().to_string();
            commit(&raw, worker.clone());
            dialog.close();
        }
    ));

    // v0.13 Slice 3 — attach the inline-syntax tab-completion
    // popover. Must happen before `present()` so the popover's
    // parent is registered while the entry is still being set up.
    crate::ui::inline_complete::attach(&entry, tag_pool);

    dialog.present();
    entry.grab_focus();
}

fn commit(raw_input: &str, worker: Option<WorkerHandle>) {
    let parsed = parser::parse(raw_input);
    let projected_tags = parsed.projected_tag_names();
    if parsed.title.is_empty() && projected_tags.is_empty() {
        debug!("quick entry: empty input, ignoring");
        return;
    }
    let Some(worker) = worker else {
        warn!("quick entry: worker unavailable; capture dropped");
        return;
    };
    glib::MainContext::default().spawn_local(async move {
        let new = NewTask {
            title: parsed.title,
            scheduled_for: parsed.scheduled_for,
            deadline: parsed.deadline,
            ..NewTask::default()
        };
        match worker.create_task(new).await {
            Ok(task) => {
                debug!(id = task.id, "quick-entry task created");
                if !projected_tags.is_empty() {
                    let mut tag_ids = Vec::with_capacity(projected_tags.len());
                    for name in projected_tags {
                        match worker.ensure_tag(name).await {
                            Ok(t) => tag_ids.push(t.id),
                            Err(e) => warn!(?e, "ensure_tag (quick entry) failed; skipping"),
                        }
                    }
                    if !tag_ids.is_empty()
                        && let Err(e) = worker.set_task_tags(task.id, tag_ids).await
                    {
                        error!(?e, task_id = task.id, "set_task_tags (quick entry) failed");
                    }
                }
            }
            Err(e) => error!(?e, "create_task (quick entry) failed"),
        }
    });
}
