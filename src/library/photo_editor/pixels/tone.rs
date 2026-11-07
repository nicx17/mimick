//! Tone, colour, the vignette, and sharpening: what the Adjust sliders and filters do.

use rayon::prelude::*;

use super::Rgba;
use crate::library::photo_editor::look::Recipe;

/// Tone and colour pixel by pixel, the vignette over the frame, then sharpening.
pub(in crate::library::photo_editor) fn apply(img: &mut Rgba, recipe: &Recipe) {
    if recipe.is_neutral() {
        return;
    }
    let tone = Tone::new(recipe, (img.w, img.h));
    let width = img.w;
    img.data
        .par_chunks_mut(width * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                tone.pixel(px, (x, y));
            }
        });
    if recipe.sharpen > 0.0 {
        sharpen(img, recipe.sharpen);
    }
}

/// A recipe made ready for one picture.
struct Tone {
    recipe: Recipe,
    /// Gains in linear light for exposure and white balance; `None` when they're all 1.
    gains: Option<[f32; 3]>,
    centre: (f32, f32),
}

impl Tone {
    fn new(recipe: &Recipe, (w, h): (usize, usize)) -> Self {
        let exposure = 2f32.powf(recipe.exposure * 2.0);
        let gains = [
            exposure * (1.0 + 0.25 * recipe.warmth),
            exposure * (1.0 - 0.2 * recipe.tint),
            exposure * (1.0 - 0.25 * recipe.warmth),
        ];
        Self {
            recipe: *recipe,
            gains: (gains != [1.0; 3]).then_some(gains),
            centre: (w as f32 / 2.0, h as f32 / 2.0),
        }
    }

