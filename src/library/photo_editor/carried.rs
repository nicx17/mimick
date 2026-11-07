//! The original's own metadata, lifted out of whichever container it came in: EXIF with its
//! maker notes, XMP, and the colour profile its pixels are in.

use std::io::Cursor;
use std::path::Path;

use image::ImageDecoder;

use super::exif_tree::{Dir, ExifTree};

const XMP_PACKET: u16 = 0x02BC;
const ICC_PROFILE: u16 = 0x8773;
/// The UUID box holding a Canon CR3's metadata, one TIFF block per part.
const CANON_UUID: [u8; 16] = [
    0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a, 0x48,
];
const JXL_CONTAINER: [u8; 12] = [
    0, 0, 0, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A,
];

/// What the copy can keep of the original's own metadata.
#[derive(Debug, Default)]
pub(super) struct Carried {
    pub exif: Option<ExifTree>,
    pub xmp: Option<Vec<u8>>,
    pub icc: Option<Vec<u8>>,
}

/// The metadata of the original at `path`; empty for formats that carry none Mimick reads.
pub(super) fn read(path: &Path) -> Carried {
    let Ok(bytes) = std::fs::read(path) else {
        return Carried::default();
    };
    let mut carried = from_bytes(&bytes);
    // RAW files are developed into sRGB, so a profile in them no longer fits the pixels.
    if crate::media_kinds::is_raw_path(path) {
        carried.icc = None;
    }
    carried
}

fn from_bytes(bytes: &[u8]) -> Carried {
    if bytes.starts_with(&[0xFF, 0xD8]) || bytes.starts_with(b"\x89PNG") || is_webp(bytes) {
        return from_image_crate(bytes);
    }
    if bytes.starts_with(&JXL_CONTAINER) {
        return from_jxl(bytes);
    }
    if bytes.starts_with(b"FUJIFILMCCD-RAW") {
        return from_raf(bytes);
    }
    match bytes.get(4..12) {
        Some(b"ftypcrx ") => return from_cr3(bytes),
        Some(head) if head.starts_with(b"ftyp") => return from_heif(bytes),
        _ => {}
    }
    from_tiff(bytes)
}

fn is_webp(bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")
}

/// JPEG, PNG, and WebP, through the image crate's readers.
fn from_image_crate(bytes: &[u8]) -> Carried {
    let decoder = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_decoder().ok());
    let Some(mut decoder) = decoder else {
        return Carried::default();
    };
    Carried {
        exif: decoder
            .exif_metadata()
            .ok()
            .flatten()
            .and_then(|exif| ExifTree::parse(strip_exif_header(&exif))),
        xmp: decoder.xmp_metadata().ok().flatten(),
        icc: decoder.icc_profile().ok().flatten(),
    }
}

fn strip_exif_header(exif: &[u8]) -> &[u8] {
    exif.strip_prefix(b"Exif\0\0").unwrap_or(exif)
}

/// TIFF and the RAW formats built on it: the file itself is the EXIF block. Olympus and
/// Panasonic put their own number where TIFF's 42 goes, which reading doesn't depend on.
fn from_tiff(bytes: &[u8]) -> Carried {
    let Some(mut tree) = ExifTree::parse(bytes) else {
        return Carried::default();
    };
    let xmp = tree.take(Dir::Ifd0, XMP_PACKET).map(|f| f.value);
    let icc = tree.take(Dir::Ifd0, ICC_PROFILE).map(|f| f.value);
    Carried {
        exif: Some(tree),
        xmp,
        icc,
    }
}

/// HEIC, HEIF, and AVIF: the EXIF and XMP items of the main picture, and its ICC profile.
fn from_heif(bytes: &[u8]) -> Carried {
    let Ok(context) = libheif_rs::HeifContext::read_from_bytes(bytes) else {
        return Carried::default();
    };
    let Ok(handle) = context.primary_image_handle() else {
        return Carried::default();
    };
    let ids = |kind: &[u8; 4]| {
        let mut ids = vec![0; handle.number_of_metadata_blocks(kind).max(0) as usize];
        let found = handle.metadata_block_ids(&mut ids, kind);
        ids.truncate(found);
        ids
    };
    let exif = ids(b"Exif")
        .into_iter()
        .find_map(|id| handle.metadata(id).ok())
        .and_then(|data| offset_tiff(&data).and_then(ExifTree::parse));
    let xmp = ids(b"mime")
        .into_iter()
        .filter(|id| handle.metadata_content_type(*id) == Some("application/rdf+xml"))
        .find_map(|id| handle.metadata(id).ok());
    Carried {
        exif,
        xmp,
        icc: handle.color_profile_raw().map(|profile| profile.data),
    }
}

/// The TIFF block after a big-endian count of bytes to skip, as HEIF and JPEG XL store EXIF.
fn offset_tiff(data: &[u8]) -> Option<&[u8]> {
    let skip = u32::from_be_bytes(data.get(..4)?.try_into().ok()?) as usize;
    data.get(4usize.checked_add(skip)?..)
}

