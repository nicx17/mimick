//! Lightbox image viewer: full-screen preview with zoom, pan, EXIF details, and keyboard navigation.
//!
//! Loads preview or original resolution images with pinch-zoom and
//! swipe navigation. Displays an EXIF metadata panel and provides
//! download-to-folder and delete-to-trash actions.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{ExifInfo, ThumbnailSize};
use crate::library::asset_object::AssetObject;
use crate::library::local_exif::{self, LocalExif};

use super::context_menu::show_asset_context_menu;
use super::download::{
    begin_download_session, finish_download_item, open_local_with_default_app, spawn_video_handoff,
    start_download, track_download_item,
};
use super::{LOCAL_ID_PREFIX, LibraryWindowUi, load_source_page, load_texture_oriented};

/// Everything we can learn about a local file off the main thread before
/// handing the data back to GTK. `exif` is the cached EXIF parse; `dims`
/// comes from a pixbuf header-only read; `mtime_iso` falls back when the
/// file has no `DateTimeOriginal` so we always show *some* date.
struct LocalProbe {
    exif: Option<LocalExif>,
    file_size: Option<u64>,
    mtime_iso: Option<String>,
    dims: Option<(u32, u32)>,
}

/// Local-timezone RFC3339, so `format_datetime_display` treats it like an EXIF timestamp.
fn systemtime_to_rfc3339(t: std::time::SystemTime) -> String {
    use chrono::{DateTime, Local};
    let dt: DateTime<Local> = t.into();
    dt.to_rfc3339()
}

/// Convert local EXIF into the API shape so one renderer handles both sources.
fn exif_from_local(local: &LocalExif, file_size: Option<u64>) -> ExifInfo {
    ExifInfo {
        make: local.make.clone(),
        model: local.model.clone(),
        lens_model: local.lens_model.clone(),
        f_number: local.f_number,
        focal_length: local.focal_length,
        iso: local.iso,
        exposure_time: local.exposure_time.clone(),
        file_size_in_byte: file_size,
        date_time_original: local.date_time_original.clone(),
        city: None,
        state: None,
        country: None,
        latitude: local.latitude,
        longitude: local.longitude,
        description: local.description.clone(),
        exif_image_width: local.image_width,
        exif_image_height: local.image_height,
    }
}

/// Cache path for a downloaded original; keeps the extension so decoders pick the right format.
fn original_preview_cache_path(
    cache_dir: &std::path::Path,
    asset_id: &str,
    filename: &str,
) -> std::path::PathBuf {
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|ext| !ext.is_empty())
        .unwrap_or("bin");
    cache_dir.join(format!("{asset_id}.{ext}"))
}

/// Hard-link or copy a cached file under its original name so drag-out targets see that name.
/// Returns `source` if the copy fails, and `None` when there is no cache directory.
fn drag_export_path(
    asset_id: &str,
    filename: &str,
    source: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let export_dir = crate::profile::cache_dir()?.join("drag_export");
    let _ = std::fs::create_dir_all(&export_dir);
    let prefix = &asset_id[..8.min(asset_id.len())];
    let export = export_dir.join(format!("{prefix}_{filename}"));
    if export.exists() {
        return Some(export);
    }
    if std::fs::hard_link(source, &export).is_ok() || std::fs::copy(source, &export).is_ok() {
        Some(export)
    } else {
        Some(source.to_path_buf())
    }
}

/// Populate the details sidebar with sectioned EXIF metadata.
///
/// Groups fall into three categories — Camera, Image, Location — each rendered
/// as an `AdwPreferencesGroup` with accent-coloured prefix icons. Empty groups
/// (e.g. an image with no GPS) are skipped so the pane stays compact.
///
/// `taken_label` decides whether the date row reads "Taken" (EXIF
/// DateTimeOriginal — the camera's capture moment) or "Modified" (filesystem
/// mtime fallback when no capture timestamp exists).
fn fill_exif_box(container: &gtk::Box, exif: &crate::api_client::ExifInfo, taken_label: &str) {
    if let Some(group) = build_camera_group(exif) {
        container.append(&group);
    }
    if let Some(group) = build_image_group(exif, taken_label) {
        container.append(&group);
    }
    if let Some(group) = build_location_group(exif) {
        container.append(&group);
    }
    if let Some(desc) = &exif.description
        && !desc.trim().is_empty()
    {
        let note_group = libadwaita::PreferencesGroup::builder()
            .title("Description")
            .build();
        let row = libadwaita::ActionRow::builder()
            .title(desc.as_str())
            .title_lines(0)
            .css_classes(["property"])
            .build();
        note_group.add(&row);
        container.append(&note_group);
    }
}

fn build_camera_group(exif: &crate::api_client::ExifInfo) -> Option<libadwaita::PreferencesGroup> {
    let group = libadwaita::PreferencesGroup::builder()
        .title("Camera")
        .build();
    let mut rows = 0u32;
    if let Some(c) = format_camera(exif) {
        group.add(&accent_row(
            "camera-photo-symbolic",
            "mimick-accent-camera",
            "Body",
            &c,
        ));
        rows += 1;
    }
    if let Some(l) = &exif.lens_model
        && !l.trim().is_empty()
    {
        group.add(&accent_row(
            "view-fullscreen-symbolic",
            "mimick-accent-camera",
            "Lens",
            l,
        ));
        rows += 1;
    }
    if let Some(exposure) = format_exposure(exif) {
        group.add(&accent_row(
            "weather-clear-symbolic",
            "mimick-accent-camera",
            "Exposure",
            &exposure,
        ));
        rows += 1;
    }
    (rows > 0).then_some(group)
}

fn build_image_group(
    exif: &crate::api_client::ExifInfo,
    taken_label: &str,
) -> Option<libadwaita::PreferencesGroup> {
    let group = libadwaita::PreferencesGroup::builder()
        .title("Image")
        .build();
    let mut rows = 0u32;
    if let (Some(w), Some(h)) = (exif.exif_image_width, exif.exif_image_height) {
        group.add(&accent_row(
            "view-grid-symbolic",
            "mimick-accent-image",
            "Dimensions",
            &format!("{w} × {h}"),
        ));
        rows += 1;
    }
    if let Some(size) = exif.file_size_in_byte {
        group.add(&accent_row(
            "drive-harddisk-symbolic",
            "mimick-accent-image",
            "Size",
            &format_bytes(size),
        ));
        rows += 1;
    }
    if let Some(dt) = &exif.date_time_original
        && !dt.trim().is_empty()
    {
        group.add(&accent_row(
            "x-office-calendar-symbolic",
            "mimick-accent-image",
            taken_label,
            &format_datetime_display(dt),
        ));
        rows += 1;
    }
    (rows > 0).then_some(group)
}

