// SPDX-License-Identifier: MIT
//! `AtriumWindow`: tag editor, search bar, toasts, undo, new-task entry.
//! Extracted from window/mod.rs in v0.22.0 split (Pass 3).

use crate::i18n::{gettext, gettext_f};

use super::*;

impl AtriumWindow {
    /// Open the per-task tag editor for `task_id` (Phase 7g).
    /// Loads the current tag set + the full tag library from the
    /// read pool, then hands off to `ui::tag_editor::open` which
    /// owns the dialog lifecycle and dispatches the apply call.
    pub fn open_tag_editor_for(&self, task_id: i64) {
        let Some(pool) = self.read_pool() else {
            return;
        };
        let Some(worker) = self.worker() else {
            return;
        };
        let task = match pool.with(|conn| atrium_core::db::read::task_by_id(conn, task_id)) {
            Ok(Some(t)) => t,
            Ok(None) => {
                error!(task_id, "tag editor: task not found");
                return;
            }
            Err(e) => {
                error!(?e, task_id, "tag editor: task_by_id failed");
                return;
            }
        };
        let current_tag_ids = pool
            .with(|conn| atrium_core::db::read::tag_ids_for_task(conn, task_id))
            .unwrap_or_default();
        let all_tags = pool
            .with(atrium_core::db::read::list_tags)
            .unwrap_or_default();
        crate::ui::tag_editor::open(self, worker, task_id, task.title, current_tag_ids, all_tags);
    }

    /// `Ctrl+T` shortcut + right-click menu entry point — operates
    /// on the focused / first-selected task. No-op if nothing is
    /// selected.
    pub fn edit_tags_focused(&self) {
        if let Some(id) = self.focused_task_id() {
            self.open_tag_editor_for(id);
        }
    }

