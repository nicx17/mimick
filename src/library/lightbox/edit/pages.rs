//! The tools' pages. Each is one or two short rows, so the photo keeps most of a phone's
//! height; rows of choices scroll sideways rather than wrap.

use gettextrs::gettext;

use gtk::prelude::*;

use super::EditAction;
use super::panel::Dial;
use crate::i18n::gettext_noop;
use crate::library::photo_editor::look::{FILTERS, SLIDERS};
use crate::library::photo_editor::{ASPECTS, MAX_STRAIGHTEN, aspect_label};

/// Turns, flips, and crop shapes, then the Straighten wheel.
pub(super) struct CropPage {
    pub(super) root: gtk::Box,
    pub(super) shapes: Vec<gtk::ToggleButton>,
    pub(super) turns: Vec<(gtk::Button, EditAction)>,
    pub(super) straighten: Dial,
}

/// One chip per slider, and the slider of the chip that's picked.
pub(super) struct AdjustPage {
    pub(super) root: gtk::Box,
    /// In `SLIDERS` order.
    pub(super) chips: Vec<gtk::ToggleButton>,
    pub(super) dial: Dial,
}

/// A tile per filter showing the photo with it, and how strongly the picked one shows.
pub(super) struct FiltersPage {
    pub(super) root: gtk::Box,
    /// In `FILTERS` order.
    pub(super) tiles: Vec<(gtk::ToggleButton, gtk::Picture)>,
    pub(super) strength: Dial,
}

/// How to hide, the bar's colour, and what to hide or remove at once.
pub(super) struct RedactPage {
    pub(super) root: gtk::Box,
    /// In `REDACT_STYLES` order.
    pub(super) styles: Vec<gtk::ToggleButton>,
    pub(super) colour: gtk::ColorDialogButton,
    pub(super) hint: gtk::Label,
    pub(super) hide_faces: gtk::Button,
    pub(super) hide_text: gtk::Button,
    pub(super) remove: gtk::Button,
    pub(super) clear: gtk::Button,
}

/// The ways to hide an area, in the order offered; the last is a solid bar.
pub(super) const REDACT_STYLES: [&str; 4] = [
    gettext_noop("Pixelate"),
    gettext_noop("Blur"),
    gettext_noop("Frosted"),
    gettext_noop("Bar"),
];

pub(super) fn crop_page() -> CropPage {
    let (shape_row, shapes) = shape_chips();
    let turns = turn_buttons();
    // A tenth of a degree per step, a whole degree with Shift.
    let straighten = Dial::wheel(
        &gettext("Straighten"),
        (-MAX_STRAIGHTEN, MAX_STRAIGHTEN),
        0.0,
        (0.1, 1.0),
    );
    // Turns and flips lead the shapes; the wheel gets a row of its own, for fine turns.
    let row = gtk::Box::builder().spacing(6).build();
    for (button, _) in &turns {
        row.append(button);
    }
    shape_row.set_hexpand(true);
    row.append(&shape_row);
    let root = page_box();
    root.append(&row);
    root.append(&straighten.root);
    CropPage {
        root,
        shapes,
        turns,
        straighten,
    }
}

pub(super) fn adjust_page() -> AdjustPage {
    let names: Vec<String> = SLIDERS.iter().map(|s| s.name()).collect();
    let (row, chips) = chip_row(&names);
    let row = sideways(&row);
    let first = SLIDERS[0];
    let dial = Dial::new(&first.name(), (first.min(), 100.0), 0.0);
    let root = page_box();
    root.append(&row);
    root.append(&dial.root);
    AdjustPage { root, chips, dial }
}

pub(super) fn filters_page() -> FiltersPage {
    let row = gtk::Box::builder().spacing(6).build();
    let mut tiles: Vec<(gtk::ToggleButton, gtk::Picture)> = Vec::new();
    for (index, (_, name, _)) in FILTERS.iter().enumerate() {
        let (tile, picture) = filter_tile(&gettext(*name), index == 0);
        if let Some((first, _)) = tiles.first() {
            tile.set_group(Some(first));
        }
        row.append(&tile);
        tiles.push((tile, picture));
    }
    let strength = Dial::new(&gettext("Strength"), (0.0, 100.0), 100.0);
    strength.root.set_visible(false);
    let root = page_box();
    root.append(&sideways(&row));
    root.append(&strength.root);
    FiltersPage {
        root,
        tiles,
        strength,
    }
}

