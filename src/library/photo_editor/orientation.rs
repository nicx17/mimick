//! Rotation and mirroring in Immich's order: mirror the original, then turn it clockwise.

use crate::api_client::{AssetEdit, MirrorAxis};
use crate::library::photo_canvas::Region;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Orientation {
    /// Clockwise quarter turns, 0 to 3.
    pub quarter_turns: u8,
    /// Mirrors of the original, applied before the turn.
    pub mirror_h: bool,
    pub mirror_v: bool,
}

impl Orientation {
    pub(super) fn rotate(&mut self, clockwise: bool) {
        self.quarter_turns = (self.quarter_turns + if clockwise { 1 } else { 3 }) % 4;
    }

    /// Flips what's on screen; after a quarter turn the screen's horizontal is the original's vertical.
    pub(super) fn flip(&mut self, horizontal_on_screen: bool) {
        if horizontal_on_screen == self.quarter_turns.is_multiple_of(2) {
            self.mirror_h = !self.mirror_h;
        } else {
            self.mirror_v = !self.mirror_v;
        }
    }

    pub(super) fn oriented_size(self, (w, h): (f64, f64)) -> (f64, f64) {
        if self.quarter_turns % 2 == 1 {
            (h, w)
        } else {
            (w, h)
        }
    }

    /// `r` in the original's pixels, as it lands on screen.
    pub(super) fn to_screen(self, r: Region, (w, h): (f64, f64)) -> Region {
        let mut r = r;
        if self.mirror_h {
            r.x = w - r.x - r.w;
        }
        if self.mirror_v {
            r.y = h - r.y - r.h;
        }
        let (mut w, mut h) = (w, h);
        for _ in 0..self.quarter_turns {
            r = Region {
                x: h - r.y - r.h,
                y: r.x,
                w: r.h,
                h: r.w,
            };
            (w, h) = (h, w);
        }
        r
    }

    /// `r` on screen, back in the original's pixels.
    pub(super) fn to_original(self, r: Region, size: (f64, f64)) -> Region {
        let (mut w, mut h) = self.oriented_size(size);
        let mut r = r;
        for _ in 0..self.quarter_turns {
            r = Region {
                x: r.y,
                y: w - r.x - r.w,
                w: r.h,
                h: r.w,
            };
            (w, h) = (h, w);
        }
        if self.mirror_h {
            r.x = w - r.x - r.w;
        }
        if self.mirror_v {
            r.y = h - r.y - r.h;
        }
        r
    }

    /// Mirror and rotate edits in the order Immich applies them; empty when unchanged.
    pub(super) fn edits(self) -> Vec<AssetEdit> {
        let mut edits = Vec::new();
        if self.mirror_h {
            edits.push(AssetEdit::Mirror {
                axis: MirrorAxis::Horizontal,
            });
        }
        if self.mirror_v {
            edits.push(AssetEdit::Mirror {
                axis: MirrorAxis::Vertical,
            });
        }
        if self.quarter_turns > 0 {
            edits.push(AssetEdit::Rotate {
                angle: u32::from(self.quarter_turns) * 90,
            });
        }
        edits
    }

    /// Folds saved edits back into one orientation, whatever order they came in.
    pub(super) fn from_edits(edits: &[AssetEdit]) -> Self {
        let mut orientation = Self::default();
        for edit in edits {
            match edit {
                AssetEdit::Mirror {
                    axis: MirrorAxis::Horizontal,
                } => orientation.mirror_h = !orientation.mirror_h,
                AssetEdit::Mirror {
                    axis: MirrorAxis::Vertical,
                } => orientation.mirror_v = !orientation.mirror_v,
                AssetEdit::Rotate { angle } => {
                    orientation.quarter_turns = ((angle / 90) % 4) as u8;
                }
                AssetEdit::Crop { .. } => {}
            }
        }
        orientation
    }
}

