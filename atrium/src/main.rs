// SPDX-License-Identifier: MIT
//! Atrium binary entry point.
//!
//! Phase 4 turns the binary into a real working task surface. `main`
//! parses CLI flags, builds the tokio runtime, and either short-circuits
//! to a fixture-only run or hands off to the GTK `adw::Application`.
//! `connect_activate` opens the DB, spawns the worker, builds the
//! window, installs actions + keyboard accelerators, and bridges
//! `TaskChanges` from the worker to the window's diff applier.

mod debug;
mod error;
mod quickentry;
mod ui;

use std::sync::OnceLock;

use adw::prelude::*;
use anyhow::{Context, Result};
use atrium_core::db::fixtures::FixtureScale;
use atrium_core::db::read_pool::ReadPool;
use atrium_core::{LibraryChanges, TaskChanges, WorkerHandle};
use gtk::glib::clone;
use gtk::{gio, glib};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::error::{AtriumError, UiError};
use crate::ui::window::AtriumWindow;

const APP_ID: &str = atrium_core::APP_ID;

/// Process-wide tokio runtime. Built once in `main`; lives until the
/// binary exits.
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| Runtime::new().expect("failed to build tokio multi-thread runtime"))
}

fn main() -> glib::ExitCode {
    let cfg = match Config::from_args(std::env::args()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return glib::ExitCode::FAILURE;
        }
    };

    init_tracing();
    install_gsettings_schema_dir();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        debug_mode = cfg.debug,
        app_id = APP_ID,
        "atrium starting"
    );

    if cfg.help_requested {
        print_help();
        return glib::ExitCode::SUCCESS;
    }
    if cfg.version_requested {
        println!("atrium {}", env!("CARGO_PKG_VERSION"));
        return glib::ExitCode::SUCCESS;
    }
    if let Some(scale) = cfg.fixture {
        return run_fixture_oneshot(scale);
    }

    // Force runtime initialisation so signal handlers can spawn onto it.
    let _ = runtime();

    let app = adw::Application::builder().application_id(APP_ID).build();
    install_actions(&app, cfg.debug);
    install_accels(&app);
    connect_startup(&app);
    connect_activate(&app, cfg.debug);

    // Strip our flags from what GApplication sees.
    let exit = app.run_with_args(&["atrium"]);

    info!("atrium exited cleanly");
    exit
}

fn run_fixture_oneshot(scale: FixtureScale) -> glib::ExitCode {
    let db_path = atrium_core::db_path();
    info!(scale = ?scale, db = %db_path.display(), "running fixture-only mode");
    match generate_fixtures(&db_path, scale) {
        Ok(summary) => {
            println!(
                "Generated {} tasks across {} projects in {} areas ({} tags) in {} ms.",
                summary.tasks, summary.projects, summary.areas, summary.tags, summary.elapsed_ms
            );
            glib::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("fixture generation failed: {e:#}");
            glib::ExitCode::FAILURE
        }
    }
}

fn generate_fixtures(
    db_path: &std::path::Path,
    scale: FixtureScale,
) -> Result<atrium_core::db::fixtures::FixtureSummary> {
    let mut conn = atrium_core::db::open(db_path)
        .with_context(|| format!("open database at {}", db_path.display()))?;
    atrium_core::db::fixtures::generate(&mut conn, scale).context("fixture generation failed")
}

fn connect_startup(app: &adw::Application) {
    app.connect_startup(|_app| {
        let installed = ui::typography::install_bundled_fonts();
        info!(font_files_present = installed, "typography ready");
        ui::typography::apply_bundled_stylesheet();
        ui::typography::register_icon_search_paths();
    });
}

fn connect_activate(app: &adw::Application, debug: bool) {
    app.connect_activate(move |app| {
        // Single-instance: present the existing window if any.
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }

        let win = AtriumWindow::new(app, debug);

        match boot_data_layer() {
            Ok(booted) => {
                win.attach_data_layer(booted.handle, booted.pool);
                bridge_task_changes(booted.task_changes_rx, &win);
                bridge_library_changes(booted.library_changes_rx, &win);
                if let Some(rx) = booted.vault_events_rx {
                    bridge_vault_events(rx, &win);
                }
                if debug {
                    let _ = debug::Pane::new();
                }
            }
            Err(e) => {
                error!(
                    ?e,
                    "data layer boot failed; window will run with read-only stub"
                );
            }
        }

        win.present();
    });
}

