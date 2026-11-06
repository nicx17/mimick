//! Selection state for the OCR boxes of one photo: whole boxes, or a run of characters.

use std::collections::BTreeSet;

use super::geometry::{contain_rect, fraction_along, quad_contains, quad_in_area, reading_order};
use crate::api_client::OcrBox;

/// A position between two characters: box index (reading order) and character offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Caret {
    pub idx: usize,
    pub offset: usize,
}

/// Boxes in reading order, plus what is selected and hovered; indices refer to that order.
#[derive(Debug, Default)]
pub(super) struct OcrState {
    boxes: Vec<OcrBox>,
    /// Per box, where each character boundary falls across it, from 0 to 1.
    stops: Vec<Vec<f64>>,
    image: (f64, f64),
    /// Whole boxes picked by clicking, tapping, or Select All.
    selected: BTreeSet<usize>,
    /// Characters from one caret to another, picked by dragging; replaces `selected`.
    range: Option<(Caret, Caret)>,
    /// Box a Shift-click extends from.
    anchor: Option<usize>,
    /// Where the current mouse drag started.
    drag_from: Option<Caret>,
    hovered: Option<usize>,
}

impl OcrState {
    /// `image` is the displayed texture's size; characters are spaced evenly.
    #[cfg(test)]
    pub(super) fn new(boxes: Vec<OcrBox>, image: (f64, f64)) -> Self {
        Self::measured(boxes, image, even_stops)
    }

