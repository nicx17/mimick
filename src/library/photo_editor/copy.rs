//! Saving edits Immich can't store: the original, decoded at full size from whatever format
//! it's in, developed like the preview and encoded as a new photo, keeping its metadata
//! unless the copy is to be a new photo of its own.

use std::path::{Path, PathBuf};

use image::ImageEncoder;

use super::carried::{self, Carried};
use super::copy_exif::{CopyDetails, exif_for_copy};
use super::embed::{JPEG_EXIF_ROOM, Metadata, jpeg_with, png_with_xmp};
use super::pixels::{Rgba, Steps, develop};
use crate::api_client::{AssetUpdate, DateChange, ImmichApiClient};

/// JPEG quality for copies: well past where artefacts show, without bloating the file.
const JPEG_QUALITY: i32 = 92;
/// PNG chunks can hold far more than a JPEG segment; this only stops a runaway block.
const PNG_EXIF_ROOM: usize = 16 << 20;

/// What a copy is made from, owned so it can be made on a worker thread.
pub(in crate::library) struct CopyJob {
    pub(super) steps: Steps,
    /// The preview's size, which `steps` are set out in.
    pub(super) preview: (f64, f64),
    pub(super) details: CopyDetails,
    /// The copy keeps the original's metadata; otherwise it's a new photo with none.
    pub(super) keep_details: bool,
    /// The original's file, which its metadata is read from.
    pub(super) original: Option<PathBuf>,
    /// The colour profile of the pixels `run` is given, when they were read out in a colour
    /// space other than the original's; it replaces the original's in the copy.
    pub(super) pixels_profile: Option<Vec<u8>>,
}

/// A copy ready to upload.
#[derive(Debug)]
pub(in crate::library) struct EncodedCopy {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
    pub mime: &'static str,
}

impl CopyJob {
    pub(in crate::library) fn with_original(mut self, path: &Path) -> Self {
        self.original = Some(path.to_path_buf());
        self
    }

    pub(in crate::library) fn with_pixels_profile(mut self, icc: Option<Vec<u8>>) -> Self {
        self.pixels_profile = icc;
        self
    }

    pub(in crate::library) fn keeps_details(&self) -> bool {
        self.keep_details
    }

    pub(in crate::library) fn details(&self) -> &CopyDetails {
        &self.details
    }

    /// Develops the original's RGBA pixels, `stride` bytes a row, and encodes the result.
    pub(in crate::library) fn run(
        self,
        pixels: &[u8],
        stride: usize,
        size: (usize, usize),
    ) -> Result<EncodedCopy, String> {
        let full = (size.0 as f64, size.1 as f64);
        let turned = (full.0 > full.1) != (self.preview.0 > self.preview.1);
        if turned && full.0 != full.1 && self.preview.0 != self.preview.1 {
            return Err("The original is turned differently from its preview".into());
        }
        let source = Rgba::from_strided(pixels, stride, size);
        let img = develop(source, &self.steps.clone().scaled(self.preview, full));
        let mut carried = self
            .original
            .as_deref()
            .map(carried::read)
            .unwrap_or_default();
        if let Some(icc) = &self.pixels_profile {
            carried.icc = Some(icc.clone());
        }
        encode(&img, self.metadata_source(carried))
    }

    /// Everything the original carries, or for a new photo only the colour profile, which
    /// says what its pixel values mean.
    fn metadata_source(&self, carried: Carried) -> Source<'_> {
        if self.keep_details {
            Source {
                carried,
                details: Some(&self.details),
            }
        } else {
            Source {
                carried: Carried {
                    icc: carried.icc,
                    ..Carried::default()
                },
                details: None,
            }
        }
    }
}

/// Where the copy's metadata comes from; no `details` for a new photo.
struct Source<'a> {
    carried: Carried,
    details: Option<&'a CopyDetails>,
}

impl Source<'_> {
    fn metadata(self, (w, h): (usize, usize), exif_room: usize) -> Metadata {
        let size = (w as u32, h as u32);
        let exif = self
            .details
            .and_then(|details| exif_for_copy(self.carried.exif, details, size, exif_room));
        Metadata {
            exif,
            xmp: self
                .carried
                .xmp
                .filter(|_| self.details.is_some())
                .map(|xmp| carried::upright_xmp(&xmp, size)),
            icc: self.carried.icc,
        }
    }
}

