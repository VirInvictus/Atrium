// SPDX-License-Identifier: MIT
//! v0.20.0 — Phase 19.5 system-notifications reminder service.
//! v0.41.0 — catch-up for missed reminders.
//! v0.73.0 — timer fix: the loop now sleeps on GLib timeout futures.
//!
//! Single loop, spawned on the GLib MainContext, that polls
//! `next_pending_reminder` and sleeps until the soonest reminder
//! fires. Wake-up sources:
//!
//! - **The sleep timer expires.** Fire `gio::Notification` for the
//!   task, record the fire (`mark_reminder_fired`), re-query.
//! - **A `Notify` ping arrives.** TaskChanges set / cleared a
//!   reminder, or the master toggle flipped — re-query.
//!
//! **Timers are `glib::timeout_future`, never `tokio::time::sleep`.**
//! The future is polled by the main context, outside any tokio
//! runtime, and `tokio::time::sleep` panics there ("must be called
//! from the context of a Tokio 1.x runtime"). Until the v0.73.0 fix
//! that panic killed the loop on its first timer branch — the
//! main context swallows the unwind — so reminders never fired. The
//! `WorkerHandle` calls stay awaitable off-runtime on purpose: mpsc
//! and oneshot need no reactor. If you find yourself reaching for a
//! tokio timer in this file, that's the bug coming back.
//!
//! As of v0.41.0 the query returns the soonest **unfired** reminder
//! whether it is in the past or the future. A reminder that came due
//! while Atrium was closed (or while the master toggle was off) is
//! therefore fired on the next launch / re-enable (catch-up), and the
//! `task_reminder_fired` side table stops it from re-firing on every
//! poll. Firing records `mark_reminder_fired` *only when it actually
//! fires*, so disabling notifications no longer permanently swallows a
//! reminder — it stays unrecorded and catches up when re-enabled.
//!
//! As of v0.73.0 read failures are no longer mistaken for "nothing
//! pending": a failed pool read logs a warning and backs off briefly
//! instead of parking the loop until the next change ping.
//!
//! The service is the GUI's reminder owner. It only runs while Atrium
//! is open — the daemon (`atriumd`, Phase 20) will own out-of-process
//! reminders later.
//!
//! Notifications open the inspector via `app.show-task::ID` (a
//! parameterised action installed alongside the existing action set in
//! main.rs). GSettings `notifications-enabled` (the master switch in
//! the preferences window) gates the fire; the service watches the key
//! and wakes when it flips, so toggling on catches up immediately.

use std::sync::Arc;
use std::time::Duration;

use atrium_core::APP_ID;
use atrium_core::WorkerHandle;
use atrium_core::db::read_pool::ReadPool;
use chrono::Utc;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use tokio::sync::Notify;
use tracing::{trace, warn};

use crate::i18n::gettext;

/// One hour. The sleep cap (defensive against clock jumps / suspend)
/// and the idle re-check interval when the master toggle is off.
const MAX_SLEEP_SECS: u64 = 3600;

/// Backoff after a failed DB read or a failed fire-recording write.
/// Short enough that catch-up reminders aren't delayed noticeably,
/// long enough that a persistently broken store doesn't spam.
const ERROR_BACKOFF_SECS: u64 = 60;

/// Fires a reminder: `fire(task_id, task_title)`. Production wires
/// this to `fire_notification`; tests substitute a spy so the suite
/// doesn't need a registered `gio::Application`. The loop runs on the
/// main context only, so the callback is neither Send nor Sync.
pub(crate) type FireFn = Box<dyn Fn(i64, &str)>;

/// Public handle on the reminder service. Cloning is cheap; every
/// clone shares the underlying `Notify`. The window's TaskChanges
/// bridge calls `wake()` after each batch so the service re-queries.
#[derive(Clone)]
pub struct ReminderService {
    notify: Arc<Notify>,
}

