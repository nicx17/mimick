//! Drawing area that outlines OCR boxes over the lightbox photo.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use super::geometry::{contain_rect, quad_in_area};
use super::state::OcrState;
use crate::api_client::OcrBox;

/// Non-interactive drawing area over the picture stack; it scrolls and zooms with the photo.
pub(crate) struct OcrLayer {
    pub(crate) area: gtk::DrawingArea,
    state: Rc<RefCell<OcrState>>,
}

impl OcrLayer {
    pub(crate) fn new() -> Self {
        let area = gtk::DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .can_target(false)
            .visible(false)
            .build();
        let state = Rc::new(RefCell::new(OcrState::default()));
        let draw_state = state.clone();
        area.set_draw_func(move |_, cr, width, height| {
            draw_boxes(
                cr,
                &draw_state.borrow(),
                (f64::from(width), f64::from(height)),
            );
        });
        Self { area, state }
    }

    /// Replaces the boxes and clears the selection; `image` is the displayed texture's size.
    pub(super) fn set_boxes(&self, boxes: Vec<OcrBox>, image: (f64, f64)) {
        *self.state.borrow_mut() = OcrState::new(boxes, image);
        self.area.queue_draw();
    }

    pub(crate) fn clear(&self) {
        *self.state.borrow_mut() = OcrState::default();
        self.area.queue_draw();
    }

    pub(super) fn has_boxes(&self) -> bool {
        !self.state.borrow().boxes().is_empty()
    }

    /// Box under `p`, given in this area's coordinates.
    pub(super) fn box_at(&self, p: (f64, f64)) -> Option<usize> {
        let area = (f64::from(self.area.width()), f64::from(self.area.height()));
        self.state.borrow().box_at(area, p)
    }

    /// Returns whether the hovered box changed.
    pub(super) fn set_hovered(&self, idx: Option<usize>) -> bool {
        let changed = self.state.borrow_mut().set_hovered(idx);
        if changed {
            self.area.queue_draw();
        }
        changed
    }

    /// Mouse press on a box; returns whether dragging should extend the selection.
    pub(super) fn press(&self, idx: usize, shift: bool, ctrl: bool) -> bool {
        let extends = self.state.borrow_mut().press(idx, shift, ctrl);
        self.area.queue_draw();
        extends
    }

    pub(super) fn click(&self, idx: usize, extend: bool) {
        self.state.borrow_mut().click(idx, extend);
        self.area.queue_draw();
    }

    pub(super) fn extend_to(&self, idx: usize) {
        self.state.borrow_mut().extend_to(idx);
        self.area.queue_draw();
    }

    pub(super) fn has_selection(&self) -> bool {
        self.state.borrow().has_selection()
    }

    pub(super) fn all_text(&self) -> Option<String> {
        self.state.borrow().all_text()
    }

    pub(super) fn select_all(&self) {
        self.state.borrow_mut().select_all();
        self.area.queue_draw();
    }

    pub(super) fn clear_selection(&self) {
        self.state.borrow_mut().clear_selection();
        self.area.queue_draw();
    }

    pub(super) fn selected_text(&self) -> Option<String> {
        self.state.borrow().selected_text()
    }

    pub(super) fn text_at(&self, idx: usize) -> Option<String> {
        self.state
            .borrow()
            .boxes()
            .get(idx)
            .map(|ocr_box| ocr_box.text.clone())
    }
}

/// Outlines every box in the accent colour, filled more strongly when hovered or selected.
fn draw_boxes(cr: &gtk::cairo::Context, state: &OcrState, area: (f64, f64)) {
    let Some(rect) = contain_rect(area, state.image()) else {
        return;
    };
    let accent = libadwaita::StyleManager::default().accent_color().to_rgba();
    let (r, g, b) = (
        f64::from(accent.red()),
        f64::from(accent.green()),
        f64::from(accent.blue()),
    );
    for (idx, ocr_box) in state.boxes().iter().enumerate() {
        let quad = quad_in_area(rect, ocr_box.corners());
        cr.move_to(quad[0].0, quad[0].1);
        for &(x, y) in &quad[1..] {
            cr.line_to(x, y);
        }
        cr.close_path();
        let active = state.is_selected(idx) || state.hovered() == Some(idx);
        let (fill_alpha, line_width) = if active { (0.35, 2.5) } else { (0.12, 1.5) };
        cr.set_source_rgba(r, g, b, fill_alpha);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(r, g, b, 1.0);
        cr.set_line_width(line_width);
        let _ = cr.stroke();
    }
}
