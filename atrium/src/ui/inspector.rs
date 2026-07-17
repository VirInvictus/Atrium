// SPDX-License-Identifier: MIT
//! Per-task Inspector dialog (Phase 7i).
//!
//! An in-window modal `adw::Dialog` exposing the editable Simple
//! Mode fields that have no other UI surface today: title, notes,
//! schedule (When), deadline, and project assignment. Tags delegate
//! to the existing Phase 7g tag editor via an "Edit Tags…" button —
//! re-implementing the picker inside the inspector would duplicate
//! logic and waste vertical space.
//!
//! Open paths:
//!   - double-click on a task row (per-row gesture in
//!     `task_list::build_factory`),
//!   - right-click the row → *Edit Details…*,
//!   - `Ctrl+I` while a row is focused / first-selected.
//!
//! `adw::Dialog` (vs the v0.0.35–36 `adw::Window` + `transient_for` +
//! `modal(true)` shape) gets us the libadwaita-standard in-window
//! presentation: solid window-bg even when the content rows are
//! narrower than the dialog, automatic Esc-to-close, slide/fade
//! animation that matches every other modal in the platform.
//!
//! Apply dispatches one `worker.update_task(TaskUpdate { … })` with
//! exactly the fields the user changed.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use atrium_core::{
    Project, ScheduledFor, Task, TaskUpdate, WorkerHandle, parse_body_checkboxes, parse_body_links,
    toggle_body_checkbox,
};
use chrono::NaiveDate;
use gtk::glib;
use gtk::glib::clone;
use gtk::pango;
use tracing::error;

use crate::i18n::{gettext, ngettext_f};

