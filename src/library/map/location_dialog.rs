//! "Set Location" dialog: search a place, click the map, or type or paste coordinates.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;
use shumate::prelude::*;

use crate::api_client::Place;
use crate::library::LibraryWindowUi;
use crate::library::context_menu::show_alert_dialog;

use super::coords::GeoPoint;

const PLACE_ZOOM: f64 = 12.0;
const WORLD_ZOOM: f64 = 2.0;
const WORLD_CENTRE: (f64, f64) = (20.0, 0.0);
const MIN_MAP_HEIGHT: i32 = 200;
const DIALOG_HEIGHT: i32 = 640;
const MAX_RESULTS_HEIGHT: i32 = 220;
const MAX_RESULTS: usize = 8;
const MIN_QUERY_CHARS: usize = 2;

/// The assets to move and where the pin starts.
pub struct LocationTarget {
    pub ids: Vec<String>,
    /// Dialog subtitle: a filename, or "3 photos".
    pub title: String,
    pub initial: Option<GeoPoint>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Map,
    Entry,
    Place,
}

/// Look up one asset's current position, then open the dialog for it.
pub fn set_location_for_asset(
    ui: Rc<LibraryWindowUi>,
    asset_id: String,
    filename: String,
    on_saved: Rc<dyn Fn()>,
) {
    glib::MainContext::default().spawn_local(async move {
        let details = match ui.ctx.api_client.fetch_asset_details(&asset_id).await {
            Ok(details) => details,
            Err(err) => return show_alert_dialog(&ui, "Could Not Load Info", &err),
        };
        let initial = details
            .exif_info
            .and_then(|exif| GeoPoint::new(exif.latitude?, exif.longitude?));
        let target = LocationTarget {
            ids: vec![asset_id],
            title: filename,
            initial,
        };
        set_location(ui, target, on_saved);
    });
}

pub fn set_location(ui: Rc<LibraryWindowUi>, target: LocationTarget, on_saved: Rc<dyn Fn()>) {
    let save = gtk::Button::builder()
        .label("Save")
        .sensitive(false)
        .css_classes(["suggested-action"])
        .build();
    let picker = Picker::new(save.clone());
    picker.connect(&ui);
    if let Some(point) = target.initial {
        picker.set_point(point, Origin::Place);
    }
    let dialog = dialog_shell(&target.title, &picker.content, &save);
    let dialog_ref = dialog.clone();
    let ui_save = ui.clone();
    save.connect_clicked(move |button| {
        if let Some(point) = picker.point.get() {
            let save = SaveRequest {
                ui: ui_save.clone(),
                ids: target.ids.clone(),
                dialog: dialog_ref.clone(),
                button: button.clone(),
                on_saved: on_saved.clone(),
            };
            save.send(point);
        }
    });
    dialog.present(Some(&ui.window));
}

fn dialog_shell(subtitle: &str, content: &gtk::Box, save: &gtk::Button) -> libadwaita::Dialog {
    let cancel = gtk::Button::builder().label("Cancel").build();
    let header = libadwaita::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .title_widget(&libadwaita::WindowTitle::new("Set Location", subtitle))
        .build();
    header.pack_start(&cancel);
    header.pack_end(save);
    let toolbar = libadwaita::ToolbarView::builder().content(content).build();
    toolbar.add_top_bar(&header);
    let dialog = libadwaita::Dialog::builder()
        .title("Set Location")
        .content_width(560)
        .content_height(DIALOG_HEIGHT)
        .child(&toolbar)
        .build();
    let dialog_ref = dialog.clone();
    cancel.connect_clicked(move |_| {
        dialog_ref.close();
    });
    dialog
}

struct SaveRequest {
    ui: Rc<LibraryWindowUi>,
    ids: Vec<String>,
    dialog: libadwaita::Dialog,
    button: gtk::Button,
    on_saved: Rc<dyn Fn()>,
}

impl SaveRequest {
    fn send(self, point: GeoPoint) {
        self.button.set_sensitive(false);
        glib::MainContext::default().spawn_local(async move {
            let api = &self.ui.ctx.api_client;
            match api
                .set_assets_location(&self.ids, point.lat, point.lon)
                .await
            {
                Ok(()) => {
                    self.dialog.close();
                    self.ui.map.populated.set(false);
                    (self.on_saved)();
                }
                Err(err) => {
                    self.button.set_sensitive(true);
                    show_alert_dialog(
                        &self.ui,
                        "Could Not Set Location",
                        &format!("Immich rejected the update: {err}"),
                    );
                }
            }
        });
    }
}

