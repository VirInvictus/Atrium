// SPDX-License-Identifier: MIT
//! Per-task tag editor (Phase 7g).
//!
//! In-window modal `adw::Dialog` with a checkbox list of every
//! existing tag plus an entry to add new ones. Opens from the
//! task-row right-click menu or the `Ctrl+T` accelerator. Apply
//! dispatches `ensure_tag` for each new name then `set_task_tags`
//! with the resulting id list — a single transactional
//! `SetTaskTags` per accept, captured for undo by the caller if
//! it wants to.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;
use atrium_core::{Tag, WorkerHandle};
use gtk::glib;
use gtk::glib::clone;
use tracing::error;

use crate::i18n::{gettext, gettext_f};

/// Open the tag editor for `task_id`.
///
/// `task_title`: the task's current title, surfaced in the dialog
///   header so the user knows which row they're editing.
/// `current_tag_ids`: the set of tags already attached to the task.
/// `all_tags`: every tag in the library, sorted by name.
/// `worker`: needed for `ensure_tag` (new tag creation) and
///   `set_task_tags` (the apply step).
pub fn open(
    parent: &impl IsA<gtk::Widget>,
    worker: WorkerHandle,
    task_id: i64,
    task_title: String,
    current_tag_ids: Vec<i64>,
    all_tags: Vec<Tag>,
) {
    let dialog = adw::Dialog::builder()
        .title(gettext("Edit Tags"))
        .content_width(380)
        .content_height(420)
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());

    // Header: which task we're editing. Truncated so a long title
    // doesn't break layout; the full text is on the tooltip.
    let header_label = gtk::Label::builder()
        // Translators: {title} is the task being edited.
        .label(gettext_f(
            "Editing: {title}",
            &[("title", &truncate(&task_title, 64))],
        ))
        .tooltip_text(task_title.clone())
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    header_label.add_css_class("dim-label");
    header_label.add_css_class("caption");

    // Selected-id state shared between the existing checkboxes and
    // the "add new tag" handler so adding+checking behave the same
    // way. New rows are appended on the fly via Rc<RefCell<…>>.
    let selected: Rc<RefCell<HashSet<i64>>> =
        Rc::new(RefCell::new(current_tag_ids.iter().copied().collect()));
    // New tag names to ensure_tag on apply. Stored separately so we
    // can resolve them to ids in the right order.
    let new_names: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    let list_box = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();

    // Populate with every existing tag.
    for tag in &all_tags {
        list_box.append(&build_tag_row(tag, &selected));
    }

    // If the library is empty, surface a hint instead of a blank list.
    if all_tags.is_empty() {
        let empty = gtk::Label::builder()
            .label(gettext("No tags yet. Add one below."))
            .halign(gtk::Align::Center)
            .build();
        empty.add_css_class("dim-label");
        list_box.append(&gtk::ListBoxRow::builder().child(&empty).build());
    }

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list_box)
        .build();

    // Add-new-tag entry: typing a name and pressing Enter (or
    // clicking the + button) appends a row to the list and marks
    // it selected. We don't dispatch ensure_tag yet — that happens
    // on Apply so a Cancel discards everything.
    let new_entry = gtk::Entry::builder()
        .placeholder_text(gettext("Add a new tag…"))
        .hexpand(true)
        .build();
    let plus_button = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        // Translators: "(Enter)" names the key that triggers the same
        // action.
        .tooltip_text(gettext("Add this tag (Enter)"))
        .css_classes(["flat"])
        .build();
    plus_button.update_property(&[gtk::accessible::Property::Label(&gettext("Add this tag"))]);

    let add_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    add_row.append(&new_entry);
    add_row.append(&plus_button);

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(12)
        .margin_end(12)
        .margin_top(8)
        .margin_bottom(8)
        .build();
    body.append(&header_label);
    body.append(&scroll);
    body.append(&add_row);

    // Footer with Cancel / Apply.
    let cancel_button = gtk::Button::builder().label(gettext("Cancel")).build();
    let apply_button = gtk::Button::builder()
        .label(gettext("Apply"))
        .css_classes(["suggested-action"])
        .build();
    let footer = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .margin_start(12)
        .margin_end(12)
        .margin_top(8)
        .margin_bottom(12)
        .build();
    footer.append(&cancel_button);
    footer.append(&apply_button);
    body.append(&footer);

    toolbar.set_content(Some(&body));
    dialog.set_child(Some(&toolbar));

    // Cancel dismisses without writes. AdwDialog handles Esc-to-close
    // itself, so we don't install a manual key controller.
    cancel_button.connect_clicked(clone!(
        #[weak]
        dialog,
        move |_| {
            dialog.close();
        }
    ));

    // Add-new-tag flow. The new name is added to `new_names` (so
    // Apply knows to ensure_tag it) and a new checked row is
    // appended to the list. Empty / duplicate names are rejected.
    let add_new = clone!(
        #[strong]
        list_box,
        #[strong]
        new_names,
        #[strong]
        selected,
        #[strong]
        all_tags,
        #[weak]
        new_entry,
        move || {
            let raw = new_entry.text().to_string();
            let name = raw.trim().trim_start_matches('#').to_string();
            if name.is_empty() {
                return;
            }
            // Reject if it duplicates an existing tag (case-
            // insensitive). Pre-check the existing row instead.
            if let Some(existing) = all_tags.iter().find(|t| t.name.eq_ignore_ascii_case(&name)) {
                selected.borrow_mut().insert(existing.id);
                if let Some(row) = list_box.row_at_index(
                    all_tags
                        .iter()
                        .position(|t| t.id == existing.id)
                        .unwrap_or(0) as i32,
                ) && let Some(check) = first_check_button(&row)
                {
                    check.set_active(true);
                }
                new_entry.set_text("");
                return;
            }
            // Reject duplicate of a name already in the new-names buffer.
            if new_names
                .borrow()
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&name))
            {
                new_entry.set_text("");
                return;
            }

            new_names.borrow_mut().push(name.clone());
            list_box.append(&build_pending_tag_row(&name));
            new_entry.set_text("");
        }
    );

    plus_button.connect_clicked({
        let add_new = add_new.clone();
        move |_| add_new()
    });
    new_entry.connect_activate(move |_| add_new());

    // Apply: ensure_tag for each pending new name, then
    // set_task_tags with the resolved id set. Closes on success;
    // on failure the dialog stays open so the user can try again.
    apply_button.connect_clicked(clone!(
        #[weak]
        dialog,
        #[strong]
        worker,
        #[strong]
        selected,
        #[strong]
        new_names,
        move |_| {
            let worker = worker.clone();
            let selected = selected.clone();
            let new_names = new_names.clone();
            glib::MainContext::default().spawn_local(async move {
                // Snapshot the borrowed state first so no RefCell
                // refs straddle the .await boundaries (clippy
                // `await_holding_refcell_ref`).
                let mut ids: Vec<i64> = selected.borrow().iter().copied().collect();
                let pending: Vec<String> = new_names.borrow().clone();
                for name in pending {
                    match worker.ensure_tag(name.clone()).await {
                        Ok(tag) => ids.push(tag.id),
                        Err(e) => {
                            error!(?e, name = %name, "ensure_tag failed during tag editor apply");
                            return;
                        }
                    }
                }
                if let Err(e) = worker.set_task_tags(task_id, ids).await {
                    error!(?e, task_id, "set_task_tags failed during tag editor apply");
                    return;
                }
                let _ = dialog.close();
            });
        }
    ));

    new_entry.grab_focus();
    dialog.present(Some(parent));
}