/// What `boot_data_layer` hands back to the GUI shell. Bundled as
/// a struct rather than a tuple so adding another field (vault
/// events, future channels) doesn't ripple through call sites.
struct BootedDataLayer {
    handle: WorkerHandle,
    task_changes_rx: mpsc::UnboundedReceiver<TaskChanges>,
    library_changes_rx: mpsc::UnboundedReceiver<LibraryChanges>,
    pool: ReadPool,
    /// `Some` when a vault is configured; carries `ConflictBackup`
    /// and `ParseFailed` events the GUI surfaces as toasts. `None`
    /// in DB-only mode.
    vault_events_rx: Option<mpsc::UnboundedReceiver<atrium_org::VaultEvent>>,
}

/// Open the DB, spawn the worker, build the read pool, and (when
/// the `vault-path` GSettings key is non-empty and usable) attach
/// the full Phase 17 two-way vault loop: writer + watcher + shared
/// `RecentWrites` set + [`VaultEvent`] channel.
///
/// Empty key or unusable path falls through to DB-only mode; the
/// boot itself only fails for genuine DB errors.
fn boot_data_layer() -> std::result::Result<BootedDataLayer, AtriumError> {
    let db_path = atrium_core::db_path();
    let conn = atrium_core::db::open(&db_path)?;
    let pool = ReadPool::new(db_path.clone(), 4);

    let (vault_config, vault_loop, events_rx) = match read_vault_setup_from_settings(&pool) {
        Ok(Some((cfg, vl, rx))) => (Some(cfg), Some(vl), Some(rx)),
        Ok(None) => (None, None, None),
        Err(e) => {
            // A bad vault setting shouldn't lock the user out of
            // their tasks. Surface the failure in the log and
            // fall back to DB-only.
            warn!(error = %e, "vault config unusable; running DB-only");
            (None, None, None)
        }
    };

    let _enter = runtime().handle().enter();
    let (handle, changes_rx, library_rx) = atrium_core::spawn_worker_with_vault(conn, vault_config);

    // Now that the worker exists, attach the watcher half of the
    // vault loop. A failure here doesn't fail the boot — the writer
    // half is already running, so DB → vault still works; only
    // vault → DB sync is missing. Log and continue.
    if let Some(vl) = vault_loop {
        match vl.attach_watcher(handle.clone()) {
            Ok(_join) => info!("vault watcher attached"),
            Err(e) => warn!(error = %e, "vault watcher failed to attach; vault → DB sync disabled"),
        }
    }

    Ok(BootedDataLayer {
        handle,
        task_changes_rx: changes_rx,
        library_changes_rx: library_rx,
        pool,
        vault_events_rx: events_rx,
    })
}

/// Read the `vault-path` GSettings key and, when set + usable,
/// build the writer-side wiring of the Phase 17 two-way loop.
/// Returns `Ok(None)` when the key is empty (the default — DB-only
/// mode); `Err` when the key is set but the path can't be used.
fn read_vault_setup_from_settings(
    pool: &ReadPool,
) -> std::result::Result<
    Option<(
        atrium_core::VaultConfig,
        atrium_org::VaultLoopHandle,
        mpsc::UnboundedReceiver<atrium_org::VaultEvent>,
    )>,
    UiError,