fn build_location_group(
    exif: &crate::api_client::ExifInfo,
) -> Option<libadwaita::PreferencesGroup> {
    let group = libadwaita::PreferencesGroup::builder()
        .title("Location")
        .build();
    let mut rows = 0u32;
    if let Some(loc) = format_location(exif) {
        group.add(&accent_row(
            "mark-location-symbolic",
            "mimick-accent-location",
            "Place",
            &loc,
        ));
        rows += 1;
    }
    if let (Some(lat), Some(lon)) = (exif.latitude, exif.longitude) {
        group.add(&accent_row(
            "find-location-symbolic",
            "mimick-accent-location",
            "Coordinates",
            &format!("{lat:.5}, {lon:.5}"),
        ));
        rows += 1;
    }
    (rows > 0).then_some(group)
}

fn accent_row(icon: &str, accent_class: &str, title: &str, value: &str) -> libadwaita::ActionRow {
    let prefix = gtk::Image::builder()
        .icon_name(icon)
        .pixel_size(16)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .css_classes(["mimick-detail-icon", accent_class])
        .build();
    let row = libadwaita::ActionRow::builder()
        .title(title)
        .subtitle(value)
        .subtitle_lines(2)
        .css_classes(["property"])
        .build();
    row.add_prefix(&prefix);
    row
}

fn format_camera(exif: &crate::api_client::ExifInfo) -> Option<String> {
    match (&exif.make, &exif.model) {
        (Some(m), Some(n)) => {
            let m = m.trim();
            let n = n.trim();
            if n.starts_with(m) {
                Some(n.to_string())
            } else {
                Some(format!("{m} {n}"))
            }
        }
        (Some(m), None) => Some(m.trim().to_string()),
        (None, Some(n)) => Some(n.trim().to_string()),
        _ => None,
    }
    .filter(|s| !s.is_empty())
}

