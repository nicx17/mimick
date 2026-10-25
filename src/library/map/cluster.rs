//! Radius clustering of map markers in Web Mercator pixel space, and the camera math around it.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;

use crate::api_client::MapMarker;

/// Markers whose positions are closer than this on screen join one cluster.
pub const CLUSTER_RADIUS_PX: f64 = 60.0;
const TILE_PX: f64 = 256.0;
const MAX_LAT: f64 = 85.051_128_78;
const MAX_CLUSTER_LEVEL: u32 = 20;
const FIT_MAX_ZOOM: f64 = 14.0;
const FIT_PADDING_ZOOM: f64 = 0.4;

type BinKey = (i64, i64);

#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    /// The seed bin; stable at one zoom level, so it identifies the marker across pans.
    pub key: BinKey,
    /// World pixel position at the clustering level.
    pub x: f64,
    pub y: f64,
    pub lat: f64,
    pub lon: f64,
    /// Indices into the marker slice the cluster was built from.
    pub members: Vec<usize>,
}

/// What a map shows: centre, fractional zoom, size in pixels, and rotation in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub lat: f64,
    pub lon: f64,
    pub zoom: f64,
    pub width: f64,
    pub height: f64,
    pub rotation: f64,
}

impl View {
    /// Width and height of the world area the rotated view covers.
    pub fn extent(&self) -> (f64, f64) {
        let (sin, cos) = (self.rotation.sin().abs(), self.rotation.cos().abs());
        (
            self.width * cos + self.height * sin,
            self.width * sin + self.height * cos,
        )
    }
}

#[derive(Default)]
struct Bin {
    members: Vec<usize>,
    sum_x: f64,
    sum_y: f64,
}

impl Bin {
    fn centre(&self) -> (f64, f64) {
        let count = self.members.len() as f64;
        (self.sum_x / count, self.sum_y / count)
    }

    fn absorb(&mut self, other: &Bin) {
        self.members.extend_from_slice(&other.members);
        self.sum_x += other.sum_x;
        self.sum_y += other.sum_y;
    }
}

fn world_size(zoom: f64) -> f64 {
    TILE_PX * 2f64.powf(zoom)
}

fn lon_fraction(lon: f64) -> f64 {
    (lon + 180.0) / 360.0
}

/// Distance from the world's top edge as a fraction of its height.
fn lat_fraction(lat: f64) -> f64 {
    let lat = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
    (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / PI) / 2.0
}

fn latitude_at(fraction: f64) -> f64 {
    (PI * (1.0 - 2.0 * fraction)).sinh().atan().to_degrees()
}

/// Web Mercator pixel position of a coordinate at an integer `zoom`.
pub fn world_px(lat: f64, lon: f64, zoom: u32) -> (f64, f64) {
    let size = world_size(f64::from(zoom));
    (lon_fraction(lon) * size, lat_fraction(lat) * size)
}

/// Integer zoom whose clusters are used while the map shows fractional `zoom`.
pub fn cluster_level(zoom: f64) -> u32 {
    (zoom.max(0.0).floor() as u32).min(MAX_CLUSTER_LEVEL)
}

fn bin_key(x: f64, y: f64) -> BinKey {
    (
        (x / CLUSTER_RADIUS_PX).floor() as i64,
        (y / CLUSTER_RADIUS_PX).floor() as i64,
    )
}

fn bin_markers(markers: &[MapMarker], level: u32) -> HashMap<BinKey, Bin> {
    let mut bins: HashMap<BinKey, Bin> = HashMap::new();
    for (index, marker) in markers.iter().enumerate() {
        let (x, y) = world_px(marker.lat, marker.lon, level);
        let bin = bins.entry(bin_key(x, y)).or_default();
        bin.members.push(index);
        bin.sum_x += x;
        bin.sum_y += y;
    }
    bins
}

