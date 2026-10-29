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
        let handled = if ctrl {
            self.handle_ocr_key(key, true) || self.handle_zoom_key(key)
        } else {
            self.handle_video_key(key)
                || self.handle_ocr_key(key, false)
                || self.handle_plain_key(key)
        };
        if handled {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    }

    /// Ctrl +/-/0 zoom; false for any other key.
    fn handle_zoom_key(&self, key: gtk::gdk::Key) -> bool {
        use gtk::gdk::Key;
        match key {
            Key::plus | Key::equal | Key::KP_Add => self.zoom_by(1.2),
            Key::minus | Key::KP_Subtract => self.zoom_by(1.0 / 1.2),
            Key::_0 | Key::KP_0 => self.zoom_reset(),
            _ => return false,
        }
        true
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
        &self,
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
        let pos = self.pos.get();
        if self.step_into_unloaded_month(pos, false) {
            return;
        }
        if pos > 0 {
            self.pos.set(pos - 1);
            self.nav_dir.set(-1);
            self.render();
        }
    }

    /// Step forward, fetching the next grid page first when at the end.
    pub(super) fn goto_next(self: &Rc<Self>) {
        let pos = self.pos.get();
        if self.step_into_unloaded_month(pos, true) {
            return;
        }
        if pos + 1 < self.ui.grid.model.n_items() {
            self.pos.set(pos + 1);
            self.nav_dir.set(1);
            self.render();
            return;
        }
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

    /// Advance once the next page lands in the grid model, then drop the handler.
    fn advance_when_page_loads(self: &Rc<Self>) {
        let model = self.ui.grid.model.clone();
        let prev_count = model.n_items();
        // One-shot handler: it disconnects itself once the new page has arrived.
        let handler_id = Rc::new(RefCell::new(None::<glib::SignalHandlerId>));
        let handler_id_clone = handler_id.clone();
        let lb = self.clone();
        let id = model.connect_items_changed(move |m, _, _, _| {
            if m.n_items() <= prev_count {
                return;
            }
            let pos = lb.pos.get();
            if pos + 1 < m.n_items() {
                lb.pos.set(pos + 1);
                lb.nav_dir.set(1);
                lb.render();
            }
            lb.header.next_btn.set_sensitive(true);
            if let Some(hid) = handler_id_clone.borrow_mut().take() {
                m.disconnect(hid);
            }
        });
        *handler_id.borrow_mut() = Some(id);
    }
}
