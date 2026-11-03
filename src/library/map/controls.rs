//! Buttons over the Map page: zoom, rotation, photos on screen, your location, and map style.

use std::f64::consts::TAU;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use crate::library::LibraryWindowUi;
use crate::library::context_menu::show_error_toast;

use super::camera::{self, ROTATE_STEP};
use super::styles::{MapAppearance, MapStyle};
use super::{locate, view};

const NORTH_TOLERANCE: f64 = 1e-3;
/// Map heights below this show only the essential buttons.
const COMPACT_BELOW_PX: i32 = 380;

pub struct MapControls {
    pub root: gtk::Box,
    zoom_in: gtk::Button,
    zoom_out: gtk::Button,
    rotate_left: gtk::Button,
    rotate_right: gtk::Button,
    north: gtk::Button,
    /// The compass card, shown only while the map is turned.
    north_card: gtk::Box,
    area: gtk::Button,
    locate: gtk::Button,
    /// The zoom and rotate cards; hidden on short maps, where pinch and rotate gestures remain.
    optional: [gtk::Box; 2],
}

pub fn build(viewport: &shumate::Viewport) -> MapControls {
    let (zoom_in, zoom_out) = (
        control("zoom-in-symbolic", "Zoom in"),
        control("zoom-out-symbolic", "Zoom out"),
    );
    let (rotate_left, rotate_right) = (
        control("object-rotate-left-symbolic", "Rotate left"),
        control("object-rotate-right-symbolic", "Rotate right"),
    );
    let (area, locate) = (
        control("image-x-generic-symbolic", "Show photos in this area"),
        control("find-location-symbolic", "Show my location"),
    );
    let north = north_button(viewport);
    let north_card = card(&[north.upcast_ref()]);
    north_card.set_visible(false);
    let optional = [
        card(&[zoom_in.upcast_ref(), zoom_out.upcast_ref()]),
        card(&[rotate_left.upcast_ref(), rotate_right.upcast_ref()]),
    ];
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .margin_top(12)
        .margin_end(12)
        .build();
    root.append(&optional[0]);
    root.append(&optional[1]);
    root.append(&north_card);
    root.append(&card(&[area.upcast_ref(), locate.upcast_ref()]));
    root.append(&card(&[style_button().upcast_ref()]));
    MapControls {
        root,
        zoom_in,
        zoom_out,
        rotate_left,
        rotate_right,
        north,
        north_card,
        area,
        locate,
        optional,
    }
}

fn style_button() -> gtk::MenuButton {
    gtk::MenuButton::builder()
        .icon_name("mimick-layers-symbolic")
        .tooltip_text("Map style")
        .menu_model(&style_menu())
        .css_classes(["flat", "mimick-map-control"])
        .build()
}

/// Drop the zoom and rotate buttons when the whole column would not fit beside a short map.
pub fn fit_height(ui: &LibraryWindowUi, height: i32) {
    let roomy = height >= COMPACT_BELOW_PX;
    for group in &ui.map.controls.optional {
        group.set_visible(roomy);
    }
}

fn control(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .css_classes(["flat", "mimick-map-control"])
        .build()
}

/// A compass showing which way north is; clicking it turns the map back.
fn north_button(viewport: &shumate::Viewport) -> gtk::Button {
    gtk::Button::builder()
        .child(&shumate::Compass::new(Some(viewport)))
        .tooltip_text("Point north")
        .css_classes(["flat", "mimick-map-control"])
        .build()
}

/// A rounded card of stacked square buttons, clipped so the corners stay clean.
fn card(buttons: &[&gtk::Widget]) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["mimick-map-card"])
        .build();
    for button in buttons {
        card.append(*button);
    }
    card
}

fn style_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append_section(None, &style_items(&[MapStyle::Immich]));
    let palettes = style_items(&MapStyle::PALETTES);
    palettes.append_submenu(Some("More Styles"), &style_items(&MapStyle::LOOKALIKES));
    menu.append_section(None, &palettes);
    menu.append_section(
        None,
        &style_items(&[MapStyle::Satellite, MapStyle::OpenStreetMap]),
    );
    menu.append_section(Some("Appearance"), &appearance_items());
    menu
}

fn appearance_items() -> gio::Menu {
    let items = gio::Menu::new();
    for appearance in MapAppearance::ALL {
        let action = format!("map.appearance::{}", appearance.id());
        items.append(Some(appearance.label()), Some(&action));
    }
    items
}

