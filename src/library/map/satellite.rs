//! VersaTiles satellite imagery, enlarging the nearest coarser tile where sharp imagery runs out.

use crate::i18n::fill;
use gettextrs::gettext;
use std::collections::HashSet;
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::Instant;

use gtk::gio;
use gtk::prelude::*;
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use shumate::subclass::prelude::*;

use crate::api_client::{ImmichApiClient, TileFetch};

const TILE_URL: &str = "https://tiles.versatiles.org/tiles/satellite";
const CREDIT: &str = "<a href=\"https://versatiles.org/sources/\">VersaTiles sources</a>";
const SOURCE_ID: &str = "versatiles-satellite";
const TILE_SIZE: u32 = 512;
const MAX_ZOOM: u32 = 19;
const MAX_DEPTH: u32 = TILE_SIZE.ilog2();
const CACHE_BYTES: u32 = 100_000_000;
const CACHE_MAX_AGE_DAYS: i64 = 7;

pub fn source(api: &Arc<ImmichApiClient>) -> shumate::MapSource {
    shumate::RasterRenderer::builder()
        .data_source(&SatelliteData::new(api))
        .id(SOURCE_ID)
        .name(gettext("Satellite"))
        .license(CREDIT)
        .max_zoom_level(MAX_ZOOM)
        .tile_size(TILE_SIZE)
        .build()
        .upcast()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TileId {
    zoom: u32,
    x: u32,
    y: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Crop {
    left: u32,
    top: u32,
    side: u32,
}

impl TileId {
    fn new(x: i32, y: i32, zoom: i32) -> Option<Self> {
        Some(Self {
            zoom: u32::try_from(zoom).ok()?,
            x: u32::try_from(x).ok()?,
            y: u32::try_from(y).ok()?,
        })
    }

    fn signed(self) -> (i32, i32, i32) {
        (self.x as i32, self.y as i32, self.zoom as i32)
    }

    fn ancestor(self, depth: u32) -> Option<Self> {
        Some(Self {
            zoom: self.zoom.checked_sub(depth)?,
            x: self.x >> depth,
            y: self.y >> depth,
        })
    }

    /// The square of `ancestor(depth)`, an image `size` pixels wide, that covers this tile.
    fn crop(self, depth: u32, size: u32) -> Option<Crop> {
        let side = size.checked_shr(depth).filter(|&side| side > 0)?;
        let mask = (1 << depth) - 1;
        Some(Crop {
            left: (self.x & mask) * side,
            top: (self.y & mask) * side,
            side,
        })
    }

    fn url(self) -> String {
        format!("{TILE_URL}/{}/{}/{}", self.zoom, self.x, self.y)
    }
}

/// Ancestor depths worth asking for, nearest first, skipping any above a known-missing one.
fn depths_to_try(tile: TileId, missing: &HashSet<TileId>) -> RangeInclusive<u32> {
    let deepest = MAX_DEPTH.min(tile.zoom);
    let known_missing = (0..=deepest).rev().find(|&depth| {
        tile.ancestor(depth)
            .is_some_and(|ancestor| missing.contains(&ancestor))
    });
    let start = known_missing
        .map_or(0, |depth| depth + 1)
        .max(tile.zoom.saturating_sub(MAX_ZOOM));
    start..=deepest
}

fn is_expired(modified: Option<&glib::DateTime>, now: &glib::DateTime) -> bool {
    let max_age = glib::TimeSpan::from_days(CACHE_MAX_AGE_DAYS);
    modified.is_none_or(|modified| now.difference(modified) > max_age)
}

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::collections::HashSet;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::time::Instant;

    use gtk::gio;
    use shumate::prelude::*;
    use shumate::subclass::prelude::*;

    use super::TileId;
    use crate::api_client::ImmichApiClient;

    #[derive(Default)]
    pub struct SatelliteData {
        pub(super) api: OnceCell<Arc<ImmichApiClient>>,
        pub(super) cache: OnceCell<shumate::FileCache>,
        pub(super) missing: RefCell<HashSet<TileId>>,
        pub(super) paused_until: Cell<Option<Instant>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SatelliteData {
        const NAME: &str = "MimickSatelliteData";
        type Type = super::SatelliteData;
        type ParentType = shumate::DataSource;
    }

    impl ObjectImpl for SatelliteData {}

    impl DataSourceImpl for SatelliteData {
        fn tile_data_future(
            &self,
            x: i32,
            y: i32,
            zoom_level: i32,
        ) -> Pin<Box<dyn Future<Output = Result<glib::Bytes, glib::Error>> + 'static>> {
            let source = self.obj().clone();
            Box::pin(async move {
                source
                    .tile_png(x, y, zoom_level, None)
                    .await?
                    .ok_or_else(|| {
                        glib::Error::new(
                            gio::IOErrorEnum::NotFound,
                            &gettextrs::gettext("No satellite imagery here"),
                        )
                    })
            })
        }

        /// The default goes through `tile_data_future`, whose binding passes libshumate a NULL source.
        fn start_request(
            &self,
            x: i32,
            y: i32,
            zoom_level: i32,
            cancellable: Option<gio::Cancellable>,
        ) -> Option<shumate::DataSourceRequest> {
            let request = shumate::DataSourceRequest::new(x, y, zoom_level);
            let (source, pending) = (self.obj().clone(), request.clone());
            glib::MainContext::default().spawn_local(async move {
                match source
                    .tile_png(x, y, zoom_level, cancellable.as_ref())
                    .await
                {
                    Ok(Some(png)) => pending.emit_data(&png, true),
                    Ok(None) => pending.complete(),
                    Err(err) => pending.emit_error(&err),
                }
            });
            Some(request)
        }
    }
}

glib::wrapper! {
    pub struct SatelliteData(ObjectSubclass<imp::SatelliteData>) @extends shumate::DataSource;
}

impl SatelliteData {
    fn new(api: &Arc<ImmichApiClient>) -> Self {
        let source: Self = glib::Object::new();
        let _ = source.imp().api.set(api.clone());
        source
    }

    fn api(&self) -> &ImmichApiClient {
        self.imp().api.get().expect("set in SatelliteData::new")
    }

    fn cache(&self) -> &shumate::FileCache {
        self.imp()
            .cache
            .get_or_init(|| shumate::FileCache::new_full(CACHE_BYTES, SOURCE_ID, None))
    }

    async fn tile_png(
        &self,
        x: i32,
        y: i32,
        zoom: i32,
        cancellable: Option<&gio::Cancellable>,
    ) -> Result<Option<glib::Bytes>, glib::Error> {
        let Some(id) = TileId::new(x, y, zoom) else {
            return Ok(None);
        };
        let Some((depth, webp)) = self.nearest_imagery(id, cancellable).await? else {
            return Ok(None);
        };
        let png = tokio::task::spawn_blocking(move || crop_png(&webp, id, depth))
            .await
            .ok()
            .flatten()
            .ok_or_else(|| tile_error(id, "the imagery could not be decoded"))?;
        Ok(Some(glib::Bytes::from_owned(png)))
    }

    /// The closest tile with imagery at or above `id`, and how many zoom levels up it is.
    async fn nearest_imagery(
        &self,
        id: TileId,
        cancellable: Option<&gio::Cancellable>,
    ) -> Result<Option<(u32, glib::Bytes)>, glib::Error> {
        let depths = depths_to_try(id, &self.imp().missing.borrow());
        for depth in depths {
            if cancellable.is_some_and(CancellableExt::is_cancelled) {
                return Err(glib::Error::new(
                    gio::IOErrorEnum::Cancelled,
                    &gettext("Cancelled"),
                ));
            }
            let Some(ancestor) = id.ancestor(depth) else {
                break;
            };
            if let Some(bytes) = self.imagery(ancestor).await? {
                return Ok(Some((depth, bytes)));
            }
            self.imp().missing.borrow_mut().insert(ancestor);
        }
        Ok(None)
    }

    /// A tile's WebP from the cache, revalidated once it is old; a stale copy is used when offline.
    async fn imagery(&self, id: TileId) -> Result<Option<glib::Bytes>, glib::Error> {
        let (x, y, zoom) = id.signed();
        let cached = self.cache().tile_future(x, y, zoom).await.ok();
        let now = glib::DateTime::now_utc().ok();
        if let (Some((bytes, _, modified)), Some(now)) = (&cached, &now)
            && !is_expired(modified.as_ref(), now)
        {
            return Ok(Some(bytes.clone()));
        }
        let etag = cached.as_ref().and_then(|(_, etag, _)| etag.as_deref());
        let fetched = self.download(id, etag).await;
        let stale = cached.map(|(bytes, ..)| bytes);
        match fetched {
            Ok(TileFetch::Found { bytes, etag }) => Ok(Some(self.store(id, bytes, etag))),
            Ok(TileFetch::Unchanged) => {
                self.cache().mark_up_to_date(x, y, zoom);
                Ok(stale)
            }
            Ok(TileFetch::Missing) => Ok(None),
            Ok(TileFetch::RetryAfter(_)) | Err(_) if stale.is_some() => Ok(stale),
            Ok(TileFetch::RetryAfter(wait)) => Err(tile_error(
                id,
                &fill(
                    gettext("VersaTiles asked to wait {seconds} s"),
                    &[("seconds", &wait.as_secs().to_string())],
                ),
            )),
            Err(err) => Err(tile_error(id, &err)),
        }
    }

    async fn download(&self, id: TileId, etag: Option<&str>) -> Result<TileFetch, String> {
        let paused = self.imp().paused_until.get();
        if paused.is_some_and(|until| Instant::now() < until) {
            return Err("waiting for VersaTiles' Retry-After".to_string());
        }
        let fetched = self.api().fetch_map_tile(&id.url(), etag).await?;
        if let TileFetch::RetryAfter(wait) = fetched {
            self.imp().paused_until.set(Some(Instant::now() + wait));
        }
        Ok(fetched)
    }

    fn store(&self, id: TileId, bytes: Vec<u8>, etag: Option<String>) -> glib::Bytes {
        let (x, y, zoom) = id.signed();
        let bytes = glib::Bytes::from_owned(bytes);
        let store = self
            .cache()
            .store_tile_future(x, y, zoom, &bytes, etag.as_deref());
        glib::MainContext::default().spawn_local(async move {
            if let Err(err) = store.await {
                log::debug!("Satellite tile not cached: {err}");
            }
        });
        bytes
    }
}

/// The part of the WebP tile that covers `id`, as a PNG the raster renderer can load.
fn crop_png(webp: &[u8], id: TileId, depth: u32) -> Option<Vec<u8>> {
    let image = webp::Decoder::new(webp).decode()?;
    let (channels, color) = if image.is_alpha() {
        (4, image::ExtendedColorType::Rgba8)
    } else {
        (3, image::ExtendedColorType::Rgb8)
    };
    let crop = id.crop(depth, image.width())?;
    let pixels = crop_pixels(&image, image.width(), channels, crop)?;
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::NoFilter)
        .write_image(&pixels, crop.side, crop.side, color)
        .ok()?;
    Some(png)
}

