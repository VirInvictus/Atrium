// SPDX-License-Identifier: MIT
//! v0.20.0 — Phase 19.5 system-notifications reminder service.
//! v0.41.0 — catch-up for missed reminders.
//!
//! Single tokio task that polls `next_pending_reminder` and sleeps
//! until the soonest reminder fires. Wake-up sources:
//!
//! - **The sleep timer expires.** Fire `gio::Notification` for the
//!   task, record the fire (`mark_reminder_fired`), re-query.
//! - **A `Notify` ping arrives.** TaskChanges set / cleared a
//!   reminder, or the master toggle flipped — re-query.
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

/// One hour. The sleep cap (defensive against clock jumps / suspend)
/// and the idle re-check interval when the master toggle is off.
const MAX_SLEEP_SECS: u64 = 3600;

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
    let notify = Arc::new(Notify::new());
    let notify_for_loop = notify.clone();

    glib::MainContext::default().spawn_local(async move {
        run(pool, worker, app, notify_for_loop).await;
    });

    ReminderService { notify }
}

async fn run(pool: ReadPool, worker: WorkerHandle, app: gio::Application, notify: Arc<Notify>) {
    let settings = gio::Settings::new(APP_ID);

    // Wake the loop when the master toggle flips, so turning
    // notifications back on catches up any reminder that came due while
    // it was off (without waiting for the hourly re-check).
    let notify_for_settings = notify.clone();
    settings.connect_changed(Some("notifications-enabled"), move |_, _| {
        notify_for_settings.notify_one();
    });

    loop {
        let next = pool
            .with(atrium_core::db::read::next_pending_reminder)
            .ok()
            .flatten();
        let Some((task_id, when)) = next else {
            // Nothing pending — sleep on the notify; wake on change.
            notify.notified().await;
            continue;
        };

        // Master switch off — don't fire and don't record it (so it
        // catches up when re-enabled). Wait for a change or an hourly
        // re-check rather than spinning on the overdue reminder.
        if !settings.boolean("notifications-enabled") {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(MAX_SLEEP_SECS)) => {}
                _ = notify.notified() => {}
            }
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
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(secs)) => {}
                _ = notify.notified() => { continue; }
            }
        } else {
            trace!(task_id, "reminder: overdue, firing on catch-up");
        }

        // At/past `when` (it was overdue, or we slept to it). Re-check
        // the world before firing: time, toggle, task still open.
        if Utc::now() < when {
            continue; // woke early (notify raced)
        }
        if !settings.boolean("notifications-enabled") {
            continue; // toggled off during the sleep — catch up later
        }
        match pool
            .with(|conn| atrium_core::db::read::task_by_id(conn, task_id))
            .ok()
            .flatten()
        {
            Some(task) if task.completed_at.is_none() => {
                fire_notification(&app, task_id, &task.title);
                // Record the fire BEFORE the next re-query so the same
                // reminder isn't returned (and re-fired). Await it to
                // order the write ahead of the read.
                if let Err(e) = worker.mark_reminder_fired(task_id, when).await {
                    warn!(?e, task_id, "reminder: mark_reminder_fired failed");
                    // Couldn't record it — back off briefly rather than
                    // tight-loop re-firing the same reminder.
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                        _ = notify.notified() => {}
                    }
                }
            }
            _ => {
                // Completed or deleted while we waited: the query
                // excludes completed tasks and deleted ones are gone,
                // so the next re-query skips it. Nothing to record.
            }
        }
    }
}

fn fire_notification(app: &gio::Application, task_id: i64, title: &str) {
    let notification = gio::Notification::new("Reminder");
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