fn build_tag_row(tag: &Tag, selected: &Rc<RefCell<HashSet<i64>>>) -> gtk::ListBoxRow {
    let check = gtk::CheckButton::builder()
        .label(format!("#{}", tag.name))
        .active(selected.borrow().contains(&tag.id))
        .hexpand(true)
        .build();
    let id = tag.id;
    let selected = selected.clone();
    check.connect_toggled(move |b| {
        if b.is_active() {
            selected.borrow_mut().insert(id);
        } else {
            selected.borrow_mut().remove(&id);
        }
    });
    gtk::ListBoxRow::builder()
        .child(&check)
        .activatable(false)
        .build()
}

/// Build a pre-checked, non-interactive row representing a new tag
/// that hasn't been written to the DB yet. Apply turns these into
/// real tags via `ensure_tag` + `set_task_tags`.
fn build_pending_tag_row(name: &str) -> gtk::ListBoxRow {
    let label = gtk::Label::builder()
        // Translators: {name} is the tag being created; the leading "#"
        // is Atrium's tag syntax — keep both. Only "new" translates.
        .label(gettext_f("#{name}  · new", &[("name", name)]))
        .halign(gtk::Align::Start)
        .build();
    label.add_css_class("dim-label");
    let check = gtk::CheckButton::builder()
        .child(&label)
        .active(true)
        .sensitive(false)
        .hexpand(true)
        .build();
    gtk::ListBoxRow::builder()
        .child(&check)
        .activatable(false)
        .build()
}

fn first_check_button(row: &gtk::ListBoxRow) -> Option<gtk::CheckButton> {
    let child = row.child()?;
    child.downcast::<gtk::CheckButton>().ok()
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_strings_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_inserts_ellipsis_at_boundary() {
        assert_eq!(truncate("Buy milk and eggs", 8), "Buy mil…");
    }

    #[test]
    fn truncate_handles_unicode_boundary() {
        assert_eq!(truncate("café-au-lait", 6), "café-…");
    }
}
