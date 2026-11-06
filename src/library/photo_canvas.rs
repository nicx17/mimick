//! Boxes over the lightbox photo, kept in image pixels: face boxes to tap, and one box to
//! draw, move, or resize (a new face, or the crop).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

mod draw;

/// Grab distance for corners and edges; generous so fingers can hit them.
const HANDLE_REACH: f64 = 22.0;
/// The smallest box a drag can make, in screen pixels.
const MIN_SIDE: f64 = 24.0;

/// A rectangle in image pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Region {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Region {
    fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

/// Where the photo is drawn in the widget, and how many widget pixels one image pixel takes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fit {
    x: f64,
    y: f64,
    scale: f64,
}

/// The rect `ContentFit::Contain` draws `image` into within `area`.
fn fit(area: (f64, f64), image: (f64, f64)) -> Option<Fit> {
    let ((aw, ah), (iw, ih)) = (area, image);
    if ![aw, ah, iw, ih].iter().all(|v| *v > 0.0) {
        return None;
    }
    let scale = (aw / iw).min(ah / ih);
    Some(Fit {
        x: (aw - iw * scale) / 2.0,
        y: (ah - ih * scale) / 2.0,
        scale,
    })
}

impl Fit {
    fn to_widget(self, r: Region) -> Region {
        Region {
            x: self.x + r.x * self.scale,
            y: self.y + r.y * self.scale,
            w: r.w * self.scale,
            h: r.h * self.scale,
        }
    }

    fn to_image(self, (x, y): (f64, f64)) -> (f64, f64) {
        ((x - self.x) / self.scale, (y - self.y) / self.scale)
    }
}

/// Which part of the selection a drag holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grip {
    Move,
    Edge {
        left: bool,
        right: bool,
        top: bool,
        bottom: bool,
    },
}

#[derive(Clone, Copy, Debug)]
enum Drag {
    Adjust { grip: Grip, start: Region },
    Draw { origin: (f64, f64) },
}

#[derive(Default)]
struct State {
    image: Cell<(f64, f64)>,
    markers: RefCell<Vec<Region>>,
    highlighted: Cell<Option<usize>>,
    selection: Cell<Option<Region>>,
    /// Width over height the selection keeps, in image pixels.
    aspect: Cell<Option<f64>>,
    /// Darken the photo outside the selection, as a crop does.
    dim_outside: Cell<bool>,
    /// A drag on the photo outside the selection draws a new one.
    can_draw: Cell<bool>,
    drag: Cell<Option<Drag>>,
    on_marker: RefCell<Option<MarkerHandler>>,
    on_selection: RefCell<Option<SelectionHandler>>,
}

type MarkerHandler = Box<dyn Fn(usize)>;
type SelectionHandler = Box<dyn Fn(Option<Region>)>;
/// Maps a point on the input widget into the layer; `None` while the layer should ignore input.
type ToLayer = Rc<dyn Fn(f64, f64) -> Option<(f64, f64)>>;

/// Boxes drawn over a photo that another widget shows; input reaches it through `attach_input`.
pub(super) struct BoxLayer {
    pub area: gtk::DrawingArea,
    state: Rc<State>,
}

impl BoxLayer {
    pub(super) fn new() -> Self {
        let area = gtk::DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .build();
        let state = Rc::new(State::default());
        let draw_state = state.clone();
        area.set_draw_func(move |_, cr, width, height| {
            let area = (f64::from(width), f64::from(height));
            if let Some(fit) = fit(area, draw_state.image.get()) {
                draw::draw(cr, &draw_state, fit);
            }
        });
        Self { area, state }
    }

    /// Size of the photo underneath; boxes are in its pixels.
    pub(super) fn set_image_size(&self, size: (f64, f64)) {
        self.state.image.set(size);
        self.area.queue_draw();
    }

    pub(super) fn image_size(&self) -> (f64, f64) {
        self.state.image.get()
    }

