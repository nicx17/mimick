//! Image decoding pipeline -- codec dispatch, RAW/HEIF/JXL/JPEG2K/SVG/PSD/WebP decoders,
//! embedded-preview extraction, EXIF orientation, and decode-cache management.
//!
//! This module was extracted from `library/mod.rs` to keep the main module
//! focused on window construction and navigation.

use gtk::prelude::*;

use super::color;

mod codecs;
mod heif_thumbnail;
mod raw_develop;
mod raw_tone;
pub(super) use heif_thumbnail::{decode_heif_thumbnail, is_heif_path};

/// Runtime flag controlling the on-disk RAW decode cache.
/// Initialised from config at startup; toggled live from the settings UI.
static RAW_CACHE_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Update the runtime RAW cache flag (called from settings and startup).
pub fn set_raw_cache_enabled(enabled: bool) {
    RAW_CACHE_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// Runtime flag: true = full demosaic, false = extract embedded JPEG preview.
static RAW_FULL_DECODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Update the runtime RAW full-decode flag (called from settings and startup).
pub fn set_raw_full_decode(enabled: bool) {
    RAW_FULL_DECODE.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextureDecoder {
    Raw,
    Heif,
    JpegXl,
    Svg,
    Jpeg,
    Webp,
    Jpeg2k,
    Psd,
    Pixbuf,
    ImageFallback,
}

pub(super) fn load_texture_blocking(path: &std::path::Path) -> Option<gdk4::Texture> {
    let started = std::time::Instant::now();
    let decoder = texture_decoder_for_path(path);
    let (result, winning_route) = decode_with_fallbacks(path, decoder);
    log_decode_result(
        path,
        decoder,
        winning_route,
        started.elapsed().as_millis(),
        &result,
    );
    result
}

/// Whether full RAW decoding develops `path`, so it can take the look of the grid's picture.
pub(super) fn develops_raw(path: &std::path::Path) -> bool {
    RAW_FULL_DECODE.load(std::sync::atomic::Ordering::Relaxed)
        && texture_decoder_for_path(path) == TextureDecoder::Raw
}

/// Like [`load_texture_blocking`], with a RAW that full decoding develops taking the look
/// of `look`, the picture the grid shows for it.
pub(super) fn load_texture_like(
    path: &std::path::Path,
    look: Option<&gdk4::Texture>,
) -> Option<gdk4::Texture> {
    look_reference(path, look)
        .and_then(|reference| decode_libraw_texture(path, Some(&reference)))
        .or_else(|| load_texture_blocking(path))
}

fn look_reference(
    path: &std::path::Path,
    look: Option<&gdk4::Texture>,
) -> Option<raw_tone::Reference> {
    look.filter(|_| develops_raw(path))
        .and_then(raw_develop::reference_from_texture)
}

/// Decode `path` to fit `max_edge` pixels on its long side; JPEGs shrink while decoding,
/// and a RAW takes the look of `look` as in [`load_texture_like`].
pub(super) fn load_texture_fitted_blocking(
    path: &std::path::Path,
    max_edge: u32,
    look: Option<&gdk4::Texture>,
) -> Option<gdk4::Texture> {
    let started = std::time::Instant::now();
    let reference = look_reference(path, look);
    let texture = decode_shrinking(path, max_edge, reference.as_ref())
        .or_else(|| load_texture_blocking(path))?;
    let texture = fit_texture(texture, max_edge);
    log::debug!(
        "Decoded {} to fit {max_edge}px in {}ms ({}x{})",
        path.display(),
        started.elapsed().as_millis(),
        texture.width(),
        texture.height(),
    );
    Some(texture)
}

/// Decoders that can shrink as they go: JPEG's DCT scaling, and full RAW decoding at half
/// size when that still fills `max_edge` (zooming in loads every pixel).
fn decode_shrinking(
    path: &std::path::Path,
    max_edge: u32,
    reference: Option<&raw_tone::Reference>,
) -> Option<gdk4::Texture> {
    match texture_decoder_for_path(path) {
        TextureDecoder::Jpeg => decode_jpeg(path, Some(max_edge)),
        TextureDecoder::Raw if develops_raw(path) => decode_raw_fitted(path, max_edge, reference),
        _ => None,
    }
}

/// Developed to fit; a decode that came out full size is cached for zooming in.
fn decode_raw_fitted(
    path: &std::path::Path,
    max_edge: u32,
    reference: Option<&raw_tone::Reference>,
) -> Option<gdk4::Texture> {
    let raw = raw_develop::develop(path, raw_develop::Size::Fit(max_edge), reference)?;
    if raw.full && RAW_CACHE_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        write_raw_decode_cache_async(path, reference, &raw.texture);
    }
    Some(raw.texture)
}

fn fitted_size(width: i32, height: i32, max_edge: u32) -> Option<(i32, i32)> {
    let max = i32::try_from(max_edge).ok()?;
    let long = width.max(height);
    if long <= max {
        return None;
    }
    let scale = f64::from(max) / f64::from(long);
    let fit = |side: i32| ((f64::from(side) * scale).round() as i32).max(1);
    Some((fit(width), fit(height)))
}

fn fit_pixbuf(pixbuf: gtk::gdk_pixbuf::Pixbuf, max_edge: u32) -> gtk::gdk_pixbuf::Pixbuf {
    fitted_size(pixbuf.width(), pixbuf.height(), max_edge)
        .and_then(|(w, h)| pixbuf.scale_simple(w, h, gtk::gdk_pixbuf::InterpType::Bilinear))
        .unwrap_or(pixbuf)
}

/// Scaled in 8-bit Display P3 when the photo is wider than sRGB, so it keeps its colours.
fn fit_texture(texture: gdk4::Texture, max_edge: u32) -> gdk4::Texture {
    if fitted_size(texture.width(), texture.height(), max_edge).is_none() {
        return texture;
    }
    let wide = (texture.color_state() != gdk4::ColorState::srgb())
        .then(color::display_p3)
        .flatten();
    let mut downloader = gdk4::TextureDownloader::new(&texture);
    downloader.set_format(gdk4::MemoryFormat::R8g8b8a8);
    if let Some(state) = &wide {
        downloader.set_color_state(state);
    }
    let (bytes, stride) = downloader.download_bytes();
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_bytes(
        &bytes,
        gtk::gdk_pixbuf::Colorspace::Rgb,
        true,
        8,
        texture.width(),
        texture.height(),
        stride as i32,
    );
    let fitted = fit_pixbuf(pixbuf, max_edge);
    let layout = color::pixbuf_layout(&fitted);
    color::tagged(&fitted.read_pixel_bytes(), &layout, wide.as_ref())
}

fn decode_with_fallbacks(
    path: &std::path::Path,
    decoder: TextureDecoder,
) -> (Option<gdk4::Texture>, TextureDecoder) {
    if let Some(texture) = decode_with_route(path, decoder) {
        return (Some(texture), decoder);
    }
    if decoder != TextureDecoder::Pixbuf
        && let Some(texture) = decode_pixbuf_texture(path)
    {
        return (Some(texture), TextureDecoder::Pixbuf);
    }
    if let Some(texture) = decode_image_texture(path) {
        return (Some(texture), TextureDecoder::ImageFallback);
    }
    (None, decoder)
}

fn decode_with_route(path: &std::path::Path, decoder: TextureDecoder) -> Option<gdk4::Texture> {
    match decoder {
        TextureDecoder::Raw => decode_raw_texture(path),
        TextureDecoder::Heif => decode_heif_texture(path),
        TextureDecoder::JpegXl => codecs::decode_jpegxl_texture(path),
        TextureDecoder::Svg => codecs::decode_svg_texture(path),
        TextureDecoder::Jpeg => decode_jpeg_texture(path),
        TextureDecoder::Webp => decode_webp_texture(path),
        TextureDecoder::Jpeg2k => codecs::decode_jpeg2k_texture(path),
        TextureDecoder::Psd => decode_psd_texture(path),
        TextureDecoder::Pixbuf => decode_pixbuf_texture(path),
        TextureDecoder::ImageFallback => None,
    }
}

fn log_decode_result(
    path: &std::path::Path,
    decoder: TextureDecoder,
    winning_route: TextureDecoder,
    elapsed_ms: u128,
    result: &Option<gdk4::Texture>,
) {
    match &result {
        Some(texture) => {
            if winning_route == decoder {
                log::debug!(
                    "Decoded {} via {:?} in {}ms ({}x{})",
                    path.display(),
                    decoder,
                    elapsed_ms,
                    texture.width(),
                    texture.height(),
                );
            } else {
                log::debug!(
                    "Decoded {} via {:?} fallback (primary {:?} failed) in {}ms ({}x{})",
                    path.display(),
                    winning_route,
                    decoder,
                    elapsed_ms,
                    texture.width(),
                    texture.height(),
                );
            }
        }
        None => log::warn!(
            "All decoders rejected {} (route {:?}) after {}ms",
            path.display(),
            decoder,
            elapsed_ms,
        ),
    }
}

fn texture_decoder_for_path(path: &std::path::Path) -> TextureDecoder {
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
    match ext.as_deref() {
        Some(ext) if crate::media_kinds::is_raw_ext(ext) => TextureDecoder::Raw,
        Some("heic" | "heif" | "hif" | "avif") => TextureDecoder::Heif,
        Some("jxl") => TextureDecoder::JpegXl,
        Some("svg" | "svgz") => TextureDecoder::Svg,
        Some("jpe" | "jpeg" | "jpg" | "insp") => TextureDecoder::Jpeg,
        Some("webp") => TextureDecoder::Webp,
        Some("jp2") => TextureDecoder::Jpeg2k,
        Some("psd") => TextureDecoder::Psd,
        Some("bmp" | "gif" | "png" | "tif" | "tiff") => TextureDecoder::Pixbuf,
        _ => TextureDecoder::ImageFallback,
    }
}

fn memory_texture(
    width: u32,
    height: u32,
    format: gdk4::MemoryFormat,
    pixels: Vec<u8>,
    stride: usize,
) -> Option<gdk4::Texture> {
    let width = i32::try_from(width).ok()?;
    let height = i32::try_from(height).ok()?;
    let bytes = glib::Bytes::from_owned(pixels);
    let texture = gdk4::MemoryTexture::new(width, height, format, &bytes, stride);
    Some(texture.upcast::<gdk4::Texture>())
}

/// Thumbnail-targeted RAW decode: embedded JPEG first, half-size demosaic only as
/// last resort. Ignores `RAW_FULL_DECODE` (which is for lightbox quality, not
/// 256-px grid tiles -- full sensor data would just be scaled away).
pub(super) fn decode_raw_thumbnail_texture(path: &std::path::Path) -> Option<gdk4::Texture> {
    if let Some(tex) = extract_libraw_thumb(path) {
        return Some(tex);
    }
    log::debug!(
        "No embedded preview in {}; thumbnail falling back to half-size decode",
        path.display()
    );
    if RAW_CACHE_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
        && let Some(texture) = read_raw_decode_cache(path, None)
    {
        return Some(texture);
    }
    raw_develop::develop(path, raw_develop::Size::Half, None).map(|raw| raw.texture)
}

fn decode_raw_texture(path: &std::path::Path) -> Option<gdk4::Texture> {
    let full_decode = RAW_FULL_DECODE.load(std::sync::atomic::Ordering::Relaxed);
    if full_decode {
        decode_full_raw_texture(path)
    } else {
        decode_raw_preview_or_fallback(path)
    }
}

fn decode_full_raw_texture(path: &std::path::Path) -> Option<gdk4::Texture> {
    decode_libraw_texture(path, None)
}

fn decode_raw_preview_or_fallback(path: &std::path::Path) -> Option<gdk4::Texture> {
    if let Some(texture) = extract_libraw_thumb(path) {
        return Some(texture);
    }
    log::debug!(
        "No embedded preview in {}; falling back to full decode",
        path.display()
    );
    decode_libraw_texture(path, None)
}

/// Return the directory used for the on-disk RAW decode cache.
fn raw_decode_cache_dir() -> std::path::PathBuf {
    crate::profile::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp").join(crate::profile::dir_segment()))
        .join("raw_decode")
}

/// Build a stable cache filename for a RAW file based on its canonical path,
/// last-modified timestamp (seconds), byte length, and the look it was toned to.  Any
/// change to the file on disk produces a different key so stale pixels are never served.
fn raw_decode_cache_key(
    path: &std::path::Path,
    reference: Option<&raw_tone::Reference>,
) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let size = meta.len();
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    use std::os::unix::ffi::OsStrExt;
    let mut hasher = blake3::Hasher::new();
    // Bumped when decodes change: v4 is 8-bit Display P3 matched channel by channel to a
    // reference picture, replacing v2's flat 16-bit linear Rec. 2020.
    hasher.update(b"v4");
    hasher.update(canonical.as_os_str().as_bytes());
    hasher.update(&mtime.to_le_bytes());
    hasher.update(&size.to_le_bytes());
    // Without one, the look comes from the file's own preview, which the rest covers.
    if let Some(reference) = reference {
        hasher.update(&reference.fingerprint());
    }
    Some(format!("{}.png", &hasher.finalize().to_hex()[..32]))
}

