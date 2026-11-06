//! Recognized-text (OCR) overlay in the lightbox: loading boxes, mouse and touch selection, and copying.

use gettextrs::gettext;
use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;

use super::Lightbox;

mod geometry;
mod layer;
mod state;

pub(super) use layer::OcrLayer;
use state::Caret;

/// Floating button over the photo that copies the selection, or all text when nothing is selected.
pub(super) fn copy_button() -> gtk::Button {
    gtk::Button::builder()
        .label(gettext("Copy All Text"))
        .halign(gtk::Align::Center)
        .valign(gtk::Align::End)
        .margin_bottom(12)
        .css_classes(["osd", "pill"])
        .visible(false)
        .build()
}

impl Lightbox {
    /// Fetches the photo's OCR boxes; results for a photo the user has left are dropped.
    pub(super) fn load_ocr(self: &Rc<Self>, asset_id: String, image: (f64, f64)) {
        let generation = self.load_gen.get();
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = lb.ui.ctx.api_client.fetch_asset_ocr(&asset_id).await;
            if lb.load_gen.get() != generation {
                return;
            }
            match result {
                Ok(boxes) if !boxes.is_empty() => {
                    lb.picture.ocr.set_boxes(boxes, image);
                    lb.sync_ocr_controls();
                }
                Ok(_) => {}
                Err(err) => log::debug!("OCR fetch failed for {asset_id}: {err}"),
            }
        });
    }

    /// All OCR input goes through the picture scroller, since the drawing layer can't take input.
    pub(super) fn connect_ocr(self: &Rc<Self>) {
        self.connect_ocr_toggle();
        self.connect_ocr_mouse_selection();
        self.connect_ocr_taps();
        self.connect_ocr_hover();
        self.on_click(&self.picture.ocr_copy, Self::copy_ocr_text);
    }

    fn connect_ocr_toggle(self: &Rc<Self>) {
        let lb = Rc::downgrade(self);
        self.actions.ocr_toggle.connect_toggled(move |btn| {
            btn.set_tooltip_text(Some(&if btn.is_active() {
                gettext("Hide recognized text (T)")
            } else {
                gettext("Show recognized text (T)")
            }));
            let Some(lb) = lb.upgrade() else {
                return;
            };
            if !btn.is_active() {
                lb.picture.ocr.clear_selection();
            }
            lb.sync_ocr_controls();
        });
    }

    /// T, Ctrl+A and Ctrl+C; returns whether the key was handled.
    pub(super) fn handle_ocr_key(self: &Rc<Self>, key: gtk::gdk::Key, ctrl: bool) -> bool {
        use gtk::gdk::Key;
        let shown = self.picture.ocr.area.is_visible();
        match (ctrl, key) {
            (false, Key::t | Key::T) if self.actions.ocr_toggle.is_visible() => self
                .actions
                .ocr_toggle
                .set_active(!self.actions.ocr_toggle.is_active()),
            (true, Key::a | Key::A) if shown => {
                self.picture.ocr.select_all();
                self.sync_ocr_controls();
            }
            (true, Key::c | Key::C) if shown => self.copy_ocr_text(),
            _ => return false,
        }
        true
    }

    /// Mouse presses on a box select text instead of panning, swiping or dragging the photo out:
    /// a click takes the line, a drag the characters swept over, a double-click one word.
    fn connect_ocr_mouse_selection(self: &Rc<Self>) {
        let drag = gtk::GestureDrag::new();
        drag.set_button(gtk::gdk::BUTTON_PRIMARY);
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        let start = Rc::new(Cell::new(None::<(f64, f64)>));
        let last_press = Rc::new(Cell::new(None::<(i64, Caret)>));
        let begin = start.clone();
        let lb = Rc::downgrade(self);
        drag.connect_drag_begin(move |gesture, x, y| {
            begin.set(None);
            let Some(lb) = lb.upgrade() else {
                return;
            };
            let Some(caret) = (!is_touch(gesture))
                .then(|| lb.ocr_caret_under(x, y))
                .flatten()
            else {
                gesture.set_state(gtk::EventSequenceState::Denied);
                return;
            };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if lb.ocr_press(gesture, caret, &last_press) {
                begin.set(Some((x, y)));
            }
            lb.sync_ocr_controls();
        });
        let lb = Rc::downgrade(self);
        drag.connect_drag_update(move |_, dx, dy| {
            let (Some((x, y)), Some(lb)) = (start.get(), lb.upgrade()) else {
                return;
            };
            if let Some(caret) = lb.ocr_caret_under(x + dx, y + dy) {
                lb.picture.ocr.drag_to(caret);
                lb.sync_ocr_controls();
            }
        });
        self.picture.scrolled.add_controller(drag);
    }

    /// A second press on the same box soon after the first picks the word there;
    /// otherwise the press selects as `press_at` says. Returns whether a drag should follow.
    fn ocr_press(
        &self,
        gesture: &gtk::GestureDrag,
        caret: Caret,
        last_press: &Cell<Option<(i64, Caret)>>,
    ) -> bool {
        let now = glib::monotonic_time();
        let window_us = gtk::Settings::default()
            .map_or(400, |settings| settings.gtk_double_click_time())
            .max(1);
        let double = last_press.take().is_some_and(|(at, prev)| {
            prev.idx == caret.idx && now - at <= i64::from(window_us) * 1000
        });
        if double {
            self.picture.ocr.select_word(caret);
            return false;
        }
        last_press.set(Some((now, caret)));
        let mods = gesture.current_event_state();
        let shift = mods.contains(gtk::gdk::ModifierType::SHIFT_MASK);
        let ctrl = mods.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        self.picture.ocr.press_at(caret, shift, ctrl)
    }

    /// Touch taps toggle boxes; drags are left to panning, pinching and swiping.
    fn connect_ocr_taps(self: &Rc<Self>) {
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_PRIMARY);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let dragged = Rc::new(Cell::new(false));
        let pressed = dragged.clone();
        click.connect_pressed(move |_, _, _, _| pressed.set(false));
        let stopped = dragged.clone();
        click.connect_stopped(move |_| stopped.set(true));
        let lb = Rc::downgrade(self);
        click.connect_released(move |gesture, _, x, y| {
            let Some(lb) = lb.upgrade() else {
                return;
            };
            if dragged.get() || !lb.picture.ocr.area.is_visible() {
                return;
            }
            let modified = gesture.current_event_state().intersects(
                gtk::gdk::ModifierType::SHIFT_MASK | gtk::gdk::ModifierType::CONTROL_MASK,
            );
            match lb.ocr_box_under(x, y) {
                Some(idx) if is_touch(gesture) => {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    lb.picture.ocr.click(idx, true);
                }
                None if !modified => lb.picture.ocr.clear_selection(),
                _ => return,
            }
            lb.sync_ocr_controls();
        });
        self.picture.scrolled.add_controller(click);
    }

    /// Text cursor and highlight over a box, with its text as a tooltip.
    fn connect_ocr_hover(self: &Rc<Self>) {
        let scrolled = &self.picture.scrolled;
        let motion = gtk::EventControllerMotion::new();
        let lb = Rc::downgrade(self);
        motion.connect_motion(move |_, x, y| {
            if let Some(lb) = lb.upgrade() {
                lb.set_ocr_hover(lb.ocr_box_under(x, y));
            }
        });
        let lb = Rc::downgrade(self);
        motion.connect_leave(move |_| {
            if let Some(lb) = lb.upgrade() {
                lb.set_ocr_hover(None);
            }
        });
        scrolled.add_controller(motion);

        scrolled.set_has_tooltip(true);
        let lb = Rc::downgrade(self);
        scrolled.connect_query_tooltip(move |_, x, y, _, tooltip| {
            let Some(lb) = lb.upgrade() else {
                return false;
            };
            let text = lb
                .ocr_box_under(f64::from(x), f64::from(y))
                .and_then(|idx| lb.picture.ocr.text_at(idx));
            match text {
                Some(text) => {
                    tooltip.set_text(Some(&text));
                    true
                }
                None => false,
            }
        });
    }

    fn set_ocr_hover(&self, idx: Option<usize>) {
        if self.picture.ocr.set_hovered(idx) {
            self.picture
                .scrolled
                .set_cursor_from_name(idx.map(|_| "text"));
        }
    }

    /// Copies the selection, or all text when nothing is selected.
    pub(super) fn copy_ocr_text(self: &Rc<Self>) {
        let ocr = &self.picture.ocr;
        let Some(text) = ocr.selected_text().or_else(|| ocr.all_text()) else {
            return;
        };
        self.picture.scrolled.clipboard().set_text(&text);
        self.picture.ocr_copy.set_label(&gettext("Copied"));
        let lb = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
            lb.sync_ocr_controls()
        });
    }

    /// Caret under a point in picture-scroller coordinates, while the text is shown.
    fn ocr_caret_under(&self, x: f64, y: f64) -> Option<Caret> {
        let point = self.ocr_layer_point(x, y)?;
        self.picture.ocr.caret_at(point)
    }

    fn ocr_layer_point(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let layer = &self.picture.ocr.area;
        if !layer.is_visible() {
            return None;
        }
        let point = self
            .picture
            .scrolled
            .compute_point(layer, &gtk::graphene::Point::new(x as f32, y as f32))?;
        Some((f64::from(point.x()), f64::from(point.y())))
    }

    /// Box under a point in picture-scroller coordinates, while the text is shown.
    pub(super) fn ocr_box_under(&self, x: f64, y: f64) -> Option<usize> {
        let point = self.ocr_layer_point(x, y)?;
        self.picture.ocr.box_at(point)
    }

    /// Shows the toggle, layer and copy button to match the current photo's boxes.
    pub(super) fn sync_ocr_controls(&self) {
        let ocr = &self.picture.ocr;
        let has_boxes = ocr.has_boxes();
        let shown = self.actions.ocr_toggle.is_active() && has_boxes;
        self.actions.ocr_toggle.set_visible(has_boxes);
        ocr.area.set_visible(shown);
        self.picture.ocr_copy.set_visible(shown);
        self.picture.ocr_copy.set_label(&if ocr.has_selection() {
            gettext("Copy Selected Text")
        } else {
            gettext("Copy All Text")
        });
        if !shown {
            ocr.set_hovered(None);
            self.picture.scrolled.set_cursor(None::<&gtk::gdk::Cursor>);
        }
    }
}

/// Touch presses must not claim the sequence, or panning and pinch-zoom stop working.
fn is_touch(controller: &impl IsA<gtk::EventController>) -> bool {
    controller
        .current_event_device()
        .is_some_and(|device| device.source() == gtk::gdk::InputSource::Touchscreen)
}