/// RGBA pixels mirrored then turned like the photo; returns the new width and height too.
pub(super) fn orient_pixels(
    src: &[u8],
    stride: usize,
    (w, h): (usize, usize),
    orientation: Orientation,
) -> (Vec<u8>, usize, usize) {
    let (ow, oh) = if orientation.quarter_turns % 2 == 1 {
        (h, w)
    } else {
        (w, h)
    };
    let mut out = vec![0u8; ow * oh * 4];
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = oriented_pixel((x, y), (w, h), orientation);
            let from = y * stride + x * 4;
            let to = (dy * ow + dx) * 4;
            out[to..to + 4].copy_from_slice(&src[from..from + 4]);
        }
    }
    (out, ow, oh)
}

fn oriented_pixel(
    (x, y): (usize, usize),
    (w, h): (usize, usize),
    orientation: Orientation,
) -> (usize, usize) {
    let mut point = (
        if orientation.mirror_h { w - 1 - x } else { x },
        if orientation.mirror_v { h - 1 - y } else { y },
    );
    let (mut width, mut height) = (w, h);
    for _ in 0..orientation.quarter_turns {
        point = (height - 1 - point.1, point.0);
        (width, height) = (height, width);
    }
    point
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(x: f64, y: f64, w: f64, h: f64) -> Region {
        Region { x, y, w, h }
    }

    #[test]
    fn a_quarter_turn_moves_the_top_left_to_the_top_right() {
        let turned = Orientation {
            quarter_turns: 1,
            ..Orientation::default()
        };
        let top_left = region(0.0, 0.0, 10.0, 20.0);
        assert_eq!(
            turned.to_screen(top_left, (100.0, 50.0)),
            region(30.0, 0.0, 20.0, 10.0)
        );
    }

    #[test]
    fn screen_and_original_round_trip_in_every_orientation() {
        let size = (120.0, 80.0);
        let r = region(10.0, 5.0, 30.0, 40.0);
        for quarter_turns in 0..4 {
            for (mirror_h, mirror_v) in [(false, false), (true, false), (false, true), (true, true)]
            {
                let o = Orientation {
                    quarter_turns,
                    mirror_h,
                    mirror_v,
                };
                assert_eq!(o.to_original(o.to_screen(r, size), size), r, "{o:?}");
            }
        }
    }

    #[test]
    fn flipping_after_a_turn_mirrors_the_other_axis() {
        let mut o = Orientation::default();
        o.rotate(true);
        o.flip(true);
        assert!(o.mirror_v && !o.mirror_h);
        o.rotate(false);
        o.flip(true);
        assert!(o.mirror_h && o.mirror_v);
    }

    #[test]
    fn edits_round_trip_through_immichs_list() {
        let o = Orientation {
            quarter_turns: 3,
            mirror_h: true,
            mirror_v: false,
        };
        assert_eq!(
            o.edits(),
            [
                AssetEdit::Mirror {
                    axis: MirrorAxis::Horizontal
                },
                AssetEdit::Rotate { angle: 270 }
            ]
        );
        assert_eq!(Orientation::from_edits(&o.edits()), o);
        assert!(Orientation::default().edits().is_empty());
    }

    #[test]
    fn pixels_turn_like_rects() {
        // 2×1 image: red then green; a quarter turn makes it 1×2, red on top.
        let src = [255, 0, 0, 255, 0, 255, 0, 255];
        let o = Orientation {
            quarter_turns: 1,
            ..Orientation::default()
        };
        let (out, w, h) = orient_pixels(&src, 8, (2, 1), o);
        assert_eq!((w, h), (1, 2));
        assert_eq!(out, [255, 0, 0, 255, 0, 255, 0, 255]);
        let mirrored = Orientation {
            mirror_h: true,
            ..Orientation::default()
        };
        let (out, _, _) = orient_pixels(&src, 8, (2, 1), mirrored);
        assert_eq!(out, [0, 255, 0, 255, 255, 0, 0, 255]);
    }
}