/// Attempt to load a previously cached demosaiced texture from disk.
/// Returns `None` if the cache entry is absent, stale, or unreadable.
fn read_raw_decode_cache(
    path: &std::path::Path,
    reference: Option<&raw_tone::Reference>,
) -> Option<gdk4::Texture> {
    let key = raw_decode_cache_key(path, reference)?;
    let cache_file = raw_decode_cache_dir().join(&key);
    if !cache_file.exists() {
        return None;
    }
    match gdk4::Texture::from_filename(&cache_file) {
        Ok(texture) => {
            log::debug!("RAW decode cache hit for {}", path.display());
            Some(texture)
        }
        Err(err) => {
            log::debug!(
                "RAW decode cache read failed for {}: {}",
                path.display(),
                err
            );
            // Remove the corrupt entry so the next decode replaces it cleanly.
            let _ = std::fs::remove_file(&cache_file);
            None
        }
    }
}

/// Persist a successfully demosaiced RAW texture to the disk cache so future
/// opens can skip the expensive libraw pipeline entirely.
fn write_raw_decode_cache(
    path: &std::path::Path,
    reference: Option<&raw_tone::Reference>,
    texture: &gdk4::Texture,
) {
    let Some(key) = raw_decode_cache_key(path, reference) else {
        return;
    };
    let cache_dir = raw_decode_cache_dir();
    if let Err(err) = std::fs::create_dir_all(&cache_dir) {
        log::debug!("RAW decode cache dir create failed: {}", err);
        return;
    }
    let cache_file = cache_dir.join(&key);
    // Save as PNG via GDK so we roundtrip losslessly through the same loader.
    if let Err(err) = save_png_atomically(texture, &cache_file) {
        log::debug!(
            "RAW decode cache write failed for {}: {}",
            path.display(),
            err
        );
    } else {
        log::debug!("RAW decode cache written for {}", path.display());
    }
}
fn save_png_atomically(texture: &gdk4::Texture, dest: &std::path::Path) -> Result<(), String> {
    let dir = dest.parent().ok_or("cache path has no parent")?;
    let tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    texture.save_to_png(tmp.path()).map_err(|e| e.to_string())?;
    tmp.persist(dest).map_err(|e| e.to_string())?;
    Ok(())
}

/// Minimum size for an embedded preview to be considered -- tiny thumbnails
/// (EXIF 160x120 stubs) are almost always useless and should be skipped in
/// favour of the full-resolution preview or the libraw fallback.
const MIN_EMBEDDED_JPEG_SIZE: usize = 4096;

/// True if the JPEG at `bytes[start..end]` uses SOF3 (lossless JPEG).
/// RAW files wrap compressed Bayer data in SOF3 containers -- these are
/// never renderable and should be skipped by the preview scanner.
fn is_lossless_jpeg(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut pos = start + 2;
    while pos + 3 < end {
        if bytes[pos] != 0xFF {
            return false;
        }
        let marker = bytes[pos + 1];
        match classify_jpeg_marker(marker) {
            MarkerKind::Fill => {
                pos += 1;
            }
            MarkerKind::Parameterless => {
                pos += 2;
            }
            MarkerKind::Sof => return marker == 0xC3,
            MarkerKind::Sos => return false,
            MarkerKind::Segment => {
                if let Some(next) = skip_segment(bytes, pos, end) {
                    pos = next;
                } else {
                    return false;
                }
            }
        }
    }
    false
}

