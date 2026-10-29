use std::sync::Arc;

use gtk::graphene::{Rect, Size};
use gtk::gsk::RoundedRect;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use super::{
    AssetObject, LaidItem, LaidRow, LibraryAssetModel, LoadRequest, MasonryCanvas, PaintResult, CORNER_RADIUS,
    SnapshotCtx, SnapshotStats, SnapshotViewport, ThumbnailCache, VIEWPORTS_AHEAD,
    VIEWPORTS_BEHIND,
};
use super::super::{
    REVEAL_MAX_BLUR, accent_bg_color, bucket_for_asset, bucket_for_row_height, cached_texture,
    placeholder_color, resolve_symbolic_icon, resolve_video_icon, reveal_progress, rows_between,
};

impl MasonryCanvas {
        /// With borders on, the whole visible canvas (headings and the space below the
        /// last row included) takes the border colour, so gaps read as one border.
        pub(super) fn paint_border_background(
            &self,
            snapshot: &gtk::Snapshot,
            viewport: &SnapshotViewport,
            canvas_w: f32,
        ) {
            if self.border_gap.get() <= 0.0 {
                return;
            }
            let color = self
                .border_color
                .get()
                .unwrap_or_else(|| gdk4::RGBA::new(1.0, 1.0, 1.0, 1.0));
            let height = (self.obj().height() as f32).max(self.layout_h.get());
            let top = viewport.scroll_y.max(0.0);
            let bottom = viewport.bottom.min(height);
            if bottom > top {
                snapshot.append_color(&color, &Rect::new(0.0, top, canvas_w, bottom - top));
            }
        }

