//! Tone for developed RAWs: the camera's own look, read off its embedded preview.
//!
//! LibRaw hands over linear light, which looks flat and washed out on screen. Mapping its
//! brightness onto the embedded JPEG's brings back the camera's contrast, black level, and
//! colour; a RAW without a usable preview gets a gentle S-curve instead.

use rayon::prelude::*;
use std::f64::consts::TAU;

/// One bin per 16-bit value.
const BINS: usize = 1 << 16;
/// Points along the matched curve: enough to follow a camera's toe and shoulder.
const STEPS: usize = 256;
/// Pixels sampled for a histogram; more only costs time.
const SAMPLES: usize = 250_000;
/// A preview smaller than 64×64 is too coarse to match.
const MIN_REFERENCE_PIXELS: u64 = 64 * 64;
/// Fallback exposure: the brightest channel's 99.7th percentile becomes white.
const FALLBACK_WHITE_QUANTILE: f64 = 0.997;
/// Fallback S-curve strength in display space; 0 is the plain sRGB curve.
const FALLBACK_CONTRAST: f64 = 0.3;
/// Luminance weights summing to 65536: linear Display P3 (LibRaw's output), and sRGB previews.
const P3_LUMA: [u64; 3] = [15009, 45330, 5197];
const SRGB_LUMA: [u64; 3] = [13933, 46871, 4732];
/// Samples handed to each parallel task when applying a curve.
const CHUNK: usize = 1 << 16;

/// How often each 16-bit value occurs.
pub(super) struct Histogram {
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
}

/// A decode's histograms: luminance for matching, each channel's for the fallback exposure.
pub(super) struct DecodeStats {
    luminance: Histogram,
    channels: [Histogram; 3],
}

/// 8-bit Display P3 for every 16-bit linear value.
pub(super) struct Lut(Box<[u8; BINS]>);

impl Lut {
    /// `encoded` gives each value's display-encoded output, 0 to 1; called in ascending order.
    fn from_encoded(mut encoded: impl FnMut(u16) -> f64) -> Self {
        Self(Box::new(std::array::from_fn(|value| {
            (encoded(value as u16).clamp(0.0, 1.0) * 255.0).round() as u8
        })))
    }
}

/// Samples LibRaw's 16-bit linear Display P3 RGB (native byte order).
pub(super) fn sample_decode(rgb16: &[u8]) -> DecodeStats {
    let (pixels, _) = rgb16.as_chunks::<6>();
    let mut stats = DecodeStats {
        luminance: Histogram::new(),
        channels: [Histogram::new(), Histogram::new(), Histogram::new()],
    };
    for pixel in pixels.iter().step_by(sample_step(pixels.len())) {
        let rgb = [0, 2, 4].map(|at| u16::from_ne_bytes([pixel[at], pixel[at + 1]]));
        stats.luminance.add(luma(rgb, P3_LUMA));
        for (histogram, value) in stats.channels.iter_mut().zip(rgb) {
            histogram.add(value);
        }
    }
    stats
}

/// Samples an 8-bit sRGB preview's luminance; `None` when it's too small to match.
pub(super) fn sample_reference(rgb8: &[u8]) -> Option<Histogram> {
    let linear: [u16; 256] =
        std::array::from_fn(|v| (srgb_decode(v as f64 / 255.0) * 65535.0).round() as u16);
    let (pixels, _) = rgb8.as_chunks::<3>();
    let mut histogram = Histogram::new();
    for pixel in pixels.iter().step_by(sample_step(pixels.len())) {
        histogram.add(luma(pixel.map(|v| linear[usize::from(v)]), SRGB_LUMA));
    }
    let enough = pixels.len() as u64 >= MIN_REFERENCE_PIXELS;
    enough.then_some(histogram)
}

/// The camera's look when its preview can show it, else the fallback S-curve.
pub(super) fn tone_lut(decoded: &DecodeStats, reference: Option<&Histogram>) -> Lut {
    if let Some(lut) = reference.and_then(|reference| matched_lut(&decoded.luminance, reference)) {
        return lut;
    }
    log::debug!("RAW tone: no usable preview, using the fallback curve");
    fallback_lut(&decoded.channels)
}