/// The fullest bins go first and absorb neighbouring bins whose centres are within the radius.
pub fn cluster(markers: &[MapMarker], level: u32) -> Vec<Cluster> {
    let bins = bin_markers(markers, level);
    let mut seeds: Vec<BinKey> = bins.keys().copied().collect();
    seeds.sort_by_key(|key| (Reverse(bins[key].members.len()), *key));
    let mut taken = HashSet::with_capacity(bins.len());
    let mut clusters = Vec::new();
    for seed in seeds {
        if taken.insert(seed) {
            clusters.push(grow(seed, &bins, &mut taken, markers));
        }
    }
    clusters
}

fn grow(
    seed: BinKey,
    bins: &HashMap<BinKey, Bin>,
    taken: &mut HashSet<BinKey>,
    markers: &[MapMarker],
) -> Cluster {
    let (seed_x, seed_y) = bins[&seed].centre();
    let mut merged = Bin::default();
    merged.absorb(&bins[&seed]);
    for key in neighbours(seed) {
        let Some(bin) = bins.get(&key) else {
            continue;
        };
        let (x, y) = bin.centre();
        if (x - seed_x).hypot(y - seed_y) <= CLUSTER_RADIUS_PX && taken.insert(key) {
            merged.absorb(bin);
        }
    }
    let (x, y) = merged.centre();
    let (lat, lon) = mean_position(&merged.members, markers);
    Cluster {
        key: seed,
        x,
        y,
        lat,
        lon,
        members: merged.members,
    }
}

fn neighbours((bx, by): BinKey) -> impl Iterator<Item = BinKey> {
    (-1..=1)
        .flat_map(move |dx| (-1..=1).map(move |dy| (bx + dx, by + dy)))
        .filter(move |&key| key != (bx, by))
}

fn mean_position(members: &[usize], markers: &[MapMarker]) -> (f64, f64) {
    let count = members.len() as f64;
    let lat = members.iter().map(|&i| markers[i].lat).sum::<f64>() / count;
    let lon = members.iter().map(|&i| markers[i].lon).sum::<f64>() / count;
    (lat, lon)
}

/// World pixels on screen at the clustering level, padded by the cluster radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelWindow {
    min: (f64, f64),
    max: (f64, f64),
}

impl PixelWindow {
    /// The clustering level and the visible window for `view`.
    pub fn around(view: &View) -> (u32, Self) {
        let level = cluster_level(view.zoom);
        let scale = 2f64.powf(view.zoom - f64::from(level));
        let (x, y) = world_px(view.lat, view.lon, level);
        let (width, height) = view.extent();
        let half_w = width / scale / 2.0 + CLUSTER_RADIUS_PX;
        let half_h = height / scale / 2.0 + CLUSTER_RADIUS_PX;
        let window = Self {
            min: (x - half_w, y - half_h),
            max: (x + half_w, y + half_h),
        };
        (level, window)
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        (self.min.0..=self.max.0).contains(&x) && (self.min.1..=self.max.1).contains(&y)
    }
}

/// Indices of markers on screen, counting copies of the world to either side.
pub fn markers_in_view(markers: &[MapMarker], view: &View) -> Vec<usize> {
    let size = world_size(view.zoom);
    let (centre_x, centre_y) = (lon_fraction(view.lon) * size, lat_fraction(view.lat) * size);
    let (width, height) = view.extent();
    markers
        .iter()
        .enumerate()
        .filter(|(_, marker)| {
            let dx = (lon_fraction(marker.lon) * size - centre_x + size / 2.0).rem_euclid(size)
                - size / 2.0;
            let dy = lat_fraction(marker.lat) * size - centre_y;
            dx.abs() <= width / 2.0 && dy.abs() <= height / 2.0
        })
        .map(|(index, _)| index)
        .collect()
}

/// Lowest zoom at which the world is at least `height` pixels tall, so it never repeats vertically.
pub fn min_zoom_for(height: f64) -> f64 {
    (height / TILE_PX).log2().max(0.0)
}

/// `lat` moved just enough that a `height`-pixel view at `zoom` stays inside the world.
pub fn clamp_centre_lat(lat: f64, zoom: f64, height: f64) -> f64 {
    let size = world_size(zoom);
    let half = (height / 2.0).min(size / 2.0);
    let y = (lat_fraction(lat) * size).clamp(half, size - half);
    latitude_at(y / size)
}

