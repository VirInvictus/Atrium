// SPDX-License-Identifier: MIT
//! v0.34.0 — unified import dialog. The GUI side of Phase 19's
//! import story: pick a source format and a file, optionally preview
//! (dry run), and import through the single-writer worker. The
//! importers themselves live in `atrium-import` (Todoist / Taskwarrior
//! / todo.txt / VTODO) and `atrium-org` (Org), so this dialog is thin
//! glue that mirrors the CLI's `run_import` per-source flow.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use atrium_core::WorkerHandle;
use atrium_import::UdaPolicy;
use gtk::gio;
use gtk::glib;

use crate::i18n::{gettext, gettext_f};

/// Open the import dialog anchored to `parent`, running imports
/// through `worker`.
pub fn open(parent: &impl IsA<gtk::Widget>, worker: WorkerHandle) {
    let dialog = adw::Dialog::builder()
        .title(gettext("Import"))
        .content_width(540)
        .content_height(560)
        .build();

    let page = adw::PreferencesPage::new();

    let group = adw::PreferencesGroup::builder()
        .title(gettext("Source"))
        .build();
    // Source formats, in the order `run_gui_import` matches on.
    // Translators: import format names — product names ("Todoist",
    // "Taskwarrior", "VTODO", "todo.txt") and file extensions stay as-is.
    let src_org = gettext("Org vault file (.org)");
    let src_todoist = gettext("Todoist CSV");
    let src_vtodo = gettext("VTODO (.ics)");
    let src_taskwarrior = gettext("Taskwarrior JSON");
    let src_todotxt = gettext("todo.txt");
    let source_model = gtk::StringList::new(&[
        src_org.as_str(),
        src_todoist.as_str(),
        src_vtodo.as_str(),
        src_taskwarrior.as_str(),
        src_todotxt.as_str(),
    ]);
    let source_row = adw::ComboRow::builder()
        .title(gettext("Format"))
        .model(&source_model)
        .build();
    group.add(&source_row);

    let file_row = adw::ActionRow::builder()
        .title(gettext("File"))
        .subtitle(gettext("No file chosen"))
        .build();
    let choose_btn = gtk::Button::builder()
        .label(gettext("Choose…"))
        .valign(gtk::Align::Center)
        .build();
    choose_btn.add_css_class("flat");
    file_row.add_suffix(&choose_btn);
    group.add(&file_row);

    let project_row = adw::EntryRow::builder()
        .title(gettext("Project name (ignored for Org)"))
        // Translators: default name of the project created to hold imported tasks.
        .text(gettext("Imported"))
        .build();
    group.add(&project_row);

    // Translators: policies for mapping Taskwarrior user-defined
    // attributes — kept as a tag, kept as a note line, or dropped.
    let uda_tag = gettext("Tag");
    let uda_note = gettext("Note");
    let uda_drop = gettext("Drop");
    let uda_model = gtk::StringList::new(&[uda_tag.as_str(), uda_note.as_str(), uda_drop.as_str()]);
    let uda_row = adw::ComboRow::builder()
        .title(gettext("Taskwarrior UDAs"))
        .subtitle(gettext(
            "How user-defined attributes map (Taskwarrior only)",
        ))
        .model(&uda_model)
        .build();
    group.add(&uda_row);

    let dryrun_row = adw::SwitchRow::builder()
        .title(gettext("Dry run"))
        .subtitle(gettext("Preview the result without writing anything"))
        .active(true)
        .build();
    group.add(&dryrun_row);
    page.add(&group);

    let result_group = adw::PreferencesGroup::builder()
        .title(gettext("Result"))
        .build();
    let result_label = gtk::Label::builder()
        .label(gettext("Choose a file, then Import."))
        .wrap(true)
        .xalign(0.0)
        .selectable(true)
        .build();
    result_label.add_css_class("dim-label");
    result_group.add(&result_label);
    page.add(&result_group);

    let chosen: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));

    // File chooser.
    {
        let chosen = chosen.clone();
        let file_row = file_row.clone();
        choose_btn.connect_clicked(move |btn| {
            let window = btn.root().and_downcast::<gtk::Window>();
            let file_dialog = gtk::FileDialog::builder()
                .title(gettext("Choose a file to import"))
                .build();
            let chosen = chosen.clone();
            let file_row = file_row.clone();
            file_dialog.open(window.as_ref(), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res
                    && let Some(path) = file.path()
                {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    file_row.set_subtitle(&name);
                    *chosen.borrow_mut() = Some(path);
                }
            });
        });
    }

    // Import button in the header.
    let import_btn = gtk::Button::builder().label(gettext("Import")).build();
    import_btn.add_css_class("suggested-action");
    {
        let chosen = chosen.clone();
        let source_row = source_row.clone();
        let project_row = project_row.clone();
        let uda_row = uda_row.clone();
        let dryrun_row = dryrun_row.clone();
        let result_label = result_label.clone();
        let worker = worker.clone();
        import_btn.connect_clicked(move |_| {
            let Some(path) = chosen.borrow().clone() else {
                result_label.set_label(&gettext("Choose a file first."));
                return;
            };
            let source = source_row.selected() as usize;
            let project = {
                let t = project_row.text().trim().to_string();
                if t.is_empty() {
                    // Translators: default name of the project created to hold imported tasks.
                    gettext("Imported")
                } else {
                    t
                }
            };
            let uda = match uda_row.selected() {
                1 => UdaPolicy::Note,
                2 => UdaPolicy::Drop,
                _ => UdaPolicy::Tag,
            };
            let dry_run = dryrun_row.is_active();
            let result_label = result_label.clone();
            let worker = worker.clone();
            result_label.set_label(&gettext("Working…"));
            glib::MainContext::default().spawn_local(async move {
                match run_gui_import(worker, source, path, project, uda, dry_run).await {
                    Ok(msg) => result_label.set_label(&msg),
                    Err(e) => result_label
                        .set_label(&gettext_f("Import failed: {error}", &[("error", &e)])),
                }
            });
        });
    }

    let header = adw::HeaderBar::new();
    header.pack_end(&import_btn);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&page)
        .build();
    toolbar.set_content(Some(&scrolled));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

