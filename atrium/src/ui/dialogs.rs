// SPDX-License-Identifier: MIT
//! Owned modal alert — the `adw::AlertDialog` replacement (Phase 22 C4).
//!
//! The surface mirrors adwaita's (heading / body, an optional extra child,
//! named responses with per-response appearance, a default response for
//! Enter and a close response for Escape / the WM close) so the call sites
//! converted mechanically, including the async [`Alert::choose_future`] that
//! every Atrium dialog uses.
//!
//! Stock `gtk::AlertDialog` was not enough: it has no extra child (the
//! tag-colour swatch picker and the perspective Name+Filter form need one),
//! no per-response styling, and index-addressed buttons. So this is a small
//! hand-rolled modal `gtk::Window`, cribbed from Conservatory's Phase 26
//! version with the async chooser added.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

#[derive(Clone, Copy, PartialEq)]
pub enum Appearance {
    Suggested,
    Destructive,
}

fn appearance_class(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::Suggested => "suggested-action",
        Appearance::Destructive => "destructive-action",
    }
}

type ResponseHandler = Box<dyn Fn(&str)>;

struct State {
    responses: RefCell<Vec<(String, gtk::Button)>>,
    handler: RefCell<Option<ResponseHandler>>,
    default_response: RefCell<Option<String>>,
    close_response: RefCell<String>,
    /// Exactly-once dispatch: a button click emits its id and closes; the
    /// window's close path (Escape, the WM close button, `close()`) emits the
    /// close response only if nothing was emitted yet.
    responded: Cell<bool>,
}

impl State {
    fn emit(&self, id: &str) {
        if self.responded.replace(true) {
            return;
        }
        if let Some(handler) = self.handler.borrow().as_ref() {
            handler(id);
        }
    }
}

pub struct Alert {
    win: gtk::Window,
    extra_slot: gtk::Box,
    button_row: gtk::Box,
    state: Rc<State>,
}

impl Alert {
    pub fn new(heading: Option<&str>, body: Option<&str>) -> Self {
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(20)
            .margin_bottom(20)
            .margin_start(20)
            .margin_end(20)
            .build();
        if let Some(heading) = heading.filter(|h| !h.is_empty()) {
            content.append(
                &gtk::Label::builder()
                    .label(heading)
                    .wrap(true)
                    .justify(gtk::Justification::Center)
                    .css_classes(["heading"])
                    .build(),
            );
        }
        if let Some(body) = body.filter(|b| !b.is_empty()) {
            content.append(
                &gtk::Label::builder()
                    .label(body)
                    .wrap(true)
                    .justify(gtk::Justification::Center)
                    .build(),
            );
        }
        let extra_slot = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        content.append(&extra_slot);
        let button_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::End)
            .margin_top(8)
            .build();
        content.append(&button_row);

        let win = gtk::Window::builder()
            .title(heading.unwrap_or_default())
            .modal(true)
            .resizable(false)
            .default_width(360)
            .child(&content)
            .build();

        let state = Rc::new(State {
            responses: RefCell::new(Vec::new()),
            handler: RefCell::new(None),
            default_response: RefCell::new(None),
            close_response: RefCell::new("close".to_string()),
            responded: Cell::new(false),
        });

        // Escape closes; any close path that skipped the buttons (Escape, the
        // WM close button) still answers, with the close response.
        let key = gtk::EventControllerKey::new();
        let weak = win.downgrade();
        key.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk::Key::Escape {
                if let Some(win) = weak.upgrade() {
                    win.close();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        win.add_controller(key);

        let close_state = state.clone();
        win.connect_close_request(move |_| {
            let id = close_state.close_response.borrow().clone();
            close_state.emit(&id);
            glib::Propagation::Proceed
        });

        Self {
            win,
            extra_slot,
            button_row,
            state,
        }
    }

    pub fn set_extra_child(&self, child: Option<&impl IsA<gtk::Widget>>) {
        while let Some(old) = self.extra_slot.first_child() {
            self.extra_slot.remove(&old);
        }
        if let Some(child) = child {
            self.extra_slot.append(child);
        }
    }

    pub fn add_response(&self, id: &str, label: &str) {
        let button = gtk::Button::with_label(label);
        let state = self.state.clone();
        let weak = self.win.downgrade();
        let response = id.to_string();
        button.connect_clicked(move |_| {
            state.emit(&response);
            if let Some(win) = weak.upgrade() {
                win.close();
            }
        });
        self.button_row.append(&button);
        self.state
            .responses
            .borrow_mut()
            .push((id.to_string(), button));
    }

    pub fn set_response_appearance(&self, id: &str, appearance: Appearance) {
        if let Some((_, button)) = self
            .state
            .responses
            .borrow()
            .iter()
            .find(|(rid, _)| rid == id)
        {
            button.add_css_class(appearance_class(appearance));
        }
    }

    /// The response activated by Enter (via the window default widget; entries
    /// opt in with `set_activates_default(true)`).
    pub fn set_default_response(&self, id: Option<&str>) {
        *self.state.default_response.borrow_mut() = id.map(str::to_string);
    }

    /// The response emitted when the dialog is dismissed without a button
    /// (Escape, the WM close). Defaults to `"close"`, matching adwaita.
    pub fn set_close_response(&self, id: &str) {
        *self.state.close_response.borrow_mut() = id.to_string();
    }

    pub fn connect_response(&self, handler: impl Fn(&str) + 'static) {
        *self.state.handler.borrow_mut() = Some(Box::new(handler));
    }

    fn present(&self, parent: &impl IsA<gtk::Widget>) {
        let root = parent.as_ref().root().and_downcast::<gtk::Window>();
        self.win.set_transient_for(root.as_ref());
        if let Some(id) = self.state.default_response.borrow().as_deref()
            && let Some((_, button)) = self
                .state
                .responses
                .borrow()
                .iter()
                .find(|(rid, _)| rid == id)
        {
            self.win.set_default_widget(Some(button));
        }
        self.win.present();
    }

    /// Present modally and await the chosen response id (the adwaita
    /// `choose_future` analogue). Resolves when a button is clicked or the
    /// dialog is dismissed (Escape / WM close → the close response).
    pub async fn choose_future(&self, parent: &impl IsA<gtk::Widget>) -> String {
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        let slot = RefCell::new(Some(tx));
        self.connect_response(move |id| {
            if let Some(tx) = slot.borrow_mut().take() {
                let _ = tx.send(id.to_string());
            }
        });
        let fallback = self.state.close_response.borrow().clone();
        self.present(parent);
        rx.await.unwrap_or(fallback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_maps_to_the_owned_classes() {
        assert_eq!(appearance_class(Appearance::Suggested), "suggested-action");
        assert_eq!(
            appearance_class(Appearance::Destructive),
            "destructive-action"
        );
    }
}
