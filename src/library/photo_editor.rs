//! Editing a photo without touching the original file: crops, turns, and flips Immich stores,
//! and levelling, the look, and redaction saved as a copy. The session the lightbox drives,
//! kept apart from its widgets.

mod carried;
mod copy;
mod copy_exif;
mod embed;
mod exif_tree;
pub(super) mod look;
mod orientation;
mod pixels;
pub(super) mod redaction;

use gettextrs::gettext;

use gtk::prelude::*;

use super::photo_canvas::{Region, keep_aspect};
use crate::api_client::{AssetEdit, LibraryAsset};
use crate::app_context::AppContext;
pub(super) use copy::{CopyJob, match_record};
pub(super) use copy_exif::CopyDetails;
use look::{FILTERS, Look};
use orientation::Orientation;
use pixels::{Rgba, Steps, develop, straighten_region};
use redaction::{RedactStyle, Redaction};

/// The furthest Straighten turns either way, in degrees.
pub(super) const MAX_STRAIGHTEN: f64 = 45.0;
/// Edge of the filter tiles' pictures, in pixels.
const FILTER_PREVIEW_EDGE: usize = 96;

/// Crop shapes: label and width over height (landscape); `None` is free, 0 is the photo's own.
/// Narrowest first: prints, cameras, screens, then wide and panoramic; each turns with the box.
pub(super) const ASPECTS: [(&str, Option<f64>); 12] = [
    (crate::i18n::gettext_noop("Free"), None),
    (crate::i18n::gettext_noop("Original"), Some(0.0)),
    (crate::i18n::gettext_noop("Square"), Some(1.0)),
    ("5:4", Some(5.0 / 4.0)),
    ("4:3", Some(4.0 / 3.0)),
    ("7:5", Some(7.0 / 5.0)),
    ("3:2", Some(3.0 / 2.0)),
    ("16:10", Some(16.0 / 10.0)),
    ("16:9", Some(16.0 / 9.0)),
    ("2:1", Some(2.0)),
    ("21:9", Some(21.0 / 9.0)),
    ("3:1", Some(3.0)),
];

/// The unedited preview's pixels, which every orientation is drawn from.
#[derive(Clone)]
struct Preview {
    bytes: glib::Bytes,
    stride: usize,
    size: (usize, usize),
}