/// Run the chosen importer through the worker and return a one-line
/// human summary. Mirrors the CLI's `run_import` per-source flow.
async fn run_gui_import(
    worker: WorkerHandle,
    source: usize,
    path: PathBuf,
    project: String,
    uda: UdaPolicy,
    dry_run: bool,
) -> Result<String, String> {
    use atrium_import::{import, vtodo};
    let read = |p: &PathBuf| {
        std::fs::read_to_string(p)
            .map_err(|e| gettext_f("cannot read file: {error}", &[("error", &e.to_string())]))
    };
    let today = chrono::Local::now().date_naive();
    match source {
        0 => {
            let s = atrium_org::org::import_org_file(&worker, &path, dry_run)
                .await
                .map_err(|e| e.to_string())?;
            // Translators: placeholder project name shown when an Org
            // vault import has no single project title.
            let vault_placeholder = gettext("(vault)");
            Ok(format_import_result(
                "Org",
                s.project_title.as_deref().unwrap_or(&vault_placeholder),
                s.tasks_created,
                s.tags_ensured,
                s.lossy.len(),
                dry_run,
            ))
        }
        1 => {
            let rows = import::todoist::parser::parse_csv(&read(&path)?).map_err(|e| {
                gettext_f("Todoist parse error: {error}", &[("error", &e.to_string())])
            })?;
            let s =
                import::todoist::mapper::import_todoist(&worker, &rows, &project, today, dry_run)
                    .await
                    .map_err(|e| e.to_string())?;
            Ok(format_import_result(
                "Todoist",
                &s.project_title,
                s.tasks_created,
                s.tags_created,
                s.lossy.len(),
                dry_run,
            ))
        }
        2 => {
            let parsed = vtodo::parse_ics(&read(&path)?).map_err(|e| {
                gettext_f("VTODO parse error: {error}", &[("error", &e.to_string())])
            })?;
            let s = vtodo::import_vtodo(&worker, &parsed, &project, dry_run)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format_import_result(
                "VTODO",
                &s.project_title,
                s.tasks_created,
                s.tags_created,
                s.lossy.len(),
                dry_run,
            ))
        }
        3 => {
            let parsed = import::taskwarrior::parser::parse_export(&read(&path)?).map_err(|e| {
                gettext_f(
                    "Taskwarrior parse error: {error}",
                    &[("error", &e.to_string())],
                )
            })?;
            let s = import::taskwarrior::mapper::import_taskwarrior(
                &worker, &parsed, &project, uda, dry_run,
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(format_import_result(
                "Taskwarrior",
                &s.project_title,
                s.tasks_created,
                s.tags_created,
                s.lossy.len(),
                dry_run,
            ))
        }
        _ => {
            let parsed = import::todotxt::parser::parse_document(&read(&path)?);
            let s = import::todotxt::mapper::import_todotxt(&worker, &parsed, &project, dry_run)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format_import_result(
                "todo.txt",
                &s.project_title,
                s.tasks_created,
                s.tags_created,
                s.lossy.len(),
                dry_run,
            ))
        }
    }
}

/// Build the human summary shown in the dialog's result pane. Pure, so
/// the count / dry-run wording is unit-testable without GTK.
fn format_import_result(
    source: &str,
    project: &str,
    tasks: usize,
    tags: usize,
    lossy: usize,
    dry_run: bool,
) -> String {
    let tasks_str = tasks.to_string();
    let tags_str = tags.to_string();
    let args: &[(&str, &str)] = &[
        ("tasks", &tasks_str),
        ("project", project),
        ("source", source),
        ("tags", &tags_str),
    ];
    let mut s = if dry_run {
        gettext_f(
            "Would import {tasks} task(s) into \"{project}\" from {source}; {tags} tag(s) ensured.",
            args,
        )
    } else {
        gettext_f(
            "Imported {tasks} task(s) into \"{project}\" from {source}; {tags} tag(s) ensured.",
            args,
        )
    };
    if lossy > 0 {
        s.push('\n');
        s.push_str(&gettext_f(
            "{lossy} field(s) didn't map cleanly (lossy) — run the CLI importer for the full report.",
            &[("lossy", &lossy.to_string())],
        ));
    }
    if dry_run {
        s.push_str("\n\n");
        s.push_str(&gettext(
            "Dry run: nothing was written. Turn off Dry run and import again to apply.",
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::format_import_result;

    #[test]
    fn dry_run_wording_and_lossy_note() {
        let s = format_import_result("Todoist", "Home", 46, 2, 3, true);
        assert!(s.contains("Would import 46 task(s)"));
        assert!(s.contains("into \"Home\""));
        assert!(s.contains("2 tag(s)"));
        assert!(s.contains("3 field(s) didn't map"));
        assert!(s.contains("Dry run: nothing was written"));
    }

    #[test]
    fn wet_run_no_lossy_is_clean() {
        let s = format_import_result("Org", "(vault)", 5, 1, 0, false);
        assert!(s.starts_with("Imported 5 task(s)"));
        assert!(!s.contains("lossy"));
        assert!(!s.contains("Dry run"));
    }
}
