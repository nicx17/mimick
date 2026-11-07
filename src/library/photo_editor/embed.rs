//! Putting metadata into the copy's file: JPEG segments, and XMP in a PNG.

/// What goes into the copy: an EXIF block (TIFF bytes), XMP, and a colour profile.
#[derive(Debug, Default)]
pub(super) struct Metadata {
    pub exif: Option<Vec<u8>>,
    pub xmp: Option<Vec<u8>>,
    pub icc: Option<Vec<u8>>,
}

/// Room for the TIFF bytes in a JPEG EXIF segment, after its length and "Exif\0\0".
pub(super) const JPEG_EXIF_ROOM: usize = 65535 - 2 - 6;
const XMP_ID: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const ICC_ID: &[u8] = b"ICC_PROFILE\0";
/// Bytes of profile per ICC segment, after its id, number, and count.
const ICC_CHUNK: usize = 65535 - 2 - 12 - 2;

/// `jpeg` with the metadata as segments after its start marker and any JFIF segment.
/// XMP too long for one segment is left out rather than split.
pub(super) fn jpeg_with(jpeg: &[u8], meta: &Metadata) -> Vec<u8> {
    let mut segments = Vec::new();
    if let Some(exif) = &meta.exif {
        segments.push((0xE1, [&b"Exif\0\0"[..], exif].concat()));
    }
    if let Some(xmp) = &meta.xmp {
        segments.push((0xE1, [XMP_ID, xmp].concat()));
    }
    if let Some(icc) = &meta.icc {
        let chunks: Vec<&[u8]> = icc.chunks(ICC_CHUNK).collect();
        let total = chunks.len().min(255) as u8;
        for (index, chunk) in chunks.iter().take(255).enumerate() {
            let head = [ICC_ID, &[index as u8 + 1, total]].concat();
            segments.push((0xE2, [&head[..], chunk].concat()));
        }
    }
    let at = after_jfif(jpeg);
    let mut out =
        Vec::with_capacity(jpeg.len() + segments.iter().map(|s| s.1.len() + 4).sum::<usize>());
    out.extend(&jpeg[..at]);
    for (marker, body) in segments {
        if body.len() + 2 <= usize::from(u16::MAX) {
            out.extend([0xFF, marker]);
            out.extend(((body.len() + 2) as u16).to_be_bytes());
            out.extend(body);
        }
    }
    out.extend(&jpeg[at..]);
    out
}

/// Where segments go: after the start marker, and after a JFIF segment straight after it.
fn after_jfif(jpeg: &[u8]) -> usize {
    if jpeg.get(2..4) != Some(&[0xFF, 0xE0]) {
        return 2;
    }
    let len = jpeg
        .get(4..6)
        .map_or(0, |b| usize::from(u16::from_be_bytes([b[0], b[1]])));
    (4 + len).min(jpeg.len())
}

/// `png` with `xmp` in an iTXt chunk right after its header chunk.
pub(super) fn png_with_xmp(png: &[u8], xmp: &[u8]) -> Vec<u8> {
    // Signature (8) and IHDR: length (4), type (4), 13 bytes of data, CRC (4).
    const AFTER_IHDR: usize = 8 + 4 + 4 + 13 + 4;
    if png.len() < AFTER_IHDR {
        return png.to_vec();
    }
    let mut chunk = b"iTXt".to_vec();
    chunk.extend(b"XML:com.adobe.xmp\0\0\0\0\0");
    chunk.extend(xmp);
    let mut out = png[..AFTER_IHDR].to_vec();
    out.extend(((chunk.len() - 4) as u32).to_be_bytes());
    out.extend(&chunk);
    out.extend(crc32(&chunk).to_be_bytes());
    out.extend(&png[AFTER_IHDR..]);
    out
}

/// The CRC-32 PNG checks its chunks with.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn segments_go_after_jfif_and_long_profiles_split() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 1, 2, 0xFF, 0xD9];
        let meta = Metadata {
            exif: Some(b"II*\0".to_vec()),
            xmp: None,
            icc: Some(vec![7; ICC_CHUNK + 1]),
        };
        let out = jpeg_with(&jpeg, &meta);
        assert_eq!(&out[..8], &jpeg[..8]);
        assert_eq!(&out[8..10], &[0xFF, 0xE1]);
        let icc_segments = out.windows(ICC_ID.len()).filter(|w| *w == ICC_ID).count();
        assert_eq!(icc_segments, 2);
        assert!(out.ends_with(&[0xFF, 0xD9]));
    }
}
