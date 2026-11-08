//! The thumbnail phones store inside HEIC and AVIF files, so a grid tile doesn't need the
//! whole photo decoded (a 48 MP HEIC is about 190 MB of pixels).

use libheif_rs::{ColorSpace, HeifContext, ImageHandle, LibHeif, RgbChroma};

/// iPhones and most Android phones store one; four is plenty to look through.
const MAX_THUMBNAILS: usize = 4;

/// HEIC, HEIF, and AVIF, which can carry an embedded thumbnail.
pub(in crate::library) fn is_heif_path(path: &std::path::Path) -> bool {
    matches!(
        super::texture_decoder_for_path(path),
        super::TextureDecoder::Heif
    )
}

/// The smallest embedded thumbnail with an edge of at least `min_edge`, decoded.
pub(in crate::library) fn decode_heif_thumbnail(
    path: &std::path::Path,
    min_edge: u32,
) -> Option<gdk4::Texture> {
    let encoded = std::fs::read(path).ok()?;
    let context = HeifContext::read_from_bytes(&encoded).ok()?;
    let primary = context.primary_image_handle().ok()?;
    let handle = smallest_big_enough(&primary, min_edge)?;
    let image = LibHeif::new()
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), None)
        .ok()?;
    let plane = image.planes().interleaved?;
    super::memory_texture(
        plane.width,
        plane.height,
        gdk4::MemoryFormat::R8g8b8a8,
        plane.data.to_vec(),
        plane.stride,
    )
}

fn smallest_big_enough(primary: &ImageHandle, min_edge: u32) -> Option<ImageHandle> {
    let mut ids = [0; MAX_THUMBNAILS];
    let count = primary.thumbnail_ids(&mut ids);
    ids[..count.min(MAX_THUMBNAILS)]
        .iter()
        .filter_map(|id| primary.thumbnail(*id).ok())
        .filter(|thumb| big_enough((thumb.width(), thumb.height()), min_edge))
        .min_by_key(|thumb| u64::from(thumb.width()) * u64::from(thumb.height()))
}

fn big_enough((width, height): (u32, u32), min_edge: u32) -> bool {
    width.max(height) >= min_edge
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_must_cover_a_tile() {
        // An iPhone's embedded thumbnail is 320x240.
        assert!(big_enough((320, 240), 256));
        assert!(!big_enough((160, 120), 256));
    }
}