enum MarkerKind {
    Fill,
    Parameterless,
    Sof,
    Sos,
    Segment,
}

fn classify_jpeg_marker(marker: u8) -> MarkerKind {
    match marker {
        0xFF => MarkerKind::Fill,
        0x01 | 0xD0..=0xD7 => MarkerKind::Parameterless,
        0xC0..=0xCF if marker != 0xC4 && marker != 0xC8 && marker != 0xCC => MarkerKind::Sof,
        0xDA => MarkerKind::Sos,
        _ => MarkerKind::Segment,
    }
}

/// Read and skip a variable-length JPEG segment. Returns the position after
/// the segment, or None if the segment is malformed.
fn skip_segment(bytes: &[u8], pos: usize, end: usize) -> Option<usize> {
    if pos + 3 >= end {
        return None;
    }
    let seg_len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
    if seg_len < 2 || pos + 2 + seg_len > end {
        return None;
    }
    Some(pos + 2 + seg_len)
}

/// Return the largest renderable JPEG embedded in the file, or None.
/// Uses structure-aware marker walking; skips lossless SOF3 payloads
/// (compressed Bayer data) and payloads below `MIN_EMBEDDED_JPEG_SIZE`.
fn extract_largest_embedded_jpeg(path: &std::path::Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    let mut best: Option<(usize, usize)> = None;
    let len = bytes.len();
    let mut i = 0;

    while i + 3 < len {
        if !is_embedded_jpeg_start(&bytes, i) {
            i += 1;
            continue;
        }
        if let Some(end) = find_jpeg_end(&bytes, i) {
            best = choose_best_embedded_jpeg(&bytes, best, i, end);
            i = end;
        } else {
            i += 2;
        }
    }

    best.map(|(start, l)| bytes[start..start + l].to_vec())
}

fn is_embedded_jpeg_start(bytes: &[u8], i: usize) -> bool {
    bytes[i] == 0xFF && bytes[i + 1] == 0xD8 && bytes[i + 2] == 0xFF && bytes[i + 3] != 0x00
}

fn choose_best_embedded_jpeg(
    bytes: &[u8],
    best: Option<(usize, usize)>,
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let payload_len = end - start;
    let candidate_ok =
        payload_len >= MIN_EMBEDDED_JPEG_SIZE && !is_lossless_jpeg(bytes, start, end);
    if candidate_ok && best.is_none_or(|(_, len)| payload_len > len) {
        Some((start, payload_len))
    } else {
        best
    }
}

/// Walk the JPEG marker structure starting at the SOI at `bytes[start]` and
/// return the byte offset just past the EOI (or the end of the buffer if the
/// file omits the trailing EOI).
///
/// The scanner reads marker segments by their declared lengths, then
/// byte-scans only through the entropy-coded section after SOS where
/// FF 00 (byte-stuffing) and FF D0-D7 (restart markers) are skipped.
fn find_jpeg_end(bytes: &[u8], start: usize) -> Option<usize> {
    let len = bytes.len();
    let mut pos = start + 2;

    loop {
        pos = seek_marker(bytes, pos);
        if pos + 1 >= len {
            return Some(len);
        }

        let marker = bytes[pos + 1];
        match marker {
            0x00 => pos += 2,
            0xD9 => return Some(pos + 2),
            0xD8 => return Some(pos),
            0xD0..=0xD7 => pos += 2,
            0xDA => match scan_sos_entropy(bytes, pos) {
                Some(next) => pos = next,
                None => return Some(len),
            },
            0x01 => pos += 2,
            _ => match skip_or_accept(bytes, pos) {
                Some(next) => pos = next,
                None => return Some(len),
            },
        }
    }
}

/// Advance past non-marker bytes and padding 0xFF fills, returning the
/// position of the next real marker prefix.
fn seek_marker(bytes: &[u8], mut pos: usize) -> usize {
    let len = bytes.len();
    while pos < len && bytes[pos] != 0xFF {
        pos += 1;
    }
    while pos + 1 < len && bytes[pos + 1] == 0xFF {
        pos += 1;
    }
    pos
}

/// Read a SOS header, then scan through entropy-coded data until the next
/// real marker. Returns the position of that marker, or None on truncation.
fn scan_sos_entropy(bytes: &[u8], pos: usize) -> Option<usize> {
    let len = bytes.len();
    if pos + 3 >= len {
        return None;
    }
    let seg_len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
    if seg_len < 2 {
        return None;
    }
    let mut p = pos + 2 + seg_len;
    while p + 1 < len {
        if bytes[p] == 0xFF {
            let next = bytes[p + 1];
            if next == 0x00 || (0xD0..=0xD7).contains(&next) {
                p += 2;
                continue;
            }
            return Some(p);
        }
        p += 1;
    }
    None
}

/// Skip a variable-length JPEG segment, or return None when the segment is
/// malformed (signals the caller to accept the whole remaining buffer).
fn skip_or_accept(bytes: &[u8], pos: usize) -> Option<usize> {
    let len = bytes.len();
    if pos + 3 >= len {
        return None;
    }
    let seg_len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
    if seg_len < 2 || pos + 2 + seg_len > len {
        return None;
    }
    Some(pos + 2 + seg_len)
}

/// Read EXIF orientation (tag 0x0112) from JPEG bytes. Returns 1..=8 or None.
/// Hand-rolled because `Pixbuf::apply_embedded_orientation` silently drops
/// EXIF on some RAW-embedded JPEGs (truncated/oversize APP1 segments).
fn read_jpeg_exif_orientation(bytes: &[u8]) -> Option<u8> {
    if !has_jpeg_soi(bytes) {
        return None;
    }
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            return None;
        }
        let marker = bytes[i + 1];
        if is_standalone_jpeg_marker(marker) {
            i += 2;
            continue;
        }
        let (seg, next) = jpeg_segment(bytes, i)?;
        if marker == 0xE1 && is_exif_segment(seg) {
            return parse_tiff_orientation(&seg[6..]);
        }
        i = next;
    }
    None
}

fn has_jpeg_soi(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8
}

fn is_standalone_jpeg_marker(marker: u8) -> bool {
    marker == 0xD8 || marker == 0xD9 || marker == 0x01 || (0xD0..=0xD7).contains(&marker)
}

fn jpeg_segment(bytes: &[u8], i: usize) -> Option<(&[u8], usize)> {
    let seg_len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
    if seg_len < 2 || i + 2 + seg_len > bytes.len() {
        return None;
    }
    Some((&bytes[i + 4..i + 2 + seg_len], i + 2 + seg_len))
}

fn is_exif_segment(seg: &[u8]) -> bool {
    seg.len() >= 6 && &seg[..6] == b"Exif\0\0"
}

/// Parse TIFF IFD0 starting at the TIFF header, return orientation if present.
fn parse_tiff_orientation(tiff: &[u8]) -> Option<u8> {
    if tiff.len() < 8 {
        return None;
    }
    let endian = TiffEndian::from_header(tiff)?;
    if endian.u16(&tiff[2..4]) != 0x002A {
        return None;
    }
    let ifd0 = endian.u32(&tiff[4..8]) as usize;
    if ifd0 + 2 > tiff.len() {
        return None;
    }
    let count = endian.u16(&tiff[ifd0..ifd0 + 2]) as usize;
    for n in 0..count {
        let off = ifd0 + 2 + n * 12;
        let entry = tiff.get(off..off + 12)?;
        if endian.u16(&entry[..2]) == 0x0112 {
            let v = endian.u16(&entry[8..10]) as u8;
            return (1..=8).contains(&v).then_some(v);
        }
    }
    None
}

#[derive(Clone, Copy)]
enum TiffEndian {
    Little,
    Big,
}

impl TiffEndian {
    fn from_header(tiff: &[u8]) -> Option<Self> {
        match &tiff[..2] {
            b"II" => Some(Self::Little),
            b"MM" => Some(Self::Big),
            _ => None,
        }
    }

    fn u16(self, p: &[u8]) -> u16 {
        match self {
            Self::Little => u16::from_le_bytes([p[0], p[1]]),
            Self::Big => u16::from_be_bytes([p[0], p[1]]),
        }
    }

