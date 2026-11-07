//! The pixel work for edits Immich can't store: pixelating, levelling, cropping, and the look.

use rayon::prelude::*;

use super::look::Recipe;
use super::orientation::{Orientation, orient_pixels};
use super::redaction::{Redaction, redact};
use crate::library::photo_canvas::Region;

mod tone;

pub(super) use tone::apply;

/// RGBA pixels, four bytes each, rows packed.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Rgba {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

impl Rgba {
    /// Copies pixels whose rows are `stride` bytes apart.
    pub(super) fn from_strided(bytes: &[u8], stride: usize, (w, h): (usize, usize)) -> Self {
        let mut data = Vec::with_capacity(w * h * 4);
        for row in 0..h {
            data.extend_from_slice(&bytes[row * stride..row * stride + w * 4]);
        }
        Self { w, h, data }
    }

    fn size(&self) -> (f64, f64) {
        (self.w as f64, self.h as f64)
    }
}

/// Every step, in order, for one render; regions are in the source's pixels.
#[derive(Clone, Debug, Default)]
pub(super) struct Steps {
    /// Hidden once the photo is level, in the levelled source's pixels.
    pub redactions: Vec<Redaction>,
    /// Clockwise degrees in the source's own orientation.
    pub degrees: f64,
    /// In the levelled source's pixels.
    pub crop: Option<Region>,
    pub orientation: Orientation,
    pub recipe: Option<Recipe>,
}

impl Steps {
    /// The same steps for a source of `to` pixels, set out on one of `from` pixels.
    pub(super) fn scaled(mut self, from: (f64, f64), to: (f64, f64)) -> Self {
        let (sx, sy) = (to.0 / from.0.max(1.0), to.1 / from.1.max(1.0));
        let scale = |r: Region| Region {
            x: r.x * sx,
            y: r.y * sy,
            w: r.w * sx,
            h: r.h * sy,
        };
        for redaction in &mut self.redactions {
            redaction.area = scale(redaction.area);
        }
        self.crop = self.crop.map(scale);
        self
    }
}

/// The source with every step applied; each step frees the one before, so a full-size
/// photo needs about two copies at a time.
pub(super) fn develop(img: Rgba, steps: &Steps) -> Rgba {
    let mut img = straighten(img, steps.degrees);
    redact(&mut img, &steps.redactions);
    let img = match steps.crop {
        Some(r) => crop(&img, r),
        None => img,
    };
    let mut img = orient(img, steps.orientation);
    if let Some(recipe) = &steps.recipe {
        apply(&mut img, recipe);
    }
    img
}

/// How much a picture turned by `degrees` must grow to fill its frame with no empty corners.
pub(super) fn fill_scale(degrees: f64, (w, h): (f64, f64)) -> f64 {
    let a = degrees.to_radians().abs();
    a.cos() + (w / h).max(h / w) * a.sin()
}

/// Turned clockwise by `degrees` about the centre and enlarged to fill the same frame.
fn straighten(src: Rgba, degrees: f64) -> Rgba {
    if degrees.abs() < 0.01 || src.w == 0 || src.h == 0 {
        return src;
    }
    let scale = fill_scale(degrees, src.size());
    let (sin, cos) = (-degrees.to_radians()).sin_cos();
    let (cx, cy) = (src.w as f64 / 2.0, src.h as f64 / 2.0);
    let mut data = vec![0u8; src.data.len()];
    data.par_chunks_mut(src.w * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let dx = (x as f64 + 0.5 - cx) / scale;
                let dy = (y as f64 + 0.5 - cy) / scale;
                let sx = cx + dx * cos - dy * sin - 0.5;
                let sy = cy + dx * sin + dy * cos - 0.5;
                px.copy_from_slice(&bilinear(&src, sx, sy));
            }
        });
    Rgba {
        w: src.w,
        h: src.h,
        data,
    }
}

fn bilinear(src: &Rgba, x: f64, y: f64) -> [u8; 4] {
    let x = x.clamp(0.0, (src.w - 1) as f64);
    let y = y.clamp(0.0, (src.h - 1) as f64);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(src.w - 1), (y0 + 1).min(src.h - 1));
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let at = |px: usize, py: usize, c: usize| f64::from(src.data[(py * src.w + px) * 4 + c]);
    let mut out = [0u8; 4];
    for (c, slot) in out.iter_mut().enumerate() {
        let top = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
        let bottom = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
        *slot = (top * (1.0 - fy) + bottom * fy).round() as u8;
    }
    out
}

/// Where `r` lands once its picture of `size` is levelled by `degrees`, as the box around it.
pub(super) fn straighten_region(r: Region, degrees: f64, size: (f64, f64)) -> Region {
    turn_box(r, degrees.to_radians(), fill_scale(degrees, size), size)
}

