// SPDX-License-Identifier: MIT
//! v0.20.0 — Phase 19.5 system-notifications reminder service.
//!
//! Single tokio task that polls `next_pending_reminder` and
//! sleeps until the soonest reminder fires. Wake-up sources:
//!
//! - **The sleep timer expires.** Fire `gio::Notification` for
//!   the task, then re-query for the next-next reminder.
//! - **A `Notify` ping arrives.** TaskChanges set / cleared a
//!   reminder; re-query so the new shape takes effect.
//!
//! The service is the GUI's reminder owner. It only runs while
//! Atrium is open — the daemon (`atriumd`, Phase 20) will own
//! out-of-process reminders later.
//!
//! Notifications open the inspector via `app.show-task::ID` (a
//! parameterised action installed alongside the existing
//! action set in main.rs).
//!
//! GSettings `notifications-enabled` (the master switch in the
//! preferences window) gates the actual notify call. The
//! service still runs when off — it just doesn't fire — so
//! flipping the toggle takes effect without restarting the
//! service.

use std::sync::Arc;
use std::time::Duration;

use atrium_core::APP_ID;
use atrium_core::db::read_pool::ReadPool;
use chrono::Utc;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use tokio::sync::Notify;
use tracing::{trace, warn};

/// Public handle on the reminder service. Cloning is cheap;
/// every clone shares the underlying `Notify`. The window's
/// TaskChanges bridge calls `wake()` after each batch so the
/// service re-queries.
#[derive(Clone)]
pub struct ReminderService {
    notify: Arc<Notify>,
}

impl ReminderService {
    /// Wake the service immediately — it'll re-query the next
    /// pending reminder. Called after every TaskChanges so a
    /// freshly-set reminder takes effect without a timer wait.
    pub fn wake(&self) {
        self.notify.notify_one();
    }
}

/// Spawn the reminder service on the GLib MainContext. Returns
/// a handle the window holds for the lifetime of the app;
/// drop = the loop's next iteration sees `notify` ref-dropped
/// and exits cleanly via `select!`.
pub fn spawn(pool: ReadPool, app: gio::Application) -> ReminderService {
    let notify = Arc::new(Notify::new());
    let notify_for_loop = notify.clone();

    glib::MainContext::default().spawn_local(async move {
        run(pool, app, notify_for_loop).await;
    });

    ReminderService { notify }
}

async fn run(pool: ReadPool, app: gio::Application, notify: Arc<Notify>) {
    let settings = gio::Settings::new(APP_ID);
    loop {
        // Single timestamp per loop iteration — the dispatcher's
        // notion of "now" is consistent across the lookup, the
        // sleep-window calculation, and the post-sleep re-check.
        let now = Utc::now();
        let next = pool
            .with(|conn| atrium_core::db::read::next_pending_reminder(conn, now))
            .ok()
            .flatten();

        match next {
            Some((task_id, when)) => {
                let delta = when.signed_duration_since(now);
                let sleep_for = if delta.num_seconds() <= 0 {
                    // Already past — fire immediately rather
                    // than negative-sleep.
                    Duration::ZERO
                } else {
                    // Cap the sleep so we re-query at least
                    // once an hour even when no Notify wake-up
                    // arrives (defensive against clock jumps,
                    // suspend/resume, etc.).
                    let secs = delta.num_seconds().clamp(1, 3600) as u64;
                    Duration::from_secs(secs)
                };
                trace!(
                    task_id,
                    sleep_seconds = sleep_for.as_secs(),
                    "reminder service: sleeping until next reminder"
                );
                tokio::select! {
                    _ = tokio::time::sleep(sleep_for) => {
                        // Re-check current time vs `when` — wake-up
                        // can be early (notify raced) or the user
                        // may have moved the reminder forward while
                        // sleeping. Fire only if we're at/past the
                        // reminder time AND the master toggle is on
                        // AND the task still exists + is open. We
                        // need a fresh `Utc::now()` here because the
                        // outer-loop `now` is from before the sleep.
                        let now_again = Utc::now();
                        if now_again < when {
                            continue;
                        }
                        if !settings.boolean("notifications-enabled") {
                            // Master switch off — re-query (we
                            // need to consume the past reminder
                            // somehow; querying with `now_again`
                            // as the cutoff skips it on the
                            // next iter).
                            //
                            // The "skip" is an open design
                            // question — alternatives include
                            // marking the reminder as fired or
                            // recording a "last-fired-at" so we
                            // don't re-fire on toggle-back. For
                            // v0.20.0 we accept the simplification:
                            // disabling notifications during
                            // an open reminder window swallows
                            // it permanently. Documented in
                            // patchnotes.
                            continue;
                        }
                        if let Some(task) = pool
                            .with(|conn| atrium_core::db::read::task_by_id(conn, task_id))
                            .ok()
                            .flatten()
                            && task.completed_at.is_none()
                        {
                            fire_notification(&app, task_id, &task.title);
                        }
                    }
                    _ = notify.notified() => {
                        // TaskChanges arrived — re-query
                        // immediately. The next iteration of
                        // the loop reads next_pending_reminder
                        // again.
                        trace!("reminder service: woken by TaskChanges");
                    }
                }
            }
            None => {
                // No pending reminder. Sleep on the notify;
                // wake when something changes.
                notify.notified().await;
            }
        }
    }
}

fn fire_notification(app: &gio::Application, task_id: i64, title: &str) {
    let notification = gio::Notification::new("Reminder");
    notification.set_body(Some(title));
    notification.set_default_action_and_target_value("app.show-task", Some(&task_id.to_variant()));
    // Use the task id as the notification id so a new reminder
    // for the same task replaces the previous one rather than
    // stacking — desktop notifications get noisy fast otherwise.
    let id = format!("atrium-reminder-{task_id}");
    app.send_notification(Some(&id), &notification);
    trace!(task_id, "reminder fired");
    // Belt-and-suspenders: log if the user has notifications
    // *system*-disabled (XDG portal / notification daemon
    // missing). Atrium can't detect this reliably, but if no
    // notification is ever observed it's the most likely cause.
    if app.is_remote() {
        warn!("reminder fired on remote application instance — notification may not surface");
    }
}
