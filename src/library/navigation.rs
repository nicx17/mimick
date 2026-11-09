use gettextrs::gettext;
use std::rc::Rc;

use gtk::prelude::*;

use super::LibraryWindowUi;
use super::state::{
    DayRange, LargeFilesFilter, LibraryPage, LibrarySource, LibraryState, Location, OddDates,
};

#[derive(Debug, PartialEq, Eq)]
enum SidebarTarget {
    Fixed(&'static str),
    Album(String),
}

pub(super) fn navigate(ui: &Rc<LibraryWindowUi>, location: Location) {
    ui.ctx.library_state.lock().navigate_to(location.clone());
    show(ui, &location);
}

pub(super) fn navigate_source(ui: &Rc<LibraryWindowUi>, source: LibrarySource) {
    navigate(ui, Location::Source(source));
}

/// A utility's filter changed: show it in place, without a Back step per change.
pub(super) fn refine(ui: &Rc<LibraryWindowUi>, location: Location) {
    ui.ctx
        .library_state
        .lock()
        .replace_location(location.clone());
    show(ui, &location);
}

/// The bottom bar's tabs switch without stacking up Back steps, as phone tab bars do.
pub(super) fn switch_tab(ui: &Rc<LibraryWindowUi>, location: Location) {
    ui.ctx.library_state.lock().switch_tab(location.clone());
    show(ui, &location);
}

/// The Back button, swipe, Alt+Left, and mouse button: leave selection mode first, as
/// phones do, then step back.
pub(super) fn back(ui: &Rc<LibraryWindowUi>) -> bool {
    if ui.select_toggle.is_active() && ui.nav.navigation_stack().n_items() == 1 {
        ui.select_toggle.set_active(false);
        return true;
    }
    go_back(ui)
}

pub(super) fn go_back(ui: &Rc<LibraryWindowUi>) -> bool {
    if ui.nav.navigation_stack().n_items() > 1 {
        ui.nav.pop();
        return true;
    }
    let Some(location) = step_to_available(ui, LibraryState::navigate_back) else {
        return false;
    };
    show(ui, &location);
    true
}

fn step_to_available(
    ui: &LibraryWindowUi,
    step: fn(&mut LibraryState) -> Option<Location>,
) -> Option<Location> {
    let features = ui.features.get();
    let mut state = ui.ctx.library_state.lock();
    loop {
        let location = step(&mut state)?;
        if super::features::is_available(&features, &location) {
            return Some(location);
        }
    }
}

pub(super) fn go_forward(ui: &Rc<LibraryWindowUi>) -> bool {
    let Some(location) = step_to_available(ui, LibraryState::navigate_forward) else {
        return false;
    };
    show(ui, &location);
    true
}

pub(super) fn reload_current(ui: &Rc<LibraryWindowUi>) {
    let location = current(ui);
    show(ui, &location);
}

pub(super) fn restore(ui: &Rc<LibraryWindowUi>) {
    log::debug!("Library window restoring its last view");
    show(ui, &current(ui));
}

pub(super) fn current(ui: &LibraryWindowUi) -> Location {
    ui.ctx.library_state.lock().location.clone()
}

pub(super) fn on_page(ui: &LibraryWindowUi) -> bool {
    matches!(current(ui), Location::Page(_))
}

fn show(ui: &Rc<LibraryWindowUi>, location: &Location) {
    log::debug!("Library view -> {location:?}");
    // The grid is rebuilt, emptying any selection, so leave selection mode with it.
    ui.select_toggle.set_active(false);
    let search_form =
        matches!(location, Location::Page(LibraryPage::Search)) || location.is_search();
    ui.search_view.root.set_reveal_child(search_form);
    if !search_form {
        ui.search_view.filters_toggle.set_active(false);
    }
    match location {
        Location::Source(source) => {
            super::controls::sync_album_order(ui, source);
            let request = ui.ctx.library_state.lock().switch_source(source.clone());
            super::apply_source_ui_state(ui, &request.1);
            ui.grid.scrolled.vadjustment().set_value(0.0);
            super::source_header::show(ui, &request.1);
            super::load_source_page(ui.clone(), request, false);
        }
        Location::Page(page) => {
            set_view_title(ui, &page_title(page));
            show_page(ui, page);
        }
    }
    sync_controls(ui, location);
    super::map::sync_layout(ui, location);
    sync_sidebar(ui);
    update_back_button(ui);
}

/// Back to the map alone: a step back when the map is the previous view.
pub(super) fn close_map_area(ui: &Rc<LibraryWindowUi>) {
    let back_is_map = matches!(
        ui.ctx.library_state.lock().previous_location(),
        Some(Location::Page(LibraryPage::Map { .. }))
    );
    if back_is_map {
        go_back(ui);
    } else {
        let album = super::map::album_shown(ui);
        navigate(ui, Location::Page(LibraryPage::Map { album }));
    }
}

fn show_page(ui: &Rc<LibraryWindowUi>, page: &LibraryPage) {
    match page {
        LibraryPage::Albums { shared_only } => {
            super::albums_view::set_shared_only(&ui.albums, *shared_only);
            if ui.albums.populated.get() {
                ui.content_stack.set_visible_child_name("albums");
            } else {
                ui.content_stack.set_visible_child_name("loading");
                super::refresh_albums_view(ui.clone());
            }
        }
        LibraryPage::SharedLinks => super::shared_links_view::show(ui),
        LibraryPage::People => super::people_view::show(ui),
        LibraryPage::Tags { path } => super::tags_view::show(ui, path.clone()),
        LibraryPage::Folders { path } => super::folders_view::show(ui, path.clone()),
        LibraryPage::Sharing => super::sharing_view::show(ui),
        LibraryPage::Search => {
            ui.content_stack.set_visible_child_name("empty");
            ui.search_view.search_entry.grab_focus();
            super::refresh_search_tags(ui.clone(), None);
        }
        LibraryPage::Map { .. } => {}
        LibraryPage::Duplicates => super::duplicates_view::show(ui),
    }
}

/// "Map", or the album's name when the map shows only its photos.
fn map_title(album: Option<&super::state::MapAlbum>) -> String {
    match album {
        Some(album) => album.name.clone(),
        None => gettext("Map"),
    }
}

pub(super) fn set_view_title(ui: &LibraryWindowUi, title: &str) {
    if let Some(page) = ui
        .nav
        .navigation_stack()
        .item(0)
        .and_downcast::<libadwaita::NavigationPage>()
    {
        libadwaita::prelude::NavigationPageExt::set_title(&page, title);
    }
}

fn page_title(page: &LibraryPage) -> String {
    let leaf = |path: &Option<String>, root: &str| {
        path.as_deref()
            .map(super::tree_page::leaf_name)
            .unwrap_or(root)
            .to_string()
    };
    match page {
        LibraryPage::Albums { shared_only: false } => gettext("Albums"),
        LibraryPage::Albums { shared_only: true } => gettext("Shared Albums"),
        LibraryPage::SharedLinks => gettext("Shared Links"),
        LibraryPage::People => gettext("People"),
        LibraryPage::Tags { path } => leaf(path, &gettext("Tags")),
        LibraryPage::Folders { path } => leaf(path, &gettext("Folders")),
        LibraryPage::Sharing => gettext("Sharing"),
        LibraryPage::Search => gettext("Search"),
        LibraryPage::Map { album } => map_title(album.as_ref()),
        LibraryPage::Duplicates => gettext("Duplicates"),
    }
}

pub(super) fn sync_controls(ui: &LibraryWindowUi, location: &Location) {
    let sort = match location {
        // Large Files comes biggest first; a date or name order would hide that.
        Location::Source(
            LibrarySource::Explore
            | LibrarySource::MapArea { .. }
            | LibrarySource::Memory { .. }
            | LibrarySource::LargeFiles { .. },
        ) => false,
        Location::Source(_) => true,
        Location::Page(LibraryPage::Albums { .. }) => true,
        Location::Page(_) => false,
    };
    ui.sort_mode.set_visible(sort);
    super::utilities::sync(ui, location);
    if matches!(location, Location::Page(_)) {
        ui.source_revealer.set_reveal_child(false);
    }
    sync_upload_button(ui);
}

/// Upload only where new photos land in view: Photos, and albums this user can add to.
/// Also called when the album list loads, since that's where an album's editors come from.
pub(super) fn sync_upload_button(ui: &LibraryWindowUi) {
    let shown = match current(ui) {
        Location::Source(
            LibrarySource::AllAssets | LibrarySource::Unified | LibrarySource::LocalAll,
        ) => true,
        Location::Source(
            LibrarySource::Album { .. }
            | LibrarySource::AlbumLocal { .. }
            | LibrarySource::AlbumUnified { .. },
        ) => super::album_assets::can_remove_here(ui),
        _ => false,
    };
    ui.upload_button.set_visible(shown);
}

/// Also live while selecting, since Back then leaves selection mode.
pub(super) fn update_back_button(ui: &LibraryWindowUi) {
    let can_go_back = ui.ctx.library_state.lock().can_go_back();
    ui.back_button
        .set_sensitive(can_go_back || ui.select_toggle.is_active());
}

pub(super) fn sync_sidebar(ui: &LibraryWindowUi) {
    let target = sidebar_target(&current(ui));
    let sidebar = &ui.sidebar;
    super::bottom_nav::sync(
        &ui.bottom_nav,
        match &target {
            SidebarTarget::Fixed(key) => key,
            SidebarTarget::Album(_) => "albums",
        },
    );
    ui.interaction.sidebar_suppressed.set(true);
    match target {
        SidebarTarget::Fixed(key) => {
            super::sidebar::select_fixed(sidebar, key);
            sidebar.albums_list.unselect_all();
        }
        SidebarTarget::Album(id) => match super::sidebar::recent_row_for_album(sidebar, &id) {
            Some(row) => {
                super::sidebar::select_fixed_row(sidebar, &row);
                sidebar.albums_list.unselect_all();
            }
            None => {
                super::sidebar::unselect_fixed(sidebar);
                match album_row(&sidebar.albums_list, &id) {
                    Some(row) => sidebar.albums_list.select_row(Some(&row)),
                    None => sidebar.albums_list.unselect_all(),
                }
            }
        },
    }
    ui.interaction.sidebar_suppressed.set(false);
}

fn album_row(list: &gtk::ListBox, album_id: &str) -> Option<gtk::ListBoxRow> {
    let mut child = list.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Ok(row) = widget.downcast::<gtk::ListBoxRow>()
            && row
                .tooltip_text()
                .as_deref()
                .and_then(|tooltip| tooltip.split_once(':'))
                .is_some_and(|(id, _)| id == album_id)
        {
            return Some(row);
        }
    }
    None
}

