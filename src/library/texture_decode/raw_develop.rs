//! Developing a RAW with LibRaw: linear Display P3 at full or half size, toned by `raw_tone`.

use std::ffi::{CString, c_int};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Instant;

use gdk4::prelude::TextureExt;
use rsraw_sys::{libraw_data_t, libraw_processed_image_t};

use super::raw_tone::{self, Encoding, Pixels, Reference};
use super::{LibrawHandle, MemImage, bitmap_thumb_rgb8, color, decompress_jpeg};

/// LibRaw's `output_color` for Display P3 (its "DCI-P3 D65"), which GTK shows as it is.
const LIBRAW_DISPLAY_P3: i32 = 7;
/// Long edge to decode an embedded preview at: plenty for its histograms, and cheap with
/// DCT scaling.
const REFERENCE_EDGE: u32 = 512;

/// How large to develop.
#[derive(Clone, Copy, Debug)]
pub(super) enum Size {
    Full,
    Half,
    /// Half when that still covers this many pixels on the long side (several times faster,
    /// as it skips demosaicing), else full.
    Fit(u32),
}

pub(super) struct Developed {
    pub(super) texture: gdk4::Texture,
    /// Every sensor pixel made it in, so it can stand in for the full decode.
    pub(super) full: bool,
}

/// `path` developed to look like `reference`, or like the camera's embedded preview when
/// there's none; `None` when LibRaw can't read it.
pub(super) fn develop(path: &Path, size: Size, reference: Option<&Reference>) -> Option<Developed> {
    let started = Instant::now();
    let mut raw = LibrawHandle::open(path)?;
    let reference = reference
        .cloned()
        .or_else(|| embedded_reference(&mut raw, path));
    let half = configure(raw.data(), size);
    if !process(&mut raw, path) {
        return None;
    }
    let image = raw.make_image(rsraw_sys::libraw_dcraw_make_mem_image, path)?;
    let texture = toned_texture(&image, reference.as_ref(), path)?;
    log_developed(path, half, reference.is_some(), started);
    Some(Developed {
        texture,
        full: !half,
    })
}

/// The look of `picture`, such as Immich's preview of the photo, read in Display P3.
pub(super) fn reference_from_texture(picture: &gdk4::Texture) -> Option<Reference> {
    let mut downloader = gdk4::TextureDownloader::new(picture);
    downloader.set_format(gdk4::MemoryFormat::R8g8b8);
    downloader.set_color_state(&color::display_p3()?);
    let (bytes, stride) = downloader.download_bytes();
    Reference::sample(&Pixels {
        bytes: &bytes,
        width: usize::try_from(picture.width()).ok()?,
        height: usize::try_from(picture.height()).ok()?,
        stride,
        bytes_per_pixel: 3,
        encoding: Encoding::DisplayP3,
    })
}

impl LibrawHandle {
    fn open(path: &Path) -> Option<Self> {
        let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: no preconditions; a null result is handled before use.
        let lr = unsafe { rsraw_sys::libraw_init(0) };
        if lr.is_null() {
            log::debug!("libraw_init returned null for {}", path.display());
            return None;
        }
        let handle = Self(lr);
        // SAFETY: `lr` is a live context, and `c_path` outlives the call.
        let rc = unsafe { rsraw_sys::libraw_open_file(lr, c_path.as_ptr()) };
        succeeded(path, "libraw_open_file", rc).then_some(handle)
    }

    fn data(&mut self) -> &mut libraw_data_t {
        // SAFETY: a handle only holds a non-null context from `libraw_init`, freed on drop.
        unsafe { &mut *self.0 }
    }

    /// One of LibRaw's steps that works on the context and returns a status.
    fn run(&mut self, step: unsafe extern "C" fn(*mut libraw_data_t) -> c_int) -> c_int {
        // SAFETY: the context is live (see `data`), and every step passed here only needs that.
        unsafe { step(self.0) }
    }

    /// An image LibRaw allocates for the caller: the developed photo, or its thumbnail.
    fn make_image(
        &mut self,
        make: unsafe extern "C" fn(*mut libraw_data_t, *mut c_int) -> *mut libraw_processed_image_t,
        path: &Path,
    ) -> Option<MemImage> {
        let mut errcode = 0;
        // SAFETY: the context is live (see `data`), and `errcode` outlives the call.
        let image = unsafe { make(self.0, &mut errcode) };
        let image = (!image.is_null()).then(|| MemImage(image));
        if image.is_none() || errcode != 0 {
            log::debug!("LibRaw image failed ({errcode}) for {}", path.display());
            return None;
        }
        image
    }
}

impl MemImage {
    fn header(&self) -> &libraw_processed_image_t {
        // SAFETY: an image only holds a non-null result of `libraw_dcraw_make_mem_*`,
        // freed on drop.
        unsafe { &*self.0 }
    }

