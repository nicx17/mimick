use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::Tag;
use crate::library::albums_view::{AlbumClick, populate_albums};
use crate::library::controls::refresh_library_after_mutation;
use crate::library::state::{LibraryPage, LibrarySource, Location};
use crate::library::{LibraryWindowUi, ensure_current_user_id, navigation, search_view};

/// Connect action handlers to the album creation widgets.
pub(super) fn connect_albums_create(ui: Rc<LibraryWindowUi>) {
    ui.albums.create_button.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| prompt_create_album(ui.clone())
    ));
}

/// Show a popup modal dialog prompting the user to name and create a new album.
pub(super) fn prompt_create_album(ui: Rc<LibraryWindowUi>) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading("Create album")
        .body("Choose a name for the new album.")
        .build();
    let entry = gtk::Entry::builder()
        .placeholder_text("Album name")
        .activates_default(true)
        .build();
    dialog.set_extra_child(Some(&entry));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("create", "Create");
    dialog.set_response_appearance("create", libadwaita::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("create"));
    dialog.set_close_response("cancel");

    let ui_for_choice = ui.clone();
    let entry_for_choice = entry.clone();
    dialog.connect_response(None, move |dlg, response| {
        if response != "create" {
            return;
        }
        let name = entry_for_choice.text().to_string();
        if name.trim().is_empty() {
            return;
        }
        let ui = ui_for_choice.clone();
        glib::MainContext::default().spawn_local(async move {
            match ui.ctx.api_client.create_album(name.trim()).await {
                Ok(_) => {
                    refresh_library_after_mutation(ui.clone(), false);
                }
                Err(err) => log::error!("Create album failed: {}", err),
            }
        });
        dlg.close();
    });
    dialog.present(Some(&ui.window));
}

/// Asynchronously fetch the latest albums from the server and redraw the album views.
///
/// Late-arriving fetches must not hijack the stack: if the user navigated to
/// another tab while we were waiting on the network, we still bind the data
/// (so the grid is current when they come back) but we do *not* force the
/// stack child to "albums" or "error".
pub(super) fn refresh_albums_view(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(async move {
        // Albums are split into yours and shared-with-you by owner, so the id must be known.
        ensure_current_user_id(&ui.ctx).await;
        match ui.ctx.api_client.fetch_library_albums().await {
            Ok(albums) => {
                let on_click = album_click_handler(ui.clone());
                populate_albums(&ui.albums, ui.ctx.clone(), albums, on_click);
                if on_albums_page(&ui) {
                    ui.content_stack.set_visible_child_name("albums");
                }
            }
            Err(err) => {
                log::warn!("Albums fetch failed: {}", err);
                if !ui.albums.populated.get() && on_albums_page(&ui) {
                    ui.error_label
                        .set_label(&format!("Could not load albums: {}", err));
                    ui.content_stack.set_visible_child_name("error");
                }
            }
        }
    });
}

pub(super) fn on_albums_page(ui: &LibraryWindowUi) -> bool {
    matches!(
        navigation::current(ui),
        Location::Page(LibraryPage::Albums { .. })
    )
}

pub(super) fn album_click_handler(ui: Rc<LibraryWindowUi>) -> AlbumClick {
    Rc::new(move |id: &str, name: String| {
        navigation::navigate_source(
            &ui,
            LibrarySource::Album {
                id: id.to_string(),
                name,
            },
        );
    })
}

/// Apply header control layout adjustments when switching view modes.
/// Refresh the search form's Tag dropdown from the server, then select `select` if given.
pub(super) fn refresh_search_tags(ui: Rc<LibraryWindowUi>, select: Option<String>) {
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.fetch_tags().await {
            Ok(tags) => {
                search_view::set_available_tags(&ui.search_view, &tags);
                if let Some(tag_id) = select {
                    search_view::select_tag(&ui.search_view, &tag_id);
                }
            }
            // Tags are optional (`tag.read`); the dropdown just stays at "Any".
            Err(err) => log::debug!("Could not load tags for search: {}", err),
        }
    });
}

pub(super) fn search_by_tag(ui: Rc<LibraryWindowUi>, tag: Tag) {
    navigation::navigate_source(
        &ui,
        LibrarySource::Tag {
            id: tag.id,
            name: tag.value,
        },
    );
}
