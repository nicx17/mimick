//! The EXIF a saved copy carries: the original's own block, maker notes and all, made to fit
//! the copy, with Immich's capture time, place, caption, and rating on top, since those may
//! have been changed in Immich since the file was made.

use chrono::DateTime;

use super::exif_tree::{Dir, ExifTree, Field, Order};
use crate::api_client::LibraryAsset;

/// What a copy keeps of its original's details as Immich has them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::library) struct CopyDetails {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub f_number: Option<f64>,
    pub focal_length: Option<f64>,
    pub iso: Option<u32>,
    pub exposure_time: Option<String>,
    /// Wall-clock capture time as EXIF writes it, and its UTC offset ("+05:30").
    pub taken: Option<(String, String)>,
    /// The photo's time zone as Immich names it.
    pub time_zone: Option<String>,
    pub gps: Option<(f64, f64)>,
    pub description: Option<String>,
    pub rating: Option<u8>,
}

impl CopyDetails {
    pub(super) fn from_asset(asset: &LibraryAsset) -> Self {
        let Some(exif) = asset.exif_info.as_ref() else {
            return Self::default();
        };
        Self {
            make: exif.make.clone(),
            model: exif.model.clone(),
            lens: exif.lens_model.clone(),
            f_number: exif.f_number,
            focal_length: exif.focal_length,
            iso: exif.iso,
            exposure_time: exif.exposure_time.clone(),
            taken: exif
                .date_time_original
                .as_deref()
                .and_then(|utc| taken(utc, asset.local_date_time.as_deref())),
            time_zone: exif.time_zone.clone(),
            gps: exif.latitude.zip(exif.longitude),
            description: exif.description.clone().filter(|d| !d.trim().is_empty()),
            rating: exif
                .rating
                .and_then(|r| u8::try_from(r).ok())
                .filter(|r| (1..=5).contains(r)),
        }
    }

    /// The capture time as RFC 3339 with its offset, as Immich takes it.
    pub(in crate::library) fn taken_rfc3339(&self) -> Option<String> {
        let (wall, offset) = self.taken.as_ref()?;
        let (date, time) = wall.split_once(' ')?;
        Some(format!("{}T{time}{offset}", date.replace(':', "-")))
    }
}

/// The wall-clock time and offset from Immich's UTC instant and its local time, which Immich
/// sends as if it were UTC. Without a local time, the UTC time with a zero offset.
fn taken(utc: &str, local: Option<&str>) -> Option<(String, String)> {
    let instant = DateTime::parse_from_rfc3339(utc).ok()?;
    let wall = local
        .and_then(|l| DateTime::parse_from_rfc3339(l).ok())
        .unwrap_or(instant);
    let minutes = (wall.naive_utc() - instant.naive_utc()).num_minutes();
    let sign = if minutes < 0 { '-' } else { '+' };
    let offset = format!("{sign}{:02}:{:02}", minutes.abs() / 60, minutes.abs() % 60);
    Some((wall.format("%Y:%m:%d %H:%M:%S").to_string(), offset))
}

/// IFD0 tags that describe how the original stored its pixels, or point at them; they'd be
/// wrong for the copy. XMP and the colour profile travel in segments of their own.
const PIXEL_LAYOUT: [u16; 38] = [
    0x00FE, 0x00FF, 0x0100, 0x0101, 0x0102, 0x0103, 0x0106, 0x0107, 0x010A, 0x0111, 0x0115, 0x0116,
    0x0117, 0x0118, 0x0119, 0x011C, 0x0120, 0x0121, 0x013D, 0x0142, 0x0143, 0x0144, 0x0145, 0x014A,
    0x0152, 0x0153, 0x015B, 0x0201, 0x0202, 0x0211, 0x0212, 0x02BC, 0x828D, 0x828E, 0x8773, 0x0154,
    0x0155, 0x0200,
];
/// DNG's tags, which describe raw sensor data the copy doesn't have.
const DNG_TAGS: std::ops::RangeInclusive<u16> = 0xC612..=0xCDFF;
/// Windows' filler, which only makes the block bigger.
const PADDING: u16 = 0xEA1C;

/// The copy's EXIF block, at most `limit` bytes: the original's when there is one, else one
/// made from Immich's values alone.
pub(super) fn exif_for_copy(
    original: Option<ExifTree>,
    details: &CopyDetails,
    size: (u32, u32),
    limit: usize,
) -> Option<Vec<u8>> {
    let made = |mut tree: ExifTree| {
        fit_to_copy(&mut tree, size);
        set_immich_values(&mut tree, details);
        fill_camera(&mut tree, details);
        tree.write(limit)
    };
    original
        .and_then(made)
        .or_else(|| made(ExifTree::empty(Order::Little)))
}

