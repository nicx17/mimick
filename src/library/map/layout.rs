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
        Location::Page(LibraryPage::Map { .. }) => Mode::MapOnly,
        Location::Source(LibrarySource::MapArea { .. }) => Mode::WithPhotos,
        _ => Mode::Off,
    }
}

/// Show or hide the map and the photo grid for the location just opened.
pub fn sync(ui: &Rc<LibraryWindowUi>, location: &Location) {
    let mode = mode_for(location);
    let full = photos_full(ui);
    let previous = ui.map.mode.replace(mode);
    ui.map.photos_full.set(full);
    show_halves(ui, mode);
    match (previous == Mode::WithPhotos, mode == Mode::WithPhotos) {
        (false, true) => open_panel(ui),
        (true, false) => close_panel(ui),
        _ => {}
    }
}

/// The map takes the controls row's place on the Map page and beside a place's photos.
pub fn hides_controls(ui: &LibraryWindowUi) -> bool {
    ui.map.mode.get() != Mode::Off
}

/// Whether a place's photos fill the window: kept while moving between places, else the
/// setting's choice. The header asks before `sync` runs, so both use this.
pub fn photos_full(ui: &LibraryWindowUi) -> bool {
    if ui.map.mode.get() == Mode::WithPhotos {
        ui.map.photos_full.get()
    } else {
        ui.ctx.config.read().data.library_map_full_grid
    }
}

/// Switches the open place's photos between beside the map and filling the window.
pub fn set_photos_full(ui: &Rc<LibraryWindowUi>, full: bool) {
    if ui.map.mode.get() != Mode::WithPhotos || ui.map.photos_full.replace(full) == full {
        return;
    }
    show_halves(ui, Mode::WithPhotos);
    if !full {
        place_divider(&ui.map.split);
    }
}

/// The map shows on the Map page and beside a place's photos, unless they fill the window.
fn show_halves(ui: &Rc<LibraryWindowUi>, mode: Mode) {
    let map_shown = match mode {
        Mode::Off => false,
        Mode::MapOnly => true,
        Mode::WithPhotos => !ui.map.photos_full.get(),
    };
    ui.map.root.set_visible(map_shown);
    ui.content_stack.set_visible(mode != Mode::MapOnly);
    crate::library::navigation::sync_controls_row(ui);
    if map_shown {
        super::view::show(ui);
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
    if try_place_divider(split) {
        return;
    }
    split.add_tick_callback(|split, _| {
        if try_place_divider(split) {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

fn try_place_divider(split: &gtk::Paned) -> bool {
    let position = match split.orientation() {
        gtk::Orientation::Horizontal => split.width() * 2 / 3,
        _ => split.height() / 2,
    };
    if position > 0 {
        split.set_position(position);
    }
    position > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::state::AssetIds;

    #[test]
    fn map_views_choose_their_layout() {
        assert_eq!(
            mode_for(&Location::Page(LibraryPage::Map { album: None })),
            Mode::MapOnly
        );
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
