//! Drawing area that outlines OCR boxes over the lightbox photo.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use super::geometry::{contain_rect, quad_in_area, slice_quad};
use super::state::{Caret, OcrState, even_stops};
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
        let area = self.area.clone();
        let state = OcrState::measured(boxes, image, |text| char_stops(&area, text));
        *self.state.borrow_mut() = state;
        self.area.queue_draw();
    }

    pub(crate) fn clear(&self) {
        *self.state.borrow_mut() = OcrState::default();
        self.area.queue_draw();
    }

    pub(in crate::library::lightbox) fn has_boxes(&self) -> bool {
        !self.state.borrow().boxes().is_empty()
    }

    /// Each box's bounds as fractions of the photo: left, top, width, and height.
    pub(in crate::library::lightbox) fn text_bounds(&self) -> Vec<(f64, f64, f64, f64)> {
        self.state
            .borrow()
            .boxes()
            .iter()
            .map(|ocr_box| {
                let corners = ocr_box.corners();
                let xs = corners.map(|c| c.0);
                let ys = corners.map(|c| c.1);
                let (left, top) = (
                    xs.iter().copied().fold(1.0, f64::min),
                    ys.iter().copied().fold(1.0, f64::min),
                );
                let (right, bottom) = (
                    xs.iter().copied().fold(0.0, f64::max),
                    ys.iter().copied().fold(0.0, f64::max),
                );
                (left, top, right - left, bottom - top)
            })
            .collect()
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

    /// The caret nearest `p` (in this area's coordinates) in the box under it.
    pub(super) fn caret_at(&self, p: (f64, f64)) -> Option<Caret> {
        let area = (f64::from(self.area.width()), f64::from(self.area.height()));
        self.state.borrow().caret_at(area, p)
    }

    /// Mouse press; returns whether dragging from here selects characters.
    pub(super) fn press_at(&self, caret: Caret, shift: bool, ctrl: bool) -> bool {
        let drags = self.state.borrow_mut().press_at(caret, shift, ctrl);
        self.area.queue_draw();
        drags
    }

    pub(super) fn drag_to(&self, caret: Caret) {
        self.state.borrow_mut().drag_to(caret);
        self.area.queue_draw();
    }

    pub(super) fn select_word(&self, caret: Caret) {
        self.state.borrow_mut().select_word(caret);
        self.area.queue_draw();
    }

    pub(super) fn click(&self, idx: usize, extend: bool) {
        self.state.borrow_mut().click(idx, extend);
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

/// Outlines every box in the accent colour, filled more strongly when hovered, and
/// strongest over the selected characters.
fn draw_boxes(cr: &gtk::cairo::Context, state: &OcrState, area: (f64, f64)) {
    let Some(rect) = contain_rect(area, state.image()) else {
        return;
    };
    let accent = libadwaita::StyleManager::default().accent_color().to_rgba();
    let rgb = (
        f64::from(accent.red()),
        f64::from(accent.green()),
        f64::from(accent.blue()),
    );
    for (idx, ocr_box) in state.boxes().iter().enumerate() {
        let quad = quad_in_area(rect, ocr_box.corners());
        let hovered = state.hovered() == Some(idx);
        let span = state.span(idx);
        trace(cr, &quad);
        cr.set_source_rgba(rgb.0, rgb.1, rgb.2, if hovered { 0.22 } else { 0.12 });
        let _ = cr.fill_preserve();
        cr.set_source_rgba(rgb.0, rgb.1, rgb.2, 1.0);
        cr.set_line_width(if hovered || span.is_some() { 2.5 } else { 1.5 });
        let _ = cr.stroke();
        if let Some((start, end)) = span {
            let stops = state.stops(idx);
            let at = |offset: usize| stops.get(offset).copied().unwrap_or(1.0);
            trace(cr, &slice_quad(&quad, at(start), at(end)));
            cr.set_source_rgba(rgb.0, rgb.1, rgb.2, 0.45);
            let _ = cr.fill();
        }
    }
}

fn trace(cr: &gtk::cairo::Context, quad: &[(f64, f64); 4]) {
    cr.move_to(quad[0].0, quad[0].1);
    for &(x, y) in &quad[1..] {
        cr.line_to(x, y);
    }
    cr.close_path();
}

/// Where each character boundary falls across `text` set in the widget's font, from 0 to 1.
/// The box's real typeface is unknown, but proportional widths beat even spacing.
fn char_stops(widget: &impl IsA<gtk::Widget>, text: &str) -> Vec<f64> {
    let layout = widget.create_pango_layout(Some(text));
    let width = f64::from(layout.extents().1.width());
    if width <= 0.0 {
        return even_stops(text);
    }
    let mut stops: Vec<f64> = text
        .char_indices()
        .map(|(byte, _)| {
            let x = layout.index_to_pos(byte as i32).x();
            (f64::from(x) / width).clamp(0.0, 1.0)
        })
        .collect();
    stops.push(1.0);
    stops
}