> {
    let settings = gio::Settings::new(atrium_core::APP_ID);
    let raw: String = settings.string("vault-path").into();
    let path = raw.trim();
    if path.is_empty() {
        return Ok(None);
    }
    let path = std::path::PathBuf::from(path);
    // Auto-create the vault directory if absent — most users
    // setting this key for the first time won't have provisioned
    // ~/Tasks/ themselves. Idempotent on already-existing dirs.
    std::fs::create_dir_all(&path).map_err(|e| UiError::VaultPathInvalid {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    info!(
        path = %path.display(),
        "vault path configured; two-way sync (writer + watcher) enabled"
    );
    let _enter = runtime().handle().enter();
    Ok(Some(atrium_org::spawn_vault_loop(path, pool.clone())))
}

/// Bridge worker → UI. tokio mpsc receivers are runtime-agnostic at
/// the waker layer, so glib's executor drives them directly.
fn bridge_task_changes(mut rx: mpsc::UnboundedReceiver<TaskChanges>, window: &AtriumWindow) {
    let win_weak = window.downgrade();
    glib::MainContext::default().spawn_local(async move {
        while let Some(changes) = rx.recv().await {
            let Some(win) = win_weak.upgrade() else {
                tracing::info!("window dropped; UI bridge exiting");
                break;
            };
            tracing::trace!(
                created = changes.created.len(),
                updated = changes.updated.len(),
                deleted = changes.deleted.len(),
                status_changed = changes.status_changed.len(),
                "TaskChanges arrived on UI thread"
            );
            win.apply_task_changes(&changes);
        }
        tracing::info!("worker changes channel closed; UI bridge exiting");
    });
}

fn bridge_library_changes(mut rx: mpsc::UnboundedReceiver<LibraryChanges>, window: &AtriumWindow) {
    let win_weak = window.downgrade();
    glib::MainContext::default().spawn_local(async move {
        while let Some(changes) = rx.recv().await {
            let Some(win) = win_weak.upgrade() else {
                tracing::info!("window dropped; library bridge exiting");
                break;
            };
            tracing::trace!(
                areas_created = changes.areas_created.len(),
                areas_updated = changes.areas_updated.len(),
                areas_deleted = changes.areas_deleted.len(),
                projects_created = changes.projects_created.len(),
                projects_updated = changes.projects_updated.len(),
                projects_deleted = changes.projects_deleted.len(),
                "LibraryChanges arrived on UI thread"
            );
            win.apply_library_changes(&changes);
        }
        tracing::info!("worker library channel closed; library bridge exiting");
    });
}

/// Bridge vault writer + watcher operational events to the toast
/// surface. ConflictBackup tells the user their external edit was
/// preserved; ParseFailed tells them a malformed `.org` file was
/// skipped on read-back.
fn bridge_vault_events(
    mut rx: mpsc::UnboundedReceiver<atrium_org::VaultEvent>,
    window: &AtriumWindow,
) {
    let win_weak = window.downgrade();
    glib::MainContext::default().spawn_local(async move {
        while let Some(event) = rx.recv().await {
            let Some(win) = win_weak.upgrade() else {
                tracing::info!("window dropped; vault-event bridge exiting");
                break;
            };
            match event {
                atrium_org::VaultEvent::ConflictBackup { source, backup } => {
                    let src_name = source
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("vault file");
                    let bak_name = backup
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("backup");
                    win.show_toast(&format!(
                        "Vault edit conflict on {src_name} — preserved as {bak_name}"
                    ));
                }
                atrium_org::VaultEvent::ParseFailed { source, error } => {
                    let src_name = source
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("vault file");
                    win.show_toast(&format!("Could not parse {src_name}: {error}"));
                }
            }
        }
        tracing::info!("vault-event channel closed; bridge exiting");
    });
}

fn install_actions(app: &adw::Application, debug: bool) {
    install_quit_action(app);
    install_about_action(app);
    install_mode_action(app);
    install_new_task_action(app);
    install_new_area_action(app);
    install_new_project_action(app);
    install_new_tag_action(app);
    install_quick_entry_action(app);
    install_search_action(app);
    install_show_list_action(app);
    install_show_shortcuts_action(app);
    if debug {
        install_fixture_action(app);
        install_memory_watch_action(app);
    }
}

fn install_memory_watch_action(app: &adw::Application) {
    let action = gio::SimpleAction::new("show-memory-watch", None);
    action.connect_activate(clone!(
        #[weak]
        app,
        move |_, _| {
            let Some(window) = app.active_window() else {
                return;
            };
            crate::debug::open_memory_watch(&window);
        }
    ));
    app.add_action(&action);
}

fn install_quick_entry_action(app: &adw::Application) {
    let action = gio::SimpleAction::new("quick-entry", None);
    action.connect_activate(clone!(
        #[weak]
        app,
        move |_, _| {
            let Some(window) = app.active_window() else {
                return;
            };
            let worker = window
                .clone()
                .downcast::<AtriumWindow>()
                .ok()
                .and_then(|w| w.worker_handle_for_quickentry());
            crate::quickentry::modal::open(&window, worker);
        }
    ));
    app.add_action(&action);
}

/// Map every keyboard shortcut from `docs/keymap.md` to its action.
/// Centralised here so the table is inspectable from one place.
fn install_accels(app: &adw::Application) {
    // App-level
    app.set_accels_for_action("app.quit", &["<Primary>q"]);
    app.set_accels_for_action("app.new-task", &["<Primary>n"]);
    app.set_accels_for_action("app.show-shortcuts", &["<Primary>question", "F1"]);
    // Quick Entry — in-app accelerator. Phase 20 adds the OS-global
    // listener via `atriumd`; for now the shortcut only fires while
    // Atrium is the focused application.
    app.set_accels_for_action("app.quick-entry", &["<Primary><Alt>space"]);
    // Phase 7a — Ctrl+F opens the search bar.
    app.set_accels_for_action("app.search", &["<Primary>f"]);

    // List navigation: Ctrl+1..6 jump to the six Simple Mode lists.
    app.set_accels_for_action("app.show-list::inbox", &["<Primary>1"]);
    app.set_accels_for_action("app.show-list::today", &["<Primary>2"]);
    app.set_accels_for_action("app.show-list::upcoming", &["<Primary>3"]);
    app.set_accels_for_action("app.show-list::anytime", &["<Primary>4"]);
    app.set_accels_for_action("app.show-list::someday", &["<Primary>5"]);
    app.set_accels_for_action("app.show-list::logbook", &["<Primary>6"]);

    // Library management (Phase 5b / 6a).
    app.set_accels_for_action("app.new-area", &["<Primary><Shift>a"]);
    app.set_accels_for_action("app.new-project", &["<Primary><Shift>n"]);
    app.set_accels_for_action("app.new-tag", &["<Primary><Shift>t"]);
    app.set_accels_for_action("win.rename-active", &["F2"]);
    app.set_accels_for_action("win.delete-active", &["<Primary><Shift>Delete"]);

    // Phase 7h — `win.delete-task`, `win.toggle-complete`,
    // `win.select-all`, and (v0.0.37) `win.bulk-clear` are bound
    // by a `gtk::ShortcutController` scoped to the task list
    // widget in `AtriumWindow::init_list_view`. Window-global
    // accels for these chords ate keystrokes that text entries
    // needed (typing a space ran toggle-complete; Esc cleared the
    // selection from inside the bottom-of-list new-task entry).
    // List scope (`ShortcutScope::Managed`) fires only when the
    // task list itself or a descendant row has focus — the right
    // thing for keyboard ops on the list, leaving every entry
    // free to handle its own keystrokes.

    // Phase 7e — focus the sidebar filter entry.
    app.set_accels_for_action("win.focus-sidebar-filter", &["<Primary>l"]);

    // Phase 7f — undo last toggle / delete (operates on the active toast cell).
    app.set_accels_for_action("win.undo", &["<Primary>z"]);

    // Phase 7g — Ctrl+T edits tags for the focused / first-selected task.
    app.set_accels_for_action("win.edit-tags-focused", &["<Primary>t"]);

    // Phase 7i — Ctrl+I (or double-click on a row) opens the Inspector.
    app.set_accels_for_action("win.edit-details-focused", &["<Primary>i"]);
}

fn install_quit_action(app: &adw::Application) {
    let action = gio::SimpleAction::new("quit", None);
    action.connect_activate(clone!(
        #[weak]
        app,
        move |_, _| app.quit()
    ));
    app.add_action(&action);
}

fn install_about_action(app: &adw::Application) {
    ui::about::install_action(app);
}

fn install_mode_action(app: &adw::Application) {
    let settings = gio::Settings::new(APP_ID);
    let initial = settings.string("mode");
    let action = gio::SimpleAction::new_stateful(
        "mode",
        Some(glib::VariantTy::STRING),
        &initial.to_variant(),
    );
    // v0.1.7 — call window.apply_mode directly after writing the
    // GSetting. The window-side `connect_changed` observer is a
    // safety net (for external writes via dconf-editor or another
    // process), but it doesn't always fire reliably on the same
    // process's writes — Brandon's v0.1.6 trace caught this:
    // "mode switched mode=builder" logged, but the observer's
    // `apply_mode` callback never ran. Calling apply_mode straight
    // from the menu activation guarantees the UI rerender lands
    // synchronously with the user's click.
    action.connect_activate(clone!(
        #[strong]
        settings,
        #[weak]
        app,
        move |action, parameter| {
            let Some(target) = parameter else { return };
            let Some(value) = target.get::<String>() else {
                return;
            };
            if let Err(e) = settings.set_string("mode", &value) {
                warn!(?e, value, "could not persist mode");
                return;
            }
            action.set_state(&value.to_variant());
            info!(mode = %value, "mode switched");
            if let Some(win) = app.active_window().and_downcast::<AtriumWindow>() {
                win.apply_mode(&value);
            }
        }
    ));
    app.add_action(&action);
}

fn install_new_task_action(app: &adw::Application) {
    let action = gio::SimpleAction::new("new-task", None);
    action.connect_activate(clone!(
        #[weak]
        app,
        move |_, _| {
            if let Some(win) = app.active_window().and_downcast::<AtriumWindow>() {
                // Things-3 idiom: focus the bottom-of-list entry; the
                // user types the title and presses Enter to commit.
                win.focus_new_task_entry();
            }
        }
    ));
    app.add_action(&action);
}

fn install_new_area_action(app: &adw::Application) {
    let action = gio::SimpleAction::new("new-area", None);
    action.connect_activate(clone!(
        #[weak]
        app,
        move |_, _| {
            if let Some(win) = app.active_window().and_downcast::<AtriumWindow>() {
                win.prompt_create_area();
            }
        }
    ));
    app.add_action(&action);
}

fn install_new_tag_action(app: &adw::Application) {
    let action = gio::SimpleAction::new("new-tag", None);
    action.connect_activate(clone!(
        #[weak]
        app,
        move |_, _| {
            if let Some(win) = app.active_window().and_downcast::<AtriumWindow>() {
                win.prompt_create_tag();
            }
        }
    ));
    app.add_action(&action);
}

fn install_new_project_action(app: &adw::Application) {
    let action = gio::SimpleAction::new("new-project", None);
    action.connect_activate(clone!(
        #[weak]
        app,
        move |_, _| {
            if let Some(win) = app.active_window().and_downcast::<AtriumWindow>() {
                win.prompt_create_project();
            }
        }
    ));
    app.add_action(&action);
}

fn install_show_list_action(app: &adw::Application) {
    let action = gio::SimpleAction::new("show-list", Some(glib::VariantTy::STRING));
    action.connect_activate(clone!(
        #[weak]
        app,
        move |_, parameter| {
            let Some(target) = parameter else { return };
            let Some(name) = target.get::<String>() else {
                return;
            };
            let idx = match name.as_str() {
                "inbox" => 0,
                "today" => 1,
                "upcoming" => 2,
                "anytime" => 3,
                "someday" => 4,
                "logbook" => 5,
                _ => return,
            };
            if let Some(win) = app.active_window().and_downcast::<AtriumWindow>() {
                win.show_list_at(idx);
            }
        }
    ));
    app.add_action(&action);
}

fn install_search_action(app: &adw::Application) {
    let action = gio::SimpleAction::new("search", None);
    action.connect_activate(clone!(
        #[weak]
        app,
        move |_, _| {
            if let Some(win) = app.active_window().and_downcast::<AtriumWindow>() {
                win.focus_search();
            }
        }
    ));
    app.add_action(&action);
}

fn install_show_shortcuts_action(app: &adw::Application) {
    let action = gio::SimpleAction::new("show-shortcuts", None);
    action.connect_activate(clone!(
        #[weak]
        app,
        move |_, _| {
            let win = ui::shortcuts::build_shortcuts_window();
            if let Some(parent) = app.active_window() {
                win.set_transient_for(Some(&parent));
            }
            win.present();
        }
    ));
    app.add_action(&action);
}

fn install_fixture_action(app: &adw::Application) {
    let action = gio::SimpleAction::new("fixture", Some(glib::VariantTy::STRING));
    action.connect_activate(clone!(
        #[weak]
        app,
        move |_, parameter| {
            let Some(target) = parameter else { return };
            let Some(value) = target.get::<String>() else {
                return;
            };
            let Some(scale) = FixtureScale::parse(&value) else {
                warn!(scale = %value, "unknown fixture scale");
                return;
            };
            info!(?scale, "queuing fixture generation");
            // v0.6.15 — run the DB write off the main thread via
            // gio::spawn_blocking (so the UI doesn't freeze on a
            // ~30 ms generate at small scale, ~150 ms at medium),
            // then resume on the main thread to poke the window
            // into rebuilding. Without that refresh the sidebar
            // stays at its old contents because the worker's
            // connection cached its view before the new rows
            // landed.
            let db_path = atrium_core::db_path();
            glib::MainContext::default().spawn_local(async move {
                let result = gio::spawn_blocking(move || generate_fixtures(&db_path, scale)).await;
                match result {
                    Ok(Ok(summary)) => {
                        info!(?summary, "fixture generation complete");
                        if let Some(window) = app.active_window()
                            && let Ok(atrium_window) =
                                window.downcast::<crate::ui::window::AtriumWindow>()
                        {
                            atrium_window.rebuild_dynamic_sidebar();
                            atrium_window.refresh_active_list();
                        }
                    }
                    Ok(Err(e)) => error!(?e, "fixture generation failed"),
                    Err(e) => error!(?e, "fixture spawn_blocking panicked"),
                }
            });
        }
    ));
    app.add_action(&action);
}

fn install_gsettings_schema_dir() {
    let Some(compiled_dir) = option_env!("ATRIUM_GSCHEMA_DIR") else {
        return;
    };
    if std::env::var_os("GSETTINGS_SCHEMA_DIR").is_some() {
        return;
    }
    // SAFETY: called once, very early in main, before any thread
    // spawn or any gio call that would observe the env var.
    unsafe {
        std::env::set_var("GSETTINGS_SCHEMA_DIR", compiled_dir);
    }
}

#[derive(Debug, Clone, Default)]
struct Config {
    debug: bool,
    fixture: Option<FixtureScale>,
    version_requested: bool,
    help_requested: bool,
}

impl Config {
    fn from_args<I, S>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut cfg = Config::default();
        let mut iter = args.into_iter().skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_ref() {
                "--debug" => cfg.debug = true,
                "--fixture" => {
                    let scale_arg = iter.next().ok_or_else(|| {
                        anyhow::anyhow!("--fixture requires a scale (small|medium|large|stress)")
                    })?;
                    let scale = FixtureScale::parse(scale_arg.as_ref()).ok_or_else(|| {
                        anyhow::anyhow!(
                            "unknown fixture scale '{}' (expected small|medium|large|stress)",
                            scale_arg.as_ref()
                        )
                    })?;
                    cfg.fixture = Some(scale);
                }
                "--version" | "-V" => cfg.version_requested = true,
                "--help" | "-h" => cfg.help_requested = true,
                other => {
                    eprintln!("warning: ignoring unknown argument '{other}'");
                }
            }
        }
        Ok(cfg)
    }
}

