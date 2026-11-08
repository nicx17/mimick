//! The Map page limited to one album's photos, with a chip naming the album that goes back to
//! every photo.

use gettextrs::gettext;
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::api_client::MapMarker;
use crate::config::MapFilters;
use crate::library::LibraryWindowUi;
use crate::library::navigation;
use crate::library::state::{LibraryPage, Location, MapAlbum};

/// The album the map shows, and its chip in the map's bottom-left corner.
pub struct AlbumScope {
    pub(super) chip: gtk::Button,
    label: gtk::Label,
    shown: RefCell<Option<MapAlbum>>,
}

impl AlbumScope {
    pub(super) fn new() -> Self {
        let label = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(24)
            .build();
        let content = gtk::Box::builder().spacing(6).build();
        content.append(&gtk::Image::from_icon_name("folder-pictures-symbolic"));
        content.append(&label);
        content.append(&gtk::Image::from_icon_name("window-close-symbolic"));
        let chip = gtk::Button::builder()
            .child(&content)
            .tooltip_text(gettext("Show all photos"))
            .halign(gtk::Align::Start)
            .valign(gtk::Align::End)
            .margin_start(12)
            .margin_bottom(12)
            .css_classes(["osd", "pill"])
            .visible(false)
            .build();
        Self {
            chip,
            label,
            shown: RefCell::default(),
        }
    }

    pub(super) fn shown(&self) -> Option<MapAlbum> {
        self.shown.borrow().clone()
    }
}

/// The album the Map page is limited to, if any.
pub fn album_shown(ui: &LibraryWindowUi) -> Option<MapAlbum> {
    ui.map.album.shown()
}

/// Take the album from the Map page just opened; true when the map now shows other photos.
/// A place's photos beside the map keep whatever the map showed.
pub(super) fn follow(ui: &LibraryWindowUi) -> bool {
    let Location::Page(LibraryPage::Map { album }) = navigation::current(ui) else {
        return false;
    };
    let scope = &ui.map.album;
    if *scope.shown.borrow() == album {
        return false;
    }
    let name = album.as_ref().map_or("", |album| album.name.as_str());
    scope.label.set_label(name);
    scope.chip.set_visible(album.is_some());
    scope.shown.replace(album);
    true
}

pub(super) fn connect(ui: &Rc<LibraryWindowUi>) {
    let weak = Rc::downgrade(ui);
    ui.map.album.chip.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            navigation::navigate(&ui, Location::Page(LibraryPage::Map { album: None }));
        }
    });
}

/// What the markers are loaded for: the library under Map Settings, or one album.
#[derive(Debug, PartialEq)]
pub(super) enum MarkerSource {
    Library(MapFilters),
    Album(String),
}

pub(super) fn marker_source(ui: &LibraryWindowUi) -> MarkerSource {
    match ui.map.album.shown() {
        Some(album) => MarkerSource::Album(album.id),
        None => MarkerSource::Library(ui.ctx.config.read().data.library_map_filters.clone()),
    }
}

pub(super) async fn fetch_markers(
    ui: &LibraryWindowUi,
    source: &MarkerSource,
) -> Result<Vec<MapMarker>, String> {
    let api = &ui.ctx.api_client;
    match source {
        MarkerSource::Library(settings) => {
            let filter = super::settings::marker_filter(settings, chrono::Local::now());
            api.fetch_map_markers(&filter).await
        }
        MarkerSource::Album(id) => api.fetch_album_map_markers(id).await,
    }
}

/// Open the Map page with only `album`'s photos.
pub fn show_album(ui: &Rc<LibraryWindowUi>, id: String, name: String) {
    let album = Some(MapAlbum { id, name });
    navigation::navigate(ui, Location::Page(LibraryPage::Map { album }));
}