/// JPEG, unless some of the picture is see-through: then PNG, which keeps that.
fn encode(img: &Rgba, source: Source<'_>) -> Result<EncodedCopy, String> {
    if img.data.as_chunks::<4>().0.iter().any(|px| px[3] < u8::MAX) {
        encode_png(img, source.metadata((img.w, img.h), PNG_EXIF_ROOM))
    } else {
        encode_jpeg(img, &source.metadata((img.w, img.h), JPEG_EXIF_ROOM))
    }
}

fn encode_jpeg(img: &Rgba, meta: &Metadata) -> Result<EncodedCopy, String> {
    let image = turbojpeg::Image {
        pixels: img.data.as_slice(),
        width: img.w,
        pitch: img.w * 4,
        height: img.h,
        format: turbojpeg::PixelFormat::RGBA,
    };
    let jpeg = turbojpeg::compress(image, JPEG_QUALITY, turbojpeg::Subsamp::Sub2x2)
        .map_err(|err| err.to_string())?;
    Ok(EncodedCopy {
        bytes: jpeg_with(&jpeg, meta),
        extension: "jpg",
        mime: "image/jpeg",
    })
}

fn encode_png(img: &Rgba, meta: Metadata) -> Result<EncodedCopy, String> {
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    if let Some(exif) = meta.exif {
        encoder
            .set_exif_metadata(exif)
            .map_err(|err| err.to_string())?;
    }
    if let Some(icc) = meta.icc {
        encoder
            .set_icc_profile(icc)
            .map_err(|err| err.to_string())?;
    }
    encoder
        .write_image(
            &img.data,
            img.w as u32,
            img.h as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|err| err.to_string())?;
    if let Some(xmp) = meta.xmp {
        bytes = png_with_xmp(&bytes, &xmp);
    }
    Ok(EncodedCopy {
        bytes,
        extension: "png",
        mime: "image/png",
    })
}

/// Sets the copy's capture time and zone, place, caption, and rating in Immich to the
/// original's; Immich then keeps them over what it reads from the file.
pub(in crate::library) async fn match_record(
    api: &ImmichApiClient,
    copy: &str,
    details: &CopyDetails,
) -> Result<(), String> {
    let ids = [copy.to_string()];
    if let (Some(date_time), Some(time_zone)) = (details.taken_rfc3339(), details.time_zone.clone())
    {
        let change = DateChange::Set {
            date_time,
            time_zone,
        };
        api.set_assets_date(&ids, &change).await?;
    }
    if let Some((lat, lon)) = details.gps {
        api.set_assets_location(&ids, lat, lon).await?;
    }
    let caption = AssetUpdate {
        description: Some(details.description.clone().unwrap_or_default()),
        ..AssetUpdate::default()
    };
    api.update_asset(copy, &caption).await?;
    if details.rating.is_some() {
        api.set_asset_rating(copy, details.rating).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(keep_details: bool) -> CopyJob {
        CopyJob {
            steps: Steps::default(),
            preview: (4.0, 2.0),
            details: CopyDetails {
                make: Some("Mimick".into()),
                ..CopyDetails::default()
            },
            keep_details,
            original: None,
            pixels_profile: None,
        }
    }

    #[test]
    fn opaque_photos_become_jpeg_and_see_through_ones_png() {
        let opaque = [10u8, 20, 30, 255].repeat(8 * 4);
        let copy = job(true).run(&opaque, 8 * 4, (8, 4)).unwrap();
        assert_eq!(copy.extension, "jpg");
        assert!(copy.bytes.windows(4).any(|w| w == b"Exif"));
        let clear = [10u8, 20, 30, 0].repeat(8 * 4);
        let copy = job(true).run(&clear, 8 * 4, (8, 4)).unwrap();
        assert_eq!(copy.extension, "png");
        assert!(copy.bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn new_photos_carry_no_details() {
        let opaque = [10u8, 20, 30, 255].repeat(8 * 4);
        let copy = job(false).run(&opaque, 8 * 4, (8, 4)).unwrap();
        assert!(!copy.bytes.windows(4).any(|w| w == b"Exif"));
    }

    #[test]
    fn originals_turned_unlike_their_preview_are_refused() {
        let tall = [0u8, 0, 0, 255].repeat(4 * 8);
        assert!(job(true).run(&tall, 4 * 4, (4, 8)).is_err());
    }
}