/// Most common city among `members`, else the most common country.
pub fn area_label(markers: &[MapMarker], members: &[usize]) -> Option<String> {
    let names = |pick: fn(&MapMarker) -> Option<&str>| {
        most_common(members.iter().filter_map(|&i| pick(&markers[i])))
    };
    names(|m| m.city.as_deref()).or_else(|| names(|m| m.country.as_deref()))
}

fn most_common<'a>(names: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for name in names.map(str::trim).filter(|name| !name.is_empty()) {
        *counts.entry(name).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(name, _)| name.to_string())
}

/// Centre latitude, longitude, and zoom that fit every marker in a `width` x `height` view.
pub fn fit(markers: &[MapMarker], width: f64, height: f64) -> Option<(f64, f64, f64)> {
    let first = markers.first()?;
    let mut bounds = (first.lat, first.lat, first.lon, first.lon);
    for marker in markers {
        bounds = (
            bounds.0.min(marker.lat),
            bounds.1.max(marker.lat),
            bounds.2.min(marker.lon),
            bounds.3.max(marker.lon),
        );
    }
    let (south, north, west, east) = bounds;
    let (left, top) = world_px(north, west, 0);
    let (right, bottom) = world_px(south, east, 0);
    let fits = (width / (right - left).max(1e-9)).min(height / (bottom - top).max(1e-9));
    let zoom = (fits.log2() - FIT_PADDING_ZOOM).clamp(1.0, FIT_MAX_ZOOM);
    Some(((south + north) / 2.0, (west + east) / 2.0, zoom))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(id: &str, lat: f64, lon: f64, city: Option<&str>) -> MapMarker {
        MapMarker {
            id: id.into(),
            lat,
            lon,
            city: city.map(Into::into),
            country: Some("Portugal".into()),
        }
    }

    fn view(lat: f64, lon: f64, zoom: f64) -> View {
        View {
            lat,
            lon,
            zoom,
            width: 800.0,
            height: 600.0,
            rotation: 0.0,
        }
    }

    #[test]
    fn world_px_matches_web_mercator() {
        assert_eq!(world_px(0.0, 0.0, 0), (128.0, 128.0));
        let (x, _) = world_px(0.0, 180.0, 1);
        assert_eq!(x, 512.0);
        let (_, y) = world_px(90.0, 0.0, 0);
        assert!(y.abs() < 1e-6);
        assert!((latitude_at(lat_fraction(51.5)) - 51.5).abs() < 1e-9);
    }

    #[test]
    fn nearby_markers_share_a_cluster_until_zoomed_in() {
        let markers = [
            marker("a", 38.7223, -9.1393, Some("Lisbon")),
            marker("b", 38.7230, -9.1400, Some("Lisbon")),
            marker("c", 41.1579, -8.6291, Some("Porto")),
        ];
        let far = cluster(&markers, 3);
        assert_eq!(far.len(), 1);
        assert_eq!(far[0].members.len(), 3);
        let near = cluster(&markers, 10);
        assert_eq!(near.len(), 2);
        let lisbon = near.iter().find(|c| c.members.len() == 2).unwrap();
        assert!((lisbon.lat - 38.72265).abs() < 1e-9);
    }

    #[test]
    fn clusters_in_neighbouring_bins_merge_when_close() {
        let level = 4;
        let (x, y) = world_px(10.0, 10.0, level);
        let edge = (x / CLUSTER_RADIUS_PX).ceil() * CLUSTER_RADIUS_PX;
        let lon_at = |px: f64| px / world_size(f64::from(level)) * 360.0 - 180.0;
        let markers = [
            marker("a", 10.0, lon_at(edge - 5.0), None),
            marker("b", 10.0, lon_at(edge + 5.0), None),
        ];
        assert_ne!(
            bin_key(edge - 5.0, y),
            bin_key(edge + 5.0, y),
            "the two markers straddle a bin edge"
        );
        assert_eq!(cluster(&markers, level).len(), 1);
    }

    #[test]
    fn the_fullest_bin_keeps_its_place() {
        let mut markers: Vec<MapMarker> = (0..5)
            .map(|i| marker(&i.to_string(), 40.0, 0.0, None))
            .collect();
        markers.push(marker("far", -40.0, 120.0, None));
        let clusters = cluster(&markers, 6);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].members.len(), 5);
    }

    #[test]
    fn cluster_level_floors_and_caps_zoom() {
        assert_eq!(cluster_level(3.9), 3);
        assert_eq!(cluster_level(-1.0), 0);
        assert_eq!(cluster_level(25.0), MAX_CLUSTER_LEVEL);
    }

    #[test]
    fn window_covers_the_viewport_plus_the_radius() {
        let (level, window) = PixelWindow::around(&view(0.0, 0.0, 4.0));
        assert_eq!(level, 4);
        let (x, y) = world_px(0.0, 0.0, 4);
        assert!(window.contains(x, y));
        assert!(window.contains(x + 400.0 + CLUSTER_RADIUS_PX - 1.0, y));
        assert!(!window.contains(x + 400.0 + CLUSTER_RADIUS_PX + 1.0, y));
        let rotated = View {
            rotation: PI / 2.0,
            ..view(0.0, 0.0, 4.0)
        };
        let (_, window) = PixelWindow::around(&rotated);
        assert!(window.contains(x, y + 400.0));
    }

    #[test]
    fn markers_in_view_wrap_around_the_antimeridian() {
        let markers = [
            marker("fiji", -17.7, 178.0, None),
            marker("samoa", -13.8, -172.0, None),
            marker("lisbon", 38.7, -9.1, None),
        ];
        let pacific = view(-15.0, 180.0, 4.0);
        assert_eq!(markers_in_view(&markers, &pacific), vec![0, 1]);
        let world = view(0.0, 0.0, 0.0);
        assert_eq!(markers_in_view(&markers, &world).len(), 3);
    }

    #[test]
    fn camera_limits_keep_one_world_on_screen() {
        assert_eq!(min_zoom_for(1024.0), 2.0);
        assert_eq!(min_zoom_for(100.0), 0.0);
        assert!((clamp_centre_lat(10.0, 3.0, 600.0) - 10.0).abs() < 1e-9);
        let top = clamp_centre_lat(85.0, 2.0, 1000.0);
        let (_, y) = world_px(top, 0.0, 2);
        assert!((y - 500.0).abs() < 1e-6);
        assert!(clamp_centre_lat(-85.0, 2.0, 1000.0) < 0.0);
    }

    #[test]
    fn rotated_views_cover_a_wider_area() {
        let quarter = View {
            rotation: PI / 2.0,
            ..view(0.0, 0.0, 2.0)
        };
        let (width, height) = quarter.extent();
        assert!((width - 600.0).abs() < 1e-9 && (height - 800.0).abs() < 1e-9);
    }

    #[test]
    fn area_label_prefers_the_most_common_city() {
        let markers = [
            marker("a", 0.0, 0.0, Some("Porto")),
            marker("b", 0.0, 0.0, Some("Lisbon")),
            marker("c", 0.0, 0.0, Some(" Lisbon ")),
            marker("d", 0.0, 0.0, None),
        ];
        assert_eq!(
            area_label(&markers, &[0, 1, 2, 3]).as_deref(),
            Some("Lisbon")
        );
        assert_eq!(area_label(&markers, &[3]).as_deref(), Some("Portugal"));
        assert_eq!(area_label(&markers, &[0, 1]).as_deref(), Some("Lisbon"));
    }

    #[test]
    fn fit_centres_on_the_markers() {
        let markers = [marker("a", 38.0, -9.0, None), marker("b", 42.0, -7.0, None)];
        let (lat, lon, zoom) = fit(&markers, 800.0, 600.0).unwrap();
        assert_eq!((lat, lon), (40.0, -8.0));
        assert!((5.0..8.0).contains(&zoom));
        let (_, _, single) = fit(&markers[..1], 800.0, 600.0).unwrap();
        assert_eq!(single, FIT_MAX_ZOOM);
        assert_eq!(fit(&[], 800.0, 600.0), None);
    }
}
