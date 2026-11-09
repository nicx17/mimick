//! Colour management. A photo's ICC profile says what its pixel values mean; GTK shows them
//! right on any screen once a texture is tagged with that colour space. Display P3 (iPhones,
//! Immich's previews) is only tagged; profiles GTK can't name, like Adobe RGB, are converted.

use gtk::gdk;
use lcms2::{
    CIExyY, CIExyYTRIPLE, ColorSpaceSignature, Intent, PixelFormat, Profile, Tag, TagSignature,
    ToneCurve, Transform,
};

/// sRGB's tone curve, which Display P3 shares, as ICC parametric curve type 4.
const SRGB_CURVE: [f64; 5] = [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045];
/// How far a profile's primaries may stray from a standard's and still count as it.
const PRIMARIES_TOLERANCE: f64 = 0.002;
/// H.273 code points, as GTK's `GdkCicpParams` takes them.
const CICP_SRGB: (u32, u32) = (1, 13);
const CICP_DISPLAY_P3: (u32, u32) = (12, 13);

/// How 8-bit pixels are laid out; only RGB and RGBA can be converted.
pub(super) struct Layout {
    pub width: i32,
    pub height: i32,
    pub stride: usize,
    pub format: gdk::MemoryFormat,
}

enum Space {
    Srgb,
    DisplayP3,
    Other(Profile),
}

/// A gdk-pixbuf's layout: 8-bit RGB, with alpha when it has some.
pub(super) fn pixbuf_layout(pixbuf: &gtk::gdk_pixbuf::Pixbuf) -> Layout {
    Layout {
        width: pixbuf.width(),
        height: pixbuf.height(),
        stride: usize::try_from(pixbuf.rowstride()).unwrap_or_default(),
        format: if pixbuf.has_alpha() {
            gdk::MemoryFormat::R8g8b8a8
        } else {
            gdk::MemoryFormat::R8g8b8
        },
    }
}

/// The ICC profile of an encoded JPEG, PNG, WebP, or TIFF, read from its headers.
pub(super) fn icc_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    use image::ImageDecoder;
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    reader.into_decoder().ok()?.icc_profile().ok().flatten()
}

/// Like [`icc_profile`], reading only as much of the file as its headers take.
pub(super) fn file_icc_profile(path: &std::path::Path) -> Option<Vec<u8>> {
    use image::ImageDecoder;
    let reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    reader.into_decoder().ok()?.icc_profile().ok().flatten()
}

/// A texture of `bytes` that GTK shows in the colour space `icc` describes; without a
/// profile, or with one that can't be read, the pixels are taken as sRGB.
pub(super) fn texture(bytes: glib::Bytes, layout: &Layout, icc: Option<&[u8]>) -> gdk::Texture {
    let profile = match icc.and_then(classify) {
        None | Some(Space::Srgb) => return tagged(&bytes, layout, None),
        Some(Space::DisplayP3) => return tagged(&bytes, layout, display_p3().as_ref()),
        Some(Space::Other(profile)) => profile,
    };
    let mut pixels = bytes.to_vec();
    match display_p3_profile() {
        Some(p3) if convert(&mut pixels, layout, &profile, &p3) => tagged(
            &glib::Bytes::from_owned(pixels),
            layout,
            display_p3().as_ref(),
        ),
        _ => tagged(&bytes, layout, None),
    }
}

/// Converts pixels in the colour space `icc` describes to sRGB in place, for keeping
/// somewhere that can't record a colour space, such as the thumbnail cache's JPEGs.
pub(super) fn convert_to_srgb(pixels: &mut [u8], layout: &Layout, icc: &[u8]) -> bool {
    let from = match classify(icc) {
        Some(Space::Other(profile)) => profile,
        Some(Space::DisplayP3) => match display_p3_profile() {
            Some(p3) => p3,
            None => return false,
        },
        Some(Space::Srgb) | None => return false,
    };
    convert(pixels, layout, &from, &Profile::new_srgb())
}

/// A texture tagged with `state`, or sRGB when `None`.
pub(super) fn tagged(
    bytes: &glib::Bytes,
    layout: &Layout,
    state: Option<&gdk::ColorState>,
) -> gdk::Texture {
    let builder = gdk::MemoryTextureBuilder::new()
        .set_bytes(Some(bytes))
        .set_width(layout.width)
        .set_height(layout.height)
        .set_stride(layout.stride)
        .set_format(layout.format);
    match state {
        Some(state) => builder.set_color_state(state).build(),
        None => builder.build(),
    }
}

/// A colour state from H.273 code points, such as a HEIC's; `None` for sRGB, which is
/// GTK's default, and for any GTK doesn't support.
pub(super) fn cicp_state(primaries: u32, transfer: u32) -> Option<gdk::ColorState> {
    if (primaries, transfer) == CICP_SRGB {
        return None;
    }
    let params = gdk::CicpParams::new();
    params.set_color_primaries(primaries);
    params.set_transfer_function(transfer);
    params.set_matrix_coefficients(0);
    params.set_range(gdk::CicpRange::Full);
    params.build_color_state().ok()
}

pub(super) fn display_p3() -> Option<gdk::ColorState> {
    cicp_state(CICP_DISPLAY_P3.0, CICP_DISPLAY_P3.1)
}

/// The profile to embed in a picture whose pixels were read out in Display P3.
pub(super) fn display_p3_icc() -> Option<Vec<u8>> {
    display_p3_profile()?.icc().ok()
}

