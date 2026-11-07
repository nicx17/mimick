//! A thumbwheel for fine adjustments: a ribbed drum with a ruler on it, lit from above. Drag
//! it, scroll over it, or use the arrow keys, and the ribs and ticks roll past the mark in the
//! middle, crowding and darkening towards the edges as the drum curves away.

use std::cell::Cell;
use std::f64::consts::FRAC_PI_2;
use std::rc::Rc;

use gtk::prelude::*;

/// Pixels of drag per unit of value, along the front of the drum: a whole degree takes a
/// deliberate movement, and a tenth about a pixel.
const PX_PER_UNIT: f64 = 12.0;
/// A longer tick every this many units; the longest marks where the value rests.
const MAJOR_EVERY: i64 = 5;
const HEIGHT: i32 = 36;
/// Narrowest the wheel gets, so it still has a few ticks to turn.
const MIN_WIDTH: i32 = 80;

/// A wheel that moves `adjustment`, snapped to its step; `rest` gets the longest tick.
pub(super) fn thumbwheel(adjustment: &gtk::Adjustment, name: &str, rest: f64) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(HEIGHT)
        .width_request(MIN_WIDTH)
        .hexpand(true)
        .focusable(true)
        .accessible_role(gtk::AccessibleRole::Slider)
        .tooltip_text(name)
        .css_classes(["mimick-wheel"])
        .build();
    area.update_property(&[gtk::accessible::Property::Label(name)]);
    let model = adjustment.clone();
    area.set_draw_func(move |area, cr, w, h| {
        let size = (f64::from(w), f64::from(h));
        draw(cr, area, size, &model, rest);
    });
    let shown = area.downgrade();
    adjustment.connect_value_changed(move |adjustment| {
        if let Some(area) = shown.upgrade() {
            area.queue_draw();
            announce(&area, adjustment);
        }
    });
    announce(&area, adjustment);
    connect_drag(&area, adjustment);
    connect_scroll(&area, adjustment);
    connect_keys(&area, adjustment);
    area
}

/// Tells screen readers the range and the value.
fn announce(area: &gtk::DrawingArea, adjustment: &gtk::Adjustment) {
    use gtk::accessible::Property;
    area.update_property(&[
        Property::ValueMin(adjustment.lower()),
        Property::ValueMax(adjustment.upper()),
        Property::ValueNow(adjustment.value()),
        Property::ValueText(&format!("{:.1}", adjustment.value())),
    ]);
}

fn set_snapped(adjustment: &gtk::Adjustment, value: f64) {
    let step = adjustment.step_increment().max(f64::EPSILON);
    adjustment.set_value((value / step).round() * step);
}

/// Dragging the ruler right brings lower values to the mark, as turning a real wheel would.
fn connect_drag(area: &gtk::DrawingArea, adjustment: &gtk::Adjustment) {
    let drag = gtk::GestureDrag::new();
    let start = Rc::new(Cell::new(0.0));
    let (begin, model, focus) = (start.clone(), adjustment.clone(), area.downgrade());
    drag.connect_drag_begin(move |gesture, _, _| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        begin.set(model.value());
        if let Some(area) = focus.upgrade() {
            area.grab_focus();
        }
    });
    let model = adjustment.clone();
    drag.connect_drag_update(move |_, dx, _| {
        set_snapped(&model, start.get() - dx / PX_PER_UNIT);
    });
    area.add_controller(drag);
}

/// A scroll-wheel click turns one step; touchpads turn it as far as the fingers move.
/// High-resolution wheels and small finger movements send fractions of a step, which add
/// up across events instead of each rounding away to nothing.
fn connect_scroll(area: &gtk::DrawingArea, adjustment: &gtk::Adjustment) {
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    let model = adjustment.clone();
    let pending = Rc::new(Cell::new(0.0));
    let carried = pending.clone();
    scroll.connect_scroll(move |controller, dx, dy| {
        let delta = if dx.abs() > dy.abs() { dx } else { dy };
        let per = if controller.unit() == gtk::gdk::ScrollUnit::Wheel {
            model.step_increment()
        } else {
            1.0 / PX_PER_UNIT
        };
        let step = model.step_increment().max(f64::EPSILON);
        let (turn, left) = whole_steps(carried.get() + delta * per, step);
        carried.set(left);
        if turn != 0.0 {
            set_snapped(&model, model.value() + turn);
        }
        glib::Propagation::Stop
    });
    scroll.connect_scroll_end(move |_| pending.set(0.0));
    area.add_controller(scroll);
}

