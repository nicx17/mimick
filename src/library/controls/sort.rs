//! The sort dropdown: its options per view, and album display orders.

use gettextrs::gettext;
use std::rc::Rc;

use glib::clone;

use crate::api_client::SortOrder;
use crate::library::state::{LibrarySortMode, LibrarySource};
use crate::library::{albums_view, explore_view, timeline};

use super::{LibraryWindowUi, navigation};

/// The sort dropdown changes options per view and sorts the visible content.
pub(super) fn connect_sort_controls(ui: &Rc<LibraryWindowUi>) {
    // Rebuild the sort dropdown model when the visible content view changes:
    // each view has its own sort taxonomy.
    ui.content_stack.connect_visible_child_notify(clone!(
        #[strong]
        ui,
        move |stack| {
            let view = stack.visible_child_name();
            let labels = sort_labels_for_view(view.as_deref());
            let model =
                gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
            if view.as_deref() == Some("grid") && ui.interaction.album_sort_applied.get() {
                let mode = ui.ctx.library_state.lock().sort_mode.clone();
                ui.interaction.sort_suppressed.set(true);
                ui.sort_mode.set_model(Some(&model));
                ui.sort_mode.set_selected(grid_sort_index(&mode));
                ui.interaction.sort_suppressed.set(false);
            } else {
                ui.sort_mode.set_model(Some(&model));
                ui.sort_mode.set_selected(0);
            }
            // Switching views resets in-view filters: Albums resets its album filter,
            // Explore its people search, and the grid views reset both.
            if view.as_deref() != Some("explore") {
                albums_view::set_search_filter(&ui.albums, "");
            }
            if view.as_deref() != Some("albums") {
                explore_view::set_people_search(&ui.explore, "");
            }
        }
    ));

    ui.sort_mode.connect_selected_notify(clone!(
        #[strong]
        ui,
        move |dropdown| apply_sort_selection(&ui, dropdown.selected())
    ));
}

/// Sort options for the visible content view.
fn sort_labels_for_view(view: Option<&str>) -> Vec<String> {
    match view {
        Some("albums") => vec![gettext("Newest"), gettext("Name"), gettext("Most assets")],
        Some("explore") => vec![gettext("Default")],
        _ => vec![
            gettext("Newest"),
            gettext("Oldest"),
            gettext("Filename"),
            gettext("File Type"),
        ],
    }
}

fn grid_sort_mode(selected: u32) -> LibrarySortMode {
    match selected {
        1 => LibrarySortMode::OldestFirst,
        2 => LibrarySortMode::Filename,
        3 => LibrarySortMode::FileType,
        _ => LibrarySortMode::NewestFirst,
    }
}

fn grid_sort_index(mode: &LibrarySortMode) -> u32 {
    match mode {
        LibrarySortMode::NewestFirst => 0,
        LibrarySortMode::OldestFirst => 1,
        LibrarySortMode::Filename => 2,
        LibrarySortMode::FileType => 3,
    }
}

fn album_sort_mode(order: Option<SortOrder>) -> LibrarySortMode {
    if order == Some(SortOrder::Asc) {
        LibrarySortMode::OldestFirst
    } else {
        LibrarySortMode::NewestFirst
    }
}

/// Date order pages are fetched in; client-side sorts fetch in the server's default.
fn fetch_order(mode: &LibrarySortMode) -> SortOrder {
    mode.server_order().unwrap_or(SortOrder::Desc)
}

/// Opening an album selects its display order; leaving it undoes that.
/// Reloading the same source keeps whatever the user picked.
pub(in crate::library) fn sync_album_order(ui: &LibraryWindowUi, source: &LibrarySource) {
    if ui.ctx.library_state.lock().source == *source {
        return;
    }
    match album_order(ui, source) {
        Some(order) => apply_album_order(ui, order),
        None if ui.interaction.album_sort_applied.get() => {
            show_sort_mode(ui, LibrarySortMode::NewestFirst);
            ui.interaction.album_sort_applied.set(false);
        }
        None => {}
    }
}

