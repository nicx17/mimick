//! Decode local and remote image data into thumbnail textures.

use crate::api_client::ThumbnailSize;
use gdk4::Texture;
use gdk4::prelude::TextureExt;
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
        Ok(raw) => Ok(raw.apply_embedded_orientation().unwrap_or(raw)),
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
        .ok_or_else(|| "Failed to scale video thumbnail".to_string())
}

/// Decodes an image file through the application's custom texture pipeline and
/// scale the result down to a 256x256 thumbnail pixbuf.
///
/// RAW paths use the thumbnail-specific decoder (embedded JPEG first, half-size
/// demosaic only as last resort) so the global "Full RAW Decoding" toggle
/// -- which is meant for lightbox quality -- never penalises grid loading.
fn custom_decode_to_thumbnail(path: &std::path::Path) -> Result<gtk::gdk_pixbuf::Pixbuf, String> {
    let full_texture = if crate::media_kinds::is_raw_path(path) {
        crate::library::decode_raw_thumbnail_texture(path)
    } else {
        crate::library::load_texture_blocking(path)
    }
    .ok_or_else(|| format!("No decoder succeeded for {}", path.display()))?;
    let mut downloader = gdk4::TextureDownloader::new(&full_texture);
    downloader.set_format(gdk4::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = downloader.download_bytes();
    let full_pixbuf = gtk::gdk_pixbuf::Pixbuf::from_mut_slice(
        bytes.to_vec(),
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
        .ok_or_else(|| "Failed to scale pixbuf".to_string())
}

pub(super) fn pixbuf_png_bytes(pixbuf: &gtk::gdk_pixbuf::Pixbuf) -> Result<Vec<u8>, String> {
    let width = pixbuf.width().max(1) as usize;
    let height = pixbuf.height().max(1) as usize;
    let channels = if pixbuf.has_alpha() { 4 } else { 3 };
    let rowstride = pixbuf.rowstride() as usize;
    let bytes = pixbuf.read_pixel_bytes();

    let packed = pack_pixel_rows(bytes.as_ref(), width, height, channels, rowstride)?;
    encode_png_bytes(&packed, width as u32, height as u32, pixbuf.has_alpha())
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
        let stream = gtk::gio::MemoryInputStream::from_bytes(&Bytes::from_owned(bytes));
        let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
            &stream,
            max_dim,
            max_dim,
            true,
            gtk::gio::Cancellable::NONE,
        )
        .map_err(|err| err.to_string())?;
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
        Ok(mem_tex.upcast::<Texture>())
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

/// A thumbnail previously written to the on-disk cache, if present and readable.
pub(super) async fn read_cached_texture(
    cache_file: std::path::PathBuf,
) -> Result<Option<Texture>, String> {
    tokio::task::spawn_blocking(move || {
        if !cache_file.exists() {
            return None;
        }
        Texture::from_filename(&cache_file).ok()
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
