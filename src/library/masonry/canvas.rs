//! Justified-row masonry layout for the photos grid.

use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use libadwaita::prelude::*;

use super::grid_view::AssetContextMenuHandler;
use crate::api_client::ThumbnailSize;
use crate::library::asset_model::LibraryAssetModel;
use crate::library::asset_object::AssetObject;
use crate::library::thumbnail_cache::ThumbnailCache;

type ActivateHandler = Rc<dyn Fn(u32)>;
type SelectModeChanger = Rc<dyn Fn(bool)>;
type MonthRequestHandler = Rc<dyn Fn(Vec<usize>)>;

pub(super) const CORNER_RADIUS: f32 = 0.0;

const VIEWPORTS_BEHIND: f32 = 0.5;
const VIEWPORTS_AHEAD: f32 = 1.0;

use super::layout::{
    LaidItem, LaidRow, LayoutConfig, first_row_at_or_after, item_at_x, pack_rows, row_at_y,
};
pub use super::quality::GridQuality;
use super::quality::{bucket_for_row_height, fallback_bucket};
use super::reveal::{REVEAL_MAX_BLUR, animations_enabled, reveal_progress};
use super::schedule::{CancelFlag, LoadRequest, LoadScheduler, SETTLE_DELAY, is_fling};

mod interaction;
mod timeline;

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;
    use std::sync::Arc;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    use super::timeline::CachedSection;
    use crate::config::DateHeadings;
    use crate::library::timeline::index::TimelineIndex;
    use crate::library::timeline::layout::{LaidHeading, LaidSection};

    use super::{
        ActivateHandler, AssetContextMenuHandler, AssetObject, CORNER_RADIUS, CancelFlag,
        GridQuality, LaidItem, LaidRow, LayoutConfig, LibraryAssetModel, LoadRequest,
        LoadScheduler, MonthRequestHandler, SETTLE_DELAY, SelectModeChanger, ThumbnailCache,
        ThumbnailSize, VIEWPORTS_AHEAD, VIEWPORTS_BEHIND, cached_texture, collect_dims,
        finish_masonry_load, is_fling, load_masonry_asset, load_target_for, pack_rows,
    };

    pub(super) enum PaintResult {
        Hit,
        Miss,
    }

    struct SnapshotCtx<'a> {
        model: &'a LibraryAssetModel,
        cache: &'a Arc<ThumbnailCache>,
        placeholder: gdk4::RGBA,
        select_tint: gdk4::RGBA,
        selection: Option<&'a gtk::MultiSelection>,
    }

    #[derive(Default)]
    pub(super) struct SnapshotStats {
        pub(super) hits: usize,
        pub(super) misses: usize,
        pub(super) painted: usize,
    }

    struct SnapshotViewport {
        scroll_y: f32,
        center: f32,
        top: f32,
        bottom: f32,
        band_top: f32,
        band_bottom: f32,
    }

    #[derive(Default)]
    pub struct MasonryCanvas {
        pub model: OnceCell<LibraryAssetModel>,
        pub cache: OnceCell<Arc<ThumbnailCache>>,
        pub selection: OnceCell<gtk::MultiSelection>,
        pub narrow: Cell<bool>,
        pub select_mode: Cell<bool>,
        pub quality: Cell<GridQuality>,
        pub rows: RefCell<Vec<LaidRow>>,
        pub cached_width: Cell<f32>,
        pub layout_h: Cell<f32>,
        pub(super) scheduler: RefCell<LoadScheduler>,
        pub last_scroll: Cell<(f64, i64)>,
        pub flinging: Cell<bool>,
        pub settle: RefCell<Option<glib::SourceId>>,
        pub timeline: RefCell<Option<Rc<RefCell<TimelineIndex>>>>,
        pub date_headings: Cell<DateHeadings>,
        pub(super) section_cache: RefCell<HashMap<usize, CachedSection>>,
        pub headings: RefCell<Vec<LaidHeading>>,
        pub sections: RefCell<Vec<LaidSection>>,
        pub month_request_handler: RefCell<Option<MonthRequestHandler>>,
        pub month_request_pending: Cell<bool>,
        pub relayout_pending: Cell<bool>,
        /// Permanently failed ids; skipped to avoid re-queueing every frame.
        pub failed: RefCell<HashSet<String>>,
        pub vadjustment: RefCell<Option<gtk::Adjustment>>,
        pub activate_handler: RefCell<Option<ActivateHandler>>,
        pub context_menu_handler: RefCell<Option<AssetContextMenuHandler>>,
        pub select_mode_changer: RefCell<Option<SelectModeChanger>>,
        /// Anchor position for Shift+Click range selection.
        pub last_selected: Cell<Option<u32>>,
        pub permission_warned: Cell<bool>,
        /// Cached video badge icon paintable, resolved lazily from the icon theme.
        pub video_icon: OnceCell<gdk4::Paintable>,
        /// Cached favorite heart icon, painted on favorited tiles.
        pub heart_icon: OnceCell<gdk4::Paintable>,
        /// Cached checkbox icon (selected state).
        pub check_icon: OnceCell<gdk4::Paintable>,
        /// Cached checkbox icon (unselected state).
        pub uncheck_icon: OnceCell<gdk4::Paintable>,
        /// True while a drag-out operation is in progress; suppresses click-to-activate.
        pub drag_active: Cell<bool>,
        /// Configurable gap between grid tiles, in pixels.
        pub border_gap: Cell<f32>,
        /// Color painted behind tiles so the gap appears as a border.
        pub border_color: Cell<Option<gdk4::RGBA>>,
        /// When each freshly loaded tile started fading in, in monotonic microseconds.
        pub reveals: RefCell<HashMap<String, i64>>,
        pub reveal_ticking: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MasonryCanvas {
        const NAME: &str = "MimickMasonryCanvas";
        type Type = super::MasonryCanvas;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for MasonryCanvas {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().add_css_class("mimick-masonry-canvas");
            self.cached_width.set(-1.0);
        }
    }

    impl WidgetImpl for MasonryCanvas {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => (120, 200, -1, -1),
                _ => {
                    let width = if for_size > 0 {
                        for_size as f32
                    } else {
                        self.cached_width.get().max(120.0)
                    };
                    let h = self.layout_for_width(width);
                    let h_i = h.ceil() as i32;
                    (h_i, h_i, -1, -1)
                }
            }
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            let w = width.max(0) as f32;
            if (w - self.cached_width.get()).abs() > 0.5 {
                self.cached_width.set(-1.0);
            }
            let _ = self.layout_for_width(w);
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let canvas_w = widget.width() as f32;
            if canvas_w <= 0.0 {
                return;
            }
            let _ = self.layout_for_width(canvas_w);

            let rows = self.rows.borrow();
            let Some(model) = self.model.get() else {
                return;
            };
            let Some(cache) = self.cache.get() else {
                return;
            };

            let viewport = self.snapshot_viewport();

            self.paint_border_background(snapshot, &viewport, canvas_w);

            let sctx = self.snapshot_ctx(model, cache);
            let stats = self.paint_snapshot_rows(snapshot, &sctx, &rows, &viewport);
            if self.timeline.borrow().is_some() {
                self.paint_timeline_extras(snapshot, viewport.top, viewport.bottom, canvas_w);
                self.request_visible_months(viewport.band_top, viewport.band_bottom);
            }
            let wanted = self.wanted_loads(model, &rows, &viewport);
            drop(rows);
            self.finish_snapshot(cache, &viewport, &stats, wanted);
        }
    }

    /// Compact layout threshold: canvas widths below this use the
    /// smallest tile tier for phone-sized screens.
    const COMPACT_WIDTH_THRESHOLD: f32 = 400.0;

    mod paint {
        include!("canvas_paint.rs");
    }

    impl MasonryCanvas {
        pub(super) fn cfg(&self) -> LayoutConfig {
            let gap = self.border_gap.get();
            if self.narrow.get() {
                let w = self.cached_width.get();
                if w > 0.0 && w < COMPACT_WIDTH_THRESHOLD {
                    LayoutConfig::compact().with_gap(gap)
                } else {
                    LayoutConfig::narrow().with_gap(gap)
                }
            } else {
                LayoutConfig::wide().with_gap(gap)
            }
        }

        pub(super) fn layout_for_width(&self, width: f32) -> f32 {
            let cached = self.cached_width.get();
            if (width - cached).abs() < 0.5 && cached >= 0.0 {
                return self.layout_h.get();
            }
            let Some(model) = self.model.get() else {
                self.cached_width.set(width);
                self.layout_h.set(0.0);
                return 0.0;
            };
            let timeline = self.timeline.borrow().clone();
            let (rows, h) = match timeline {
                Some(index) => {
                    let layout = self.timeline_layout(model, &index.borrow(), width);
                    *self.headings.borrow_mut() = layout.headings;
                    *self.sections.borrow_mut() = layout.sections;
                    (layout.rows, layout.height)
                }
                None => pack_rows(&collect_dims(model), width, self.cfg()),
            };
            *self.rows.borrow_mut() = rows;
            self.cached_width.set(width);
            self.layout_h.set(h);
            h
        }

        pub(super) fn invalidate_layout(&self) {
            // Pending and textures are NOT cleared here — they are independent
            // of layout and are owned per-asset across reflows.
            self.cached_width.set(-1.0);
            self.layout_h.set(0.0);
            self.rows.borrow_mut().clear();
            self.obj().queue_resize();
        }

        fn load_request(
            &self,
            model: &LibraryAssetModel,
            it: &LaidItem,
            row: &LaidRow,
            viewport_center: f32,
            bucket: ThumbnailSize,
        ) -> Option<LoadRequest> {
            let (asset_id, local_path, is_local_only) = load_target_for(model, it)?;
            let lookup_bucket = if is_local_only {
                ThumbnailSize::Thumbnail
            } else {
                bucket
            };
            if self.load_is_blocked(&asset_id, lookup_bucket) {
                return None;
            }
            let cell_center = row.y + row.h * 0.5;
            Some(LoadRequest {
                priority: (cell_center - viewport_center).abs(),
                asset_id,
                asset_index: it.asset_index,
                bucket,
                local_path,
                is_local: is_local_only,
            })
        }

        fn load_is_blocked(&self, asset_id: &str, bucket: ThumbnailSize) -> bool {
            self.failed.borrow().contains(asset_id)
                || self
                    .cache
                    .get()
                    .is_none_or(|cache| cache.peek_cached(asset_id, bucket).is_some())
        }

        fn finish_snapshot(
            &self,
            cache: &ThumbnailCache,
            viewport: &SnapshotViewport,
            stats: &SnapshotStats,
            wanted: Vec<LoadRequest>,
        ) {
            let started = self.scheduler.borrow_mut().plan(wanted);
            self.log_snapshot(cache, viewport, stats, started.len());
            for (request, cancel) in started {
                self.spawn_load(request, cancel);
            }
        }

        fn log_snapshot(
            &self,
            cache: &ThumbnailCache,
            viewport: &SnapshotViewport,
            stats: &SnapshotStats,
            queued: usize,
        ) {
            let (cache_n, cache_bytes, cache_max, cache_evicts) = cache.cache_stats();
            log::debug!(
                "masonry snapshot y={:.0} painted={} hits={} misses={} pending={} queued={} cache={}/{}/{}MB evicts={}",
                viewport.scroll_y,
                stats.painted,
                stats.hits,
                stats.misses,
                self.scheduler.borrow().in_flight(),
                queued,
                cache_n,
                cache_bytes / (1024 * 1024),
                cache_max / (1024 * 1024),
                cache_evicts,
            );
        }

        pub(super) fn viewport(&self) -> (f32, f32) {
            let Some(adj) = self.find_vadjustment() else {
                return (0.0, self.obj().height() as f32);
            };
            let scroll_y = self
                .obj()
                .parent()
                .and_then(|parent| super::scrolled_child(&parent))
                .and_then(|child| {
                    self.obj()
                        .compute_point(&child, &gtk::graphene::Point::new(0.0, 0.0))
                })
                .map(|origin| -origin.y())
                .unwrap_or(adj.value() as f32);
            (scroll_y, adj.page_size() as f32)
        }

        pub(super) fn find_vadjustment(&self) -> Option<gtk::Adjustment> {
            if let Some(adj) = self.vadjustment.borrow().clone() {
                return Some(adj);
            }
            let mut node: Option<gtk::Widget> = self.obj().parent();
            while let Some(w) = node {
                if let Some(sw) = w.downcast_ref::<gtk::ScrolledWindow>() {
                    let adj = sw.vadjustment();
                    let weak = self.obj().downgrade();
                    adj.connect_value_changed(move |adj| {
                        if let Some(canvas) = weak.upgrade() {
                            canvas.imp().note_scroll(adj);
                            canvas.queue_draw();
                        }
                    });
                    *self.vadjustment.borrow_mut() = Some(adj.clone());
                    return Some(adj);
                }
                node = w.parent();
            }
            None
        }

        fn note_scroll(&self, adj: &gtk::Adjustment) {
            let now = glib::monotonic_time();
            let (last_value, last_time) = self.last_scroll.replace((adj.value(), now));
            let fling = is_fling(adj.value() - last_value, now - last_time, adj.page_size());
            self.flinging.set(fling);
            if !fling {
                return;
            }
            if let Some(id) = self.settle.take() {
                id.remove();
            }
            let weak = self.obj().downgrade();
            let id = glib::timeout_add_local_once(SETTLE_DELAY, move || {
                if let Some(canvas) = weak.upgrade() {
                    let imp = canvas.imp();
                    imp.settle.take();
                    imp.flinging.set(false);
                    canvas.queue_draw();
                }
            });
            self.settle.replace(Some(id));
        }

        fn spawn_load(&self, request: LoadRequest, cancel: CancelFlag) {
            let Some(cache) = self.cache.get().cloned() else {
                return;
            };
            let Some(model) = self.model.get().cloned() else {
                return;
            };
            let widget = self.obj().clone();
            log::trace!(
                "masonry spawn_load id={} bucket={:?} local={}",
                request.asset_id,
                request.bucket,
                request.is_local
            );
            let painted_bucket = if request.is_local {
                ThumbnailSize::Thumbnail
            } else {
                request.bucket
            };
            let reveal = cached_texture(&cache, &request.asset_id, painted_bucket).is_none();
            glib::MainContext::default().spawn_local(async move {
                let result = load_masonry_asset(&cache, &request, &cancel).await;
                finish_masonry_load(&widget, &model, &request, result, reveal);
            });
        }
    }
}

