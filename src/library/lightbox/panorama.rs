//! 360° photos in the lightbox: a "360°" pill, and a view you can drag, pinch, and scroll to
//! look around, drawn over the flat photo.

use gettextrs::gettext;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gtk::gdk;
use gtk::prelude::*;

mod projection;

use super::Lightbox;
use crate::library::context_menu::connect_context_menu;
use projection::{Camera, Equirect};

/// Immich's `projectionType` for photos that wrap all the way around.
const EQUIRECTANGULAR: &str = "EQUIRECTANGULAR";
/// Frames past this many pixels are drawn smaller and stretched, to keep dragging smooth.
const MAX_FRAME_PIXELS: f64 = 2_500_000.0;
const KEY_TURN: f64 = 10.0 * std::f64::consts::PI / 180.0;
const KEY_ZOOM: f64 = 1.2;
const SCROLL_ZOOM: f64 = 1.1;
/// Each key, whether it needs Shift (bare Left and Right go to the next photo), and its turn.
const TURN_KEYS: [(gdk::Key, bool, (f64, f64)); 4] = [
    (gdk::Key::Up, false, (0.0, KEY_TURN)),
    (gdk::Key::Down, false, (0.0, -KEY_TURN)),
    (gdk::Key::Left, true, (-KEY_TURN, 0.0)),
    (gdk::Key::Right, true, (KEY_TURN, 0.0)),
];
const ZOOM_KEYS: [(gdk::Key, f64); 5] = [
    (gdk::Key::plus, KEY_ZOOM),
    (gdk::Key::equal, KEY_ZOOM),
    (gdk::Key::KP_Add, KEY_ZOOM),
    (gdk::Key::minus, 1.0 / KEY_ZOOM),
    (gdk::Key::KP_Subtract, 1.0 / KEY_ZOOM),
];

/// Whether Immich's `projectionType` marks a 360° photo.
pub(super) fn is_panorama(projection: Option<&str>) -> bool {
    projection.is_some_and(|kind| kind.eq_ignore_ascii_case(EQUIRECTANGULAR))
}

/// The pill, the view over the photo, and what the view is showing.
pub(super) struct Panorama {
    pub(super) button: gtk::ToggleButton,
    pub(super) layer: gtk::Overlay,
    frame: gtk::Picture,
    size: gtk::DrawingArea,
    source: RefCell<Option<Arc<Equirect>>>,
    camera: Cell<Camera>,
    drag_start: Cell<Camera>,
    pinch_start: Cell<Camera>,
    /// A frame is being drawn; `queued` asks for one more once it lands.
    drawing: Cell<bool>,
    queued: Cell<bool>,
}