    fn u32(self, p: &[u8]) -> u32 {
        match self {
            Self::Little => u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            Self::Big => u32::from_be_bytes([p[0], p[1], p[2], p[3]]),
        }
    }
}

/// Apply EXIF orientation (1..=8) to a Pixbuf via rotate + optional flip.
fn apply_exif_orientation(pixbuf: &gtk::gdk_pixbuf::Pixbuf, orient: u8) -> gtk::gdk_pixbuf::Pixbuf {
    use gtk::gdk_pixbuf::PixbufRotation;
    let rotated = match orient {
        3 | 4 => pixbuf.rotate_simple(PixbufRotation::Upsidedown),
        5 | 8 => pixbuf.rotate_simple(PixbufRotation::Counterclockwise),
        6 | 7 => pixbuf.rotate_simple(PixbufRotation::Clockwise),
        _ => Some(pixbuf.clone()),
    }
    .unwrap_or_else(|| pixbuf.clone());
    match orient {
        2 | 4 | 5 | 7 => rotated.flip(true).unwrap_or(rotated),
        _ => rotated,
    }
}

/// Check whether a thumbnail has already been rotated to display orientation
/// by the camera firmware. Compares the thumbnail's aspect ratio to the raw
/// sensor's: if a 90-degree flip is needed (flip=5 or flip=6) but the
/// thumbnail is already in the rotated aspect (portrait when sensor is
/// landscape, or vice versa), the thumbnail is pre-rotated.
fn is_thumbnail_prerotated(
    thumb_w: i32,
    thumb_h: i32,
    sensor_w: u32,
    sensor_h: u32,
    flip: std::ffi::c_int,
) -> bool {
    // Only relevant for 90-degree rotations.
    if flip != 5 && flip != 6 {
        return false;
    }
    let sensor_landscape = sensor_w >= sensor_h;
    let thumb_landscape = thumb_w >= thumb_h;
    // After a 90-degree rotation the aspect flips. If the thumbnail already
    // has the flipped aspect (portrait when sensor is landscape), the camera
    // stored it pre-rotated.
    sensor_landscape != thumb_landscape
}

/// Decode JPEG bytes to a texture, applying EXIF orientation or libraw flip.
///
/// The `flip` and `sensor_dims` parameters come from the RAW container (libraw).
/// When the decoded JPEG's aspect ratio already matches the rotated sensor
/// orientation, the container flip is skipped (the camera pre-rotated the
/// preview). When the JPEG carries its own EXIF orientation > 1, that value
/// is trusted unconditionally.
fn jpeg_bytes_to_oriented_texture(
    bytes: &[u8],
    flip: i32,
    sensor_dims: (u32, u32),
) -> Option<gdk4::Texture> {
    let stream = gtk::gio::MemoryInputStream::from_bytes(&glib::Bytes::from(bytes));
    let raw_pixbuf =
        gtk::gdk_pixbuf::Pixbuf::from_stream(&stream, gtk::gio::Cancellable::NONE).ok()?;
    let icc = color::icc_profile(bytes);
    let exif_orient = read_jpeg_exif_orientation(bytes);
    let oriented = match exif_orient {
        // Embedded JPEG carries a meaningful rotation -- trust it.
        Some(o) if o > 1 => apply_exif_orientation(&raw_pixbuf, o),
        // EXIF says identity (1) or no EXIF tag at all:
        // apply the container flip only if the thumbnail is NOT pre-rotated.
        _ => {
            if is_thumbnail_prerotated(
                raw_pixbuf.width(),
                raw_pixbuf.height(),
                sensor_dims.0,
                sensor_dims.1,
                flip,
            ) {
                raw_pixbuf
            } else {
                apply_libraw_flip(&raw_pixbuf, flip)
            }
        }
    };
    pixbuf_texture(&oriented, icc.as_deref())
}

fn extract_primary_embedded_jpeg(
    path: &std::path::Path,
    flip: i32,
    sensor_dims: (u32, u32),
) -> Option<gdk4::Texture> {
    if let Some(scan) = extract_largest_embedded_jpeg(path) {
        let scan_len = scan.len();
        if let Some(texture) = jpeg_bytes_to_oriented_texture(&scan, flip, sensor_dims) {
            log::debug!(
                "Extracted embedded JPEG preview ({} bytes via SOI-scan, flip={}) from {}",
                scan_len,
                flip,
                path.display()
            );
            return Some(texture);
        }
        log::debug!(
            "SOI-scanned JPEG ({} bytes) failed to decode for {}; falling back to libraw",
            scan_len,
            path.display()
        );
    }
    None
}

/// Embedded RAW preview: SOI-scan for the largest JPEG (primary), fall back
/// to libraw's bitmap/JPEG thumbnail for files that store the preview as
/// TIFF strips (Samsung DNG, OnePlus DNG, etc.). Full demosaic path untouched.
fn extract_libraw_thumb(path: &std::path::Path) -> Option<gdk4::Texture> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: Interfacing with the external LibRaw C API via FFI bindings.
    // We check that the context is initialized successfully (non-null),
    // and wrap the raw pointers in custom RAII guards (LibrawHandle, MemImage)
    // to guarantee that resources are cleaned up correctly when they go out of scope.
    unsafe {
        let lr = rsraw_sys::libraw_init(0);
        if lr.is_null() {
            return extract_primary_embedded_jpeg(path, 0, (0, 0));
        }
        let _guard = LibrawHandle(lr);
        if rsraw_sys::libraw_open_file(lr, c_path.as_ptr()) != 0 {
            log::debug!(
                "libraw cannot open {}; scanning for an embedded JPEG anyway",
                path.display()
            );
            return extract_primary_embedded_jpeg(path, 0, (0, 0));
        }

        let flip = (*lr).sizes.flip;
        let sensor_dims = ((*lr).sizes.width as u32, (*lr).sizes.height as u32);

        if let Some(texture) = extract_primary_embedded_jpeg(path, flip, sensor_dims) {
            return Some(texture);
        }

        unpack_libraw_fallback_thumb(lr, flip, sensor_dims, path)
    }
}

/// Unpack and decode the libraw fallback thumbnail (JPEG or bitmap).
unsafe fn unpack_libraw_fallback_thumb(
    lr: *mut rsraw_sys::libraw_data_t,
    flip: i32,
    sensor_dims: (u32, u32),
    path: &std::path::Path,
) -> Option<gdk4::Texture> {
    unsafe {
        if rsraw_sys::libraw_unpack_thumb(lr) != 0 {
            log::debug!("No embedded thumbnail in {}", path.display());
            return None;
        }
        let mut errcode = 0i32;
        let img = rsraw_sys::libraw_dcraw_make_mem_thumb(lr, &mut errcode);
        if img.is_null() || errcode != 0 {
            log::debug!(
                "libraw_dcraw_make_mem_thumb failed ({}) for {}",
                errcode,
                path.display()
            );
            return None;
        }
        let _img_guard = MemImage(img);
        let data_size = (*img).data_size as usize;
        // SAFETY: The image buffer is owned by the `img` memory structure, and its layout is
        // guaranteed to have at least `data_size` bytes. The slice does not outlive its owner
        // (as it is dropped prior to `_img_guard` and its contents are immediately copied).
        let bytes = std::slice::from_raw_parts((*img).data.as_ptr(), data_size);

        if (*img).type_ == rsraw_sys::LibRaw_image_formats_LIBRAW_IMAGE_JPEG {
            decode_libraw_jpeg_thumb(bytes, flip, sensor_dims, data_size, path)
        } else {
            decode_libraw_bitmap_thumb(bytes, &*img, flip, sensor_dims, path)
        }
    }
}

/// Decode a JPEG thumbnail extracted by libraw.
fn decode_libraw_jpeg_thumb(
    bytes: &[u8],
    flip: i32,
    sensor_dims: (u32, u32),
    data_size: usize,
    path: &std::path::Path,
) -> Option<gdk4::Texture> {
    let texture = jpeg_bytes_to_oriented_texture(bytes, flip, sensor_dims);
    if texture.is_some() {
        log::debug!(
            "Extracted embedded JPEG preview ({} bytes via libraw, flip={}) from {}",
            data_size,
            flip,
            path.display()
        );
    } else {
        log::debug!(
            "libraw JPEG thumb failed to decode via pixbuf for {}",
            path.display()
        );
    }
    texture
}

