//! Timeline painting: placeholder tiles for months not loaded yet, and the date headings.

use gtk::graphene::{Point, Rect};
use gtk::pango;
use gtk::prelude::*;
use gtk::subclass::prelude::ObjectSubclassExt;

use super::timeline::all_selected;
use super::{imp, placeholder_color};
use crate::library::timeline::layout::{ESTIMATED_ASPECT, HEADING_HEIGHT, LaidHeading};

const HEADING_INSET: f32 = 12.0;
const PLACEHOLDER_GAP: f32 = 2.0;

impl imp::MasonryCanvas {
    pub(super) fn paint_timeline_extras(
        &self,
        snapshot: &gtk::Snapshot,
        top: f32,
        bottom: f32,
        width: f32,
    ) {
        self.paint_estimated_sections(snapshot, top, bottom, width);
        self.paint_headings(snapshot, top, bottom);
    }

    fn paint_estimated_sections(
        &self,
        snapshot: &gtk::Snapshot,
        top: f32,
        bottom: f32,
        width: f32,
    ) {
        let cfg = self.cfg();
        let gap = cfg.gap.max(PLACEHOLDER_GAP);
        let row_h = (cfg.min_row_height + cfg.max_row_height) / 2.0;
        let per_row = ((width - 2.0 * gap) / (ESTIMATED_ASPECT * row_h))
            .floor()
            .max(1.0);
        let tile_w = (width - gap * (per_row + 1.0)) / per_row;
        let color = placeholder_color();
        let sections = self.sections.borrow();
        for section in sections
            .iter()
            .filter(|s| !s.loaded && s.y + s.h > top && s.y < bottom)
        {
            let end = section.y + section.h;
            let mut y = section.y + gap;
            while y < end.min(bottom) {
                let h = row_h.min(end - y);
                if y + h > top {
                    for i in 0..per_row as usize {
                        let x = gap + i as f32 * (tile_w + gap);
                        snapshot.append_color(&color, &Rect::new(x, y, tile_w, h));
                    }
                }
                y += row_h + gap;
            }
        }
    }

    fn paint_headings(&self, snapshot: &gtk::Snapshot, top: f32, bottom: f32) {
        let widget = self.obj();
        let color = match self.border_color.get() {
            Some(border) if self.border_gap.get() > 0.0 => readable_on(&border),
            _ => widget.color(),
        };
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
        for heading in self
            .headings
            .borrow()
            .iter()
            .filter(|h| h.y + HEADING_HEIGHT > top && h.y < bottom)
        {
            let layout = widget.create_pango_layout(Some(&heading.text));
            layout.set_attributes(Some(&attrs));
            let (text_w, text_h) = layout.pixel_size();
            snapshot.save();
            snapshot.translate(&Point::new(
                HEADING_INSET,
                heading.y + (HEADING_HEIGHT - text_h as f32) / 2.0,
            ));
            snapshot.append_layout(&layout, &color);
            snapshot.restore();
            if self.select_mode.get() {
                self.paint_heading_box(snapshot, heading, text_w as f32);
            }
        }
    }

    /// In selection mode, a box after the heading, checked once all its photos are;
    /// tapping the heading toggles them.
    fn paint_heading_box(&self, snapshot: &gtk::Snapshot, heading: &LaidHeading, text_w: f32) {
        const BOX: f32 = 20.0;
        let x = HEADING_INSET + text_w + 10.0;
        let y = heading.y + (HEADING_HEIGHT - BOX) / 2.0;
        let checked = self
            .selection
            .get()
            .is_some_and(|sel| all_selected(sel, heading.first, heading.count));
        if checked {
            self.paint_checked_box(snapshot, x, y, BOX);
        } else {
            Self::paint_unchecked_box(snapshot, x, y, BOX);
        }
    }
}

/// Near-black or near-white heading text, whichever reads better on `background`.
fn readable_on(background: &gdk4::RGBA) -> gdk4::RGBA {
    let luminance =
        0.2126 * background.red() + 0.7152 * background.green() + 0.0722 * background.blue();
    if luminance > 0.5 {
        gdk4::RGBA::new(0.1, 0.1, 0.1, 1.0)
    } else {
        gdk4::RGBA::new(0.95, 0.95, 0.95, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::readable_on;

    #[test]
    fn headings_stay_readable_on_the_border_colour() {
        let white = gdk4::RGBA::new(1.0, 1.0, 1.0, 1.0);
        let black = gdk4::RGBA::new(0.0, 0.0, 0.0, 1.0);
        let yellow = gdk4::RGBA::new(1.0, 0.9, 0.2, 1.0);
        let navy = gdk4::RGBA::new(0.1, 0.1, 0.4, 1.0);
        assert!(readable_on(&white).red() < 0.5);
        assert!(readable_on(&yellow).red() < 0.5);
        assert!(readable_on(&black).red() > 0.5);
        assert!(readable_on(&navy).red() > 0.5);
    }
}