fn sidebar_target(location: &Location) -> SidebarTarget {
    match location {
        Location::Page(page) => SidebarTarget::Fixed(page_key(page)),
        Location::Source(source) => source_target(source),
    }
}

fn page_key(page: &LibraryPage) -> &'static str {
    match page {
        LibraryPage::Albums { shared_only: false } => "albums",
        LibraryPage::Albums { shared_only: true } => "shared-albums",
        LibraryPage::SharedLinks => "shared-links",
        LibraryPage::People => "people",
        LibraryPage::Tags { .. } => "tags",
        LibraryPage::Folders { .. } => "folders",
        LibraryPage::Sharing => "sharing",
        LibraryPage::Search => "search",
        LibraryPage::Map { .. } => "map",
        LibraryPage::Duplicates => "duplicates",
    }
}

fn source_target(source: &LibrarySource) -> SidebarTarget {
    let key = match source {
        LibrarySource::Album { id, .. }
        | LibrarySource::AlbumLocal { id, .. }
        | LibrarySource::AlbumUnified { id, .. } => return SidebarTarget::Album(id.clone()),
        LibrarySource::AllAssets
        | LibrarySource::Unified
        | LibrarySource::LocalAll
        | LibrarySource::Memory { .. } => "photos",
        LibrarySource::Explore | LibrarySource::Place { .. } | LibrarySource::Thing { .. } => {
            "explore"
        }
        LibrarySource::Person { .. } => "people",
        LibrarySource::Partner { .. } => "sharing",
        LibrarySource::Tag { .. } => "tags",
        LibrarySource::Folder { .. } => "folders",
        LibrarySource::MapArea { .. } => "map",
        LibrarySource::Favorites => "favorites",
        LibrarySource::Archive => "archive",
        LibrarySource::Trash => "trash",
        LibrarySource::LargeFiles { .. }
        | LibrarySource::MissingLocation { .. }
        | LibrarySource::UnusualDates { .. } => super::utilities::sidebar_key(source),
        LibrarySource::SmartSearch { .. }
        | LibrarySource::MetadataSearch { .. }
        | LibrarySource::OcrSearch { .. }
        | LibrarySource::AdvancedSearch { .. }
        | LibrarySource::LocalSearch { .. }
        | LibrarySource::UnifiedSearch { .. }
        | LibrarySource::Similar { .. } => "search",
    };
    SidebarTarget::Fixed(key)
}