fn turn_box(r: Region, radians: f64, scale: f64, (w, h): (f64, f64)) -> Region {
    let (sin, cos) = radians.sin_cos();
    let (cx, cy) = (w / 2.0, h / 2.0);
    let corners = [
        (r.x, r.y),
        (r.x + r.w, r.y),
        (r.x, r.y + r.h),
        (r.x + r.w, r.y + r.h),
    ]
    .map(|(x, y)| {
        let (dx, dy) = (x - cx, y - cy);
        (
            cx + scale * (dx * cos - dy * sin),
            cy + scale * (dx * sin + dy * cos),
        )
    });
    let min_x = corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let max_x = corners
        .iter()
        .map(|p| p.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_y = corners
        .iter()
        .map(|p| p.1)
        .fold(f64::NEG_INFINITY, f64::max);
    Region {
        x: min_x,
        y: min_y,
        w: max_x - min_x,
        h: max_y - min_y,
    }
}

/// The part of the picture inside `r`, at least a pixel.
fn crop(src: &Rgba, r: Region) -> Rgba {
    let x0 = (r.x.round().max(0.0) as usize).min(src.w.saturating_sub(1));
    let y0 = (r.y.round().max(0.0) as usize).min(src.h.saturating_sub(1));
    let w = (r.w.round().max(1.0) as usize).min(src.w - x0);
    let h = (r.h.round().max(1.0) as usize).min(src.h - y0);
    let mut data = Vec::with_capacity(w * h * 4);
    for y in y0..y0 + h {
        data.extend_from_slice(&src.data[(y * src.w + x0) * 4..(y * src.w + x0 + w) * 4]);
    }
    Rgba { w, h, data }
}

fn orient(src: Rgba, orientation: Orientation) -> Rgba {
    if orientation == Orientation::default() {
        return src;
    }
    let (data, w, h) = orient_pixels(&src.data, src.w * 4, (src.w, src.h), orientation);
    Rgba { w, h, data }
}

/// A copy no longer or wider than `edge`, each pixel the average of those it covers.
pub(super) fn shrink(src: &Rgba, edge: usize) -> Rgba {
    let scale = (edge as f64 / src.w.max(src.h).max(1) as f64).min(1.0);
    let w = ((src.w as f64 * scale).round() as usize).max(1);
    let h = ((src.h as f64 * scale).round() as usize).max(1);
    let mut data = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let (y0, y1) = (y * src.h / h, ((y + 1) * src.h / h).max(y * src.h / h + 1));
        for x in 0..w {
            let (x0, x1) = (x * src.w / w, ((x + 1) * src.w / w).max(x * src.w / w + 1));
            data.extend(average(src, (x0, y0, x1, y1)));
        }
    }
    Rgba { w, h, data }
}

pub(super) fn average(img: &Rgba, (x0, y0, x1, y1): (usize, usize, usize, usize)) -> [u8; 4] {
    let mut sum = [0u64; 4];
    for y in y0..y1 {
        for px in img.data[(y * img.w + x0) * 4..(y * img.w + x1) * 4]
            .as_chunks::<4>()
            .0
        {
            for (total, v) in sum.iter_mut().zip(px) {
                *total += u64::from(*v);
            }
        }
    }
    let n = ((x1 - x0) * (y1 - y0)).max(1) as u64;
    sum.map(|total| (total / n) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: usize, h: usize, px: [u8; 4]) -> Rgba {
        Rgba {
            w,
            h,
            data: px.repeat(w * h),
        }
    }

    fn region(x: f64, y: f64, w: f64, h: f64) -> Region {
        Region { x, y, w, h }
    }

    #[test]
    fn levelling_grows_the_picture_just_enough() {
        assert!((fill_scale(0.0, (400.0, 300.0)) - 1.0).abs() < 1e-12);
        let square = fill_scale(45.0, (100.0, 100.0));
        assert!((square - std::f64::consts::SQRT_2).abs() < 1e-9);
    }

    #[test]
    fn level_pictures_are_left_alone() {
        let img = Rgba {
            w: 2,
            h: 1,
            data: vec![1, 2, 3, 255, 4, 5, 6, 255],
        };
        assert_eq!(straighten(img.clone(), 0.0), img);
        assert_eq!(develop(img.clone(), &Steps::default()), img);
    }

    #[test]
    fn levelled_regions_grow_to_hold_what_turned() {
        let size = (400.0, 300.0);
        let r = region(190.0, 140.0, 20.0, 20.0);
        assert_eq!(straighten_region(r, 0.0, size), r);
        let turned = straighten_region(r, 10.0, size);
        assert!(turned.w > r.w && turned.h > r.h);
    }

    #[test]
    fn shrinking_averages_and_keeps_the_shape() {
        let img = Rgba {
            w: 4,
            h: 2,
            data: [[0, 0, 0, 255], [200, 0, 0, 255]].concat().repeat(4),
        };
        let small = shrink(&img, 2);
        assert_eq!((small.w, small.h), (2, 1));
        assert_eq!(&small.data[..4], &[100, 0, 0, 255]);
        let steps = Steps {
            crop: Some(region(10.0, 10.0, 20.0, 20.0)),
            ..Steps::default()
        };
        let full = steps.scaled((100.0, 100.0), (400.0, 200.0));
        assert_eq!(full.crop, Some(region(40.0, 20.0, 80.0, 40.0)));
    }

    #[test]
    fn crops_take_the_box_and_stay_inside() {
        let mut img = flat(4, 4, [0, 0, 0, 255]);
        img.data[(4 + 2) * 4] = 9;
        let cut = crop(&img, region(2.0, 1.0, 10.0, 1.0));
        assert_eq!((cut.w, cut.h), (2, 1));
        assert_eq!(cut.data[0], 9);
    }
}
