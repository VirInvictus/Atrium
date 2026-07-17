// SPDX-License-Identifier: MIT
//! `AtriumWindow`: Forecast / Review / Logbook / Agenda / Calendar / Board page refreshers + calendar nav.
//! Extracted from window/mod.rs in v0.22.0 split (Pass 3).

use crate::i18n::gettext;

use super::*;

impl AtriumWindow {
    /// Phase 12 — rebuild the Forecast page from the read pool
    /// and mount it into the `forecast_host` AdwBin. Called from
    /// `refresh_active_list` when the active view becomes
    /// Forecast, and from `apply_task_changes` if the active view
    /// is currently Forecast (so a drag-to-reschedule, completion
    /// toggle, or worker-driven mutation refreshes the cards).
    pub(super) fn refresh_forecast_page(&self) {
        let Some(pool) = self.read_pool() else {
            self.imp().forecast_host.set_child(None::<&gtk::Widget>);
            return;
        };
        let today = Local::now().date_naive();
        let forecast_tasks = pool
            .with(|conn| {
                atrium_core::db::read::list_forecast(
                    conn,
                    today,
                    crate::ui::forecast::FORECAST_WINDOW_DAYS,
                )
            })
            .unwrap_or_default();
        let overdue = pool
            .with(|conn| atrium_core::db::read::list_overdue(conn, today))
            .unwrap_or_default();
        // v0.6.17 — click → open in Inspector. Same shape board /
        // agenda use; reuses the existing `win.edit-details-for(id)`
        // action so keyboard shortcut + row click + this callback
        // all funnel through one path.
        let weak = self.downgrade();
        let on_click = move |task_id: i64| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let _ = WidgetExt::activate_action(
                &window,
                "win.edit-details-for",
                Some(&task_id.to_variant()),
            );
        };
        let widget = crate::ui::forecast::build_page(
            today,
            &forecast_tasks,
            &overdue,
            self.worker(),
            on_click,
        );
        let hosted = self.wrap_with_agenda_toggle(widget.upcast());
        self.imp().forecast_host.set_child(Some(&hosted));
    }

    /// v0.39.0 — Builder-only Bands/Strip layout toggle for the merged
    /// Agenda view. "Bands" is the chronological Agenda layout
    /// (`ActiveList::Agenda`); "Strip" is the 30-day Forecast layout
    /// (`ActiveList::Forecast`). Returns `None` in Simple Mode — the
    /// Strip is a Builder-only layout, so Simple sees only the Bands
    /// page with no toggle.
    fn build_agenda_layout_toggle(&self) -> Option<gtk::Widget> {
        if !self.imp().current_mode_is_builder.get() {
            return None;
        }
        let on_strip = matches!(self.active_list(), ActiveList::Forecast);

        // Translators: the two Agenda layout toggle labels — "Bands"
        // is the chronological section layout, "Strip" the 30-day
        // Forecast projection.
        let bands = gtk::ToggleButton::builder()
            .label(gettext("Bands"))
            .active(!on_strip)
            .build();
        let strip = gtk::ToggleButton::builder()
            .label(gettext("Strip"))
            .active(on_strip)
            .build();
        strip.set_group(Some(&bands));
        bands.update_property(&[gtk::accessible::Property::Label(&gettext(
            "Agenda bands layout",
        ))]);
        strip.update_property(&[gtk::accessible::Property::Label(&gettext(
            "Forecast strip layout (30-day)",
        ))]);

        let weak_b = self.downgrade();
        bands.connect_toggled(move |btn| {
            // Only the newly-activated button acts; set_active_list is a
            // no-op when already on that view, so the programmatic
            // .active() above can't loop.
            if btn.is_active()
                && let Some(win) = weak_b.upgrade()
            {
                win.set_active_list(ActiveList::Agenda);
            }
        });
        let weak_s = self.downgrade();
        strip.connect_toggled(move |btn| {
            if btn.is_active()
                && let Some(win) = weak_s.upgrade()
            {
                win.set_active_list(ActiveList::Forecast);
            }
        });

        let bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .halign(gtk::Align::Center)
            .margin_top(8)
            .margin_bottom(2)
            .build();
        bar.add_css_class("linked");
        bar.append(&bands);
        bar.append(&strip);
        Some(bar.upcast())
    }

    /// Wrap an Agenda/Forecast page widget with the Builder-only
    /// layout toggle. In Simple Mode (no toggle) the page is returned
    /// unwrapped.
    fn wrap_with_agenda_toggle(&self, content: gtk::Widget) -> gtk::Widget {
        match self.build_agenda_layout_toggle() {
            Some(toggle) => {
                let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
                vbox.append(&toggle);
                content.set_vexpand(true);
                vbox.append(&content);
                vbox.upcast()
            }
            None => content,
        }
    }

    /// Phase 13 → v0.7.2 — rebuild the Review page. Renders two
    /// sections in one surface: the project review queue (Phase 13),
    /// and the canonical Weekly Walk (the open-tasks-this-week
    /// filter formerly seeded as the "Weekly Review" Perspective).
    /// Called when the active list becomes Review, and from
    /// `apply_library_changes` so Mark-Reviewed clicks drop the
    /// row immediately, and from `apply_task_changes` so
    /// completions in the weekly walk drop their row immediately.
    pub(super) fn refresh_review_page(&self) {
        let Some(pool) = self.read_pool() else {
            self.imp().review_host.set_child(None::<&gtk::Widget>);
            return;
        };
        let today = Local::now().date_naive();
        let queue = pool
            .with(|conn| atrium_core::db::read::list_review_queue(conn, today))
            .unwrap_or_default();

        // Weekly walk — open tasks matching REVIEW_WEEKLY_WALK_FILTER.
        // We load every task and filter in-memory; the weekly walk
        // isn't a hot path (it rebuilds only on Review-page open or
        // a relevant delta), and the filter expression has predicates
        // that the SQL fast-path can't all translate cleanly.
        let all_tasks = pool
            .with(atrium_core::db::read::list_all_tasks)
            .unwrap_or_default();
        let tag_names = pool
            .with(atrium_core::db::read::tag_names_per_task)
            .unwrap_or_default();
        let project_titles = self.imp().project_titles.borrow().clone();
        let area_titles = self.imp().area_titles.borrow().clone();
        let project_areas = self.project_areas_map();
        let query = crate::ui::filter::parse(atrium_core::db::REVIEW_WEEKLY_WALK_FILTER);
        let weekly_tasks = crate::ui::filter::apply(
            all_tasks,
            &query,
            today,
            &tag_names,
            &project_titles,
            &project_areas,
            &area_titles,
        );
        // v0.7.4 — exclude tasks marked reviewed within the last 7
        // days. Mark Reviewed is a manual user action; the page
        // hides the row for one cycle. After 7 days the row
        // resurfaces if it still matches the weekly-walk filter.
        let cutoff = today - chrono::Duration::days(7);
        let weekly_tasks: Vec<atrium_core::Task> = weekly_tasks
            .into_iter()
            .filter(|t| {
                t.last_reviewed_at
                    .is_none_or(|when| when.date_naive() < cutoff)
            })
            .collect();

        let tag_pills: crate::ui::task_list::TagPillMap = pool
            .with(atrium_core::db::read::tag_info_per_task)
            .unwrap_or_default();

        let weak = self.downgrade();
        let on_click = move |task_id: i64| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let _ = WidgetExt::activate_action(
                &window,
                "win.edit-details-for",
                Some(&task_id.to_variant()),
            );
        };

        // v0.7.4 — Mark Reviewed callback. Clicking the per-row
        // button dispatches `worker.mark_task_reviewed(id)`; the
        // worker emits a TaskChanges{updated} delta which triggers
        // refresh_review_page (apply_task_changes routes to the
        // Review-rebuild branch when the active list is Review).
        let weak = self.downgrade();
        let on_mark_reviewed = move |task_id: i64| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let Some(worker) = window.worker() else {
                return;
            };
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker.mark_task_reviewed(task_id).await {
                    error!(?e, task_id, "mark_task_reviewed failed");
                }
            });
        };

        let widget = crate::ui::review::build_page(
            today,
            &queue,
            &weekly_tasks,
            &project_titles,
            &area_titles,
            &tag_pills,
            self.worker(),
            on_click,
            on_mark_reviewed,
        );
        self.imp().review_host.set_child(Some(&widget));
    }

    /// v0.6.0 (Slice C2) — rebuild the Logbook page with day-band
    /// grouping (Today / Yesterday / Last 7 Days / Older) and mount
    /// it into the `logbook_host` AdwBin. Replaces the flat list
    /// rendering Logbook used to share with Inbox / Today / etc.
    /// Called when the active list becomes Logbook, and from
    /// `apply_task_changes` if the active list is Logbook (so a
    /// completion toggle on another list drops a freshly-finished
    /// task into the Today band immediately).
    pub(super) fn refresh_logbook_page(&self) {
        let Some(pool) = self.read_pool() else {
            self.imp().logbook_host.set_child(None::<&gtk::Widget>);
            return;
        };
        let today = Local::now().date_naive();
        let tasks = pool
            .with(atrium_core::db::read::list_logbook)
            .unwrap_or_default();
        let tag_pills: crate::ui::task_list::TagPillMap = pool
            .with(atrium_core::db::read::tag_info_per_task)
            .unwrap_or_default();
        let project_titles = self.imp().project_titles.borrow().clone();
        let area_titles = self.imp().area_titles.borrow().clone();
        let project_areas: HashMap<i64, Option<i64>> = self
            .imp()
            .project_meta
            .borrow()
            .iter()
            .map(|(id, p)| (*id, p.area_id))
            .collect();
        let widget = crate::ui::logbook::build_page(
            today,
            &tasks,
            &project_titles,
            &project_areas,
            &area_titles,
            &tag_pills,
        );
        self.imp().logbook_host.set_child(Some(&widget));
    }

    /// v0.6.4 (Slice D2) — rebuild the Agenda canonical page. Loads
    /// every open task, runs each through `agenda::classify` to
    /// bucket into Overdue / Today / Tomorrow / This Week / Next
    /// Week, and mounts the resulting widget into `agenda_host`.
    pub(super) fn refresh_agenda_page(&self) {
        let Some(pool) = self.read_pool() else {
            self.imp().agenda_host.set_child(None::<&gtk::Widget>);
            return;
        };
        let today = Local::now().date_naive();
        // The agenda is open-tasks-only; we'd otherwise pay the cost
        // of pulling completed rows from the Logbook just to filter
        // them out. `list_all_tasks` is the simplest one-shot but
        // the same compose-of-existing-helpers we use elsewhere.
        let tasks = pool
            .with(atrium_core::db::read::list_all_tasks)
            .unwrap_or_default();
        let project_titles = self.imp().project_titles.borrow().clone();
        let tag_pills: crate::ui::task_list::TagPillMap = pool
            .with(atrium_core::db::read::tag_info_per_task)
            .unwrap_or_default();
        let weak = self.downgrade();
        let on_click = move |task_id: i64| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let _ = WidgetExt::activate_action(
                &window,
                "win.edit-details-for",
                Some(&task_id.to_variant()),
            );
        };
        let widget =
            crate::ui::agenda::build_page(today, &tasks, &project_titles, &tag_pills, on_click);
        let hosted = self.wrap_with_agenda_toggle(widget);
        self.imp().agenda_host.set_child(Some(&hosted));
    }

    /// Phase 12.5 — open the Calendar Month View. No-op in Simple
    /// Mode (Calendar is Builder-only); the accelerator stays
    /// bound system-wide so users in Builder always get the
    /// shortcut, but it doesn't leak the Builder feature into
    /// Simple's surface.
    pub fn show_calendar(&self) {
        let mode = self.settings().string("mode");
        if mode != "builder" {
            return;
        }
        self.set_active_list(ActiveList::Calendar);
    }

    /// Phase 12.5 — return the cached calendar viewed-month, or
    /// today's first-of-month if the user hasn't navigated yet.
    /// Lazy init keeps the field default-clean (NaiveDate has no
    /// Default).
    pub(super) fn calendar_viewed_or_today(&self) -> chrono::NaiveDate {
        let cached = self.imp().calendar_viewed.get();
        cached.unwrap_or_else(|| {
            let today = Local::now().date_naive();
            crate::ui::calendar::first_of_month(today)
        })
    }

    /// Phase 12.5 — set the calendar's currently-viewed month and
    /// refresh the page if Calendar is the active view. Always
    /// stores `first_of_month(date)` to keep the field canonical.
    pub fn set_calendar_viewed(&self, date: chrono::NaiveDate) {
        let normalised = crate::ui::calendar::first_of_month(date);
        self.imp().calendar_viewed.set(Some(normalised));
        if matches!(self.active_list(), ActiveList::Calendar) {
            self.refresh_calendar_page();
            self.refresh_page_subtitle();
        }
    }

    /// Phase 12.5 — bump the calendar's viewed-month by ±1.
    pub fn calendar_step_month(&self, forward: bool) {
        let current = self.calendar_viewed_or_today();
        let next = if forward {
            crate::ui::calendar::next_month(current)
        } else {
            crate::ui::calendar::previous_month(current)
        };
        self.set_calendar_viewed(next);
    }

    /// Phase 12.5 — jump back to today's month.
    pub fn calendar_jump_to_today(&self) {
        let today = Local::now().date_naive();
        self.set_calendar_viewed(today);
    }

    /// Re-render only the page-title subtitle (used by
    /// `set_calendar_viewed` so the month/year header tracks nav
    /// without a full set_active_list pass).
    pub(super) fn refresh_page_subtitle(&self) {
        let active = self.active_list();
        let subtitle = self.subtitle_for(&active);
        let lbl = &self.imp().page_subtitle_label;
        lbl.set_label(&subtitle);
        lbl.set_visible(!subtitle.is_empty());
    }

    /// Phase 12.5 — rebuild the Calendar Month View from the read
    /// pool and mount it into `calendar_host`. Called from
    /// `refresh_active_list` when Calendar becomes active and from
    /// the nav helpers above when the user pages months.
    pub(super) fn refresh_calendar_page(&self) {
        let Some(pool) = self.read_pool() else {
            self.imp().calendar_host.set_child(None::<&gtk::Widget>);
            return;
        };
        let today = Local::now().date_naive();
        let viewed = self.calendar_viewed_or_today();
        // Calendar uses every open task with a scheduled date —
        // load all tasks and let `build_month_grid` filter. The
        // month-scoped query would be tighter, but the worst-case
        // count (10K open tasks * map-by-date) is still fast.
        let tasks = pool
            .with(atrium_core::db::read::list_all_tasks)
            .unwrap_or_default();
        let weak_prev = self.downgrade();
        let on_prev = move || {
            if let Some(win) = weak_prev.upgrade() {
                win.calendar_step_month(false);
            }
        };
        let weak_next = self.downgrade();
        let on_next = move || {
            if let Some(win) = weak_next.upgrade() {
                win.calendar_step_month(true);
            }
        };
        let weak_today = self.downgrade();
        let on_today = move || {
            if let Some(win) = weak_today.upgrade() {
                win.calendar_jump_to_today();
            }
        };
        let weak_pick = self.downgrade();
        let on_pick = move |year: i32, month: u32| {
            let Some(win) = weak_pick.upgrade() else {
                return;
            };
            if let Some(d) = chrono::NaiveDate::from_ymd_opt(year, month, 1) {
                win.set_calendar_viewed(d);
            }
        };
        let weak_click = self.downgrade();
        let on_row_click = move |task_id: i64| {
            let Some(win) = weak_click.upgrade() else {
                return;
            };
            let _ = WidgetExt::activate_action(
                &win,
                "win.edit-details-for",
                Some(&task_id.to_variant()),
            );
        };
        // Double-click on a calendar cell drills into the standard
        // list view scoped to `scheduled:<DATE>`. Reuses the
        // SearchResults active list so the user gets the full
        // editing affordances (drag, multi-select, complete) on
        // the day's tasks rather than being stuck in the calendar
        // peek-popover.
        let weak_drill = self.downgrade();
        let on_day_drill = move |target: chrono::NaiveDate| {
            let Some(win) = weak_drill.upgrade() else {
                return;
            };
            let expr = format!("scheduled:{}", target.format("%Y-%m-%d"));
            win.set_active_list(ActiveList::SearchResults(expr));
        };
        let compact = self.default_width() > 0
            && self.default_width() < crate::ui::calendar::COMPACT_WIDTH_THRESHOLD;
        let widget = crate::ui::calendar::build_page(
            viewed,
            today,
            &tasks,
            self.worker(),
            compact,
            crate::ui::calendar::CalendarCallbacks {
                on_prev,
                on_next,
                on_today,
                on_pick_month: on_pick,
                on_row_click,
                on_day_drill,
            },
        );
        self.imp().calendar_host.set_child(Some(&widget));
    }

    /// v0.6.0 (Slice D1 GUI) — rebuild the kanban board page for a
    /// saved Perspective whose `renderer = "board"`. The grouping
    /// engine lives in `atrium_core::render`; this method just
    /// orchestrates the load + group + mount. Returns `Ok(())`
    /// when the page renders, `Err` when the perspective's renderer
    /// config is malformed (caller falls back to list rendering).
    pub(super) fn refresh_board_page(
        &self,
        perspective: &atrium_core::Perspective,
    ) -> Result<(), atrium_core::RendererError> {
        let renderer = atrium_core::Renderer::from_columns(
            &perspective.renderer,
            perspective.renderer_config.as_deref(),
        )?;
        let cfg = match renderer {
            atrium_core::Renderer::Board(cfg) => cfg,
            atrium_core::Renderer::List => return Ok(()),
        };
        let Some(pool) = self.read_pool() else {
            self.imp().board_host.set_child(None::<&gtk::Widget>);
            return Ok(());
        };
        // Same load shape as the list-renderer perspective path —
        // run the saved filter expression, apply sorts / bm25 /
        // SQL fast-path, then group. v0.6.6 — use the SQL
        // translation evaluator (shipped at v0.5.3) to push the
        // filter to SQLite when expressible. At fixture scale
        // (~1000 tasks, ~870 open) the in-memory path was loading
        // every row + iterating in Rust on every drop. The SQL
        // path lets us skip both the round-trip cost and the
        // per-row evaluator work for the 80% of expressions that
        // translate cleanly.
        let parsed = crate::ui::filter::parse(&perspective.filter_expr);
        let today = Local::now().date_naive();
        let tag_map: HashMap<i64, Vec<String>> = pool
            .with(atrium_core::db::read::tag_names_per_task)
            .unwrap_or_default();
        let project_titles = self.imp().project_titles.borrow().clone();
        let project_areas = self.project_areas_map();
        let area_titles = self.imp().area_titles.borrow().clone();
        let mut filtered: Vec<atrium_core::Task> = if let Some(expr) = &parsed.expr
            && let Some(clause) = atrium_search::try_translate(expr, today)
        {
            // SQL fast-path: load only the matching rows. Saves the
            // load-everything + iterate-in-Rust cost on every drop.
            let params: Vec<atrium_core::SqlBindValue> =
                clause.params.iter().map(Into::into).collect();
            pool.with(|conn| atrium_core::db::read::list_tasks_matching(conn, &clause.sql, &params))
                .unwrap_or_default()
        } else {
            // Fallback: load all + in-memory filter (regex / fuzzy
            // / composite is:today / etc. that the translator
            // doesn't yet cover).
            let tasks = pool
                .with(atrium_core::db::read::list_all_tasks)
                .unwrap_or_default();
            crate::ui::filter::apply(
                tasks,
                &parsed,
                today,
                &tag_map,
                &project_titles,
                &project_areas,
                &area_titles,
            )
        };
        // Apply explicit `sort:` modifiers when present. Mirrors
        // what filter::apply would have done on the fallback path.
        if !parsed.sorts.is_empty() {
            // sort_tasks lives on filter::apply's path; the SQL
            // path skips it. Re-sort here so both paths agree.
            crate::ui::filter::sort_tasks_by_specs(&mut filtered, &parsed.sorts);
        }
        // bm25 ranking still applies inside a board's rows when the
        // saved expression has bare text and no explicit sort.
        if parsed.sorts.is_empty()
            && let Some(expr) = &parsed.expr
        {
            let terms = atrium_search::collect_text_terms(expr);
            if !terms.is_empty() {
                let scores = pool
                    .with(|conn| atrium_core::db::read::bm25_for_terms(conn, &terms))
                    .unwrap_or_default();
                if !scores.is_empty() {
                    crate::ui::filter::rank_by_bm25_recency(&mut filtered, &scores, today);
                }
            }
        }
        let mut columns = atrium_core::group_into_board(&filtered, &cfg, &tag_map);
        // v0.46.0 — apply the persisted intra-column order, then snapshot
        // each column's ordered ids so the drop handler can compute a new
        // order without re-reading.
        let positions = pool
            .with(|c| atrium_core::db::read::board_card_positions(c, perspective.id))
            .unwrap_or_default();
        for col in &mut columns {
            atrium_core::order_column_tasks(&mut col.tasks, &col.label, &positions);
        }
        let column_orders: std::collections::HashMap<String, Vec<i64>> = columns
            .iter()
            .map(|c| {
                (
                    c.label.to_ascii_lowercase(),
                    c.tasks.iter().map(|t| t.id).collect(),
                )
            })
            .collect();
        let perspective_id = perspective.id;

        // Tag pills + worker handle for the row's secondary metadata
        // line and interactive checkbox. The pill map carries the
        // colour each tag was configured with so the kanban renders
        // the same Pango-coloured pills the regular list does.
        let tag_pills: crate::ui::task_list::TagPillMap = pool
            .with(atrium_core::db::read::tag_info_per_task)
            .unwrap_or_default();

        // v0.43.0 — richer cards. Blocked-state set + per-task statistics
        // cookie ([done/total], subtasks folded with body checkboxes),
        // reusing the same resolver the list rows use so board and list
        // agree. Computed only over the filtered set the board renders.
        let blocked_ids = pool
            .with(atrium_core::db::read::blocked_task_ids)
            .unwrap_or_default();
        let cookie_for = self.build_cookie_resolver();
        let subtask_cookies: HashMap<i64, String> = filtered
            .iter()
            .map(|t| (t.id, cookie_for(t.id, &t.note)))
            .collect();

        let worker = self.worker();

        // Click → open the task in the Inspector. Reuses the
        // already-wired `win.edit-details-for(i64)` action that the
        // regular list and the keyboard shortcut both go through.
        let weak_click = self.downgrade();
        let on_click = move |task_id: i64| {
            let Some(window) = weak_click.upgrade() else {
                return;
            };
            // Disambiguate: WidgetExt::activate_action walks up the
            // hierarchy looking for a matching action group; we want
            // the window's own action group (the "win." namespace).
            let _ = WidgetExt::activate_action(
                &window,
                "win.edit-details-for",
                Some(&task_id.to_variant()),
            );
        };

        // v0.6.3 — drag-drop between columns. Each card on the board
        // is a drop target. The effect depends on the board axis:
        //
        // - **Tag axis:** recompute the task's tag set with
        //   `move_to_column` and dispatch ensure_tag + set_task_tags.
        // - **Status axis (v0.38.0):** translate the drop into a real
        //   state change via `status_move` — set `orig_keyword`, and
        //   flip completion through `toggle_complete` (full checkbox
        //   semantics, so a recurring task rolls forward) when the
        //   destination column's completion differs from the task's.
        //
        // The pool and worker are re-fetched per drop so the closure
        // stays a plain Fn (no captured borrows of cell-borrowed maps).
        let cfg_for_drop = cfg.clone();
        let orders_for_drop = column_orders.clone();
        let weak_drop = self.downgrade();
        let on_drop = move |dragged_id: i64,
                            dest: crate::ui::board::DropDestination,
                            before_id: Option<i64>| {
            let Some(window) = weak_drop.upgrade() else {
                return;
            };
            let Some(worker) = window.worker() else {
                return;
            };
            let Some(pool) = window.read_pool() else {
                return;
            };
            let dest_str: Option<String> = match &dest {
                crate::ui::board::DropDestination::Column(n) => Some(n.clone()),
                crate::ui::board::DropDestination::Other => None,
            };
            // Position-store key for the destination column (lowercased;
            // the trailing bucket keys on the "Other" label).
            let dest_key = match &dest {
                crate::ui::board::DropDestination::Column(n) => n.to_ascii_lowercase(),
                crate::ui::board::DropDestination::Other => {
                    atrium_core::OTHER_COLUMN_LABEL.to_ascii_lowercase()
                }
            };
            // Which column is the card in now (per the render snapshot)?
            let source_key = orders_for_drop
                .iter()
                .find(|(_, ids)| ids.contains(&dragged_id))
                .map(|(k, _)| k.clone());
            let same_column = source_key.as_deref() == Some(dest_key.as_str());

            // Compute the destination column's new ordered id list: drop
            // the dragged id if present, then insert it before `before_id`
            // (or append when the drop landed on empty column space).
            let mut new_order: Vec<i64> =
                orders_for_drop.get(&dest_key).cloned().unwrap_or_default();
            new_order.retain(|&id| id != dragged_id);
            match before_id.and_then(|bid| new_order.iter().position(|&id| id == bid)) {
                Some(idx) => new_order.insert(idx, dragged_id),
                None => new_order.push(dragged_id),
            }

            let cfg_axis = cfg_for_drop.axis;
            let cfg_for_async = cfg_for_drop.clone();
            glib::MainContext::default().spawn_local(async move {
                // 1. Cross-column drops change the card's membership (tag
                //    set or status), exactly as a column-level drop always
                //    has. Same-column drops only reorder.
                if !same_column {
                    match cfg_axis {
                        atrium_core::BoardAxis::Tag => {
                            let map = pool
                                .with(atrium_core::db::read::tag_names_per_task)
                                .unwrap_or_default();
                            let current = map.get(&dragged_id).cloned().unwrap_or_default();
                            let new_names = atrium_core::move_to_column(
                                &current,
                                &cfg_for_async,
                                dest_str.as_deref(),
                            );
                            if !tag_lists_equal_case_insensitive(&current, &new_names) {
                                let mut ids: Vec<i64> = Vec::with_capacity(new_names.len());
                                for name in new_names {
                                    match worker.ensure_tag(name).await {
                                        Ok(t) => ids.push(t.id),
                                        Err(e) => warn!(?e, "kanban move ensure_tag failed"),
                                    }
                                }
                                if let Err(e) = worker.set_task_tags(dragged_id, ids).await {
                                    error!(?e, dragged_id, "kanban move set_task_tags failed");
                                }
                            }
                        }
                        atrium_core::BoardAxis::Status => {
                            if let Some(task) = pool
                                .with(|conn| atrium_core::db::read::task_by_id(conn, dragged_id))
                                .ok()
                                .flatten()
                            {
                                let current_completed = task.completed_at.is_some();
                                let change = atrium_core::status_move(
                                    &cfg_for_async,
                                    dest_str.as_deref(),
                                    current_completed,
                                );
                                let update = atrium_core::TaskUpdate::new(dragged_id)
                                    .orig_keyword(change.orig_keyword);
                                if let Err(e) = worker.update_task(update).await {
                                    error!(?e, dragged_id, "kanban status move update_task failed");
                                } else if change.completed != current_completed
                                    && let Err(e) = worker.toggle_complete(dragged_id).await
                                {
                                    error!(
                                        ?e,
                                        dragged_id, "kanban status move toggle_complete failed"
                                    );
                                }
                            }
                        }
                    }
                }
                // 2. Persist the destination column's new intra-column
                //    order (side-table write, no TaskChanges delta).
                if let Err(e) = worker
                    .reorder_board_column(perspective_id, dest_key, new_order)
                    .await
                {
                    error!(?e, perspective_id, "kanban reorder_board_column failed");
                }
                // 3. Re-render the board in the new order.
                window.refresh_active_list();
            });
        };

        // Configure button → the existing win.configure-renderer
        // action (a no-op off a Perspective; a board always is one).
        let weak_cfg = self.downgrade();
        let on_configure = move || {
            if let Some(window) = weak_cfg.upgrade() {
                let _ = WidgetExt::activate_action(&window, "win.configure-renderer", None);
            }
        };

        // v0.45.0 — per-column "Add card" entry creates a task stamped
        // with the column's membership (tag or status), reusing the
        // inline parser. cfg is cloned so the closure stays a plain Fn.
        let cfg_for_add = cfg.clone();
        let weak_add = self.downgrade();
        let on_add = move |dest: crate::ui::board::DropDestination, text: String| {
            if let Some(window) = weak_add.upgrade() {
                window.create_card_in_column(&cfg_for_add, dest, text);
            }
        };

        let widget = crate::ui::board::build_page(
            &perspective.name,
            &columns,
            &tag_pills,
            &project_titles,
            &subtask_cookies,
            &blocked_ids,
            &cfg.limits,
            worker,
            on_click,
            on_drop,
            on_configure,
            on_add,
        );
        self.imp().board_host.set_child(Some(&widget));
        Ok(())
    }

    /// Phase 10 — refresh the side pane based on the current
    /// selection. Single-task selection → populate; otherwise →
    /// clear back to the empty-state placeholder.
    ///
    /// v0.1.4 — gated on `mode = builder`. In Simple Mode the pane
    /// host is hidden and `pane.clear()` is held permanently; we
    /// don't want a selection change in Simple Mode to repopulate
    /// the editor with a stale task that the user can't see anyway
    /// (and that would resurface immediately on a flip back to
    /// Builder, ignoring whatever they're actually selecting now).
    pub(super) fn refresh_inspector_pane(&self) {
        let pane_opt = self.imp().inspector_pane.borrow().clone();
        let Some(pane) = pane_opt else {
            debug!("refresh_inspector_pane: no pane installed yet");
            return;
        };
        // v0.1.6 — read from the synchronous Cell instead of
        // round-tripping through GSettings. apply_mode is the only
        // writer; the value is set before any of apply_mode's
        // sibling work runs, so any callback that lands here in
        // the same event-loop iteration sees the just-flipped
        // mode (which the GSettings string compare in v0.1.5 was
        // sometimes missing).
        let builder = self.imp().current_mode_is_builder.get();
        if !builder {
            debug!("refresh_inspector_pane: simple mode → clear");
            pane.clear();
            return;
        }
        let selected = self.selected_task_ids();
        if selected.len() != 1 {
            debug!(
                n = selected.len(),
                "refresh_inspector_pane: not 1-selected → clear"
            );
            pane.clear();
            return;
        }
        let id = selected[0];
        if pane.current_task_id() == Some(id) {
            debug!(
                id,
                "refresh_inspector_pane: already showing this task → noop"
            );
            return;
        }
        let Some(pool) = self.read_pool() else {
            debug!("refresh_inspector_pane: no read pool yet");
            return;
        };
        let task = match pool.with(|c| atrium_core::db::read::task_by_id(c, id)) {
            Ok(Some(t)) => t,
            _ => {
                debug!(id, "refresh_inspector_pane: task not found → clear");
                pane.clear();
                return;
            }
        };
        let projects = pool
            .with(atrium_core::db::read::list_projects)
            .unwrap_or_default();
        let tag_count = pool
            .with(|c| atrium_core::db::read::tag_ids_for_task(c, id))
            .unwrap_or_default()
            .len();
        let clock_entries = pool
            .with(|c| atrium_core::db::read::list_clock_entries(c, id))
            .unwrap_or_default();
        debug!(id, "refresh_inspector_pane: set_task");
        pane.set_task(task, projects, tag_count, clock_entries);
    }
}
