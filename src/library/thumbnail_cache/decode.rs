//! Decode local and remote image data into thumbnail textures.

use crate::api_client::ThumbnailSize;
use crate::i18n::fill;
use crate::library::color;
use gdk4::Texture;
use gdk4::prelude::TextureExt;
use gettextrs::gettext;
use glib::Bytes;

/// Decode a local file to a 256x256 thumbnail pixbuf, routing through video,
/// RAW, or standard image pipelines as appropriate.
pub(super) fn decode_local_pixbuf(
    path: &std::path::Path,
) -> Result<gtk::gdk_pixbuf::Pixbuf, String> {
    if crate::media_kinds::is_video_path(path) {
        return ffmpeg_extract_thumbnail(path);
    }
    if crate::media_kinds::is_raw_path(path) {
        return custom_decode_to_thumbnail(path).or_else(|_| {
            gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, 256, 256, true)
                .map(|raw| raw.apply_embedded_orientation().unwrap_or(raw))
                .map_err(|e| e.to_string())
        });
    }
    match gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, 256, 256, true) {
        Ok(raw) => Ok(in_srgb(
            raw.apply_embedded_orientation().unwrap_or(raw),
            path,
        )),
        Err(err) => {
            log::debug!(
                "gdk_pixbuf direct load failed for {}: {}; attempting custom decoder",
                path.display(),
                err
            );
            custom_decode_to_thumbnail(path)
        }
    }
}

/// gdk-pixbuf ignores colour profiles and the cache's JPEGs can't keep one, so a wide-gamut
/// photo's thumbnail is brought into sRGB here. Textures from the decoders arrive tagged,
/// and GTK does that while `custom_decode_to_thumbnail` reads them out.
fn in_srgb(pixbuf: gtk::gdk_pixbuf::Pixbuf, path: &std::path::Path) -> gtk::gdk_pixbuf::Pixbuf {
    let Some(icc) = color::file_icc_profile(path) else {
        return pixbuf;
    };
    let layout = color::pixbuf_layout(&pixbuf);
    let mut pixels = pixbuf.read_pixel_bytes().to_vec();
    if !color::convert_to_srgb(&mut pixels, &layout, &icc) {
        return pixbuf;
    }
    gtk::gdk_pixbuf::Pixbuf::from_bytes(
        &Bytes::from_owned(pixels),
        gtk::gdk_pixbuf::Colorspace::Rgb,
        pixbuf.has_alpha(),
        8,
        layout.width,
        layout.height,
        pixbuf.rowstride(),
    )
}

/// Extract a thumbnail frame from a video file using `ffmpeg`.
fn ffmpeg_extract_thumbnail(path: &std::path::Path) -> Result<gtk::gdk_pixbuf::Pixbuf, String> {
    let tmp_file = tempfile::Builder::new()
        .prefix("mimick_vthumb_")
        .suffix(".png")
        .tempfile()
        .map_err(|e| format!("failed to create temp file: {}", e))?;
    let tmp_path = tmp_file.path().to_path_buf();

    // Try 1s seek first (avoids black intro), fall back to 0s.
    if !run_ffmpeg_frame(path, "1", &tmp_path) {
        run_ffmpeg_frame(path, "0", &tmp_path);
    }

    if !tmp_path.exists() {
        return Err(format!(
            "ffmpeg failed to extract frame from {}",
            path.display()
        ));
    }
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_file(&tmp_path).map_err(|e| e.to_string())?;
    scale_pixbuf_to_thumbnail(pixbuf)
}

/// Run ffmpeg to extract a single frame at the given seek position.
fn run_ffmpeg_frame(input: &std::path::Path, seek_sec: &str, output: &std::path::Path) -> bool {
    use std::process::Command;
    Command::new("ffmpeg")
        .args(["-y", "-ss", seek_sec, "-i"])
        .arg(input)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=256:-1",
            "-loglevel",
            "error",
        ])
        .arg(output)
        .output()
        .map(|o| o.status.success() && output.exists())
        .unwrap_or(false)
}

/// Scale a pixbuf down to fit within 256x256 if needed.
fn scale_pixbuf_to_thumbnail(
    pixbuf: gtk::gdk_pixbuf::Pixbuf,
) -> Result<gtk::gdk_pixbuf::Pixbuf, String> {
    let (w, h) = (pixbuf.width(), pixbuf.height());
    if w <= 256 && h <= 256 {
        return Ok(pixbuf);
    }
    let scale = (256.0 / w as f64).min(256.0 / h as f64);
    let tw = ((w as f64 * scale).round() as i32).max(1);
    let th = ((h as f64 * scale).round() as i32).max(1);
    pixbuf
        .scale_simple(tw, th, gtk::gdk_pixbuf::InterpType::Bilinear)
        .ok_or_else(|| gettext("Failed to scale video thumbnail"))
}