fn format_exposure(exif: &crate::api_client::ExifInfo) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(f) = exif.f_number {
        parts.push(format!("ƒ/{f:.1}"));
    }
    if let Some(et) = &exif.exposure_time
        && !et.trim().is_empty()
    {
        parts.push(et.trim().to_string());
    }
    if let Some(iso) = exif.iso {
        parts.push(format!("ISO {iso}"));
    }
    if let Some(focal) = exif.focal_length {
        parts.push(format!("{focal:.0}mm"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn format_location(exif: &crate::api_client::ExifInfo) -> Option<String> {
    let parts: Vec<&str> = [&exif.city, &exif.state, &exif.country]
        .into_iter()
        .filter_map(|s| s.as_deref().map(str::trim).filter(|t| !t.is_empty()))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// Format a byte count value into a human-readable size string (e.g. KB, MB, GB).
fn format_bytes(n: u64) -> String {
    const KIB: f64 = 1024.0;
    let n_f = n as f64;
    if n_f >= KIB * KIB * KIB {
        format!("{:.2} GB", n_f / (KIB * KIB * KIB))
    } else if n_f >= KIB * KIB {
        format!("{:.2} MB", n_f / (KIB * KIB))
    } else if n_f >= KIB {
        format!("{:.1} KB", n_f / KIB)
    } else {
        format!("{} B", n)
    }
}

/// Format an ISO 8601 timestamp for display, converting from UTC to the
/// user's local timezone.
///
/// Immich normalises `date_time_original` and `fileCreatedAt` to UTC before
/// storage, so a photo taken at 19:55:15+05:30 is stored as
/// 2024-01-15T14:25:15.000Z. We parse the UTC value and convert it to the
/// system's local timezone so the displayed time matches what the camera
/// originally recorded. Falls back to the raw string if parsing fails.
fn format_datetime_display(iso: &str) -> String {
    use chrono::{DateTime, Local, Utc};
    // Try offset-aware parse first (handles +05:30, Z, etc.)
    if let Ok(dt) = DateTime::parse_from_rfc3339(iso) {
        let local: DateTime<Local> = dt.into();
        return local.format("%Y-%m-%d %H:%M:%S UTC%:z").to_string();
    }
    // Fallback: try treating as UTC
    if let Ok(dt) = iso.parse::<DateTime<Utc>>() {
        let local: DateTime<Local> = dt.into();
        return local.format("%Y-%m-%d %H:%M:%S UTC%:z").to_string();
    }
    // Last resort: strip trailing fractional seconds / timezone suffix
    iso.get(..19).unwrap_or(iso).replace('T', " ").to_string()
}

/// Truncate a filename to a maximum character limit, appending an ellipsis if needed.
fn truncate_filename(name: &str, max_chars: usize) -> String {
    let count = name.chars().count();
    if count <= max_chars {
        return name.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let head: String = name.chars().take(keep).collect();
    format!("{}…", head)
}

/// Apply zoom to a lightbox Picture. Zoom is fit-relative: 1.0 = the size the
/// texture would occupy inside `viewer` under Contain layout. >1.0 overflows
/// the viewer for panning. Returns the computed content dimensions when zoomed.
fn apply_lightbox_zoom(
    picture: &gtk::Picture,
    viewer: &gtk::ScrolledWindow,
    zoom: f64,
) -> Option<(f64, f64)> {
    if (zoom - 1.0).abs() < 0.001 {
        picture.set_size_request(-1, -1);
        return None;
    }
    let Some(paintable) = picture.paintable() else {
        picture.set_size_request(-1, -1);
        return None;
    };
    let nw = paintable.intrinsic_width().max(1) as f64;
    let nh = paintable.intrinsic_height().max(1) as f64;
    let viewer_w = viewer.width().max(1) as f64;
    let viewer_h = viewer.height().max(1) as f64;
    let texture_aspect = nw / nh;
    let viewer_aspect = viewer_w / viewer_h;
    let (fit_w, fit_h) = if viewer_aspect > texture_aspect {
        (viewer_h * texture_aspect, viewer_h)
    } else {
        (viewer_w, viewer_w / texture_aspect)
    };
    let cw = fit_w * zoom;
    let ch = fit_h * zoom;
    picture.set_size_request(cw as i32, ch as i32);
    Some((cw, ch))
}

/// One request to load an asset into the inactive picture.
struct PictureLoad {
    target: gtk::Picture,
    target_is_a: bool,
    asset_id: String,
    filename: String,
    mime: String,
    local_path: String,
    full_res: bool,
    is_video: bool,
}

/// What a finished load shows; applied only if the user is still on that asset.
enum LoadResult {
    /// Texture plus the file that drag-out should export.
    Texture(gdk4::Texture, Option<std::path::PathBuf>),
    /// "Preview unavailable" card; a path enables "Open in external app".
    Unavailable(Option<String>),
    /// Video poster failed; keep whatever cached thumbnail is showing.
    KeepPoster,
}

/// Grid item properties the lightbox renders.
struct AssetInfo {
    asset_id: String,
    filename: String,
    local_path: String,
    mime: String,
    created: String,
    sync_state: u32,
}

impl AssetInfo {
    fn from_item(item: &AssetObject) -> Self {
        Self {
            asset_id: item.property("id"),
            filename: item.property("filename"),
            local_path: item.property("local-path"),
            mime: item.property("mime-type"),
            created: item.property("created-at"),
            sync_state: item.property("sync-state"),
        }
    }

    fn is_video(&self) -> bool {
        crate::media_kinds::asset_kind(&self.mime) == crate::media_kinds::AssetKind::Video
    }

    fn is_local(&self) -> bool {
        !self.local_path.is_empty() && self.asset_id.starts_with(LOCAL_ID_PREFIX)
    }
}

/// Lightbox page widgets plus the state shared by its signal handlers.
/// Handlers hold strong `Rc`s, so it lives as long as the page's widgets.
struct Lightbox {
    ui: Rc<LibraryWindowUi>,
    header: LightboxHeader,
    picture: PictureArea,
    unavailable: UnavailableCard,
    video_badge_button: gtk::Button,
    actions: ActionBar,
    details: DetailsPane,
    pos: Cell<u32>,
    // Increments on every navigation. Async load tasks capture the generation
    // they were started for and skip UI writes if the user has navigated away
    // by the time their decode finishes (relevant for slow RAW files).
    load_gen: Cell<u64>,
    active_a: Cell<bool>,
    zoom_level: Cell<f64>,
    // -1 = back/prev (slide right), +1 = forward/next (slide left), 0 = no transition
    nav_dir: Cell<i8>,
    // Cursor position over the picture area, so zoom can be focal-point aware.
    // None when the cursor is outside the viewer; zoom then uses the centre.
    cursor_pos: Cell<Option<(f64, f64)>>,
    drag_start: Cell<(f64, f64)>,
    pinch_start: Cell<f64>,
    unavailable_path: RefCell<Option<String>>,
    video_badge_target: RefCell<Option<(String, String, String)>>,
    // Original file exported by drag-out. Updated by the load logic whenever an
    // asset is displayed (from its local path or the preview cache).
    drag_path: RefCell<Option<std::path::PathBuf>>,
}

/// Construct and present the fullscreen lightbox view for a selected asset.
pub(super) fn open_lightbox(ui: Rc<LibraryWindowUi>, position: u32) {
    let Some(item) = ui.grid.model.item(position).and_downcast::<AssetObject>() else {
        return;
    };
    let lightbox = Lightbox::new(ui, position, &item.property::<String>("filename"));
    lightbox.connect_overlays();
    lightbox.connect_navigation();
    lightbox.connect_zoom_controls();
    lightbox.connect_pointer_gestures();
    lightbox.connect_keys();
    lightbox.connect_actions();
    lightbox.render();
    lightbox.ui.nav.push(&lightbox.header.page);
}

fn lightbox_picture() -> gtk::Picture {
    gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Contain)
        .vexpand(true)
        .hexpand(true)
        .css_classes(["mimick-lightbox-picture"])
        .build()
}

fn crossfade_overlay(child: &impl IsA<gtk::Widget>, align: gtk::Align) -> gtk::Revealer {
    gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(180)
        .reveal_child(false)
        .halign(align)
        .valign(align)
        .child(child)
        .can_target(false)
        .build()
}

fn details_text_label() -> gtk::Label {
    gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(28)
        .build()
}

/// Title length that still fits in the header beside the nav buttons.
fn lightbox_title_cap(ui: &LibraryWindowUi) -> usize {
    if ui.split.is_collapsed() { 14 } else { 24 }
}

/// Navigation page with its header bar (back, prev/next, details toggle).
struct LightboxHeader {
    page: libadwaita::NavigationPage,
    toolbar: libadwaita::ToolbarView,
    prev_btn: gtk::Button,
    next_btn: gtk::Button,
    details_btn: gtk::ToggleButton,
}

/// Two pictures in a sliding stack, their scroller, and the overlay holding the spinner.
struct PictureArea {
    stack: gtk::Stack,
    picture_a: gtk::Picture,
    picture_b: gtk::Picture,
    scrolled: gtk::ScrolledWindow,
    overlay: gtk::Overlay,
    loader: gtk::Revealer,
}

/// "Preview unavailable" card shown over the picture when nothing can be decoded.
struct UnavailableCard {
    overlay: gtk::Revealer,
    filename: gtk::Label,
    mime: gtk::Label,
    open: gtk::Button,
}

/// Zoom controls, resolution toggle, and download button under the picture.
struct ActionBar {
    bar: gtk::Box,
    resolution_toggle: gtk::ToggleButton,
    download: gtk::Button,
    zoom_group: gtk::Box,
    zoom_in: gtk::Button,
    zoom_out: gtk::Button,
    zoom_reset: gtk::Button,
}

/// Sidebar with the filename, sync summary, and EXIF groups.
struct DetailsPane {
    pane: gtk::ScrolledWindow,
    filename: gtk::Label,
    summary: gtk::Label,
    loading: gtk::Label,
    exif: gtk::Box,
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build()
}

fn build_header(ui: &Rc<LibraryWindowUi>, initial_filename: &str) -> LightboxHeader {
    let page = libadwaita::NavigationPage::builder()
        .title(truncate_filename(initial_filename, lightbox_title_cap(ui)))
        .can_pop(true)
        .build();
    let toolbar = libadwaita::ToolbarView::builder().build();
    let header = libadwaita::HeaderBar::builder()
        .show_back_button(false)
        .build();
    let back_btn = icon_button("mimick-library-symbolic", "Back to library");
    back_btn.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| {
            ui.nav.pop();
        }
    ));
    let prev_btn = icon_button("go-previous-symbolic", "Previous (Left)");
    let next_btn = icon_button("go-next-symbolic", "Next (Right)");
    let details_btn = gtk::ToggleButton::builder()
        .icon_name("dialog-information-symbolic")
        .tooltip_text("Toggle details (I)")
        .active(false)
        .build();
    header.pack_start(&back_btn);
    header.pack_start(&prev_btn);
    header.pack_start(&next_btn);
    header.pack_end(&details_btn);
    toolbar.add_top_bar(&header);
    page.set_child(Some(&toolbar));
    LightboxHeader {
        page,
        toolbar,
        prev_btn,
        next_btn,
        details_btn,
    }
}

fn build_picture_area() -> PictureArea {
    // Two picture widgets in a stack so navigation can slide between them.
    let picture_a = lightbox_picture();
    let picture_b = lightbox_picture();
    let stack = gtk::Stack::builder()
        .transition_duration(180)
        .vexpand(true)
        .hexpand(true)
        .build();
    stack.add_named(&picture_a, Some("a"));
    stack.add_named(&picture_b, Some("b"));
    stack.set_visible_child_name("a");
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&stack)
        .vexpand(true)
        .hexpand(true)
        .kinetic_scrolling(false)
        .min_content_width(120)
        .build();

    // Spinner overlay: a centered Mimick app icon that rotates while a
    // full-resolution texture is being fetched / decoded. Hidden by default;
    // `load_into_picture` reveals it after a short delay.
    let loader_icon = gtk::Image::builder()
        .icon_name("dev.nicx.mimick")
        .pixel_size(72)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["mimick-loader-icon"])
        .build();
    let loader = crossfade_overlay(&loader_icon, gtk::Align::Center);
    let overlay = gtk::Overlay::builder().build();
    overlay.set_child(Some(&scrolled));
    overlay.add_overlay(&loader);
    PictureArea {
        stack,
        picture_a,
        picture_b,
        scrolled,
        overlay,
        loader,
    }
}