/// Fujifilm RAF: the full-size JPEG preview inside carries the camera's EXIF.
fn from_raf(bytes: &[u8]) -> Carried {
    let at = |i: usize| {
        bytes
            .get(i..i + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let jpeg = at(84).zip(at(88)).and_then(|(start, len)| {
        bytes.get(start as usize..(start as usize).checked_add(len as usize)?)
    });
    jpeg.map_or_else(Carried::default, from_image_crate)
}

/// JPEG XL in its container: the Exif and XML boxes. Brotli-packed boxes aren't read.
fn from_jxl(bytes: &[u8]) -> Carried {
    let mut carried = Carried::default();
    for (kind, payload) in boxes(bytes) {
        match &kind {
            b"Exif" => carried.exif = offset_tiff(payload).and_then(ExifTree::parse),
            b"xml " => carried.xmp = Some(payload.to_vec()),
            _ => {}
        }
    }
    carried
}

/// Canon CR3: IFD0, Exif, and GPS each come as a TIFF block of their own. Canon's maker
/// note is set out for its own block, so it can't move into a JPEG's and is left out.
fn from_cr3(bytes: &[u8]) -> Carried {
    let parts = boxes(bytes)
        .find(|(kind, _)| kind == b"moov")
        .and_then(|(_, moov)| {
            boxes(moov).find(|(kind, body)| kind == b"uuid" && body.starts_with(&CANON_UUID))
        })
        .map(|(_, body)| &body[16..]);
    let Some(parts) = parts else {
        return Carried::default();
    };
    let block = |name: &[u8; 4]| {
        boxes(parts)
            .find(|(kind, _)| kind == name)
            .and_then(|(_, data)| ExifTree::parse(data))
    };
    let Some(mut tree) = block(b"CMT1") else {
        return Carried::default();
    };
    if let Some(exif) = block(b"CMT2") {
        tree.adopt(Dir::Exif, exif);
    }
    if let Some(gps) = block(b"CMT4") {
        tree.adopt(Dir::Gps, gps);
    }
    Carried {
        exif: Some(tree),
        ..Carried::default()
    }
}

/// The boxes of an ISO base media file, as type and contents.
fn boxes(data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let head = data.get(at..at + 8)?;
        let size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let kind = [head[4], head[5], head[6], head[7]];
        let (body_at, end) = match size {
            0 => (at + 8, data.len()),
            1 => {
                let large = data.get(at + 8..at + 16)?;
                let large = u64::from_be_bytes(large.try_into().ok()?) as usize;
                (at + 16, at.checked_add(large)?)
            }
            _ => (at + 8, at.checked_add(size)?),
        };
        let body = data.get(body_at..end)?;
        at = end;
        Some((kind, body))
    })
}

/// The original's XMP with its orientation upright and its pixel size the copy's.
pub(super) fn upright_xmp(xmp: &[u8], (w, h): (u32, u32)) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(xmp) else {
        return xmp.to_vec();
    };
    let (w, h) = (w.to_string(), h.to_string());
    let changes = [
        ("tiff:Orientation", "1"),
        ("exif:PixelXDimension", w.as_str()),
        ("exif:PixelYDimension", h.as_str()),
        ("tiff:ImageWidth", w.as_str()),
        ("tiff:ImageLength", h.as_str()),
    ];
    let mut text = text.to_string();
    for (name, value) in changes {
        text = set_xmp_value(&text, name, value);
    }
    text.into_bytes()
}

/// Every `name="…"` attribute and `<name>…</name>` element in `text`, set to `value`.
fn set_xmp_value(text: &str, name: &str, value: &str) -> String {
    let attribute = replace_between(text, &format!("{name}=\""), "\"", value);
    replace_between(
        &attribute,
        &format!("<{name}>"),
        &format!("</{name}>"),
        value,
    )
}

fn replace_between(text: &str, open: &str, close: &str, value: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(open) {
        let after = start + open.len();
        let Some(len) = rest[after..].find(close) else {
            break;
        };
        out.push_str(&rest[..after]);
        out.push_str(value);
        rest = &rest[after + len..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xmp_orientation_and_size_follow_the_copy() {
        let xmp = br#"<rdf:Description tiff:Orientation="6" exif:PixelXDimension="4000">
<tiff:ImageWidth>4000</tiff:ImageWidth><xmp:Rating>4</xmp:Rating></rdf:Description>"#;
        let text = String::from_utf8(upright_xmp(xmp, (1200, 900))).unwrap();
        assert!(text.contains(r#"tiff:Orientation="1""#));
        assert!(text.contains(r#"exif:PixelXDimension="1200""#));
        assert!(text.contains("<tiff:ImageWidth>1200</tiff:ImageWidth>"));
        assert!(text.contains("<xmp:Rating>4</xmp:Rating>"));
    }

    #[test]
    fn media_boxes_are_walked_in_order() {
        let mut data = Vec::new();
        for (kind, body) in [(b"Exif", &b"\0\0\0\0II"[..]), (b"xml ", &b"<x/>"[..])] {
            data.extend((8 + body.len() as u32).to_be_bytes());
            data.extend(kind);
            data.extend(body);
        }
        let found: Vec<[u8; 4]> = boxes(&data).map(|(kind, _)| kind).collect();
        assert_eq!(found, [*b"Exif", *b"xml "]);
        assert_eq!(offset_tiff(b"\0\0\0\x02xxII*\0"), Some(&b"II*\0"[..]));
    }
}
