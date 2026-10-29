//! "Album Options": display order and comments and likes, saved as soon as they change.

use std::cell::Cell;
use std::rc::Rc;

use libadwaita::prelude::*;

use crate::api_client::{AlbumUpdate, LibraryAlbum, SortOrder};

use super::LibraryWindowUi;
use super::album_assets::showing_album;

const ORDERS: [SortOrder; 2] = [SortOrder::Desc, SortOrder::Asc];

pub(super) fn present(ui: &Rc<LibraryWindowUi>, album: LibraryAlbum) {
    let dialog = libadwaita::PreferencesDialog::builder()
        .title("Album Options")
        .content_width(420)
        .content_height(360)
        .build();
    let order = order_row(album.order);
    let activity = libadwaita::SwitchRow::builder()
        .title("Comments and Likes")
        .subtitle("Let people in this album like and comment in the Immich apps")
        .active(album.is_activity_enabled)
        .build();
    let display = libadwaita::PreferencesGroup::builder()
        .title("Display")
        .build();
    display.add(&order);
    let social = libadwaita::PreferencesGroup::builder()
        .title("Activity")
        .build();
    social.add(&activity);
    let page = libadwaita::PreferencesPage::new();
    page.add(&display);
    page.add(&social);
    dialog.add(&page);
    connect_order(ui, &dialog, &order, &album);
    connect_activity(ui, &dialog, &activity, &album);
    dialog.present(Some(&ui.window));
}

fn order_row(order: Option<SortOrder>) -> libadwaita::ComboRow {
    let row = libadwaita::ComboRow::builder()
        .title("Display Order")
        .model(&gtk::StringList::new(&["Newest First", "Oldest First"]))
        .build();
    row.set_selected(u32::from(order == Some(SortOrder::Asc)));
    row
}

fn connect_order(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
    row: &libadwaita::ComboRow,
    album: &LibraryAlbum,
) {
    let saved = Rc::new(Cell::new(row.selected()));
    let (ui, dialog, album_id) = (ui.clone(), dialog.clone(), album.id.clone());
    row.connect_selected_notify(move |row| {
        let selected = row.selected();
        if selected == saved.get() {
            return;
        }
        let order = ORDERS[selected as usize % ORDERS.len()];
        let update = AlbumUpdate {
            order: Some(order),
            ..AlbumUpdate::default()
        };
        let (ui, dialog, row, saved, album_id) = (
            ui.clone(),
            dialog.clone(),
            row.clone(),
            saved.clone(),
            album_id.clone(),
        );
        glib::MainContext::default().spawn_local(async move {
            match ui.ctx.api_client.update_album(&album_id, &update).await {
                Ok(()) => {
                    saved.set(selected);
                    after_order_saved(&ui, &album_id, order);
                }
                Err(err) => {
                    row.set_selected(saved.get());
                    dialog.add_toast(libadwaita::Toast::new(&format!("Couldn't save: {err}")));
                }
            }
        });
    });
}

fn after_order_saved(ui: &Rc<LibraryWindowUi>, album_id: &str, order: SortOrder) {
    ui.ctx.library_state.lock().set_album_order(album_id, order);
    if showing_album(ui, album_id) {
        super::controls::apply_album_order(ui, Some(order));
        super::navigation::reload_current(ui);
    }
}

fn connect_activity(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
    row: &libadwaita::SwitchRow,
    album: &LibraryAlbum,
) {
    let saved = Rc::new(Cell::new(row.is_active()));
    let (ui, dialog, album_id) = (ui.clone(), dialog.clone(), album.id.clone());
    row.connect_active_notify(move |row| {
        let active = row.is_active();
        if active == saved.get() {
            return;
        }
        let update = AlbumUpdate {
            is_activity_enabled: Some(active),
            ..AlbumUpdate::default()
        };
        let (ui, dialog, row, saved, album_id) = (
            ui.clone(),
            dialog.clone(),
            row.clone(),
            saved.clone(),
            album_id.clone(),
        );
        glib::MainContext::default().spawn_local(async move {
            match ui.ctx.api_client.update_album(&album_id, &update).await {
                Ok(()) => saved.set(active),
                Err(err) => {
                    row.set_active(saved.get());
                    dialog.add_toast(libadwaita::Toast::new(&format!("Couldn't save: {err}")));
                }
            }
        });
    });
}