fn print_help() {
    println!("Atrium — native GNOME task manager.");
    println!();
    println!("USAGE:");
    println!("    atrium [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    --debug                Enable in-app debug surface (spec §3.4).");
    println!("    --fixture <SCALE>      Generate fixture data and exit.");
    println!(
        "                           SCALE: small (1K) | medium (10K) | large (50K) | stress (100K)"
    );
    println!("    -V, --version          Print version and exit.");
    println!("    -h, --help             Print this help and exit.");
}

fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,atrium=debug,atrium_core=debug"));

    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .compact()
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parses_debug_flag() {
        let cfg = Config::from_args(["atrium", "--debug"]).unwrap();
        assert!(cfg.debug);
        assert!(cfg.fixture.is_none());
    }

    #[test]
    fn config_default_no_debug() {
        let cfg = Config::from_args(["atrium"]).unwrap();
        assert!(!cfg.debug);
        assert!(cfg.fixture.is_none());
        assert!(!cfg.help_requested);
        assert!(!cfg.version_requested);
    }

    #[test]
    fn config_parses_fixture_scale() {
        let cfg = Config::from_args(["atrium", "--fixture", "medium"]).unwrap();
        assert!(matches!(cfg.fixture, Some(FixtureScale::Medium)));
    }

    #[test]
    fn config_rejects_invalid_fixture_scale() {
        let result = Config::from_args(["atrium", "--fixture", "huge"]);
        assert!(result.is_err());
    }

    #[test]
    fn config_rejects_fixture_without_scale() {
        let result = Config::from_args(["atrium", "--fixture"]);
        assert!(result.is_err());
    }

    #[test]
    fn config_help_and_version_flags() {
        let cfg = Config::from_args(["atrium", "--help"]).unwrap();
        assert!(cfg.help_requested);
        let cfg = Config::from_args(["atrium", "-V"]).unwrap();
        assert!(cfg.version_requested);
    }
}
