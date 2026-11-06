//! Lightbox image viewer: full-screen preview with zoom, pan, EXIF details, and keyboard navigation.
//!
//! Loads preview or original resolution images with pinch-zoom and
//! swipe navigation. Displays an EXIF metadata panel and provides
//! download-to-folder and delete-to-trash actions.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

use crate::library::asset_object::AssetObject;

use super::video_player::VideoPlayer;
use super::{LOCAL_ID_PREFIX, LibraryWindowUi};

mod activity;
mod albums;
mod details;
mod edit;
mod faces;
mod fullscreen;
mod interaction;
mod live;
mod location;
mod ocr;
mod people;
mod rating;
mod slideshow;
mod stack;
mod strip;
mod video;
mod widgets;
#[cfg(test)]
use details::{edited_original_cache_path, original_preview_cache_path};
pub(super) use widgets::{
    ActionBar, DetailsPane, LightboxHeader, PictureArea, UnavailableCard, build_action_bar,
    build_details_pane, build_header, build_picture_area, build_split_view, build_unavailable_card,
    build_video_badge, build_viewer, lightbox_title_cap, set_viewer_margins,
};

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
    asset_id: String,
    filename: String,
    mime: String,
    local_path: String,
    full_res: bool,
    is_video: bool,
    edited: bool,
}

impl PictureLoad {
    /// Local copy of an edited asset: the fallback picture and the drag-out file.
    fn edited_local_path(&self) -> Option<std::path::PathBuf> {
        (self.edited && !self.local_path.is_empty())
            .then(|| std::path::PathBuf::from(&self.local_path))
    }
}

/// What a finished load shows; applied only if the user is still on that asset.
enum LoadResult {
    /// A texture, the file drag-out exports, and the file to decode in full when zooming in.
    Texture {
        texture: gdk4::Texture,
        drag_path: Option<std::path::PathBuf>,
        full_source: Option<std::path::PathBuf>,
    },
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
    favorite: bool,
    archived: bool,
    edited: bool,
    live_video_id: String,
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
            favorite: item.is_favorite(),
            archived: item.is_archived(),
            edited: item.is_edited(),
            live_video_id: item.live_video_id(),
        }
    }

    fn is_video(&self) -> bool {
        crate::media_kinds::asset_kind(&self.mime) == crate::media_kinds::AssetKind::Video
    }

    fn decoded_from_disk(&self) -> bool {
        !self.local_path.is_empty() && !self.edited && !self.is_video()
    }

    fn is_local(&self) -> bool {
        !self.local_path.is_empty() && self.asset_id.starts_with(LOCAL_ID_PREFIX)
    }
}

/// Lightbox page widgets plus the state shared by its signal handlers.
/// The window's `interaction.lightbox` slot owns it while open; handlers must hold `Weak`s.
pub(super) struct Lightbox {
    ui: Rc<LibraryWindowUi>,
    header: LightboxHeader,
    picture: PictureArea,
    unavailable: UnavailableCard,
    video_badge_button: gtk::Button,
    live_button: gtk::ToggleButton,
    live_layer: gtk::Box,
    motion: RefCell<Option<Rc<super::video_player::MotionPlayer>>>,
    /// A stack member picked in the stack strip, shown in place of the grid item.
    member: RefCell<Option<AssetObject>>,
    stack: stack::StackStrip,
    face_mode: faces::FaceMode,
    edit_mode: edit::EditMode,
    slideshow: slideshow::Slideshow,
    actions: ActionBar,
    details: DetailsPane,
    viewer: gtk::Box,
    pos: Cell<u32>,
    // Increments on every navigation. Async load tasks capture the generation
    // they were started for and skip UI writes if the user has navigated away
    // by the time their decode finishes (relevant for slow RAW files).
    load_gen: Cell<u64>,
    zoom_level: Cell<f64>,
    // Cursor position over the picture area, so zoom can be focal-point aware.
    // None when the cursor is outside the viewer; zoom then uses the centre.
    cursor_pos: Cell<Option<(f64, f64)>>,
    drag_start: Cell<(f64, f64)>,
    pinch_start: Cell<f64>,
    unavailable_path: RefCell<Option<String>>,
    video: RefCell<Option<Rc<VideoPlayer>>>,
    fullscreen_handler: RefCell<Option<glib::SignalHandlerId>>,
    collapse_handler: RefCell<Option<glib::SignalHandlerId>>,
    // Original file exported by drag-out. Updated by the load logic whenever an
    // asset is displayed (from its local path or the preview cache).
    drag_path: RefCell<Option<std::path::PathBuf>>,
    full_source: RefCell<Option<std::path::PathBuf>>,
    // A capture date was edited: reload the grid when the lightbox closes, since the
    // asset's timeline position moved (reloading now would shift the open photo).
    reload_on_close: Cell<bool>,
    chrome: fullscreen::Chrome,
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
    lightbox.connect_ocr();
    lightbox.connect_keys();
    lightbox.connect_actions();
    lightbox.connect_fullscreen();
    lightbox.connect_live();
    lightbox.connect_slideshow();
    lightbox.connect_stack();
    lightbox.connect_faces();
    lightbox.connect_edit_mode();
    lightbox.render();
    lightbox.ui.nav.push(&lightbox.header.page);
    let slot = &lightbox.ui.interaction.lightbox;
    if let Some(previous) = slot.replace(Some(lightbox.clone())) {
        lightbox.reload_on_close.set(previous.reload_on_close.get());
        previous.release();
    }
    log::debug!("Lightbox opened at position {position}");
}