    pub(super) fn wire_search_bar(&self) {
        let bar = self.imp().search_bar.clone();
        let entry = self.imp().search_entry.clone();
        let button = self.imp().search_button.clone();
        let help_button = self.imp().search_help_button.clone();

        // v0.6.9 — register the SearchEntry as the bar's input. GTK
        // emits "The search bar does not have an entry connected to
        // it. Call gtk_search_bar_connect_entry() to connect one."
        // on every captured key event when this isn't done. The
        // `key-capture-widget=task_list_view` property on the bar
        // forwards keystrokes; without `connect_entry` they have
        // nowhere to land. Our entry sits inside a wrapper Box (so
        // the `?` help button can sit alongside), so the bar can't
        // auto-discover it as a direct child.
        bar.connect_entry(&entry);

        // Hook the toggle button to the search bar's search-mode.
        button
            .bind_property("active", &bar, "search-mode-enabled")
            .sync_create()
            .bidirectional()
            .build();

        // v0.4.1 — operator-reference popover. Attaches to the `?`
        // GtkMenuButton in the search bar; click opens a structured
        // quick-reference for the search expression language. The
        // popover content is built once at wire time; subsequent
        // opens reuse the same widget.
        help_button.set_popover(Some(&build_search_help_popover()));

        // search-changed fires after `search-delay` ms (set in .ui).
        // We use it as our debounced input.
        entry.connect_search_changed(clone!(
            #[weak(rename_to = win)]
            self,
            move |entry| {
                let q = entry.text().to_string();
                if q.trim().is_empty() {
                    // If search bar is open and user cleared the
                    // text, fall back to Today rather than rendering
                    // empty results. Also clear any standing
                    // filter-warning fingerprint so next typo
                    // re-toasts, and clear any warning styling on
                    // the entry.
                    win.imp().last_filter_warning.replace(None);
                    entry.remove_css_class("warning");
                    if matches!(win.active_list(), ActiveList::SearchResults(_)) {
                        win.set_active_list(ActiveList::Today);
                        win.select_sidebar_row_for(ActiveList::Today);
                    }
                    return;
                }
                // v0.2.2 — flag obvious typos before the SELECT runs.
                // The parsed FilterQuery is computed cheaply; the
                // warning toast deduplicates against the last
                // fingerprint so successive refreshes don't spam.
                //
                // v0.4.0 — also tint the search entry with the
                // libadwaita `.warning` accent when the expression
                // has unknown tokens. Removed when the user fixes
                // the typo.
                let parsed = crate::ui::filter::parse(&q);
                win.surface_filter_warnings(&parsed);
                if parsed.warnings.is_empty() {
                    entry.remove_css_class("warning");
                } else {
                    entry.add_css_class("warning");
                }
                // v0.4.1 — push the committed query onto the history
                // ring buffer (de-duped against the most recent entry,
                // capped at SEARCH_HISTORY_MAX). Reset the navigation
                // cursor — typing always represents "fresh search,"
                // not "I'm browsing through history."
                {
                    let mut history = win.imp().search_history.borrow_mut();
                    push_history_entry(&mut history, q.clone(), SEARCH_HISTORY_MAX);
                }
                win.imp().search_history_cursor.replace(None);
                win.set_active_list(ActiveList::SearchResults(q));
            }
        ));

        // v0.4.1 — search-history navigation. ↑ recalls the previous
        // query, ↓ moves toward newer / current. The handler reads
        // and mutates `search_history_cursor`; cycle_history_cursor
        // is a pure-Rust helper so the logic is unit-testable.
        let key_ctrl = gtk::EventControllerKey::new();
        key_ctrl.connect_key_pressed(clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                let direction = match key {
                    gtk::gdk::Key::Up => HistoryDirection::Older,
                    gtk::gdk::Key::Down => HistoryDirection::Newer,
                    _ => return glib::Propagation::Proceed,
                };
                let entry = win.imp().search_entry.clone();
                let history = win.imp().search_history.borrow().clone();
                let cursor = *win.imp().search_history_cursor.borrow();
                let next = cycle_history_cursor(cursor, history.len(), direction);
                win.imp().search_history_cursor.replace(next);
                if let Some(idx) = next
                    && let Some(text) = history.get(idx)
                {
                    // set_text re-fires the search-changed handler,
                    // which pushes onto history. The dedup-against-
                    // last-entry guard in push_history_entry keeps
                    // that from snowballing.
                    entry.set_text(text);
                    entry.set_position(-1);
                }
                glib::Propagation::Stop
            }
        ));
        entry.add_controller(key_ctrl);

        // Esc inside the entry closes the bar.
        entry.connect_stop_search(clone!(
            #[weak]
            bar,
            #[weak]
            button,
            move |_| {
                bar.set_search_mode(false);
                button.set_active(false);
            }
        ));
    }

    /// Public action target — `Ctrl+F` opens the search bar and
    /// focuses the entry.
    pub fn focus_search(&self) {
        self.imp().search_bar.set_search_mode(true);
        self.imp().search_button.set_active(true);
        self.imp().search_entry.grab_focus();
    }

    /// Generic toast helper. Used for non-undo notifications like the
    /// filter-parse warning surface. Times out at 4 seconds — long
    /// enough to read, short enough not to linger.
    pub fn show_toast(&self, message: &str) {
        self.imp().toast_button.set_visible(false);
        self.present_toast(message, 4);
    }

    /// Reveal the owned toast pill with `message` and arm an auto-hide
    /// after `secs`. Newest-wins: a new toast cancels the pending timer
    /// so a burst keeps the latest message up for its full window.
    /// Phase 22 C3.
    fn present_toast(&self, message: &str, secs: u64) {
        let imp = self.imp();
        imp.toast_label.set_label(message);
        imp.toast_revealer.set_reveal_child(true);
        if let Some(id) = imp.toast_timeout.take() {
            id.remove();
        }
        let id = glib::timeout_add_local_once(
            std::time::Duration::from_secs(secs),
            clone!(
                #[weak(rename_to = win)]
                self,
                move || {
                    win.imp().toast_timeout.replace(None);
                    win.imp().toast_revealer.set_reveal_child(false);
                }
            ),
        );
        imp.toast_timeout.replace(Some(id));
    }

    /// Cancel any pending auto-hide and hide the toast immediately.
    fn hide_toast(&self) {
        let imp = self.imp();
        if let Some(id) = imp.toast_timeout.take() {
            id.remove();
        }
        imp.toast_revealer.set_reveal_child(false);
    }

    /// Wire the toast's Undo button once, at window setup. A click
    /// consumes the shared `last_undo` cell — the same slot `Ctrl+Z`
    /// reads — and hides the toast.
    pub(super) fn wire_toast(&self) {
        self.imp().toast_button.connect_clicked(clone!(
            #[weak(rename_to = win)]
            self,
            move |_| {
                win.invoke_last_undo();
                win.hide_toast();
            }
        ));
    }

    /// v0.2.2 — surface unknown `key:value` tokens in a search /
    /// perspective expression as a toast so users notice typos
    /// (`tga:foo`) instead of having the filter silently no-op.
    /// Deduplicated against `last_filter_warning` so refreshes of
    /// the same query (e.g. TaskChanges arrivals on a SearchResults
    /// view) don't re-toast.
    pub fn surface_filter_warnings(&self, parsed: &crate::ui::filter::FilterQuery) {
        if parsed.warnings.is_empty() {
            // Clear the cell so the same warning re-toasts later if
            // the user edits and re-types the same typo.
            self.imp().last_filter_warning.replace(None);
            return;
        }
        // De-duplicate by joined-warning fingerprint. Same fingerprint
        // = same bad input, don't re-toast.
        let fingerprint = parsed.warnings.join(" ");
        if self.imp().last_filter_warning.borrow().as_ref() == Some(&fingerprint) {
            return;
        }
        self.imp().last_filter_warning.replace(Some(fingerprint));
        let preview = parsed.warnings.iter().take(3).cloned().collect::<Vec<_>>();
        let suffix = if parsed.warnings.len() > preview.len() {
            // Translators: appended to the "Unknown filter" toast when
            // more unknown tokens exist than are shown; {n} is the
            // number of additional tokens. Note the leading space.
            gettext_f(
                " (+{n} more)",
                &[("n", &(parsed.warnings.len() - preview.len()).to_string())],
            )
        } else {
            String::new()
        };
        // Translators: {warnings} is a comma-separated list of the
        // unrecognised filter tokens; {suffix} is the "(+N more)"
        // overflow marker or empty.
        let message = gettext_f(
            "Unknown filter: {warnings}{suffix}",
            &[("warnings", &preview.join(", ")), ("suffix", &suffix)],
        );
        self.show_toast(&message);
    }

    /// Show a toast with an Undo button. The undo closure runs at most
    /// once — whichever of the toast button or the `Ctrl+Z` accel
    /// (Phase 7f) fires first consumes the shared `last_undo` cell.
    /// 6 s timeout. Phase 7b's daily-driver safety net.
    pub fn show_undo_toast<F: FnOnce() + 'static>(&self, message: &str, undo: F) {
        let cell: UndoCell = Rc::new(RefCell::new(Some(Box::new(undo))));
        // Share the cell with the window so `win.undo` (Ctrl+Z) and the
        // toast's Undo button (wired once in `wire_toast`) take from the
        // same slot — whoever fires first wins.
        self.imp().last_undo.replace(Some(cell));
        let button = self.imp().toast_button.get();
        button.set_label(&gettext("Undo"));
        button.set_visible(true);
        self.present_toast(message, 6);
    }

    /// Walk every sidebar row and unparent any stashed context-menu
    /// popover. Idempotent — rows without a stashed popover (the
    /// canonical rows, section headers) are skipped. Phase 8h fix
    /// for the "Finalizing GtkListBoxRow … but it still has children
    /// left" GTK warning. Called from `rebuild_dynamic_sidebar`
    /// before the remove-rows loop, and from `close_request` so the
    /// app close path is also clean.
    pub(super) fn unparent_sidebar_context_menus(&self) {
        let list_box = self.imp().sidebar_list.clone();
        let mut idx = 0;
        while let Some(row) = list_box.row_at_index(idx) {
            unsafe {
                if let Some(popover) = row.steal_data::<gtk::PopoverMenu>("atrium-context-popover")
                {
                    popover.unparent();
                }
            }
            idx += 1;
        }
    }

    /// Add or remove the `atrium-high-legibility` CSS class on the
    /// window. The matching selector in `data/style.css` swaps the
    /// UI font family to Atkinson Hyperlegible. Phase 8c.
    pub(super) fn apply_high_legibility(&self, on: bool) {
        if on {
            self.add_css_class("atrium-high-legibility");
        } else {
            self.remove_css_class("atrium-high-legibility");
        }
    }

    /// If a task row holds focus (or is the ancestor / focus-target
    /// inside the list view), flip its title stack into edit mode
    /// and return `true`. Used by F2 (Phase 7f) so the same chord
    /// that renames a sidebar item also opens the title editor on
    /// the focused task row. Replaces the v0.0.36 EditableLabel-based
    /// path; the stack's "edit" page is a plain GtkEntry that we
    /// populate from the bound display label and focus + select-all.
    pub fn start_edit_focused_row(&self) -> bool {
        let Some(focused) = self.focus() else {
            return false;
        };
        if let Some(row) = find_task_row(&focused) {
            return start_edit_on_row(&row);
        }
        false
    }

    /// Invoke the most recent undo callback, if any is still alive.
    /// Bound to `Ctrl+Z` via `win.undo`. Idempotent — once consumed,
    /// the cell stays empty until the next `show_undo_toast`.
    pub fn invoke_last_undo(&self) {
        let cell_opt = self.imp().last_undo.borrow().clone();
        if let Some(cell) = cell_opt
            && let Some(f) = cell.borrow_mut().take()
        {
            f();
        }
    }

    pub(super) fn wire_new_task_entry(&self) {
        let entry = self.imp().new_task_entry.clone();
        entry.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |entry| {
                let title = entry.text().to_string();
                let trimmed = title.trim();
                if trimmed.is_empty() {
                    return;
                }
                win.create_task_with_title(trimmed.to_string());
                entry.set_text("");
            }
        ));
        // v0.13 Slice 3 — the tab-completion popover gets attached
        // from `attach_data_layer` instead of here so the read
        // pool is guaranteed to exist when tag candidates need
        // fetching.
    }

    /// Focus the bottom-of-list entry. The Ctrl+N action targets this
    /// instead of immediately spawning a "New task" title — the
    /// Things-3 idiom is "type the title first, hit Enter to commit".
    pub fn focus_new_task_entry(&self) {
        self.imp().new_task_entry.grab_focus();
    }
}
