//! Painting the boxes: face boxes, and the selection with its handles and crop shading.

use super::{Fit, Region, State};

pub(super) fn draw(cr: &gtk::cairo::Context, state: &State, fit: Fit) {
    let image = state.image.get();
    let photo = fit.to_widget(Region {
        x: 0.0,
        y: 0.0,
        w: image.0,
        h: image.1,
    });
    if state.dim_outside.get()
        && let Some(sel) = state.selection.get()
    {
        dim_outside(cr, photo, fit.to_widget(sel));
    }
    let highlighted = state.highlighted.borrow();
    let accent = libadwaita::StyleManager::default().accent_color().to_rgba();
    for (index, marker) in state.markers.borrow().iter().enumerate() {
        let lit = highlighted.contains(&index).then_some(&accent);
        if lit.is_some() || state.outline_all.get() {
            stroke_box(cr, fit.to_widget(*marker), lit);
        }
    }
    if let Some(sel) = state.selection.get() {
        draw_selection(cr, fit.to_widget(sel), state.dim_outside.get());
    }
}

fn dim_outside(cr: &gtk::cairo::Context, photo: Region, sel: Region) {
    cr.set_fill_rule(gtk::cairo::FillRule::EvenOdd);
    cr.rectangle(photo.x, photo.y, photo.w, photo.h);
    cr.rectangle(sel.x, sel.y, sel.w, sel.h);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
    let _ = cr.fill();
    cr.set_fill_rule(gtk::cairo::FillRule::Winding);
}

/// A face box: white with a dark halo, or thicker in the `lit` colour (the system accent)
/// when pointed at or picked.
fn stroke_box(cr: &gtk::cairo::Context, r: Region, lit: Option<&gtk::gdk::RGBA>) {
    let width = if lit.is_some() { 3.0 } else { 2.0 };
    rounded(cr, r, 6.0);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.45);
    cr.set_line_width(width + 2.0);
    let _ = cr.stroke_preserve();
    match lit {
        Some(color) => cr.set_source_rgba(
            f64::from(color.red()),
            f64::from(color.green()),
            f64::from(color.blue()),
            1.0,
        ),
        None => cr.set_source_rgba(1.0, 1.0, 1.0, 0.9),
    }
    cr.set_line_width(width);
    let _ = cr.stroke();
}

/// The editable box with corner handles; a crop also gets rule-of-thirds lines and a heavier,
/// outlined frame so it stands out on bright photos.
fn draw_selection(cr: &gtk::cairo::Context, r: Region, crop: bool) {
    // Border width, longest corner arm, and corner width.
    let (border, arm_max, handle) = if crop {
        (2.0, 28.0, 5.0)
    } else {
        (1.5, 18.0, 4.0)
    };
    cr.rectangle(r.x, r.y, r.w, r.h);
    stroke_white(cr, border, crop);
    if crop {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.45);
        cr.set_line_width(1.0);
        for i in 1..3 {
            let f = f64::from(i) / 3.0;
            cr.move_to(r.x + r.w * f, r.y);
            cr.line_to(r.x + r.w * f, r.y + r.h);
            cr.move_to(r.x, r.y + r.h * f);
            cr.line_to(r.x + r.w, r.y + r.h * f);
        }
        let _ = cr.stroke();
    }
    let arm = (r.w.min(r.h) / 4.0).clamp(8.0, arm_max);
    for (cx, cy, sx, sy) in [
        (r.x, r.y, 1.0, 1.0),
        (r.x + r.w, r.y, -1.0, 1.0),
        (r.x, r.y + r.h, 1.0, -1.0),
        (r.x + r.w, r.y + r.h, -1.0, -1.0),
    ] {
        cr.move_to(cx + sx * arm, cy);
        cr.line_to(cx, cy);
        cr.line_to(cx, cy + sy * arm);
    }
    stroke_white(cr, handle, crop);
}

/// Strokes the current path white, over a dark outline when `outlined`.
fn stroke_white(cr: &gtk::cairo::Context, width: f64, outlined: bool) {
    if outlined {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.5);
        cr.set_line_width(width + 2.0);
        let _ = cr.stroke_preserve();
    }
    cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    cr.set_line_width(width);
    let _ = cr.stroke();
}

fn rounded(cr: &gtk::cairo::Context, r: Region, radius: f64) {
    let radius = radius.min(r.w / 2.0).min(r.h / 2.0);
    let pi = std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(r.x + r.w - radius, r.y + radius, radius, -pi / 2.0, 0.0);
    cr.arc(
        r.x + r.w - radius,
        r.y + r.h - radius,
        radius,
        0.0,
        pi / 2.0,
    );
    cr.arc(r.x + radius, r.y + r.h - radius, radius, pi / 2.0, pi);
    cr.arc(r.x + radius, r.y + radius, radius, pi, 1.5 * pi);
    cr.close_path();
}