/// Open the inspector for `task`. Loads of `all_projects` happen
/// in the caller (window) so the dialog itself stays free of
/// read-pool concerns. `on_edit_tags` is invoked when the user
/// hits the "Edit Tags…" button — the caller routes that to the
/// existing tag editor with the right pre-loaded state.
pub fn open<F, N>(
    parent: &impl IsA<gtk::Widget>,
    worker: WorkerHandle,
    task: Task,
    all_projects: Vec<Project>,
    current_tag_count: usize,
    on_edit_tags: F,
    on_navigate_uuid: N,
) where
    F: Fn(i64) + 'static,
    N: Fn(String) + 'static,
{
    let dialog = adw::Dialog::builder()
        .title(gettext("Edit Task"))
        .content_width(560)
        .content_height(640)
        .build();

    // ── Header bar with explicit Cancel / Apply ──────────────────
    // Buttons in the header bar mirror the GNOME pattern; the form
    // area below stays free for content. AdwDialog supplies its own
    // close-button + Esc handling, so we hide the libadwaita-default
    // chrome buttons and route the user through Cancel / Apply.
    let cancel_button = gtk::Button::builder().label(gettext("Cancel")).build();
    let apply_button = gtk::Button::builder()
        .label(gettext("Apply"))
        .css_classes(["suggested-action"])
        .build();
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&cancel_button);
    header.pack_end(&apply_button);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);

    // ── Title (owned entry row inside its own group) ─────────────
    let (title_row, title_entry) = crate::ui::rows::entry_row(&gettext("Title"), &task.title);
    let title_group = crate::ui::rows::group(None, None);
    title_group.add(&title_row);

    // ── Schedule + Deadline + Project (one group) ────────────────
    let schedule_state: Rc<RefCell<Option<ScheduledFor>>> =
        Rc::new(RefCell::new(task.scheduled_for));
    let schedule_button = build_schedule_button(&schedule_state);
    schedule_button.add_css_class("flat");
    let schedule_row = crate::ui::rows::row(
        &gettext("Schedule"),
        None,
        Some(schedule_button.upcast_ref()),
    );

    let deadline_state: Rc<RefCell<Option<NaiveDate>>> = Rc::new(RefCell::new(task.deadline));
    let deadline_button = build_date_button(&deadline_state, format_deadline_label);
    deadline_button.add_css_class("flat");
    let deadline_row = crate::ui::rows::row(
        &gettext("Deadline"),
        None,
        Some(deadline_button.upcast_ref()),
    );

    // No defer_until editor here: `defer_until` is a Builder-only
    // field (spec §3.1 / §4 — "Builder-only; hidden in Simple"). The
    // Simple inspector deliberately omits it so Simple Mode stays the
    // calm, no-defer-dates surface the spec promises; the Builder
    // Inspector pane exposes it. The stored value is untouched.

    // Project — owned combo row (title + dropdown).
    let (project_row, project_dd) = build_project_combo_row(&all_projects, task.project_id);

    let dates_group = crate::ui::rows::group(None, None);
    dates_group.add(&schedule_row);
    dates_group.add(&deadline_row);
    dates_group.add(&project_row);

    // ── Tags row (its own group, with Edit Tags… suffix) ─────────
    let tag_count_text = if current_tag_count == 0 {
        gettext("No tags")
    } else {
        ngettext_f(
            "{n} tag",
            "{n} tags",
            current_tag_count as u32,
            &[("n", &current_tag_count.to_string())],
        )
    };
    let edit_tags_button = gtk::Button::builder()
        .label(gettext("Edit Tags…"))
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    let tags_row = crate::ui::rows::row(
        &gettext("Tags"),
        Some(&tag_count_text),
        Some(edit_tags_button.upcast_ref()),
    );
    let tags_group = crate::ui::rows::group(None, None);
    tags_group.add(&tags_row);

    // ── Notes (its own group with a header + a card-styled
    //    GtkTextView). AdwPreferencesGroup with title "Notes" gives
    //    the standard form-section header; the TextView lives in a
    //    ScrolledWindow with the `view` + `card` classes so it
    //    reads as a writable surface, not floating text.
    let notes_buffer = gtk::TextBuffer::builder().text(&task.note).build();
    let notes_view = gtk::TextView::builder()
        .buffer(&notes_buffer)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(10)
        .bottom_margin(10)
        .left_margin(10)
        .right_margin(10)
        .build();
    notes_view.add_css_class("atrium-inspector-notes");
    notes_view.add_css_class("atrium-note-body");
    let notes_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&notes_view)
        .min_content_height(180)
        .build();
    notes_scroll.add_css_class("card");
    notes_scroll.add_css_class("view");
    let notes_group = crate::ui::rows::group(Some(&gettext("Notes")), None);
    notes_group.add(&notes_scroll);

    // ── v0.15.0 — Body checkboxes (Phase 18.5 Tier-2). Identical
    // shape to the Builder Mode pane minus the immediate worker
    // dispatch — Simple Mode is modal, so a checkbox toggle just
    // edits the buffer text. The dialog's Apply button picks up
    // the resulting note string; Cancel discards both text edits
    // and toggles together (Apply/Cancel transactional surface).
    let checklist_group = Rc::new(crate::ui::rows::group(Some(&gettext("Checklist")), None));
    let rebuild_subtasks = Rc::new({
        let buffer = notes_buffer.clone();
        let group = checklist_group.clone();
        move || {
            group.clear();
            let body = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            let checkboxes = parse_body_checkboxes(&body);
            if checkboxes.is_empty() {
                group.widget().set_visible(false);
                return;
            }
            group.widget().set_visible(true);
            for cb in checkboxes {
                let check = gtk::CheckButton::builder()
                    .active(cb.state.is_done())
                    .valign(gtk::Align::Center)
                    .build();
                check.set_inconsistent(matches!(
                    cb.state,
                    atrium_core::CheckboxState::Indeterminate
                ));
                let line_index = cb.line_index;
                let buffer_for_click = buffer.clone();
                check.connect_toggled(move |_| {
                    let current = buffer_for_click
                        .text(
                            &buffer_for_click.start_iter(),
                            &buffer_for_click.end_iter(),
                            false,
                        )
                        .to_string();
                    let updated = toggle_body_checkbox(&current, line_index);
                    if updated == current {
                        return;
                    }
                    buffer_for_click.set_text(&updated);
                });
                let label = gtk::Label::builder()
                    .label(&cb.label)
                    .xalign(0.0)
                    .wrap(true)
                    .hexpand(true)
                    .build();
                let hbox = gtk::Box::builder()
                    .orientation(gtk::Orientation::Horizontal)
                    .spacing(12)
                    .margin_top(8)
                    .margin_bottom(8)
                    .margin_start(12)
                    .margin_end(12)
                    .build();
                hbox.append(&check);
                hbox.append(&label);
                let row = gtk::ListBoxRow::builder()
                    .activatable(false)
                    .child(&hbox)
                    .build();
                group.add(&row);
            }
        }
    });
    rebuild_subtasks();
    let rebuild_for_changed = rebuild_subtasks.clone();
    notes_buffer.connect_changed(move |_| {
        rebuild_for_changed();
    });

    // ── v0.19.0 — Phase 18.5 Tier-2 Org-link rendering.
    // Same shape as inspector_pane.rs's link wiring (see there
    // for the full rationale): tag link spans, click resolves
    // to UUID, navigation callback handles the rest. Simple
    // Mode dialog dismisses on link-click navigation since the
    // dialog is modal — opening another inspector for the
    // linked task is the right semantic.
    let on_navigate_uuid = Rc::new(on_navigate_uuid);
    let link_tag = notes_buffer
        .create_tag(Some("link"), &[("underline", &pango::Underline::Single)])
        .expect("link tag created exactly once per buffer");
    link_tag.set_foreground(Some("@accent_color"));
    let apply_link_tags = {
        let buffer = notes_buffer.clone();
        let tag = link_tag.clone();
        Rc::new(move || {
            let body = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            buffer.remove_tag(&tag, &buffer.start_iter(), &buffer.end_iter());
            for link in parse_body_links(&body) {
                let start_char = body[..link.range.start].chars().count() as i32;
                let end_char = body[..link.range.end].chars().count() as i32;
                let start_iter = buffer.iter_at_offset(start_char);
                let end_iter = buffer.iter_at_offset(end_char);
                buffer.apply_tag(&tag, &start_iter, &end_iter);
            }
        })
    };
    apply_link_tags();
    {
        let apply = apply_link_tags.clone();
        notes_buffer.connect_changed(move |_| apply());
    }
    let click_gesture = gtk::GestureClick::builder().button(1).build();
    let view_for_click = notes_view.clone();
    let buffer_for_click = notes_buffer.clone();
    let navigate_for_click = on_navigate_uuid.clone();
    let dialog_for_click = dialog.clone();
    click_gesture.connect_released(move |_, _, x, y| {
        let (bx, by) =
            view_for_click.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        let Some(iter) = view_for_click.iter_at_location(bx, by) else {
            return;
        };
        let body = buffer_for_click
            .text(
                &buffer_for_click.start_iter(),
                &buffer_for_click.end_iter(),
                false,
            )
            .to_string();
        let click_char = iter.offset() as usize;
        for link in parse_body_links(&body) {
            let start_char = body[..link.range.start].chars().count();
            let end_char = body[..link.range.end].chars().count();
            if click_char >= start_char && click_char < end_char {
                navigate_for_click(link.target_uuid);
                dialog_for_click.close();
                return;
            }
        }
    });
    notes_view.add_controller(click_gesture);

    // ── Owned page container holds the five groups; the Page brings
    //    its own vertical scrolling.
    let page = crate::ui::rows::page();
    page.add(&title_group);
    page.add(&dates_group);
    page.add(&tags_group);
    page.add(&checklist_group);
    page.add(&notes_group);

    toolbar.set_content(Some(page.widget()));
    dialog.set_child(Some(&toolbar));

    // Cancel dismisses without writes. Esc-to-close is handled by
    // AdwDialog directly (it consumes the keystroke and runs its
    // own close-attempt path) — no manual key controller needed.
    cancel_button.connect_clicked(clone!(
        #[weak]
        dialog,
        move |_| {
            dialog.close();
        }
    ));

    // Edit Tags hand-off. Close the inspector first; the caller's
    // closure opens the tag editor against the same task id.
    let on_edit_tags = Rc::new(on_edit_tags);
    edit_tags_button.connect_clicked(clone!(
        #[weak]
        dialog,
        #[strong]
        on_edit_tags,
        move |_| {
            let _ = dialog.close();
            on_edit_tags(task.id);
        }
    ));

    // Apply — diff against the snapshot we opened with and dispatch
    // a single `update_task`. Empty title is rejected so the user
    // can't accidentally blank the row.
    let original_title = task.title.clone();
    let original_note = task.note.clone();
    let original_schedule = task.scheduled_for;
    let original_deadline = task.deadline;
    let original_project = task.project_id;
    let task_id = task.id;
    apply_button.connect_clicked(clone!(
        #[weak]
        dialog,
        #[weak]
        title_entry,
        #[weak]
        notes_buffer,
        #[weak]
        project_dd,
        #[strong]
        worker,
        #[strong]
        schedule_state,
        #[strong]
        deadline_state,
        #[strong]
        all_projects,
        move |_| {
            let new_title_raw = title_entry.text().to_string();
            let new_title = new_title_raw.trim().to_string();
            if new_title.is_empty() {
                title_entry.add_css_class("error");
                title_entry.grab_focus();
                return;
            }
            let new_note = notes_buffer
                .text(&notes_buffer.start_iter(), &notes_buffer.end_iter(), false)
                .to_string();
            let new_schedule = *schedule_state.borrow();
            let new_deadline = *deadline_state.borrow();
            let new_project = project_id_from_combo_row(&project_dd, &all_projects);

            let mut update = TaskUpdate::new(task_id);
            if new_title != original_title {
                update = update.title(new_title);
            }
            if new_note != original_note {
                update = update.note(new_note);
            }
            if new_schedule != original_schedule {
                update = update.schedule(new_schedule);
            }
            if new_deadline != original_deadline {
                update = update.deadline_value(new_deadline);
            }
            if new_project != original_project {
                update = update.project(new_project);
            }

            if update.is_noop() {
                let _ = dialog.close();
                return;
            }

            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker.update_task(update).await {
                    error!(?e, task_id, "inspector apply failed");
                    return;
                }
                let _ = dialog.close();
            });
        }
    ));

    title_entry.grab_focus();
    dialog.present(Some(parent));
}