impl ReminderService {
    /// Wake the service immediately — it'll re-query the next pending
    /// reminder. Called after every TaskChanges so a freshly-set
    /// reminder takes effect without a timer wait.
    pub fn wake(&self) {
        self.notify.notify_one();
    }
}

/// Spawn the reminder service on the GLib MainContext. Returns a
/// handle the window holds for the lifetime of the app; drop = the
/// loop's next iteration sees `notify` ref-dropped and exits cleanly.
pub fn spawn(pool: ReadPool, worker: WorkerHandle, app: gio::Application) -> ReminderService {
    let fire: FireFn = Box::new(move |task_id, title| fire_notification(&app, task_id, title));
    let settings = gio::Settings::new(APP_ID);
    spawn_with_on(&glib::MainContext::default(), pool, worker, fire, settings)
}

/// Test/alternate entry point: same loop with an explicit main
/// context, an injected fire callback, and injected settings. `spawn`
/// is this plus the gio notification closure and the default context.
/// Tests pass a private context so they don't fight the GTK tests
/// over ownership of the default one.
pub(crate) fn spawn_with_on(
    ctx: &glib::MainContext,
    pool: ReadPool,
    worker: WorkerHandle,
    fire: FireFn,
    settings: gio::Settings,
) -> ReminderService {
    let notify = Arc::new(Notify::new());
    let notify_for_loop = notify.clone();

    ctx.spawn_local(async move {
        run(pool, worker, fire, settings, notify_for_loop).await;
    });

    ReminderService { notify }
}

async fn run(
    pool: ReadPool,
    worker: WorkerHandle,
    fire: FireFn,
    settings: gio::Settings,
    notify: Arc<Notify>,
) {
    // Wake the loop when the master toggle flips, so turning
    // notifications back on catches up any reminder that came due while
    // it was off (without waiting for the hourly re-check).
    let notify_for_settings = notify.clone();
    settings.connect_changed(Some("notifications-enabled"), move |_, _| {
        notify_for_settings.notify_one();
    });

    loop {
        let next = match pool.with(atrium_core::db::read::next_pending_reminder) {
            Ok(next) => next,
            Err(e) => {
                // Can't tell pending from empty — don't park the loop
                // on the notify (nothing may ever wake it); back off
                // and retry.
                warn!(?e, "reminder: pending query failed; backing off");
                sleep_or_wake(&notify, Duration::from_secs(ERROR_BACKOFF_SECS)).await;
                continue;
            }
        };
        let Some((task_id, when)) = next else {
            // Nothing pending — sleep on the notify; wake on change.
            notify.notified().await;
            continue;
        };

        // Master switch off — don't fire and don't record it (so it
        // catches up when re-enabled). Wait for a change or an hourly
        // re-check rather than spinning on the overdue reminder.
        if !settings.boolean("notifications-enabled") {
            sleep_or_wake(&notify, Duration::from_secs(MAX_SLEEP_SECS)).await;
            continue;
        }

        let delta = when.signed_duration_since(Utc::now());
        if delta.num_seconds() > 0 {
            // Future reminder — sleep until it (capped for clock jumps),
            // or wake early on a change and re-query.
            let secs = delta.num_seconds().clamp(1, MAX_SLEEP_SECS as i64) as u64;
            trace!(
                task_id,
                sleep_seconds = secs,
                "reminder: sleeping until due"
            );
            sleep_or_wake(&notify, Duration::from_secs(secs)).await;
            if Utc::now() < when {
                continue; // woke early (notify raced)
            }
        } else {
            trace!(task_id, "reminder: overdue, firing on catch-up");
        }

        // At/past `when` (it was overdue, or we slept to it). Re-check
        // the world before firing: time, toggle, task still open.
        if !settings.boolean("notifications-enabled") {
            continue; // toggled off during the sleep — catch up later
        }
        match pool.with(|conn| atrium_core::db::read::task_by_id(conn, task_id)) {
            Ok(Some(task)) if task.completed_at.is_none() => {
                fire(task_id, &task.title);
                // Record the fire BEFORE the next re-query so the same
                // reminder isn't returned (and re-fired). Await it to
                // order the write ahead of the read.
                if let Err(e) = worker.mark_reminder_fired(task_id, when).await {
                    warn!(?e, task_id, "reminder: mark_reminder_fired failed");
                    // Couldn't record it — back off briefly rather than
                    // tight-loop re-firing the same reminder.
                    sleep_or_wake(&notify, Duration::from_secs(ERROR_BACKOFF_SECS)).await;
                }
            }
            Ok(Some(_)) => {
                // Completed while we waited: the query excludes
                // completed tasks, so the next re-query skips it.
                // Nothing to record.
            }
            Ok(None) => {
                // Deleted while we waited: likewise gone from the
                // next re-query. Nothing to record.
            }
            Err(e) => {
                // Can't confirm the task is still open — don't fire,
                // and don't tight-loop on the same reminder.
                warn!(?e, task_id, "reminder: task read failed; backing off");
                sleep_or_wake(&notify, Duration::from_secs(ERROR_BACKOFF_SECS)).await;
            }
        }
    }
}

