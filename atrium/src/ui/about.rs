// SPDX-License-Identifier: MIT
//! "About Atrium" dialog. Built imperatively (no .ui template) since
//! it's small and benefits from carrying the runtime version directly.

use adw::prelude::*;
use gtk::glib;

const REPO_URL: &str = "https://github.com/VirInvictus/Atrium";
const ISSUE_URL: &str = "https://github.com/VirInvictus/Atrium/issues";
const COPYRIGHT: &str = "© 2026 Brandon LaRocque";

pub fn show(parent: &impl IsA<gtk::Widget>) {
    let about = adw::AboutDialog::builder()
        .application_name("Atrium")
        .application_icon(atrium_core::APP_ID)
        .developer_name("Brandon LaRocque")
        .version(env!("CARGO_PKG_VERSION"))
        .website(REPO_URL)
        .issue_url(ISSUE_URL)
        .license_type(gtk::License::MitX11)
        .copyright(COPYRIGHT)
        .comments("A native GNOME task manager — Things 3 clarity, OmniFocus depth, mode-switched.")
        .build();

    about.set_developers(&["Brandon LaRocque <larocque.brandon@gmail.com>"]);
    about.set_designers(&["Brandon LaRocque"]);

    // Acknowledge the upstream influences explicitly — this is a
    // portfolio-piece detail that matters to the project's framing.
    about.add_acknowledgement_section(
        Some("Built on the shoulders of"),
        &[
            "Things 3 by Cultured Code",
            "OmniFocus by The Omni Group",
            "Org-mode by Carsten Dominik, Bastien Guerry, et al.",
            "NetNewsWire by Brent Simmons (single-writer SQLite discipline)",
        ],
    );

    about.add_legal_section(
        "Bundled fonts",
        Some("These typefaces ship with Atrium under SIL OFL 1.1."),
        gtk::License::Custom,
        Some(
            "Inter — © The Inter Project Authors\n\
             Source Serif 4 — © Adobe\n\
             JetBrains Mono — © JetBrains s.r.o.\n\n\
             See data/fonts/*-LICENSE.* for the full SIL OFL 1.1 terms.",
        ),
    );

    about.present(Some(&parent.upcast_ref::<gtk::Widget>().clone()));
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
