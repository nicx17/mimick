//! Tone for developed RAWs: the look of the picture the grid shows, matched channel by channel.
//!
//! LibRaw hands over linear light, which looks flat and washed out on screen. Mapping each
//! channel's distribution onto a reference picture's (Immich's preview of a synced photo, or
//! the camera's embedded JPEG) brings back its contrast, black level, white balance, and
//! colour. A RAW with no reference gets a gentle S-curve instead.

use rayon::prelude::*;
use std::f64::consts::TAU;

/// One bin per 16-bit value.
const BINS: usize = 1 << 16;
/// Steps along each matched curve: enough to follow a camera's toe and shoulder.
const STEPS: usize = 256;
const LEVELS: usize = STEPS + 1;
/// Pixels sampled for a histogram; more only costs time.
const SAMPLES: usize = 250_000;
/// A reference smaller than 64×64 is too coarse to match.
const MIN_REFERENCE_PIXELS: usize = 64 * 64;
/// Fallback exposure: the brightest channel's 99.7th percentile becomes white.
const FALLBACK_WHITE_QUANTILE: f64 = 0.997;
/// Fallback S-curve strength in display space; 0 is the plain sRGB curve.
const FALLBACK_CONTRAST: f64 = 0.3;
/// Linear sRGB to linear Display P3: LibRaw's own matrix for its Display P3 output.
const SRGB_TO_P3: [[f64; 3]; 3] = [
    [0.822_488, 0.177_511, 0.0],
    [0.033_2, 0.966_8, 0.0],
    [0.017_089, 0.072_411, 0.910_499],
];
/// Pixels handed to each parallel task when toning.
const PIXELS_PER_TASK: usize = 1 << 15;

/// How often each 16-bit value occurs.
struct Histogram {
    bins: Vec<u32>,
    total: u64,
}

impl Histogram {
    fn new() -> Self {
        Self {
            bins: vec![0; BINS],
            total: 0,
        }
    }

    fn add(&mut self, value: u16) {
        self.bins[usize::from(value)] += 1;
        self.total += 1;
    }

    /// The value at each of `levels` (ascending, 0 to 1), by nearest rank.
    fn quantiles(&self, levels: &[f64]) -> Vec<u16> {
        let last_rank = self.total.saturating_sub(1) as f64;
        let (mut bin, mut below) = (0, 0u64);
        levels
            .iter()
            .map(|level| {
                let rank = (level * last_rank).round() as u64;
                while bin < BINS - 1 && below + u64::from(self.bins[bin]) <= rank {
                    below += u64::from(self.bins[bin]);
                    bin += 1;
                }
                bin as u16
            })
            .collect()
    }

    /// The values at each step along a curve, 0 to 1.
    fn steps(&self) -> [u16; LEVELS] {
        let levels: Vec<f64> = (0..LEVELS).map(|i| i as f64 / STEPS as f64).collect();
        let values = self.quantiles(&levels);
        std::array::from_fn(|i| values[i])
    }
}

/// Which encoding a reference picture's 8-bit pixels are in.
#[derive(Clone, Copy)]
pub(super) enum Encoding {
    Srgb,
    DisplayP3,
}

/// A reference picture's 8-bit pixels: RGB first in each pixel, rows `stride` bytes apart.
pub(super) struct Pixels<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) stride: usize,
    pub(super) bytes_per_pixel: usize,
    pub(super) encoding: Encoding,
}

/// A picture's look: where its linear Display P3 channels' values fall, step by step.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Reference([[u16; LEVELS]; 3]);

impl Reference {
    /// `None` when the picture is too small to match, or shorter than its layout says.
    pub(super) fn sample(pixels: &Pixels<'_>) -> Option<Self> {
        let needed =
            pixels.stride * pixels.height.saturating_sub(1) + pixels.width * pixels.bytes_per_pixel;
        if pixels.width * pixels.height < MIN_REFERENCE_PIXELS || pixels.bytes.len() < needed {
            return None;
        }
        let linear: [f64; 256] = std::array::from_fn(|v| srgb_decode(v as f64 / 255.0));
        let mut histograms = [Histogram::new(), Histogram::new(), Histogram::new()];
        for rgb in sampled_pixels(pixels) {
            let rgb = rgb.map(|v| linear[usize::from(v)]);
            let p3 = match pixels.encoding {
                Encoding::DisplayP3 => rgb,
                Encoding::Srgb => {
                    SRGB_TO_P3.map(|row| row.iter().zip(rgb).map(|(m, v)| m * v).sum())
                }
            };
            for (histogram, value) in histograms.iter_mut().zip(p3) {
                histogram.add((value.clamp(0.0, 1.0) * 65535.0).round() as u16);
            }
        }
        Some(Self(histograms.map(|histogram| histogram.steps())))
    }

    /// Bytes that change whenever the look does, for keying cached decodes.
    pub(super) fn fingerprint(&self) -> Vec<u8> {
        self.0
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect()
    }
}