/// Upright, the copy's size, and nothing about the original's pixel layout.
fn fit_to_copy(tree: &mut ExifTree, (w, h): (u32, u32)) {
    tree.retain(Dir::Ifd0, |tag| {
        !PIXEL_LAYOUT.contains(&tag) && !DNG_TAGS.contains(&tag) && tag != PADDING
    });
    tree.retain(Dir::Exif, |tag| tag != PADDING);
    tree.set(Dir::Ifd0, tree.short(0x0112, 1));
    tree.set(Dir::Exif, tree.long(0xA002, w));
    tree.set(Dir::Exif, tree.long(0xA003, h));
}

/// Capture time, place, caption, and rating as Immich has them now.
fn set_immich_values(tree: &mut ExifTree, d: &CopyDetails) {
    set_or_drop(
        tree,
        Dir::Ifd0,
        0x010E,
        d.description.as_deref().map(|t| ExifTree::ascii(0x010E, t)),
    );
    let rating = d.rating.map(|r| tree.short(0x4746, u16::from(r)));
    set_or_drop(tree, Dir::Ifd0, 0x4746, rating);
    tree.take(Dir::Ifd0, 0x4749);
    if let Some((wall, offset)) = &d.taken {
        tree.set(Dir::Exif, ExifTree::ascii(0x9003, wall));
        tree.set(Dir::Exif, ExifTree::ascii(0x9011, offset));
    }
    if let Some((lat, lon)) = d.gps {
        if tree.get(Dir::Gps, 0x0000).is_none() {
            tree.set(Dir::Gps, ExifTree::bytes(0x0000, 1, vec![2, 3, 0, 0]));
        }
        tree.set(
            Dir::Gps,
            ExifTree::ascii(0x0001, if lat < 0.0 { "S" } else { "N" }),
        );
        tree.set(Dir::Gps, tree.rationals(0x0002, &degrees(lat.abs())));
        tree.set(
            Dir::Gps,
            ExifTree::ascii(0x0003, if lon < 0.0 { "W" } else { "E" }),
        );
        tree.set(Dir::Gps, tree.rationals(0x0004, &degrees(lon.abs())));
    }
}

fn set_or_drop(tree: &mut ExifTree, dir: Dir, tag: u16, field: Option<Field>) {
    match field {
        Some(field) => tree.set(dir, field),
        None => {
            tree.take(dir, tag);
        }
    }
}

/// Camera details the original's block lacks, from Immich; the block's own win.
fn fill_camera(tree: &mut ExifTree, d: &CopyDetails) {
    let texts = [
        (Dir::Ifd0, 0x010F, d.make.as_deref()),
        (Dir::Ifd0, 0x0110, d.model.as_deref()),
        (Dir::Exif, 0xA434, d.lens.as_deref()),
    ];
    for (dir, tag, text) in texts {
        if let Some(text) = text.filter(|t| !t.trim().is_empty() && tree.get(dir, tag).is_none()) {
            tree.set(dir, ExifTree::ascii(tag, text));
        }
    }
    let rationals = [
        (
            0x829A,
            d.exposure_time.as_deref().and_then(exposure_rational),
        ),
        (0x829D, d.f_number.map(thousandths)),
        (0x920A, d.focal_length.map(thousandths)),
    ];
    for (tag, value) in rationals {
        if let Some(value) = value.filter(|_| tree.get(Dir::Exif, tag).is_none()) {
            tree.set(Dir::Exif, tree.rationals(tag, &[value]));
        }
    }
    if let Some(iso) = d.iso.filter(|_| tree.get(Dir::Exif, 0x8827).is_none()) {
        tree.set(
            Dir::Exif,
            tree.short(0x8827, iso.min(u32::from(u16::MAX)) as u16),
        );
    }
    if tree.get(Dir::Exif, 0x9000).is_none() {
        tree.set(Dir::Exif, ExifTree::bytes(0x9000, 7, b"0232".to_vec()));
    }
}

fn thousandths(v: f64) -> (u32, u32) {
    ((v.max(0.0) * 1000.0).round() as u32, 1000)
}

/// "1/125" as it is; "0.5" or "2" as a fraction.
fn exposure_rational(text: &str) -> Option<(u32, u32)> {
    if let Some((n, d)) = text.split_once('/') {
        return Some((n.trim().parse().ok()?, d.trim().parse().ok()?)).filter(|(_, d)| *d > 0);
    }
    let secs: f64 = text.trim().parse().ok()?;
    if secs <= 0.0 {
        None
    } else if secs < 1.0 {
        Some((1, (1.0 / secs).round() as u32))
    } else {
        Some(((secs * 10.0).round() as u32, 10))
    }
}

