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

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use atrium_core::db::read_pool::ReadPool;
use atrium_core::{NewTask, QuickEntryTemplate, WorkerHandle};
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
    // v0.18.0 — Phase 18.5 Tier-1 Quick Entry templates. Pre-load
    // the configured templates once. Empty Vec = no picker bar
    // (the modal renders the standard Quick Entry shape exactly
    // like before). Templates fetched eagerly from the read pool
    // so the picker can render synchronously when the modal
    // appears; the read is small (typically 5-25 rows) and
    // happens off the GTK main loop's hot path.
    let templates: Vec<QuickEntryTemplate> = tag_pool
        .as_ref()
        .and_then(|pool| {
            pool.with(atrium_core::db::read::list_quick_entry_templates)
                .ok()
        })
        .unwrap_or_default();
    let active_template: Rc<RefCell<Option<QuickEntryTemplate>>> = Rc::new(RefCell::new(None));

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
    // v0.18.0 — picker bar above the entry. Hidden when no
    // templates are configured. Each button activates its
    // template (sets the active state + pre-fills the entry's
    // text with the template's prefix). Clicking the active
    // template again deactivates it.
    let picker = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    picker.add_css_class("atrium-quickentry-templates");
    picker.set_visible(!templates.is_empty());
    for tmpl in &templates {
        let label = match tmpl.shortcut_key.as_deref() {
            Some(k) => format!("{k} · {}", tmpl.name),
            None => tmpl.name.clone(),
        };
        let button = gtk::ToggleButton::builder()
            .label(&label)
            .css_classes(["flat"])
            .build();
        let entry_for_click = entry.clone();
        let active_for_click = active_template.clone();
        let template_for_click = tmpl.clone();
        button.connect_toggled(move |btn| {
            if btn.is_active() {
                // Activate this template — clear any previous
                // selection in the same row (handled below via
                // siblings_for_picker), pre-fill entry text.
                *active_for_click.borrow_mut() = Some(template_for_click.clone());
                entry_for_click.set_text(&template_for_click.prefix);
                entry_for_click.set_position(-1);
            } else if active_for_click
                .borrow()
                .as_ref()
                .is_some_and(|t| t.id == template_for_click.id)
            {
                *active_for_click.borrow_mut() = None;
                entry_for_click.set_text("");
            }
        });
        picker.append(&button);
    }
    // Mutual-exclusion among picker buttons: deactivating others
    // when one becomes active. Walk the picker's children at
    // toggle time. The closure captures `picker`-by-weak so the
    // dialog drops cleanly even if a button outlives the modal
    // through a stuck signal.
    {
        let picker_for_dedup = picker.downgrade();
        let mut child = picker.first_child();
        while let Some(node) = child {
            if let Some(btn) = node.downcast_ref::<gtk::ToggleButton>() {
                let btn_self = btn.clone();
                let picker_weak = picker_for_dedup.clone();
                btn.connect_toggled(move |b| {
                    if !b.is_active() {
                        return;
                    }
                    let Some(p) = picker_weak.upgrade() else {
                        return;
                    };
                    let mut sibling = p.first_child();
                    while let Some(s) = sibling {
                        if let Some(other) = s.downcast_ref::<gtk::ToggleButton>()
                            && !other.eq(&btn_self)
                            && other.is_active()
                        {
                            other.set_active(false);
                        }
                        sibling = s.next_sibling();
                    }
                });
            }
            child = node.next_sibling();
        }
    }
    body.append(&picker);
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

    // v0.18.0 — `:c` style shortcut sniff. When the entry text
    // starts with `:LETTER` and LETTER matches a template's
    // shortcut_key, activate that template + replace the typed
    // prefix with the template's prefix. The user gets to see
    // the template kick in mid-typing without leaving the
    // keyboard.
    if !templates.is_empty() {
        let templates_for_changed = templates.clone();
        let active_for_changed = active_template.clone();
        let picker_for_changed = picker.downgrade();
        entry.connect_changed(move |e| {
            // Only attempt if no template is active yet — once
            // activated, the user is typing into the template's
            // pre-fill and we don't re-interpret leading `:`
            // characters as triggers.
            if active_for_changed.borrow().is_some() {
                return;
            }
            let text = e.text().to_string();
            // Matches `:` + single char, optionally followed by
            // a space + rest. The trigger consumes the `:c `
            // prefix; bare `:c` (no trailing space yet) is
            // ignored so a user typing `:c` mid-word doesn't
            // get hijacked.
            let Some(rest) = text.strip_prefix(':') else {
                return;
            };
            let trigger_char = rest.chars().next();
            let Some(trigger) = trigger_char else {
                return;
            };
            // The worker validates shortcut keys as single ASCII
            // alphanumerics (`validate_shortcut_key` in worker.rs).
            // Mirror that here so a stray `:🎉 ` or `:.` doesn't
            // attempt template matching that could never succeed.
            if !trigger.is_ascii_alphanumeric() {
                return;
            }
            // Require the user to have committed the trigger
            // with a trailing space (or end-of-text on a single
            // char). Without this, every `:` prefix attempts to
            // activate after each keystroke, which is jarring.
            let after = &rest[trigger.len_utf8()..];
            if !after.is_empty() && !after.starts_with(' ') {
                return;
            }
            let trigger_str = trigger.to_string();
            let Some(template) = templates_for_changed
                .iter()
                .find(|t| t.shortcut_key.as_deref() == Some(trigger_str.as_str()))
            else {
                return;
            };
            // Activate: stash the template, replace the entry
            // text with the template's prefix, mark the matching
            // picker button as active.
            *active_for_changed.borrow_mut() = Some(template.clone());
            e.set_text(&template.prefix);
            e.set_position(-1);
            if let Some(p) = picker_for_changed.upgrade() {
                let mut child = p.first_child();
                while let Some(node) = child {
                    if let Some(btn) = node.downcast_ref::<gtk::ToggleButton>() {
                        // The label is "shortcut · name" or just
                        // name; checking the label for an exact
                        // prefix match lets us identify which
                        // button owns this template without a
                        // separate id-tracking cell.
                        if btn
                            .label()
                            .as_deref()
                            .is_some_and(|l| l.starts_with(&format!("{trigger_str} · ")))
                            && !btn.is_active()
                        {
                            btn.set_active(true);
                        }
                    }
                    child = node.next_sibling();
                }
            }
        });
    }

    // Enter commits.
    let active_for_commit = active_template.clone();
    entry.connect_activate(clone!(
        #[weak]
        dialog,
        move |entry| {
            let raw = entry.text().to_string();
            let template = active_for_commit.borrow().clone();
            commit(&raw, worker.clone(), template);
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

fn commit(raw_input: &str, worker: Option<WorkerHandle>, template: Option<QuickEntryTemplate>) {
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
        // v0.18.0 — apply the active template (if any) on top
        // of the parsed inline syntax. Project routes via
        // `target_project_id`; default_tags merge with the
        // inline `#tag` set (template tags first so the inline
        // tags can override on tag-name conflicts via the
        // ensure_tag idempotent lookup).
        let project_id = template.as_ref().and_then(|t| t.target_project_id);
        let mut tag_names: Vec<String> = template
            .as_ref()
            .map(|t| t.default_tags.clone())
            .unwrap_or_default();
        for name in projected_tags {
            if !tag_names.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
                tag_names.push(name);
            }
        }
        let new = NewTask {
            title: parsed.title,
            scheduled_for: parsed.scheduled_for,
            deadline: parsed.deadline,
            project_id,
            ..NewTask::default()
        };
        match worker.create_task(new).await {
            Ok(task) => {
                debug!(id = task.id, "quick-entry task created");
                if !tag_names.is_empty() {
                    let mut tag_ids = Vec::with_capacity(tag_names.len());
                    for name in tag_names {
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