/// Sleep for `d` on a GLib timeout source, waking early when the
/// service is pinged. This is THE timer primitive for this loop —
/// see the module doc for why tokio timers are forbidden here.
async fn sleep_or_wake(notify: &Notify, d: Duration) {
    let timeout = glib::timeout_future(d);
    let notified = notify.notified();
    tokio::select! {
        _ = timeout => {}
        _ = notified => {}
    }
}

fn fire_notification(app: &gio::Application, task_id: i64, title: &str) {
    // Translators: heading of a task-reminder system notification.
    let notification = gio::Notification::new(&gettext("Reminder"));
    notification.set_body(Some(title));
    notification.set_default_action_and_target_value("app.show-task", Some(&task_id.to_variant()));
    // Use the task id as the notification id so a new reminder for the
    // same task replaces the previous one rather than stacking.
    let id = format!("atrium-reminder-{task_id}");
    app.send_notification(Some(&id), &notification);
    trace!(task_id, "reminder fired");
    if app.is_remote() {
        warn!("reminder fired on remote application instance — notification may not surface");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for the v0.73.0 timer fix. Before it, the loop
    /// slept on `tokio::time::sleep` while polled by the main context;
    /// the first timer branch panicked off-runtime, the main context
    /// swallowed the unwind, and the loop died — reminders never
    /// fired. This drives the REAL loop (real pool, real worker, real
    /// GLib timers) with a FUTURE reminder so the sleep branch is the
    /// code under test, and a spy in place of the gio notification (a
    /// registered `gio::Application` can't exist in the test harness).
    /// The reminder must fire exactly once and be recorded in
    /// `task_reminder_fired` (the pending query then returns nothing).
    #[test]
    fn future_reminder_fires_once_through_the_real_loop() {
        // Pin the schema dir to the copy build.rs compiled for THIS
        // build. `install_gsettings_schema_dir` respects an inherited
        // GSETTINGS_SCHEMA_DIR, and agent shells can carry one from an
        // unrelated environment; the test needs the fresh schema with
        // the current keys, so it overrides unconditionally.
        // SAFETY: test-only process; set before any GSettings call.
        unsafe {
            std::env::set_var("GSETTINGS_SCHEMA_DIR", env!("ATRIUM_GSCHEMA_DIR"));
            // Isolate GSettings from dconf: the in-memory backend keeps
            // the notifications-enabled toggle in-process. No other
            // test in this binary constructs Settings, so the
            // process-wide env switch can't leak on anyone.
            std::env::set_var("GSETTINGS_BACKEND", "memory");
        }

        let db_path =
            std::env::temp_dir().join(format!("atrium-reminder-test-{}.db", std::process::id()));
        for p in [
            db_path.clone(),
            db_path.with_extension("db-wal"),
            db_path.with_extension("db-shm"),
        ] {
            let _ = std::fs::remove_file(&p);
        }

        let conn = atrium_core::db::open(&db_path).expect("open test db");
        let pool = ReadPool::new(db_path.clone(), 2);

        // The worker task needs a runtime to live on; the loop's awaits
        // (mpsc + oneshot) deliberately don't.
        let _enter = crate::runtime().handle().enter();
        let (worker, _task_rx, _lib_rx) = atrium_core::spawn_worker(conn);

        // A reminder just over a second out: delta.num_seconds() > 0
        // is what routes through the sleep branch — the branch that
        // used to panic off-runtime — so that is the code under test.
        let due = Utc::now() + chrono::Duration::milliseconds(1500);
        let task = crate::runtime()
            .block_on(worker.create_task(atrium_core::NewTask {
                title: "Reminder regression".to_string(),
                reminder_at: Some(due),
                ..Default::default()
            }))
            .expect("create reminder task");

        let fired: Arc<std::sync::Mutex<Vec<(i64, String)>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let fired_for_assert = fired.clone();
        let fire: FireFn =
            Box::new(move |id, title| fired.lock().unwrap().push((id, title.to_string())));

        let settings = gio::Settings::new(APP_ID);
        settings.set_boolean("notifications-enabled", true).unwrap();

        // A private main context: GTK-touching tests in this binary
        // acquire the default context on their own threads, and
        // `spawn_local` refuses to run where the context is owned
        // elsewhere. The test thread owns this one end to end, and
        // every timer below (and `timeout_future` inside the loop)
        // resolves on it. Timer sources are spawned futures rather
        // than `timeout_add_local` because that helper pins to the
        // PROCESS-default context and would fight the GTK tests (or
        // never fire on this private one).
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            let _service = spawn_with_on(&ctx, pool.clone(), worker.clone(), fire, settings);

            // Run the context until the spy records the fire, with a
            // watchdog so a silent loop (the old bug) fails the test
            // instead of hanging the suite.
            let main_loop = glib::MainLoop::new(Some(&ctx), false);
            let fired_for_poll = fired_for_assert.clone();
            ctx.spawn_local({
                let main_loop = main_loop.clone();
                async move {
                    loop {
                        glib::timeout_future(Duration::from_millis(20)).await;
                        if !fired_for_poll.lock().unwrap().is_empty() {
                            main_loop.quit();
                            return;
                        }
                    }
                }
            });
            ctx.spawn_local({
                let main_loop = main_loop.clone();
                async move {
                    glib::timeout_future(Duration::from_secs(10)).await;
                    main_loop.quit();
                }
            });
            main_loop.run();

            assert_eq!(
                *fired_for_assert.lock().unwrap(),
                vec![(task.id, "Reminder regression".to_string())],
                "the reminder loop must fire exactly the spawned reminder"
            );

            // The fire must be recorded, or the loop would re-fire the
            // same reminder on its next re-query. Run the context a
            // while longer and assert neither happens.
            let quiet_loop = glib::MainLoop::new(Some(&ctx), false);
            ctx.spawn_local({
                let quiet_loop = quiet_loop.clone();
                async move {
                    glib::timeout_future(Duration::from_millis(500)).await;
                    quiet_loop.quit();
                }
            });
            quiet_loop.run();

            assert_eq!(
                fired_for_assert.lock().unwrap().len(),
                1,
                "a recorded fire must not repeat"
            );
            let pending = pool
                .with(atrium_core::db::read::next_pending_reminder)
                .expect("pending query");
            assert_eq!(pending, None, "fired reminder must be recorded");
        })
        .expect("acquire test main context");

        for p in [
            db_path.clone(),
            db_path.with_extension("db-wal"),
            db_path.with_extension("db-shm"),
        ] {
            let _ = std::fs::remove_file(&p);
        }
    }
}