    /// The pixel bytes LibRaw allocates after the header, borrowed for as long as the image.
    fn bytes(&self) -> &[u8] {
        let len = self.header().data_size as usize;
        // SAFETY: LibRaw allocates `data_size` bytes after the header, freed with it.
        unsafe { std::slice::from_raw_parts(std::ptr::addr_of!((*self.0).data).cast(), len) }
    }
}

/// The camera's embedded preview as a reference, from the thumbnail LibRaw already located
/// (no second pass over the file).
fn embedded_reference(raw: &mut LibrawHandle, path: &Path) -> Option<Reference> {
    if raw.run(rsraw_sys::libraw_unpack_thumb) != 0 {
        return None;
    }
    let thumb = raw.make_image(rsraw_sys::libraw_dcraw_make_mem_thumb, path)?;
    let header = thumb.header();
    let (width, height) = (usize::from(header.width), usize::from(header.height));
    if header.type_ == rsraw_sys::LibRaw_image_formats_LIBRAW_IMAGE_JPEG {
        let image = decompress_jpeg(thumb.bytes(), Some(REFERENCE_EDGE)).ok()?;
        return srgb_reference(&image.pixels, image.width, image.height);
    }
    if header.colors != 3 {
        return None;
    }
    let rgb = bitmap_thumb_rgb8(thumb.bytes(), header.bits, width * height * 3)?;
    srgb_reference(&rgb, width, height)
}

fn srgb_reference(rgb: &[u8], width: usize, height: usize) -> Option<Reference> {
    Reference::sample(&Pixels {
        bytes: rgb,
        width,
        height,
        stride: width * 3,
        bytes_per_pixel: 3,
        encoding: Encoding::Srgb,
    })
}

/// Linear 16-bit Display P3 at the camera's white balance, with LibRaw's auto-brightening
/// off: `raw_tone` sets the exposure. Returns whether it develops at half size.
fn configure(data: &mut libraw_data_t, size: Size) -> bool {
    let half = match size {
        Size::Full => false,
        Size::Half => true,
        Size::Fit(edge) => u32::from(data.sizes.width.max(data.sizes.height)) / 2 >= edge,
    };
    let params = &mut data.params;
    // Chosen after opening, once the sensor size is known: LibRaw re-reads it to process.
    params.half_size = i32::from(half);
    params.use_camera_wb = 1;
    params.user_qual = 3;
    params.output_bps = 16;
    params.output_color = LIBRAW_DISPLAY_P3;
    params.gamm[0] = 1.0;
    params.gamm[1] = 1.0;
    params.no_auto_bright = 1;
    half
}

/// Unpack and demosaic, as `configure` set up.
fn process(raw: &mut LibrawHandle, path: &Path) -> bool {
    succeeded(path, "libraw_unpack", raw.run(rsraw_sys::libraw_unpack))
        && succeeded(
            path,
            "libraw_dcraw_process",
            raw.run(rsraw_sys::libraw_dcraw_process),
        )
}

/// LibRaw's 16-bit linear image toned to 8-bit Display P3: half the memory of the 16-bit
/// pixels, and what the screen shows anyway.
fn toned_texture(
    image: &MemImage,
    reference: Option<&Reference>,
    path: &Path,
) -> Option<gdk4::Texture> {
    let header = image.header();
    let (width, height) = (usize::from(header.width), usize::from(header.height));
    let rgb16 = image.bytes();
    if header.colors != 3 || header.bits != 16 || rgb16.len() < width * height * 6 {
        log::debug!(
            "libraw produced {}-channel {}-bit output for {} -- skipping",
            header.colors,
            header.bits,
            path.display()
        );
        return None;
    }
    let started = Instant::now();
    let tone = raw_tone::tone(&raw_tone::sample_decode(rgb16), reference);
    let toned = raw_tone::apply(&tone, &rgb16[..width * height * 6]);
    log::debug!(
        "Toned {} in {}ms",
        path.display(),
        started.elapsed().as_millis()
    );
    let layout = color::Layout {
        width: i32::from(header.width),
        height: i32::from(header.height),
        stride: width * 3,
        format: gdk4::MemoryFormat::R8g8b8,
    };
    Some(color::tagged(
        &glib::Bytes::from_owned(toned),
        &layout,
        color::display_p3().as_ref(),
    ))
}

fn succeeded(path: &Path, step: &str, rc: i32) -> bool {
    if rc != 0 {
        log::debug!("{step} failed ({rc}) for {}", path.display());
    }
    rc == 0
}

fn log_developed(path: &Path, half: bool, matched: bool, started: Instant) {
    log::debug!(
        "Developed {} at {} size, {} in {}ms",
        path.display(),
        if half { "half" } else { "full" },
        if matched {
            "toned like its reference"
        } else {
            "with the fallback tone"
        },
        started.elapsed().as_millis()
    );
}
