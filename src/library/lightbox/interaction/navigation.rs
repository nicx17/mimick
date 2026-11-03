use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use super::super::Lightbox;
use crate::library::{load_source_page, timeline};

impl Lightbox {
    pub(super) fn handle_key(
        self: &Rc<Self>,
        key: gtk::gdk::Key,
        mods: gtk::gdk::ModifierType,
    ) -> glib::Propagation {
        let ctrl = mods.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gtk::gdk::ModifierType::SHIFT_MASK);
        let handled = if ctrl {
            self.handle_ocr_key(key, true) || self.handle_zoom_key(key)
        } else {
            self.handle_video_key(key)
                || self.handle_ocr_key(key, false)
                || self.handle_edit_key(key, shift)
                || self.handle_plain_key(key)
        };
        if handled {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    }

    /// Ctrl +/-/0 zoom; false for any other key.
    fn handle_zoom_key(self: &Rc<Self>, key: gtk::gdk::Key) -> bool {
        use gtk::gdk::Key;
        match key {
            Key::plus | Key::equal | Key::KP_Add => self.zoom_by(1.2),
            Key::minus | Key::KP_Subtract => self.zoom_by(1.0 / 1.2),
            Key::_0 | Key::KP_0 => self.zoom_reset(),
            _ => return false,
        }
        true
    }

    /// 0 to 5 rate, Shift+D downloads, E opens Edit Info; false when the key does none of these.
    fn handle_edit_key(self: &Rc<Self>, key: gtk::gdk::Key, shift: bool) -> bool {
        use gtk::gdk::Key;
        match key {
            Key::D if shift => press_if_shown(&self.actions.download),
            Key::e | Key::E if self.details.edit_btn.is_visible() => {
                self.edit_current();
                true
            }
            _ => rating_digit(key).is_some_and(|stars| self.details.rating.rate(stars)),
        }
    }

    /// Keys without Ctrl: arrows, I, Delete, F11, F, A, Escape; false for any other key.
    fn handle_plain_key(self: &Rc<Self>, key: gtk::gdk::Key) -> bool {
        use gtk::gdk::Key;
        match key {
            Key::Left => self.goto_prev(),
            Key::Right => self.goto_next(),
            Key::i | Key::I => self
                .header
                .details_btn
                .set_active(!self.header.details_btn.is_active()),
            Key::Delete => self.confirm_trash_current(),
            Key::F11 => self.toggle_fullscreen(),
            Key::f | Key::F => self.toggle_current_favorite(),
            Key::a | Key::A => self.toggle_current_archive(),
            Key::Escape if self.ui.window.is_fullscreen() => self.ui.window.unfullscreen(),
            Key::Escape => {
                self.ui.nav.pop();
            }
            _ => return false,
        }
        true
    }

