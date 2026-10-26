use std::rc::Rc;

use gtk::prelude::*;

use super::LibraryWindowUi;
use super::state::{LibraryPage, LibrarySource, LibraryState, Location};

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

pub(super) fn current(ui: &LibraryWindowUi) -> Location {
    ui.ctx.library_state.lock().location.clone()
}

pub(super) fn on_page(ui: &LibraryWindowUi) -> bool {
    matches!(current(ui), Location::Page(_))
}

fn show(ui: &Rc<LibraryWindowUi>, location: &Location) {
    let search_form =
        matches!(location, Location::Page(LibraryPage::Search)) || location.is_search();
    ui.search_view.root.set_reveal_child(search_form);
    match location {
        Location::Source(source) => {
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
    let map = Location::Page(LibraryPage::Map);
    let back_is_map = ui.ctx.library_state.lock().previous_location() == Some(&map);
    if back_is_map {
        go_back(ui);
    } else {
        navigate(ui, map);
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
        LibraryPage::Map => {}
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
        LibraryPage::Albums { shared_only: false } => "Albums".to_string(),
        LibraryPage::Albums { shared_only: true } => "Shared Albums".to_string(),
        LibraryPage::SharedLinks => "Shared Links".to_string(),
        LibraryPage::People => "People".to_string(),
        LibraryPage::Tags { path } => leaf(path, "Tags"),
        LibraryPage::Folders { path } => leaf(path, "Folders"),
        LibraryPage::Sharing => "Sharing".to_string(),
        LibraryPage::Search => "Search".to_string(),
        LibraryPage::Map => "Map".to_string(),
    }
}

pub(super) fn sync_controls(ui: &LibraryWindowUi, location: &Location) {
    let sort = match location {
        Location::Source(LibrarySource::Explore | LibrarySource::MapArea { .. }) => false,
        Location::Source(_) => true,
        Location::Page(LibraryPage::Albums { .. }) => true,
        Location::Page(_) => false,
    };
    ui.sort_mode.set_visible(sort);
    if matches!(location, Location::Page(_)) {
        ui.source_revealer.set_reveal_child(false);
    }
}

pub(super) fn update_back_button(ui: &LibraryWindowUi) {
    let can_go_back = ui.ctx.library_state.lock().can_go_back();
    ui.back_button.set_sensitive(can_go_back);
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
    ui.sidebar_suppressed.set(true);
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
    ui.sidebar_suppressed.set(false);
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
    let key = match location {
        Location::Page(page) => match page {
            LibraryPage::Albums { shared_only: false } => "albums",
            LibraryPage::Albums { shared_only: true } => "shared-albums",
            LibraryPage::SharedLinks => "shared-links",
            LibraryPage::People => "people",
            LibraryPage::Tags { .. } => "tags",
            LibraryPage::Folders { .. } => "folders",
            LibraryPage::Sharing => "sharing",
            LibraryPage::Search => "search",
            LibraryPage::Map => "map",
        },
        Location::Source(source) => match source {
            LibrarySource::Album { id, .. }
            | LibrarySource::AlbumLocal { id, .. }
            | LibrarySource::AlbumUnified { id, .. } => return SidebarTarget::Album(id.clone()),
            LibrarySource::AllAssets | LibrarySource::Unified | LibrarySource::LocalAll => "photos",
            LibrarySource::Explore | LibrarySource::Place { .. } | LibrarySource::Thing { .. } => {
                "explore"
            }
            LibrarySource::Person { .. } => "people",
            LibrarySource::Tag { .. } => "tags",
            LibrarySource::Folder { .. } => "folders",
            LibrarySource::MapArea { .. } => "map",
            LibrarySource::Favorites => "favorites",
            LibrarySource::Archive => "archive",
            LibrarySource::Trash => "trash",
            LibrarySource::SmartSearch { .. }
            | LibrarySource::MetadataSearch { .. }
            | LibrarySource::OcrSearch { .. }
            | LibrarySource::AdvancedSearch { .. }
            | LibrarySource::LocalSearch { .. }
            | LibrarySource::UnifiedSearch { .. } => "search",
        },
    };
    SidebarTarget::Fixed(key)
}

pub(super) fn location_for_key(key: &str) -> Option<Location> {
    let source = |source| Some(Location::Source(source));
    let page = |page| Some(Location::Page(page));
    match key {
        "photos" => source(LibrarySource::AllAssets),
        "explore" => source(LibrarySource::Explore),
        "map" => page(LibraryPage::Map),
        "favorites" => source(LibrarySource::Favorites),
        "archive" => source(LibrarySource::Archive),
        "trash" => source(LibrarySource::Trash),
        "search" => page(LibraryPage::Search),
        "people" => page(LibraryPage::People),
        "tags" => page(LibraryPage::Tags { path: None }),
        "folders" => page(LibraryPage::Folders { path: None }),
        "albums" => page(LibraryPage::Albums { shared_only: false }),
        "shared-albums" => page(LibraryPage::Albums { shared_only: true }),
        "shared-links" => page(LibraryPage::SharedLinks),
        "sharing" => page(LibraryPage::Sharing),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: [&str; 14] = [
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