    fn pixel(&self, px: &mut [u8], at: (usize, usize)) {
        let mut c = [px[0], px[1], px[2]].map(|v| f32::from(v) / 255.0);
        if let Some(gains) = self.gains {
            c = light(c, gains);
        }
        c = self.colour(self.tones(c));
        c = self.vignette(c, at);
        for (slot, v) in px.iter_mut().zip(c) {
            *slot = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }

    /// Shadows and highlights by brightness, then brightness and contrast.
    fn tones(&self, c: [f32; 3]) -> [f32; 3] {
        let r = &self.recipe;
        let l = luma(c);
        let lift = 0.35 * (r.shadows * (1.0 - l).powi(2) + r.highlights * l.powi(2));
        c.map(|v| {
            let v = (v + lift).clamp(0.0, 1.0);
            let v = v + r.brightness * v * (1.0 - v);
            (v - 0.5) * (1.0 + r.contrast) + 0.5
        })
    }

    /// Saturation, then the filters' grey, brown, and faded blacks.
    fn colour(&self, c: [f32; 3]) -> [f32; 3] {
        let r = &self.recipe;
        let l = luma(c);
        let mut out = c.map(|v| l + (v - l) * (1.0 + r.saturation));
        if r.mono > 0.0 {
            out = mix(out, [l; 3], r.mono);
        }
        if r.sepia > 0.0 {
            out = mix(out, [l * 1.07 + 0.03, l * 0.95 + 0.01, l * 0.78], r.sepia);
        }
        if r.fade > 0.0 {
            out = out.map(|v| r.fade + v * (1.0 - r.fade));
        }
        out
    }

    /// Darkens the edges (or lightens them, below zero), more towards the corners.
    fn vignette(&self, c: [f32; 3], (x, y): (usize, usize)) -> [f32; 3] {
        let amount = self.recipe.vignette;
        if amount == 0.0 {
            return c;
        }
        let (cx, cy) = self.centre;
        let nx = (x as f32 + 0.5 - cx) / cx;
        let ny = (y as f32 + 0.5 - cy) / cy;
        let reach = smoothstep(
            0.3,
            1.0,
            (nx * nx + ny * ny).sqrt() / std::f32::consts::SQRT_2,
        );
        if amount > 0.0 {
            c.map(|v| v * (1.0 - 0.75 * amount * reach))
        } else {
            c.map(|v| v + (1.0 - v) * -amount * 0.5 * reach)
        }
    }
}

/// Exposure and white balance, in linear light.
fn light(c: [f32; 3], gains: [f32; 3]) -> [f32; 3] {
    let mut out = c;
    for (v, gain) in out.iter_mut().zip(gains) {
        *v = (v.powf(2.2) * gain).powf(1.0 / 2.2);
    }
    out
}

fn luma([r, g, b]: [f32; 3]) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Unsharp masking: each pixel pushed away from its neighbours' average.
fn sharpen(img: &mut Rgba, amount: f32) {
    let radius = (img.w.max(img.h) / 1200).max(1);
    let across = blur_rows(&img.data, img.w, radius);
    let blurred = blur_columns(&across, img.w, img.h, radius);
    img.data
        .par_chunks_mut(4)
        .zip(blurred.par_chunks(4))
        .for_each(|(px, soft)| {
            for c in 0..3 {
                let v = f32::from(px[c]);
                let edge = v - f32::from(soft[c]);
                px[c] = (v + 1.5 * amount * edge).clamp(0.0, 255.0).round() as u8;
            }
        });
}

/// Each pixel averaged with the ones up to `radius` either side of it.
fn blur_rows(data: &[u8], w: usize, radius: usize) -> Vec<u8> {
    let mut out = vec![0u8; data.len()];
    out.par_chunks_mut(w * 4)
        .zip(data.par_chunks(w * 4))
        .for_each(|(dst, src)| {
            for x in 0..w {
                let (a, b) = (x.saturating_sub(radius), (x + radius).min(w - 1));
                let n = (b - a + 1) as u32;
                for c in 0..4 {
                    let sum: u32 = (a..=b).map(|i| u32::from(src[i * 4 + c])).sum();
                    dst[x * 4 + c] = (sum / n) as u8;
                }
            }
        });
    out
}

/// Each pixel averaged with the ones up to `radius` above and below it.
fn blur_columns(data: &[u8], w: usize, h: usize, radius: usize) -> Vec<u8> {
    let stride = w * 4;
    let mut out = vec![0u8; data.len()];
    out.par_chunks_mut(stride).enumerate().for_each(|(y, dst)| {
        let (a, b) = (y.saturating_sub(radius), (y + radius).min(h - 1));
        let n = (b - a + 1) as u32;
        for (i, slot) in dst.iter_mut().enumerate() {
            let sum: u32 = (a..=b).map(|row| u32::from(data[row * stride + i])).sum();
            *slot = (sum / n) as u8;
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::photo_editor::look::{Filter, Look, Slider};

    fn flat(w: usize, h: usize, px: [u8; 4]) -> Rgba {
        Rgba {
            w,
            h,
            data: px.repeat(w * h),
        }
    }

    #[test]
    fn mono_turns_colour_grey() {
        let mut img = flat(3, 3, [200, 40, 90, 255]);
        let mono = Look {
            filter: Filter::Mono,
            ..Look::default()
        };
        apply(&mut img, &mono.recipe());
        assert!(
            img.data
                .chunks(4)
                .all(|px| px[0] == px[1] && px[1] == px[2])
        );
    }

    #[test]
    fn exposure_brightens_and_neutral_changes_nothing() {
        let mut look = Look::default();
        look.adjust.set(Slider::Exposure, 50.0);
        let mut img = flat(2, 2, [100, 100, 100, 255]);
        apply(&mut img, &look.recipe());
        assert!(img.data[0] > 100 && img.data[3] == 255);
        let mut same = flat(2, 2, [100, 100, 100, 255]);
        apply(&mut same, &Look::default().recipe());
        assert_eq!(same, flat(2, 2, [100, 100, 100, 255]));
    }

    #[test]
    fn sharpening_leaves_flat_colour_flat() {
        let mut img = flat(5, 5, [80, 120, 160, 255]);
        sharpen(&mut img, 1.0);
        assert_eq!(img, flat(5, 5, [80, 120, 160, 255]));
    }
}