impl Panorama {
    pub(super) fn new() -> Rc<Self> {
        let frame = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Fill)
            .can_shrink(true)
            .hexpand(true)
            .vexpand(true)
            .build();
        let size = gtk::DrawingArea::builder()
            .focusable(true)
            .hexpand(true)
            .vexpand(true)
            .build();
        let layer = gtk::Overlay::builder()
            .child(&frame)
            .visible(false)
            .css_classes(["mimick-panorama"])
            .build();
        layer.add_overlay(&size);
        let view = Rc::new(Self {
            button: pill(),
            layer,
            frame,
            size,
            source: RefCell::default(),
            camera: Cell::default(),
            drag_start: Cell::default(),
            pinch_start: Cell::default(),
            drawing: Cell::new(false),
            queued: Cell::new(false),
        });
        view.connect_gestures();
        view
    }

    pub(super) fn is_active(&self) -> bool {
        self.layer.is_visible()
    }

    /// The area to attach the photo's right-click and long-press menu to.
    pub(super) fn input(&self) -> &gtk::DrawingArea {
        &self.size
    }

    /// Cover the flat photo, facing the middle of the panorama once its pixels arrive.
    fn open(&self) {
        self.camera.set(Camera::default());
        self.layer.set_visible(true);
        self.size.grab_focus();
    }

    /// Wrap `texture` around the viewer, keeping the direction already looked in.
    fn set_texture(self: &Rc<Self>, texture: &gdk::Texture) {
        self.source.replace(Some(Arc::new(download(texture))));
        self.draw();
    }

    /// Back to the flat photo; the pixels are dropped, as they can be large.
    pub(super) fn hide(&self) {
        self.layer.set_visible(false);
        self.source.take();
        self.frame.set_paintable(None::<&gdk::Paintable>);
    }

    pub(super) fn zoom(self: &Rc<Self>, factor: f64) {
        self.look(self.camera.get().zoomed(factor));
    }

    pub(super) fn reset(self: &Rc<Self>) {
        self.look(Camera::default());
    }

    /// Up and Down tilt, Shift+Left and Shift+Right turn, +/- zoom; false for any other key.
    pub(super) fn handle_key(self: &Rc<Self>, key: gdk::Key, shift: bool) -> bool {
        if let Some(&(_, factor)) = ZOOM_KEYS.iter().find(|(k, _)| *k == key) {
            self.zoom(factor);
            return true;
        }
        let turn = TURN_KEYS
            .iter()
            .find(|(k, needs_shift, _)| *k == key && (!needs_shift || shift));
        let Some(&(_, _, (yaw, pitch))) = turn else {
            return false;
        };
        self.look(self.camera.get().turned(yaw, pitch));
        true
    }

    fn look(self: &Rc<Self>, camera: Camera) {
        self.camera.set(camera);
        self.draw();
    }

    /// Draw a frame on a worker thread; while one is out, only the latest request waits.
    fn draw(self: &Rc<Self>) {
        if self.drawing.replace(true) {
            self.queued.set(true);
            return;
        }
        let (Some(source), Some((width, height))) =
            (self.source.borrow().clone(), self.frame_size())
        else {
            self.drawing.set(false);
            return;
        };
        let camera = self.camera.get();
        let view = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let pixels = gtk::gio::spawn_blocking(move || {
                projection::render(&source, camera, width, height)
            })
            .await;
            let Some(view) = view.upgrade() else {
                return;
            };
            view.drawing.set(false);
            if let Ok(pixels) = pixels
                && view.is_active()
            {
                view.frame
                    .set_paintable(Some(&frame_texture(pixels, width, height)));
            }
            if view.queued.replace(false) {
                view.draw();
            }
        });
    }

    /// The view's size in device pixels, scaled down to `MAX_FRAME_PIXELS`.
    fn frame_size(&self) -> Option<(usize, usize)> {
        let scale = f64::from(self.size.scale_factor());
        let width = f64::from(self.size.width()) * scale;
        let height = f64::from(self.size.height()) * scale;
        if width < 1.0 || height < 1.0 {
            return None;
        }
        let shrink = (MAX_FRAME_PIXELS / (width * height)).sqrt().min(1.0);
        Some(((width * shrink) as usize, (height * shrink) as usize))
    }

    fn connect_gestures(self: &Rc<Self>) {
        self.connect_drag();
        self.connect_pinch();
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let view = Rc::downgrade(self);
        scroll.connect_scroll(move |_, _, dy| {
            if let Some(view) = view.upgrade() {
                view.zoom(if dy < 0.0 {
                    SCROLL_ZOOM
                } else {
                    1.0 / SCROLL_ZOOM
                });
            }
            glib::Propagation::Stop
        });
        self.size.add_controller(scroll);
        let view = Rc::downgrade(self);
        self.size.connect_resize(move |_, _, _| {
            if let Some(view) = view.upgrade() {
                view.draw();
            }
        });
    }

    fn connect_drag(self: &Rc<Self>) {
        let drag = gtk::GestureDrag::new();
        let view = Rc::downgrade(self);
        drag.connect_drag_begin(move |_, _, _| {
            if let Some(view) = view.upgrade() {
                view.drag_start.set(view.camera.get());
                view.size.grab_focus();
            }
        });
        let view = Rc::downgrade(self);
        drag.connect_drag_update(move |_, dx, dy| {
            if let Some(view) = view.upgrade() {
                let width = f64::from(view.size.width());
                view.look(view.drag_start.get().dragged(dx, dy, width));
            }
        });
        self.size.add_controller(drag);
    }

    fn connect_pinch(self: &Rc<Self>) {
        let pinch = gtk::GestureZoom::new();
        let view = Rc::downgrade(self);
        pinch.connect_begin(move |pinch, _| {
            pinch.set_state(gtk::EventSequenceState::Claimed);
            if let Some(view) = view.upgrade() {
                view.pinch_start.set(view.camera.get());
            }
        });
        let view = Rc::downgrade(self);
        pinch.connect_scale_changed(move |_, scale| {
            if let Some(view) = view.upgrade() {
                view.look(view.pinch_start.get().zoomed(scale));
            }
        });
        self.size.add_controller(pinch);
    }
}

