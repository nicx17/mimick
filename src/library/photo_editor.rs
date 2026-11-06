//! Crop, rotate, and mirror a photo in Immich without touching the original file: the edit
//! session the lightbox drives, kept apart from its widgets.

mod orientation;

use gettextrs::gettext;

use gtk::prelude::*;

use super::photo_canvas::{Region, keep_aspect};
use crate::api_client::{AssetEdit, LibraryAsset};
use crate::app_context::AppContext;
use orientation::{Orientation, orient_pixels};

/// Crop shapes: label and width over height (landscape); `None` is free, 0 is the photo's own.
pub(super) const ASPECTS: [(&str, Option<f64>); 6] = [
    (crate::i18n::gettext_noop("Free"), None),
    (crate::i18n::gettext_noop("Original"), Some(0.0)),
    (crate::i18n::gettext_noop("Square"), Some(1.0)),
    ("4:3", Some(4.0 / 3.0)),
    ("3:2", Some(3.0 / 2.0)),
    ("16:9", Some(16.0 / 9.0)),
];

/// The unedited preview's pixels, which every orientation is drawn from.
struct Preview {
    bytes: glib::Bytes,
    stride: usize,
    size: (usize, usize),
}

/// One photo being edited: its unedited preview, and the crop, turns, and flips so far.
pub(super) struct EditSession {
    pub asset_id: String,
    preview: Preview,
    full: (f64, f64),
    orientation: Orientation,
    /// In the preview's original-orientation pixels; `None` keeps the whole photo.
    crop: Option<Region>,
    was_edited: bool,
    /// The thumbhash before saving; Immich changes it once the saved edit is rendered.
    pub thumbhash: Option<String>,
}

impl EditSession {
    /// The unedited preview, the photo's full size, and any edits Immich already has.
    pub(super) async fn load(
        ctx: &AppContext,
        asset_id: String,
        was_edited: bool,
    ) -> Result<Self, String> {
        let texture = ctx.thumbnail_cache.load_preview(&asset_id, false).await?;
        let asset = ctx.api_client.fetch_asset_by_id(&asset_id).await.ok();
        let edits = ctx
            .api_client
            .fetch_edits(&asset_id)
            .await
            .unwrap_or_default();
        let mut downloader = gtk::gdk::TextureDownloader::new(&texture);
        downloader.set_format(gtk::gdk::MemoryFormat::R8g8b8a8);
        let (bytes, stride) = downloader.download_bytes();
        let preview = Preview {
            bytes,
            stride,
            size: (texture.width() as usize, texture.height() as usize),
        };
        let size = preview_size(&preview);
        let full = full_size(asset.as_ref(), size);
        Ok(Self {
            thumbhash: asset.as_ref().and_then(|a| a.thumbhash.clone()),
            asset_id,
            crop: saved_crop(&edits, size, full),
            orientation: Orientation::from_edits(&edits),
            preview,
            full,
            was_edited,
        })
    }

    /// The preview turned and flipped as it stands, drawn on a worker thread. The future owns
    /// what it needs, so no borrow of the session is held while it runs.
    pub(super) fn oriented_texture(
        &self,
    ) -> impl std::future::Future<Output = Option<gtk::gdk::Texture>> + 'static {
        let (bytes, stride, size) = (
            self.preview.bytes.clone(),
            self.preview.stride,
            self.preview.size,
        );
        let orientation = self.orientation;
        async move {
            let (pixels, w, h) = tokio::task::spawn_blocking(move || {
                orient_pixels(&bytes, stride, size, orientation)
            })
            .await
            .ok()?;
            let texture = gtk::gdk::MemoryTexture::new(
                w as i32,
                h as i32,
                gtk::gdk::MemoryFormat::R8g8b8a8,
                &glib::Bytes::from_owned(pixels),
                w * 4,
            );
            Some(texture.upcast())
        }
    }

    /// The crop box as it lands on the turned preview.
    pub(super) fn screen_crop(&self) -> Region {
        let size = preview_size(&self.preview);
        let crop = self.crop.unwrap_or(whole(size));
        self.orientation.to_screen(crop, size)
    }

    /// Takes the crop box drawn on the turned preview.
    pub(super) fn set_screen_crop(&mut self, screen: Region) {
        let size = preview_size(&self.preview);
        let crop = self.orientation.to_original(screen, size);
        self.crop = (!covers(crop, size)).then_some(crop);
    }

    pub(super) fn rotate(&mut self, clockwise: bool) {
        self.orientation.rotate(clockwise);
    }

    /// Flips what's on screen, left to right or top to bottom.
    pub(super) fn flip(&mut self, horizontal: bool) {
        self.orientation.flip(horizontal);
    }

    pub(super) fn reset(&mut self) {
        self.orientation = Orientation::default();
        self.crop = None;
    }

    pub(super) fn edits(&self) -> Vec<AssetEdit> {
        build_edits(
            self.crop,
            preview_size(&self.preview),
            self.full,
            self.orientation,
        )
    }

    /// Saving does something: there are edits, or old ones to remove.
    pub(super) fn has_changes(&self) -> bool {
        !self.edits().is_empty() || self.was_edited
    }

    pub(super) fn was_edited(&self) -> bool {
        self.was_edited
    }
}

