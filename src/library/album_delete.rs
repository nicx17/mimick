//! "Delete Album": the album goes, its photos stay, and folders that would bring it back
//! (sync re-creates missing albums by name) are detached first.

use std::path::Path;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::LibraryAlbum;
use crate::config::{AlbumFolders, album_folders, detach_album_from_watch_paths};
use crate::sync_index::ShardedSyncIndex;
use crate::watch_path_display::display_watch_path;

use super::LibraryWindowUi;
use super::album_assets::{refresh_albums, showing_album};
use super::context_menu::show_alert_dialog;
use super::navigation;
use super::state::{LibraryPage, Location};

pub(super) fn confirm_delete(ui: &Rc<LibraryWindowUi>, album: LibraryAlbum) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let folders = folders_for(&ui, &album).await;
        if !folders.is_empty() && uploads_running(&ui) {
            let body = format!(
                "Mimick is still uploading. Delete “{}” when uploads finish, so they don't create it again.",
                album.album_name
            );
            show_alert_dialog(&ui, "Uploads in Progress", &body);
            return;
        }
        if !folders.is_empty() && settings_open(&ui) {
            show_alert_dialog(
                &ui,
                "Close Settings First",
                "Deleting this album changes your watch folders, and saving the open Settings window would undo that. Close Settings, then delete the album.",
            );
            return;
        }
        present(&ui, album, folders);
    });
}

pub(super) fn delete_body(members: usize, has_shared_link: bool) -> String {
    let mut lines = vec!["Its photos stay in your library.".to_string()];
    match members {
        0 => {}
        1 => lines.push("It's also removed for the person it's shared with.".to_string()),
        n => lines.push(format!(
            "It's also removed for the {n} people it's shared with."
        )),
    }
    if has_shared_link {
        lines.push("Its shared links stop working.".to_string());
    }
    lines.join(" ")
}

async fn folders_for(ui: &LibraryWindowUi, album: &LibraryAlbum) -> AlbumFolders {
    let entries = ui.ctx.live_watch_paths.lock().clone();
    let same_name = ui
        .ctx
        .library_state
        .lock()
        .albums
        .iter()
        .filter(|other| other.album_name == album.album_name)
        .count();
    let name = (same_name <= 1).then(|| album.album_name.clone());
    let (index, id) = (ui.ctx.sync_index.clone(), album.id.clone());
    let synced = tokio::task::spawn_blocking(move || synced_dirs(&index, &id))
        .await
        .unwrap_or_default();
    album_folders(&entries, &album.id, name.as_deref(), &synced)
}

