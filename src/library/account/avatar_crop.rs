//! The step before a new profile picture uploads: a square of the photo with a round cut-out
//! showing what the avatar will show. Drag to move it; pinch, scroll, the slider, or +/- to
//! zoom; the arrow keys move it too.

use std::cell::{Cell, RefCell};
use std::io::Cursor;

use gettextrs::gettext;
use gtk::gdk;
use gtk::graphene::{Rect, Size};
use gtk::gsk::{self, RoundedRect};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use image::{DynamicImage, ImageDecoder, ImageReader};
use libadwaita::prelude::*;

/// Side of the uploaded picture; avatars show far smaller.
const OUTPUT_EDGE: u32 = 512;
/// Longest edge drawn while cropping; the crop itself uses the full picture.
const PREVIEW_EDGE: u32 = 1600;
const MAX_ZOOM: f64 = 6.0;
/// How far an arrow key moves the picture, in screen pixels.
const KEY_STEP: f64 = 12.0;
/// Each scroll step or +/- press zooms by this much.
const ZOOM_STEP: f64 = 1.1;

/// The square being cropped, in the full picture's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
struct Square {
    cx: f64,
    cy: f64,
    side: f64,
}

impl Square {
    /// The largest centred square, as the picture starts.
    fn initial((width, height): (f64, f64)) -> Self {
        Self {
            cx: width / 2.0,
            cy: height / 2.0,
            side: width.min(height),
        }
    }

    /// `zoom` times closer than the largest square, kept inside the picture.
    fn zoomed(self, zoom: f64, dims: (f64, f64)) -> Self {
        let side = dims.0.min(dims.1) / zoom.clamp(1.0, MAX_ZOOM);
        Self { side, ..self }.clamped(dims)
    }

    fn moved(self, dx: f64, dy: f64, dims: (f64, f64)) -> Self {
        Self {
            cx: self.cx + dx,
            cy: self.cy + dy,
            ..self
        }
        .clamped(dims)
    }

    fn clamped(self, (width, height): (f64, f64)) -> Self {
        let half = self.side / 2.0;
        Self {
            cx: self.cx.clamp(half, (width - half).max(half)),
            cy: self.cy.clamp(half, (height - half).max(half)),
            side: self.side,
        }
    }

