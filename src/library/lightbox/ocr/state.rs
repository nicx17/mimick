//! Selection state for the OCR boxes of one photo.

use std::collections::BTreeSet;

use super::geometry::{contain_rect, quad_contains, quad_in_area, reading_order};
use crate::api_client::OcrBox;

/// Boxes in reading order, plus what is selected and hovered; indices refer to that order.
#[derive(Debug, Default)]
pub(super) struct OcrState {
    boxes: Vec<OcrBox>,
    image: (f64, f64),
    selected: BTreeSet<usize>,
    /// Box a Shift-click or drag extends from.
    anchor: Option<usize>,
    hovered: Option<usize>,
}

impl OcrState {
    /// `image` is the displayed texture's size, used to place the normalized boxes.
    pub(super) fn new(boxes: Vec<OcrBox>, image: (f64, f64)) -> Self {
        Self {
            boxes: reading_order(boxes),
            image,
            ..Self::default()
        }
    }

    pub(super) fn boxes(&self) -> &[OcrBox] {
        &self.boxes
    }

    pub(super) fn image(&self) -> (f64, f64) {
        self.image
    }

    pub(super) fn is_selected(&self, idx: usize) -> bool {
        self.selected.contains(&idx)
    }

    pub(super) fn hovered(&self) -> Option<usize> {
        self.hovered
    }

    /// Returns whether the hovered box changed, so callers redraw only when needed.
    pub(super) fn set_hovered(&mut self, idx: Option<usize>) -> bool {
        let changed = self.hovered != idx;
        self.hovered = idx;
        changed
    }

    /// Topmost box under `p`, in the coordinates of a widget of size `area`.
    pub(super) fn box_at(&self, area: (f64, f64), p: (f64, f64)) -> Option<usize> {
        let rect = contain_rect(area, self.image)?;
        self.boxes
            .iter()
            .rposition(|ocr_box| quad_contains(&quad_in_area(rect, ocr_box.corners()), p))
    }

    /// Mouse press on a box; returns whether dragging should extend the selection.
    pub(super) fn press(&mut self, idx: usize, shift: bool, ctrl: bool) -> bool {
        if ctrl {
            self.click(idx, true);
            return false;
        }
        if shift {
            self.extend_to(idx);
        } else {
            self.click(idx, false);
        }
        true
    }

    /// Selects only `idx`, or toggles it when `extend` is set.
    pub(super) fn click(&mut self, idx: usize, extend: bool) {
        if extend {
            if !self.selected.remove(&idx) {
                self.selected.insert(idx);
            }
        } else {
            self.selected = BTreeSet::from([idx]);
        }
        self.anchor = Some(idx);
    }

    /// Selects every box between the anchor and `idx`.
    pub(super) fn extend_to(&mut self, idx: usize) {
        let anchor = *self.anchor.get_or_insert(idx);
        self.selected = (anchor.min(idx)..=anchor.max(idx)).collect();
    }

    pub(super) fn select_all(&mut self) {
        self.selected = (0..self.boxes.len()).collect();
    }

    pub(super) fn clear_selection(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }

    pub(super) fn has_selection(&self) -> bool {
        !self.selected.is_empty()
    }

    /// Selected boxes' text, one per line in reading order.
    pub(super) fn selected_text(&self) -> Option<String> {
        self.join_text(self.selected.iter().copied())
    }

    /// Every box's text, one per line in reading order.
    pub(super) fn all_text(&self) -> Option<String> {
        self.join_text(0..self.boxes.len())
    }

    fn join_text(&self, indices: impl Iterator<Item = usize>) -> Option<String> {
        let lines: Vec<&str> = indices
            .filter_map(|idx| self.boxes.get(idx))
            .map(|ocr_box| ocr_box.text.as_str())
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::super::geometry::ob;
    use super::*;

    #[test]
    fn click_replaces_selection_and_extend_toggles() {
        let mut state = OcrState::new(
            vec![ob("a", 0.0, 0.0), ob("b", 0.0, 0.25), ob("c", 0.0, 0.5)],
            (100.0, 100.0),
        );
        state.click(0, false);
        state.click(2, true);
        assert_eq!(state.selected_text().as_deref(), Some("a\nc"));
        state.click(0, true);
        assert_eq!(state.selected_text().as_deref(), Some("c"));
        state.click(1, false);
        assert_eq!(state.selected_text().as_deref(), Some("b"));
        state.clear_selection();
        assert_eq!(state.selected_text(), None);
    }

    #[test]
    fn select_all_copies_in_reading_order() {
        let mut state = OcrState::new(
            vec![
                ob("right", 0.625, 0.5),
                ob("left", 0.125, 0.51),
                ob("top", 0.5, 0.25),
            ],
            (100.0, 100.0),
        );
        state.select_all();
        assert_eq!(state.selected_text().as_deref(), Some("top\nleft\nright"));
    }

    #[test]
    fn box_at_ignores_letterbox() {
        let state = OcrState::new(vec![ob("a", 0.125, 0.25)], (2000.0, 1000.0));
        assert_eq!(state.box_at((1000.0, 1000.0), (200.0, 390.0)), Some(0));
        assert_eq!(state.box_at((1000.0, 1000.0), (200.0, 100.0)), None);
        assert_eq!(state.box_at((0.0, 0.0), (0.0, 0.0)), None);
    }

    fn four_rows() -> OcrState {
        OcrState::new(
            vec![
                ob("a", 0.0, 0.0),
                ob("b", 0.0, 0.25),
                ob("c", 0.0, 0.5),
                ob("d", 0.0, 0.75),
            ],
            (100.0, 100.0),
        )
    }

    #[test]
    fn extend_to_selects_the_range_from_the_last_click() {
        let mut state = four_rows();
        state.click(1, false);
        state.extend_to(3);
        assert_eq!(state.selected_text().as_deref(), Some("b\nc\nd"));
        state.extend_to(0);
        assert_eq!(state.selected_text().as_deref(), Some("a\nb"));
    }

    #[test]
    fn extend_to_without_a_click_selects_one_box() {
        let mut state = four_rows();
        state.extend_to(2);
        assert_eq!(state.selected_text().as_deref(), Some("c"));
    }

    #[test]
    fn all_text_joins_every_box_in_reading_order() {
        let mut state = OcrState::new(
            vec![ob("right", 0.625, 0.5), ob("top", 0.5, 0.25)],
            (100.0, 100.0),
        );
        state.click(0, false);
        assert_eq!(state.all_text().as_deref(), Some("top\nright"));
        assert_eq!(OcrState::default().all_text(), None);
    }

    #[test]
    fn has_selection_follows_clicks_and_clearing() {
        let mut state = four_rows();
        assert!(!state.has_selection());
        state.click(2, false);
        assert!(state.has_selection());
        state.clear_selection();
        assert!(!state.has_selection());
    }

    #[test]
    fn press_selects_extends_or_toggles_and_reports_drag() {
        let mut state = four_rows();
        assert!(state.press(1, false, false));
        assert_eq!(state.selected_text().as_deref(), Some("b"));
        assert!(state.press(3, true, false));
        assert_eq!(state.selected_text().as_deref(), Some("b\nc\nd"));
        assert!(!state.press(0, false, true));
        assert_eq!(state.selected_text().as_deref(), Some("a\nb\nc\nd"));
    }
}
