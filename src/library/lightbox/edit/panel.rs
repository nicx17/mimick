//! Edit mode's widgets: the Cancel and Save header, and the panel of tools under the photo.

use gettextrs::gettext;

use gtk::prelude::*;

use super::{Tool, ToolButton};
use crate::library::photo_editor::{ASPECTS, aspect_label};

/// Without the window buttons, so Cancel, the title, and Save fit a phone.
pub(super) fn edit_header(cancel: &gtk::Button, save: &gtk::Button) -> libadwaita::HeaderBar {
    let header = libadwaita::HeaderBar::builder()
        .show_back_button(false)
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .title_widget(&libadwaita::WindowTitle::new(&gettext("Edit Photo"), ""))
        .visible(false)
        .build();
    header.pack_start(cancel);
    header.pack_end(save);
    header
}

/// Crop shapes, the turn and flip tools, then Reset and Revert.
pub(super) fn edit_panel(
    shapes: &gtk::ScrolledWindow,
    tools: &[ToolButton],
    reset: &gtk::Button,
    revert: &gtk::Button,
) -> gtk::Box {
    // One row of equal buttons; on narrow windows their names wrap instead of the row.
    let tool_row = gtk::Box::builder()
        .spacing(6)
        .homogeneous(true)
        .halign(gtk::Align::Center)
        .build();
    for (button, _) in tools {
        tool_row.append(button);
    }
    let footer = gtk::Box::builder().spacing(6).build();
    footer.append(reset);
    footer.append(&gtk::Box::builder().hexpand(true).build());
    footer.append(revert);
    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .visible(false)
        .css_classes(["mimick-edit-panel"])
        .build();
    panel.append(shapes);
    panel.append(&tool_row);
    panel.append(&footer);
    panel
}

pub(super) fn tool_buttons() -> Vec<ToolButton> {
    let tools: [Tool; 4] = [
        ("mimick-rotate-left-symbolic", gettext("Rotate Left"), |s| {
            s.rotate(false)
        }),
        (
            "mimick-rotate-right-symbolic",
            gettext("Rotate Right"),
            |s| s.rotate(true),
        ),
        ("mimick-flip-horizontal-symbolic", gettext("Mirror"), |s| {
            s.flip(true)
        }),
        ("mimick-flip-vertical-symbolic", gettext("Flip"), |s| {
            s.flip(false)
        }),
    ];
    tools
        .into_iter()
        .map(|(icon, label, action)| (tool_button(icon, &label), action))
        .collect()
}

/// An icon over its name, so each tool says what it does.
fn tool_button(icon: &str, label: &str) -> gtk::Button {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(
        &gtk::Label::builder()
            .label(label)
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["caption"])
            .build(),
    );
    gtk::Button::builder()
        .child(&content)
        .css_classes(["flat", "mimick-edit-tool"])
        .build()
}

/// The crop shapes as chips, always in view; the row scrolls sideways on narrow windows.
pub(super) fn shape_chips() -> (gtk::ScrolledWindow, Vec<gtk::ToggleButton>) {
    let row = gtk::Box::builder()
        .spacing(6)
        .halign(gtk::Align::Center)
        .build();
    let mut shapes: Vec<gtk::ToggleButton> = Vec::new();
    for (index, (_, ratio)) in ASPECTS.iter().enumerate() {
        let glyph = shape_glyph(*ratio);
        let content = gtk::Box::builder().spacing(6).build();
        content.append(&glyph);
        content.append(&gtk::Label::new(Some(&aspect_label(index))));
        let chip = gtk::ToggleButton::builder()
            .child(&content)
            .active(index == 0)
            .css_classes(["mimick-edit-chip"])
            .build();
        // The picture is drawn in the chip's text color, which changes when it's picked.
        chip.connect_toggled(move |_| glyph.queue_draw());
        if let Some(first) = shapes.first() {
            chip.set_group(Some(first));
        }
        row.append(&chip);
        shapes.push(chip);
    }
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .child(&row)
        .build();
    (scroller, shapes)
}

/// Room for a crop shape's picture in its chip, in pixels.
const GLYPH_ROOM: (i32, i32) = (20, 14);
/// The free shape is drawn as a plain, dashed rectangle of this width over height.
const FREE_GLYPH_RATIO: f64 = 1.4;

/// A small picture of the shape: the photo icon for Original, else an outline of the shape.
fn shape_glyph(ratio: Option<f64>) -> gtk::Widget {
    if ratio == Some(0.0) {
        return gtk::Image::from_icon_name("image-x-generic-symbolic").upcast();
    }
    let area = gtk::DrawingArea::builder()
        .content_width(GLYPH_ROOM.0)
        .content_height(GLYPH_ROOM.1)
        .valign(gtk::Align::Center)
        .build();
    area.set_draw_func(move |area, cr, width, height| {
        draw_shape(
            cr,
            &area.color(),
            ratio,
            (f64::from(width), f64::from(height)),
        );
    });
    area.upcast()
}

/// Outlines the shape in the chip's text color, dashed when the shape is free.
fn draw_shape(
    cr: &gtk::cairo::Context,
    color: &gtk::gdk::RGBA,
    ratio: Option<f64>,
    room: (f64, f64),
) {
    const LINE: f64 = 1.5;
    let (w, h) = glyph_box(
        ratio.unwrap_or(FREE_GLYPH_RATIO),
        (room.0 - LINE, room.1 - LINE),
    );
    cr.set_source_rgba(
        f64::from(color.red()),
        f64::from(color.green()),
        f64::from(color.blue()),
        f64::from(color.alpha()),
    );
    cr.set_line_width(LINE);
    if ratio.is_none() {
        cr.set_dash(&[3.0, 2.0], 0.0);
    }
    cr.rectangle((room.0 - w) / 2.0, (room.1 - h) / 2.0, w, h);
    let _ = cr.stroke();
}

/// The largest box of `ratio` (width over height) that fits in `room`.
fn glyph_box(ratio: f64, (w, h): (f64, f64)) -> (f64, f64) {
    if w / h > ratio {
        (h * ratio, h)
    } else {
        (w, w / ratio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_pictures_fill_their_room_in_their_own_shape() {
        assert_eq!(glyph_box(1.0, (20.0, 14.0)), (14.0, 14.0));
        assert_eq!(glyph_box(2.0, (20.0, 14.0)), (20.0, 10.0));
        let (w, h) = glyph_box(4.0 / 3.0, (20.0, 14.0));
        assert!((w - 56.0 / 3.0).abs() < 1e-9 && (h - 14.0).abs() < 1e-9);
    }
}