    /// Left, top, and side in whole pixels, for cropping.
    fn pixels(self) -> (u32, u32, u32) {
        let side = self.side.round().max(1.0);
        let left = (self.cx - side / 2.0).round().max(0.0);
        let top = (self.cy - side / 2.0).round().max(0.0);
        (left as u32, top as u32, side as u32)
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct AvatarCrop {
        pub(super) texture: RefCell<Option<gdk::Texture>>,
        pub(super) dims: Cell<(f64, f64)>,
        pub(super) square: Cell<Square>,
        pub(super) drag_start: Cell<Square>,
        pub(super) pinch_start: Cell<f64>,
        pub(super) zoom: RefCell<Option<gtk::Adjustment>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AvatarCrop {
        const NAME: &str = "MimickAvatarCrop";
        type Type = super::AvatarCrop;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for AvatarCrop {}

    impl WidgetImpl for AvatarCrop {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (200, 360, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct AvatarCrop(ObjectSubclass<imp::AvatarCrop>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AvatarCrop {
    fn new(texture: gdk::Texture, dims: (f64, f64), zoom: &gtk::Adjustment) -> Self {
        let crop: Self = glib::Object::builder()
            .property("focusable", true)
            .property("hexpand", true)
            .property("vexpand", true)
            .build();
        crop.update_property(&[gtk::accessible::Property::Label(&gettext(
            "Picture to crop; drag or use the arrow keys to move it",
        ))]);
        let imp = crop.imp();
        imp.texture.replace(Some(texture));
        imp.dims.set(dims);
        imp.square.set(Square::initial(dims));
        imp.zoom.replace(Some(zoom.clone()));
        let weak = crop.downgrade();
        zoom.connect_value_changed(move |zoom| {
            if let Some(crop) = weak.upgrade() {
                crop.apply_zoom(zoom.value());
            }
        });
        crop.connect_input();
        crop
    }

    fn square(&self) -> Square {
        self.imp().square.get()
    }

    fn apply_zoom(&self, zoom: f64) {
        let imp = self.imp();
        imp.square
            .set(imp.square.get().zoomed(zoom, imp.dims.get()));
        self.queue_draw();
    }

    fn zoom_by(&self, factor: f64) {
        if let Some(zoom) = self.imp().zoom.borrow().as_ref() {
            zoom.set_value(zoom.value() * factor);
        }
    }

    /// Screen pixels per picture pixel at the current zoom.
    fn scale(&self) -> f64 {
        let view = f64::from(self.width().min(self.height()));
        view / self.square().side.max(1.0)
    }

    /// Move the picture by `(dx, dy)` screen pixels from `from`.
    fn drag_from(&self, from: Square, dx: f64, dy: f64) {
        let imp = self.imp();
        let scale = self.scale();
        imp.square
            .set(from.moved(-dx / scale, -dy / scale, imp.dims.get()));
        self.queue_draw();
    }

    fn connect_input(&self) {
        let drag = gtk::GestureDrag::new();
        let weak = self.downgrade();
        drag.connect_drag_begin(move |_, _, _| {
            if let Some(crop) = weak.upgrade() {
                crop.grab_focus();
                crop.imp().drag_start.set(crop.square());
            }
        });
        let weak = self.downgrade();
        drag.connect_drag_update(move |_, dx, dy| {
            if let Some(crop) = weak.upgrade() {
                crop.drag_from(crop.imp().drag_start.get(), dx, dy);
            }
        });
        self.add_controller(drag);
        self.connect_zoom_input();
        self.add_controller(self.key_input());
    }

    /// Pinch and the scroll wheel.
    fn connect_zoom_input(&self) {
        let pinch = gtk::GestureZoom::new();
        let weak = self.downgrade();
        pinch.connect_begin(move |_, _| {
            if let Some(crop) = weak.upgrade()
                && let Some(zoom) = crop.imp().zoom.borrow().as_ref()
            {
                crop.imp().pinch_start.set(zoom.value());
            }
        });
        let weak = self.downgrade();
        pinch.connect_scale_changed(move |_, scale| {
            if let Some(crop) = weak.upgrade()
                && let Some(zoom) = crop.imp().zoom.borrow().as_ref()
            {
                zoom.set_value(crop.imp().pinch_start.get() * scale);
            }
        });
        self.add_controller(pinch);
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let weak = self.downgrade();
        scroll.connect_scroll(move |_, _, dy| {
            if let Some(crop) = weak.upgrade() {
                crop.zoom_by(ZOOM_STEP.powf(-dy));
            }
            glib::Propagation::Stop
        });
        self.add_controller(scroll);
    }

    fn key_input(&self) -> gtk::EventControllerKey {
        let keys = gtk::EventControllerKey::new();
        let weak = self.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(crop) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if crop.handle_key(key) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        keys
    }

    fn handle_key(&self, key: gdk::Key) -> bool {
        let (dx, dy) = match key {
            gdk::Key::Left => (KEY_STEP, 0.0),
            gdk::Key::Right => (-KEY_STEP, 0.0),
            gdk::Key::Up => (0.0, KEY_STEP),
            gdk::Key::Down => (0.0, -KEY_STEP),
            gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => {
                self.zoom_by(ZOOM_STEP);
                return true;
            }
            gdk::Key::minus | gdk::Key::KP_Subtract => {
                self.zoom_by(1.0 / ZOOM_STEP);
                return true;
            }
            _ => return false,
        };
        self.drag_from(self.square(), dx, dy);
        true
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let Some(texture) = self.imp().texture.borrow().clone() else {
            return;
        };
        let (width, height) = (self.width() as f32, self.height() as f32);
        let view = width.min(height);
        let frame = Rect::new((width - view) / 2.0, (height - view) / 2.0, view, view);
        let square = self.square();
        let scale = self.scale();
        let (iw, ih) = self.imp().dims.get();
        let left = f64::from(frame.x()) - (square.cx - square.side / 2.0) * scale;
        let top = f64::from(frame.y()) - (square.cy - square.side / 2.0) * scale;
        snapshot.push_clip(&frame);
        snapshot.append_texture(
            &texture,
            &Rect::new(
                left as f32,
                top as f32,
                (iw * scale) as f32,
                (ih * scale) as f32,
            ),
        );
        draw_cutout(snapshot, &frame);
        snapshot.pop();
    }
}

/// Dim everything outside the circle the avatar shows, and outline it.
fn draw_cutout(snapshot: &gtk::Snapshot, frame: &Rect) {
    let radius = Size::new(frame.width() / 2.0, frame.height() / 2.0);
    let circle = RoundedRect::new(*frame, radius, radius, radius, radius);
    snapshot.push_mask(gsk::MaskMode::InvertedAlpha);
    snapshot.push_rounded_clip(&circle);
    snapshot.append_color(&gdk::RGBA::WHITE, frame);
    snapshot.pop();
    snapshot.pop();
    snapshot.append_color(&gdk::RGBA::new(0.0, 0.0, 0.0, 0.55), frame);
    snapshot.pop();
    let ring = gdk::RGBA::new(1.0, 1.0, 1.0, 0.85);
    snapshot.append_border(&circle, &[2.0; 4], &[ring; 4]);
}

/// The picture upright, and a smaller copy for the screen.
struct Decoded {
    image: DynamicImage,
    preview: gdk::Texture,
}

fn decode(bytes: &[u8]) -> Result<(DynamicImage, DynamicImage), String> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| err.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|err| err.to_string())?;
    // Phone photos are often stored sideways with an EXIF note to turn them.
    let orientation = decoder.orientation().map_err(|err| err.to_string())?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(|err| err.to_string())?;
    image.apply_orientation(orientation);
    let preview = image.thumbnail(PREVIEW_EDGE, PREVIEW_EDGE);
    Ok((image, preview))
}

fn texture_of(preview: &DynamicImage) -> gdk::Texture {
    let rgba = preview.to_rgba8();
    let (width, height) = rgba.dimensions();
    gdk::MemoryTexture::new(
        width as i32,
        height as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(rgba.into_raw()),
        width as usize * 4,
    )
    .upcast()
}

/// The square cut from the full picture, scaled to `OUTPUT_EDGE`, as PNG bytes.
fn encode(image: &DynamicImage, square: Square) -> Result<Vec<u8>, String> {
    let (left, top, side) = square.pixels();
    let cropped = image.crop_imm(left, top, side, side).resize_exact(
        OUTPUT_EDGE,
        OUTPUT_EDGE,
        image::imageops::FilterType::Lanczos3,
    );
    let mut png = Vec::new();
    cropped
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    Ok(png)
}

/// Let the user crop `bytes`: the cropped PNG, `Ok(None)` if they cancelled, or why the
/// picture couldn't be opened.
pub(super) async fn crop(
    parent: &impl IsA<gtk::Widget>,
    bytes: Vec<u8>,
) -> Result<Option<Vec<u8>>, String> {
    let (image, preview) = tokio::task::spawn_blocking(move || decode(&bytes))
        .await
        .map_err(|err| err.to_string())??;
    let decoded = Decoded {
        preview: texture_of(&preview),
        image,
    };
    let Some(square) = choose_square(parent, &decoded).await else {
        return Ok(None);
    };
    let image = decoded.image;
    tokio::task::spawn_blocking(move || encode(&image, square))
        .await
        .map_err(|err| err.to_string())?
        .map(Some)
}

/// Show the crop dialog; the chosen square, or `None` when it closes without Set.
async fn choose_square(parent: &impl IsA<gtk::Widget>, decoded: &Decoded) -> Option<Square> {
    let dims = (
        f64::from(decoded.image.width()),
        f64::from(decoded.image.height()),
    );
    let zoom = gtk::Adjustment::new(1.0, 1.0, MAX_ZOOM, 0.1, 0.5, 0.0);
    let crop = AvatarCrop::new(decoded.preview.clone(), dims, &zoom);
    let (dialog, set) = crop_dialog(&crop, &zoom);
    let (done, chosen) = tokio::sync::oneshot::channel();
    let done = RefCell::new(Some(done));
    let (weak, weak_dialog) = (crop.downgrade(), dialog.downgrade());
    set.connect_clicked(move |_| {
        if let (Some(crop), Some(done)) = (weak.upgrade(), done.borrow_mut().take()) {
            done.send(crop.square()).ok();
        }
        if let Some(dialog) = weak_dialog.upgrade() {
            dialog.close();
        }
    });
    dialog.present(Some(parent));
    crop.grab_focus();
    // Closing without Set drops the sender, which reads as no choice.
    chosen.await.ok()
}

/// The dialog: Cancel and Set in the header, the picture, and a zoom slider under it.
fn crop_dialog(crop: &AvatarCrop, zoom: &gtk::Adjustment) -> (libadwaita::Dialog, gtk::Button) {
    let set = gtk::Button::builder()
        .label(gettext("Set"))
        .css_classes(["suggested-action"])
        .build();
    let header = libadwaita::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let cancel = gtk::Button::builder().label(gettext("Cancel")).build();
    header.pack_start(&cancel);
    header.pack_end(&set);
    let slider = gtk::Scale::builder()
        .adjustment(zoom)
        .hexpand(true)
        .tooltip_text(gettext("Zoom"))
        .build();
    let zoom_row = gtk::Box::builder().spacing(6).build();
    zoom_row.append(&gtk::Image::from_icon_name("zoom-out-symbolic"));
    zoom_row.append(&slider);
    zoom_row.append(&gtk::Image::from_icon_name("zoom-in-symbolic"));
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    content.append(crop);
    content.append(&zoom_row);
    let toolbar = libadwaita::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    let dialog = libadwaita::Dialog::builder()
        .title(gettext("Crop Picture"))
        .content_width(420)
        .content_height(520)
        .child(&toolbar)
        .default_widget(&set)
        .build();
    let weak = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = weak.upgrade() {
            dialog.close();
        }
    });
    (dialog, set)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: (f64, f64) = (400.0, 200.0);

    #[test]
    fn the_square_starts_as_large_as_fits_and_centred() {
        let square = Square::initial(WIDE);
        assert_eq!((square.cx, square.cy, square.side), (200.0, 100.0, 200.0));
        assert_eq!(square.pixels(), (100, 0, 200));
    }

    #[test]
    fn zooming_shrinks_the_square_within_limits() {
        let square = Square::initial(WIDE).zoomed(2.0, WIDE);
        assert_eq!(square.side, 100.0);
        assert_eq!(Square::initial(WIDE).zoomed(0.5, WIDE).side, 200.0);
        assert_eq!(
            Square::initial(WIDE).zoomed(100.0, WIDE).side,
            200.0 / MAX_ZOOM
        );
    }

    #[test]
    fn moving_stops_at_the_picture_edges() {
        let square = Square::initial(WIDE).moved(1000.0, 1000.0, WIDE);
        assert_eq!((square.cx, square.cy), (300.0, 100.0));
        let zoomed = Square::initial(WIDE)
            .zoomed(2.0, WIDE)
            .moved(-1000.0, 30.0, WIDE);
        assert_eq!((zoomed.cx, zoomed.cy), (50.0, 130.0));
    }
}
