//! The photo's menu, as a popover by the photo or a sheet from the bottom (also F10 and a
//! swipe up), and the hooks that keep the lightbox in step with what the menu changes.

use std::rc::Rc;

use gtk::prelude::*;

use super::super::Lightbox;
use crate::library::context_menu::{AssetMenuHooks, Menu, MenuTarget, item_menu};

impl Lightbox {
    /// A fast swipe up on the photo opens the ⋯ sheet. It only looks at the swipe once it
    /// ends and never claims the touch, so the sideways swipe, pinch, and long-press still work.
    pub(super) fn connect_swipe_up(self: &Rc<Self>) {
        let swipe = gtk::GestureSwipe::builder().touch_only(true).build();
        let lb = Rc::downgrade(self);
        swipe.connect_swipe(move |_, vx, vy| {
            if let Some(lb) = lb.upgrade()
                && is_swipe_up(vx, vy)
                && lb.photo_at_rest()
            {
                lb.show_menu_sheet();
            }
        });
        self.picture.scrolled.add_controller(swipe);
    }

    /// Not zoomed, editing, tagging faces, or playing a slideshow, where swipes mean other things.
    pub(in crate::library::lightbox) fn photo_at_rest(&self) -> bool {
        (self.zoom_level.get() - 1.0).abs() < 0.001
            && !self.edit_mode.is_active()
            && !self.face_mode.is_active()
            && !self.slideshow.is_running()
    }

    pub(super) fn show_menu_at(self: &Rc<Self>, x: f64, y: f64) {
        self.show_menu_on(&self.picture.scrolled, x, y);
    }

    pub(super) fn show_menu_on(self: &Rc<Self>, parent: &impl IsA<gtk::Widget>, x: f64, y: f64) {
        if let Some(menu) = self.photo_menu(parent, x, y) {
            menu.popup();
        }
    }

    /// The ⋯ button's menu (also F10 and a swipe up): every action as an icon in a sheet.
    pub(in crate::library::lightbox) fn show_menu_sheet(self: &Rc<Self>) {
        let button = &self.header.menu_btn;
        if let Some(menu) = self.photo_menu(button, 0.0, 0.0) {
            menu.present_sheet(button);
        }
    }

    fn photo_menu(self: &Rc<Self>, parent: &impl IsA<gtk::Widget>, x: f64, y: f64) -> Option<Menu> {
        let target = MenuTarget {
            item: self.current_item()?,
            position: self.pos.get(),
        };
        Some(item_menu(
            self.ui.clone(),
            parent,
            target,
            x,
            y,
            self.menu_hooks(),
        ))
    }

    /// Hooks for the right-click menu so trash/edit there update this lightbox.
    pub(super) fn menu_hooks(self: &Rc<Self>) -> AssetMenuHooks {
        let lb_trashed = self.clone();
        let lb_edited = self.clone();
        let lb_fullscreen = Rc::downgrade(self);
        AssetMenuHooks {
            on_trashed: Rc::new(move || lb_trashed.after_trash()),
            on_edited: Rc::new(move |date_changed| lb_edited.after_edit(date_changed)),
            toggle_fullscreen: Some(Rc::new(move || {
                if let Some(lb) = lb_fullscreen.upgrade() {
                    lb.toggle_fullscreen();
                }
            })),
        }
    }
}

/// Swipe speed, in pixels per second, that counts as a deliberate swipe up.
const SWIPE_UP_SPEED: f64 = 500.0;

/// Fast, upward, and mostly vertical, so a sideways swipe to the next photo never counts.
pub(in crate::library::lightbox) fn is_swipe_up(vx: f64, vy: f64) -> bool {
    vy < -SWIPE_UP_SPEED && vy.abs() > vx.abs() * 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fast_upward_swipes_open_the_sheet() {
        assert!(is_swipe_up(40.0, -900.0));
        assert!(!is_swipe_up(0.0, 900.0), "downward");
        assert!(!is_swipe_up(0.0, -200.0), "too slow");
        assert!(
            !is_swipe_up(-800.0, -700.0),
            "mostly sideways, a photo change"
        );
    }
}
