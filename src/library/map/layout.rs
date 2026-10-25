//! The map alone on the Map page, and beside (or above) the photo grid for a map area.

use std::rc::Rc;

use gtk::prelude::*;

use crate::library::LibraryWindowUi;
use crate::library::state::{LibraryPage, LibrarySource, Location};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Off,
    MapOnly,
    WithPhotos,
}

fn mode_for(location: &Location) -> Mode {
    match location {
        Location::Page(LibraryPage::Map) => Mode::MapOnly,
        Location::Source(LibrarySource::MapArea { .. }) => Mode::WithPhotos,
        _ => Mode::Off,
    }
}

/// Show or hide the map and the photo grid for the location just opened.
pub fn sync(ui: &Rc<LibraryWindowUi>, location: &Location) {
    let mode = mode_for(location);
    let previous = ui.map.mode.replace(mode);
    ui.map.root.set_visible(mode != Mode::Off);
    ui.content_stack.set_visible(mode != Mode::MapOnly);
    ui.controls_row.set_visible(mode == Mode::Off);
    if mode != Mode::Off {
        super::view::show(ui);
    }
    match (previous == Mode::WithPhotos, mode == Mode::WithPhotos) {
        (false, true) => open_panel(ui),
        (true, false) => close_panel(ui),
        _ => {}
    }
}

/// Re-split the window when a narrow layout turns the panel from beside to below the map.
pub fn connect(ui: &Rc<LibraryWindowUi>) {
    let weak = Rc::downgrade(ui);
    ui.map.split.connect_orientation_notify(move |split| {
        if weak
            .upgrade()
            .is_some_and(|ui| ui.map.mode.get() == Mode::WithPhotos)
        {
            place_divider(split);
        }
    });
}

fn open_panel(ui: &LibraryWindowUi) {
    place_divider(&ui.map.split);
    if !ui.split.is_collapsed() && ui.split.shows_sidebar() {
        ui.split.set_show_sidebar(false);
        ui.map.sidebar_restore.set(true);
    }
}

fn close_panel(ui: &Rc<LibraryWindowUi>) {
    if ui.map.sidebar_restore.replace(false) {
        ui.split.set_show_sidebar(true);
    }
    super::view::clear_selection(ui);
}

/// Two thirds of the width for the map beside the photos, or half the height above them.
fn place_divider(split: &gtk::Paned) {
    let position = match split.orientation() {
        gtk::Orientation::Horizontal => split.width() * 2 / 3,
        _ => split.height() / 2,
    };
    if position > 0 {
        split.set_position(position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::state::AssetIds;

    #[test]
    fn map_views_choose_their_layout() {
        assert_eq!(mode_for(&Location::Page(LibraryPage::Map)), Mode::MapOnly);
        let area = Location::Source(LibrarySource::MapArea {
            ids: AssetIds(["a".to_string()].into()),
            label: "Lisbon".into(),
        });
        assert_eq!(mode_for(&area), Mode::WithPhotos);
        assert_eq!(
            mode_for(&Location::Source(LibrarySource::Favorites)),
            Mode::Off
        );
    }
}
