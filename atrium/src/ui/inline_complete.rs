// SPDX-License-Identifier: MIT
//! Tab-completion popover for inline-syntax-aware `gtk::Entry`s.
//!
//! Wires `atrium_inline::completions` (pure context detection +
//! candidate filtering) into a small `gtk::Popover` that floats
//! below the entry. Active when the user types a `#` / `@` / `!`
//! and shows candidates that match what they've typed so far.
//!
//! Supported surfaces:
//!
//! - `AtriumWindow::imp().new_task_entry` — bottom-of-list capture
//!   (v0.13.0).
//! - The Quick Entry modal's main entry (v0.13.0).
//! - The inline-rename `Entry` on every task-list row (v0.13.2).
//!
//! On the task-list rows, the popover attaches in the factory's
//! `setup()` callback — once per row's lifetime. Setup runs ahead
//! of `bind()` and survives across recycles, so the popover's
//! state lives with the entry rather than leaking on each
//! re-bind. Until the user enters edit mode, the entry is
//! invisible (the title_stack shows the display label) and the
//! popover's listeners stay quiet — no overhead for the common
//! case where no one is renaming a row.
//!
//! Key handling:
//!
//! - **Tab** — accept the highlighted candidate (or first if none
//!   highlighted).
//! - **↓ / ↑** — move the highlight; opens the popover if a context
//!   exists and the popover isn't open yet.
//! - **Enter** — accept the highlighted candidate, then let the
//!   entry's normal `activate` signal fire (commit the task).
//!   When the popover *isn't* open, Enter passes through unchanged.
//! - **Escape** — dismiss the popover. When the popover isn't open,
//!   Escape passes through (e.g. the Quick Entry modal's
//!   close-on-Escape).

use std::cell::RefCell;
use std::rc::Rc;

use atrium_core::db::read_pool::ReadPool;
use atrium_inline::completions::{self, CompletionContext, PRIORITY_LEVELS, SCHEDULE_KEYWORDS};
use gtk::gdk;
use gtk::glib::Propagation;
use gtk::prelude::*;

/// Per-entry state shared between the text-changed listener and
/// the key-press handler. The list of currently-shown candidates
/// is live; the popover is created once and re-used.
struct State {
    popover: gtk::Popover,
    list: gtk::ListBox,
    candidates: RefCell<Vec<String>>,
}

impl State {
    fn new(entry: &gtk::Entry) -> Rc<Self> {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Browse)
            .build();
        list.add_css_class("boxed-list");

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_width(200)
            .max_content_height(220)
            .propagate_natural_height(true)
            .child(&list)
            .build();

        let popover = gtk::Popover::builder()
            .child(&scroll)
            .autohide(false)
            .has_arrow(false)
            .position(gtk::PositionType::Bottom)
            .build();
        popover.add_css_class("atrium-inline-complete");
        popover.set_parent(entry);

        Rc::new(Self {
            popover,
            list,
            candidates: RefCell::new(Vec::new()),
        })
    }

    fn show(&self, candidates: Vec<String>) {
        // Rebuild the rows. Avoiding `remove_all` because GTK4's
        // ListBox doesn't have it; we walk children explicitly.
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        for c in &candidates {
            let label = gtk::Label::builder()
                .label(c)
                .xalign(0.0)
                .margin_start(8)
                .margin_end(8)
                .margin_top(4)
                .margin_bottom(4)
                .build();
            let row = gtk::ListBoxRow::builder().child(&label).build();
            self.list.append(&row);
        }
        if let Some(first) = self.list.first_child().and_downcast::<gtk::ListBoxRow>() {
            self.list.select_row(Some(&first));
        }
        self.candidates.replace(candidates);
        self.popover.popup();
    }

    fn hide(&self) {
        self.popover.popdown();
        self.candidates.replace(Vec::new());
    }

    fn is_open(&self) -> bool {
        self.popover.is_visible()
    }

    fn move_selection(&self, delta: i32) {
        let len = self.candidates.borrow().len() as i32;
        if len == 0 {
            return;
        }
        let current = self.list.selected_row().map_or(0, |r| r.index());
        let next = ((current + delta).rem_euclid(len)).max(0);
        if let Some(row) = self.list.row_at_index(next) {
            self.list.select_row(Some(&row));
        }
    }

    /// The candidate string the user would accept right now.
    /// Falls back to the first when no row is selected (defensive
    /// — `Browse` selection mode keeps a row selected as long as
    /// the list isn't empty, but the borrow is cheap).
    fn selected_candidate(&self) -> Option<String> {
        let idx = self.list.selected_row()?.index();
        self.candidates.borrow().get(idx as usize).cloned()
    }
}