    pub(super) fn set_markers(&self, markers: Vec<Region>) {
        *self.state.markers.borrow_mut() = markers;
        self.area.queue_draw();
    }

    pub(super) fn highlight(&self, marker: Option<usize>) {
        self.state.highlighted.set(marker);
        self.area.queue_draw();
    }

    /// Marker `index` in the layer's own coordinates, for pointing a popover at it.
    pub(super) fn marker_rect(&self, index: usize) -> Option<Region> {
        let fit = self.fit()?;
        let marker = *self.state.markers.borrow().get(index)?;
        Some(fit.to_widget(marker))
    }

    /// Topmost marker under `p`, given in the layer's coordinates.
    pub(super) fn marker_at(&self, p: (f64, f64)) -> Option<usize> {
        let point = self.fit()?.to_image(p);
        self.state
            .markers
            .borrow()
            .iter()
            .rposition(|marker| marker.contains(point))
    }

    pub(super) fn selection(&self) -> Option<Region> {
        self.state.selection.get()
    }

    /// The selection in the layer's own coordinates.
    pub(super) fn selection_rect(&self) -> Option<Region> {
        Some(self.fit()?.to_widget(self.state.selection.get()?))
    }

    pub(super) fn set_selection(&self, region: Option<Region>) {
        self.state.selection.set(region);
        self.area.queue_draw();
    }

    pub(super) fn set_aspect(&self, aspect: Option<f64>) {
        self.state.aspect.set(aspect);
    }

    pub(super) fn set_crop_look(&self, crop: bool) {
        self.state.dim_outside.set(crop);
    }

    pub(super) fn set_can_draw(&self, can_draw: bool) {
        self.state.can_draw.set(can_draw);
    }

    pub(super) fn connect_marker_tapped(&self, f: impl Fn(usize) + 'static) {
        *self.state.on_marker.borrow_mut() = Some(Box::new(f));
    }

    pub(super) fn connect_selection_changed(&self, f: impl Fn(Option<Region>) + 'static) {
        *self.state.on_selection.borrow_mut() = Some(Box::new(f));
    }

    fn fit(&self) -> Option<Fit> {
        let area = (f64::from(self.area.width()), f64::from(self.area.height()));
        fit(area, self.state.image.get())
    }

    /// Drags and taps on `input`, in `phase`; a drag the layer takes is claimed so nothing
    /// under it pans or swipes, and anything else is left alone.
    pub(super) fn attach_input(
        &self,
        input: &impl IsA<gtk::Widget>,
        phase: gtk::PropagationPhase,
        to_layer: impl Fn(f64, f64) -> Option<(f64, f64)> + 'static,
    ) {
        let to_layer: ToLayer = Rc::new(to_layer);
        input.add_controller(self.drag_controller(phase, to_layer.clone()));
        input.add_controller(self.tap_controller(phase, to_layer));
    }

    fn drag_controller(&self, phase: gtk::PropagationPhase, to_layer: ToLayer) -> gtk::GestureDrag {
        let drag = gtk::GestureDrag::new();
        drag.set_propagation_phase(phase);
        let (state, area, map) = (self.state.clone(), self.area.clone(), to_layer.clone());
        drag.connect_drag_begin(move |gesture, x, y| {
            let started = map(x, y).is_some_and(|point| begin_drag(&state, &area, point));
            gesture.set_state(if started {
                gtk::EventSequenceState::Claimed
            } else {
                gtk::EventSequenceState::Denied
            });
        });
        let (state, area) = (self.state.clone(), self.area.clone());
        drag.connect_drag_update(move |gesture, dx, dy| {
            let Some((x, y)) = gesture.start_point() else {
                return;
            };
            if let (Some(start), Some(now)) = (to_layer(x, y), to_layer(x + dx, y + dy)) {
                update_drag(&state, &area, start, now);
            }
        });
        let state = self.state.clone();
        drag.connect_drag_end(move |_, _, _| {
            if state.drag.take().is_some()
                && let Some(notify) = state.on_selection.borrow().as_ref()
            {
                notify(state.selection.get());
            }
        });
        drag
    }