fn display_p3_profile() -> Option<Profile> {
    let xy = |x, y| CIExyY { x, y, Y: 1.0 };
    let primaries = CIExyYTRIPLE {
        Red: xy(0.680, 0.320),
        Green: xy(0.265, 0.690),
        Blue: xy(0.150, 0.060),
    };
    let curve = ToneCurve::new_parametric(4, &SRGB_CURVE).ok()?;
    Profile::new_rgb(&xy(0.3127, 0.3290), &primaries, &[&curve, &curve, &curve]).ok()
}

/// sRGB and Display P3 by their primaries, which is how their many profiles differ in name only.
fn classify(icc: &[u8]) -> Option<Space> {
    let profile = Profile::new_icc(icc).ok()?;
    if profile.color_space() != ColorSpaceSignature::RgbData {
        return None;
    }
    let primaries = primaries_of(&profile);
    let matches = |reference: Option<Profile>| {
        let wanted = reference.as_ref().and_then(primaries_of);
        primaries
            .zip(wanted)
            .is_some_and(|(a, b)| same_primaries(&a, &b))
    };
    if matches(Some(Profile::new_srgb())) {
        Some(Space::Srgb)
    } else if matches(display_p3_profile()) {
        Some(Space::DisplayP3)
    } else {
        Some(Space::Other(profile))
    }
}

/// The red, green, and blue colorants of a matrix profile; `None` for LUT-based ones.
fn primaries_of(profile: &Profile) -> Option<[f64; 9]> {
    let read = |sig| match profile.read_tag(sig) {
        Tag::CIEXYZ(xyz) => Some([xyz.X, xyz.Y, xyz.Z]),
        _ => None,
    };
    let red = read(TagSignature::RedColorantTag)?;
    let green = read(TagSignature::GreenColorantTag)?;
    let blue = read(TagSignature::BlueColorantTag)?;
    Some([
        red[0], red[1], red[2], green[0], green[1], green[2], blue[0], blue[1], blue[2],
    ])
}

fn same_primaries(a: &[f64; 9], b: &[f64; 9]) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (x - y).abs() < PRIMARIES_TOLERANCE)
}

/// Converts in place, row by row so padding between rows is left alone; false when the
/// format isn't 8-bit RGB or RGBA, or Little CMS can't link the two profiles.
fn convert(pixels: &mut [u8], layout: &Layout, from: &Profile, to: &Profile) -> bool {
    let Ok(width) = usize::try_from(layout.width) else {
        return false;
    };
    let rows = Rows {
        stride: layout.stride,
        width,
    };
    match layout.format {
        gdk::MemoryFormat::R8g8b8 => convert_rows::<3>(pixels, rows, from, to, PixelFormat::RGB_8),
        gdk::MemoryFormat::R8g8b8a8 => {
            convert_rows::<4>(pixels, rows, from, to, PixelFormat::RGBA_8)
        }
        _ => false,
    }
}

#[derive(Clone, Copy)]
struct Rows {
    stride: usize,
    width: usize,
}

/// Alpha, the fourth byte of RGBA, stays as it is.
fn convert_rows<const N: usize>(
    pixels: &mut [u8],
    rows: Rows,
    from: &Profile,
    to: &Profile,
    format: PixelFormat,
) -> bool
where
    [u8; N]: lcms2::Pod,
{
    let Ok(transform) =
        Transform::<[u8; N], [u8; N]>::new(from, format, to, format, Intent::RelativeColorimetric)
    else {
        return false;
    };
    let row_bytes = rows.width * N;
    for row in pixels.chunks_mut(rows.stride.max(row_bytes)) {
        if let Some(row) = row.get_mut(..row_bytes) {
            transform.transform_in_place(row.as_chunks_mut::<N>().0);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adobe_rgb() -> Profile {
        let xy = |x, y| CIExyY { x, y, Y: 1.0 };
        let primaries = CIExyYTRIPLE {
            Red: xy(0.64, 0.33),
            Green: xy(0.21, 0.71),
            Blue: xy(0.15, 0.06),
        };
        let curve = ToneCurve::new(2.199_218_75);
        Profile::new_rgb(&xy(0.3127, 0.3290), &primaries, &[&curve, &curve, &curve]).unwrap()
    }

    #[test]
    fn profiles_are_told_apart_by_their_primaries() {
        let srgb = Profile::new_srgb().icc().unwrap();
        let p3 = display_p3_icc().unwrap();
        let adobe = adobe_rgb().icc().unwrap();
        assert!(matches!(classify(&srgb), Some(Space::Srgb)));
        assert!(matches!(classify(&p3), Some(Space::DisplayP3)));
        assert!(matches!(classify(&adobe), Some(Space::Other(_))));
        assert!(classify(b"not a profile").is_none());
    }

    #[test]
    fn adobe_rgb_green_becomes_a_less_extreme_p3_green() {
        let layout = Layout {
            width: 1,
            height: 1,
            stride: 3,
            format: gdk::MemoryFormat::R8g8b8,
        };
        let p3 = display_p3_profile().unwrap();
        let mut out = [0, 255, 0];
        assert!(convert(&mut out, &layout, &adobe_rgb(), &p3));
        // Adobe RGB's green lies outside P3, so it lands on P3's green edge.
        assert!(out[1] > 240 && out[0] < 60 && out[2] < 60, "{out:?}");
    }

    #[test]
    fn conversion_skips_the_padding_between_rows() {
        let layout = Layout {
            width: 1,
            height: 2,
            stride: 4,
            format: gdk::MemoryFormat::R8g8b8,
        };
        let mut pixels = [10, 20, 30, 99, 40, 50, 60];
        assert!(convert(
            &mut pixels,
            &layout,
            &adobe_rgb(),
            &Profile::new_srgb()
        ));
        assert_eq!(pixels[3], 99);
    }
}