pub(in crate::library) fn apply_album_order(ui: &LibraryWindowUi, order: Option<SortOrder>) {
    show_sort_mode(ui, album_sort_mode(order));
    ui.interaction.album_sort_applied.set(true);
}

fn album_order(ui: &LibraryWindowUi, source: &LibrarySource) -> Option<Option<SortOrder>> {
    let (LibrarySource::Album { id, .. }
    | LibrarySource::AlbumLocal { id, .. }
    | LibrarySource::AlbumUnified { id, .. }) = source
    else {
        return None;
    };
    let state = ui.ctx.library_state.lock();
    state
        .albums
        .iter()
        .find(|album| album.id == *id)
        .map(|album| album.order)
}

/// Set the sort without reloading, and show it if the grid's options are in the dropdown.
pub(in crate::library) fn show_sort_mode(ui: &LibraryWindowUi, mode: LibrarySortMode) {
    let index = grid_sort_index(&mode);
    ui.ctx.library_state.lock().apply_sort(mode);
    if ui.content_stack.visible_child_name().as_deref() == Some("grid") {
        ui.interaction.sort_suppressed.set(true);
        ui.sort_mode.set_selected(index);
        ui.interaction.sort_suppressed.set(false);
    }
}

/// Route a sort choice by active view. Albums uses an album-specific sort
/// taxonomy; Explore intentionally doesn't sort the people row (curated server order).
fn apply_sort_selection(ui: &Rc<LibraryWindowUi>, selected: u32) {
    if ui.interaction.sort_suppressed.get() {
        return;
    }
    let view = ui.content_stack.visible_child_name();
    if view.as_deref() == Some("albums") {
        let mode = match selected {
            1 => albums_view::AlbumsSort::Name,
            2 => albums_view::AlbumsSort::MostAssets,
            _ => albums_view::AlbumsSort::Newest,
        };
        albums_view::set_sort_mode(&ui.albums, mode);
        return;
    }
    ui.interaction.album_sort_applied.set(false);
    let sort_mode = grid_sort_mode(selected);
    let mut state = ui.ctx.library_state.lock();
    let order_changed = fetch_order(&state.sort_mode) != fetch_order(&sort_mode);
    state.apply_sort(sort_mode);
    if order_changed
        || timeline::eligible(&state.source, &state.sort_mode) != timeline::is_active(ui)
    {
        drop(state);
        navigation::reload_current(ui);
        return;
    }
    ui.grid
        .model
        .reset(&ui.ctx, &state.assets, &state.sort_mode);
}

#[cfg(test)]
mod tests {
    use super::{album_sort_mode, grid_sort_index, grid_sort_mode, sort_labels_for_view};
    use crate::api_client::SortOrder;
    use crate::library::state::LibrarySortMode;

    #[test]
    fn grid_sort_index_round_trips() {
        for i in 0..4 {
            assert_eq!(grid_sort_index(&grid_sort_mode(i)), i);
        }
        assert_eq!(grid_sort_mode(1), LibrarySortMode::OldestFirst);
    }

    #[test]
    fn album_order_picks_the_date_sort() {
        assert_eq!(
            album_sort_mode(Some(SortOrder::Asc)),
            LibrarySortMode::OldestFirst
        );
        assert_eq!(
            album_sort_mode(Some(SortOrder::Desc)),
            LibrarySortMode::NewestFirst
        );
        assert_eq!(album_sort_mode(None), LibrarySortMode::NewestFirst);
    }

    #[test]
    fn sort_labels_follow_the_visible_view() {
        assert_eq!(sort_labels_for_view(Some("albums"))[2], "Most assets");
        assert_eq!(sort_labels_for_view(Some("explore")), &["Default"]);
        assert_eq!(sort_labels_for_view(Some("grid"))[1], "Oldest");
        assert_eq!(sort_labels_for_view(None)[0], "Newest");
    }
}