/// One photo being edited: its unedited preview, and every edit so far.
pub(super) struct EditSession {
    pub asset_id: String,
    /// The original as Immich has it, whose details a saved copy keeps.
    pub asset: Option<LibraryAsset>,
    preview: Preview,
    full: (f64, f64),
    orientation: Orientation,
    /// In the levelled preview's original-orientation pixels; `None` keeps the whole photo.
    crop: Option<Region>,
    /// Clockwise degrees on screen that level the photo.
    straighten: f64,
    pub look: Look,
    /// Areas to hide, in the levelled preview's original-orientation pixels.
    redactions: Vec<Redaction>,
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
            asset,
            asset_id,
            crop: saved_crop(&edits, size, full),
            orientation: Orientation::from_edits(&edits),
            straighten: 0.0,
            look: Look::default(),
            redactions: Vec::new(),
            preview,
            full,
            was_edited,
        })
    }

    /// The preview with every edit, drawn on a worker thread; `cropped` false shows the whole
    /// frame for the crop box. The future owns what it needs, so it holds no borrow.
    pub(super) fn developed_texture(
        &self,
        cropped: bool,
    ) -> impl std::future::Future<Output = Option<gtk::gdk::Texture>> + 'static {
        let (preview, steps) = (self.preview.clone(), self.steps(cropped));
        async move {
            let img = tokio::task::spawn_blocking(move || develop(preview.rgba(), &steps))
                .await
                .ok()?;
            Some(texture_of(img))
        }
    }

    /// One small picture per filter, in `FILTERS` order, with the sliders as they are.
    pub(super) fn filter_previews(
        &self,
    ) -> impl std::future::Future<Output = Vec<gtk::gdk::Texture>> + 'static {
        let (preview, mut steps, look) = (self.preview.clone(), self.steps(true), self.look);
        steps.recipe = None;
        async move {
            let tiles = tokio::task::spawn_blocking(move || {
                let base = pixels::shrink(&develop(preview.rgba(), &steps), FILTER_PREVIEW_EDGE);
                FILTERS
                    .iter()
                    .map(|(filter, _, _)| {
                        let mut tile = base.clone();
                        let look = Look {
                            filter: *filter,
                            strength: 1.0,
                            ..look
                        };
                        pixels::apply(&mut tile, &look.recipe());
                        tile
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();
            tiles.into_iter().map(texture_of).collect()
        }
    }

    fn steps(&self, cropped: bool) -> Steps {
        Steps {
            redactions: self.redactions.clone(),
            degrees: self.original_degrees(),
            crop: self.crop.filter(|_| cropped),
            orientation: self.orientation,
            recipe: Some(self.look.recipe()),
        }
    }

    /// What saving a copy needs, set out on the preview; `keep_details` keeps the original's
    /// metadata, else the copy is a new photo.
    pub(super) fn copy_job(&self, keep_details: bool) -> CopyJob {
        CopyJob {
            steps: self.steps(true),
            preview: preview_size(&self.preview),
            details: self
                .asset
                .as_ref()
                .map(CopyDetails::from_asset)
                .unwrap_or_default(),
            keep_details,
            original: None,
        }
    }

    pub(super) fn preview_size(&self) -> (f64, f64) {
        preview_size(&self.preview)
    }

    pub(super) fn straighten(&self) -> f64 {
        self.straighten
    }

    pub(super) fn set_straighten(&mut self, degrees: f64) {
        self.straighten = degrees.clamp(-MAX_STRAIGHTEN, MAX_STRAIGHTEN);
    }

    /// Levelling in the original's own pixels: on a mirrored photo it turns the other way.
    fn original_degrees(&self) -> f64 {
        if self.orientation.mirror_h == self.orientation.mirror_v {
            self.straighten
        } else {
            -self.straighten
        }
    }

    /// An area drawn on the developed, cropped preview, in the levelled preview's pixels.
    fn levelled(&self, shown: Region) -> Region {
        let crop = self.screen_crop();
        let framed = Region {
            x: shown.x + crop.x,
            y: shown.y + crop.y,
            ..shown
        };
        self.orientation
            .to_original(framed, preview_size(&self.preview))
    }

    /// Hides an area drawn on the developed, cropped preview; returns its index.
    pub(super) fn add_redaction(&mut self, shown: Region, style: RedactStyle) -> usize {
        let area = self.levelled(shown);
        self.redactions.push(Redaction { area, style });
        self.redactions.len() - 1
    }

    /// Moves or resizes area `index` to `shown`, on the developed, cropped preview.
    pub(super) fn move_redaction(&mut self, index: usize, shown: Region) {
        let area = self.levelled(shown);
        if let Some(redaction) = self.redactions.get_mut(index) {
            redaction.area = area;
        }
    }

    pub(super) fn redaction_style(&self, index: usize) -> Option<RedactStyle> {
        self.redactions.get(index).map(|r| r.style)
    }

    pub(super) fn set_redaction_style(&mut self, index: usize, style: RedactStyle) {
        if let Some(redaction) = self.redactions.get_mut(index) {
            redaction.style = style;
        }
    }

    /// Hides areas given in the unedited preview's pixels, as found by face or text
    /// recognition; areas already hidden aren't added twice.
    pub(super) fn add_original_redactions(&mut self, areas: &[Region], style: RedactStyle) {
        let size = preview_size(&self.preview);
        for area in areas {
            let area = straighten_region(*area, self.original_degrees(), size);
            if !self.redactions.iter().any(|r| r.area == area) {
                self.redactions.push(Redaction { area, style });
            }
        }
    }

    /// The hidden areas as they land on the developed, cropped preview.
    pub(super) fn shown_redactions(&self) -> Vec<Region> {
        let size = preview_size(&self.preview);
        let crop = self.screen_crop();
        self.redactions
            .iter()
            .map(|r| {
                let screen = self.orientation.to_screen(r.area, size);
                Region {
                    x: screen.x - crop.x,
                    y: screen.y - crop.y,
                    ..screen
                }
            })
            .collect()
    }

    pub(super) fn remove_redaction(&mut self, index: usize) {
        if index < self.redactions.len() {
            self.redactions.remove(index);
        }
    }

    pub(super) fn clear_redactions(&mut self) {
        self.redactions.clear();
    }

    pub(super) fn has_redactions(&self) -> bool {
        !self.redactions.is_empty()
    }

    /// Saving makes a new photo: some edit is one Immich can't store.
    pub(super) fn needs_copy(&self) -> bool {
        self.straighten.abs() >= 0.05 || !self.look.is_neutral() || !self.redactions.is_empty()
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
        self.straighten = 0.0;
        self.look = Look::default();
        self.redactions.clear();
    }

    pub(super) fn edits(&self) -> Vec<AssetEdit> {
        build_edits(
            self.crop,
            preview_size(&self.preview),
            self.full,
            self.orientation,
        )
    }

    /// Saving does something: there are edits, old ones to remove, or a copy to make.
    pub(super) fn has_changes(&self) -> bool {
        !self.edits().is_empty() || self.was_edited || self.needs_copy()
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

impl Preview {
    fn rgba(&self) -> Rgba {
        Rgba::from_strided(&self.bytes, self.stride, self.size)
    }
}

fn texture_of(img: Rgba) -> gtk::gdk::Texture {
    gtk::gdk::MemoryTexture::new(
        img.w as i32,
        img.h as i32,
        gtk::gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(img.data),
        img.w * 4,
    )
    .upcast()
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