/// Every pixel up to `SAMPLES`, else an even grid of them.
fn sampled_pixels<'a>(pixels: &'a Pixels<'a>) -> impl Iterator<Item = [u8; 3]> + 'a {
    let step = ((pixels.width * pixels.height) as f64 / SAMPLES as f64)
        .sqrt()
        .ceil()
        .max(1.0) as usize;
    (0..pixels.height).step_by(step).flat_map(move |y| {
        (0..pixels.width).step_by(step).map(move |x| {
            let at = y * pixels.stride + x * pixels.bytes_per_pixel;
            [pixels.bytes[at], pixels.bytes[at + 1], pixels.bytes[at + 2]]
        })
    })
}

/// Each channel's histogram of a decode.
pub(super) struct DecodeStats([Histogram; 3]);

/// Samples LibRaw's 16-bit linear Display P3 RGB (native byte order).
pub(super) fn sample_decode(rgb16: &[u8]) -> DecodeStats {
    let (pixels, _) = rgb16.as_chunks::<6>();
    let mut histograms = [Histogram::new(), Histogram::new(), Histogram::new()];
    for pixel in pixels.iter().step_by((pixels.len() / SAMPLES).max(1)) {
        for (histogram, sample) in histograms.iter_mut().zip(pixel.as_chunks::<2>().0) {
            histogram.add(u16::from_ne_bytes(*sample));
        }
    }
    DecodeStats(histograms)
}

/// 8-bit Display P3 for every 16-bit linear value of one channel.
#[derive(Clone)]
struct Lut(Box<[u8; BINS]>);

impl Lut {
    /// `encoded` gives each value's display-encoded output, 0 to 1; called in ascending order.
    fn from_encoded(mut encoded: impl FnMut(u16) -> f64) -> Self {
        Self(Box::new(std::array::from_fn(|value| {
            (encoded(value as u16).clamp(0.0, 1.0) * 255.0).round() as u8
        })))
    }
}

/// One table per channel.
pub(super) struct Tone([Lut; 3]);

/// The reference's look when every channel can be matched, else the fallback S-curve.
pub(super) fn tone(decoded: &DecodeStats, reference: Option<&Reference>) -> Tone {
    let matched = reference.and_then(|reference| {
        let [r, g, b] = [0, 1, 2].map(|c| matched_lut(&decoded.0[c], &reference.0[c]));
        Some(Tone([r?, g?, b?]))
    });
    matched.unwrap_or_else(|| {
        log::debug!("RAW tone: nothing to match, using the fallback curve");
        let lut = fallback_lut(&decoded.0);
        Tone([lut.clone(), lut.clone(), lut])
    })
}

/// LibRaw's 16-bit samples (native byte order) to 8-bit through `tone`, in parallel.
pub(super) fn apply(tone: &Tone, rgb16: &[u8]) -> Vec<u8> {
    let mut out = vec![0; rgb16.len() / 2];
    out.par_chunks_mut(PIXELS_PER_TASK * 3)
        .zip(rgb16.par_chunks(PIXELS_PER_TASK * 6))
        .for_each(|(chunk, samples)| {
            let pixels = chunk.as_chunks_mut::<3>().0.iter_mut();
            for (pixel, sample) in pixels.zip(samples.as_chunks::<6>().0) {
                let channels = pixel.iter_mut().zip(&tone.0);
                for ((dst, lut), value) in channels.zip(sample.as_chunks::<2>().0) {
                    *dst = lut.0[usize::from(u16::from_ne_bytes(*value))];
                }
            }
        });
    out
}

/// The curve that gives a decoded channel the reference channel's distribution.
fn matched_lut(decoded: &Histogram, reference: &[u16; LEVELS]) -> Option<Lut> {
    let points = curve_points(&decoded.steps(), reference);
    if points.len() < 2 {
        return None;
    }
    let mut segment = 0;
    Some(Lut::from_encoded(|value| {
        let x = f64::from(value);
        while segment + 2 < points.len() && x > points[segment + 1].0 {
            segment += 1;
        }
        srgb_encode(interpolate(points[segment], points[segment + 1], x) / 65535.0)
    }))
}

/// Step pairs with strictly rising inputs; outputs never fall.
fn curve_points(xs: &[u16], ys: &[u16]) -> Vec<(f64, f64)> {
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(xs.len());
    let mut top = 0;
    for (&x, &y) in xs.iter().zip(ys) {
        top = top.max(y);
        if points.last().is_none_or(|&(last, _)| f64::from(x) > last) {
            points.push((f64::from(x), f64::from(top)));
        }
    }
    points
}

/// Straight line between two points, held flat beyond them.
fn interpolate((x0, y0): (f64, f64), (x1, y1): (f64, f64), x: f64) -> f64 {
    if x <= x0 {
        y0
    } else if x >= x1 {
        y1
    } else {
        y0 + (y1 - y0) * (x - x0) / (x1 - x0)
    }
}

/// Auto exposure like LibRaw's, gentler on highlights, then an S-curve for a toe and contrast.
fn fallback_lut(channels: &[Histogram; 3]) -> Lut {
    let white = channels
        .iter()
        .map(|histogram| histogram.quantiles(&[FALLBACK_WHITE_QUANTILE])[0])
        .max()
        .unwrap_or(u16::MAX)
        .max(1);
    Lut::from_encoded(|value| {
        let v = srgb_encode((f64::from(value) / f64::from(white)).min(1.0));
        v - FALLBACK_CONTRAST * (TAU * v).sin() / TAU
    })
}