/// Decodes an image file through the application's custom texture pipeline and
/// scale the result down to a 256x256 thumbnail pixbuf.
///
/// RAW paths use the thumbnail-specific decoder (embedded JPEG first, half-size
/// demosaic only as last resort) so the global "Full RAW Decoding" toggle
/// -- which is meant for lightbox quality -- never penalises grid loading.
fn custom_decode_to_thumbnail(path: &std::path::Path) -> Result<gtk::gdk_pixbuf::Pixbuf, String> {
    let full_texture = thumbnail_source(path).ok_or_else(|| {
        fill(
            gettext("No decoder succeeded for {path}"),
            &[("path", &path.display().to_string())],
        )
    })?;
    let mut downloader = gdk4::TextureDownloader::new(&full_texture);
    downloader.set_format(gdk4::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = downloader.download_bytes();
    // Wraps the downloaded pixels rather than copying them again.
    let full_pixbuf = gtk::gdk_pixbuf::Pixbuf::from_bytes(
        &bytes,
        gtk::gdk_pixbuf::Colorspace::Rgb,
        true,
        8,
        full_texture.width(),
        full_texture.height(),
        stride as i32,
    );
    let w = full_pixbuf.width();
    let h = full_pixbuf.height();
    let scale = (256.0 / w as f64).min(256.0 / h as f64).min(1.0);
    let tw = ((w as f64 * scale).round() as i32).max(1);
    let th = ((h as f64 * scale).round() as i32).max(1);
    full_pixbuf
        .scale_simple(tw, th, gtk::gdk_pixbuf::InterpType::Bilinear)
        .ok_or_else(|| gettext("Failed to scale pixbuf"))
}

/// The texture a thumbnail is scaled from: a HEIC's embedded thumbnail when it has one,
/// otherwise the whole photo, decoded only a few at a time.
fn thumbnail_source(path: &std::path::Path) -> Option<Texture> {
    if crate::library::is_heif_path(path)
        && let Some(thumbnail) = crate::library::decode_heif_thumbnail(path, THUMBNAIL_EDGE)
    {
        return Some(thumbnail);
    }
    let _slot = FullDecodeSlot::acquire();
    if crate::media_kinds::is_raw_path(path) {
        crate::library::decode_raw_thumbnail_texture(path)
    } else {
        crate::library::load_texture_blocking(path)
    }
}

const THUMBNAIL_EDGE: u32 = 256;
/// Full-size decodes for grid thumbnails at once; each can take hundreds of MB, and up to
/// 16 thumbnails load in parallel.
const FULL_DECODE_SLOTS: usize = 3;
static FULL_DECODES: parking_lot::Mutex<usize> = parking_lot::Mutex::new(0);
static FULL_DECODE_FREED: parking_lot::Condvar = parking_lot::Condvar::new();

/// Held while a full-size decode runs; waits while every slot is taken.
struct FullDecodeSlot;

impl FullDecodeSlot {
    fn acquire() -> Self {
        let mut used = FULL_DECODES.lock();
        while *used >= FULL_DECODE_SLOTS {
            FULL_DECODE_FREED.wait(&mut used);
        }
        *used += 1;
        Self
    }
}

impl Drop for FullDecodeSlot {
    fn drop(&mut self) {
        *FULL_DECODES.lock() -= 1;
        FULL_DECODE_FREED.notify_one();
    }
}

/// The bytes to keep on disk: JPEG, a fraction of PNG's size and quicker to read back,
/// unless the thumbnail has transparent pixels.
pub(super) fn pixbuf_cache_bytes(pixbuf: &gtk::gdk_pixbuf::Pixbuf) -> Result<Vec<u8>, String> {
    let width = pixbuf.width().max(1) as usize;
    let height = pixbuf.height().max(1) as usize;
    let channels = if pixbuf.has_alpha() { 4 } else { 3 };
    let rowstride = pixbuf.rowstride() as usize;
    let bytes = pixbuf.read_pixel_bytes();
    let packed = pack_pixel_rows(bytes.as_ref(), width, height, channels, rowstride)?;
    let (width, height) = (width as u32, height as u32);
    if channels == 4 && !is_opaque(&packed) {
        return encode_png_bytes(&packed, width, height, true);
    }
    let rgb = if channels == 4 {
        drop_alpha(&packed)
    } else {
        packed
    };
    encode_jpeg_bytes(&rgb, width, height)
}

fn is_opaque(rgba: &[u8]) -> bool {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[3] == u8::MAX)
}

fn drop_alpha(rgba: &[u8]) -> Vec<u8> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect()
}

/// Quality 85 keeps a 256 px thumbnail free of visible artefacts.
fn encode_jpeg_bytes(rgb: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let mut encoded = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 85)
        .write_image(rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|err| err.to_string())?;
    Ok(encoded)
}