/// LibRaw's 16-bit samples (native byte order) to 8-bit through `lut`, in parallel.
pub(super) fn apply(lut: &Lut, rgb16: &[u8]) -> Vec<u8> {
    let mut out = vec![0; rgb16.len() / 2];
    out.par_chunks_mut(CHUNK)
        .zip(rgb16.par_chunks(CHUNK * 2))
        .for_each(|(chunk, samples)| {
            for (dst, sample) in chunk.iter_mut().zip(samples.as_chunks::<2>().0) {
                *dst = lut.0[usize::from(u16::from_ne_bytes(*sample))];
            }
        });
    out
}

/// The curve that gives the decode the preview's distribution of brightness; applied to
/// each channel alike, as camera engines do, it restores their saturation too.
fn matched_lut(decoded: &Histogram, reference: &Histogram) -> Option<Lut> {
    let levels: Vec<f64> = (0..=STEPS).map(|step| step as f64 / STEPS as f64).collect();
    let points = curve_points(&decoded.quantiles(&levels), &reference.quantiles(&levels));
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

/// Quantile pairs with strictly rising inputs; outputs never fall.
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

fn luma(rgb: [u16; 3], weights: [u64; 3]) -> u16 {
    let sum: u64 = rgb
        .iter()
        .zip(weights)
        .map(|(&v, w)| u64::from(v) * w)
        .sum();
    (sum >> 16) as u16
}

/// Every pixel up to `SAMPLES`, else an even spread of them.
fn sample_step(pixels: usize) -> usize {
    (pixels / SAMPLES).max(1)
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

    #[test]
    fn quantiles_walk_the_sorted_values() {
        let h = histogram([5, 1, 9, 3, 7]);
        assert_eq!(h.quantiles(&[0.0, 0.25, 0.5, 1.0]), [1, 3, 5, 9]);
    }

    #[test]
    fn white_stays_white_in_both_luminances() {
        assert_eq!(luma([65535; 3], P3_LUMA), 65535);
        assert_eq!(luma([65535; 3], SRGB_LUMA), 65535);
        assert_eq!(luma([0; 3], P3_LUMA), 0);
    }

    #[test]
    fn matching_a_preview_with_the_same_tones_keeps_the_plain_curve() {
        let same = || histogram((0..=60_000).step_by(7));
        let lut = matched_lut(&same(), &same()).unwrap();
        for value in [500, 9_000, 30_000, 59_000] {
            assert!(lut.0[value].abs_diff(plain(value as u16)) <= 1, "{value}");
        }
    }

    #[test]
    fn matching_takes_on_the_preview_brightness() {
        let decoded = histogram((0..=10_000).step_by(3));
        let brighter = histogram((0..=40_000).step_by(12));
        let lut = matched_lut(&decoded, &brighter).unwrap();
        assert!(lut.0[5_000].abs_diff(plain(20_000)) <= 1);
        assert!(
            lut.0[20_000].abs_diff(plain(40_000)) <= 1,
            "beyond the decode's range holds the top"
        );
    }

    #[test]
    fn a_flat_decode_cannot_be_matched() {
        let flat = histogram([1_000; 50]);
        assert!(matched_lut(&flat, &histogram(0..5_000)).is_none());
    }

    #[test]
    fn tiny_previews_are_not_used() {
        assert!(sample_reference(&[128; 3 * 63 * 63]).is_none());
        assert!(sample_reference(&[128; 3 * 64 * 64]).is_some());
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
    fn no_preview_falls_back() {
        let stats = sample_decode(&rgb16(&[[2_000, 4_000, 6_000]; 100]));
        assert_eq!(tone_lut(&stats, None).0[6_000], 255);
    }

    #[test]
    fn apply_maps_every_channel() {
        let lut = Lut::from_encoded(|value| f64::from(value) / 65535.0);
        let out = apply(&lut, &rgb16(&[[0, 32_768, 65_535], [65_535, 0, 257]]));
        assert_eq!(out, [0, 128, 255, 255, 0, 1]);
    }
}