/// Folders holding files Mimick uploaded into the album.
fn synced_dirs(index: &ShardedSyncIndex, album_id: &str) -> Vec<String> {
    let mut dirs: Vec<String> = index
        .records_under_path(Path::new("/"))
        .into_iter()
        .filter(|(_, record)| record.album_id.as_deref() == Some(album_id))
        .filter_map(|(path, _)| {
            Path::new(&path)
                .parent()
                .map(|dir| dir.to_string_lossy().into_owned())
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

fn settings_open(ui: &LibraryWindowUi) -> bool {
    ui.app
        .windows()
        .iter()
        .any(|window| window.widget_name() == "mimick-settings-window")
}

fn uploads_running(ui: &LibraryWindowUi) -> bool {
    let state = ui.ctx.state.lock();
    state.queue_size > 0 || state.active_workers > 0
}

fn present(ui: &Rc<LibraryWindowUi>, album: LibraryAlbum, folders: AlbumFolders) {
    let members = album.album_users.len().saturating_sub(1);
    let dialog = libadwaita::AlertDialog::builder()
        .heading(format!("Delete “{}”?", album.album_name))
        .body(delete_body(members, album.has_shared_link))
        .build();
    let keep = folder_choices(&dialog, &folders);
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete Album");
    dialog.set_response_appearance("delete", libadwaita::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let ui_delete = ui.clone();
    dialog.connect_response(None, move |_, response| {
        if response == "delete" {
            let keep_watching = keep.as_ref().is_none_or(gtk::CheckButton::is_active);
            delete(&ui_delete, album.clone(), folders.clone(), keep_watching);
        }
    });
    dialog.present(Some(&ui.window));
}

/// Folder lists and, when folders sync with the album, the keep-or-stop choice;
/// returns the "keep backing up" option.
fn folder_choices(
    dialog: &libadwaita::AlertDialog,
    folders: &AlbumFolders,
) -> Option<gtk::CheckButton> {
    if folders.is_empty() {
        return None;
    }
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .build();
    let mut keep = None;
    if !folders.linked.is_empty() {
        content.append(&caption("These folders sync with this album:"));
        content.append(&folder_list(&folders.linked));
        let keep_button = gtk::CheckButton::builder()
            .label("Keep backing them up to your library")
            .active(true)
            .build();
        let stop_button = gtk::CheckButton::builder()
            .label("Stop watching these folders")
            .group(&keep_button)
            .build();
        content.append(&keep_button);
        content.append(&stop_button);
        keep = Some(keep_button);
    }
    if !folders.named_after.is_empty() {
        content.append(&caption(
            "So the album doesn't come back, photos in these folders go to your library instead of an album named after the folder:",
        ));
        content.append(&folder_list(&folders.named_after));
    }
    dialog.set_extra_child(Some(&content));
    keep
}

fn caption(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .wrap(true)
        .xalign(0.0)
        .css_classes(["caption"])
        .build()
}

fn folder_list(paths: &[String]) -> gtk::ScrolledWindow {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    for path in paths {
        list.append(
            &libadwaita::ActionRow::builder()
                .title(display_watch_path(path))
                .use_markup(false)
                .title_lines(2)
                .build(),
        );
    }
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(180)
        .child(&list)
        .build()
}

fn delete(
    ui: &Rc<LibraryWindowUi>,
    album: LibraryAlbum,
    folders: AlbumFolders,
    keep_watching: bool,
) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        if let Err(err) = ui.ctx.api_client.delete_album(&album.id).await {
            let body = format!("Immich rejected the request: {err}");
            show_alert_dialog(&ui, "Could Not Delete Album", &body);
            return;
        }
        detach_folders(&ui, &folders, keep_watching);
        after_delete(&ui, &album);
    });
}

/// Rewrite the watch folders, and tell the file monitor when folders stop being watched.
fn detach_folders(ui: &LibraryWindowUi, folders: &AlbumFolders, keep_watching: bool) {
    let (paths, background_sync) = {
        let mut config = ui.ctx.config.write();
        if !detach_album_from_watch_paths(&mut config.data.watch_paths, folders, keep_watching) {
            return;
        }
        if !config.save() {
            log::error!("Failed to save config after deleting an album");
        }
        (
            config.data.watch_paths.clone(),
            config.data.background_sync_enabled,
        )
    };
    *ui.ctx.live_watch_paths.lock() = paths.clone();
    ui.ctx.state.lock().watched_folder_count = paths.len();
    if !keep_watching {
        let monitored = if background_sync { paths } else { Vec::new() };
        ui.ctx
            .monitor_handle
            .replace_watch_paths(monitored, background_sync);
    }
}

fn after_delete(ui: &Rc<LibraryWindowUi>, album: &LibraryAlbum) {
    let message = format!("Deleted “{}”", album.album_name);
    ui.toasts.add_toast(libadwaita::Toast::new(&message));
    refresh_albums(ui);
    if showing_album(ui, &album.id) {
        let albums = Location::Page(LibraryPage::Albums { shared_only: false });
        navigation::navigate(ui, albums);
    }
    ui.ctx.library_state.lock().forget_album(&album.id);
    navigation::update_back_button(ui);
}

#[cfg(test)]
mod tests {
    use super::delete_body;

    #[test]
    fn delete_body_warns_who_else_loses_the_album() {
        assert_eq!(delete_body(0, false), "Its photos stay in your library.");
        assert_eq!(
            delete_body(1, false),
            "Its photos stay in your library. It's also removed for the person it's shared with."
        );
        assert_eq!(
            delete_body(3, true),
            "Its photos stay in your library. It's also removed for the 3 people it's shared with. Its shared links stop working."
        );
    }
}