        pub(super) fn snapshot_ctx<'a>(
            &'a self,
            model: &'a LibraryAssetModel,
            cache: &'a Arc<ThumbnailCache>,
        ) -> SnapshotCtx<'a> {
            SnapshotCtx {
                model,
                cache,
                placeholder: placeholder_color(),
                select_tint: gdk4::RGBA::new(0.30, 0.55, 0.95, 0.35),
                selection: self.selection.get(),
            }
        }

        pub(super) fn snapshot_viewport(&self) -> SnapshotViewport {
            let (scroll_y, viewport_h) = self.viewport();
            SnapshotViewport {
                scroll_y,
                center: scroll_y + viewport_h * 0.5,
                top: scroll_y,
                bottom: scroll_y + viewport_h,
                band_top: scroll_y - viewport_h * VIEWPORTS_BEHIND,
                band_bottom: scroll_y + viewport_h * (1.0 + VIEWPORTS_AHEAD),
            }
        }

        pub(super) fn paint_snapshot_rows(
            &self,
            snapshot: &gtk::Snapshot,
            sctx: &SnapshotCtx<'_>,
            rows: &[LaidRow],
            viewport: &SnapshotViewport,
        ) -> SnapshotStats {
            let mut stats = SnapshotStats::default();
            for row in rows_between(rows, viewport.top, viewport.bottom) {
                for it in &row.items {
                    stats.record(self.paint_item(snapshot, sctx, row, it));
                }
            }
            stats
        }

        pub(super) fn wanted_loads(
            &self,
            model: &LibraryAssetModel,
            rows: &[LaidRow],
            viewport: &SnapshotViewport,
        ) -> Vec<LoadRequest> {
            if self.flinging.get() {
                return Vec::new();
            }
            let mut wanted = Vec::new();
            for row in rows_between(rows, viewport.band_top, viewport.band_bottom) {
                let bucket = bucket_for_row_height(row.h, self.quality.get());
                wanted.extend(
                    row.items.iter().filter_map(|it| {
                        self.load_request(model, it, row, viewport.center, bucket)
                    }),
                );
            }
            wanted
        }

        fn paint_item(
            &self,
            snapshot: &gtk::Snapshot,
            sctx: &SnapshotCtx<'_>,
            row: &LaidRow,
            it: &LaidItem,
        ) -> PaintResult {
            let asset = sctx
                .model
                .item(it.asset_index)
                .and_downcast::<AssetObject>();
            let rect = Rect::new(it.x, row.y, it.w, row.h);

            let Some(asset) = asset else {
                snapshot.append_color(&sctx.placeholder, &rect);
                return PaintResult::Miss;
            };

            let asset_id = asset.property::<String>("id");
            let local_path = asset.property::<String>("local-path");
            let bucket = bucket_for_asset(&asset_id, &local_path, row.h, self.quality.get());
            let cached = cached_texture(sctx.cache, &asset_id, bucket);

            let clipped = CORNER_RADIUS > 0.0;
            if clipped {
                let corner = Size::new(CORNER_RADIUS, CORNER_RADIUS);
                let rounded = RoundedRect::new(rect, corner, corner, corner, corner);
                snapshot.push_rounded_clip(&rounded);
            }

            let result = if let Some(tex) = cached.as_ref() {
                self.paint_texture(snapshot, tex, &rect, &sctx.placeholder, &asset_id);
                PaintResult::Hit
            } else {
                snapshot.append_color(&sctx.placeholder, &rect);
                PaintResult::Miss
            };

            let selected = sctx
                .selection
                .map(|s| s.is_selected(it.asset_index))
                .unwrap_or(false);
            if selected {
                snapshot.append_color(&sctx.select_tint, &rect);
            }

            self.paint_badges(snapshot, &asset, it, row, selected);

            if clipped {
                snapshot.pop();
            }
            result
        }

        /// A loaded thumbnail, faded in and un-blurred while its reveal runs.
        fn paint_texture(
            &self,
            snapshot: &gtk::Snapshot,
            tex: &gdk4::Texture,
            rect: &Rect,
            placeholder: &gdk4::RGBA,
            asset_id: &str,
        ) {
            let started = self.reveals.borrow().get(asset_id).copied();
            let Some(alpha) =
                started.and_then(|start| reveal_progress(glib::monotonic_time() - start))
            else {
                snapshot.append_texture(tex, rect);
                return;
            };
            snapshot.append_color(placeholder, rect);
            snapshot.push_clip(rect);
            let blur = REVEAL_MAX_BLUR * (1.0 - alpha);
            let blurred = blur > 0.1;
            if blurred {
                snapshot.push_blur(f64::from(blur));
            }
            snapshot.push_opacity(f64::from(alpha));
            snapshot.append_texture(tex, rect);
            snapshot.pop();
            if blurred {
                snapshot.pop();
            }
            snapshot.pop();
        }

        /// Start fading `asset_id` in, with one frame-clock tick driving every running reveal.
        pub(in crate::library::masonry::canvas) fn start_reveal(&self, asset_id: &str) {
            self.reveals
                .borrow_mut()
                .insert(asset_id.to_string(), glib::monotonic_time());
            if self.reveal_ticking.replace(true) {
                return;
            }
            self.obj().add_tick_callback(|canvas, _| {
                let imp = canvas.imp();
                let now = glib::monotonic_time();
                imp.reveals
                    .borrow_mut()
                    .retain(|_, start| reveal_progress(now - *start).is_some());
                canvas.queue_draw();
                if imp.reveals.borrow().is_empty() {
                    imp.reveal_ticking.set(false);
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
        }

        /// Overlays on top of the thumbnail: select-mode checkbox, video play badge,
        /// and the favorite heart.
        fn paint_badges(
            &self,
            snapshot: &gtk::Snapshot,
            asset: &AssetObject,
            it: &LaidItem,
            row: &LaidRow,
            selected: bool,
        ) {
            if self.select_mode.get() {
                self.paint_checkbox(snapshot, it.x, row.y, row.h, selected);
            }
            let is_video = asset
                .property::<String>("asset-type")
                .eq_ignore_ascii_case("VIDEO");
            if is_video {
                self.paint_video_badge(snapshot, it.x, row.y, it.w, row.h);
            }
            if asset.is_favorite() {
                self.paint_favorite_badge(snapshot, it.x + it.w, row.y + row.h, row.h);
            }
        }

        /// Paint the video play-icon badge centred on the cell.
        ///
        /// A semi-transparent dark circle is drawn behind the icon for contrast,
        /// then the cached `mimick-video-symbolic` icon is rendered on top.
        fn paint_video_badge(
            &self,
            snapshot: &gtk::Snapshot,
            cell_x: f32,
            cell_y: f32,
            cell_w: f32,
            cell_h: f32,
        ) {
            let icon = self
                .video_icon
                .get_or_init(|| resolve_video_icon(&self.obj()));

            let icon_size = (cell_h * 0.22).clamp(16.0, 48.0);
            let bg_size = icon_size * 1.6;

            // Centre the background circle.
            let bg_x = cell_x + (cell_w - bg_size) * 0.5;
            let bg_y = cell_y + (cell_h - bg_size) * 0.5;
            let bg_rect = Rect::new(bg_x, bg_y, bg_size, bg_size);
            let half = bg_size * 0.5;
            let corner = Size::new(half, half);
            let rounded = RoundedRect::new(bg_rect, corner, corner, corner, corner);

            let bg_color = gdk4::RGBA::new(0.0, 0.0, 0.0, 0.55);
            snapshot.push_rounded_clip(&rounded);
            snapshot.append_color(&bg_color, &bg_rect);
            snapshot.pop();

            // Centre the icon inside the circle.
            let icon_x = cell_x + (cell_w - icon_size) * 0.5;
            let icon_y = cell_y + (cell_h - icon_size) * 0.5;
            snapshot.save();
            snapshot.translate(&gtk::graphene::Point::new(icon_x, icon_y));
            icon.snapshot(
                snapshot.upcast_ref::<gdk4::Snapshot>(),
                icon_size as f64,
                icon_size as f64,
            );
            snapshot.restore();
        }

        /// Paint a white heart in the bottom-right corner (`right`, `bottom`) of a
        /// favorited tile. A dark copy offset by one pixel keeps it legible on bright photos.
        fn paint_favorite_badge(
            &self,
            snapshot: &gtk::Snapshot,
            right: f32,
            bottom: f32,
            cell_h: f32,
        ) {
            let icon = self
                .heart_icon
                .get_or_init(|| resolve_symbolic_icon(&self.obj(), "mimick-heart-filled-symbolic"));
            let Some(symbolic) = icon.dynamic_cast_ref::<gtk::SymbolicPaintable>() else {
                return;
            };
            let size = (cell_h * 0.12).clamp(14.0, 22.0);
            let margin = (size * 0.4).max(6.0);
            let shadow = gdk4::RGBA::new(0.0, 0.0, 0.0, 0.45);
            let white = gdk4::RGBA::new(1.0, 1.0, 1.0, 0.95);
            for (offset, color) in [(1.0, shadow), (0.0, white)] {
                snapshot.save();
                snapshot.translate(&gtk::graphene::Point::new(
                    right - margin - size + offset,
                    bottom - margin - size + offset,
                ));
                symbolic.snapshot_symbolic(
                    snapshot.upcast_ref::<gdk4::Snapshot>(),
                    size as f64,
                    size as f64,
                    &[color],
                );
                snapshot.restore();
            }
        }

        /// Paint a checkbox indicator in the top-left corner of a tile.
        fn paint_checkbox(
            &self,
            snapshot: &gtk::Snapshot,
            cell_x: f32,
            cell_y: f32,
            cell_h: f32,
            selected: bool,
        ) {
            let box_size = (cell_h * 0.14).clamp(16.0, 26.0);
            let margin = 6.0_f32;
            let bx = cell_x + margin;
            let by = cell_y + margin;
            if selected {
                self.paint_checked_box(snapshot, bx, by, box_size);
            } else {
                Self::paint_unchecked_box(snapshot, bx, by, box_size);
            }
        }

        /// Accent-filled square with a checkmark icon.
        fn paint_checked_box(&self, snapshot: &gtk::Snapshot, bx: f32, by: f32, box_size: f32) {
            let r = 4.0_f32;
            let corner = Size::new(r, r);
            let rect = Rect::new(bx, by, box_size, box_size);
            let rounded = RoundedRect::new(rect, corner, corner, corner, corner);
            snapshot.push_rounded_clip(&rounded);
            snapshot.append_color(&accent_bg_color(), &rect);
            snapshot.pop();

            let icon = self
                .check_icon
                .get_or_init(|| resolve_symbolic_icon(&self.obj(), "object-select-symbolic"));
            let inset = 3.0_f32;
            let icon_size = box_size - inset * 2.0;
            snapshot.save();
            snapshot.translate(&gtk::graphene::Point::new(bx + inset, by + inset));
            icon.snapshot(
                snapshot.upcast_ref::<gdk4::Snapshot>(),
                icon_size as f64,
                icon_size as f64,
            );
            snapshot.restore();
        }

        /// Bordered empty square (outline only).
        fn paint_unchecked_box(snapshot: &gtk::Snapshot, bx: f32, by: f32, box_size: f32) {
            let r = 4.0_f32;
            let border_w = 2.0_f32;
            let corner = Size::new(r, r);
            let outer_rect = Rect::new(bx, by, box_size, box_size);
            let outer_round = RoundedRect::new(outer_rect, corner, corner, corner, corner);

            snapshot.push_rounded_clip(&outer_round);
            snapshot.append_color(&gdk4::RGBA::new(1.0, 1.0, 1.0, 0.8), &outer_rect);

            let ir = (r - border_w).max(0.0);
            let ic = Size::new(ir, ir);
            let inner = Rect::new(
                bx + border_w,
                by + border_w,
                box_size - border_w * 2.0,
                box_size - border_w * 2.0,
            );
            let inner_round = RoundedRect::new(inner, ic, ic, ic, ic);
            snapshot.push_rounded_clip(&inner_round);
            snapshot.append_color(&gdk4::RGBA::new(0.0, 0.0, 0.0, 0.3), &inner);
            snapshot.pop();
            snapshot.pop();
        }
}
