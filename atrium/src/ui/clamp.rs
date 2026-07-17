// SPDX-License-Identifier: MIT
//! Owned width clamp — the `AdwClamp` successor (Phase 22 tail, the
//! tiling-forward layout work). Caps its single child's width at
//! [`MAX_WIDTH`] and centres it horizontally, so the task list stays a calm
//! Things-3 column on wide tiles instead of stretching edge-to-edge.
//! Vertical sizing passes straight through.
//!
//! It wraps the list's `GtkScrolledWindow` (not the `GtkListView` inside it),
//! so the scroller still scrolls natively and the ListView keeps its row
//! virtualisation — no need to implement `GtkScrollable` and forward
//! adjustments the way `AdwClampScrollable` did.
//!
//! Used from `data/window.ui` as `<object class="AtriumClamp">` with a single
//! `<child>`; the type is registered via [`Clamp::ensure_registered`] before
//! the window template inflates.

use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

/// The Things-3-calm maximum column width. Was `AdwClamp`'s `maximum-size`
/// of 960 (spec §5 list surface; bumped there at v0.6.11).
pub const MAX_WIDTH: i32 = 960;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Clamp {
        pub child: RefCell<Option<gtk::Widget>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Clamp {
        const NAME: &'static str = "AtriumClamp";
        type Type = super::Clamp;
        type ParentType = gtk::Widget;
        type Interfaces = (gtk::Buildable,);
    }

    impl ObjectImpl for Clamp {
        fn dispose(&self) {
            if let Some(child) = self.child.borrow_mut().take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Clamp {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            self.child
                .borrow()
                .as_ref()
                .map(|c| c.request_mode())
                .unwrap_or(gtk::SizeRequestMode::ConstantSize)
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.child.borrow().clone() else {
                return (0, 0, -1, -1);
            };
            // When measuring height (vertical) for a given width, the child
            // only ever gets the capped width, so measure it at that.
            let child_for_size =
                if orientation == gtk::Orientation::Vertical && for_size > MAX_WIDTH {
                    MAX_WIDTH
                } else {
                    for_size
                };
            let (min, nat, min_baseline, nat_baseline) = child.measure(orientation, child_for_size);
            if orientation == gtk::Orientation::Horizontal {
                // Natural width caps at MAX_WIDTH; the minimum still tracks
                // the child so it can shrink below the cap on a narrow tile.
                (
                    min.min(MAX_WIDTH),
                    nat.min(MAX_WIDTH),
                    min_baseline,
                    nat_baseline,
                )
            } else {
                (min, nat, min_baseline, nat_baseline)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let Some(child) = self.child.borrow().clone() else {
                return;
            };
            let child_w = width.min(MAX_WIDTH);
            let x = ((width - child_w) / 2).max(0);
            child.size_allocate(&gtk::gdk::Rectangle::new(x, 0, child_w, height), baseline);
        }
    }

    impl BuildableImpl for Clamp {
        fn add_child(
            &self,
            builder: &gtk::Builder,
            child: &glib::Object,
            child_type: Option<&str>,
        ) {
            if let Some(widget) = child.downcast_ref::<gtk::Widget>() {
                self.obj().set_child(Some(widget));
            } else {
                self.parent_add_child(builder, child, child_type);
            }
        }
    }
}

glib::wrapper! {
    pub struct Clamp(ObjectSubclass<imp::Clamp>)
        @extends gtk::Widget,
        @implements gtk::Buildable;
}

impl Clamp {
    /// Ensure the type is registered with GObject before a `GtkBuilder`
    /// template that names `AtriumClamp` inflates. Cheap + idempotent.
    pub fn ensure_registered() {
        let _ = Self::static_type();
    }

    pub fn set_child(&self, child: Option<&impl IsA<gtk::Widget>>) {
        if let Some(old) = self.imp().child.borrow_mut().take() {
            old.unparent();
        }
        if let Some(child) = child {
            let child = child.clone().upcast::<gtk::Widget>();
            child.set_parent(self);
            self.imp().child.replace(Some(child));
        }
    }
}

impl Default for Clamp {
    fn default() -> Self {
        glib::Object::new()
    }
}
