// SPDX-License-Identifier: MIT
//! v0.31.0 — first-run / onboarding.
//!
//! A brand-new database (no tasks, no projects, no areas) shows a
//! welcoming `AdwStatusPage` with three next-steps instead of an empty
//! Inbox. It clears itself the moment the user creates anything — no
//! GSetting, no seeding. The page is a named child of the existing
//! `content_stack`; `refresh_active_list` yields to it while the
//! cached `db_empty` flag is set, and the task / library change
//! handlers recompute that flag so the page appears and disappears in
//! step with the data.

use crate::i18n::gettext;

use super::*;

impl AtriumWindow {
    /// Build the owned empty-state page (Phase 22 C2) and parent it into
    /// the `content_status_host` box the template provides. Called once at
    /// window setup, before the first content refresh. `update_empty_state`
    /// swaps its title / description / icon per active list; the defaults
    /// here mirror the strings the template used to carry, so any fallback
    /// render still reads sensibly.
    pub(super) fn setup_content_status(&self) {
        let status = crate::ui::status_page::status_page(
            Some("checkmark-symbolic"),
            &gettext("No tasks yet"),
            Some(&gettext(
                "Press Ctrl+N or the + button to create your first task.",
            )),
        );
        self.imp().content_status_host.append(status.widget());
        let _ = self.imp().content_status.set(status);
    }

    /// Build the onboarding status page and add it to the content stack
    /// as the `"onboarding"` page. Called once at window setup.
    pub(super) fn setup_onboarding_page(&self) {
        let status = crate::ui::status_page::status_page(
            Some("io.github.virinvictus.atrium"),
            &gettext("Welcome to Atrium"),
            Some(&gettext("Your tasks live here. Start one of three ways:")),
        );

        let buttons = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::Center)
            .build();

        let create_project = pill_button(&gettext("Create your first project"));
        create_project.connect_clicked(clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.prompt_create_project()
        ));

        let capture = pill_button(&gettext("Capture a task"));
        capture.connect_clicked(clone!(
            #[weak(rename_to = win)]
            self,
            move |_| {
                // The Quick Entry modal is an app-level action so the
                // shortcut works window-independently; activate it.
                let _ = WidgetExt::activate_action(&win, "app.quick-entry", None);
            }
        ));

        let vault = pill_button(&gettext("Set up an Org vault"));
        vault.connect_clicked(clone!(
            #[weak(rename_to = win)]
            self,
            move |_| crate::ui::preferences::open(&win)
        ));

        buttons.append(&create_project);
        buttons.append(&capture);
        buttons.append(&vault);
        status.set_child(&buttons);

        self.imp()
            .content_stack
            .add_named(status.widget(), Some("onboarding"));
    }

    /// Recompute and cache whether the library is empty. Short-circuits
    /// on the first task so a populated DB never lists projects/areas.
    pub(super) fn recompute_db_empty(&self) -> bool {
        let empty = self
            .read_pool()
            .and_then(|pool| {
                pool.with(|conn| {
                    if atrium_core::db::read::count_tasks(conn)? > 0 {
                        return Ok(false);
                    }
                    Ok(atrium_core::db::read::list_areas(conn)?.is_empty()
                        && atrium_core::db::read::list_projects(conn)?.is_empty())
                })
                .ok()
            })
            .unwrap_or(false);
        self.imp().db_empty.set(empty);
        empty
    }

    /// Reconcile the onboarding page with the current data. Returns
    /// `true` when it took over the display (the caller should skip its
    /// normal content refresh). Called from the change handlers.
    pub(super) fn sync_onboarding(&self) -> bool {
        let empty = self.recompute_db_empty();
        let showing =
            self.imp().content_stack.visible_child_name().as_deref() == Some("onboarding");
        if empty {
            if !showing {
                self.imp()
                    .content_stack
                    .set_visible_child_name("onboarding");
            }
            true
        } else if showing {
            // Just became non-empty — restore the active list view.
            self.refresh_active_list();
            true
        } else {
            false
        }
    }
}

fn pill_button(label: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.add_css_class("pill");
    b.add_css_class("suggested-action");
    b
}
