//! A round badge that flashes over the middle of the video when it plays, pauses, or
//! changes volume, then grows and fades away, as video sites show it.

use std::cell::Cell;

use gtk::prelude::*;

/// Two classes running the same keyframes under different names: switching between them
/// is what makes GTK restart the animation for a quick second flash.
const FLASH_CLASSES: [&str; 2] = ["mimick-osd-flash-a", "mimick-osd-flash-b"];

pub(super) struct Osd {
    pub(super) root: gtk::Box,
    icon: gtk::Image,
    label: gtk::Label,
    flip: Cell<bool>,
}

impl Osd {
    pub(super) fn new() -> Self {
        let icon = gtk::Image::builder().pixel_size(40).build();
        let label = gtk::Label::builder()
            .css_classes(["heading"])
            .visible(false)
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            // Clicks and taps go through to the video underneath.
            .can_target(false)
            .css_classes(["mimick-video-osd"])
            .build();
        root.append(&icon);
        root.append(&label);
        Self {
            root,
            icon,
            label,
            flip: Cell::new(false),
        }
    }

    /// Show `icon`, with `text` under it when given, and let it fade.
    pub(super) fn flash(&self, icon: &str, text: Option<&str>) {
        self.icon.set_icon_name(Some(icon));
        self.label.set_label(text.unwrap_or_default());
        self.label.set_visible(text.is_some());
        let flip = !self.flip.get();
        self.flip.set(flip);
        self.root
            .remove_css_class(FLASH_CLASSES[usize::from(!flip)]);
        self.root.add_css_class(FLASH_CLASSES[usize::from(flip)]);
    }
}