fn build_unavailable_card() -> UnavailableCard {
    let title = gtk::Label::builder()
        .label("Preview unavailable")
        .css_classes(["title-3"])
        .build();
    let filename = gtk::Label::builder()
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(42)
        .build();
    let mime = gtk::Label::builder().css_classes(["dim-label"]).build();
    let open = gtk::Button::builder()
        .label("Open in external app")
        .css_classes(["suggested-action"])
        .build();
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["mimick-preview-unavailable"])
        .build();
    card.append(&title);
    card.append(&filename);
    card.append(&mime);
    card.append(&open);
    UnavailableCard {
        overlay: crossfade_overlay(&card, gtk::Align::Fill),
        filename,
        mime,
        open,
    }
}

// Video poster badge: clickable play icon shown over the still thumbnail
// when the current asset is a video; clicking hands off to an external player.
fn build_video_badge() -> gtk::Button {
    let icon = gtk::Image::builder()
        .icon_name("mimick-video-symbolic")
        .pixel_size(72)
        .css_classes(vec!["mimick-video-badge".to_string()])
        .build();
    gtk::Button::builder()
        .child(&icon)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .tooltip_text("Play video in external player")
        .css_classes(vec!["circular".to_string(), "flat".to_string()])
        .visible(false)
        .build()
}

fn build_action_bar(initial_full: bool) -> ActionBar {
    let resolution_toggle = gtk::ToggleButton::builder()
        .label(if initial_full { "Raw" } else { "Prev" })
        .tooltip_text("Toggle preview vs original full-resolution image")
        .active(initial_full)
        .build();
    let download = icon_button("mimick-download-symbolic", "Download asset");
    let zoom_out = icon_button("zoom-out-symbolic", "Zoom out (Ctrl+-)");
    let zoom_in = icon_button("zoom-in-symbolic", "Zoom in (Ctrl++)");
    let zoom_reset = gtk::Button::builder()
        .label("100%")
        .tooltip_text("Reset zoom (Ctrl+0)")
        .build();
    let zoom_group = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(vec!["linked".to_string()])
        .build();
    zoom_group.append(&zoom_out);
    zoom_group.append(&zoom_reset);
    zoom_group.append(&zoom_in);
    let bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .build();
    let spacer = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .build();
    bar.append(&zoom_group);
    bar.append(&spacer);
    bar.append(&resolution_toggle);
    bar.append(&download);
    ActionBar {
        bar,
        resolution_toggle,
        download,
        zoom_group,
        zoom_in,
        zoom_out,
        zoom_reset,
    }
}

fn build_details_pane() -> DetailsPane {
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .margin_top(14)
        .margin_bottom(14)
        .margin_start(10)
        .margin_end(10)
        .build();
    let pane = gtk::ScrolledWindow::builder()
        .child(&inner)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(false)
        .min_content_width(180)
        .max_content_width(320)
        .css_classes(vec!["mimick-details-pane".to_string()])
        .build();
    let filename = details_text_label();
    filename.add_css_class("title-3");
    let summary = details_text_label();
    let loading = gtk::Label::builder()
        .xalign(0.0)
        .label("Loading details…")
        .css_classes(vec!["dim-label".to_string()])
        .build();
    let exif = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .visible(false)
        .build();
    inner.append(&filename);
    inner.append(&summary);
    inner.append(&loading);
    inner.append(&exif);
    DetailsPane {
        pane,
        filename,
        summary,
        loading,
        exif,
    }
}

fn build_viewer(picture: &PictureArea, actions: &ActionBar) -> gtk::Box {
    let viewer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(4)
        .margin_bottom(8)
        .margin_start(4)
        .margin_end(4)
        .hexpand(true)
        .build();
    viewer.append(&picture.overlay);
    viewer.append(&actions.bar);
    viewer
}

/// Picture viewer with the details pane as a sidebar that follows the window's narrow layout.
fn build_split_view(
    ui: &LibraryWindowUi,
    viewer: &gtk::Box,
    details: &DetailsPane,
    details_btn: &gtk::ToggleButton,
) -> libadwaita::OverlaySplitView {
    let body = libadwaita::OverlaySplitView::builder()
        .sidebar_position(gtk::PackType::End)
        .show_sidebar(false)
        .collapsed(ui.split.is_collapsed())
        .enable_show_gesture(true)
        .enable_hide_gesture(true)
        .min_sidebar_width(180.0)
        .max_sidebar_width(320.0)
        .sidebar_width_fraction(0.4)
        .build();
    body.set_content(Some(viewer));
    body.set_sidebar(Some(&details.pane));
    details_btn
        .bind_property("active", &body, "show-sidebar")
        .sync_create()
        .bidirectional()
        .build();
    ui.split
        .bind_property("collapsed", &body, "collapsed")
        .sync_create()
        .build();
    body
}

impl Lightbox {
    /// Build the page's widget tree; signal handlers are connected by the `connect_*` methods.
    fn new(ui: Rc<LibraryWindowUi>, position: u32, initial_filename: &str) -> Rc<Self> {
        let header = build_header(&ui, initial_filename);
        let picture = build_picture_area();
        let unavailable = build_unavailable_card();
        picture.overlay.add_overlay(&unavailable.overlay);
        let video_badge_button = build_video_badge();
        picture.overlay.add_overlay(&video_badge_button);

        let initial_full = ui.ctx.config.read().data.library_preview_full_resolution;
        let actions = build_action_bar(initial_full);
        let details = build_details_pane();
        let viewer = build_viewer(&picture, &actions);
        let body = build_split_view(&ui, &viewer, &details, &header.details_btn);
        header.toolbar.set_content(Some(&body));

        Rc::new(Self {
            ui,
            header,
            picture,
            unavailable,
            video_badge_button,
            actions,
            details,
            pos: Cell::new(position),
            load_gen: Cell::new(0),
            active_a: Cell::new(true),
            zoom_level: Cell::new(1.0),
            nav_dir: Cell::new(0),
            cursor_pos: Cell::new(None),
            drag_start: Cell::new((0.0, 0.0)),
            pinch_start: Cell::new(1.0),
            unavailable_path: RefCell::new(None),
            video_badge_target: RefCell::new(None),
            drag_path: RefCell::new(None),
        })
    }

    /// Wire the external-open button, the video play badge, and the drag-out source.
    fn connect_overlays(self: &Rc<Self>) {
        let lb = self.clone();
        self.unavailable.open.connect_clicked(move |_| {
            if let Some(path) = lb.unavailable_path.borrow().as_deref() {
                open_local_with_default_app(path);
            }
        });

        let lb = self.clone();
        self.video_badge_button.connect_clicked(move |_| {
            let Some((local_path, asset_id, filename)) = lb.video_badge_target.borrow().clone()
            else {
                return;
            };
            if !local_path.is_empty() {
                open_local_with_default_app(&local_path);
            } else {
                spawn_video_handoff(lb.ui.clone(), asset_id, filename);
            }
        });

        let drag_source = gtk::DragSource::new();
        drag_source.set_actions(gtk::gdk::DragAction::COPY);
        let lb = self.clone();
        drag_source.connect_prepare(move |_source, _x, _y| {
            let path = lb.drag_path.borrow().clone()?;
            if !path.exists() {
                return None;
            }
            let file = gtk::gio::File::for_path(&path);
            Some(gtk::gdk::ContentProvider::for_value(&file.to_value()))
        });
        self.picture.overlay.add_controller(drag_source);
    }