pub(super) use slideshow::SlideshowOptions;
pub(super) use stack::{append_unstack_button, stack_selected};

/// Starts a slideshow at `position`, opening the lightbox there unless it's already open.
pub(super) fn start_slideshow_at(
    ui: &Rc<LibraryWindowUi>,
    position: u32,
    options: Option<SlideshowOptions>,
) {
    if ui.interaction.lightbox.borrow().is_none() {
        open_lightbox(ui.clone(), position);
    }
    let lightbox = ui.interaction.lightbox.borrow().clone();
    if let Some(lightbox) = lightbox {
        lightbox.start_slideshow(options);
    }
}

/// Face mode for `item`, opening the lightbox on it first when it isn't open.
pub(super) fn tag_people(ui: &Rc<LibraryWindowUi>, item: &AssetObject) {
    open_on(ui, item, Lightbox::enter_face_mode);
}

/// Edit mode for `item`, opening the lightbox on it first when it isn't open.
pub(super) fn edit_photo(ui: &Rc<LibraryWindowUi>, item: &AssetObject) {
    open_on(ui, item, Lightbox::enter_edit_mode);
}

fn open_on(ui: &Rc<LibraryWindowUi>, item: &AssetObject, start: fn(&Rc<Lightbox>)) {
    if ui.interaction.lightbox.borrow().is_none() {
        let model = &ui.grid.model;
        let found = (0..model.n_items()).find(|&pos| {
            model
                .item(pos)
                .and_downcast::<AssetObject>()
                .is_some_and(|candidate| candidate == *item)
        });
        let Some(position) = found else {
            return;
        };
        open_lightbox(ui.clone(), position);
    }
    let lightbox = ui.interaction.lightbox.borrow().clone();
    if let Some(lightbox) = lightbox {
        start(&lightbox);
    }
}

pub(super) fn close_lightbox(ui: &LibraryWindowUi) {
    if let Some(lightbox) = ui.interaction.lightbox.take() {
        log::debug!("Lightbox closed with its window");
        lightbox.release();
    }
}

impl Lightbox {
    fn release(&self) {
        self.stop_slideshow();
        self.close_fullscreen();
        if let Some(handler) = self.collapse_handler.take() {
            self.ui.split.disconnect(handler);
        }
        self.close_video();
        self.close_live();
        self.picture.strip.stop();
        for picture in self.picture.strip.pictures() {
            picture.set_paintable(None::<&gtk::gdk::Paintable>);
        }
    }

    fn close(self: &Rc<Self>) {
        self.release();
        if self.reload_on_close.replace(false) {
            crate::library::refresh_library_after_mutation(self.ui.clone(), true);
        }
        let slot = &self.ui.interaction.lightbox;
        if slot
            .borrow()
            .as_ref()
            .is_some_and(|open| Rc::ptr_eq(open, self))
        {
            slot.take();
        }
    }
}

impl Drop for Lightbox {
    fn drop(&mut self) {
        log::debug!("Lightbox dropped");
    }
}

#[cfg(test)]
mod tests {
    use super::{edited_original_cache_path, original_preview_cache_path};

    #[test]
    fn edited_original_cache_path_never_reuses_the_original() {
        let cache = std::path::Path::new("/tmp/previews");
        assert_eq!(
            edited_original_cache_path(cache, "remote-id"),
            cache.join("remote-id-edited.bin")
        );
        assert_ne!(
            edited_original_cache_path(cache, "remote-id"),
            original_preview_cache_path(cache, "remote-id", "x.bin")
        );
    }

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