/// A sidebar key and the view it opens.
type KeyLocation = (&'static str, fn() -> Location);

/// Sidebar keys and where each one goes; a table, so adding a page doesn't grow a `match`.
const KEY_LOCATIONS: [KeyLocation; 18] = [
    ("photos", || Location::Source(LibrarySource::AllAssets)),
    ("explore", || Location::Source(LibrarySource::Explore)),
    ("map", || Location::Page(LibraryPage::Map { album: None })),
    ("favorites", || Location::Source(LibrarySource::Favorites)),
    ("archive", || Location::Source(LibrarySource::Archive)),
    ("trash", || Location::Source(LibrarySource::Trash)),
    ("search", || Location::Page(LibraryPage::Search)),
    ("people", || Location::Page(LibraryPage::People)),
    ("tags", || Location::Page(LibraryPage::Tags { path: None })),
    ("folders", || {
        Location::Page(LibraryPage::Folders { path: None })
    }),
    ("albums", || {
        Location::Page(LibraryPage::Albums { shared_only: false })
    }),
    ("shared-albums", || {
        Location::Page(LibraryPage::Albums { shared_only: true })
    }),
    ("shared-links", || Location::Page(LibraryPage::SharedLinks)),
    ("sharing", || Location::Page(LibraryPage::Sharing)),
    ("duplicates", || Location::Page(LibraryPage::Duplicates)),
    ("large-files", || {
        Location::Source(LibrarySource::LargeFiles {
            filter: LargeFilesFilter::default(),
        })
    }),
    ("missing-location", || {
        Location::Source(LibrarySource::MissingLocation {
            range: DayRange::default(),
        })
    }),
    ("unusual-dates", || {
        Location::Source(LibrarySource::UnusualDates {
            which: OddDates::default(),
        })
    }),
];

pub(super) fn location_for_key(key: &str) -> Option<Location> {
    KEY_LOCATIONS
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, location)| location())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: [&str; 18] = [
        "photos",
        "explore",
        "map",
        "favorites",
        "archive",
        "trash",
        "search",
        "people",
        "tags",
        "folders",
        "albums",
        "shared-albums",
        "shared-links",
        "sharing",
        "duplicates",
        "large-files",
        "missing-location",
        "unusual-dates",
    ];

    #[test]
    fn every_sidebar_key_round_trips() {
        for key in KEYS {
            let location = location_for_key(key).unwrap();
            assert_eq!(
                sidebar_target(&location),
                SidebarTarget::Fixed(key),
                "{key}"
            );
        }
        assert_eq!(location_for_key("nope"), None);
    }

    #[test]
    fn pages_are_titled_by_their_current_level() {
        assert_eq!(page_title(&LibraryPage::Tags { path: None }), "Tags");
        assert_eq!(
            page_title(&LibraryPage::Folders {
                path: Some("/photos/2024".into())
            }),
            "2024"
        );
        assert_eq!(
            page_title(&LibraryPage::Albums { shared_only: true }),
            "Shared Albums"
        );
    }

    #[test]
    fn opened_items_highlight_their_section() {
        let person = Location::Source(LibrarySource::Person {
            id: "p".into(),
            name: "Ana".into(),
        });
        assert_eq!(sidebar_target(&person), SidebarTarget::Fixed("people"));
        let partner = Location::Source(LibrarySource::Partner {
            id: "u2".into(),
            name: "Ana".into(),
        });
        assert_eq!(sidebar_target(&partner), SidebarTarget::Fixed("sharing"));
        let place = Location::Source(LibrarySource::Place {
            city: "Kyoto".into(),
        });
        assert_eq!(sidebar_target(&place), SidebarTarget::Fixed("explore"));
        let nested_tags = Location::Page(LibraryPage::Tags {
            path: Some("Trips".into()),
        });
        assert_eq!(sidebar_target(&nested_tags), SidebarTarget::Fixed("tags"));
        let album = Location::Source(LibrarySource::AlbumUnified {
            id: "a1".into(),
            name: "Trips".into(),
        });
        assert_eq!(sidebar_target(&album), SidebarTarget::Album("a1".into()));
    }
}
