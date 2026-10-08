//! EXIF / XMP metadata extraction for local files.
//!
//! Uses `nom-exif` — pure-Rust, no native dependencies. Reads metadata from
//! JPEG, PNG, TIFF, WebP, HEIF / HEIC, JXL and MP4 / MOV. RAW formats fall
//! through as "no metadata"; that case is hidden in the UI rather than shown
//! as an error.
//!
//! Results are cached on disk keyed by `(mtime, size)` so re-opening the
//! lightbox on the same file doesn't re-parse a multi-megabyte image.
//! EXIF / XMP extraction via `nom-exif`. Disk-cached by `(mtime, size)`.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use nom_exif::{EntryValue, Exif, ExifTag, read_exif as nom_read_exif};
use serde::{Deserialize, Serialize};

/// Mirrors remote `ExifInfo` so the renderer is source-agnostic.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LocalExif {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens_model: Option<String>,
    pub f_number: Option<f64>,
    pub focal_length: Option<f64>,
    pub iso: Option<u32>,
    pub exposure_time: Option<String>,
    /// RFC3339; consumed by `format_datetime_display`.
    pub date_time_original: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub description: Option<String>,
}

impl LocalExif {
    /// True when no useful fields were parsed.
    pub fn is_empty(&self) -> bool {
        self.make.is_none()
            && self.model.is_none()
            && self.lens_model.is_none()
            && self.f_number.is_none()
            && self.focal_length.is_none()
            && self.iso.is_none()
            && self.exposure_time.is_none()
            && self.date_time_original.is_none()
            && self.latitude.is_none()
            && self.longitude.is_none()
            && self.image_width.is_none()
            && self.image_height.is_none()
            && self.description.is_none()
    }
}

/// Returns `None` if the file has no recognisable metadata.
pub fn read_exif(path: &Path) -> Option<LocalExif> {
    let exif = nom_read_exif(path)
        .map_err(|err| {
            log::debug!("nom-exif read failed for {}: {}", path.display(), err);
            err
        })
        .ok()?;

    let local = LocalExif {
        make: text(&exif, ExifTag::Make),
        model: text(&exif, ExifTag::Model),
        lens_model: text(&exif, ExifTag::LensModel).or_else(|| text(&exif, ExifTag::LensMake)),
        f_number: rational_f64(&exif, ExifTag::FNumber),
        focal_length: rational_f64(&exif, ExifTag::FocalLength),
        iso: integer_u32(&exif, ExifTag::ISOSpeedRatings),
        exposure_time: exposure_time(&exif),
        date_time_original: datetime_rfc3339(&exif),
        latitude: exif
            .gps_info()
            .and_then(nom_exif::GPSInfo::latitude_decimal),
        longitude: exif
            .gps_info()
            .and_then(nom_exif::GPSInfo::longitude_decimal),
        image_width: integer_u32(&exif, ExifTag::ImageWidth)
            .or_else(|| integer_u32(&exif, ExifTag::ExifImageWidth)),
        image_height: integer_u32(&exif, ExifTag::ImageHeight)
            .or_else(|| integer_u32(&exif, ExifTag::ExifImageHeight)),
        description: text(&exif, ExifTag::ImageDescription).filter(|s| !s.trim().is_empty()),
    };

    if local.is_empty() { None } else { Some(local) }
}

/// Disk-cached `read_exif`; key is `(mtime_nanos, size)`.
pub fn load_or_extract(cache_root: &Path, path: &Path) -> Option<LocalExif> {
    let key = cache_key(path)?;
    let cache_path = cache_root.join("exif").join(format!("{}.json", key.digest));

    if let Ok(mut f) = fs::File::open(&cache_path) {
        let mut buf = String::new();
        if f.read_to_string(&mut buf).is_ok()
            && let Ok(entry) = serde_json::from_str::<CacheEntry>(&buf)
            && entry.mtime_nanos == key.mtime_nanos
            && entry.size == key.size
        {
            return entry.exif;
        }
    }

    let parsed = read_exif(path);

    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let entry = CacheEntry {
        mtime_nanos: key.mtime_nanos,
        size: key.size,
        exif: parsed.clone(),
    };
    if let Ok(payload) = serde_json::to_vec(&entry)
        && let Ok(mut f) = fs::File::create(&cache_path)
    {
        let _ = f.write_all(&payload);
    }

    parsed
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    mtime_nanos: u128,
    size: u64,
    exif: Option<LocalExif>,
}

struct CacheKey {
    digest: String,
    mtime_nanos: u128,
    size: u64,
}

fn cache_key(path: &Path) -> Option<CacheKey> {
    let meta = fs::metadata(path).ok()?;
    let size = meta.len();
    let mtime = meta.modified().ok()?;
    let mtime_nanos = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let digest = blake3::hash(canonical.as_os_str().as_encoded_bytes())
        .to_hex()
        .to_string();
    Some(CacheKey {
        digest,
        mtime_nanos,
        size,
    })
}

