//! A 360° (equirectangular) photo seen through a camera, drawn on the CPU across threads.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use rayon::prelude::*;

/// Narrowest and widest view across, and where a panorama opens.
pub(in crate::library::lightbox) const MIN_FOV: f64 = 30.0 * PI / 180.0;
pub(in crate::library::lightbox) const MAX_FOV: f64 = 120.0 * PI / 180.0;
const DEFAULT_FOV: f64 = 80.0 * PI / 180.0;
/// Stops just short of straight up or down, where the view would spin.
const MAX_PITCH: f64 = FRAC_PI_2 - 0.001;
const BYTES_PER_PIXEL: usize = 3;

/// RGB pixels of a whole panorama: 360° across and 180° from top to bottom.
pub(in crate::library::lightbox) struct Equirect {
    pub pixels: glib::Bytes,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
}

impl Equirect {
    fn pixel(&self, x: usize, y: usize) -> [f64; 3] {
        let at = y * self.stride + x * BYTES_PER_PIXEL;
        let p = &self.pixels[at..at + BYTES_PER_PIXEL];
        [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]
    }

    /// Bilinear sample at (`u`, `v`) in pixels; wraps across the 360° seam.
    fn sample(&self, u: f64, v: f64) -> [u8; 3] {
        let v = v.clamp(0.0, (self.height - 1) as f64);
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let wrap = |x: f64| (x as i64).rem_euclid(self.width as i64) as usize;
        let (left, right) = (wrap(x0), wrap(x0 + 1.0));
        let (top, bottom) = (y0 as usize, (y0 as usize + 1).min(self.height - 1));
        let corners = [
            self.pixel(left, top),
            self.pixel(right, top),
            self.pixel(left, bottom),
            self.pixel(right, bottom),
        ];
        let mut out = [0u8; 3];
        for (channel, value) in out.iter_mut().enumerate() {
            let upper = corners[0][channel] * (1.0 - fx) + corners[1][channel] * fx;
            let lower = corners[2][channel] * (1.0 - fx) + corners[3][channel] * fx;
            *value = (upper * (1.0 - fy) + lower * fy).round() as u8;
        }
        out
    }
}

/// Where the view looks: `yaw` turns right, `pitch` looks up, `fov` is the width it spans.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::library::lightbox) struct Camera {
    pub yaw: f64,
    pub pitch: f64,
    pub fov: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            fov: DEFAULT_FOV,
        }
    }
}

impl Camera {
    /// Turned by `yaw` and tilted by `pitch` radians, stopping at straight up and down.
    pub fn turned(self, yaw: f64, pitch: f64) -> Self {
        Self {
            yaw: (self.yaw + yaw).rem_euclid(TAU),
            pitch: (self.pitch + pitch).clamp(-MAX_PITCH, MAX_PITCH),
            fov: self.fov,
        }
    }

    /// A drag of (`dx`, `dy`) pixels on a view `width` wide: the photo moves with the pointer.
    pub fn dragged(self, dx: f64, dy: f64, width: f64) -> Self {
        let per_pixel = self.fov / width.max(1.0);
        self.turned(-dx * per_pixel, dy * per_pixel)
    }

    /// Zoomed in by `factor` (above 1) or out (below 1).
    pub fn zoomed(self, factor: f64) -> Self {
        Self {
            fov: (self.fov / factor).clamp(MIN_FOV, MAX_FOV),
            ..self
        }
    }
}

/// The camera's rotation and lens, worked out once per frame.
struct View {
    sin_yaw: f64,
    cos_yaw: f64,
    sin_pitch: f64,
    cos_pitch: f64,
    half_width: f64,
    half_height: f64,
    width: usize,
    height: usize,
}