/// Degrees, minutes, and seconds to a thousandth.
fn degrees(v: f64) -> [(u32, u32); 3] {
    let whole = v.trunc();
    let minutes = ((v - whole) * 60.0).trunc();
    let seconds = (v - whole - minutes / 60.0) * 3600.0;
    [
        (whole as u32, 1),
        (minutes as u32, 1),
        ((seconds * 1000.0).round() as u32, 1000),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::photo_editor::embed::{JPEG_EXIF_ROOM, Metadata, jpeg_with};

    fn details() -> CopyDetails {
        CopyDetails {
            make: Some("Ignored".into()),
            lens: Some("23mm F2".into()),
            f_number: Some(2.8),
            iso: Some(400),
            taken: Some(("2024:01:15 19:55:15".into(), "+05:30".into())),
            gps: Some((-33.8568, 151.2153)),
            description: Some("Harbour".into()),
            rating: Some(4),
            ..CopyDetails::default()
        }
    }

    /// A big-endian original with a maker note, turned, with a strip and an old caption.
    fn original() -> ExifTree {
        let mut tree = ExifTree::empty(Order::Big);
        tree.set(Dir::Ifd0, ExifTree::ascii(0x010E, "Old caption"));
        tree.set(Dir::Ifd0, ExifTree::ascii(0x010F, "Canon"));
        tree.set(Dir::Ifd0, tree.short(0x0112, 6));
        tree.set(Dir::Ifd0, tree.long(0x0111, 99_999));
        tree.set(Dir::Exif, ExifTree::ascii(0x9003, "2020:01:01 00:00:00"));
        tree.set_maker_note(b"CanonNotes", 400);
        ExifTree::parse(&tree.write(usize::MAX).unwrap()).unwrap()
    }

    #[test]
    fn capture_times_keep_their_wall_clock_and_offset() {
        let (wall, offset) =
            taken("2024-01-15T14:25:15.000Z", Some("2024-01-15T19:55:15.000Z")).unwrap();
        assert_eq!(
            (wall.as_str(), offset.as_str()),
            ("2024:01:15 19:55:15", "+05:30")
        );
        let west = taken("2024-01-15T14:25:15Z", Some("2024-01-15T09:25:15Z")).unwrap();
        assert_eq!(west.1, "-05:00");
        assert_eq!(taken("2024-01-15T14:25:15Z", None).unwrap().1, "+00:00");
        assert_eq!(
            details().taken_rfc3339().as_deref(),
            Some("2024-01-15T19:55:15+05:30")
        );
    }

    #[test]
    fn exposure_times_become_fractions() {
        assert_eq!(exposure_rational("1/125"), Some((1, 125)));
        assert_eq!(exposure_rational("0.5"), Some((1, 2)));
        assert_eq!(exposure_rational("2"), Some((20, 10)));
        assert_eq!(exposure_rational("1/0"), None);
    }

    #[test]
    fn the_originals_block_is_kept_fitted_with_immichs_values_on_top() {
        let block =
            exif_for_copy(Some(original()), &details(), (1200, 800), JPEG_EXIF_ROOM).unwrap();
        let tree = ExifTree::parse(&block).unwrap();
        assert_eq!(tree.order, Order::Big);
        assert!(tree.has_maker_note());
        assert_eq!(tree.get(Dir::Ifd0, 0x0112).unwrap().value, [0, 1]);
        assert!(tree.get(Dir::Ifd0, 0x0111).is_none());
        assert_eq!(tree.get(Dir::Ifd0, 0x010F).unwrap().value, b"Canon\0");
        assert_eq!(tree.get(Dir::Exif, 0xA002).unwrap().value, [0, 0, 4, 176]);

        let meta = Metadata {
            exif: Some(block),
            ..Metadata::default()
        };
        let pixels = vec![200u8; 8 * 8 * 4];
        let image = turbojpeg::Image {
            pixels: pixels.as_slice(),
            width: 8,
            pitch: 8 * 4,
            height: 8,
            format: turbojpeg::PixelFormat::RGBA,
        };
        let jpeg = turbojpeg::compress(image, 90, turbojpeg::Subsamp::Sub2x2).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("copy.jpg");
        std::fs::write(&path, jpeg_with(&jpeg, &meta)).unwrap();
        let read = crate::library::local_exif::read_exif(&path).unwrap();
        assert_eq!(read.make.as_deref(), Some("Canon"));
        assert_eq!(read.lens_model.as_deref(), Some("23mm F2"));
        assert_eq!(read.iso, Some(400));
        assert_eq!(read.description.as_deref(), Some("Harbour"));
        assert_eq!(
            read.date_time_original.as_deref(),
            Some("2024-01-15T19:55:15+05:30")
        );
        assert!((read.latitude.unwrap() + 33.8568).abs() < 1e-4);
        assert!((read.longitude.unwrap() - 151.2153).abs() < 1e-4);
    }

    #[test]
    fn without_an_original_block_immichs_values_make_one() {
        let block = exif_for_copy(None, &details(), (10, 10), JPEG_EXIF_ROOM).unwrap();
        let tree = ExifTree::parse(&block).unwrap();
        assert_eq!(tree.get(Dir::Ifd0, 0x010F).unwrap().value, b"Ignored\0");
        assert!(!tree.has_maker_note());
    }
}