fn crop_pixels(pixels: &[u8], width: u32, channels: usize, crop: Crop) -> Option<Vec<u8>> {
    let [width, left, top, side] = [width, crop.left, crop.top, crop.side].map(|n| n as usize);
    let row = side * channels;
    let mut cropped = Vec::with_capacity(row * side);
    for y in top..top + side {
        let start = (y * width + left) * channels;
        cropped.extend_from_slice(pixels.get(start..start + row)?);
    }
    Some(cropped)
}

fn tile_error(id: TileId, message: &str) -> glib::Error {
    let text = format!("Satellite tile {}/{}/{}: {message}", id.zoom, id.x, id.y);
    glib::Error::new(gio::IOErrorEnum::Failed, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn tile(zoom: u32, x: u32, y: u32) -> TileId {
        TileId { zoom, x, y }
    }

    #[test]
    fn ancestors_and_crops_cover_the_tile() {
        let london = tile(14, 8190, 5447);
        assert_eq!(london.ancestor(2), Some(tile(12, 2047, 1361)));
        assert_eq!(
            london.crop(2, TILE_SIZE),
            Some(Crop {
                left: 256,
                top: 384,
                side: 128
            })
        );
        assert_eq!(
            london.crop(0, TILE_SIZE),
            Some(Crop {
                left: 0,
                top: 0,
                side: TILE_SIZE
            })
        );
        assert_eq!(london.crop(MAX_DEPTH + 1, TILE_SIZE), None);
        assert_eq!(tile(1, 1, 0).ancestor(2), None);
        assert_eq!(london.url(), format!("{TILE_URL}/14/8190/5447"));
        assert_eq!(TileId::new(8190, 5447, 14), Some(london));
        assert_eq!(TileId::new(-1, 0, 3), None);
    }

    #[test]
    fn depths_skip_known_missing_ancestors_and_missing_zooms() {
        let street = tile(16, 32739, 21790);
        assert_eq!(depths_to_try(street, &HashSet::new()), 0..=MAX_DEPTH);
        let missing = HashSet::from([street.ancestor(3).unwrap()]);
        assert_eq!(depths_to_try(street, &missing), 4..=MAX_DEPTH);
        assert_eq!(
            depths_to_try(tile(21, 0, 0), &HashSet::new()),
            2..=MAX_DEPTH
        );
        assert_eq!(depths_to_try(tile(3, 4, 2), &HashSet::new()), 0..=3);
    }

    #[test]
    fn cached_tiles_expire_after_a_week() {
        let now = glib::DateTime::now_utc().unwrap();
        let days_ago = |days: i32| now.add_days(-days).unwrap();
        assert!(!is_expired(Some(&days_ago(6)), &now));
        assert!(is_expired(Some(&days_ago(8)), &now));
        assert!(is_expired(None, &now));
    }

    #[test]
    fn crops_copy_the_covered_pixels() {
        let pixels: Vec<u8> = (0..16).flat_map(|i| [i, i, i]).collect();
        let crop = Crop {
            left: 2,
            top: 1,
            side: 2,
        };
        let cropped = crop_pixels(&pixels, 4, 3, crop).unwrap();
        let firsts: Vec<u8> = cropped.chunks(3).map(|pixel| pixel[0]).collect();
        assert_eq!(firsts, [6, 7, 10, 11]);
        let outside = Crop {
            left: 3,
            top: 3,
            side: 2,
        };
        assert_eq!(crop_pixels(&pixels, 4, 3, outside), None);
    }
}
