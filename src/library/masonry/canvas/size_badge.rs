//! Large Files: each tile's file size, in a dark pill at its bottom left.

use gtk::graphene::{Point, Rect, Size};
use gtk::gsk::RoundedRect;
use gtk::pango;
use gtk::prelude::*;
use gtk::subclass::prelude::{ObjectSubclassExt, ObjectSubclassIsExt};

use super::{LaidItem, LaidRow, MasonryCanvas, imp};
use crate::library::asset_object::AssetObject;

impl MasonryCanvas {
    /// Label every tile with its file size, as Large Files does.
    pub fn set_show_file_sizes(&self, show: bool) {
        if self.imp().show_sizes.replace(show) != show {
            self.queue_draw();
        }
    }

    pub fn shows_file_sizes(&self) -> bool {
        self.imp().show_sizes.get()
    }
}

impl imp::MasonryCanvas {
    pub(super) fn paint_size_pill(
        &self,
        snapshot: &gtk::Snapshot,
        asset: &AssetObject,
        it: &LaidItem,
        row: &LaidRow,
    ) {
        let bytes = asset.file_size();
        if bytes == 0 {
            return;
        }
        let size = (row.h * 0.12).clamp(14.0, 20.0);
        let layout = self
            .obj()
            .create_pango_layout(Some(&glib::format_size(bytes)));
        let mut font = pango::FontDescription::new();
        font.set_weight(pango::Weight::Bold);
        font.set_absolute_size(f64::from(size * 0.72) * f64::from(pango::SCALE));
        layout.set_font_description(Some(&font));
        let (_, text) = layout.pixel_extents();
        let (pad, margin) = (size * 0.4, (size * 0.4).max(6.0));
        let height = size + pad * 0.5;
        let width = (text.width() as f32 + pad * 2.0).min(it.w - margin * 2.0);
        let pill = Rect::new(
            it.x + margin,
            row.y + row.h - margin - height,
            width,
            height,
        );
        let radius = Size::new(height * 0.5, height * 0.5);
        snapshot.push_rounded_clip(&RoundedRect::new(pill, radius, radius, radius, radius));
        snapshot.append_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.6), &pill);
        snapshot.translate(&Point::new(
            pill.x() + pad,
            pill.y() + (height - text.height() as f32) * 0.5,
        ));
        snapshot.append_layout(&layout, &gdk4::RGBA::new(1.0, 1.0, 1.0, 0.95));
        snapshot.pop();
    }
}