    fn tap_controller(&self, phase: gtk::PropagationPhase, to_layer: ToLayer) -> gtk::GestureClick {
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(phase);
        let (state, area) = (self.state.clone(), self.area.clone());
        click.connect_released(move |gesture, _, x, y| {
            let Some(point) = to_layer(x, y) else {
                return;
            };
            let size = (f64::from(area.width()), f64::from(area.height()));
            let Some(fit) = fit(size, state.image.get()) else {
                return;
            };
            let point = fit.to_image(point);
            let hit = state
                .markers
                .borrow()
                .iter()
                .rposition(|marker| marker.contains(point));
            if let (Some(index), Some(on_marker)) = (hit, state.on_marker.borrow().as_ref()) {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                on_marker(index);
            }
        });
        click
    }
}

fn begin_drag(state: &State, area: &gtk::DrawingArea, point: (f64, f64)) -> bool {
    let size = (f64::from(area.width()), f64::from(area.height()));
    let Some(fit) = fit(size, state.image.get()) else {
        return false;
    };
    let grip = state
        .selection
        .get()
        .and_then(|sel| grip_at(fit.to_widget(sel), point));
    let drag = match (grip, state.selection.get()) {
        (Some(grip), Some(start)) => Drag::Adjust { grip, start },
        _ if state.can_draw.get() => Drag::Draw {
            origin: clamp_point(fit.to_image(point), state.image.get()),
        },
        _ => return false,
    };
    state.drag.set(Some(drag));
    true
}

/// A drag from `start` to `now`, both in the layer's coordinates.
fn update_drag(state: &State, area: &gtk::DrawingArea, start: (f64, f64), now: (f64, f64)) {
    let size = (f64::from(area.width()), f64::from(area.height()));
    let (Some(fit), Some(drag)) = (fit(size, state.image.get()), state.drag.get()) else {
        return;
    };
    let image = state.image.get();
    let min = MIN_SIDE / fit.scale;
    let region = match drag {
        Drag::Adjust { grip, start: from } => adjust(
            from,
            grip,
            ((now.0 - start.0) / fit.scale, (now.1 - start.1) / fit.scale),
            image,
            min,
        ),
        Drag::Draw { origin } => span(origin, clamp_point(fit.to_image(now), image)),
    };
    let region = match state.aspect.get() {
        Some(aspect) => keep_aspect(region, aspect, image),
        None => region,
    };
    state.selection.set(Some(region));
    area.queue_draw();
}

/// The part of `sel` (in widget pixels) under `p`, corners and edges before the inside.
fn grip_at(sel: Region, (x, y): (f64, f64)) -> Option<Grip> {
    let near = |a: f64, b: f64| (a - b).abs() <= HANDLE_REACH;
    let within_x = x >= sel.x - HANDLE_REACH && x <= sel.x + sel.w + HANDLE_REACH;
    let within_y = y >= sel.y - HANDLE_REACH && y <= sel.y + sel.h + HANDLE_REACH;
    if !(within_x && within_y) {
        return None;
    }
    let edge = Grip::Edge {
        left: near(x, sel.x),
        right: near(x, sel.x + sel.w) && !near(x, sel.x),
        top: near(y, sel.y),
        bottom: near(y, sel.y + sel.h) && !near(y, sel.y),
    };
    match edge {
        Grip::Edge {
            left: false,
            right: false,
            top: false,
            bottom: false,
        } => sel.contains((x, y)).then_some(Grip::Move),
        edge => Some(edge),
    }
}