pub(super) fn redact_page() -> RedactPage {
    let hint = gtk::Label::builder()
        .label(gettext(
            "Drag over what to hide. Tap a box to move, resize, or change it.",
        ))
        .wrap(true)
        .xalign(0.0)
        .css_classes(["caption", "dim-label"])
        .build();
    let names: Vec<String> = REDACT_STYLES.iter().map(|name| gettext(*name)).collect();
    let (style_row, styles) = chip_row(&names);
    let colour = bar_colour_button();
    style_row.append(&colour);
    let hide_faces = action_chip(&gettext("Hide Faces"));
    let hide_text = action_chip(&gettext("Hide Text"));
    let remove = gtk::Button::builder()
        .label(gettext("Remove"))
        .visible(false)
        .css_classes(["flat", "mimick-edit-revert"])
        .build();
    let clear = gtk::Button::builder()
        .label(gettext("Clear All"))
        .sensitive(false)
        .css_classes(["flat"])
        .build();
    let actions = gtk::Box::builder().spacing(6).build();
    for button in [&hide_faces, &hide_text, &remove, &clear] {
        actions.append(button);
    }
    let root = page_box();
    root.append(&hint);
    root.append(&sideways(&style_row));
    root.append(&sideways(&actions));
    RedactPage {
        root,
        styles,
        colour,
        hint,
        hide_faces,
        hide_text,
        remove,
        clear,
    }
}

/// Picks the colour of bars; black at first, hidden until Bar is picked.
fn bar_colour_button() -> gtk::ColorDialogButton {
    let colour = gtk::ColorDialogButton::new(Some(
        gtk::ColorDialog::builder()
            .title(gettext("Bar Colour"))
            .with_alpha(false)
            .build(),
    ));
    colour.set_rgba(&gtk::gdk::RGBA::BLACK);
    colour.set_visible(false);
    colour.set_tooltip_text(Some(&gettext("Bar Colour")));
    colour
}

fn action_chip(label: &str) -> gtk::Button {
    gtk::Button::builder()
        .label(label)
        .css_classes(["mimick-edit-chip"])
        .build()
}

fn page_box() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build()
}

/// A row that scrolls sideways when the window is too narrow for it.
fn sideways(row: &gtk::Box) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .child(row)
        .build()
}

/// Chips that pick one of `names`; the first starts picked.
fn chip_row(names: &[String]) -> (gtk::Box, Vec<gtk::ToggleButton>) {
    let row = gtk::Box::builder().spacing(6).build();
    let mut chips: Vec<gtk::ToggleButton> = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let chip = gtk::ToggleButton::builder()
            .label(name)
            .active(index == 0)
            .css_classes(["mimick-edit-chip"])
            .build();
        if let Some(first) = chips.first() {
            chip.set_group(Some(first));
        }
        row.append(&chip);
        chips.push(chip);
    }
    (row, chips)
}

/// Edge of a filter tile's picture, in pixels.
const TILE_EDGE: i32 = 56;

fn filter_tile(name: &str, active: bool) -> (gtk::ToggleButton, gtk::Picture) {
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .width_request(TILE_EDGE)
        .height_request(TILE_EDGE)
        .css_classes(["mimick-filter-picture"])
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    content.append(&picture);
    content.append(
        &gtk::Label::builder()
            .label(name)
            .css_classes(["caption"])
            .build(),
    );
    let tile = gtk::ToggleButton::builder()
        .child(&content)
        .active(active)
        .css_classes(["flat", "mimick-filter-tile"])
        .build();
    (tile, picture)
}

/// Turns and flips as icons, named for screen readers and on hover.
fn turn_buttons() -> Vec<(gtk::Button, EditAction)> {
    let turns: [(&str, String, EditAction); 4] = [
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
    turns
        .into_iter()
        .map(|(icon, name, action)| {
            let button = gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(&name)
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            button.update_property(&[gtk::accessible::Property::Label(&name)]);
            (button, action)
        })
        .collect()
}

/// The crop shapes as chips, always in view; the row scrolls sideways on narrow windows.
fn shape_chips() -> (gtk::ScrolledWindow, Vec<gtk::ToggleButton>) {
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
    (sideways(&row), shapes)
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