// ── helpers ──────────────────────────────────────────────────────

/// Schedule button: shows the current schedule, opens a popover
/// with three presets (Today / Someday / Clear) plus a calendar
/// for arbitrary dates.
fn build_schedule_button(state: &Rc<RefCell<Option<ScheduledFor>>>) -> gtk::MenuButton {
    let label_widget = gtk::Label::builder()
        .label(format_schedule_label(state.borrow().as_ref()))
        .build();
    let button = gtk::MenuButton::builder().child(&label_widget).build();

    let popover = gtk::Popover::new();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .build();

    let today_button = gtk::Button::builder()
        .label(gettext("Today"))
        .css_classes(["flat"])
        .build();
    let tomorrow_button = gtk::Button::builder()
        .label(gettext("Tomorrow"))
        .css_classes(["flat"])
        .build();
    let someday_button = gtk::Button::builder()
        .label(gettext("Someday"))
        .css_classes(["flat"])
        .build();
    let clear_button = gtk::Button::builder()
        .label(gettext("Clear"))
        .css_classes(["flat"])
        .build();

    let calendar = gtk::Calendar::new();

    body.append(&today_button);
    body.append(&tomorrow_button);
    body.append(&someday_button);
    body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    body.append(&calendar);
    body.append(&clear_button);
    popover.set_child(Some(&body));
    button.set_popover(Some(&popover));

    let today = chrono::Local::now().date_naive();
    let tomorrow = today + chrono::Duration::days(1);

    today_button.connect_clicked(clone!(
        #[strong]
        state,
        #[weak]
        label_widget,
        #[weak]
        popover,
        move |_| {
            *state.borrow_mut() = Some(ScheduledFor::Date(today));
            label_widget.set_label(&format_schedule_label(state.borrow().as_ref()));
            popover.popdown();
        }
    ));
    tomorrow_button.connect_clicked(clone!(
        #[strong]
        state,
        #[weak]
        label_widget,
        #[weak]
        popover,
        move |_| {
            *state.borrow_mut() = Some(ScheduledFor::Date(tomorrow));
            label_widget.set_label(&format_schedule_label(state.borrow().as_ref()));
            popover.popdown();
        }
    ));
    someday_button.connect_clicked(clone!(
        #[strong]
        state,
        #[weak]
        label_widget,
        #[weak]
        popover,
        move |_| {
            *state.borrow_mut() = Some(ScheduledFor::Someday);
            label_widget.set_label(&format_schedule_label(state.borrow().as_ref()));
            popover.popdown();
        }
    ));
    clear_button.connect_clicked(clone!(
        #[strong]
        state,
        #[weak]
        label_widget,
        #[weak]
        popover,
        move |_| {
            *state.borrow_mut() = None;
            label_widget.set_label(&format_schedule_label(state.borrow().as_ref()));
            popover.popdown();
        }
    ));
    calendar.connect_day_selected(clone!(
        #[strong]
        state,
        #[weak]
        label_widget,
        #[weak]
        popover,
        move |cal| {
            let date = match calendar_to_naive_date(cal) {
                Some(d) => d,
                None => return,
            };
            *state.borrow_mut() = Some(ScheduledFor::Date(date));
            label_widget.set_label(&format_schedule_label(state.borrow().as_ref()));
            popover.popdown();
        }
    ));

    button
}

