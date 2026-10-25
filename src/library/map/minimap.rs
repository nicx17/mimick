//! Small map with a pin in the details pane's location section, reused from photo to photo.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;
use shumate::prelude::*;

use crate::library::LibraryWindowUi;
use crate::library::navigation;
use crate::library::state::{LibraryPage, Location};

use super::coords::GeoPoint;

const MINIMAP_ZOOM: f64 = 13.0;
const MINIMAP_HEIGHT: i32 = 180;

/// One mini map per window; moving it keeps its tiles instead of drawing a new map.
pub struct Minimap {
    button: gtk::Button,
    map: shumate::SimpleMap,
    pin: shumate::Marker,
    point: Rc<Cell<GeoPoint>>,
}

/// Add the mini map, centred on `point`, to `group`; clicking it opens the Map page there.
pub fn attach(ui: &Rc<LibraryWindowUi>, group: &libadwaita::PreferencesGroup, point: GeoPoint) {
    if !ui.features.get().map {
        return;
    }
    let mut slot = ui.map.minimap.borrow_mut();
    let minimap = slot.get_or_insert_with(|| build(ui, point));
    minimap.show(point);
    if minimap.button.parent().is_some() {
        minimap.button.unparent();
    }
    group.add(&minimap.button);
}

fn build(ui: &Rc<LibraryWindowUi>, point: GeoPoint) -> Minimap {
    let map = shumate::SimpleMap::builder()
        .show_zoom_buttons(false)
        .height_request(MINIMAP_HEIGHT)
        .can_target(false)
        .build();
    if let Some(scale) = map.scale() {
        scale.set_visible(false);
    }
    let pin = pin(point);
    if let Some(viewport) = map.viewport() {
        let layer = shumate::MarkerLayer::new(&viewport);
        layer.add_marker(&pin);
        map.add_overlay_layer(&layer);
    }
    ui.map.tiles.attach(ui, &map);
    let point = Rc::new(Cell::new(point));
    let button = gtk::Button::builder()
        .child(&map)
        .tooltip_text("Show on map")
        .margin_top(8)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["flat", "mimick-minimap"])
        .build();
    let (weak, current) = (Rc::downgrade(ui), point.clone());
    button.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            open_on_map(&ui, current.get());
        }
    });
    Minimap {
        button,
        map,
        pin,
        point,
    }
}

impl Minimap {
    fn show(&self, point: GeoPoint) {
        self.point.set(point);
        self.pin.set_location(point.lat, point.lon);
        if let (Some(inner), Some(viewport)) = (self.map.map(), self.map.viewport()) {
            viewport.set_zoom_level(MINIMAP_ZOOM);
            inner.center_on(point.lat, point.lon);
        }
    }
}

/// Round marker for one position.
pub fn pin(point: GeoPoint) -> shumate::Marker {
    let dot = gtk::Box::builder().css_classes(["mimick-map-pin"]).build();
    let marker = shumate::Marker::builder().child(&dot).build();
    marker.set_location(point.lat, point.lon);
    marker
}

fn open_on_map(ui: &Rc<LibraryWindowUi>, point: GeoPoint) {
    ui.nav.pop();
    navigation::navigate(ui, Location::Page(LibraryPage::Map));
    super::view::focus(ui, point);
}
