use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;

use super::Lightbox;

const HIDE_DELAY: Duration = Duration::from_secs(3);

pub(super) struct Chrome {
    hover: gtk::EventControllerMotion,
    pointer: Cell<(f64, f64)>,
    hide_at: Cell<Instant>,
    armed: Cell<bool>,
}

impl Chrome {
    pub(super) fn new() -> Self {
        Self {
            hover: gtk::EventControllerMotion::new(),
            pointer: Cell::new((0.0, 0.0)),
            hide_at: Cell::new(Instant::now()),
            armed: Cell::new(false),
        }
    }
}

impl Lightbox {
    pub(super) fn connect_fullscreen(self: &Rc<Self>) {
        let lb = Rc::downgrade(self);
        let handler = self.ui.window.connect_fullscreened_notify(move |window| {
            if let Some(lb) = lb.upgrade() {
                lb.apply_fullscreen(window.is_fullscreen());
            }
        });
        *self.fullscreen_handler.borrow_mut() = Some(handler);
        let lb = Rc::downgrade(self);
        self.header.fullscreen_btn.connect_clicked(move |_| {
            if let Some(lb) = lb.upgrade() {
                lb.toggle_fullscreen();
            }
        });
        self.header.bar.add_controller(self.chrome.hover.clone());
        self.connect_chrome_reveal();
    }

    pub(super) fn toggle_fullscreen(&self) {
        let window = &self.ui.window;
        window.set_fullscreened(!window.is_fullscreen());
    }

    pub(super) fn close_fullscreen(&self) {
        if let Some(handler) = self.fullscreen_handler.take() {
            self.ui.window.disconnect(handler);
        }
        if self.ui.window.is_fullscreen() {
            self.ui.window.unfullscreen();
        }
    }

    fn apply_fullscreen(&self, fullscreen: bool) {
        let toolbar = &self.header.toolbar;
        toolbar.set_extend_content_to_top_edge(fullscreen);
        toolbar.set_reveal_top_bars(!fullscreen);
        self.actions.bar.set_visible(!fullscreen);
        super::set_viewer_margins(&self.viewer, fullscreen);
        if fullscreen {
            self.viewer.add_css_class("mimick-fullscreen");
            self.header.details_btn.set_active(false);
        } else {
            self.viewer.remove_css_class("mimick-fullscreen");
        }
        let (icon, tooltip) = if fullscreen {
            ("view-restore-symbolic", "Exit fullscreen (F11)")
        } else {
            ("view-fullscreen-symbolic", "Fullscreen (F11)")
        };
        self.header.fullscreen_btn.set_icon_name(icon);
        self.header.fullscreen_btn.set_tooltip_text(Some(tooltip));
        if let Some(player) = self.video.borrow().as_ref() {
            player.set_fullscreen(fullscreen);
        }
    }

    fn connect_chrome_reveal(self: &Rc<Self>) {
        let motion = gtk::EventControllerMotion::new();
        let lb = Rc::downgrade(self);
        motion.connect_motion(move |_, x, y| {
            if let Some(lb) = lb.upgrade()
                && lb.chrome.pointer.replace((x, y)) != (x, y)
            {
                lb.reveal_chrome();
            }
        });
        self.picture.overlay.add_controller(motion);
        let tap = gtk::GestureClick::new();
        let lb = Rc::downgrade(self);
        tap.connect_pressed(move |_, n_press, _, _| {
            if n_press == 1
                && let Some(lb) = lb.upgrade()
            {
                lb.reveal_chrome();
            }
        });
        self.picture.overlay.add_controller(tap);
    }

    fn reveal_chrome(self: &Rc<Self>) {
        if !self.ui.window.is_fullscreen() {
            return;
        }
        self.header.toolbar.set_reveal_top_bars(true);
        self.chrome.hide_at.set(Instant::now() + HIDE_DELAY);
        if !self.chrome.armed.replace(true) {
            self.arm_chrome_hide(HIDE_DELAY);
        }
    }

    fn arm_chrome_hide(self: &Rc<Self>, delay: Duration) {
        let lb = Rc::downgrade(self);
        glib::timeout_add_local_once(delay, move || {
            let Some(lb) = lb.upgrade() else {
                return;
            };
            let remaining = lb
                .chrome
                .hide_at
                .get()
                .saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                lb.arm_chrome_hide(remaining);
                return;
            }
            lb.chrome.armed.set(false);
            if lb.ui.window.is_fullscreen() && !lb.chrome.hover.contains_pointer() {
                lb.header.toolbar.set_reveal_top_bars(false);
            }
        });
    }
}