fn build_date_button(
    state: &Rc<RefCell<Option<NaiveDate>>>,
    formatter: fn(Option<&NaiveDate>) -> String,
) -> gtk::MenuButton {
    let label_widget = gtk::Label::builder()
        .label(formatter(state.borrow().as_ref()))
        .build();
    let button = gtk::MenuButton::builder().child(&label_widget).build();

    let popover = gtk::Popover::new();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .build();

    let today_button = gtk::Button::builder()
        .label(gettext("Today"))
        .css_classes(["flat"])
        .build();
    let tomorrow_button = gtk::Button::builder()
        .label(gettext("Tomorrow"))
        .css_classes(["flat"])
        .build();
    let clear_button = gtk::Button::builder()
        .label(gettext("Clear"))
        .css_classes(["flat"])
        .build();
    let calendar = gtk::Calendar::new();

    body.append(&today_button);
    body.append(&tomorrow_button);
    body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    body.append(&calendar);
    body.append(&clear_button);
    popover.set_child(Some(&body));
    button.set_popover(Some(&popover));

    let today = chrono::Local::now().date_naive();
    let tomorrow = today + chrono::Duration::days(1);

    today_button.connect_clicked(clone!(
        #[strong]
        state,
        #[weak]
        label_widget,
        #[weak]
        popover,
        move |_| {
            *state.borrow_mut() = Some(today);
            label_widget.set_label(&formatter(state.borrow().as_ref()));
            popover.popdown();
        }
    ));
    tomorrow_button.connect_clicked(clone!(
        #[strong]
        state,
        #[weak]
        label_widget,
        #[weak]
        popover,
        move |_| {
            *state.borrow_mut() = Some(tomorrow);
            label_widget.set_label(&formatter(state.borrow().as_ref()));
            popover.popdown();
        }
    ));
    clear_button.connect_clicked(clone!(
        #[strong]
        state,
        #[weak]
        label_widget,
        #[weak]
        popover,
        move |_| {
            *state.borrow_mut() = None;
            label_widget.set_label(&formatter(state.borrow().as_ref()));
            popover.popdown();
        }
    ));
    calendar.connect_day_selected(clone!(
        #[strong]
        state,
        #[weak]
        label_widget,
        #[weak]
        popover,
        move |cal| {
            let date = match calendar_to_naive_date(cal) {
                Some(d) => d,
                None => return,
            };
            *state.borrow_mut() = Some(date);
            label_widget.set_label(&formatter(state.borrow().as_ref()));
            popover.popdown();
        }
    ));

    button
}