/// Attach the completion popover to `entry`. `tag_pool` is the
/// read pool the popover consults to enumerate tag candidates.
/// Pass `None` to disable tag completion (e.g. on a surface where
/// the read pool isn't available); `@` and `!` completion still
/// work since their candidate sets are static.
pub fn attach(entry: &gtk::Entry, tag_pool: Option<ReadPool>) {
    let state = State::new(entry);

    // Re-evaluate the context every time text or cursor changes.
    let refresh = {
        let entry = entry.clone();
        let state = state.clone();
        let tag_pool = tag_pool.clone();
        move || refresh_state(&entry, &state, tag_pool.as_ref())
    };

    // `notify::text` fires on every character typed/deleted; the
    // cursor-position notify covers arrow-key moves through an
    // existing token (e.g. backing up into `@mo`).
    {
        let refresh = refresh.clone();
        entry.connect_changed(move |_| refresh());
    }
    {
        let refresh = refresh.clone();
        entry.connect_notify_local(Some("cursor-position"), move |_, _| refresh());
    }

    // Key controller — capture-phase so Tab/arrow/Enter/Esc reach
    // us before the entry's default handlers (which would, for
    // example, advance focus on Tab).
    let key_ctrl = gtk::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let entry = entry.clone();
        let state = state.clone();
        key_ctrl.connect_key_pressed(move |_, key, _, _| {
            if !state.is_open() {
                // Down opens the popover from a still-empty state
                // when the cursor is on a recognised token —
                // mirrors how a desktop-search box reveals its
                // suggestions on first arrow-key.
                if key == gdk::Key::Down {
                    refresh_state(&entry, &state, tag_pool.as_ref());
                    if state.is_open() {
                        return Propagation::Stop;
                    }
                }
                return Propagation::Proceed;
            }
            match key {
                gdk::Key::Tab => {
                    if let Some(chosen) = state.selected_candidate() {
                        accept_candidate(&entry, &chosen);
                    }
                    state.hide();
                    Propagation::Stop
                }
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    // Accept the candidate, then let Enter
                    // continue so the entry's `activate` signal
                    // commits the task. Without the popdown the
                    // popover would still be open after commit.
                    if let Some(chosen) = state.selected_candidate() {
                        accept_candidate(&entry, &chosen);
                    }
                    state.hide();
                    // Returning Stop prevents the activate signal,
                    // so Enter inside an open popover only accepts
                    // the candidate — the user presses Enter again
                    // to commit. Matches Things 3's autocomplete
                    // ergonomics.
                    Propagation::Stop
                }
                gdk::Key::Down => {
                    state.move_selection(1);
                    Propagation::Stop
                }
                gdk::Key::Up => {
                    state.move_selection(-1);
                    Propagation::Stop
                }
                gdk::Key::Escape => {
                    state.hide();
                    Propagation::Stop
                }
                _ => Propagation::Proceed,
            }
        });
    }
    entry.add_controller(key_ctrl);

    // Hide on focus-out so a click elsewhere doesn't leave a
    // floating popover behind.
    let focus_ctrl = gtk::EventControllerFocus::new();
    {
        let state = state.clone();
        focus_ctrl.connect_leave(move |_| state.hide());
    }
    entry.add_controller(focus_ctrl);
}

fn refresh_state(entry: &gtk::Entry, state: &Rc<State>, tag_pool: Option<&ReadPool>) {
    let text = entry.text().to_string();
    let cursor = entry.position() as usize;
    let cursor = utf8_byte_offset(&text, cursor);
    let ctx = completions::context_at(&text, cursor);
    let candidates = candidates_for(ctx, tag_pool);
    if candidates.is_empty() {
        state.hide();
    } else {
        state.show(candidates);
    }
}

/// Candidate list for a given context.
///
/// Tag candidates pull from the read pool; if the pool returns an
/// error we silently fall back to an empty list rather than ruin
/// the user's typing — completion is best-effort.
fn candidates_for(ctx: CompletionContext, tag_pool: Option<&ReadPool>) -> Vec<String> {
    match ctx {
        CompletionContext::Tag(prefix) => {
            let Some(pool) = tag_pool else {
                return Vec::new();
            };
            let names = pool
                .with(atrium_core::db::read::list_tags)
                .ok()
                .unwrap_or_default()
                .into_iter()
                .map(|t| t.name)
                .collect::<Vec<_>>();
            completions::matches(&prefix, names)
        }
        CompletionContext::Schedule(prefix) => {
            completions::matches(&prefix, SCHEDULE_KEYWORDS.iter().copied())
        }
        CompletionContext::Priority(prefix) => {
            completions::matches(&prefix, PRIORITY_LEVELS.iter().copied())
        }
        CompletionContext::None => Vec::new(),
    }
}

fn accept_candidate(entry: &gtk::Entry, chosen: &str) {
    let text = entry.text().to_string();
    let cursor_chars = entry.position() as usize;
    let cursor_bytes = utf8_byte_offset(&text, cursor_chars);
    let (new_text, new_cursor_bytes) = completions::replace_token(&text, cursor_bytes, chosen);
    let new_cursor_chars = char_count_at_byte(&new_text, new_cursor_bytes) as i32;
    entry.set_text(&new_text);
    entry.set_position(new_cursor_chars);
}

/// Convert a GTK char-count cursor position to a byte offset
/// suitable for `completions::context_at` / `replace_token`.
/// GTK's `Entry::position()` reports characters; the parser
/// works on byte slices.
fn utf8_byte_offset(text: &str, char_pos: usize) -> usize {
    text.char_indices()
        .nth(char_pos)
        .map_or(text.len(), |(i, _)| i)
}

/// Inverse — convert a byte offset back to a character count so
/// `entry.set_position` lands the cursor where the parser said it
/// should be.
fn char_count_at_byte(text: &str, byte_pos: usize) -> usize {
    let clamped = byte_pos.min(text.len());
    text[..clamped].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_byte_offset_handles_ascii() {
        assert_eq!(utf8_byte_offset("hello", 3), 3);
        assert_eq!(utf8_byte_offset("hello", 5), 5);
        // Past-end clamps.
        assert_eq!(utf8_byte_offset("hello", 99), 5);
    }

    #[test]
    fn utf8_byte_offset_handles_multibyte() {
        // "café" — 4 chars, 5 bytes (é is 2 bytes).
        assert_eq!(utf8_byte_offset("café", 0), 0);
        assert_eq!(utf8_byte_offset("café", 3), 3);
        assert_eq!(utf8_byte_offset("café", 4), 5);
    }

    #[test]
    fn char_count_at_byte_inverse_of_offset() {
        let text = "café";
        assert_eq!(char_count_at_byte(text, 5), 4);
        assert_eq!(char_count_at_byte(text, 3), 3);
        assert_eq!(char_count_at_byte(text, 99), 4);
    }
}