/// "360°" pill beside Live, shown on 360° photos.
fn pill() -> gtk::ToggleButton {
    gtk::ToggleButton::builder()
        .label(gettext("360°"))
        .tooltip_text(gettext("Look around (P)"))
        .css_classes(["osd", "pill", "mimick-panorama-button"])
        .visible(false)
        .build()
}

/// The texture's pixels as tightly packed RGB rows.
fn download(texture: &gdk::Texture) -> Equirect {
    let mut downloader = gdk::TextureDownloader::new(texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8);
    let (pixels, stride) = downloader.download_bytes();
    Equirect {
        pixels,
        width: texture.width().max(1) as usize,
        height: texture.height().max(1) as usize,
        stride,
    }
}

fn frame_texture(pixels: Vec<u8>, width: usize, height: usize) -> gdk::MemoryTexture {
    gdk::MemoryTexture::new(
        width as i32,
        height as i32,
        gdk::MemoryFormat::R8g8b8,
        &glib::Bytes::from_owned(pixels),
        width * 3,
    )
}

impl Lightbox {
    pub(super) fn connect_panorama(self: &Rc<Self>) {
        let lb = Rc::downgrade(self);
        self.panorama.button.connect_toggled(move |button| {
            if let Some(lb) = lb.upgrade() {
                if button.is_active() {
                    lb.enter_panorama();
                } else {
                    lb.panorama.hide();
                }
            }
        });
        let lb = Rc::downgrade(self);
        connect_context_menu(self.panorama.input(), move |x, y| {
            if let Some(lb) = lb.upgrade() {
                lb.show_menu_at(x, y);
            }
        });
        // The flat photo is sharpened or replaced while the view is open: follow it.
        for picture in self.picture.strip.pictures() {
            let lb = Rc::downgrade(self);
            picture.connect_paintable_notify(move |picture| {
                if let Some(lb) = lb.upgrade()
                    && lb.panorama.is_active()
                    && *picture == lb.active_picture()
                {
                    lb.show_panorama_texture();
                }
            });
        }
    }

    /// A new photo: the pill hides until its details say it's a 360° photo.
    pub(super) fn sync_panorama(&self) {
        self.panorama.button.set_active(false);
        self.panorama.button.set_visible(false);
    }

    /// Offer the 360° view for Immich's `projection`, and open it as Immich does.
    pub(super) fn offer_panorama(self: &Rc<Self>, projection: Option<&str>) {
        let is_image = self.current_item().is_some_and(|item| {
            item.property::<String>("asset-type")
                .eq_ignore_ascii_case("IMAGE")
        });
        let offered = is_image && is_panorama(projection);
        self.panorama.button.set_visible(offered);
        let busy =
            self.slideshow.is_running() || self.face_mode.is_active() || self.edit_mode.is_active();
        if offered && !busy {
            self.panorama.button.set_active(true);
        }
    }

    pub(super) fn leave_panorama(&self) {
        self.panorama.button.set_active(false);
    }

    fn enter_panorama(self: &Rc<Self>) {
        self.zoom_reset();
        self.panorama.open();
        self.show_panorama_texture();
        // Swaps in the full-size original, when there is one, for a sharper view.
        self.sharpen_for_zoom(f64::INFINITY);
    }

    fn show_panorama_texture(self: &Rc<Self>) {
        if let Some(texture) = self
            .active_picture()
            .paintable()
            .and_downcast::<gdk::Texture>()
        {
            self.panorama.set_texture(&texture);
        }
    }

    /// P switches the 360° view; in it, arrows and +/- look around. False for other keys.
    pub(super) fn handle_panorama_key(self: &Rc<Self>, key: gdk::Key, shift: bool) -> bool {
        if matches!(key, gdk::Key::p | gdk::Key::P) && self.panorama.button.is_visible() {
            let button = &self.panorama.button;
            button.set_active(!button.is_active());
            return true;
        }
        self.panorama.is_active() && self.panorama.handle_key(key, shift)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_equirectangular_photos_are_panoramas() {
        assert!(is_panorama(Some("EQUIRECTANGULAR")));
        assert!(is_panorama(Some("equirectangular")));
        assert!(!is_panorama(Some("CUBEMAP")));
        assert!(!is_panorama(None));
    }
}
