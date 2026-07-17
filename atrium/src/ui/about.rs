// SPDX-License-Identifier: MIT
//! "About Atrium" dialog. Built imperatively (no .ui template) since
//! it's small and benefits from carrying the runtime version directly.

use adw::prelude::*;
use gtk::glib;

use crate::i18n::gettext;

const REPO_URL: &str = "https://github.com/VirInvictus/Atrium";
const COPYRIGHT: &str = "© 2026 Brandon LaRocque";

pub fn show(parent: &impl IsA<gtk::Widget>) {
    // Plain GTK since Phase 22 (C1): gtk::AboutDialog is a real toplevel,
    // not an in-window sheet. issue_url has no gtk::AboutDialog equivalent,
    // so the tracker rides the website label; the adwaita acknowledgement /
    // legal sections map onto credit sections.
    let about = gtk::AboutDialog::builder()
        .program_name("Atrium")
        .logo_icon_name(atrium_core::APP_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .website(REPO_URL)
        .website_label(gettext("Website & issue tracker"))
        .license_type(gtk::License::MitX11)
        .copyright(COPYRIGHT)
        // Translators: "Things 3" and "OmniFocus" are product names — keep them as-is.
        .comments(gettext(
            "A native GNOME task manager — Things 3 clarity, OmniFocus depth, mode-switched.",
        ))
        .authors(vec!["Brandon LaRocque <larocque.brandon@gmail.com>"])
        .artists(vec!["Brandon LaRocque"])
        .modal(true)
        .build();

    // Acknowledge the upstream influences explicitly — this is a
    // portfolio-piece detail that matters to the project's framing.
    about.add_credit_section(
        &gettext("Built on the shoulders of"),
        &[
            "Things 3 by Cultured Code",
            "OmniFocus by The Omni Group",
            "Org-mode by Carsten Dominik, Bastien Guerry, et al.",
            "NetNewsWire by Brent Simmons (single-writer SQLite discipline)",
        ],
    );

    // Bundled fonts: adwaita's dedicated legal section has no gtk::AboutDialog
    // analogue, so the attributions ride a credit section. Full SIL OFL 1.1
    // terms travel in data/fonts/*-LICENSE.* as before.
    // Translators: "SIL OFL 1.1" is a license identifier — keep it as-is.
    about.add_credit_section(
        &gettext("Bundled fonts (SIL OFL 1.1)"),
        &[
            "Inter — The Inter Project Authors",
            "Source Serif 4 — Adobe",
            "JetBrains Mono — JetBrains s.r.o.",
        ],
    );

    let window = parent
        .upcast_ref::<gtk::Widget>()
        .root()
        .and_downcast::<gtk::Window>();
    about.set_transient_for(window.as_ref());
    about.present();
}

/// Glib action handler — wired into the application as `app.about`.
pub fn install_action(app: &adw::Application) {
    let action = gtk::gio::SimpleAction::new("about", None);
    action.connect_activate(glib::clone!(
        #[weak]
        app,
        move |_, _| {
            if let Some(window) = app.active_window() {
                show(&window);
            }
        }
    ));
    app.add_action(&action);
}
