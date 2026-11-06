//! Menu entries that open related views of one photo: similar photos, and the photo in Photos.

use gettextrs::gettext;
use std::rc::Rc;

use gtk::prelude::*;

use super::LibraryWindowUi;
use super::asset_object::AssetObject;
use super::context_menu::Menu;
use super::state::{LibrarySortMode, LibrarySource, Location};

/// "View Similar Photos" and "View in Timeline", where each one applies.
pub(super) fn append_related_buttons(menu: &Menu, ui: &Rc<LibraryWindowUi>, item: &AssetObject) {
    let location = super::navigation::current(ui);
    if ui.features.get().smart_search && !is_similar_to(&location, &item.id()) {
        let (ui, id, filename) = (ui.clone(), item.id(), item.filename());
        menu.add_with_icon(
            "system-search-symbolic",
            &gettext("View Similar Photos"),
            false,
            move || view_similar(&ui, id.clone(), filename.clone()),
        );
    }
    if can_view_in_timeline(ui, item, &location) {
        let (ui, id, month) = (ui.clone(), item.id(), month_of(item));
        menu.add_with_icon(
            "x-office-calendar-symbolic",
            &gettext("View in Timeline"),
            false,
            move || view_in_timeline(&ui, id.clone(), month.clone()),
        );
    }
}

fn view_similar(ui: &Rc<LibraryWindowUi>, asset_id: String, filename: String) {
    leave_lightbox(ui);
    super::navigation::navigate_source(ui, LibrarySource::Similar { asset_id, filename });
}

/// Opens Photos at the photo's month and rings its tile once the month has loaded.
fn view_in_timeline(ui: &Rc<LibraryWindowUi>, asset_id: String, month: String) {
    leave_lightbox(ui);
    super::controls::show_sort_mode(ui, LibrarySortMode::NewestFirst);
    ui.interaction
        .timeline_reveal
        .replace(Some((asset_id, month)));
    super::navigation::navigate_source(ui, LibrarySource::AllAssets);
}

/// Photos lists only your own photos that aren't archived, as in Immich.
fn can_view_in_timeline(ui: &LibraryWindowUi, item: &AssetObject, location: &Location) -> bool {
    let in_photos = matches!(
        location,
        Location::Source(LibrarySource::AllAssets | LibrarySource::Unified)
    );
    !in_photos
        && !item.is_archived()
        && month_of(item).len() == 7
        && super::actions::is_mine(ui, item)
}

fn is_similar_to(location: &Location, asset_id: &str) -> bool {
    matches!(location, Location::Source(LibrarySource::Similar { asset_id: id, .. }) if id == asset_id)
}

/// `YYYY-MM` of the photo's local capture time, the key the timeline groups months by.
fn month_of(item: &AssetObject) -> String {
    item.local_date_time().chars().take(7).collect()
}

/// The grid behind the lightbox is about to change, so the lightbox closes first.
fn leave_lightbox(ui: &LibraryWindowUi) {
    if ui.nav.navigation_stack().n_items() > 1 {
        ui.nav.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similar_photos_are_not_offered_for_the_query_photo() {
        let location = Location::Source(LibrarySource::Similar {
            asset_id: "a".into(),
            filename: "a.jpg".into(),
        });
        assert!(is_similar_to(&location, "a"));
        assert!(!is_similar_to(&location, "b"));
        assert!(!is_similar_to(
            &Location::Source(LibrarySource::AllAssets),
            "a"
        ));
    }
}