impl View {
    fn new(camera: Camera, width: usize, height: usize) -> Self {
        let half_width = (camera.fov / 2.0).tan();
        Self {
            sin_yaw: camera.yaw.sin(),
            cos_yaw: camera.yaw.cos(),
            sin_pitch: camera.pitch.sin(),
            cos_pitch: camera.pitch.cos(),
            half_width,
            half_height: half_width * height as f64 / width.max(1) as f64,
            width,
            height,
        }
    }

    /// Where the ray through view pixel (`col`, `row`) meets the panorama, as fractions
    /// of its width (0 to 1, left to right) and height (0 at the top).
    fn ray(&self, col: usize, row: usize) -> (f64, f64) {
        let x = (2.0 * (col as f64 + 0.5) / self.width as f64 - 1.0) * self.half_width;
        let y = (1.0 - 2.0 * (row as f64 + 0.5) / self.height as f64) * self.half_height;
        let (y1, z1) = (
            y * self.cos_pitch + self.sin_pitch,
            self.cos_pitch - y * self.sin_pitch,
        );
        let (x2, z2) = (
            x * self.cos_yaw + z1 * self.sin_yaw,
            z1 * self.cos_yaw - x * self.sin_yaw,
        );
        let longitude = x2.atan2(z2);
        let latitude = y1.atan2(x2.hypot(z2));
        (longitude / TAU + 0.5, 0.5 - latitude / PI)
    }
}

/// The view `camera` sees, as `width`×`height` RGB pixels with no row padding.
pub(in crate::library::lightbox) fn render(
    source: &Equirect,
    camera: Camera,
    width: usize,
    height: usize,
) -> Vec<u8> {
    let view = View::new(camera, width, height);
    let mut out = vec![0u8; width * height * BYTES_PER_PIXEL];
    let (scale_u, scale_v) = (source.width as f64, source.height as f64);
    out.par_chunks_mut(width * BYTES_PER_PIXEL)
        .enumerate()
        .for_each(|(row, line)| {
            let (pixels, _) = line.as_chunks_mut::<BYTES_PER_PIXEL>();
            for (col, pixel) in pixels.iter_mut().enumerate() {
                let (u, v) = view.ray(col, row);
                pixel.copy_from_slice(&source.sample(u * scale_u - 0.5, v * scale_v - 0.5));
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    fn centre(camera: Camera) -> (f64, f64) {
        View::new(camera, 101, 101).ray(50, 50)
    }

    #[test]
    fn the_centre_of_the_view_follows_the_camera() {
        assert!(close(centre(Camera::default()), (0.5, 0.5)));
        let right = Camera::default().turned(FRAC_PI_2, 0.0);
        assert!(close(centre(right), (0.75, 0.5)));
        let up = Camera::default().turned(0.0, PI / 4.0);
        assert!(close(centre(up), (0.5, 0.25)));
    }

    #[test]
    fn dragging_right_looks_left_and_dragging_down_looks_up() {
        let moved = Camera::default().dragged(100.0, 100.0, 800.0);
        assert!(moved.yaw > PI && moved.pitch > 0.0);
    }

    #[test]
    fn tilt_and_zoom_stop_at_their_limits() {
        let camera = Camera::default().turned(0.0, 10.0).zoomed(100.0);
        assert!((camera.pitch - MAX_PITCH).abs() < 1e-9);
        assert!((camera.fov - MIN_FOV).abs() < 1e-9);
        assert!((Camera::default().zoomed(0.01).fov - MAX_FOV).abs() < 1e-9);
    }

    #[test]
    fn rendering_samples_the_panorama_and_wraps_the_seam() {
        // Four columns: red, green, blue, white; the camera faces the seam between green and blue.
        let row = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let source = Equirect {
            pixels: glib::Bytes::from_owned([row, row].concat()),
            width: 4,
            height: 2,
            stride: 12,
        };
        let ahead = render(&source, Camera::default(), 1, 1);
        assert_eq!(ahead, [0, 128, 128]);
        let behind = render(&source, Camera::default().turned(PI, 0.0), 1, 1);
        assert_eq!(behind, [255, 128, 128]);
    }
}