/// Search, map, and coordinate entry, kept in step around one chosen point.
struct Picker {
    content: gtk::Box,
    search: gtk::SearchEntry,
    results: gtk::ListBox,
    results_scroller: gtk::ScrolledWindow,
    places: RefCell<Vec<Place>>,
    search_generation: Cell<u32>,
    map: shumate::SimpleMap,
    layer: shumate::MarkerLayer,
    pin: RefCell<Option<shumate::Marker>>,
    coords: libadwaita::EntryRow,
    paste: gtk::Button,
    save: gtk::Button,
    point: Cell<Option<GeoPoint>>,
    syncing_entry: Cell<bool>,
}

impl Picker {
    fn new(save: gtk::Button) -> Rc<Self> {
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search places")
            .search_delay(400)
            .build();
        let (results, results_scroller) = results_list();
        let (map, layer) = picker_map();
        let (coords, paste) = coords_row();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        content.append(&search);
        content.append(&results_scroller);
        content.append(&map);
        content.append(&coords_list(&coords));
        Rc::new(Self {
            content,
            search,
            results,
            results_scroller,
            places: RefCell::default(),
            search_generation: Cell::new(0),
            map,
            layer,
            pin: RefCell::default(),
            coords,
            paste,
            save,
            point: Cell::new(None),
            syncing_entry: Cell::new(false),
        })
    }

    fn connect(self: &Rc<Self>, ui: &Rc<LibraryWindowUi>) {
        ui.map.tiles.attach(ui, &self.map);
        super::camera::hold_in_world(&self.map);
        self.connect_map_clicks();
        self.connect_coords();
        self.connect_search(ui);
    }

    fn set_point(&self, point: GeoPoint, origin: Origin) {
        self.point.set(Some(point));
        self.move_pin(point);
        if origin != Origin::Entry {
            self.syncing_entry.set(true);
            self.coords.set_text(&point.display());
            self.syncing_entry.set(false);
        }
        self.coords.remove_css_class("error");
        self.save.set_sensitive(true);
        if origin == Origin::Map {
            return;
        }
        if let (Some(map), Some(viewport)) = (self.map.map(), self.map.viewport()) {
            if origin == Origin::Place || viewport.zoom_level() < PLACE_ZOOM {
                viewport.set_zoom_level(PLACE_ZOOM);
            }
            map.center_on(point.lat, point.lon);
        }
    }

    fn move_pin(&self, point: GeoPoint) {
        let mut pin = self.pin.borrow_mut();
        match pin.as_ref() {
            Some(marker) => marker.set_location(point.lat, point.lon),
            None => {
                let marker = super::minimap::pin(point);
                self.layer.add_marker(&marker);
                *pin = Some(marker);
            }
        }
    }

    fn connect_map_clicks(self: &Rc<Self>) {
        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(self);
        click.connect_released(move |_, presses, x, y| {
            if let Some(picker) = weak.upgrade()
                && presses == 1
            {
                picker.place_at(x, y);
            }
        });
        self.map.add_controller(click);
    }

    fn place_at(&self, x: f64, y: f64) {
        let Some(viewport) = self.map.viewport() else {
            return;
        };
        let (lat, lon) = viewport.widget_coords_to_location(&self.map, x, y);
        if let Some(point) = GeoPoint::new(lat, lon) {
            self.set_point(point, Origin::Map);
        }
    }

    fn connect_coords(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.coords.connect_changed(move |_| {
            if let Some(picker) = weak.upgrade()
                && !picker.syncing_entry.get()
            {
                picker.read_entry();
            }
        });
        let weak = Rc::downgrade(self);
        self.paste.connect_clicked(move |_| {
            if let Some(picker) = weak.upgrade() {
                picker.paste_clipboard();
            }
        });
    }

    fn read_entry(&self) {
        let text = self.coords.text();
        match GeoPoint::parse(&text) {
            Some(point) => self.set_point(point, Origin::Entry),
            None => {
                self.save.set_sensitive(false);
                if text.trim().is_empty() {
                    self.coords.remove_css_class("error");
                } else {
                    self.coords.add_css_class("error");
                }
            }
        }
    }

