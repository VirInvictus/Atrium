// SPDX-License-Identifier: MIT
//! Modal alerts, now from vir-gtk's widget kit (1.4.0). `Alert`,
//! `Appearance`, and `close_on_escape` are re-exports of
//! `vir_gtk::widgets`; the historical `crate::ui::dialogs::` paths keep
//! resolving. What stays app-side is [`AlertChoose::choose_future`], the
//! async chooser every Atrium dialog awaits: it wraps a tokio oneshot
//! around the kit's `connect_response`, and lives here because the crate
//! itself carries no async runtime.

use gtk::prelude::*;
use std::cell::RefCell;

pub use vir_gtk::widgets::{Alert, Appearance, close_on_escape};

/// The async chooser, implemented for the kit's `Alert`. Import the trait
/// where `dialog.choose_future(parent).await` is called.
pub trait AlertChoose {
    /// Present modally and await the chosen response id (the adwaita
    /// `choose_future` analogue). Resolves when a button is clicked or the
    /// dialog is dismissed (Escape / WM close, which emit the close
    /// response, `"cancel"` on every Atrium alert via
    /// `set_close_response`).
    ///
    /// Atrium awaits this from main-loop tasks; keep it that way, the
    /// future is `!Send` like any future holding GObject references.
    async fn choose_future(&self, parent: &impl IsA<gtk::Widget>) -> String;
}

impl AlertChoose for Alert {
    async fn choose_future(&self, parent: &impl IsA<gtk::Widget>) -> String {
        let fallback = self.close_response();
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        let slot = RefCell::new(Some(tx));
        self.connect_response(move |id| {
            if let Some(tx) = slot.borrow_mut().take() {
                let _ = tx.send(id.to_string());
            }
        });
        self.present(Some(parent));
        rx.await.unwrap_or(fallback)
    }
}