    /// Prev/next buttons, hidden on narrow layouts.
    fn connect_navigation(self: &Rc<Self>) {
        // On narrow widths, hide the prev/next header buttons -- Left/Right
        // keyboard shortcuts still work, and the saved space lets the title fit.
        // The details toggle stays visible so users can still reach the EXIF pane.
        let sync_nav_visibility = {
            let prev_btn = self.header.prev_btn.clone();
            let next_btn = self.header.next_btn.clone();
            let split = self.ui.split.clone();
            move || {
                let show = !split.is_collapsed();
                prev_btn.set_visible(show);
                next_btn.set_visible(show);
            }
        };
        sync_nav_visibility();
        self.ui
            .split
            .connect_notify_local(Some("collapsed"), move |_, _| sync_nav_visibility());

        let lb = self.clone();
        self.header
            .prev_btn
            .connect_clicked(move |_| lb.goto_prev());
        let lb = self.clone();
        self.header
            .next_btn
            .connect_clicked(move |_| lb.goto_next());
    }

    /// Zoom buttons, pinch, drag-to-pan, double/middle click, and Ctrl+scroll.
    fn connect_zoom_controls(self: &Rc<Self>) {
        let lb = self.clone();
        self.actions
            .zoom_in
            .connect_clicked(move |_| lb.zoom_by(1.2));
        let lb = self.clone();
        self.actions
            .zoom_out
            .connect_clicked(move |_| lb.zoom_by(1.0 / 1.2));
        let lb = self.clone();
        self.actions
            .zoom_reset
            .connect_clicked(move |_| lb.zoom_reset());

        self.connect_pan_and_pinch();

        // Double-click on the picture: zoom in 2x toward the click position.
        let double_click = gtk::GestureClick::new();
        double_click.set_button(gtk::gdk::BUTTON_PRIMARY);
        let lb = self.clone();
        double_click.connect_pressed(move |_, n_press, x, y| {
            if n_press == 2 {
                lb.cursor_pos.set(Some((x, y)));
                lb.set_zoom(lb.zoom_level.get() * 2.0);
            }
        });
        self.picture.scrolled.add_controller(double_click);

        // Middle-click: reset zoom to 100%.
        let middle_click = gtk::GestureClick::new();
        middle_click.set_button(gtk::gdk::BUTTON_MIDDLE);
        let lb = self.clone();
        middle_click.connect_pressed(move |_, _, _, _| lb.zoom_reset());
        self.picture.scrolled.add_controller(middle_click);

        // Ctrl+wheel zoom on the picture area, captured before the scrolled window
        // can use it for panning. Listening on both axes so trackpad two-finger
        // scrolls (which sometimes emit horizontal deltas) still trigger zoom.
        let zoom_scroll =
            gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        zoom_scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        let lb = self.clone();
        zoom_scroll.connect_scroll(move |ctrl, dx, dy| lb.handle_zoom_scroll(ctrl, dx, dy));
        self.picture.scrolled.add_controller(zoom_scroll);
    }

    /// Pinch-to-zoom and click-and-drag panning, grouped so they don't fight over touches.
    fn connect_pan_and_pinch(self: &Rc<Self>) {
        // Trackpad pinch-to-zoom. On the picture scroller so it shares a stable
        // coordinate frame with drag (see drag comment below).
        let pinch = gtk::GestureZoom::new();
        let lb = self.clone();
        pinch.connect_begin(move |_, _| lb.pinch_start.set(lb.zoom_level.get()));
        let lb = self.clone();
        pinch.connect_scale_changed(move |_, scale| lb.set_zoom(lb.pinch_start.get() * scale));
        self.picture.scrolled.add_controller(pinch.clone());

        // Click-and-drag panning when zoomed in. Attached to the scroller, not the
        // picture stack: the stack moves under the cursor when we update the scroll
        // adjustments, so an offset measured relative to it oscillates frame-to-frame
        // and jitters the image.
        let drag = gtk::GestureDrag::new();
        drag.set_button(gtk::gdk::BUTTON_PRIMARY);
        let lb = self.clone();
        drag.connect_drag_begin(move |_, _, _| {
            let hadj = lb.picture.scrolled.hadjustment();
            let vadj = lb.picture.scrolled.vadjustment();
            lb.drag_start.set((hadj.value(), vadj.value()));
        });
        let lb = self.clone();
        drag.connect_drag_update(move |_, off_x, off_y| {
            let (sx0, sy0) = lb.drag_start.get();
            lb.picture.scrolled.hadjustment().set_value(sx0 - off_x);
            lb.picture.scrolled.vadjustment().set_value(sy0 - off_y);
        });
        self.picture.scrolled.add_controller(drag.clone());
        drag.group_with(&pinch);
    }

    /// Cursor tracking for focal zoom, the right-click menu, and swipe navigation.
    fn connect_pointer_gestures(self: &Rc<Self>) {
        let motion = gtk::EventControllerMotion::new();
        let lb = self.clone();
        motion.connect_motion(move |_, x, y| lb.cursor_pos.set(Some((x, y))));
        let lb = self.clone();
        motion.connect_leave(move |_| lb.cursor_pos.set(None));
        self.picture.scrolled.add_controller(motion);

        // Right-click: open the standard asset context menu.
        let right_click = gtk::GestureClick::new();
        right_click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let lb = self.clone();
        right_click.connect_pressed(move |_, _, x, y| {
            show_asset_context_menu(lb.ui.clone(), &lb.picture.scrolled, lb.pos.get(), x, y);
        });
        self.picture.scrolled.add_controller(right_click);

        // Horizontal swipe for prev/next navigation; ignored when zoomed in.
        let swipe = gtk::GestureSwipe::new();
        swipe.set_touch_only(false);
        let lb = self.clone();
        swipe.connect_swipe(move |_, vx, _vy| lb.handle_swipe(vx));
        self.picture.stack.add_controller(swipe);
    }

    /// Ctrl +/-/0 zoom, Left/Right navigate, I toggles details, Escape closes.
    fn connect_keys(self: &Rc<Self>) {
        let key_controller = gtk::EventControllerKey::new();
        let lb = self.clone();
        key_controller.connect_key_pressed(move |_, key, _, mods| lb.handle_key(key, mods));
        self.header.page.add_controller(key_controller);
    }

    /// Download button and the preview/original resolution toggle.
    fn connect_actions(self: &Rc<Self>) {
        let lb = self.clone();
        self.actions.download.connect_clicked(move |_| {
            let Some(item) = lb
                .ui
                .grid
                .model
                .item(lb.pos.get())
                .and_downcast::<AssetObject>()
            else {
                return;
            };
            let asset_id = item.property::<String>("id");
            if !asset_id.starts_with(LOCAL_ID_PREFIX) {
                start_download(lb.ui.clone(), asset_id, item.property("filename"));
            }
        });

        let lb = self.clone();
        self.actions.resolution_toggle.connect_toggled(move |btn| {
            btn.set_label(if btn.is_active() { "Raw" } else { "Prev" });
            lb.render();
        });
    }