fn srgb_encode(linear: f64) -> f64 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_decode(encoded: f64) -> f64 {
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn histogram(values: impl IntoIterator<Item = u16>) -> Histogram {
        let mut histogram = Histogram::new();
        values.into_iter().for_each(|v| histogram.add(v));
        histogram
    }

    fn plain(value: u16) -> u8 {
        (srgb_encode(f64::from(value) / 65535.0) * 255.0).round() as u8
    }

    fn rgb16(pixels: &[[u16; 3]]) -> Vec<u8> {
        pixels
            .iter()
            .flatten()
            .flat_map(|v| v.to_ne_bytes())
            .collect()
    }

    fn solid(rgb: [u8; 3], side: usize, encoding: Encoding) -> Option<Reference> {
        let bytes: Vec<u8> = rgb.repeat(side * side);
        Reference::sample(&Pixels {
            bytes: &bytes,
            width: side,
            height: side,
            stride: side * 3,
            bytes_per_pixel: 3,
            encoding,
        })
    }

    #[test]
    fn quantiles_walk_the_sorted_values() {
        let h = histogram([5, 1, 9, 3, 7]);
        assert_eq!(h.quantiles(&[0.0, 0.25, 0.5, 1.0]), [1, 3, 5, 9]);
    }

    #[test]
    fn matching_a_channel_with_the_same_tones_keeps_the_plain_curve() {
        let same = || histogram((0..=60_000).step_by(7));
        let lut = matched_lut(&same(), &same().steps()).unwrap();
        for value in [500, 9_000, 30_000, 59_000] {
            assert!(lut.0[value].abs_diff(plain(value as u16)) <= 1, "{value}");
        }
    }

    #[test]
    fn matching_takes_on_the_reference_brightness() {
        let decoded = histogram((0..=10_000).step_by(3));
        let brighter = histogram((0..=40_000).step_by(12));
        let lut = matched_lut(&decoded, &brighter.steps()).unwrap();
        assert!(lut.0[5_000].abs_diff(plain(20_000)) <= 1);
        assert!(
            lut.0[20_000].abs_diff(plain(40_000)) <= 1,
            "beyond the decode's range holds the top"
        );
    }

    #[test]
    fn a_flat_channel_cannot_be_matched() {
        let flat = histogram([1_000; 50]);
        assert!(matched_lut(&flat, &histogram(0..5_000).steps()).is_none());
    }

    #[test]
    fn references_are_read_in_display_p3() {
        let p3_red = solid([255, 0, 0], 64, Encoding::DisplayP3).unwrap();
        assert_eq!((p3_red.0[0][STEPS], p3_red.0[1][STEPS]), (65535, 0));
        // sRGB's red sits inside Display P3: less red, a little green and blue.
        let srgb_red = solid([255, 0, 0], 64, Encoding::Srgb).unwrap();
        assert_eq!(srgb_red.0[0][STEPS], 53_902);
        assert_eq!(srgb_red.0[1][STEPS], 2_176);
        assert_ne!(p3_red.fingerprint(), srgb_red.fingerprint());
    }

    #[test]
    fn tiny_or_short_references_are_not_used() {
        assert!(solid([128; 3], 63, Encoding::Srgb).is_none());
        let short = Pixels {
            bytes: &[0; 10],
            width: 64,
            height: 64,
            stride: 192,
            bytes_per_pixel: 3,
            encoding: Encoding::Srgb,
        };
        assert!(Reference::sample(&short).is_none());
    }

    #[test]
    fn fallback_curve_rises_to_white_with_darker_shadows() {
        let channel = || histogram((0..=20_000).step_by(2));
        let lut = fallback_lut(&[channel(), channel(), channel()]);
        assert_eq!(lut.0[0], 0);
        assert_eq!(lut.0[20_000], 255);
        assert!(lut.0.windows(2).all(|pair| pair[0] <= pair[1]));
        let plain_exposed = (srgb_encode(1_000.0 / 19_940.0) * 255.0).round() as u8;
        assert!(lut.0[1_000] < plain_exposed, "the toe deepens shadows");
    }

    #[test]
    fn no_reference_falls_back_on_every_channel() {
        let stats = sample_decode(&rgb16(&[[2_000, 4_000, 6_000]; 100]));
        let tone = tone(&stats, None);
        assert!(tone.0.iter().all(|lut| lut.0[6_000] == 255));
    }

    #[test]
    fn apply_uses_each_channel_s_own_table() {
        let tone = Tone([
            Lut::from_encoded(|value| f64::from(value) / 65535.0),
            Lut::from_encoded(|_| 0.0),
            Lut::from_encoded(|_| 1.0),
        ]);
        let out = apply(&tone, &rgb16(&[[32_768, 9, 9], [65_535, 9, 9]]));
        assert_eq!(out, [128, 0, 255, 255, 0, 255]);
    }
}
