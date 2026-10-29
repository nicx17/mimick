//! Photo actions tied to an album: what is open, removing from it, its cover, leaving it.

use std::rc::Rc;

use gtk::prelude::*;

use crate::api_client::{AlbumUpdate, BulkOutcome, LibraryAlbum};

use super::LibraryWindowUi;
use super::context_menu::show_error_toast;
use super::dialogs::toast_with_action;
use super::header_format::{count_label, owns_album};
use super::navigation;
use super::state::{LibraryPage, LibrarySource, Location};

pub(super) fn current_album_id(ui: &LibraryWindowUi) -> Option<String> {
    match navigation::current(ui) {
        Location::Source(
            LibrarySource::Album { id, .. } | LibrarySource::AlbumUnified { id, .. },
        ) => Some(id),
        _ => None,
    }
}

pub(super) fn showing_album(ui: &LibraryWindowUi, album_id: &str) -> bool {
    current_album_id(ui).as_deref() == Some(album_id)
}

/// Show the album's photos, closing the lightbox first if it is open.
pub(super) fn open_album(ui: &Rc<LibraryWindowUi>, album_id: &str, name: &str) {
    if ui.nav.navigation_stack().n_items() > 1 {
        ui.nav.pop();
    }
    navigation::navigate_source(
        ui,
        LibrarySource::Album {
            id: album_id.to_string(),
            name: name.to_string(),
        },
    );
}

/// Reload album counts and covers in the sidebar and on the Albums page.
pub(super) fn refresh_albums(ui: &Rc<LibraryWindowUi>) {
    ui.albums.populated.set(false);
    super::load_albums(ui.clone());
    if matches!(
        navigation::current(ui),
        Location::Page(LibraryPage::Albums { .. })
    ) {
        super::refresh_albums_view(ui.clone());
    }
}

pub(super) fn current_album(ui: &LibraryWindowUi) -> Option<LibraryAlbum> {
    let id = current_album_id(ui)?;
    let state = ui.ctx.library_state.lock();
    state.albums.iter().find(|album| album.id == id).cloned()
}

/// Whether the open album lets this user remove photos (owners and editors).
pub(super) fn can_remove_here(ui: &LibraryWindowUi) -> bool {
    let me = ui.ctx.current_user_id.lock().clone();
    match (current_album(ui), me) {
        (Some(album), Some(me)) => album.can_edit_assets(&me),
        _ => false,
    }
}

pub(super) fn sync_bulk_remove(ui: &LibraryWindowUi) {
    ui.bulk_remove_from_album.set_visible(can_remove_here(ui));
}

pub(super) fn removed_message(outcome: BulkOutcome, album: &str) -> String {
    if outcome.done == 0 {
        return format!("Only the owner can remove these from “{album}”");
    }
    let mut message = format!(
        "Removed {} from “{album}”",
        count_label(outcome.done as u64)
    );
    if outcome.denied > 0 {
        message.push_str(&format!(" · {} belong to someone else", outcome.denied));
    }
    message
}

/// Take `asset_ids` out of the open album; the toast offers Undo.
pub(super) fn remove(ui: &Rc<LibraryWindowUi>, asset_ids: Vec<String>, on_removed: Rc<dyn Fn()>) {
    let Some(album) = current_album(ui) else {
        return;
    };
    if asset_ids.is_empty() {
        return;
    }
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match ui
            .ctx
            .api_client
            .remove_from_album(&album.id, &asset_ids)
            .await
        {
            Ok(results) => after_remove(&ui, &album, &results, &on_removed),
            Err(err) => show_error_toast(
                &ui,
                "Couldn't remove from the album",
                &format!("Immich rejected the request: {err}"),
            ),
        }
    });
}

fn after_remove(
    ui: &Rc<LibraryWindowUi>,
    album: &LibraryAlbum,
    results: &[crate::api_client::BulkIdResult],
    on_removed: &Rc<dyn Fn()>,
) {
    let removed: Vec<String> = results
        .iter()
        .filter(|result| result.success)
        .map(|result| result.id.clone())
        .collect();
    let message = removed_message(BulkOutcome::from_results(results), &album.album_name);
    if removed.is_empty() {
        ui.toasts.add_toast(libadwaita::Toast::new(&message));
        return;
    }
    if showing_album(ui, &album.id) {
        super::actions::remove_assets_from_grid(ui, &removed);
    }
    on_removed();
    let (ui_undo, id, name) = (ui.clone(), album.id.clone(), album.album_name.clone());
    ui.toasts
        .add_toast(toast_with_action(&message, "Undo", move || {
            super::album_picker::add_and_report(
                &ui_undo,
                id.clone(),
                name.clone(),
                removed.clone(),
                Rc::new(|| {}),
            )
        }));
    refresh_albums(ui);
}

/// The open album when this user owns it, so its cover and options can change.
pub(super) fn owned_current_album(ui: &LibraryWindowUi) -> Option<LibraryAlbum> {
    let me = ui.ctx.current_user_id.lock().clone()?;
    current_album(ui).filter(|album| owns_album(album, &me))
}

pub(super) fn set_cover(ui: &Rc<LibraryWindowUi>, album_id: String, asset_id: String) {
    let update = AlbumUpdate {
        album_thumbnail_asset_id: Some(asset_id),
        ..AlbumUpdate::default()
    };
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.update_album(&album_id, &update).await {
            Ok(()) => {
                ui.toasts
                    .add_toast(libadwaita::Toast::new("Album cover updated"));
                refresh_albums(&ui);
            }
            Err(err) => show_error_toast(&ui, "Couldn't update the album", &err),
        }
    });
}

pub(super) fn confirm_leave(ui: &Rc<LibraryWindowUi>, album: LibraryAlbum) {
    let ui_leave = ui.clone();
    let heading = format!("Leave “{}”?", album.album_name);
    super::dialogs::confirm(
        ui,
        &heading,
        "You'll stop seeing its photos unless the owner shares it with you again.",
        "Leave",
        move || leave(&ui_leave, album.clone()),
    );
}

fn leave(ui: &Rc<LibraryWindowUi>, album: LibraryAlbum) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.leave_album(&album.id).await {
            Ok(()) => {
                let message = format!("Left “{}”", album.album_name);
                ui.toasts.add_toast(libadwaita::Toast::new(&message));
                refresh_albums(&ui);
                if showing_album(&ui, &album.id) {
                    let shared = Location::Page(LibraryPage::Albums { shared_only: true });
                    navigation::navigate(&ui, shared);
                }
            }
            Err(err) => show_error_toast(
                &ui,
                "Couldn't leave the album",
                &format!("Immich rejected the request: {err}"),
            ),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_message_variants() {
        let o = |done, duplicate, denied| BulkOutcome {
            done,
            duplicate,
            denied,
        };
        assert_eq!(
            removed_message(o(1, 0, 0), "Trip"),
            "Removed 1 item from “Trip”"
        );
        assert_eq!(
            removed_message(o(3, 0, 2), "Trip"),
            "Removed 3 items from “Trip” · 2 belong to someone else"
        );
        assert_eq!(
            removed_message(o(0, 0, 2), "Trip"),
            "Only the owner can remove these from “Trip”"
        );
    }
}