fn pack_pixel_rows(
    src: &[u8],
    width: usize,
    height: usize,
    channels: usize,
    rowstride: usize,
) -> Result<Vec<u8>, String> {
    let mut packed = Vec::with_capacity(width * height * channels);
    for row in 0..height {
        let start = row
            .checked_mul(rowstride)
            .ok_or_else(|| "pixbuf row offset overflow".to_string())?;
        let end = start
            .checked_add(width * channels)
            .ok_or_else(|| "pixbuf row end overflow".to_string())?;
        let row_bytes = src
            .get(start..end)
            .ok_or_else(|| "pixbuf row outside buffer".to_string())?;
        packed.extend_from_slice(row_bytes);
    }
    Ok(packed)
}

fn encode_png_bytes(
    packed: &[u8],
    width: u32,
    height: u32,
    has_alpha: bool,
) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let color = if has_alpha {
        image::ColorType::Rgba8
    } else {
        image::ColorType::Rgb8
    };
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(packed, width, height, color.into())
        .map_err(|err| err.to_string())?;
    Ok(encoded)
}

pub(super) fn target_dim_for_bucket(size: ThumbnailSize) -> i32 {
    match size {
        ThumbnailSize::Thumbnail => 256,
        ThumbnailSize::Preview => 1440,
        ThumbnailSize::Fullsize => 2560,
    }
}

/// Asynchronously decode image bytes into a scaled texture.
pub(super) async fn decode_to_scaled_texture(
    bytes: Vec<u8>,
    max_dim: i32,
) -> Result<Texture, String> {
    tokio::task::spawn_blocking(move || -> Result<Texture, String> {
        // Immich makes previews of wide-gamut photos in Display P3, with the profile attached.
        let icc = color::icc_profile(&bytes);
        let stream = gtk::gio::MemoryInputStream::from_bytes(&Bytes::from_owned(bytes));
        let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
            &stream,
            max_dim,
            max_dim,
            true,
            gtk::gio::Cancellable::NONE,
        )
        .map_err(|err| err.to_string())?;
        let layout = color::pixbuf_layout(&pixbuf);
        Ok(color::texture(
            pixbuf.read_pixel_bytes(),
            &layout,
            icc.as_deref(),
        ))
    })
    .await
    .map_err(|err| err.to_string())?
}

pub(super) fn check_cancelled(is_cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if is_cancelled() {
        Err("cancelled".to_string())
    } else {
        Ok(())
    }
}

/// A cached file's bytes, if it's there; reading it counts as using it.
pub(super) async fn read_cached_bytes(
    cache_file: std::path::PathBuf,
) -> Result<Option<Vec<u8>>, String> {
    tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&cache_file).ok()?;
        crate::cache_manager::mark_used(&cache_file);
        Some(bytes)
    })
    .await
    .map_err(|err| err.to_string())
}

/// A thumbnail previously written to the on-disk cache, if present and readable.
pub(super) async fn read_cached_texture(
    cache_file: std::path::PathBuf,
) -> Result<Option<Texture>, String> {
    tokio::task::spawn_blocking(move || {
        if !cache_file.exists() {
            return None;
        }
        let texture = Texture::from_filename(&cache_file).ok()?;
        crate::cache_manager::mark_used(&cache_file);
        Some(texture)
    })
    .await
    .map_err(|err| err.to_string())
}

pub(super) fn pixbuf_to_texture(pixbuf: &gtk::gdk_pixbuf::Pixbuf) -> Texture {
    let format = if pixbuf.has_alpha() {
        gdk4::MemoryFormat::R8g8b8a8
    } else {
        gdk4::MemoryFormat::R8g8b8
    };
    let bytes = pixbuf.read_pixel_bytes();
    let mem_tex = gdk4::MemoryTexture::new(
        pixbuf.width(),
        pixbuf.height(),
        format,
        &bytes,
        pixbuf.rowstride() as usize,
    );
    use gtk::prelude::Cast;
    mem_tex.upcast::<Texture>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fully_opaque_thumbnails_lose_their_alpha() {
        assert!(is_opaque(&[1, 2, 3, 255, 4, 5, 6, 255]));
        assert!(!is_opaque(&[1, 2, 3, 255, 4, 5, 6, 0]));
        assert_eq!(
            drop_alpha(&[1, 2, 3, 255, 4, 5, 6, 255]),
            [1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn opaque_thumbnails_are_written_as_jpeg() {
        let encoded = encode_jpeg_bytes(&[200; 2 * 2 * 3], 2, 2).unwrap();
        assert_eq!(&encoded[..3], &[0xFF, 0xD8, 0xFF]);
    }

    #[test]
    fn full_decode_slots_free_up_when_dropped() {
        let held: Vec<FullDecodeSlot> = (0..FULL_DECODE_SLOTS)
            .map(|_| FullDecodeSlot::acquire())
            .collect();
        assert_eq!(*FULL_DECODES.lock(), FULL_DECODE_SLOTS);
        drop(held);
        assert_eq!(*FULL_DECODES.lock(), 0);
    }
}