/// Owned combo row with "Inbox (no project)" at index 0 followed by every
/// project. Returns the row (for placement) and its dropdown (pre-selected to
/// the task's current project) for the value query.
fn build_project_combo_row(
    projects: &[Project],
    current: Option<i64>,
) -> (gtk::ListBoxRow, gtk::DropDown) {
    // Translators: first dropdown entry — the task belongs to no project.
    let inbox_label = gettext("Inbox (no project)");
    let mut items: Vec<&str> = vec![inbox_label.as_str()];
    for p in projects {
        items.push(p.title.as_str());
    }
    let (row, dropdown) = crate::ui::rows::combo_row(&gettext("Project"), None, &items);
    let pos: u32 = match current {
        None => 0,
        Some(id) => projects
            .iter()
            .position(|p| p.id == id)
            .map_or(0, |i| (i + 1) as u32),
    };
    dropdown.set_selected(pos);
    (row, dropdown)
}

fn project_id_from_combo_row(dropdown: &gtk::DropDown, projects: &[Project]) -> Option<i64> {
    let selected = dropdown.selected();
    if selected == 0 {
        return None;
    }
    let idx = (selected as usize).saturating_sub(1);
    projects.get(idx).map(|p| p.id)
}

pub(crate) fn format_schedule_label(value: Option<&ScheduledFor>) -> String {
    match value {
        None => gettext("No schedule"),
        Some(ScheduledFor::Someday) => gettext("Someday"),
        Some(ScheduledFor::Date(d)) => d.format("%a · %b %-d, %Y").to_string(),
    }
}