/// The crop box shaped to `preset` (see `ASPECTS`), turned to match the box's orientation.
pub(super) fn shaped_crop(preset: f64, selection: Region, screen: (f64, f64)) -> (f64, Region) {
    let ratio = if preset == 0.0 {
        screen.0 / screen.1.max(1.0)
    } else {
        preset
    };
    let ratio = if (selection.h > selection.w) == (ratio > 1.0) {
        1.0 / ratio
    } else {
        ratio
    };
    (ratio, keep_aspect(selection, ratio, screen))
}

pub(super) fn aspect_label(index: usize) -> String {
    ASPECTS
        .get(index)
        .map_or_else(String::new, |(label, _)| gettext(*label))
}

fn preview_size(preview: &Preview) -> (f64, f64) {
    (preview.size.0 as f64, preview.size.1 as f64)
}

fn whole((w, h): (f64, f64)) -> Region {
    Region {
        x: 0.0,
        y: 0.0,
        w,
        h,
    }
}

/// Within a pixel of the whole photo counts as no crop.
fn covers(r: Region, (w, h): (f64, f64)) -> bool {
    r.x <= 1.0 && r.y <= 1.0 && r.x + r.w >= w - 1.0 && r.y + r.h >= h - 1.0
}

/// The original's pixel size, from EXIF or the asset, turned to match the preview.
fn full_size(asset: Option<&LibraryAsset>, preview: (f64, f64)) -> (f64, f64) {
    let exif = asset.and_then(|a| a.exif_info.as_ref());
    let dims = exif
        .and_then(|e| e.exif_image_width.zip(e.exif_image_height))
        .or_else(|| asset.and_then(|a| a.width.zip(a.height)))
        .filter(|(w, h)| *w > 0 && *h > 0)
        .map(|(w, h)| (f64::from(w), f64::from(h)));
    let Some((w, h)) = dims else {
        return preview;
    };
    if (w > h) != (preview.0 > preview.1) && w != h {
        (h, w)
    } else {
        (w, h)
    }
}

/// The crop Immich has saved, scaled from full-size to preview pixels.
fn saved_crop(edits: &[AssetEdit], preview: (f64, f64), full: (f64, f64)) -> Option<Region> {
    edits.iter().find_map(|edit| match *edit {
        AssetEdit::Crop {
            x,
            y,
            width,
            height,
        } => {
            let (sx, sy) = (preview.0 / full.0, preview.1 / full.1);
            Some(Region {
                x: f64::from(x) * sx,
                y: f64::from(y) * sy,
                w: f64::from(width) * sx,
                h: f64::from(height) * sy,
            })
        }
        _ => None,
    })
}

/// Edits for Immich: the crop in full-size pixels first, then the mirrors and the turn.
fn build_edits(
    crop: Option<Region>,
    preview: (f64, f64),
    full: (f64, f64),
    orientation: Orientation,
) -> Vec<AssetEdit> {
    let mut edits = Vec::new();
    if let Some(crop) = crop {
        let (sx, sy) = (full.0 / preview.0, full.1 / preview.1);
        let x = (crop.x * sx).round().clamp(0.0, full.0 - 1.0);
        let y = (crop.y * sy).round().clamp(0.0, full.1 - 1.0);
        let width = (crop.w * sx).round().clamp(1.0, full.0 - x);
        let height = (crop.h * sy).round().clamp(1.0, full.1 - y);
        edits.push(AssetEdit::Crop {
            x: x as u32,
            y: y as u32,
            width: width as u32,
            height: height as u32,
        });
    }
    edits.extend(orientation.edits());
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_scale_to_full_size_and_stay_inside() {
        let crop = Region {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 300.0,
        };
        let edits = build_edits(
            Some(crop),
            (200.0, 300.0),
            (2000.0, 3000.0),
            Orientation::default(),
        );
        assert_eq!(
            edits,
            [AssetEdit::Crop {
                x: 100,
                y: 200,
                width: 1000,
                height: 2800
            }]
        );
    }

    #[test]
    fn saved_crops_come_back_in_preview_pixels() {
        let saved = [AssetEdit::Crop {
            x: 100,
            y: 200,
            width: 1000,
            height: 2800,
        }];
        let crop = saved_crop(&saved, (200.0, 300.0), (2000.0, 3000.0)).unwrap();
        assert_eq!((crop.x, crop.y, crop.w, crop.h), (10.0, 20.0, 100.0, 280.0));
    }

    #[test]
    fn full_size_follows_the_previews_orientation() {
        let asset: LibraryAsset = serde_json::from_value(serde_json::json!({
            "id": "a",
            "originalFileName": "a.jpg",
            "originalMimeType": "image/jpeg",
            "fileCreatedAt": "2026-03-15T12:00:00.000Z",
            "type": "IMAGE",
            "exifInfo": { "exifImageWidth": 4000, "exifImageHeight": 3000 }
        }))
        .unwrap();
        assert_eq!(full_size(Some(&asset), (1080.0, 1440.0)), (3000.0, 4000.0));
        assert_eq!(full_size(Some(&asset), (1440.0, 1080.0)), (4000.0, 3000.0));
        assert_eq!(full_size(None, (10.0, 20.0)), (10.0, 20.0));
    }

    #[test]
    fn a_crop_of_the_whole_photo_is_no_crop() {
        assert!(covers(whole((100.0, 50.0)), (100.0, 50.0)));
        assert!(!covers(
            Region {
                x: 5.0,
                y: 0.0,
                w: 95.0,
                h: 50.0
            },
            (100.0, 50.0)
        ));
    }
}