/// `start` moved or resized by `d` image pixels, kept inside the image and at least `min` wide.
fn adjust(start: Region, grip: Grip, d: (f64, f64), image: (f64, f64), min: f64) -> Region {
    let (iw, ih) = image;
    match grip {
        Grip::Move => Region {
            x: (start.x + d.0).clamp(0.0, (iw - start.w).max(0.0)),
            y: (start.y + d.1).clamp(0.0, (ih - start.h).max(0.0)),
            ..start
        },
        Grip::Edge {
            left,
            right,
            top,
            bottom,
        } => {
            let (mut x0, mut y0) = (start.x, start.y);
            let (mut x1, mut y1) = (start.x + start.w, start.y + start.h);
            if left {
                x0 = (x0 + d.0).clamp(0.0, x1 - min);
            }
            if right {
                x1 = (x1 + d.0).clamp(x0 + min, iw);
            }
            if top {
                y0 = (y0 + d.1).clamp(0.0, y1 - min);
            }
            if bottom {
                y1 = (y1 + d.1).clamp(y0 + min, ih);
            }
            span((x0, y0), (x1, y1))
        }
    }
}

fn span((ax, ay): (f64, f64), (bx, by): (f64, f64)) -> Region {
    Region {
        x: ax.min(bx),
        y: ay.min(by),
        w: (ax - bx).abs(),
        h: (ay - by).abs(),
    }
}

fn clamp_point((x, y): (f64, f64), (w, h): (f64, f64)) -> (f64, f64) {
    (x.clamp(0.0, w), y.clamp(0.0, h))
}

/// Shrinks `r` around its centre to `aspect` (width over height), staying inside the image.
pub(super) fn keep_aspect(r: Region, aspect: f64, (iw, ih): (f64, f64)) -> Region {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let (mut w, mut h) = (r.w, r.h);
    if w / h.max(f64::EPSILON) > aspect {
        w = h * aspect;
    } else {
        h = w / aspect;
    }
    let x = (cx - w / 2.0).clamp(0.0, (iw - w).max(0.0));
    let y = (cy - h / 2.0).clamp(0.0, (ih - h).max(0.0));
    Region { x, y, w, h }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(x: f64, y: f64, w: f64, h: f64) -> Region {
        Region { x, y, w, h }
    }

    #[test]
    fn photos_fit_centred_and_map_both_ways() {
        let fit = fit((200.0, 100.0), (100.0, 100.0)).unwrap();
        assert_eq!(
            fit,
            Fit {
                x: 50.0,
                y: 0.0,
                scale: 1.0
            }
        );
        assert_eq!(fit.to_image((60.0, 10.0)), (10.0, 10.0));
        assert!(super::fit((0.0, 10.0), (10.0, 10.0)).is_none());
    }

    #[test]
    fn corners_win_over_moving() {
        let sel = region(100.0, 100.0, 200.0, 200.0);
        let corner = grip_at(sel, (102.0, 98.0));
        assert_eq!(
            corner,
            Some(Grip::Edge {
                left: true,
                right: false,
                top: true,
                bottom: false
            })
        );
        assert_eq!(grip_at(sel, (200.0, 200.0)), Some(Grip::Move));
        assert_eq!(grip_at(sel, (10.0, 10.0)), None);
    }

    #[test]
    fn moves_stay_inside_and_resizes_keep_a_minimum() {
        let start = region(10.0, 10.0, 50.0, 50.0);
        let moved = adjust(start, Grip::Move, (-100.0, 500.0), (100.0, 100.0), 5.0);
        assert_eq!(moved, region(0.0, 50.0, 50.0, 50.0));
        let left = Grip::Edge {
            left: true,
            right: false,
            top: false,
            bottom: false,
        };
        let squeezed = adjust(start, left, (100.0, 0.0), (100.0, 100.0), 5.0);
        assert_eq!(squeezed, region(55.0, 10.0, 5.0, 50.0));
    }

    #[test]
    fn aspect_shrinks_around_the_centre() {
        let square = keep_aspect(region(0.0, 0.0, 200.0, 100.0), 1.0, (200.0, 100.0));
        assert_eq!(square, region(50.0, 0.0, 100.0, 100.0));
        let wide = keep_aspect(region(0.0, 0.0, 100.0, 100.0), 2.0, (100.0, 100.0));
        assert_eq!(wide, region(0.0, 25.0, 100.0, 50.0));
    }
}
