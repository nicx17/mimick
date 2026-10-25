use std::rc::Rc;

use crate::api_client::LibraryFeatures;

use super::LibraryWindowUi;
use super::explore_view::ExploreSections;
use super::state::{LibraryPage, LibrarySource, Location};

pub(super) fn load(ui: &Rc<LibraryWindowUi>) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let features = ui.ctx.api_client.fetch_library_features().await;
        ui.features.set(features);
        let explore_changed = apply(&ui, features);
        let current = super::navigation::current(&ui);
        if !is_available(&features, &current) {
            super::navigation::navigate_source(&ui, LibrarySource::Timeline);
        } else if explore_changed && current == Location::Source(LibrarySource::Explore) {
            super::load_explore_landing(ui.clone());
        }
    });
}

fn apply(ui: &LibraryWindowUi, features: LibraryFeatures) -> bool {
    let rows = [
        ("trash", features.trash),
        ("people", features.people),
        ("search", features.search),
        ("tags", features.tags),
        ("folders", features.folders),
        ("shared-links", features.shared_links),
        ("map", features.map),
    ];
    for (key, visible) in rows {
        super::sidebar::set_row_visible(&ui.sidebar, key, visible);
    }
    super::bottom_nav::set_button_visible(&ui.bottom_nav, "people", features.people);
    super::bottom_nav::set_button_visible(&ui.bottom_nav, "map", features.map);
    super::search_view::set_available_modes(&ui.search_view, features.smart_search, features.ocr);
    super::search_view::set_filter_features(
        &ui.search_view,
        features.ocr,
        features.ratings,
        features.tags,
    );
    let sections = ExploreSections {
        people: features.people,
        places: features.places,
        things: features.smart_search,
    };
    let changed = ui.explore.sections.replace(sections) != sections;
    if changed {
        ui.explore.populated.set(false);
    }
    changed
}

pub(super) fn is_available(features: &LibraryFeatures, location: &Location) -> bool {
    match location {
        Location::Source(LibrarySource::Trash) => features.trash,
        Location::Source(LibrarySource::Person { .. }) | Location::Page(LibraryPage::People) => {
            features.people
        }
        Location::Source(LibrarySource::Tag { .. }) | Location::Page(LibraryPage::Tags { .. }) => {
            features.tags
        }
        Location::Source(LibrarySource::Folder { .. })
        | Location::Page(LibraryPage::Folders { .. }) => features.folders,
        Location::Page(LibraryPage::SharedLinks) => features.shared_links,
        Location::Page(LibraryPage::Search) => features.search,
        Location::Source(LibrarySource::Place { .. }) => features.places,
        Location::Source(LibrarySource::Thing { .. } | LibrarySource::SmartSearch { .. }) => {
            features.smart_search
        }
        Location::Source(LibrarySource::OcrSearch { .. }) => features.ocr,
        Location::Source(LibrarySource::MapArea { .. }) | Location::Page(LibraryPage::Map) => {
            features.map
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turned_off_features_make_their_views_unavailable() {
        let features = LibraryFeatures {
            trash: false,
            people: false,
            tags: false,
            ..LibraryFeatures::default()
        };
        let person = Location::Source(LibrarySource::Person {
            id: "p".into(),
            name: "Ana".into(),
        });
        assert!(!is_available(&features, &person));
        assert!(!is_available(
            &features,
            &Location::Source(LibrarySource::Trash)
        ));
        assert!(!is_available(
            &features,
            &Location::Page(LibraryPage::Tags { path: None })
        ));
        assert!(is_available(
            &features,
            &Location::Source(LibrarySource::Favorites)
        ));
        assert!(is_available(
            &LibraryFeatures::default(),
            &Location::Page(LibraryPage::People)
        ));
    }
}