    /// Zoom on Ctrl+scroll; plain scrolling falls through to panning.
    pub(super) fn handle_zoom_scroll(
        self: &Rc<Self>,
        ctrl: &gtk::EventControllerScroll,
        dx: f64,
        dy: f64,
    ) -> glib::Propagation {
        let mods = ctrl.current_event_state();
        if !mods.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
            return glib::Propagation::Proceed;
        }
        let delta = if dy != 0.0 { dy } else { dx };
        if delta == 0.0 {
            return glib::Propagation::Proceed;
        }
        self.zoom_by(if delta < 0.0 { 1.1 } else { 1.0 / 1.1 });
        glib::Propagation::Stop
    }

    pub(super) fn goto_prev(self: &Rc<Self>) {
        let velocity = self.take_motion();
        let pos = self.pos.get();
        let step = if !self.step_into_unloaded_month(pos, false) && pos > 0 {
            -1
        } else {
            0
        };
        self.slide(step, velocity);
    }

    /// Step forward, fetching the next grid page first when at the end.
    pub(super) fn goto_next(self: &Rc<Self>) {
        let velocity = self.take_motion();
        let pos = self.pos.get();
        if self.step_into_unloaded_month(pos, true) {
            self.slide(0, velocity);
            return;
        }
        if pos + 1 < self.ui.grid.model.n_items() {
            self.slide(1, velocity);
            return;
        }
        self.slide(0, velocity);
        let next_request = self.ui.ctx.library_state.lock().load_next_page_if_needed();
        let Some(req) = next_request else {
            return;
        };
        self.header.next_btn.set_sensitive(false);
        self.advance_when_page_loads();
        load_source_page(self.ui.clone(), req, true);
    }

    /// Photos timeline: load the month past `pos` first, then step into it.
    fn step_into_unloaded_month(self: &Rc<Self>, pos: u32, forward: bool) -> bool {
        let lb = self.clone();
        let started = timeline::load_neighbor_then(&self.ui, pos, forward, move |inserted| {
            lb.header.prev_btn.set_sensitive(true);
            lb.header.next_btn.set_sensitive(true);
            let Some((at, added)) = inserted else {
                return;
            };
            if at <= lb.pos.get() {
                lb.pos.set(lb.pos.get() + added);
            }
            if forward {
                lb.goto_next();
            } else {
                lb.goto_prev();
            }
        });
        if started {
            let button = if forward {
                &self.header.next_btn
            } else {
                &self.header.prev_btn
            };
            button.set_sensitive(false);
        }
        started
    }

    /// Spring toward the photo `step` away, or back into place for 0; the step lands when it settles.
    pub(in crate::library::lightbox) fn slide(self: &Rc<Self>, step: i32, velocity: f64) {
        let strip = self.picture.strip.clone();
        if step == 0 && strip.progress().abs() < f64::EPSILON {
            self.show_overlays(true);
            return;
        }
        if step != 0 {
            self.leave_current();
        }
        let lb = Rc::downgrade(self);
        strip.animate_to(f64::from(step), velocity, move || {
            let Some(lb) = lb.upgrade() else {
                return;
            };
            if step != 0 {
                lb.commit(step);
            }
            lb.show_overlays(true);
        });
    }

    fn leave_current(self: &Rc<Self>) {
        if (self.zoom_level.get() - 1.0).abs() > 0.001 {
            self.zoom_reset();
        }
        self.drop_full_resolution();
        let player = self.video.borrow().clone();
        if let Some(player) = player {
            player.stop();
        }
        self.fill_neighbours();
        self.show_overlays(false);
    }

    fn take_motion(self: &Rc<Self>) -> f64 {
        let Some((to, velocity)) = self.picture.strip.stop() else {
            return 0.0;
        };
        if to.abs() > 0.5 && self.picture.strip.progress() * to > 0.0 {
            self.commit(to.round() as i32);
        }
        velocity
    }

    fn commit(self: &Rc<Self>, step: i32) {
        self.pos.set(self.pos.get().saturating_add_signed(step));
        self.picture.strip.rotate(step);
        log::debug!("Lightbox stepped {step} to position {}", self.pos.get());
        self.render();
    }

    pub(in crate::library::lightbox) fn connect_swipe_tracker(self: &Rc<Self>) {
        let tracker = self.picture.strip.tracker();
        let lb = Rc::downgrade(self);
        tracker.connect_prepare(move |_, _| {
            if let Some(lb) = lb.upgrade() {
                lb.fill_neighbours();
            }
        });
        let lb = Rc::downgrade(self);
        tracker.connect_begin_swipe(move |_| {
            if let Some(lb) = lb.upgrade() {
                lb.picture.strip.stop();
                lb.show_overlays(false);
            }
        });
        let strip = self.picture.strip.downgrade();
        tracker.connect_update_swipe(move |_, progress| {
            if let Some(strip) = strip.upgrade() {
                strip.set_progress(progress);
            }
        });
        let lb = Rc::downgrade(self);
        tracker.connect_end_swipe(move |_, velocity, to| {
            if let Some(lb) = lb.upgrade() {
                lb.slide(to.round() as i32, velocity);
            }
        });
    }

    /// Advance once the next page lands in the grid model, then drop the handler.
    fn advance_when_page_loads(self: &Rc<Self>) {
        let model = self.ui.grid.model.clone();
        let prev_count = model.n_items();
        // One-shot handler: it disconnects itself once the new page has arrived.
        let handler_id = Rc::new(RefCell::new(None::<glib::SignalHandlerId>));
        let handler_id_clone = handler_id.clone();
        let lb = Rc::downgrade(self);
        let id = model.connect_items_changed(move |m, _, _, _| {
            if m.n_items() <= prev_count {
                return;
            }
            let Some(lb) = lb.upgrade() else {
                return;
            };
            if lb.pos.get() + 1 < m.n_items() {
                lb.slide(1, 0.0);
            }
            lb.header.next_btn.set_sensitive(true);
            if let Some(hid) = handler_id_clone.borrow_mut().take() {
                m.disconnect(hid);
            }
        });
        *handler_id.borrow_mut() = Some(id);
    }
}

fn press_if_shown(button: &gtk::Button) -> bool {
    let shown = button.is_visible() && button.is_sensitive();
    if shown {
        button.emit_clicked();
    }
    shown
}

/// The rating a digit key stands for, from 0 to 5.
fn rating_digit(key: gtk::gdk::Key) -> Option<u8> {
    let digit = key.to_unicode()?.to_digit(10)?;
    u8::try_from(digit).ok().filter(|stars| *stars <= 5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::gdk::Key;

    #[test]
    fn digits_up_to_five_are_ratings() {
        assert_eq!(rating_digit(Key::_0), Some(0));
        assert_eq!(rating_digit(Key::_5), Some(5));
        assert_eq!(rating_digit(Key::KP_3), Some(3));
        assert_eq!(rating_digit(Key::_6), None);
        assert_eq!(rating_digit(Key::e), None);
    }
}