/// The whole steps in `amount`, and what's left over for the next scroll event.
fn whole_steps(amount: f64, step: f64) -> (f64, f64) {
    // A hair of slack, so 0.3 / 0.1 counts as three steps, not 2.999…
    let steps = (amount / step + amount.signum() * 1e-9).trunc();
    (steps * step, amount - steps * step)
}

/// Arrows step by the adjustment's step, or its page with Shift.
fn connect_keys(area: &gtk::DrawingArea, adjustment: &gtk::Adjustment) {
    use gtk::gdk::Key;
    let keys = gtk::EventControllerKey::new();
    let model = adjustment.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let step = if mods.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
            model.page_increment()
        } else {
            model.step_increment()
        };
        let delta = match key {
            Key::Left | Key::Down | Key::KP_Left | Key::KP_Down => -step,
            Key::Right | Key::Up | Key::KP_Right | Key::KP_Up => step,
            _ => return glib::Propagation::Proceed,
        };
        set_snapped(&model, model.value() + delta);
        glib::Propagation::Stop
    });
    area.add_controller(keys);
}

fn draw(
    cr: &gtk::cairo::Context,
    area: &gtk::DrawingArea,
    size: (f64, f64),
    adjustment: &gtk::Adjustment,
    rest: f64,
) {
    let fg = area.color();
    let _ = cr.save();
    rounded(cr, size, RADIUS);
    cr.clip();
    let (r, g, b) = rgb(&fg);
    cr.set_source_rgba(r, g, b, 0.08);
    let _ = cr.paint();
    ribs(cr, size, adjustment.value());
    ticks(cr, &fg, size, adjustment, rest);
    let _ = cr.restore();
    depth(cr, size);
    mark(cr, size);
}

/// Corner radius of the drum's window.
const RADIUS: f64 = 8.0;

/// Raised ribs between the ticks, lit where they face the viewer.
fn ribs(cr: &gtk::cairo::Context, (w, h): (f64, f64), value: f64) {
    let around = value.round() as i64;
    let reach = (w / 2.0 * FRAC_PI_2 / PX_PER_UNIT).ceil() as i64;
    for rib in around - reach..=around + reach {
        let start = on_drum(rib as f64 + 0.2 - value, w);
        let end = on_drum(rib as f64 + 0.6 - value, w);
        let (Some((x0, facing)), Some((x1, _))) = (start, end) else {
            continue;
        };
        cr.rectangle(x0, 0.0, x1 - x0, h);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.17 * facing);
        let _ = cr.fill();
        cr.rectangle(x1, 0.0, 1.0, h);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.45 * facing);
        let _ = cr.fill();
    }
}

/// Shading that makes the drum look round and set into the panel: dark where it turns away
/// at the sides, a highlight along the top, shadow along the bottom, and a recessed rim.
fn depth(cr: &gtk::cairo::Context, (w, h): (f64, f64)) {
    rounded(cr, (w, h), RADIUS);
    let sides = gtk::cairo::LinearGradient::new(0.0, 0.0, w, 0.0);
    for (at, alpha) in [(0.0, 0.65), (0.35, 0.0), (0.65, 0.0), (1.0, 0.65)] {
        sides.add_color_stop_rgba(at, 0.0, 0.0, 0.0, alpha);
    }
    let _ = cr.set_source(&sides);
    let _ = cr.fill_preserve();
    let light = gtk::cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
    light.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.2);
    light.add_color_stop_rgba(0.4, 1.0, 1.0, 1.0, 0.0);
    light.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.3);
    let _ = cr.set_source(&light);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.4);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

