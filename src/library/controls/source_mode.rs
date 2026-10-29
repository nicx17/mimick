//! Remote, local, and unified library source selection.

use std::rc::Rc;

use glib::clone;

use crate::library::state::LibrarySource;

use super::{LibraryWindowUi, navigation};

/// Remote/Local/Unified dropdown.
pub fn connect_source_controls(ui: &Rc<LibraryWindowUi>) {
    ui.source_mode.connect_selected_notify(clone!(
        #[strong]
        ui,
        move |dropdown| {
            if ui.interaction.source_mode_suppressed.get() {
                return;
            }
            let source = source_for_selection(dropdown.selected(), current_album(&ui));
            navigation::navigate_source(&ui, source);
        }
    ));
}

/// The album being browsed, if any, as `(id, name)`.
pub fn current_album(ui: &LibraryWindowUi) -> Option<(String, String)> {
    match ui.ctx.library_state.lock().source.clone() {
        LibrarySource::Album { id, name }
        | LibrarySource::AlbumLocal { id, name }
        | LibrarySource::AlbumUnified { id, name } => Some((id, name)),
        _ => None,
    }
}

/// Source for a Remote (0) / Local (1) / Unified (2) choice, scoped to the open album if any.
pub fn source_for_selection(selected: u32, album: Option<(String, String)>) -> LibrarySource {
    match (selected, album) {
        (1, Some((id, name))) => LibrarySource::AlbumLocal { id, name },
        (1, None) => LibrarySource::LocalAll,
        (2, Some((id, name))) => LibrarySource::AlbumUnified { id, name },
        (2, None) => LibrarySource::Unified,
        (_, Some((id, name))) => LibrarySource::Album { id, name },
        (_, None) => LibrarySource::AllAssets,
    }
}
