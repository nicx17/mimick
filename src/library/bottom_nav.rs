use std::rc::Rc;

use gtk::prelude::*;

use super::LibraryWindowUi;

const ITEMS: [(&str, &str, &str); 5] = [
    ("photos", "Photos", "image-x-generic-symbolic"),
    ("explore", "Explore", "view-grid-symbolic"),
    ("map", "Map", "mark-location-symbolic"),
    ("albums", "Albums", "folder-pictures-symbolic"),
    ("people", "People", "avatar-default-symbolic"),
];

pub(super) struct BottomNav {
    pub root: gtk::Box,
    pub revealer: gtk::Revealer,
    buttons: Vec<(String, gtk::ToggleButton)>,
}

pub(super) fn build_bottom_nav() -> BottomNav {
    let row = gtk::Box::builder()
        .homogeneous(true)
        .spacing(4)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(8)
        .margin_end(8)
        .build();
    let buttons: Vec<(String, gtk::ToggleButton)> = ITEMS
        .iter()
        .map(|(key, label, icon)| (key.to_string(), nav_button(label, icon)))
        .collect();
    for (_, button) in &buttons {
        row.append(button);
    }
    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideUp)
        .reveal_child(true)
        .child(&row)
        .build();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .visible(false)
        .css_classes(["mimick-bottom-nav"])
        .build();
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    root.append(&revealer);
    BottomNav {
        root,
        revealer,
        buttons,
    }
}

fn nav_button(label: &str, icon: &str) -> gtk::ToggleButton {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .build();
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(
        &gtk::Label::builder()
            .label(label)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["caption"])
            .build(),
    );
    gtk::ToggleButton::builder()
        .child(&content)
        .tooltip_text(label)
        .css_classes(["flat"])
        .build()
}

pub(super) fn connect_bottom_nav(ui: &Rc<LibraryWindowUi>) {
    ui.bulk
        .bar
        .bind_property("reveal-child", &ui.bottom_nav.revealer, "reveal-child")
        .invert_boolean()
        .sync_create()
        .build();
    for (key, button) in &ui.bottom_nav.buttons {
        let (ui, key) = (ui.clone(), key.clone());
        button.connect_clicked(move |_| {
            if let Some(location) = super::navigation::location_for_key(&key) {
                super::navigation::navigate(&ui, location);
            }
        });
    }
}

pub(super) fn set_button_visible(nav: &BottomNav, key: &str, visible: bool) {
    for (button_key, button) in &nav.buttons {
        if button_key == key {
            button.set_visible(visible);
        }
    }
}

pub(super) fn sync(nav: &BottomNav, active_key: &str) {
    for (key, button) in &nav.buttons {
        button.set_active(key == active_key);
    }
}