    /// Like `new`, with `measure` giving each text's character boundaries from 0 to 1.
    pub(super) fn measured(
        boxes: Vec<OcrBox>,
        image: (f64, f64),
        measure: impl Fn(&str) -> Vec<f64>,
    ) -> Self {
        let boxes = reading_order(boxes);
        let stops = boxes.iter().map(|b| measure(&b.text)).collect();
        Self {
            boxes,
            stops,
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

    /// The caret nearest `p` inside the box under it.
    pub(super) fn caret_at(&self, area: (f64, f64), p: (f64, f64)) -> Option<Caret> {
        let idx = self.box_at(area, p)?;
        let rect = contain_rect(area, self.image)?;
        let quad = quad_in_area(rect, self.boxes[idx].corners());
        let along = fraction_along(&quad, p);
        Some(Caret {
            idx,
            offset: nearest_stop(&self.stops[idx], along),
        })
    }

    /// Character boundaries across box `idx`, from 0 to 1.
    pub(super) fn stops(&self, idx: usize) -> &[f64] {
        self.stops.get(idx).map_or(&[], Vec::as_slice)
    }

    /// Mouse press at `caret`; returns whether a drag from here should select characters.
    pub(super) fn press_at(&mut self, caret: Caret, shift: bool, ctrl: bool) -> bool {
        if ctrl {
            self.click(caret.idx, true);
            return false;
        }
        if shift {
            let from = self.range_start().unwrap_or(caret);
            self.range = Some((from, caret));
            self.drag_from = Some(from);
        } else {
            self.click(caret.idx, false);
            self.drag_from = Some(caret);
        }
        true
    }

    /// Where a Shift-click extends from: the current range's start, else the last clicked box.
    fn range_start(&self) -> Option<Caret> {
        self.range
            .map(|(from, _)| from)
            .or_else(|| self.anchor.map(|idx| Caret { idx, offset: 0 }))
    }

    /// Mouse drag reached `caret`: select the characters between it and where the drag began.
    pub(super) fn drag_to(&mut self, caret: Caret) {
        if let Some(from) = self.drag_from
            && from != caret
        {
            self.range = Some((from, caret));
        }
    }

    /// Double-click: the word around `caret`.
    pub(super) fn select_word(&mut self, caret: Caret) {
        let Some(text) = self.boxes.get(caret.idx).map(|b| b.text.as_str()) else {
            return;
        };
        let (start, end) = word_around(text, caret.offset);
        self.selected.clear();
        self.range = Some((
            Caret {
                idx: caret.idx,
                offset: start,
            },
            Caret {
                idx: caret.idx,
                offset: end,
            },
        ));
        self.anchor = Some(caret.idx);
    }

    /// Selects only `idx`, or toggles it when `extend` is set.
    pub(super) fn click(&mut self, idx: usize, extend: bool) {
        self.range = None;
        if extend {
            if !self.selected.remove(&idx) {
                self.selected.insert(idx);
            }
        } else {
            self.selected = BTreeSet::from([idx]);
        }
        self.anchor = Some(idx);
    }

    pub(super) fn select_all(&mut self) {
        self.range = None;
        self.selected = (0..self.boxes.len()).collect();
    }

    pub(super) fn clear_selection(&mut self) {
        self.selected.clear();
        self.range = None;
        self.anchor = None;
        self.drag_from = None;
    }

    pub(super) fn has_selection(&self) -> bool {
        (0..self.boxes.len()).any(|idx| self.span(idx).is_some())
    }

    /// Selected characters of box `idx` as `(start, end)` offsets; `None` when none are.
    pub(super) fn span(&self, idx: usize) -> Option<(usize, usize)> {
        let len = self.boxes.get(idx)?.text.chars().count();
        let (start, end) = match self.range {
            Some((a, b)) => {
                let (from, to) = (a.min(b), a.max(b));
                if idx < from.idx || idx > to.idx {
                    return None;
                }
                let start = if idx == from.idx { from.offset } else { 0 };
                let end = if idx == to.idx { to.offset } else { len };
                (start.min(len), end.min(len))
            }
            None if self.selected.contains(&idx) => (0, len),
            None => return None,
        };
        (start < end).then_some((start, end))
    }

    /// Selected text, one box per line in reading order.
    pub(super) fn selected_text(&self) -> Option<String> {
        let lines: Vec<String> = (0..self.boxes.len())
            .filter_map(|idx| {
                let (start, end) = self.span(idx)?;
                let text = &self.boxes[idx].text;
                Some(text.chars().skip(start).take(end - start).collect())
            })
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    /// Every box's text, one per line in reading order.
    pub(super) fn all_text(&self) -> Option<String> {
        let lines: Vec<&str> = self.boxes.iter().map(|b| b.text.as_str()).collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

/// Boundaries for evenly wide characters, the fallback when text can't be measured.
pub(super) fn even_stops(text: &str) -> Vec<f64> {
    let n = text.chars().count().max(1);
    (0..=n).map(|i| i as f64 / n as f64).collect()
}

/// Offset of the boundary closest to `along`.
fn nearest_stop(stops: &[f64], along: f64) -> usize {
    stops
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (*a - along).abs().total_cmp(&(*b - along).abs()))
        .map_or(0, |(offset, _)| offset)
}

/// Character offsets of the word containing `offset`; whitespace ends a word.
fn word_around(text: &str, offset: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let at = offset.min(chars.len().saturating_sub(1));
    if chars.get(at).is_none_or(|c| c.is_whitespace()) {
        return (0, chars.len());
    }
    let start = chars[..at]
        .iter()
        .rposition(|c| c.is_whitespace())
        .map_or(0, |i| i + 1);
    let end = chars[at..]
        .iter()
        .position(|c| c.is_whitespace())
        .map_or(chars.len(), |i| at + i);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::super::geometry::ob;
    use super::*;

    fn caret(idx: usize, offset: usize) -> Caret {
        Caret { idx, offset }
    }

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

    fn lines() -> OcrState {
        OcrState::new(
            vec![
                ob("hello world", 0.0, 0.0),
                ob("second line", 0.0, 0.25),
                ob("third", 0.0, 0.5),
            ],
            (100.0, 100.0),
        )
    }

    #[test]
    fn a_click_picks_the_whole_box_and_a_drag_picks_characters() {
        let mut state = lines();
        assert!(state.press_at(caret(0, 6), false, false));
        assert_eq!(state.selected_text().as_deref(), Some("hello world"));
        state.drag_to(caret(0, 11));
        assert_eq!(state.selected_text().as_deref(), Some("world"));
        state.drag_to(caret(0, 2));
        assert_eq!(state.selected_text().as_deref(), Some("llo "));
    }

    #[test]
    fn a_drag_across_boxes_takes_the_tail_middle_and_head() {
        let mut state = lines();
        state.press_at(caret(0, 6), false, false);
        state.drag_to(caret(2, 3));
        assert_eq!(
            state.selected_text().as_deref(),
            Some("world\nsecond line\nthi")
        );
    }

    #[test]
    fn shift_extends_and_ctrl_toggles_whole_boxes() {
        let mut state = lines();
        state.press_at(caret(1, 0), false, false);
        assert!(state.press_at(caret(1, 6), true, false));
        assert_eq!(state.selected_text().as_deref(), Some("second"));
        assert!(!state.press_at(caret(2, 1), false, true));
        assert_eq!(state.selected_text().as_deref(), Some("second line\nthird"));
    }

    #[test]
    fn double_click_selects_a_word() {
        let mut state = lines();
        state.select_word(caret(1, 9));
        assert_eq!(state.selected_text().as_deref(), Some("line"));
        assert_eq!(word_around("a b", 1), (0, 3));
    }

    #[test]
    fn carets_land_between_characters() {
        let state = OcrState::new(vec![ob("abcd", 0.0, 0.0)], (100.0, 100.0));
        // The box spans x 0–25 of a 100 px photo: a quarter across it is after "a".
        let at = state.caret_at((100.0, 100.0), (6.5, 3.0));
        assert_eq!(at, Some(caret(0, 1)));
        assert_eq!(nearest_stop(&even_stops("abcd"), 1.0), 4);
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
    fn has_selection_follows_clicks_ranges_and_clearing() {
        let mut state = lines();
        assert!(!state.has_selection());
        state.click(2, false);
        assert!(state.has_selection());
        state.clear_selection();
        assert!(!state.has_selection());
        state.press_at(caret(0, 3), false, false);
        state.drag_to(caret(0, 3));
        assert!(state.has_selection());
    }
}
