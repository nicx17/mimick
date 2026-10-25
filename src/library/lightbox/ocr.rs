use std::collections::BTreeSet;

use crate::api_client::OcrBox;

const ROW_THRESHOLD: f64 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ImageRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub(super) fn contain_rect(area: (f64, f64), image: (f64, f64)) -> Option<ImageRect> {
    let (area_w, area_h) = area;
    let (image_w, image_h) = image;
    if ![area_w, area_h, image_w, image_h].iter().all(|v| *v > 0.0) {
        return None;
    }
    let scale = (area_w / image_w).min(area_h / image_h);
    let (w, h) = (image_w * scale, image_h * scale);
    Some(ImageRect {
        x: (area_w - w) / 2.0,
        y: (area_h - h) / 2.0,
        w,
        h,
    })
}

pub(super) fn quad_in_area(rect: ImageRect, corners: [(f64, f64); 4]) -> [(f64, f64); 4] {
    corners.map(|(nx, ny)| (rect.x + nx * rect.w, rect.y + ny * rect.h))
}

pub(super) fn quad_contains(quad: &[(f64, f64); 4], p: (f64, f64)) -> bool {
    let (mut positive, mut negative) = (false, false);
    for i in 0..4 {
        let (ax, ay) = quad[i];
        let (bx, by) = quad[(i + 1) % 4];
        let cross = (bx - ax) * (p.1 - ay) - (by - ay) * (p.0 - ax);
        positive |= cross > 0.0;
        negative |= cross < 0.0;
    }
    !(positive && negative)
}

pub(super) fn reading_order(mut boxes: Vec<OcrBox>) -> Vec<OcrBox> {
    boxes.sort_by(|a, b| a.y1.total_cmp(&b.y1));
    let mut rows: Vec<Vec<OcrBox>> = Vec::new();
    for ocr_box in boxes {
        match rows.last_mut() {
            Some(row) if ocr_box.y1 - row[0].y1 < ROW_THRESHOLD => row.push(ocr_box),
            _ => rows.push(vec![ocr_box]),
        }
    }
    rows.into_iter()
        .flat_map(|mut row| {
            row.sort_by(|a, b| a.x1.total_cmp(&b.x1));
            row
        })
        .collect()
}

#[derive(Debug, Default)]
pub(super) struct OcrState {
    boxes: Vec<OcrBox>,
    image: (f64, f64),
    selected: BTreeSet<usize>,
    hovered: Option<usize>,
}

impl OcrState {
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

    pub(super) fn set_hovered(&mut self, idx: Option<usize>) -> bool {
        let changed = self.hovered != idx;
        self.hovered = idx;
        changed
    }

    pub(super) fn box_at(&self, area: (f64, f64), p: (f64, f64)) -> Option<usize> {
        let rect = contain_rect(area, self.image)?;
        self.boxes
            .iter()
            .rposition(|ocr_box| quad_contains(&quad_in_area(rect, ocr_box.corners()), p))
    }

    pub(super) fn click(&mut self, idx: usize, extend: bool) {
        if extend {
            if !self.selected.remove(&idx) {
                self.selected.insert(idx);
            }
        } else {
            self.selected = BTreeSet::from([idx]);
        }
    }

    pub(super) fn select_all(&mut self) {
        self.selected = (0..self.boxes.len()).collect();
    }

    pub(super) fn clear_selection(&mut self) {
        self.selected.clear();
    }

    pub(super) fn selected_text(&self) -> Option<String> {
        if self.selected.is_empty() {
            return None;
        }
        let lines: Vec<&str> = self
            .selected
            .iter()
            .filter_map(|&idx| self.boxes.get(idx))
            .map(|ocr_box| ocr_box.text.as_str())
            .collect();
        Some(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ob(text: &str, x: f64, y: f64) -> OcrBox {
        OcrBox {
            text: text.to_string(),
            x1: x,
            y1: y,
            x2: x + 0.25,
            y2: y,
            x3: x + 0.25,
            y3: y + 0.0625,
            x4: x,
            y4: y + 0.0625,
        }
    }

    #[test]
    fn contain_rect_letterboxes_wide_image() {
        assert_eq!(
            contain_rect((1000.0, 1000.0), (2000.0, 1000.0)),
            Some(ImageRect {
                x: 0.0,
                y: 250.0,
                w: 1000.0,
                h: 500.0
            })
        );
    }

    #[test]
    fn contain_rect_pillarboxes_tall_image() {
        assert_eq!(
            contain_rect((1000.0, 500.0), (500.0, 1000.0)),
            Some(ImageRect {
                x: 375.0,
                y: 0.0,
                w: 250.0,
                h: 500.0
            })
        );
    }

    #[test]
    fn contain_rect_rejects_empty_sizes() {
        assert_eq!(contain_rect((0.0, 500.0), (100.0, 100.0)), None);
        assert_eq!(contain_rect((500.0, 500.0), (100.0, 0.0)), None);
    }

    #[test]
    fn quad_maps_into_image_rect() {
        let rect = ImageRect {
            x: 0.0,
            y: 250.0,
            w: 1000.0,
            h: 500.0,
        };
        assert_eq!(
            quad_in_area(rect, [(0.125, 0.25), (0.5, 0.25), (0.5, 0.5), (0.125, 0.5)]),
            [
                (125.0, 375.0),
                (500.0, 375.0),
                (500.0, 500.0),
                (125.0, 500.0)
            ]
        );
    }

    #[test]
    fn quad_contains_handles_rotated_box() {
        let diamond = [(50.0, 0.0), (100.0, 50.0), (50.0, 100.0), (0.0, 50.0)];
        assert!(quad_contains(&diamond, (50.0, 50.0)));
        assert!(quad_contains(&diamond, (50.0, 0.0)));
        assert!(!quad_contains(&diamond, (10.0, 10.0)));
    }

    #[test]
    fn reading_order_groups_rows_then_columns() {
        let order = reading_order(vec![
            ob("right", 0.625, 0.5),
            ob("left", 0.125, 0.51),
            ob("top", 0.5, 0.25),
        ]);
        let texts: Vec<_> = order.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, ["top", "left", "right"]);
    }

    #[test]
    fn reading_order_survives_staircase_rows() {
        let boxes: Vec<_> = (0..30)
            .map(|i| ob(&i.to_string(), 1.0 - i as f64 * 0.03, i as f64 * 0.015))
            .collect();
        let mut texts: Vec<_> = reading_order(boxes).into_iter().map(|b| b.text).collect();
        texts.sort();
        let mut expected: Vec<_> = (0..30).map(|i| i.to_string()).collect();
        expected.sort();
        assert_eq!(texts, expected);
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
}