    fn handle_swipe(self: &Rc<Self>, vx: f64) {
        // Ignore swipes when zoomed in — those should pan instead.
        if (self.zoom_level.get() - 1.0).abs() > 0.01 {
            return;
        }
        // vx < 0 means finger moved left → go to next asset.
        // vx > 0 means finger moved right → go to previous asset.
        const MIN_VELOCITY: f64 = 50.0;
        if vx < -MIN_VELOCITY {
            self.goto_next();
        } else if vx > MIN_VELOCITY {
            self.goto_prev();
        }
    }

    fn handle_key(
        self: &Rc<Self>,
        key: gtk::gdk::Key,
        mods: gtk::gdk::ModifierType,
    ) -> glib::Propagation {
        let ctrl = mods.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        match (ctrl, key) {
            (true, gtk::gdk::Key::plus)
            | (true, gtk::gdk::Key::equal)
            | (true, gtk::gdk::Key::KP_Add) => self.zoom_by(1.2),
            (true, gtk::gdk::Key::minus) | (true, gtk::gdk::Key::KP_Subtract) => {
                self.zoom_by(1.0 / 1.2)
            }
            (true, gtk::gdk::Key::_0) | (true, gtk::gdk::Key::KP_0) => self.zoom_reset(),
            (false, gtk::gdk::Key::Left) => self.goto_prev(),
            (false, gtk::gdk::Key::Right) => self.goto_next(),
            (false, gtk::gdk::Key::i) | (false, gtk::gdk::Key::I) => self
                .header
                .details_btn
                .set_active(!self.header.details_btn.is_active()),
            (false, gtk::gdk::Key::Escape) => {
                self.ui.nav.pop();
            }
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// Zoom on Ctrl+scroll; plain scrolling falls through to panning.
    fn handle_zoom_scroll(
        &self,
        ctrl: &gtk::EventControllerScroll,
        dx: f64,
        dy: f64,
    ) -> glib::Propagation {
        let mods = ctrl.current_event_state();
        if !mods.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
            return glib::Propagation::Proceed;
        }
        let delta = if dy != 0.0 { dy } else { dx };
        if delta == 0.0 {
            return glib::Propagation::Proceed;
        }
        self.zoom_by(if delta < 0.0 { 1.1 } else { 1.0 / 1.1 });
        glib::Propagation::Stop
    }

    fn goto_prev(self: &Rc<Self>) {
        let pos = self.pos.get();
        if pos > 0 {
            self.pos.set(pos - 1);
            self.nav_dir.set(-1);
            self.render();
        }
    }

    /// Step forward, fetching the next grid page first when at the end.
    fn goto_next(self: &Rc<Self>) {
        let pos = self.pos.get();
        if pos + 1 < self.ui.grid.model.n_items() {
            self.pos.set(pos + 1);
            self.nav_dir.set(1);
            self.render();
            return;
        }
        let next_request = self.ui.ctx.library_state.lock().load_next_page_if_needed();
        let Some(req) = next_request else {
            return;
        };
        self.header.next_btn.set_sensitive(false);
        self.advance_when_page_loads();
        load_source_page(self.ui.clone(), req, true);
    }

    /// Advance once the next page lands in the grid model, then drop the handler.
    fn advance_when_page_loads(self: &Rc<Self>) {
        let model = self.ui.grid.model.clone();
        let prev_count = model.n_items();
        // One-shot handler: it disconnects itself once the new page has arrived.
        let handler_id = Rc::new(RefCell::new(None::<glib::SignalHandlerId>));
        let handler_id_clone = handler_id.clone();
        let lb = self.clone();
        let id = model.connect_items_changed(move |m, _, _, _| {
            if m.n_items() <= prev_count {
                return;
            }
            let pos = lb.pos.get();
            if pos + 1 < m.n_items() {
                lb.pos.set(pos + 1);
                lb.nav_dir.set(1);
                lb.render();
            }
            lb.header.next_btn.set_sensitive(true);
            if let Some(hid) = handler_id_clone.borrow_mut().take() {
                m.disconnect(hid);
            }
        });
        *handler_id.borrow_mut() = Some(id);
    }

    fn active_picture(&self) -> gtk::Picture {
        if self.active_a.get() {
            self.picture.picture_a.clone()
        } else {
            self.picture.picture_b.clone()
        }
    }

    fn zoom_by(&self, factor: f64) {
        self.set_zoom(self.zoom_level.get() * factor);
    }

    fn zoom_reset(&self) {
        self.set_zoom(1.0);
    }

    /// Zoom to `z` (clamped to 1–10×), keeping the focal point under the cursor.
    fn set_zoom(&self, z: f64) {
        let z_new = z.clamp(1.0, 10.0);
        let z_old = self.zoom_level.get();
        let zoom_label = format!("{}%", (z_new * 100.0).round() as i32);
        if (z_new - z_old).abs() < 0.0001 {
            self.actions.zoom_reset.set_label(&zoom_label);
            return;
        }

        // Pick the focal point: cursor if inside the viewer, else centre.
        let scrolled = &self.picture.scrolled;
        let viewer_w = scrolled.width().max(1) as f64;
        let viewer_h = scrolled.height().max(1) as f64;
        let (fx, fy) = self
            .cursor_pos
            .get()
            .filter(|&(x, y)| x >= 0.0 && y >= 0.0 && x <= viewer_w && y <= viewer_h)
            .unwrap_or((viewer_w / 2.0, viewer_h / 2.0));

        let hadj = scrolled.hadjustment();
        let vadj = scrolled.vadjustment();
        let ratio = z_new / z_old.max(0.0001);
        let target_scroll_x = (hadj.value() + fx) * ratio - fx;
        let target_scroll_y = (vadj.value() + fy) * ratio - fy;

        self.zoom_level.set(z_new);
        let content = apply_lightbox_zoom(&self.active_picture(), scrolled, z_new);
        self.actions.zoom_reset.set_label(&zoom_label);

        // Pre-set adjustment ranges to match the new content size so the
        // scroll position can be applied in the same frame. Without this
        // the value would be clamped to the stale (old-zoom) range and
        // corrected only after layout, causing a one-frame flicker.
        let (cw, ch) = content.unwrap_or((viewer_w, viewer_h));
        hadj.set_upper(cw.max(viewer_w));
        hadj.set_page_size(viewer_w);
        vadj.set_upper(ch.max(viewer_h));
        vadj.set_page_size(viewer_h);
        hadj.set_value(target_scroll_x);
        vadj.set_value(target_scroll_y);
    }

    /// Show the asset at `pos`: title, details, action buttons, and picture.
    fn render(self: &Rc<Self>) {
        let pos = self.pos.get();
        let Some(item) = self.ui.grid.model.item(pos).and_downcast::<AssetObject>() else {
            return;
        };
        let info = AssetInfo::from_item(&item);

        self.header.page.set_title(&truncate_filename(
            &info.filename,
            lightbox_title_cap(&self.ui),
        ));
        self.show_summary(&info);
        self.header.prev_btn.set_sensitive(pos > 0);
        self.header
            .next_btn
            .set_sensitive(pos + 1 < self.ui.grid.model.n_items());

        let show_remote_actions = !info.is_local() && !info.is_video();
        self.actions
            .resolution_toggle
            .set_visible(show_remote_actions);
        self.actions.download.set_visible(show_remote_actions);
        self.actions.zoom_group.set_visible(!info.is_video());

        self.start_picture_load(&info);
        self.details.loading.set_visible(true);
        if info.is_local() && !info.is_video() {
            self.load_local_details(pos, info.local_path);
        } else {
            self.load_remote_details(pos, info.asset_id);
        }
    }

    /// Filename and sync summary; clears the previous asset's EXIF rows.
    fn show_summary(&self, info: &AssetInfo) {
        self.details.filename.set_label(&info.filename);
        let sync_label = match info.sync_state {
            2 => "On Immich and locally",
            1 => "Local only",
            _ => "On Immich only",
        };
        self.details.summary.set_label(&format!(
            "{} · {}\nCreated: {}",
            info.mime,
            sync_label,
            format_datetime_display(&info.created)
        ));
        while let Some(c) = self.details.exif.first_child() {
            self.details.exif.remove(&c);
        }
        self.details.exif.set_visible(false);
    }

    /// Load into the *inactive* picture and commit the slide transition only after
    /// the texture is set, so the user keeps seeing the current image (with the
    /// loader spinner) until the new one is actually ready.
    fn start_picture_load(self: &Rc<Self>, info: &AssetInfo) {
        let target_is_a = !self.active_a.get();
        let target = if target_is_a {
            self.picture.picture_a.clone()
        } else {
            self.picture.picture_b.clone()
        };
        self.zoom_level.set(1.0);
        apply_lightbox_zoom(&target, &self.picture.scrolled, 1.0);
        self.actions.zoom_reset.set_label("100%");
        self.picture
            .stack
            .set_transition_type(match self.nav_dir.get() {
                1 => gtk::StackTransitionType::SlideLeft,
                -1 => gtk::StackTransitionType::SlideRight,
                _ => gtk::StackTransitionType::None,
            });
        self.load_into_picture(PictureLoad {
            target,
            target_is_a,
            asset_id: info.asset_id.clone(),
            filename: info.filename.clone(),
            mime: info.mime.clone(),
            local_path: info.local_path.clone(),
            full_res: self.actions.resolution_toggle.is_active(),
            is_video: info.is_video(),
        });
        // The slide direction applies to this navigation only.
        self.nav_dir.set(0);
    }

    /// Reset overlays, show any cached thumbnail, then load the real picture asynchronously.
    fn load_into_picture(self: &Rc<Self>, load: PictureLoad) {
        let generation = self.load_gen.get().wrapping_add(1);
        self.load_gen.set(generation);
        self.prepare_for_load(&load);
        // Reveal the spinner after a short delay so fast cache hits and
        // quick JPEG decodes don't flash it. Local paths get a longer
        // delay since most JPEGs decode in well under 250ms, but RAW
        // and large TIFF decodes can run for seconds and need feedback.
        let delay_ms = if load.local_path.is_empty() { 120 } else { 250 };
        let cancel_loader = self.arm_loader_spinner(delay_ms);

        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = lb.fetch_picture(&load).await;
            if lb.load_gen.get() != generation {
                return;
            }
            lb.show_load_result(&load, result);
            cancel_loader.set(true);
            lb.picture.loader.set_reveal_child(false);
        });
    }

    /// Clear the previous asset's overlays and drag path, set up the video badge,
    /// and show the cached grid thumbnail until the real picture arrives.
    fn prepare_for_load(&self, load: &PictureLoad) {
        self.unavailable.overlay.set_reveal_child(false);
        self.unavailable.overlay.set_can_target(false);
        *self.unavailable_path.borrow_mut() = None;
        // Clear drag path while loading; updated once resolved.
        *self.drag_path.borrow_mut() = None;
        log::debug!(
            "Lightbox load: asset={} file={:?} mime={} source={} full_res={} kind={}",
            load.asset_id,
            load.filename,
            load.mime,
            if load.local_path.is_empty() {
                "remote"
            } else {
                "local"
            },
            load.full_res,
            if load.is_video { "video" } else { "image" },
        );
        // Videos use the still thumbnail as a poster + play badge; image
        // decoders would all fail and fall through to "unavailable".
        *self.video_badge_target.borrow_mut() = load.is_video.then(|| {
            (
                load.local_path.clone(),
                load.asset_id.clone(),
                load.filename.clone(),
            )
        });
        self.video_badge_button.set_visible(load.is_video);
        // Show the cached grid thumbnail immediately while the real picture loads.
        if let Some(texture) = self
            .ui
            .ctx
            .thumbnail_cache
            .get_cached(&load.asset_id, ThumbnailSize::Preview)
        {
            load.target.set_paintable(Some(&texture));
        }
    }

    /// Reveal the spinner after `delay_ms` unless the returned flag is set first.
    fn arm_loader_spinner(&self, delay_ms: u64) -> Rc<Cell<bool>> {
        let loader_for_arm = self.picture.loader.clone();
        // Set once the load finishes so the delayed spinner never appears.
        let cancel_loader = Rc::new(Cell::new(false));
        let cancel_for_arm = cancel_loader.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(delay_ms), move || {
            if !cancel_for_arm.get() {
                loader_for_arm.set_reveal_child(true);
            }
            glib::ControlFlow::Break
        });
        cancel_loader
    }

    /// Pick the source: video poster, local file, original download, or preview thumbnail.
    async fn fetch_picture(&self, load: &PictureLoad) -> LoadResult {
        if load.is_video {
            // Use the grid's preview thumbnail as the poster.
            return match self.load_preview_thumbnail(&load.asset_id).await {
                Ok(texture) => LoadResult::Texture(texture, None),
                Err(_) => LoadResult::KeepPoster,
            };
        }
        if !load.local_path.is_empty() {
            let local = std::path::PathBuf::from(&load.local_path);
            if let Some(texture) = load_texture_oriented(&local).await {
                return LoadResult::Texture(texture, Some(local));
            }
            if load.asset_id.starts_with(LOCAL_ID_PREFIX) {
                return LoadResult::Unavailable(Some(load.local_path.clone()));
            }
        }
        if load.full_res {
            return self.fetch_original(load).await;
        }
        match self.load_preview_thumbnail(&load.asset_id).await {
            Ok(texture) => LoadResult::Texture(texture, None),
            Err(_) => LoadResult::Unavailable(None),
        }
    }

    async fn load_preview_thumbnail(&self, asset_id: &str) -> Result<gdk4::Texture, String> {
        self.ui
            .ctx
            .thumbnail_cache
            .load_thumbnail(asset_id, ThumbnailSize::Preview)
            .await
    }

    /// Original file from the preview cache, downloaded on a miss.
    async fn fetch_original(&self, load: &PictureLoad) -> LoadResult {
        let Some(cache_dir) = crate::profile::cache_dir().map(|p| p.join("preview")) else {
            return LoadResult::Unavailable(None);
        };
        let _ = tokio::fs::create_dir_all(&cache_dir).await;
        let temp = original_preview_cache_path(&cache_dir, &load.asset_id, &load.filename);
        if temp.exists() {
            log::debug!(
                "Lightbox original cache hit for {} at {}",
                load.asset_id,
                temp.display(),
            );
        } else if let Err(err) = self.download_original(&load.asset_id, &temp).await {
            log::warn!("Lightbox original fetch failed: {}", err);
            return LoadResult::Unavailable(None);
        }
        match load_texture_oriented(&temp).await {
            Some(texture) => LoadResult::Texture(
                texture,
                drag_export_path(&load.asset_id, &load.filename, &temp),
            ),
            None => LoadResult::Unavailable(Some(temp.display().to_string())),
        }
    }

    /// Download an original, registered with the download progress tracker.
    async fn download_original(
        &self,
        asset_id: &str,
        temp: &std::path::Path,
    ) -> Result<(), String> {
        let ctx = &self.ui.ctx;
        let download_started = std::time::Instant::now();
        begin_download_session(ctx, format!("preview {asset_id}"));
        let progress = track_download_item(
            ctx,
            asset_id.to_string(),
            Some(format!("preview {asset_id}")),
            None,
        );
        let result = ctx
            .api_client
            .download_original_to_file(asset_id, temp, Some(progress))
            .await;
        finish_download_item(ctx, asset_id);
        result?;
        let downloaded_bytes = tokio::fs::metadata(temp)
            .await
            .as_ref()
            .map(std::fs::Metadata::len)
            .unwrap_or(0);
        log::debug!(
            "Lightbox original downloaded for {} in {}ms ({} bytes)",
            asset_id,
            download_started.elapsed().as_millis(),
            downloaded_bytes,
        );
        Ok(())
    }

    /// Apply a load result, then slide to the target picture.
    fn show_load_result(self: &Rc<Self>, load: &PictureLoad, result: LoadResult) {
        match result {
            LoadResult::Texture(texture, drag_path) => {
                load.target.set_paintable(Some(&texture));
                *self.drag_path.borrow_mut() = drag_path;
            }
            LoadResult::Unavailable(path) => {
                self.unavailable.filename.set_label(&load.filename);
                self.unavailable.mime.set_label(&load.mime);
                self.unavailable.open.set_visible(path.is_some());
                *self.unavailable_path.borrow_mut() = path;
                self.unavailable.overlay.set_can_target(true);
                self.unavailable.overlay.set_reveal_child(true);
            }
            LoadResult::KeepPoster => {}
        }
        // Defer the child switch by one idle so the target picture
        // re-measures with the new texture before the slide starts.
        let pic_stack = self.picture.stack.clone();
        let target_is_a = load.target_is_a;
        let lb = self.clone();
        glib::idle_add_local_once(move || {
            pic_stack.set_visible_child_name(if target_is_a { "a" } else { "b" });
            lb.active_a.set(target_is_a);
        });
    }

    /// Local image: parse EXIF on a blocking worker (cached on disk, so repeat opens are cheap).
    fn load_local_details(self: &Rc<Self>, pos: u32, local_path: String) {
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let cache_root = local_exif::cache_root();
            let probed = tokio::task::spawn_blocking(move || {
                probe_local_file(&cache_root, std::path::Path::new(&local_path))
            })
            .await
            .ok();
            if lb.pos.get() != pos {
                return;
            }
            lb.details.loading.set_visible(false);
            // Render whatever we have. Files without an EXIF block (Unsplash,
            // screenshots, edited copies) still get the Image group populated from
            // filesystem + Pixbuf data, so the user always sees *something*.
            if let Some((exif, taken_label)) = probed.as_ref().and_then(local_details_exif) {
                fill_exif_box(&lb.details.exif, &exif, taken_label);
                lb.details.exif.set_visible(true);
            }
        });
    }

    /// Fetch EXIF from the server; dropped if the user has moved to another asset.
    fn load_remote_details(self: &Rc<Self>, pos: u32, asset_id: String) {
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = lb.ui.ctx.api_client.fetch_asset_details(&asset_id).await;
            if lb.pos.get() != pos {
                return;
            }
            lb.details.loading.set_visible(false);
            if let Ok(details) = result
                && let Some(exif) = details.exif_info
            {
                fill_exif_box(&lb.details.exif, &exif, "Taken");
                lb.details.exif.set_visible(true);
            }
        });
    }
}