    fn paste_clipboard(&self) {
        let coords = self.coords.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Ok(Some(text)) = coords.clipboard().read_text_future().await {
                coords.set_text(text.trim());
            }
        });
    }

    fn connect_search(self: &Rc<Self>, ui: &Rc<LibraryWindowUi>) {
        let (weak, ui) = (Rc::downgrade(self), ui.clone());
        self.search.connect_search_changed(move |entry| {
            if let Some(picker) = weak.upgrade() {
                picker.search_places(&ui, entry.text().trim().to_string());
            }
        });
        let weak = Rc::downgrade(self);
        self.results.connect_row_activated(move |_, row| {
            if let Some(picker) = weak.upgrade() {
                picker.choose_place(row.index());
            }
        });
    }

    fn search_places(self: &Rc<Self>, ui: &Rc<LibraryWindowUi>, query: String) {
        let generation = self.search_generation.get().wrapping_add(1);
        self.search_generation.set(generation);
        if query.chars().count() < MIN_QUERY_CHARS {
            self.show_places(Vec::new());
            return;
        }
        let (picker, api) = (self.clone(), ui.ctx.api_client.clone());
        glib::MainContext::default().spawn_local(async move {
            let places = api.search_places(&query).await.unwrap_or_else(|err| {
                log::warn!("Place search failed: {err}");
                Vec::new()
            });
            if picker.search_generation.get() == generation {
                picker.show_places(places);
            }
        });
    }

    fn show_places(&self, mut places: Vec<Place>) {
        places.truncate(MAX_RESULTS);
        self.results.remove_all();
        for place in &places {
            let row = libadwaita::ActionRow::builder()
                .title(&place.name)
                .subtitle(place_region(place))
                .use_markup(false)
                .activatable(true)
                .build();
            self.results.append(&row);
        }
        self.results_scroller.set_visible(!places.is_empty());
        self.places.replace(places);
    }

    fn choose_place(&self, index: i32) {
        let point = usize::try_from(index)
            .ok()
            .and_then(|i| self.places.borrow().get(i).cloned())
            .and_then(|place| GeoPoint::new(place.latitude, place.longitude));
        if let Some(point) = point {
            self.show_places(Vec::new());
            self.set_point(point, Origin::Place);
        }
    }
}

/// Search results, scrolling once there are more than fit above the map.
fn results_list() -> (gtk::ListBox, gtk::ScrolledWindow) {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(MAX_RESULTS_HEIGHT)
        .visible(false)
        .build();
    (list, scroller)
}

/// The map fills the dialog's spare height, down to a minimum on small screens.
fn picker_map() -> (shumate::SimpleMap, shumate::MarkerLayer) {
    let map = shumate::SimpleMap::builder()
        .height_request(MIN_MAP_HEIGHT)
        .vexpand(true)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["mimick-location-map"])
        .build();
    let viewport = map.viewport().expect("SimpleMap creates its viewport");
    viewport.set_zoom_level(WORLD_ZOOM);
    if let Some(inner) = map.map() {
        inner.center_on(WORLD_CENTRE.0, WORLD_CENTRE.1);
    }
    let layer = shumate::MarkerLayer::new(&viewport);
    map.add_overlay_layer(&layer);
    (map, layer)
}

fn coords_row() -> (libadwaita::EntryRow, gtk::Button) {
    let coords = libadwaita::EntryRow::builder()
        .title("Latitude, Longitude")
        .build();
    let paste = gtk::Button::builder()
        .icon_name("edit-paste-symbolic")
        .tooltip_text("Paste coordinates")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    coords.add_suffix(&paste);
    (coords, paste)
}

fn coords_list(coords: &libadwaita::EntryRow) -> gtk::ListBox {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list.append(coords);
    list
}

/// "District, Region" under a search result, skipping missing or repeated parts.
fn place_region(place: &Place) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in [&place.admin2name, &place.admin1name].into_iter().flatten() {
        let part = part.trim();
        if !part.is_empty() && part != place.name && !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(name: &str, admin2: Option<&str>, admin1: Option<&str>) -> Place {
        Place {
            name: name.into(),
            latitude: 0.0,
            longitude: 0.0,
            admin1name: admin1.map(Into::into),
            admin2name: admin2.map(Into::into),
        }
    }

    #[test]
    fn place_region_skips_missing_and_repeated_parts() {
        assert_eq!(
            place_region(&place("Sintra", Some("Lisboa"), Some("Lisbon"))),
            "Lisboa, Lisbon"
        );
        assert_eq!(
            place_region(&place("Porto", Some("Porto"), Some("Porto"))),
            ""
        );
        assert_eq!(
            place_region(&place("Faro", None, Some("Faro District"))),
            "Faro District"
        );
        assert_eq!(place_region(&place("X", Some(" "), None)), "");
    }
}
