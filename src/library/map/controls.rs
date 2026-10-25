//! Buttons over the Map page: zoom, rotation, photos on screen, your location, and map style.

use std::f64::consts::TAU;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use crate::library::LibraryWindowUi;
use crate::library::context_menu::show_alert_dialog;

use super::camera::{self, ROTATE_STEP};
use super::styles::MapStyle;
use super::{locate, view};

const NORTH_TOLERANCE: f64 = 1e-3;

pub struct MapControls {
    pub root: gtk::Box,
    zoom_in: gtk::Button,
    zoom_out: gtk::Button,
    rotate_left: gtk::Button,
    rotate_right: gtk::Button,
    north: gtk::Button,
    area: gtk::Button,
    locate: gtk::Button,
}

pub fn build(viewport: &shumate::Viewport) -> MapControls {
    let controls = MapControls {
        root: gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .margin_top(12)
            .margin_end(12)
            .build(),
        zoom_in: control("zoom-in-symbolic", "Zoom in"),
        zoom_out: control("zoom-out-symbolic", "Zoom out"),
        rotate_left: control("object-rotate-left-symbolic", "Rotate left"),
        rotate_right: control("object-rotate-right-symbolic", "Rotate right"),
        north: north_button(viewport),
        area: control("image-x-generic-symbolic", "Show photos in this area"),
        locate: control("find-location-symbolic", "Show my location"),
    };
    let style = gtk::MenuButton::builder()
        .icon_name("mimick-layers-symbolic")
        .tooltip_text("Map style")
        .menu_model(&style_menu())
        .css_classes(["osd"])
        .build();
    controls
        .root
        .append(&group(&[&controls.zoom_in, &controls.zoom_out]));
    controls
        .root
        .append(&group(&[&controls.rotate_left, &controls.rotate_right]));
    controls.root.append(&controls.north);
    controls
        .root
        .append(&group(&[&controls.area, &controls.locate]));
    controls.root.append(&style);
    controls
}

fn control(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .css_classes(["osd"])
        .build()
}

/// A compass showing which way north is; only shown while the map is turned.
fn north_button(viewport: &shumate::Viewport) -> gtk::Button {
    gtk::Button::builder()
        .child(&shumate::Compass::new(Some(viewport)))
        .tooltip_text("Point north")
        .css_classes(["osd", "circular"])
        .halign(gtk::Align::Center)
        .visible(false)
        .build()
}

fn group(buttons: &[&gtk::Button]) -> gtk::Box {
    let group = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["linked"])
        .build();
    for button in buttons {
        group.append(*button);
    }
    group
}

fn style_menu() -> gio::Menu {
    let sections: [&[MapStyle]; 3] = [
        &[MapStyle::Immich],
        &[
            MapStyle::Colorful,
            MapStyle::Natural,
            MapStyle::Muted,
            MapStyle::Gray,
            MapStyle::Toner,
        ],
        &[MapStyle::OpenStreetMap],
    ];
    let menu = gio::Menu::new();
    for styles in sections {
        let section = gio::Menu::new();
        for style in styles {
            let action = format!("map.style::{}", style.id());
            section.append(Some(style.label()), Some(&action));
        }
        menu.append_section(None, &section);
    }
    menu
}

pub fn connect(ui: &Rc<LibraryWindowUi>) {
    let controls = &ui.map.controls;
    on_click(&controls.zoom_in, ui, |ui| {
        if let Some(map) = ui.map.map.map() {
            map.zoom_in();
        }
    });
    on_click(&controls.zoom_out, ui, |ui| {
        if let Some(map) = ui.map.map.map() {
            map.zoom_out();
        }
    });
    on_click(&controls.rotate_left, ui, |ui| {
        camera::rotate_by(&ui.map.map, -ROTATE_STEP)
    });
    on_click(&controls.rotate_right, ui, |ui| {
        camera::rotate_by(&ui.map.map, ROTATE_STEP)
    });
    on_click(&controls.north, ui, |ui| camera::reset_north(&ui.map.map));
    on_click(&controls.area, ui, show_area);
    on_click(&controls.locate, ui, locate_me);
    install_style_action(ui);
}

fn on_click(button: &gtk::Button, ui: &Rc<LibraryWindowUi>, action: fn(&Rc<LibraryWindowUi>)) {
    let weak = Rc::downgrade(ui);
    button.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            action(&ui);
        }
    });
}

/// Show the north button only while the map is turned away from north.
pub fn sync_north(ui: &LibraryWindowUi) {
    let Some(viewport) = ui.map.map.viewport() else {
        return;
    };
    let rotation = viewport.rotation().rem_euclid(TAU);
    let turned = rotation > NORTH_TOLERANCE && rotation < TAU - NORTH_TOLERANCE;
    ui.map.controls.north.set_visible(turned);
}

fn show_area(ui: &Rc<LibraryWindowUi>) {
    if !view::open_visible(ui) {
        toast(ui, "No photos in this area");
    }
}

fn locate_me(ui: &Rc<LibraryWindowUi>) {
    let button = ui.map.controls.locate.clone();
    button.set_sensitive(false);
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match locate::current_position(&ui.window).await {
            Ok(point) => view::show_here(&ui, point),
            Err(err) => show_alert_dialog(&ui, "Location Unavailable", &err),
        }
        button.set_sensitive(true);
    });
}

fn install_style_action(ui: &Rc<LibraryWindowUi>) {
    let current = ui.map.tiles.style().id().to_variant();
    let action = gio::SimpleAction::new_stateful("style", Some(glib::VariantTy::STRING), &current);
    let weak = Rc::downgrade(ui);
    action.connect_change_state(move |action, value| {
        let (Some(ui), Some(value)) = (weak.upgrade(), value) else {
            return;
        };
        let Some(id) = value.get::<String>() else {
            return;
        };
        action.set_state(value);
        choose_style(&ui, MapStyle::from_id(&id));
    });
    let actions = gio::SimpleActionGroup::new();
    actions.add_action(&action);
    ui.map.root.insert_action_group("map", Some(&actions));
}

fn choose_style(ui: &Rc<LibraryWindowUi>, style: MapStyle) {
    ui.map.tiles.set_style(ui, style);
    let mut config = ui.ctx.config.write();
    config.data.library_map_style = style.id().to_string();
    if !config.save() {
        log::warn!("Could not save the map style");
    }
}

fn toast(ui: &LibraryWindowUi, message: &str) {
    ui.map.root.add_toast(libadwaita::Toast::new(message));
}