fn style_items(styles: &[MapStyle]) -> gio::Menu {
    let items = gio::Menu::new();
    for style in styles {
        let action = format!("map.style::{}", style.id());
        items.append(Some(style.label()), Some(&action));
    }
    items
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
    install_map_actions(ui);
    connect_keys(ui);
}

/// Plus and minus zoom, brackets rotate, N points north, and Ctrl+L finds you, while a map shows.
fn connect_keys(ui: &Rc<LibraryWindowUi>) {
    let keys = gtk::EventControllerKey::new();
    let weak = Rc::downgrade(ui);
    keys.connect_key_pressed(move |_, keyval, _, mods| {
        let handled = weak.upgrade().is_some_and(|ui| {
            ui.map.root.is_mapped()
                && crate::library::controls::no_dialog_open(&ui)
                && map_key(&ui, keyval, mods)
        });
        if handled {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    ui.window.add_controller(keys);
}

fn map_key(ui: &Rc<LibraryWindowUi>, keyval: gtk::gdk::Key, mods: gtk::gdk::ModifierType) -> bool {
    use gtk::gdk::{Key, ModifierType};
    if mods.intersects(ModifierType::ALT_MASK | ModifierType::SUPER_MASK) {
        return false;
    }
    let ctrl = mods.contains(ModifierType::CONTROL_MASK);
    match (ctrl, keyval.to_lower()) {
        (true, Key::l) if ui.map.controls.locate.is_sensitive() => locate_me(ui),
        (true, Key::l) => {}
        (false, Key::plus | Key::equal | Key::KP_Add) => zoom(ui, true),
        (false, Key::minus | Key::KP_Subtract) => zoom(ui, false),
        (false, Key::bracketleft) => camera::rotate_by(&ui.map.map, -ROTATE_STEP),
        (false, Key::bracketright) => camera::rotate_by(&ui.map.map, ROTATE_STEP),
        (false, Key::n) => camera::reset_north(&ui.map.map),
        _ => return false,
    }
    true
}

fn zoom(ui: &LibraryWindowUi, closer: bool) {
    if let Some(map) = ui.map.map.map() {
        if closer {
            map.zoom_in();
        } else {
            map.zoom_out();
        }
    }
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
    ui.map.controls.north_card.set_visible(turned);
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
            Err(err) => show_error_toast(&ui, "Your location isn't available", &err),
        }
        button.set_sensitive(true);
    });
}

fn install_map_actions(ui: &Rc<LibraryWindowUi>) {
    let tiles = &ui.map.tiles;
    let actions = gio::SimpleActionGroup::new();
    actions.add_action(&choice_action(ui, "style", tiles.style().id(), |ui, id| {
        choose_style(ui, MapStyle::from_id(id));
    }));
    actions.add_action(&choice_action(
        ui,
        "appearance",
        tiles.appearance().id(),
        |ui, id| {
            choose_appearance(ui, MapAppearance::from_id(id));
        },
    ));
    ui.map.root.insert_action_group("map", Some(&actions));
}

fn choice_action(
    ui: &Rc<LibraryWindowUi>,
    name: &str,
    current: &str,
    choose: fn(&Rc<LibraryWindowUi>, &str),
) -> gio::SimpleAction {
    let state = current.to_variant();
    let action = gio::SimpleAction::new_stateful(name, Some(glib::VariantTy::STRING), &state);
    let weak = Rc::downgrade(ui);
    action.connect_change_state(move |action, value| {
        let (Some(ui), Some(value)) = (weak.upgrade(), value) else {
            return;
        };
        let Some(id) = value.get::<String>() else {
            return;
        };
        action.set_state(value);
        choose(&ui, &id);
    });
    action
}

fn choose_style(ui: &Rc<LibraryWindowUi>, style: MapStyle) {
    ui.map.tiles.set_style(ui, style);
    let mut config = ui.ctx.config.write();
    config.data.library_map_style = style.id().to_string();
    if !config.save() {
        log::warn!("Could not save the map style");
    }
}

fn choose_appearance(ui: &Rc<LibraryWindowUi>, appearance: MapAppearance) {
    ui.map.tiles.set_appearance(appearance);
    let mut config = ui.ctx.config.write();
    config.data.library_map_appearance = appearance.id().to_string();
    if !config.save() {
        log::warn!("Could not save the map appearance");
    }
}

fn toast(ui: &LibraryWindowUi, message: &str) {
    ui.map.root.add_toast(libadwaita::Toast::new(message));
}