/// Metadata, pixel size, and cached EXIF for a local file; does blocking I/O.
fn probe_local_file(cache_root: &std::path::Path, path: &std::path::Path) -> LocalProbe {
    let meta = std::fs::metadata(path).ok();
    let file_size = meta.as_ref().map(std::fs::Metadata::len);
    let mtime_iso = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .map(systemtime_to_rfc3339);
    // Pixbuf header-only read — covers JPEG, PNG, GIF, TIFF, WebP and HEIF/AVIF
    // (when the matching pixbuf loaders are installed).
    let dims = gtk::gdk_pixbuf::Pixbuf::file_info(path).and_then(|(_, w, h)| {
        let w = u32::try_from(w).ok()?;
        let h = u32::try_from(h).ok()?;
        Some((w, h))
    });
    LocalProbe {
        exif: local_exif::load_or_extract(cache_root, path),
        file_size,
        mtime_iso,
        dims,
    }
}

/// EXIF for the details pane with gaps filled from the filesystem; `None` if nothing is known.
fn local_details_exif(probe: &LocalProbe) -> Option<(ExifInfo, &'static str)> {
    if probe.exif.is_none()
        && probe.file_size.is_none()
        && probe.dims.is_none()
        && probe.mtime_iso.is_none()
    {
        return None;
    }
    let mut info = probe.exif.clone().unwrap_or_default();
    info.image_width = info.image_width.or(probe.dims.map(|(w, _)| w));
    info.image_height = info.image_height.or(probe.dims.map(|(_, h)| h));
    let used_mtime_fallback = info.date_time_original.is_none();
    let taken_label = if used_mtime_fallback {
        info.date_time_original = probe.mtime_iso.clone();
        "Modified"
    } else {
        "Taken"
    };
    Some((exif_from_local(&info, probe.file_size), taken_label))
}

#[cfg(test)]
mod tests {
    use super::original_preview_cache_path;

    #[test]
    fn original_preview_cache_path_keeps_decoder_extension() {
        let cache = std::path::Path::new("/tmp/previews");
        assert_eq!(
            original_preview_cache_path(cache, "remote-id", "PXL_20250516_222429137.dng"),
            cache.join("remote-id.dng")
        );
        assert_eq!(
            original_preview_cache_path(cache, "remote-id", "extensionless"),
            cache.join("remote-id.bin")
        );
    }
}
