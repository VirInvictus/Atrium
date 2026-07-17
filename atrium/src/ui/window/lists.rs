// SPDX-License-Identifier: MIT
//! `AtriumWindow`: active-list rendering, task/library-change appliers, empty state.
//! Extracted from window/mod.rs in v0.22.0 split (Pass 3).

use crate::i18n::{gettext, gettext_f};

use super::*;

impl AtriumWindow {
    pub fn refresh_active_list(&self) {
        let Some(store) = self.imp().store.borrow().clone() else {
            return;
        };
        let Some(pool) = self.read_pool() else {
            // Read pool not attached yet — show empty state, will refresh on attach.
            store.remove_all();
            self.update_empty_state(&store);
            return;
        };

        // v0.31.0 — a pristine library shows the onboarding page
        // instead of an empty list. The flag is kept current by the
        // change handlers (and at startup).
        if self.imp().db_empty.get() {
            self.imp()
                .content_stack
                .set_visible_child_name("onboarding");
            return;
        }

        let active = self.active_list();
        let today = Local::now().date_naive();

        // Phase 12 — Forecast is a Builder stub no longer; it
        // renders a real calendar-axis page.
        if matches!(active, ActiveList::Forecast) {
            store.remove_all();
            self.refresh_forecast_page();
            self.imp().content_stack.set_visible_child_name("forecast");
            return;
        }

        // Phase 13 — Review is a Builder stub no longer; it
        // renders the project-review queue.
        if matches!(active, ActiveList::Review) {
            store.remove_all();
            self.refresh_review_page();
            self.imp().content_stack.set_visible_child_name("review");
            return;
        }

        // v0.6.0 (Slice C2) — Logbook gets its own page with day-band
        // grouping (Today / Yesterday / Last 7 Days / Older). The
        // regular list page used to render Logbook flat; the new
        // grouping is harder to express through a single GtkListView,
        // so we split it out the same way Forecast / Review do.
        if matches!(active, ActiveList::Logbook) {
            store.remove_all();
            self.refresh_logbook_page();
            self.imp().content_stack.set_visible_child_name("logbook");
            return;
        }

        // v0.6.4 (Slice D2) — Agenda canonical page. Org-mode-style
        // chronological view with five sections.
        if matches!(active, ActiveList::Agenda) {
            store.remove_all();
            self.refresh_agenda_page();
            self.imp().content_stack.set_visible_child_name("agenda");
            return;
        }

        // Phase 12.5 (v0.11.0) — Calendar Month View. Builder-only
        // paper-calendar lens; sidebar entry already filtered out
        // in Simple mode, but defend in depth here too.
        if matches!(active, ActiveList::Calendar) {
            store.remove_all();
            self.refresh_calendar_page();
            self.imp().content_stack.set_visible_child_name("calendar");
            return;
        }

        // Phase 14 — saved perspective. Resolve the filter
        // expression from the meta cache, run it through the same
        // parse + apply pipeline as the search bar, and render the
        // matching tasks in the standard list view. The "list" page
        // owns the rendering — the perspective is a saved query, not
        // a separate page.
        if let ActiveList::Perspective(id) = &active {
            // v0.6.0 (Slice D1 GUI) — perspective whose
            // `renderer = "board"` renders as a kanban instead of
            // a flat list. We branch *before* the list path: the
            // board page has its own host box in the content
            // stack, no shared GtkListView state.
            let perspective_snapshot = self.imp().perspective_meta.borrow().get(id).cloned();
            if let Some(p) = perspective_snapshot
                && p.renderer.eq_ignore_ascii_case("board")
            {
                store.remove_all();
                match self.refresh_board_page(&p) {
                    Ok(()) => {
                        self.imp().content_stack.set_visible_child_name("board");
                        return;
                    }
                    Err(err) => {
                        // Bad renderer_config — surface a toast and
                        // fall through to the list path so the user
                        // still sees their tasks.
                        error!(
                            ?err,
                            perspective_id = id,
                            "board renderer_config malformed; falling back to list"
                        );
                    }
                }
            }
            self.imp().content_stack.set_visible_child_name("list");
            let expr = self
                .imp()
                .perspective_meta
                .borrow()
                .get(id)
                .map(|p| p.filter_expr.clone());
            let Some(expr) = expr else {
                // Perspective row vanished from underneath us
                // (e.g., deleted in another worker iteration). Drop
                // back to Today.
                store.remove_all();
                self.update_empty_state(&store);
                return;
            };
            let parsed = crate::ui::filter::parse(&expr);
            // v0.2.2 — surface unknown-token warnings against the
            // saved expression so users notice when a Perspective's
            // filter has a typo. Deduped so we don't re-toast on
            // every refresh.
            self.surface_filter_warnings(&parsed);
            // v0.6.18 — SQL fast-path for the list-renderer
            // perspective path. v0.5.3 shipped the translation
            // evaluator and v0.6.6 wired it into the board path;
            // this loop was the deferred case noted in the v0.5.3
            // patchnote. Translatable filters (most: is:open,
            // tag:work, due:today, …) load only matching rows
            // instead of pulling the full task table and
            // filtering in Rust. The fallback path keeps the
            // in-memory `filter::apply` for expressions the
            // translator can't yet express (regex, fuzzy,
            // composite is:today / etc.).
            //
            // We need both the name-only `TagMap` (for filter
            // evaluation) and the colour-bearing `TagPillMap`
            // (for row rendering). Pre-v0.6.18 these were two
            // separate DB roundtrips with the same JOIN; now
            // we fetch the pill map once and derive the name
            // map from it.
            let tag_pills: crate::ui::task_list::TagPillMap = pool
                .with(atrium_core::db::read::tag_info_per_task)
                .unwrap_or_default();
            let tag_map: TagMap = crate::ui::task_list::tag_names_from_pills(&tag_pills);
            let project_areas = self.project_areas_map();
            let mut tasks: Vec<Task> = if let Some(expr) = &parsed.expr
                && let Some(clause) = atrium_search::try_translate(expr, today)
            {
                let params: Vec<atrium_core::SqlBindValue> =
                    clause.params.iter().map(Into::into).collect();
                pool.with(|conn| {
                    atrium_core::db::read::list_tasks_matching(conn, &clause.sql, &params)
                })
                .unwrap_or_default()
            } else {
                let loaded = match pool.with(atrium_core::db::read::list_all_tasks) {
                    Ok(t) => t,
                    Err(e) => {
                        error!(?e, perspective_id = id, "failed to load perspective");
                        store.remove_all();
                        self.update_empty_state(&store);
                        return;
                    }
                };
                let blocked_ids = pool
                    .with(atrium_core::db::read::blocked_task_ids)
                    .unwrap_or_default();
                crate::ui::filter::apply_with_blocked(
                    loaded,
                    &parsed,
                    today,
                    &tag_map,
                    &self.imp().project_titles.borrow(),
                    &project_areas,
                    &self.imp().area_titles.borrow(),
                    &blocked_ids,
                )
            };
            // sort: modifiers — both paths need this; only the
            // in-memory `filter::apply` would have applied it
            // pre-v0.6.18. Honour the modifier on either path.
            if !parsed.sorts.is_empty() {
                crate::ui::filter::sort_tasks_by_specs(&mut tasks, &parsed.sorts);
            }
            // v0.5.2 — bm25-rank a Perspective whose filter
            // contains bare text and doesn't pin a sort.
            // `bm25_pinned_sort` mirrors the meaning of
            // `parsed.sorts.is_empty()` for the post-store
            // sort_by_position skip below.
            let bm25_pinned_sort = if parsed.sorts.is_empty() {
                let terms = parsed
                    .expr
                    .as_ref()
                    .map(atrium_search::collect_text_terms)
                    .unwrap_or_default();
                if !terms.is_empty() {
                    let scores = pool
                        .with(|conn| atrium_core::db::read::bm25_for_terms(conn, &terms))
                        .unwrap_or_default();
                    if !scores.is_empty() {
                        crate::ui::filter::rank_by_bm25_recency(&mut tasks, &scores, today);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };
            let context_for = self.build_context_resolver(&active);
            let area_color_for = self.build_area_color_resolver();
            let cookie_for = self.build_cookie_resolver();
            let blocked_ids = pool
                .with(atrium_core::db::read::blocked_task_ids)
                .unwrap_or_default();
            replace_store_with_tags_seq(
                &store,
                &tasks,
                &tag_pills,
                false,
                &blocked_ids,
                context_for,
                area_color_for,
                cookie_for,
            );
            // v0.4.1 — `sort:KEY` modifiers in the saved
            // perspective override position order. apply()
            // already sorted the Vec; just don't clobber it
            // with sort_by_position. v0.5.2 — same skip when
            // bm25 ranking ordered the Vec.
            if parsed.sorts.is_empty() && !bm25_pinned_sort {
                apply_nesting(&store);
            }
            self.update_empty_state(&store);
            return;
        }

        let result: Result<Vec<Task>, _> = pool.with(|conn| match &active {
            ActiveList::Inbox => atrium_core::db::read::list_inbox(conn),
            ActiveList::Today => atrium_core::db::read::list_today(conn, today),
            ActiveList::Upcoming => atrium_core::db::read::list_upcoming(conn, today),
            ActiveList::Anytime => atrium_core::db::read::list_anytime(conn, today),
            ActiveList::Someday => atrium_core::db::read::list_someday(conn),
            ActiveList::Logbook => atrium_core::db::read::list_logbook(conn),
            ActiveList::Project(id) => atrium_core::db::read::list_project(conn, *id),
            ActiveList::Area(id) => atrium_core::db::read::list_area(conn, *id),
            ActiveList::Tag(id) => atrium_core::db::read::list_tasks_with_tag(conn, *id),
            ActiveList::SearchResults(query) => {
                // v0.6.18 — search expressions take the SQL
                // fast-path when the parser can translate them.
                // For 80%+ of typed queries (`tag:work`,
                // `is:overdue`, `due:today`, …) this avoids the
                // load-everything-then-filter-in-Rust pattern.
                // Untranslatable expressions (regex / fuzzy /
                // composite is:today) fall through to the
                // load-everything path; `filter::apply` below
                // handles the rest.
                let parsed = crate::ui::filter::parse(query);
                let Some(expr) = parsed.expr.as_ref() else {
                    return Ok(Vec::new());
                };
                if let Some(clause) = atrium_search::try_translate(expr, today) {
                    let params: Vec<atrium_core::SqlBindValue> =
                        clause.params.iter().map(Into::into).collect();
                    atrium_core::db::read::list_tasks_matching(conn, &clause.sql, &params)
                } else {
                    atrium_core::db::read::list_all_tasks(conn)
                }
            }
            ActiveList::Forecast
            | ActiveList::Review
            | ActiveList::Agenda
            | ActiveList::Calendar
            | ActiveList::Perspective(_) => {
                // Unreachable — gated above. Keeps the match exhaustive.
                Ok(Vec::new())
            }
        });

        match result {
            Ok(tasks) => {
                // v0.6.18 — single tag-info fetch covers both maps.
                let tag_pills: crate::ui::task_list::TagPillMap = pool
                    .with(atrium_core::db::read::tag_info_per_task)
                    .unwrap_or_default();
                let tag_map: TagMap = crate::ui::task_list::tag_names_from_pills(&tag_pills);
                // v0.4.1 — capture whether the user's search expression
                // pinned a sort order so the post-store sort_by_position
                // call can skip when the query already sorted the Vec.
                let mut search_pinned_sort = false;
                let tasks = if let ActiveList::SearchResults(q) = &active {
                    let parsed = crate::ui::filter::parse(q);
                    search_pinned_sort = !parsed.sorts.is_empty();
                    let project_areas = self.project_areas_map();
                    let blocked_ids = pool
                        .with(atrium_core::db::read::blocked_task_ids)
                        .unwrap_or_default();
                    let mut filtered = crate::ui::filter::apply_with_blocked(
                        tasks,
                        &parsed,
                        today,
                        &tag_map,
                        &self.imp().project_titles.borrow(),
                        &project_areas,
                        &self.imp().area_titles.borrow(),
                        &blocked_ids,
                    );
                    // v0.5.2 — bm25 ranking when bare text is in the
                    // query and the user hasn't pinned a sort. We
                    // flip `search_pinned_sort` so the post-store
                    // sort_by_position doesn't clobber the rank.
                    if !search_pinned_sort {
                        let terms = parsed
                            .expr
                            .as_ref()
                            .map(atrium_search::collect_text_terms)
                            .unwrap_or_default();
                        if !terms.is_empty() {
                            let scores = pool
                                .with(|conn| atrium_core::db::read::bm25_for_terms(conn, &terms))
                                .unwrap_or_default();
                            if !scores.is_empty() {
                                crate::ui::filter::rank_by_bm25_recency(
                                    &mut filtered,
                                    &scores,
                                    today,
                                );
                                search_pinned_sort = true;
                            }
                        }
                    }
                    filtered
                } else {
                    tasks
                };
                // Phase 11 — sequential project rendering. Only on
                // a single-project view AND only when the project
                // has sequential=true. Other views (Today, Inbox,
                // Area aggregates) never dim rows.
                let sequential = match &active {
                    ActiveList::Project(id) => self
                        .imp()
                        .project_meta
                        .borrow()
                        .get(id)
                        .is_some_and(|p| p.sequential),
                    _ => false,
                };
                let context_for = self.build_context_resolver(&active);
                let area_color_for = self.build_area_color_resolver();
                let cookie_for = self.build_cookie_resolver();
                let blocked_ids = self
                    .read_pool()
                    .and_then(|p| p.with(atrium_core::db::read::blocked_task_ids).ok())
                    .unwrap_or_default();
                replace_store_with_tags_seq(
                    &store,
                    &tasks,
                    &tag_pills,
                    sequential,
                    &blocked_ids,
                    context_for,
                    area_color_for,
                    cookie_for,
                );
                // Skip the position sort when the search expression
                // pinned a sort — apply() already ordered the Vec.
                if !search_pinned_sort {
                    apply_nesting(&store);
                }
            }
            Err(e) => {
                error!(?e, ?active, "failed to load active list");
                store.remove_all();
            }
        }
        self.update_empty_state(&store);
    }

    /// React to a library-level delta. The sidebar rebuilds from
    /// scratch (small enough; rare events); the active selection is
    /// re-established afterward by walking the freshly-built
    /// `sidebar_targets` for a matching `ActiveList`. Phase 5.5
    /// polish — keeps the highlight where the user left it instead
    /// of dropping back to Today on every rename / new / move.
    pub fn apply_library_changes(&self, changes: &LibraryChanges) {
        let active = self.active_list();
        let deleted = match active {
            ActiveList::Project(id) => changes.projects_deleted.contains(&id),
            ActiveList::Area(id) => changes.areas_deleted.contains(&id),
            ActiveList::Perspective(id) => changes.perspectives_deleted.contains(&id),
            _ => false,
        };
        self.rebuild_dynamic_sidebar();
        // v0.31.0 — reconcile onboarding first; if it took over the
        // display (library now empty, or just left empty), skip the
        // normal refresh.
        if self.sync_onboarding() {
            return;
        }
        if deleted {
            // Active entity is gone — fall back to Today.
            self.set_active_list(ActiveList::Today);
            self.select_sidebar_row_for(ActiveList::Today);
        } else {
            self.select_sidebar_row_for(active);
            self.refresh_active_list();
        }
    }

    /// Find the sidebar row whose target equals `active` and select
    /// it. Used after `rebuild_dynamic_sidebar` to preserve the
    /// user's selection across rebuilds.
    pub(super) fn select_sidebar_row_for(&self, active: ActiveList) {
        // v0.39.0 — Forecast is the Agenda view's Builder-only "Strip"
        // layout, reached via the in-page toggle rather than its own
        // sidebar row. Highlight the Agenda row for it.
        let active = if active == ActiveList::Forecast {
            ActiveList::Agenda
        } else {
            active
        };
        let targets = self.imp().sidebar_targets.borrow();
        for (i, t) in targets.iter().enumerate() {
            if t.as_ref() == Some(&active)
                && let Some(row) = self.imp().sidebar_list.row_at_index(i as i32)
            {
                self.imp().sidebar_list.select_row(Some(&row));
                return;
            }
        }
    }

    pub fn apply_task_changes(&self, changes: &TaskChanges) {
        let Some(store) = self.imp().store.borrow().clone() else {
            return;
        };
        // v0.31.0 — the first captured task dismisses onboarding (and
        // deleting the last one restores it). When onboarding takes
        // over, it reloads the list itself, so skip the incremental
        // apply below.
        if self.sync_onboarding() {
            return;
        }
        let active = self.active_list();
        // Phase 12 — Forecast view rebuilds in full on any task
        // delta. Day-card layout depends on date grouping that's
        // cheaper to recompute than to diff in place.
        if matches!(active, ActiveList::Forecast) {
            self.refresh_forecast_page();
            self.refresh_counts();
            self.refresh_canonical_badges();
            self.refresh_dynamic_badges();
            return;
        }
        // v0.6.0 (Slice C2) — Logbook day-band view. Same shape as
        // Forecast: rebuild on any delta so a freshly-completed task
        // lands in the Today band immediately.
        if matches!(active, ActiveList::Logbook) {
            self.refresh_logbook_page();
            self.refresh_counts();
            self.refresh_canonical_badges();
            self.refresh_dynamic_badges();
            return;
        }
        // v0.6.4 (Slice D2) — Agenda canonical page. Composite over
        // dates + completion + defer; rebuild on any delta so a
        // toggled task slides between sections immediately.
        if matches!(active, ActiveList::Agenda) {
            self.refresh_agenda_page();
            self.refresh_counts();
            self.refresh_canonical_badges();
            self.refresh_dynamic_badges();
            return;
        }
        // Phase 12.5 — Calendar Month View rebuilds in full on any
        // task delta. Drag-to-reschedule produces a TaskChanges
        // update for the dropped task, and the cleanest path to
        // re-render the cells is to rebuild the grid from scratch
        // (same shape as Forecast / Agenda above).
        if matches!(active, ActiveList::Calendar) {
            self.refresh_calendar_page();
            self.refresh_counts();
            self.refresh_canonical_badges();
            self.refresh_dynamic_badges();
            return;
        }
        // v0.7.4 — Review canonical page's "This week" section
        // depends on the weekly-walk filter result + the per-task
        // last_reviewed_at exclusion. Any task delta (especially a
        // MarkTaskReviewed update) needs to rerun both, so rebuild
        // the page in full when the active view is Review.
        if matches!(active, ActiveList::Review) {
            self.refresh_review_page();
            self.refresh_counts();
            self.refresh_canonical_badges();
            self.refresh_dynamic_badges();
            return;
        }
        // Phase 14 — perspective views run a saved filter expression
        // against the global task set. The diff applier doesn't have
        // visibility into the filter, so rerun the read query (same
        // path SearchResults takes — cheap; FTS5-backed when the
        // expression has freeform text).
        if matches!(active, ActiveList::Perspective(_)) {
            self.refresh_active_list();
            self.refresh_counts();
            self.refresh_canonical_badges();
            self.refresh_dynamic_badges();
            return;
        }
        let today = Local::now().date_naive();
        // Re-load the per-task tag pill map so the diff applier
        // renders updated pills with their colours. Drop the older
        // name-only TagMap here — the diff applier no longer needs
        // it (only filter::apply does, and it isn't called inside
        // apply_task_changes).
        let tag_pills: crate::ui::task_list::TagPillMap = self
            .read_pool()
            .and_then(|p| p.with(atrium_core::db::read::tag_info_per_task).ok())
            .unwrap_or_default();
        // Phase 11 — propagate the sequential flag so the diff
        // applier recomputes queued state when the active view is
        // a sequential project.
        let sequential = match &active {
            ActiveList::Project(id) => self
                .imp()
                .project_meta
                .borrow()
                .get(id)
                .is_some_and(|p| p.sequential),
            _ => false,
        };
        let context_for = self.build_context_resolver(&active);
        let area_color_for = self.build_area_color_resolver();
        let cookie_for = self.build_cookie_resolver();
        // v0.29.0 — fresh blocked set so completing a prerequisite
        // unblocks its dependents in the same delta (recomputed across
        // the store inside apply_changes_seq).
        let blocked_ids = self
            .read_pool()
            .and_then(|p| p.with(atrium_core::db::read::blocked_task_ids).ok())
            .unwrap_or_default();
        crate::ui::task_list::apply_changes_seq(
            &store,
            changes,
            active,
            today,
            &tag_pills,
            sequential,
            &blocked_ids,
            context_for,
            area_color_for,
            cookie_for,
        );
        // Subtasks (v0.23.0) — re-nest after the delta so a freshly
        // created / reparented child lands under its parent without a
        // full reload. Falls back to position order for flat sets.
        apply_nesting(&store);
        self.update_empty_state(&store);
        // Phase 5c: any task delta might have moved a count.
        self.refresh_counts();
        self.refresh_canonical_badges();
        self.refresh_dynamic_badges();
    }

    pub(super) fn update_empty_state(&self, store: &gio::ListStore) {
        let active = self.active_list();
        let stack = self.imp().content_stack.clone();

        if store.n_items() == 0 {
            if let Some(status) = self.imp().content_status.get() {
                let (title, description) = self.empty_state_copy(&active);
                status.set_title(&title);
                status.set_description(Some(&description));
                status.set_icon_name(Some(icon_for(&active)));
            }
            stack.set_visible_child_name("empty");
        } else {
            stack.set_visible_child_name("list");
        }
    }

    pub(super) fn empty_state_copy(&self, active: &ActiveList) -> (String, String) {
        match active {
            ActiveList::Inbox => (
                gettext("Inbox zero"),
                gettext(
                    "Catch a thought with Ctrl+N or the entry below — Atrium will keep it safe until you place it.",
                ),
            ),
            ActiveList::Today => (
                gettext("Clear plate today"),
                gettext(
                    "Nothing scheduled and no deadlines crossing the horizon. Glance at Upcoming for what's next, or take the afternoon back.",
                ),
            ),
            ActiveList::Upcoming => (
                gettext("Open horizon"),
                gettext("Schedule a task to a future date and it'll surface here, sorted by when."),
            ),
            ActiveList::Anytime => (
                gettext("Nothing waiting"),
                gettext(
                    "Open tasks without a date land here — your low-pressure pool to dip into when there's time.",
                ),
            ),
            ActiveList::Someday => (
                gettext("Park it for later"),
                gettext(
                    "Ideas and maybes belong here. Scheduled to Someday means \"on the radar, no commitment yet\".",
                ),
            ),
            ActiveList::Logbook => (
                gettext("Nothing logged yet"),
                gettext(
                    "Completed tasks settle here in reverse chronological order — your record of the work done.",
                ),
            ),
            ActiveList::Project(_) => (
                gettext_f(
                    "{title} is empty",
                    &[("title", &self.title_for(active.clone()))],
                ),
                gettext(
                    "Add the first task with the entry below, or capture quickly with Ctrl+Alt+Space.",
                ),
            ),
            ActiveList::Area(_) => (
                gettext_f(
                    "Nothing open in {title}",
                    &[("title", &self.title_for(active.clone()))],
                ),
                gettext(
                    "An area aggregates open tasks across its projects. Add a project under it, then file tasks into the project.",
                ),
            ),
            ActiveList::Tag(_) => (
                gettext_f(
                    "No tasks tagged {tag}",
                    &[("tag", &self.title_for(active.clone()))],
                ),
                // Translators: `#tag` is the literal inline-capture
                // syntax and must stay as-is.
                gettext("Apply this tag from a task's Inspector or with #tag in Quick Entry."),
            ),
            ActiveList::SearchResults(q) if q.trim().is_empty() => (
                gettext("Search Atrium"),
                // Translators: `tag:errand`, `due:today`, and
                // `is:overdue` are literal search-grammar tokens and
                // must not be translated.
                gettext(
                    "Type to find tasks by title or note. Try filters too: tag:errand, due:today, is:overdue.",
                ),
            ),
            ActiveList::SearchResults(q) => (
                gettext_f("No matches for \u{201c}{query}\u{201d}", &[("query", q)]),
                gettext(
                    "Search covers task titles, notes, and filter expressions. Check spelling, or try a broader term.",
                ),
            ),
            ActiveList::Forecast => (
                gettext("Open horizon"),
                gettext(
                    "Schedule, deadline, or defer a task and it'll appear here on its day. Drag rows between days to reschedule.",
                ),
            ),
            ActiveList::Review => (
                gettext("All caught up"),
                gettext(
                    "Projects with a review interval surface here when their last review goes stale — oldest first.",
                ),
            ),
            ActiveList::Perspective(_) => (
                gettext_f(
                    "{title} is quiet",
                    &[("title", &self.title_for(active.clone()))],
                ),
                gettext(
                    "No tasks currently match this perspective's filter expression. Adjust the filter or wait for matches to appear.",
                ),
            ),
            ActiveList::Agenda => (
                gettext("Nothing on the agenda"),
                gettext(
                    "No overdue, today, or near-term scheduled tasks — the next two weeks are clear.",
                ),
            ),
            ActiveList::Calendar => (
                gettext("Open month"),
                gettext(
                    "Schedule a task and its day cell will fill in. Page Up / Page Down to navigate months; Today resets to the current month.",
                ),
            ),
        }
    }
}