/// Decode a bitmap (uncompressed RGB) thumbnail extracted by libraw.
fn decode_libraw_bitmap_thumb(
    bytes: &[u8],
    img: &rsraw_sys::libraw_processed_image_t,
    flip: i32,
    sensor_dims: (u32, u32),
    path: &std::path::Path,
) -> Option<gdk4::Texture> {
    let pixbuf = bitmap_thumb_pixbuf(bytes, img, path)?;
    let (width, height) = (pixbuf.width(), pixbuf.height());
    let prerotated = is_thumbnail_prerotated(width, height, sensor_dims.0, sensor_dims.1, flip);
    let oriented = if prerotated {
        pixbuf
    } else {
        apply_libraw_flip(&pixbuf, flip)
    };
    let texture = pixbuf_to_texture(&oriented);
    if texture.is_some() {
        log::debug!(
            "Extracted embedded bitmap preview (flip={}, prerotated={}) from {}",
            flip,
            prerotated,
            path.display()
        );
    }
    texture
}

fn bitmap_thumb_pixbuf(
    bytes: &[u8],
    img: &rsraw_sys::libraw_processed_image_t,
    path: &std::path::Path,
) -> Option<gtk::gdk_pixbuf::Pixbuf> {
    let width = img.width as i32;
    let height = img.height as i32;
    let colors = img.colors as i32;
    if colors != 3 {
        log::debug!(
            "Embedded thumbnail has {} channels for {} -- skipping",
            colors,
            path.display()
        );
        return None;
    }
    let row_stride = width.checked_mul(colors)?;
    let samples = usize::try_from(row_stride.checked_mul(height)?).ok()?;
    let Some(pixels) = bitmap_thumb_rgb8(bytes, img.bits, samples) else {
        log::debug!(
            "Embedded thumbnail has unsupported {}-bit layout for {} -- skipping",
            img.bits,
            path.display()
        );
        return None;
    };
    Some(gtk::gdk_pixbuf::Pixbuf::from_mut_slice(
        pixels,
        gtk::gdk_pixbuf::Colorspace::Rgb,
        false,
        8,
        width,
        height,
        row_stride,
    ))
}

fn bitmap_thumb_rgb8(bytes: &[u8], bits: u16, samples: usize) -> Option<Vec<u8>> {
    match bits {
        8 => bytes.get(..samples).map(<[u8]>::to_vec),
        16 => {
            let wide = bytes.get(..samples.checked_mul(2)?)?;
            Some(
                wide.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| (u16::from_ne_bytes(*p) >> 8) as u8)
                    .collect(),
            )
        }
        _ => None,
    }
}

/// Convert a `Pixbuf` to a `gdk4::Texture`, taking its pixels as sRGB.
fn pixbuf_to_texture(pixbuf: &gtk::gdk_pixbuf::Pixbuf) -> Option<gdk4::Texture> {
    pixbuf_texture(pixbuf, None)
}

/// A `Pixbuf`'s pixels in the colour space `icc` describes.
fn pixbuf_texture(pixbuf: &gtk::gdk_pixbuf::Pixbuf, icc: Option<&[u8]>) -> Option<gdk4::Texture> {
    Some(color::texture(
        pixbuf.read_pixel_bytes(),
        &color::pixbuf_layout(pixbuf),
        icc,
    ))
}

/// libheif's RGBA in the colour space the file declares: an ICC profile (iPhones), or
/// H.273 codes that GTK takes as they are.
fn heif_texture(
    handle: &libheif_rs::ImageHandle,
    pixels: &[u8],
    (width, height): (u32, u32),
    stride: usize,
) -> Option<gdk4::Texture> {
    let layout = color::Layout {
        width: i32::try_from(width).ok()?,
        height: i32::try_from(height).ok()?,
        stride,
        format: gdk4::MemoryFormat::R8g8b8a8,
    };
    let bytes = glib::Bytes::from(pixels);
    if let Some(icc) = handle.color_profile_raw() {
        return Some(color::texture(bytes, &layout, Some(&icc.data)));
    }
    let state = handle.color_profile_nclx().and_then(|nclx| {
        color::cicp_state(
            nclx.color_primaries() as u32,
            nclx.transfer_characteristics() as u32,
        )
    });
    Some(color::tagged(&bytes, &layout, state.as_ref()))
}

/// Apply libraw `flip` orientation to a `Pixbuf`.
///
/// libraw flip values:
///   0 — no rotation (identity)
///   3 — 180 degrees
///   5 — 90 CCW (270 CW)
///   6 — 90 CW
///
/// Other values (rare) are left unrotated.
fn apply_libraw_flip(
    pixbuf: &gtk::gdk_pixbuf::Pixbuf,
    flip: std::ffi::c_int,
) -> gtk::gdk_pixbuf::Pixbuf {
    use gtk::gdk_pixbuf::PixbufRotation;
    match flip {
        3 => pixbuf
            .rotate_simple(PixbufRotation::Upsidedown)
            .unwrap_or_else(|| pixbuf.clone()),
        5 => pixbuf
            .rotate_simple(PixbufRotation::Counterclockwise)
            .unwrap_or_else(|| pixbuf.clone()),
        6 => pixbuf
            .rotate_simple(PixbufRotation::Clockwise)
            .unwrap_or_else(|| pixbuf.clone()),
        _ => pixbuf.clone(),
    }
}

/// Decode a RAW via vendored libraw with camera white balance + AHD demosaic at full
/// resolution, toned like `reference` (else the camera's JPEG). Output goes through a disk
/// cache keyed by (path, mtime, size, look) so a re-open skips re-demosaicing.
fn decode_libraw_texture(
    path: &std::path::Path,
    reference: Option<&raw_tone::Reference>,
) -> Option<gdk4::Texture> {
    let cache_enabled = RAW_CACHE_ENABLED.load(std::sync::atomic::Ordering::Relaxed);
    if cache_enabled && let Some(texture) = read_raw_decode_cache(path, reference) {
        return Some(texture);
    }
    let texture =
        raw_develop::develop(path, raw_develop::Size::Full, reference).map(|raw| raw.texture);
    if let Some(ref tex) = texture
        && cache_enabled
    {
        write_raw_decode_cache_async(path, reference, tex);
    }
    texture
}

struct LibrawHandle(*mut rsraw_sys::libraw_data_t);

impl Drop for LibrawHandle {
    fn drop(&mut self) {
        unsafe { rsraw_sys::libraw_close(self.0) };
    }
}

struct MemImage(*mut rsraw_sys::libraw_processed_image_t);

impl Drop for MemImage {
    fn drop(&mut self) {
        unsafe { rsraw_sys::libraw_dcraw_clear_mem(self.0) };
    }
}

fn write_raw_decode_cache_async(
    path: &std::path::Path,
    reference: Option<&raw_tone::Reference>,
    texture: &gdk4::Texture,
) {
    let path_clone = path.to_path_buf();
    let reference = reference.cloned();
    let tex_clone = texture.clone();
    let write = move || write_raw_decode_cache(&path_clone, reference.as_ref(), &tex_clone);
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::spawn_blocking(write);
    } else {
        std::thread::spawn(write);
    }
}

fn decode_heif_texture(path: &std::path::Path) -> Option<gdk4::Texture> {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

    let libheif = LibHeif::new();
    let encoded = std::fs::read(path).ok()?;
    let context = HeifContext::read_from_bytes(&encoded).ok()?;
    let handle = context.primary_image_handle().ok()?;
    let image = libheif
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), None)
        .ok()?;
    let plane = image.planes().interleaved?;
    heif_texture(
        &handle,
        plane.data,
        (plane.width, plane.height),
        plane.stride,
    )
}

fn decode_image_texture(path: &std::path::Path) -> Option<gdk4::Texture> {
    let reader = match image::ImageReader::open(path) {
        Ok(reader) => reader,
        Err(err) => {
            log::debug!("image-rs open failed for {}: {}", path.display(), err);
            return None;
        }
    };
    let reader = match reader.with_guessed_format() {
        Ok(reader) => reader,
        Err(err) => {
            log::debug!(
                "image-rs format probe failed for {}: {}",
                path.display(),
                err
            );
            return None;
        }
    };
    match reader.decode() {
        Ok(image) => dynamic_image_texture(image),
        Err(err) => {
            log::debug!("image-rs decode failed for {}: {}", path.display(), err);
            None
        }
    }
}