fn text(exif: &Exif, tag: ExifTag) -> Option<String> {
    let value = exif.get(tag)?;
    value
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn integer_u32(exif: &Exif, tag: ExifTag) -> Option<u32> {
    let value = exif.get(tag)?;
    match value {
        EntryValue::U32(v) => Some(*v),
        EntryValue::U16(v) => Some(u32::from(*v)),
        EntryValue::U8(v) => Some(u32::from(*v)),
        _ => None,
    }
}

fn rational_f64(exif: &Exif, tag: ExifTag) -> Option<f64> {
    let value = exif.get(tag)?;
    if let Some(r) = value.as_urational() {
        let n = r.numerator();
        let d = r.denominator();
        if d == 0 || n == 0 {
            return None;
        }
        return Some(f64::from(n) / f64::from(d));
    }
    value.as_f64()
}

/// Display exposure as "1/250" rather than the raw rational.
fn exposure_time(exif: &Exif) -> Option<String> {
    let r = exif.get(ExifTag::ExposureTime)?.as_urational()?;
    let n = r.numerator();
    let d = r.denominator();
    if d == 0 || n == 0 {
        return None;
    }
    if n == 1 {
        Some(format!("1/{d}"))
    } else if n > d {
        // Long exposures like 2/1 → "2 s"
        let secs = f64::from(n) / f64::from(d);
        Some(format!("{secs:.1} s"))
    } else {
        // Reduce 5/100 → "1/20"-ish display by formatting as the fraction.
        Some(format!("{n}/{d}"))
    }
}

/// Combine DateTimeOriginal with OffsetTimeOriginal (when present) into a
/// stable RFC3339 string.
fn datetime_rfc3339(exif: &Exif) -> Option<String> {
    use nom_exif::ExifDateTime;
    let dt = exif.get(ExifTag::DateTimeOriginal)?.as_datetime()?;
    let formatted = match dt {
        ExifDateTime::Aware(dt) => dt.to_rfc3339(),
        ExifDateTime::Naive(naive) => {
            // Try to attach an offset if one was recorded separately.
            let offset = exif
                .get(ExifTag::OffsetTimeOriginal)
                .and_then(|v| v.as_str())
                .and_then(parse_offset);
            match offset {
                Some(off) => format!("{}{}", naive.format("%Y-%m-%dT%H:%M:%S"), off),
                None => format!("{}Z", naive.format("%Y-%m-%dT%H:%M:%S")),
            }
        }
    };
    Some(formatted)
}

/// Validate a raw "+HH:MM" / "-HH:MM" offset string from OffsetTimeOriginal.
fn parse_offset(s: &str) -> Option<String> {
    let s = s.trim();
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let sign = s.as_bytes()[0];
    if sign != b'+' && sign != b'-' {
        return None;
    }
    if s.as_bytes()[3] != b':' {
        return None;
    }
    s[1..3].parse::<u8>().ok().filter(|h| *h <= 14)?;
    s[4..6].parse::<u8>().ok().filter(|m| *m <= 59)?;
    Some(s.to_string())
}

/// Cache directory for EXIF entries; lives under the same XDG cache root used
/// by the thumbnail cache.
pub fn cache_root() -> PathBuf {
    crate::profile::cache_dir().unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_empty_detects_no_fields() {
        assert!(LocalExif::default().is_empty());
    }

    #[test]
    fn is_empty_false_when_any_field_set() {
        let e = LocalExif {
            iso: Some(400),
            ..Default::default()
        };
        assert!(!e.is_empty());

        let e = LocalExif {
            description: Some("hi".into()),
            ..Default::default()
        };
        assert!(!e.is_empty());
    }

    #[test]
    fn parse_offset_accepts_typical_form() {
        assert_eq!(parse_offset("+05:30").as_deref(), Some("+05:30"));
        assert_eq!(parse_offset("-08:00").as_deref(), Some("-08:00"));
    }

    #[test]
    fn parse_offset_rejects_bad_strings() {
        assert!(parse_offset("0530").is_none());
        assert!(parse_offset("+5:30").is_none());
        assert!(parse_offset("+15:00").is_none());
        assert!(parse_offset("+05:60").is_none());
        assert!(parse_offset("").is_none());
    }

    #[test]
    fn cache_key_invalidates_on_mtime_change() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let k1 = cache_key(tmp.path()).expect("key1");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(tmp.path(), b"changed").unwrap();
        let k2 = cache_key(tmp.path()).expect("key2");
        assert_eq!(k1.digest, k2.digest, "digest is canonical-path-based");
        assert_ne!(
            k1.mtime_nanos, k2.mtime_nanos,
            "rewriting the file must bump mtime"
        );
    }

    #[test]
    fn load_or_extract_caches_negative_result() {
        // A plain text file has no EXIF; verify we persist the negative
        // result so we don't re-parse on every reopen.
        let cache_dir = tempfile::tempdir().expect("cache dir");
        let mut data = tempfile::NamedTempFile::new().expect("tempfile");
        use std::io::Write;
        data.write_all(b"not an image").unwrap();
        let first = load_or_extract(cache_dir.path(), data.path());
        let count_after_first = std::fs::read_dir(cache_dir.path().join("exif"))
            .map(|d| d.count())
            .unwrap_or(0);
        let second = load_or_extract(cache_dir.path(), data.path());
        assert_eq!(first.is_some(), second.is_some());
        assert_eq!(count_after_first, 1, "negative result is persisted once");
    }
}
