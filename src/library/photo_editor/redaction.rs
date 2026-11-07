//! Hiding parts of a photo: coarse blocks, a heavy blur, a blur with grain over it, or a solid
//! bar. Sizes follow the picture, so the preview and the full-size copy look alike.

use super::pixels::{Rgba, average};
use crate::library::photo_canvas::Region;

/// How an area is hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::library) enum RedactStyle {
    Pixelate,
    Blur,
    /// A blur with grain on top, which also leaves nothing smooth to sharpen back.
    Frosted,
    /// A solid bar of this colour.
    Bar([u8; 3]),
}

/// One hidden area, in the levelled preview's original-orientation pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::library) struct Redaction {
    pub area: Region,
    pub style: RedactStyle,
}

/// Grain on a frosted area, either way, in levels out of 255.
const GRAIN: i32 = 28;

/// Hides every area of `img`, each clipped to the picture.
pub(super) fn redact(img: &mut Rgba, redactions: &[Redaction]) {
    for redaction in redactions {
        let Some(cell) = clip(img, redaction.area) else {
            continue;
        };
        match redaction.style {
            RedactStyle::Pixelate => pixelate(img, cell),
            RedactStyle::Blur => blur(img, cell),
            RedactStyle::Frosted => {
                blur(img, cell);
                grain(img, cell);
            }
            RedactStyle::Bar([r, g, b]) => fill(img, cell, [r, g, b, u8::MAX]),
        }
    }
}

/// `area` as whole pixels `(x0, y0, x1, y1)` inside the picture; `None` when outside it.
fn clip(img: &Rgba, area: Region) -> Option<(usize, usize, usize, usize)> {
    let x0 = area.x.max(0.0) as usize;
    let y0 = area.y.max(0.0) as usize;
    let x1 = ((area.x + area.w).ceil().max(0.0) as usize).min(img.w);
    let y1 = ((area.y + area.h).ceil().max(0.0) as usize).min(img.h);
    (x0 < x1 && y0 < y1).then_some((x0, y0, x1, y1))
}

/// Blocks sized to the picture.
fn pixelate(img: &mut Rgba, (x0, y0, x1, y1): (usize, usize, usize, usize)) {
    let block = (img.w.max(img.h) / 48).max(4);
    for by in (y0..y1).step_by(block) {
        for bx in (x0..x1).step_by(block) {
            let cell = (bx, by, (bx + block).min(x1), (by + block).min(y1));
            let colour = average(img, cell);
            fill(img, cell, colour);
        }
    }
}

fn fill(img: &mut Rgba, cell: (usize, usize, usize, usize), colour: [u8; 4]) {
    let (x0, y0, x1, y1) = cell;
    for y in y0..y1 {
        let row = &mut img.data[(y * img.w + x0) * 4..(y * img.w + x1) * 4];
        for px in row.as_chunks_mut::<4>().0 {
            *px = colour;
        }
    }
}

/// Three box blurs across and down, close to a Gaussian, reaching only inside the area.
fn blur(img: &mut Rgba, (x0, y0, x1, y1): (usize, usize, usize, usize)) {
    let radius = (img.w.max(img.h) / 60).max(3);
    let (w, h) = (x1 - x0, y1 - y0);
    let mut area = Vec::with_capacity(w * h * 4);
    for y in y0..y1 {
        area.extend_from_slice(&img.data[(y * img.w + x0) * 4..(y * img.w + x1) * 4]);
    }
    for _ in 0..3 {
        box_blur(&mut area, (w, h), radius, true);
        box_blur(&mut area, (w, h), radius, false);
    }
    for (row, y) in area.chunks_exact(w * 4).zip(y0..y1) {
        img.data[(y * img.w + x0) * 4..(y * img.w + x1) * 4].copy_from_slice(row);
    }
}