pub(crate) fn format_deadline_label(value: Option<&NaiveDate>) -> String {
    match value {
        None => gettext("No deadline"),
        Some(d) => d.format("%a · %b %-d, %Y").to_string(),
    }
}

/// Phase 11 — defer-until label. Same shape as deadline; v0.6.11
/// rephrased the empty state from "Available now" (which read as a
/// status, not a date — confusing because every undeferred task is
/// "available now") to "Not deferred" so the absence of a date
/// reads as a date-shaped fact.
pub(crate) fn format_defer_label(value: Option<&NaiveDate>) -> String {
    match value {
        None => gettext("Not deferred"),
        Some(d) => d.format("%a · %b %-d, %Y").to_string(),
    }
}

fn calendar_to_naive_date(cal: &gtk::Calendar) -> Option<NaiveDate> {
    let dt = cal.date();
    NaiveDate::from_ymd_opt(dt.year(), dt.month() as u32, dt.day_of_month() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_label_handles_all_variants() {
        assert_eq!(format_schedule_label(None), "No schedule");
        assert_eq!(
            format_schedule_label(Some(&ScheduledFor::Someday)),
            "Someday"
        );
        let d = NaiveDate::from_ymd_opt(2026, 5, 25).unwrap();
        let label = format_schedule_label(Some(&ScheduledFor::Date(d)));
        assert!(label.contains("May"));
        assert!(label.contains("25"));
    }

    #[test]
    fn deadline_label_handles_none_and_some() {
        assert_eq!(format_deadline_label(None), "No deadline");
        let d = NaiveDate::from_ymd_opt(2026, 6, 5).unwrap();
        let label = format_deadline_label(Some(&d));
        assert!(label.contains("Jun"));
        assert!(label.contains("5"));
    }
}