fn dynamic_image_texture(image: image::DynamicImage) -> Option<gdk4::Texture> {
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    memory_texture(
        width,
        height,
        gdk4::MemoryFormat::R8g8b8a8,
        rgba.into_raw(),
        usize::try_from(width).ok()?.checked_mul(4)?,
    )
}

fn decode_pixbuf_texture(path: &std::path::Path) -> Option<gdk4::Texture> {
    let raw = match gtk::gdk_pixbuf::Pixbuf::from_file(path) {
        Ok(pixbuf) => pixbuf,
        Err(err) => {
            log::debug!("pixbuf decode failed for {}: {}", path.display(), err);
            return None;
        }
    };
    let pixbuf = raw.apply_embedded_orientation().unwrap_or(raw);
    pixbuf_texture(&pixbuf, color::file_icc_profile(path).as_deref())
}

fn decode_jpeg_texture(path: &std::path::Path) -> Option<gdk4::Texture> {
    decode_jpeg(path, None)
}

fn decode_jpeg(path: &std::path::Path, max_edge: Option<u32>) -> Option<gdk4::Texture> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::warn!("JPEG read failed for {}: {}", path.display(), err);
            return None;
        }
    };
    let orient = read_jpeg_exif_orientation(&bytes).filter(|&o| o != 1);
    let icc = color::icc_profile(&bytes);
    let image = match decompress_jpeg(&bytes, max_edge) {
        Ok(image) => image,
        Err(err) => {
            log::debug!("turbojpeg decode failed for {}: {}", path.display(), err);
            return None;
        }
    };
    let width: i32 = image.width.try_into().ok()?;
    let height: i32 = image.height.try_into().ok()?;
    let oversized = max_edge.is_some_and(|edge| fitted_size(width, height, edge).is_some());
    if orient.is_none() && !oversized {
        let layout = color::Layout {
            width,
            height,
            stride: image.pitch,
            format: gdk4::MemoryFormat::R8g8b8,
        };
        let pixels = glib::Bytes::from_owned(image.pixels);
        return Some(color::texture(pixels, &layout, icc.as_deref()));
    }
    let raw_pixbuf = gtk::gdk_pixbuf::Pixbuf::from_mut_slice(
        image.pixels,
        gtk::gdk_pixbuf::Colorspace::Rgb,
        false,
        8,
        width,
        height,
        image.pitch as i32,
    );
    let oriented = match orient {
        Some(o) => apply_exif_orientation(&raw_pixbuf, o),
        None => raw_pixbuf,
    };
    let fitted = match max_edge {
        Some(edge) => fit_pixbuf(oriented, edge),
        None => oriented,
    };
    pixbuf_texture(&fitted, icc.as_deref())
}

fn decompress_jpeg(
    bytes: &[u8],
    max_edge: Option<u32>,
) -> Result<turbojpeg::Image<Vec<u8>>, turbojpeg::Error> {
    let mut decompressor = turbojpeg::Decompressor::new()?;
    let header = decompressor.read_header(bytes)?;
    let factor = max_edge.map_or(turbojpeg::ScalingFactor::ONE, |edge| {
        jpeg_scale_for(header.width.max(header.height), edge)
    });
    decompressor.set_scaling_factor(factor)?;
    let scaled = header.scaled(factor);
    let format = turbojpeg::PixelFormat::RGB;
    let pitch = scaled.width * format.size();
    let mut image = turbojpeg::Image {
        pixels: vec![0; scaled.height * pitch],
        width: scaled.width,
        pitch,
        height: scaled.height,
        format,
    };
    decompressor.decompress(bytes, image.as_deref_mut())?;
    Ok(image)
}

fn jpeg_scale_for(long_edge: usize, max_edge: u32) -> turbojpeg::ScalingFactor {
    let target = max_edge as usize;
    turbojpeg::Decompressor::supported_scaling_factors()
        .into_iter()
        .filter(|factor| factor.num() <= factor.denom() && factor.scale(long_edge) >= target)
        .min_by_key(|factor| factor.scale(long_edge))
        .unwrap_or(turbojpeg::ScalingFactor::ONE)
}

fn decode_webp_texture(path: &std::path::Path) -> Option<gdk4::Texture> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::warn!("WebP read failed for {}: {}", path.display(), err);
            return None;
        }
    };
    let decoder = webp::Decoder::new(&bytes);
    let image = match decoder.decode() {
        Some(image) => image,
        None => {
            log::debug!("libwebp decode failed for {}", path.display());
            return None;
        }
    };
    let layout = webp_layout(&image)?;
    let icc = color::icc_profile(&bytes);
    let pixels = glib::Bytes::from_owned(image.to_vec());
    Some(color::texture(pixels, &layout, icc.as_deref()))
}

fn webp_layout(image: &webp::WebPImage) -> Option<color::Layout> {
    let (format, bpp) = if image.is_alpha() {
        (gdk4::MemoryFormat::R8g8b8a8, 4)
    } else {
        (gdk4::MemoryFormat::R8g8b8, 3)
    };
    Some(color::Layout {
        width: i32::try_from(image.width()).ok()?,
        height: i32::try_from(image.height()).ok()?,
        stride: usize::try_from(image.width()).ok()?.checked_mul(bpp)?,
        format,
    })
}

fn decode_psd_texture(path: &std::path::Path) -> Option<gdk4::Texture> {
    let bytes = std::fs::read(path)
        .map_err(|err| log::warn!("PSD read failed for {}: {}", path.display(), err))
        .ok()?;
    let psd = psd::Psd::from_bytes(&bytes)
        .map_err(|err| log::warn!("PSD parse failed for {}: {:?}", path.display(), err))
        .ok()?;
    let width = psd.width();
    let height = psd.height();
    memory_texture(
        width,
        height,
        gdk4::MemoryFormat::R8g8b8a8,
        psd.rgba(),
        usize::try_from(width).ok()?.checked_mul(4)?,
    )
}

