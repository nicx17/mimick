//! The search field over the Map page: pick a place and the map flies there.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

use crate::library::LibraryWindowUi;

use super::place_search::{
    self, Found, FoundPlace, MIN_QUERY_CHARS, PlaceSearch, SEARCH_DELAY_MS, SearchFooter,
};

const SEARCH_WIDTH: i32 = 360;
/// The map's side margins, the gap, and the button column on the right.
const CONTROLS_ROOM: i32 = 12 + 8 + 36 + 12;
const MAX_RESULTS_HEIGHT: i32 = 280;

pub struct MapSearch {
    pub root: gtk::Box,
    pub pin_layer: shumate::MarkerLayer,
    entry: gtk::SearchEntry,
    results: gtk::ListBox,
    card: gtk::Box,
    footer: SearchFooter,
    places: RefCell<Vec<FoundPlace>>,
    generation: Cell<u32>,
    searcher: OnceCell<Rc<PlaceSearch>>,
}

pub fn build(viewport: &shumate::Viewport) -> MapSearch {
    let entry = gtk::SearchEntry::builder()
        .placeholder_text("Search places")
        .search_delay(SEARCH_DELAY_MS)
        .css_classes(["mimick-map-search"])
        .build();
    let results = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["mimick-map-results"])
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .child(&results)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(MAX_RESULTS_HEIGHT)
        .build();
    let footer = place_search::search_footer();
    footer.root.add_css_class("mimick-map-credit");
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["mimick-map-card"])
        .visible(false)
        .build();
    card.append(&scroller);
    card.append(&footer.root);
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .margin_top(12)
        .margin_start(12)
        .build();
    root.append(&entry);
    root.append(&card);
    MapSearch {
        root,
        pin_layer: shumate::MarkerLayer::new(viewport),
        entry,
        results,
        card,
        footer,
        places: RefCell::default(),
        generation: Cell::new(0),
        searcher: OnceCell::new(),
    }
}

/// Keep the field clear of the buttons on narrow maps.
pub fn fit_width(ui: &LibraryWindowUi, width: i32) {
    let room = (width - CONTROLS_ROOM).clamp(0, SEARCH_WIDTH);
    ui.map.search.root.set_width_request(room);
}

/// The window's shared search, also used by the Set Location dialog.
pub fn searcher(ui: &LibraryWindowUi) -> Rc<PlaceSearch> {
    ui.map
        .search
        .searcher
        .get_or_init(|| PlaceSearch::new(ui.ctx.api_client.clone()))
        .clone()
}

/// Ctrl+F jumps to the field while a map is on screen, and selects what is there.
fn focus_field(ui: &Rc<LibraryWindowUi>) -> bool {
    let entry = &ui.map.search.entry;
    if !entry.is_mapped() || !crate::library::controls::no_dialog_open(ui) {
        return false;
    }
    entry.grab_focus();
    entry.select_region(0, -1);
    true
}

pub fn connect(ui: &Rc<LibraryWindowUi>) {
    crate::library::controls::add_shortcut(ui, "<Control>f", focus_field);
    let search = &ui.map.search;
    let weak = Rc::downgrade(ui);
    search.entry.connect_search_changed(move |entry| {
        if let Some(ui) = weak.upgrade() {
            find(&ui, entry.text().trim(), MIN_QUERY_CHARS);
        }
    });
    let weak = Rc::downgrade(ui);
    search.entry.connect_activate(move |entry| {
        if let Some(ui) = weak.upgrade() {
            submit(&ui, entry.text().trim());
        }
    });
    search.entry.connect_stop_search(|entry| entry.set_text(""));
    let weak = Rc::downgrade(ui);
    search.results.connect_row_activated(move |_, row| {
        if let (Some(ui), Ok(index)) = (weak.upgrade(), usize::try_from(row.index())) {
            choose(&ui, index);
        }
    });
}

/// Enter picks the first result once there is one, and searches short names right away.
fn submit(ui: &Rc<LibraryWindowUi>, query: &str) {
    if ui.map.search.card.is_visible() && !ui.map.search.places.borrow().is_empty() {
        choose(ui, 0);
    } else {
        find(ui, query, 1);
    }
}

fn find(ui: &Rc<LibraryWindowUi>, query: &str, min_chars: usize) {
    let search = &ui.map.search;
    let generation = search.generation.get().wrapping_add(1);
    search.generation.set(generation);
    if query.chars().count() < min_chars {
        search.show(None);
        if query.is_empty() {
            search.pin_layer.remove_all();
        }
        return;
    }
    let (searcher, bias) = (searcher(ui), place_search::bias_of(&ui.map.map));
    if let Some(found) = searcher.cached(query, bias) {
        search.show(Some(found));
        return;
    }
    search.searching();
    let weak = Rc::downgrade(ui);
    searcher.search(query.to_string(), bias, move |found| {
        if let Some(ui) = weak.upgrade()
            && ui.map.search.generation.get() == generation
        {
            ui.map.search.show(Some(found));
        }
    });
}

fn choose(ui: &Rc<LibraryWindowUi>, index: usize) {
    let search = &ui.map.search;
    let Some(place) = search.places.borrow().get(index).cloned() else {
        return;
    };
    search
        .generation
        .set(search.generation.get().wrapping_add(1));
    search.show(None);
    search.pin_layer.remove_all();
    search
        .pin_layer
        .add_marker(&super::markers::searched(place.point, &place.name));
    place_search::fly_to(&ui.map.map, &place);
    if let Some(map) = ui.map.map.map() {
        map.grab_focus();
    }
}

impl MapSearch {
    fn searching(&self) {
        self.footer.searching();
        self.card.set_visible(true);
    }

    fn show(&self, found: Option<Found>) {
        self.results.remove_all();
        let Some(found) = found else {
            self.card.set_visible(false);
            self.places.replace(Vec::new());
            return;
        };
        for place in &found.places {
            self.results.append(&place_search::place_row(place));
        }
        if found.places.is_empty() {
            self.results.append(&no_places_row());
        }
        self.footer.answered(found.source);
        self.card.set_visible(true);
        self.places.replace(found.places);
    }
}

fn no_places_row() -> libadwaita::ActionRow {
    libadwaita::ActionRow::builder()
        .title("No places found")
        .activatable(false)
        .build()
}