/// One box blur along rows (`across`) or columns, with running sums so the radius costs
/// nothing; pixels past the edge repeat the edge.
fn box_blur(data: &mut [u8], (w, h): (usize, usize), radius: usize, across: bool) {
    let (lines, len) = if across { (h, w) } else { (w, h) };
    let at = |line: usize, i: usize| {
        if across {
            (line * w + i) * 4
        } else {
            (i * w + line) * 4
        }
    };
    let window = (2 * radius + 1) as u32;
    let mut out = vec![[0u8; 4]; len];
    for line in 0..lines {
        let px = |i: isize| at(line, i.clamp(0, len as isize - 1) as usize);
        let mut sum = [0u32; 4];
        for i in -(radius as isize)..=radius as isize {
            add(&mut sum, &data[px(i)..px(i) + 4], 1);
        }
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = sum.map(|total| (total / window) as u8);
            let (leaving, entering) = (
                px(i as isize - radius as isize),
                px(i as isize + radius as isize + 1),
            );
            add(&mut sum, &data[entering..entering + 4], 1);
            add(&mut sum, &data[leaving..leaving + 4], -1);
        }
        for (i, colour) in out.iter().enumerate() {
            data[at(line, i)..at(line, i) + 4].copy_from_slice(colour);
        }
    }
}

fn add(sum: &mut [u32; 4], px: &[u8], sign: i32) {
    for (total, v) in sum.iter_mut().zip(px) {
        *total = total.wrapping_add_signed(sign * i32::from(*v));
    }
}

/// Fixed noise from each pixel's position, the same on every render.
fn grain(img: &mut Rgba, (x0, y0, x1, y1): (usize, usize, usize, usize)) {
    for y in y0..y1 {
        for x in x0..x1 {
            let hash = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663);
            let noise = (hash >> 8) as i32 % (2 * GRAIN + 1) - GRAIN;
            let at = (y * img.w + x) * 4;
            for v in &mut img.data[at..at + 3] {
                *v = (i32::from(*v) + noise).clamp(0, 255) as u8;
            }
        }
    }
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

    fn area(x: f64, y: f64, w: f64, h: f64, style: RedactStyle) -> Redaction {
        Redaction {
            area: Region { x, y, w, h },
            style,
        }
    }

    #[test]
    fn pixelating_evens_out_the_area_only() {
        let mut img = flat(8, 8, [0, 0, 0, 255]);
        img.data[0] = 255;
        redact(&mut img, &[area(0.0, 0.0, 4.0, 4.0, RedactStyle::Pixelate)]);
        let red: Vec<u8> = (0..4).map(|x| img.data[x * 4]).collect();
        assert_eq!(red, [15, 15, 15, 15]);
        assert_eq!(img.data[4 * 4], 0);
    }

    #[test]
    fn bars_cover_in_their_colour_and_stay_inside() {
        let mut img = flat(4, 4, [9, 9, 9, 255]);
        redact(
            &mut img,
            &[area(2.0, -5.0, 10.0, 6.0, RedactStyle::Bar([200, 10, 20]))],
        );
        assert_eq!(&img.data[2 * 4..3 * 4], &[200, 10, 20, 255]);
        assert_eq!(&img.data[..4], &[9, 9, 9, 255]);
        assert_eq!(&img.data[(4 + 2) * 4..(4 + 3) * 4], &[9, 9, 9, 255]);
    }

    #[test]
    fn blurring_spreads_a_point_and_keeps_flat_colour() {
        let mut img = flat(9, 9, [0, 0, 0, 255]);
        img.data[(4 * 9 + 4) * 4] = 255;
        redact(&mut img, &[area(0.0, 0.0, 9.0, 9.0, RedactStyle::Blur)]);
        assert!(img.data[(4 * 9 + 4) * 4] < 255);
        assert!(img.data[(4 * 9 + 5) * 4] > 0);
        let mut plain = flat(6, 6, [40, 80, 120, 255]);
        redact(&mut plain, &[area(0.0, 0.0, 6.0, 6.0, RedactStyle::Blur)]);
        assert_eq!(plain, flat(6, 6, [40, 80, 120, 255]));
    }

    #[test]
    fn frosting_adds_the_same_grain_every_time() {
        let mut once = flat(8, 8, [128, 128, 128, 255]);
        let mut twice = once.clone();
        let frost = [area(0.0, 0.0, 8.0, 8.0, RedactStyle::Frosted)];
        redact(&mut once, &frost);
        redact(&mut twice, &frost);
        assert_eq!(once, twice);
        assert!(once.data.as_chunks::<4>().0.iter().any(|px| px[0] != 128));
    }
}
