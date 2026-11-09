//! Developing a RAW with LibRaw: linear Display P3 at full or half size, toned by `raw_tone`.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Instant;

use rsraw_sys::{libraw_data_t, libraw_processed_image_t};

use super::raw_tone::{self, Histogram};
use super::{LibrawHandle, MemImage, bitmap_thumb_rgb8, color, decompress_jpeg};

/// LibRaw's `output_color` for Display P3 (its "DCI-P3 D65"), which GTK shows as it is.
const LIBRAW_DISPLAY_P3: i32 = 7;
/// Long edge to decode the preview at: plenty for its histogram, and cheap with DCT scaling.
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

/// `path` developed and toned like the camera's own JPEG; `None` when LibRaw can't read it.
pub(super) fn develop(path: &Path, size: Size) -> Option<Developed> {
    let started = Instant::now();
    // SAFETY: LibRaw's C API. The handle and its images are freed by RAII guards, dropped
    // images first, and nothing borrowed from them outlives this block.
    unsafe {
        let handle = open(path)?;
        let reference = preview_luminance(handle.0);
        let half = configure(handle.0, size);
        if !process(handle.0, path) {
            return None;
        }
        let image = make_image(handle.0, path)?;
        let texture = toned_texture(image.0, reference.as_ref(), path)?;
        log_developed(path, half, reference.is_some(), started);
        Some(Developed {
            texture,
            full: !half,
        })
    }
}

unsafe fn open(path: &Path) -> Option<LibrawHandle> {
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let lr = unsafe { rsraw_sys::libraw_init(0) };
    if lr.is_null() {
        log::debug!("libraw_init returned null for {}", path.display());
        return None;
    }
    let handle = LibrawHandle(lr);
    let rc = unsafe { rsraw_sys::libraw_open_file(lr, c_path.as_ptr()) };
    succeeded(path, "libraw_open_file", rc).then_some(handle)
}

/// The embedded preview's luminance, from the thumbnail LibRaw already located (no second
/// pass over the file); `None` when there's none this can decode, or it's too small.
unsafe fn preview_luminance(lr: *mut libraw_data_t) -> Option<Histogram> {
    unsafe {
        if rsraw_sys::libraw_unpack_thumb(lr) != 0 {
            return None;
        }
        let mut errcode = 0;
        let thumb = rsraw_sys::libraw_dcraw_make_mem_thumb(lr, &mut errcode);
        if thumb.is_null() {
            return None;
        }
        let thumb = MemImage(thumb);
        if errcode != 0 {
            return None;
        }
        let bytes = image_bytes(thumb.0);
        let rgb = if (*thumb.0).type_ == rsraw_sys::LibRaw_image_formats_LIBRAW_IMAGE_JPEG {
            decompress_jpeg(bytes, Some(REFERENCE_EDGE)).ok()?.pixels
        } else {
            bitmap_rgb(thumb.0, bytes)?
        };
        raw_tone::sample_reference(&rgb)
    }
}

/// A bitmap thumbnail as 8-bit RGB; `None` for anything but three channels.
unsafe fn bitmap_rgb(thumb: *const libraw_processed_image_t, bytes: &[u8]) -> Option<Vec<u8>> {
    let (width, height, colors, bits) = unsafe {
        (
            (*thumb).width,
            (*thumb).height,
            (*thumb).colors,
            (*thumb).bits,
        )
    };
    if colors != 3 {
        return None;
    }
    bitmap_thumb_rgb8(bytes, bits, usize::from(width) * usize::from(height) * 3)
}

/// Linear 16-bit Display P3 at the camera's white balance, with LibRaw's auto-brightening
/// off: `raw_tone` sets the exposure. Returns whether it develops at half size.
unsafe fn configure(lr: *mut libraw_data_t, size: Size) -> bool {
    unsafe {
        let sizes = &(*lr).sizes;
        let half = match size {
            Size::Full => false,
            Size::Half => true,
            Size::Fit(edge) => u32::from(sizes.width.max(sizes.height)) / 2 >= edge,
        };
        let params = &mut (*lr).params;
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
}

/// Unpack and demosaic, as `configure` set up.
unsafe fn process(lr: *mut libraw_data_t, path: &Path) -> bool {
    unsafe {
        succeeded(path, "libraw_unpack", rsraw_sys::libraw_unpack(lr))
            && succeeded(
                path,
                "libraw_dcraw_process",
                rsraw_sys::libraw_dcraw_process(lr),
            )
    }
}

unsafe fn make_image(lr: *mut libraw_data_t, path: &Path) -> Option<MemImage> {
    let mut errcode = 0;
    let image = unsafe { rsraw_sys::libraw_dcraw_make_mem_image(lr, &mut errcode) };
    let image = (!image.is_null()).then(|| MemImage(image));
    if image.is_none() || errcode != 0 {
        log::debug!(
            "libraw_dcraw_make_mem_image failed ({errcode}) for {}",
            path.display()
        );
        return None;
    }
    image
}

/// LibRaw's 16-bit linear image toned to 8-bit Display P3: half the memory of the 16-bit
/// pixels, and what the screen shows anyway.
unsafe fn toned_texture(
    image: *const libraw_processed_image_t,
    reference: Option<&Histogram>,
    path: &Path,
) -> Option<gdk4::Texture> {
    let (width, height, colors, bits) = unsafe {
        (
            (*image).width,
            (*image).height,
            (*image).colors,
            (*image).bits,
        )
    };
    let rgb16 = unsafe { image_bytes(image) };
    let pixels = usize::from(width) * usize::from(height);
    if colors != 3 || bits != 16 || rgb16.len() < pixels * 6 {
        log::debug!(
            "libraw produced {colors}-channel {bits}-bit output for {} -- skipping",
            path.display()
        );
        return None;
    }
    let started = Instant::now();
    let lut = raw_tone::tone_lut(&raw_tone::sample_decode(rgb16), reference);
    let toned = raw_tone::apply(&lut, &rgb16[..pixels * 6]);
    log::debug!(
        "Toned {} in {}ms",
        path.display(),
        started.elapsed().as_millis()
    );
    let layout = color::Layout {
        width: i32::from(width),
        height: i32::from(height),
        stride: usize::from(width) * 3,
        format: gdk4::MemoryFormat::R8g8b8,
    };
    Some(color::tagged(
        &glib::Bytes::from_owned(toned),
        &layout,
        color::display_p3().as_ref(),
    ))
}

/// The pixel bytes LibRaw allocates after an image's header; valid while the image is.
unsafe fn image_bytes<'a>(image: *const libraw_processed_image_t) -> &'a [u8] {
    unsafe {
        let data = std::ptr::addr_of!((*image).data).cast::<u8>();
        std::slice::from_raw_parts(data, (*image).data_size as usize)
    }
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
            "toned like its preview"
        } else {
            "with the fallback tone"
        },
        started.elapsed().as_millis()
    );
}