use super::load::{collect_dims, load_with_fallback, propagate_dimensions};
mod helpers {
    include!("canvas_helpers.rs");
}
use helpers::{
    accent_bg_color, bucket_for_asset, cached_texture, load_masonry_asset, load_target_for,
    placeholder_color, resolve_symbolic_icon, resolve_video_icon, rows_between, scrolled_child,
};

glib::wrapper! {
    pub struct MasonryCanvas(ObjectSubclass<imp::MasonryCanvas>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl imp::SnapshotStats {
    fn record(&mut self, result: imp::PaintResult) {
        match result {
            imp::PaintResult::Hit => self.hits += 1,
            imp::PaintResult::Miss => self.misses += 1,
        }
        self.painted += 1;
    }
}

/// Explain a thumbnail permission error once per grid.
fn warn_missing_permission(widget: &MasonryCanvas) {
    let imp = widget.imp();
    if imp.permission_warned.replace(true) {
        return;
    }
    if let Some(window) = widget
        .root()
        .and_downcast::<libadwaita::ApplicationWindow>()
    {
        let dialog = libadwaita::AlertDialog::builder()
            .heading("Missing API Permissions")
            .body("Your API key is missing permissions required to view thumbnails. Please ensure the key has 'asset.view' enabled.")
            .build();
        dialog.add_response("ok", "OK");
        dialog.present(Some(&window));
    }
}

fn apply_masonry_load_result(
    widget: &MasonryCanvas,
    model: &LibraryAssetModel,
    request: &LoadRequest,
    result: &Result<gdk4::Texture, String>,
    reveal: bool,
) -> bool {
    let imp = widget.imp();
    let asset_id = request.asset_id.as_str();
    match result {
        Ok(tex) => {
            log::trace!(
                "masonry load OK id={} dims=({}x{})",
                asset_id,
                tex.width(),
                tex.height(),
            );
            if reveal && animations_enabled() {
                imp.start_reveal(asset_id);
            }
            propagate_dimensions(model, request.asset_index, asset_id, tex)
        }
        Err(e) if e == "cancelled" => {
            log::trace!("masonry load cancelled id={}", asset_id);
            false
        }
        Err(e) => {
            imp.failed.borrow_mut().insert(asset_id.to_string());
            if e.contains("HTTP 401") || e.contains("HTTP 403") {
                warn_missing_permission(widget);
            }
            log::warn!("masonry load ERR id={} err={}", asset_id, e);
            false
        }
    }
}

fn finish_masonry_load(
    widget: &MasonryCanvas,
    model: &LibraryAssetModel,
    request: &LoadRequest,
    result: Result<gdk4::Texture, String>,
    reveal: bool,
) {
    let imp = widget.imp();
    let asset_id = request.asset_id.as_str();
    let dims_changed = apply_masonry_load_result(widget, model, request, &result, reveal);
    imp.scheduler.borrow_mut().finish(asset_id);
    if dims_changed {
        imp.relayout_for_position(request.asset_index);
    }
    widget.queue_draw();
}
