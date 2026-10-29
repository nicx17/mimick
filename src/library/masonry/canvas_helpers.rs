use gtk::prelude::*;

use super::{
    AssetObject, CancelFlag, GridQuality, LaidItem, LaidRow, LibraryAssetModel, LoadRequest,
    MasonryCanvas, ThumbnailCache, ThumbnailSize, bucket_for_row_height, fallback_bucket,
    first_row_at_or_after, load_with_fallback,
};

/// Get the current theme accent background colour from libadwaita.
pub(super) fn scrolled_child(widget: &gtk::Widget) -> Option<gtk::Widget> {
    let mut node = Some(widget.clone());
    while let Some(current) = node {
        if let Some(scrolled) = current.downcast_ref::<gtk::ScrolledWindow>() {
            return scrolled.child();
        }
        node = current.parent();
    }
    None
}

pub(super) fn accent_bg_color() -> gdk4::RGBA {
    let accent = libadwaita::StyleManager::default().accent_color();
    accent.to_rgba()
}

pub(super) fn placeholder_color() -> gdk4::RGBA {
    if libadwaita::StyleManager::default().is_dark() {
        gdk4::RGBA::new(0.20, 0.20, 0.22, 1.0)
    } else {
        gdk4::RGBA::new(0.90, 0.90, 0.92, 1.0)
    }
}

/// Resolve the video badge icon from the icon theme. The result is cached
/// per-canvas via a `OnceCell` so the theme lookup only happens once.
pub(super) fn resolve_video_icon(widget: &MasonryCanvas) -> gdk4::Paintable {
    resolve_symbolic_icon(widget, "mimick-video-symbolic")
}

pub(super) fn resolve_symbolic_icon(widget: &MasonryCanvas, icon_name: &str) -> gdk4::Paintable {
    let display = widget
        .native()
        .map(|n| n.display())
        .unwrap_or_else(|| gtk::gdk::Display::default().expect("default GDK display"));
    let theme = gtk::IconTheme::for_display(&display);
    let icon = theme.lookup_icon(
        icon_name,
        &[],
        48,
        1,
        gtk::TextDirection::None,
        gtk::IconLookupFlags::FORCE_SYMBOLIC,
    );
    icon.upcast::<gdk4::Paintable>()
}

pub(super) fn bucket_for_asset(
    asset_id: &str,
    local_path: &str,
    row_h: f32,
    quality: GridQuality,
) -> ThumbnailSize {
    if !local_path.is_empty() && asset_id.starts_with(crate::library::LOCAL_ID_PREFIX) {
        ThumbnailSize::Thumbnail
    } else {
        bucket_for_row_height(row_h, quality)
    }
}

pub(super) fn cached_texture(
    cache: &ThumbnailCache,
    asset_id: &str,
    bucket: ThumbnailSize,
) -> Option<gdk4::Texture> {
    let get = |size| cache.get_cached(asset_id, size);
    get(bucket).or_else(|| fallback_bucket(bucket).and_then(get))
}

pub(super) fn rows_between(rows: &[LaidRow], top: f32, bottom: f32) -> impl Iterator<Item = &LaidRow> {
    rows[first_row_at_or_after(rows, top)..]
        .iter()
        .take_while(move |row| row.y <= bottom)
}

pub(super) fn load_target_for(model: &LibraryAssetModel, it: &LaidItem) -> Option<(String, String, bool)> {
    let asset = model.item(it.asset_index).and_downcast::<AssetObject>()?;
    let asset_id = asset.property::<String>("id");
    let local_path = asset.property::<String>("local-path");
    let is_local = !local_path.is_empty() && asset_id.starts_with(crate::library::LOCAL_ID_PREFIX);
    Some((asset_id, local_path, is_local))
}

pub(super) async fn load_masonry_asset(
    cache: &ThumbnailCache,
    request: &LoadRequest,
    cancel: &CancelFlag,
) -> Result<gdk4::Texture, String> {
    let is_cancelled = || cancel.get();
    if request.is_local {
        let path = std::path::PathBuf::from(&request.local_path);
        cache
            .load_local_thumbnail_cancellable(&request.asset_id, &path, &is_cancelled)
            .await
    } else {
        load_with_fallback(cache, &request.asset_id, request.bucket, &is_cancelled).await
    }
}