fn ticks(
    cr: &gtk::cairo::Context,
    fg: &gtk::gdk::RGBA,
    (w, h): (f64, f64),
    adjustment: &gtk::Adjustment,
    rest: f64,
) {
    let value = adjustment.value();
    let reach = (w / 2.0 * FRAC_PI_2 / PX_PER_UNIT).ceil() as i64;
    let (r, g, b) = rgb(fg);
    let around = value.round() as i64;
    for tick in around - reach..=around + reach {
        let at = tick as f64;
        if at < adjustment.lower() || at > adjustment.upper() {
            continue;
        }
        let Some((x, depth)) = on_drum(at - value, w) else {
            continue;
        };
        let length = h * tick_share(tick, rest);
        cr.set_source_rgba(r, g, b, 0.85 * depth);
        cr.set_line_width(if tick % MAJOR_EVERY == 0 { 1.5 } else { 1.0 });
        cr.move_to(x, (h - length) / 2.0);
        cr.line_to(x, (h + length) / 2.0);
        let _ = cr.stroke();
    }
}

/// Where a tick `offset` units from the value sits on a drum filling `width`, and how much
/// it faces the viewer (1 in front, near 0 at the sides); `None` round the back.
fn on_drum(offset: f64, width: f64) -> Option<(f64, f64)> {
    let radius = width / 2.0;
    let angle = offset * PX_PER_UNIT / radius;
    (angle.abs() < FRAC_PI_2).then(|| (radius + radius * angle.sin(), angle.cos()))
}

/// How much of the height a tick takes: most where the value rests, more every few units.
fn tick_share(tick: i64, rest: f64) -> f64 {
    if (tick as f64 - rest).abs() < f64::EPSILON {
        0.72
    } else if tick % MAJOR_EVERY == 0 {
        0.5
    } else {
        0.28
    }
}

/// The fixed mark the ruler turns under, in the accent colour.
fn mark(cr: &gtk::cairo::Context, (w, h): (f64, f64)) {
    let accent = libadwaita::StyleManager::default().accent_color().to_rgba();
    let (r, g, b) = rgb(&accent);
    let x = (w / 2.0).round();
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(2.0);
    cr.move_to(x, 3.0);
    cr.line_to(x, h - 3.0);
    let _ = cr.stroke();
    for (y, dy) in [(0.0, 5.0), (h, -5.0)] {
        cr.move_to(x - 4.0, y);
        cr.line_to(x + 4.0, y);
        cr.line_to(x, y + dy);
        cr.close_path();
    }
    let _ = cr.fill();
}

fn rounded(cr: &gtk::cairo::Context, (w, h): (f64, f64), radius: f64) {
    let pi = std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(w - radius, radius, radius, -pi / 2.0, 0.0);
    cr.arc(w - radius, h - radius, radius, 0.0, pi / 2.0);
    cr.arc(radius, h - radius, radius, pi / 2.0, pi);
    cr.arc(radius, radius, radius, pi, 1.5 * pi);
    cr.close_path();
}

fn rgb(colour: &gtk::gdk::RGBA) -> (f64, f64, f64) {
    (
        f64::from(colour.red()),
        f64::from(colour.green()),
        f64::from(colour.blue()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_crowd_and_fade_towards_the_edges() {
        let (x, depth) = on_drum(0.0, 300.0).unwrap();
        assert!((x - 150.0).abs() < 1e-9 && (depth - 1.0).abs() < 1e-9);
        let (near, _) = on_drum(1.0, 300.0).unwrap();
        let (far, far_depth) = on_drum(15.0, 300.0).unwrap();
        let (farther, _) = on_drum(16.0, 300.0).unwrap();
        assert!((near - 150.0 - PX_PER_UNIT).abs() < 0.1);
        assert!(farther - far < near - 150.0);
        assert!(far_depth < 0.5);
        assert!(on_drum(40.0, 300.0).is_none());
    }

    #[test]
    fn fractions_of_a_wheel_click_add_up() {
        // A high-resolution wheel reports a quarter click at a time.
        let (mut moved, mut left) = (0.0, 0.0);
        for _ in 0..4 {
            let (turn, rest) = whole_steps(left + 0.25 * 0.1, 0.1);
            moved += turn;
            left = rest;
        }
        assert!((moved - 0.1).abs() < 1e-9);
        assert!((whole_steps(0.3, 0.1).0 - 0.3).abs() < 1e-9);
        let (back, _) = whole_steps(-0.25, 0.1);
        assert!((back + 0.2).abs() < 1e-9);
    }

    #[test]
    fn the_rest_tick_is_longest() {
        assert!(tick_share(0, 0.0) > tick_share(5, 0.0));
        assert!(tick_share(5, 0.0) > tick_share(3, 0.0));
    }
}