#[cfg(test)]
mod texture_decoder_tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn assert_fixture_texture(name: &str, dimensions: (i32, i32)) {
        let texture = load_texture_blocking(&fixture(name))
            .unwrap_or_else(|| panic!("fixture `{name}` should decode into a texture"));
        assert_eq!(
            (texture.width(), texture.height()),
            dimensions,
            "fixture `{name}` decoded with unexpected dimensions"
        );
    }

    #[test]
    fn jpeg_scale_picks_the_smallest_factor_that_still_covers_the_edge() {
        assert_eq!(jpeg_scale_for(16384, 2560).scale(16384), 4096);
        assert_eq!(jpeg_scale_for(6000, 2560).scale(6000), 3000);
        assert_eq!(jpeg_scale_for(1000, 2560), turbojpeg::ScalingFactor::ONE);
    }

    #[test]
    fn fitted_decodes_shrink_to_the_edge_without_upscaling() {
        let dir = tempfile::tempdir().unwrap();
        let jpeg = dir.path().join("big.jpg");
        image::RgbImage::new(1600, 1200).save(&jpeg).unwrap();
        let png = dir.path().join("wide.png");
        image::RgbImage::new(1000, 500).save(&png).unwrap();
        let size = |path: &std::path::Path, edge| {
            let texture = load_texture_fitted_blocking(path, edge, None).unwrap();
            (texture.width(), texture.height())
        };
        assert_eq!(size(&jpeg, 400), (400, 300));
        assert_eq!(size(&jpeg, 500), (500, 375));
        assert_eq!(size(&png, 250), (250, 125));
        assert_eq!(size(&png, 4000), (1000, 500));
    }

    #[test]
    fn routes_special_lightbox_formats_before_loader_fallbacks() {
        for ext in crate::media_kinds::RAW_EXTENSIONS.iter() {
            assert_eq!(
                texture_decoder_for_path(std::path::Path::new(&format!("camera.{ext}"))),
                TextureDecoder::Raw,
                ".{ext} should run through the RAW pipeline"
            );
        }

        for ext in ["avif", "heic", "heif", "hif"] {
            assert_eq!(
                texture_decoder_for_path(std::path::Path::new(&format!("phone.{ext}"))),
                TextureDecoder::Heif
            );
        }
        assert_eq!(
            texture_decoder_for_path(std::path::Path::new("wide.JXL")),
            TextureDecoder::JpegXl
        );
    }

    #[test]
    fn every_supported_image_extension_has_a_decoder_route() {
        for ext in crate::media_kinds::SUPPORTED.iter() {
            let Some(mime) = crate::media_kinds::mime_for(ext) else {
                continue;
            };
            if crate::media_kinds::asset_kind(mime) == crate::media_kinds::AssetKind::Image {
                let route =
                    texture_decoder_for_path(std::path::Path::new(&format!("fixture.{ext}")));
                assert!(
                    matches!(
                        route,
                        TextureDecoder::Raw
                            | TextureDecoder::Heif
                            | TextureDecoder::JpegXl
                            | TextureDecoder::Svg
                            | TextureDecoder::Jpeg
                            | TextureDecoder::Webp
                            | TextureDecoder::Jpeg2k
                            | TextureDecoder::Psd
                            | TextureDecoder::Pixbuf
                            | TextureDecoder::ImageFallback
                    ),
                    "image extension `.{ext}` has no lightbox decoder route"
                );
            }
        }
    }

    #[test]
    fn fixture_standard_formats_decode_to_textures() {
        for name in ["sample.jpg", "sample.webp"] {
            assert_fixture_texture(name, (16, 12));
        }
    }

    #[test]
    fn fixture_svg_decodes_to_texture() {
        assert_fixture_texture("sample.svg", (16, 12));
    }

    #[test]
    fn fixture_heif_family_decodes_to_textures() {
        assert_fixture_texture("sample.avif", (16, 12));
        assert_fixture_texture("sample.heic", (64, 64));
    }

    #[test]
    fn fixture_jpegxl_decodes_to_texture() {
        assert_fixture_texture("sample.jxl", (16, 12));
    }

    #[test]
    fn fixture_dng_decode_pipeline_does_not_panic() {
        // Synthetic DNG has no embedded preview; libraw may reject it.
        // Verifies the pipeline completes without panicking.
        RAW_FULL_DECODE.store(true, std::sync::atomic::Ordering::Relaxed);
        let _result = load_texture_blocking(&fixture("sample.dng"));
        RAW_FULL_DECODE.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    #[test]
    fn largest_embedded_jpeg_picks_biggest_soi_payload() {
        // Build a synthetic buffer with two JPEG SOI/EOI blocks: a small one
        // (below the 4KB minimum — should be skipped) and a large one.
        // Both use valid JPEG marker structure with declared segment lengths.

        /// Build a minimal valid JPEG with an APP marker segment of the given
        /// filler size, terminated by EOI.
        fn build_jpeg(app_marker: u8, filler_size: usize) -> Vec<u8> {
            // APP segment: FF <marker> <len_hi> <len_lo> <filler...>
            let seg_len = (filler_size + 2) as u16; // +2 for the length field itself
            let mut v = vec![0xFFu8, 0xD8, 0xFF, app_marker];
            v.extend_from_slice(&seg_len.to_be_bytes());
            v.extend_from_slice(&vec![0x42u8; filler_size]);
            v.extend_from_slice(&[0xFF, 0xD9]); // EOI
            v
        }

        let small = build_jpeg(0xE0, 30); // ~36 bytes — well below MIN_EMBEDDED_JPEG_SIZE
        let large = build_jpeg(0xE1, 8192); // ~8200 bytes — above threshold

        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00; 16]);
        buf.extend_from_slice(&small);
        buf.extend_from_slice(&[0x00; 64]);
        buf.extend_from_slice(&large);
        buf.extend_from_slice(&[0x00; 16]);

        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&buf).unwrap();
        let got = extract_largest_embedded_jpeg(tmp.path()).expect("scanner should find a JPEG");

        assert_eq!(got.len(), large.len(), "should return the larger payload");
        assert_eq!(&got[..4], &[0xFF, 0xD8, 0xFF, 0xE1]);
        assert_eq!(&got[got.len() - 2..], &[0xFF, 0xD9]);
    }

    #[test]
    fn largest_embedded_jpeg_skips_byte_stuffed_ff00() {
        // FF 00 inside entropy data must NOT be mistaken for an SOI (which is
        // FF D8 FF xx with xx != 00). A file with only an FF 00 sequence and
        // no real SOI should return None.
        let buf: Vec<u8> = vec![0x00, 0xFF, 0x00, 0xFF, 0xD8, 0xFF, 0x00, 0x42];
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&buf).unwrap();
        let got = extract_largest_embedded_jpeg(tmp.path());
        assert!(got.is_none(), "byte-stuffed FF 00 must not match as SOI");
    }

    #[test]
    fn largest_embedded_jpeg_accepts_implicit_eoi_at_eof() {
        // Some DNG/CR3/RW2 files omit the trailing FF D9 — the scanner should
        // accept the payload up to EOF when no explicit EOI is found.
        let seg_len = (8192 + 2) as u16;
        let mut jpeg = vec![0xFFu8, 0xD8, 0xFF, 0xE0];
        jpeg.extend_from_slice(&seg_len.to_be_bytes());
        jpeg.extend_from_slice(&vec![0x42u8; 8192]);
        // NO trailing FF D9 — implicit end at EOF.

        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00; 16]);
        buf.extend_from_slice(&jpeg);

        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&buf).unwrap();
        let got =
            extract_largest_embedded_jpeg(tmp.path()).expect("scanner should accept implicit EOI");

        // The payload should span from the SOI to the end of the file.
        assert_eq!(got.len(), buf.len() - 16, "should capture SOI to EOF");
        assert_eq!(&got[..4], &[0xFF, 0xD8, 0xFF, 0xE0]);
    }

    #[test]
    fn jpeg_exif_orientation_parses_tag_0112() {
        // Hand-built minimal JPEG: SOI + APP1(Exif/TIFF little-endian, IFD0
        // with a single Orientation=6 entry) + EOI. Verifies our parser walks
        // segments, finds tag 0x0112, and returns the 16-bit SHORT value.
        let mut tiff: Vec<u8> = Vec::new();
        tiff.extend_from_slice(b"II"); // little-endian
        tiff.extend_from_slice(&0x002A_u16.to_le_bytes());
        tiff.extend_from_slice(&8_u32.to_le_bytes()); // IFD0 offset
        tiff.extend_from_slice(&1_u16.to_le_bytes()); // one entry
        tiff.extend_from_slice(&0x0112_u16.to_le_bytes()); // tag = Orientation
        tiff.extend_from_slice(&3_u16.to_le_bytes()); // type = SHORT
        tiff.extend_from_slice(&1_u32.to_le_bytes()); // count
        tiff.extend_from_slice(&6_u16.to_le_bytes()); // value (low half)
        tiff.extend_from_slice(&0_u16.to_le_bytes()); // value (high half)

        let mut app1: Vec<u8> = Vec::new();
        app1.extend_from_slice(b"Exif\0\0");
        app1.extend_from_slice(&tiff);
        let app1_len = (app1.len() + 2) as u16;

        let mut jpeg: Vec<u8> = vec![0xFF, 0xD8]; // SOI
        jpeg.push(0xFF);
        jpeg.push(0xE1); // APP1
        jpeg.extend_from_slice(&app1_len.to_be_bytes());
        jpeg.extend_from_slice(&app1);
        jpeg.extend_from_slice(&[0xFF, 0xD9]); // EOI

        assert_eq!(read_jpeg_exif_orientation(&jpeg), Some(6));
    }

    #[test]
    fn jpeg_exif_orientation_none_when_missing() {
        let jpeg = vec![0xFF, 0xD8, 0xFF, 0xD9];
        assert_eq!(read_jpeg_exif_orientation(&jpeg), None);
    }

    #[test]
    fn is_lossless_jpeg_detects_sof3() {
        // SOI + SOF3 marker (FF C3) with a 10-byte segment + EOI.
        let mut lossless = vec![0xFFu8, 0xD8, 0xFF, 0xC3];
        lossless.extend_from_slice(&12_u16.to_be_bytes()); // segment length
        lossless.extend_from_slice(&[0x00; 10]); // segment body
        lossless.extend_from_slice(&[0xFF, 0xD9]); // EOI
        assert!(
            is_lossless_jpeg(&lossless, 0, lossless.len()),
            "SOF3 should be detected as lossless"
        );
    }

    #[test]
    fn is_lossless_jpeg_allows_baseline() {
        // SOI + SOF0 (baseline, FF C0) -- not lossless.
        let mut baseline = vec![0xFFu8, 0xD8, 0xFF, 0xC0];
        baseline.extend_from_slice(&12_u16.to_be_bytes());
        baseline.extend_from_slice(&[0x00; 10]);
        baseline.extend_from_slice(&[0xFF, 0xD9]);
        assert!(
            !is_lossless_jpeg(&baseline, 0, baseline.len()),
            "SOF0 should NOT be detected as lossless"
        );
    }

    #[test]
    fn is_lossless_jpeg_with_app_segments_before_sof() {
        // SOI + APP1 (FF E1) + SOF3 (FF C3) -- lossless despite APP prefix.
        let mut data = vec![0xFFu8, 0xD8];
        // APP1 segment with 20 bytes of filler
        data.extend_from_slice(&[0xFF, 0xE1]);
        data.extend_from_slice(&22_u16.to_be_bytes());
        data.extend_from_slice(&[0x42; 20]);
        // SOF3
        data.extend_from_slice(&[0xFF, 0xC3]);
        data.extend_from_slice(&12_u16.to_be_bytes());
        data.extend_from_slice(&[0x00; 10]);
        data.extend_from_slice(&[0xFF, 0xD9]);
        assert!(is_lossless_jpeg(&data, 0, data.len()));
    }

    #[test]
    fn soi_scanner_skips_lossless_jpeg() {
        // Build a file with TWO JPEGs: a large SOF3 (lossless, should be skipped)
        // and a smaller SOF0 (baseline, should be selected).

        fn build_jpeg_with_sof(sof_marker: u8, filler: usize) -> Vec<u8> {
            let mut v = vec![0xFFu8, 0xD8]; // SOI
            // SOF segment
            v.extend_from_slice(&[0xFF, sof_marker]);
            let seg_len = (filler + 2) as u16;
            v.extend_from_slice(&seg_len.to_be_bytes());
            v.extend_from_slice(&vec![0x42u8; filler]);
            v.extend_from_slice(&[0xFF, 0xD9]); // EOI
            v
        }

        let lossless = build_jpeg_with_sof(0xC3, 16000); // 16KB lossless -- larger
        let baseline = build_jpeg_with_sof(0xC0, 8000); // 8KB baseline -- smaller

        let mut buf = Vec::new();
        buf.extend_from_slice(&[0x00; 16]);
        buf.extend_from_slice(&lossless);
        buf.extend_from_slice(&[0x00; 32]);
        buf.extend_from_slice(&baseline);
        buf.extend_from_slice(&[0x00; 16]);

        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&buf).unwrap();
        let got = extract_largest_embedded_jpeg(tmp.path())
            .expect("scanner should find the baseline JPEG");

        // Should get the baseline (SOF0), not the larger lossless (SOF3).
        assert_eq!(got.len(), baseline.len());
        assert_eq!(&got[..2], &[0xFF, 0xD8]);
        assert_eq!(got[3], 0xC0, "should select the SOF0 JPEG, not SOF3");
    }

    #[test]
    fn thumbnail_prerotated_detects_portrait_on_landscape_sensor() {
        // Landscape sensor (6000x4000), flip=5 (90-degree), portrait thumb (480x640).
        assert!(is_thumbnail_prerotated(480, 640, 6000, 4000, 5));
    }

    #[test]
    fn thumbnail_prerotated_false_for_matching_aspect() {
        // Landscape sensor, flip=5, landscape thumb -- not pre-rotated.
        assert!(!is_thumbnail_prerotated(640, 480, 6000, 4000, 5));
    }

    #[test]
    fn thumbnail_prerotated_false_for_no_rotation() {
        // flip=0 (no rotation) -- never considered pre-rotated.
        assert!(!is_thumbnail_prerotated(480, 640, 6000, 4000, 0));
    }

    #[test]
    fn thumbnail_prerotated_false_for_180_rotation() {
        // flip=3 (180-degree) -- aspect doesn't change, never pre-rotated.
        assert!(!is_thumbnail_prerotated(480, 640, 6000, 4000, 3));
    }

    fn noise_jpeg(width: u32, height: u32) -> Vec<u8> {
        let pixels: Vec<u8> = (0..width * height * 3)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode(&pixels, width, height, image::ExtendedColorType::Rgb8)
            .expect("encode jpeg");
        jpeg
    }

    #[test]
    fn embedded_jpeg_is_found_when_libraw_cannot_open_the_file() {
        let jpeg = noise_jpeg(96, 64);
        assert!(jpeg.len() >= MIN_EMBEDDED_JPEG_SIZE);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unknown-camera.arw");
        let mut bytes = b"NOT A RAW HEADER".repeat(64);
        bytes.extend_from_slice(&jpeg);
        bytes.extend_from_slice(&[0u8; 512]);
        std::fs::write(&path, bytes).unwrap();
        let texture = extract_libraw_thumb(&path).expect("embedded preview");
        assert_eq!((texture.width(), texture.height()), (96, 64));
    }

    #[test]
    fn bitmap_thumb_passes_8_bit_samples_through() {
        let bytes = [1u8, 2, 3, 4, 5, 6, 7];
        assert_eq!(
            bitmap_thumb_rgb8(&bytes, 8, 6),
            Some(vec![1, 2, 3, 4, 5, 6])
        );
    }

    #[test]
    fn bitmap_thumb_keeps_high_byte_of_16_bit_samples() {
        let bytes: Vec<u8> = [0x1234u16, 0xff00, 0x00ff]
            .iter()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        assert_eq!(
            bitmap_thumb_rgb8(&bytes, 16, 3),
            Some(vec![0x12, 0xff, 0x00])
        );
    }

    #[test]
    fn bitmap_thumb_rejects_short_buffers_and_odd_depths() {
        assert_eq!(bitmap_thumb_rgb8(&[0u8; 5], 8, 6), None);
        assert_eq!(bitmap_thumb_rgb8(&[0u8; 11], 16, 6), None);
        assert_eq!(bitmap_thumb_rgb8(&[0u8; 12], 12, 6), None);
    }

    #[test]
    fn raw_cache_png_is_written_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("entry.png");
        let texture = memory_texture(2, 2, gdk4::MemoryFormat::R8g8b8, vec![200; 12], 6).unwrap();
        save_png_atomically(&texture, &dest).unwrap();
        let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(entries.len(), 1, "temp file left behind");
        let reloaded = gdk4::Texture::from_filename(&dest).unwrap();
        assert_eq!((reloaded.width(), reloaded.height()), (2, 2));
    }

    #[test]
    fn raw_cache_key_tracks_file_contents_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.nef");
        std::fs::write(&path, b"first").unwrap();
        let first = raw_decode_cache_key(&path, None).unwrap();
        assert_eq!(raw_decode_cache_key(&path, None).unwrap(), first);
        std::fs::write(&path, b"second, longer").unwrap();
        assert_ne!(raw_decode_cache_key(&path, None).unwrap(), first);
    }

    #[test]
    fn raw_cache_key_tracks_the_look() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.nef");
        std::fs::write(&path, b"raw").unwrap();
        let look = |rgb: [u8; 3]| {
            let bytes = rgb.repeat(64 * 64);
            raw_tone::Reference::sample(&raw_tone::Pixels {
                bytes: &bytes,
                width: 64,
                height: 64,
                stride: 64 * 3,
                bytes_per_pixel: 3,
                encoding: raw_tone::Encoding::DisplayP3,
            })
            .unwrap()
        };
        let own = raw_decode_cache_key(&path, None).unwrap();
        let warm = raw_decode_cache_key(&path, Some(&look([200, 120, 60]))).unwrap();
        let cool = raw_decode_cache_key(&path, Some(&look([60, 120, 200]))).unwrap();
        assert!(own != warm && warm != cool && own != cool);
    }
}
