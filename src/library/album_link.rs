//! Album-folder linking, sync dialog, and link/unlink actions.
//!
//! Presents a dialog to associate a local watch folder with a remote
//! Immich album for bidirectional synchronization. Handles link state
//! persistence and triggers the initial diff when a new link is created.

use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::library::album_sync::AlbumDiff;
use crate::library::state::LibrarySource;

use super::LibraryWindowUi;

pub(super) fn refresh_album_link_row(ui: &LibraryWindowUi, source: &LibrarySource) {
    let name = match source {
        LibrarySource::Album { name, .. }
        | LibrarySource::AlbumLocal { name, .. }
        | LibrarySource::AlbumUnified { name, .. } => name,
        _ => {
            ui.album_link_row.set_visible(false);
            if let Some(parent) = ui.album_link_row.parent() {
                parent.set_visible(false);
            }
            return;
        }
    };

    ui.album_link_row.set_visible(true);
    if let Some(parent) = ui.album_link_row.parent() {
        parent.set_visible(true);
    }

    let entries = ui.ctx.live_watch_paths.lock().clone();
    match crate::config::watch_entry_for_album(name, &entries) {
        Some(entry) => {
            ui.album_link_row.set_title("Linked folder");
            ui.album_link_row
                .set_subtitle(&crate::watch_path_display::display_watch_path_inline(
                    entry.path(),
                ));
            ui.album_link_button.set_label("Unlink");
            ui.album_sync_button.set_visible(true);
        }
        None => {
            ui.album_link_row.set_title("No local folder linked");
            ui.album_link_row
                .set_subtitle("Drop files in the linked folder to sync this album");
            ui.album_link_button.set_label("Link folder\u{2026}");
            ui.album_sync_button.set_visible(false);
        }
    }
}

pub(super) fn connect_album_link_row(ui: Rc<LibraryWindowUi>, _listbox: gtk::ListBox) {
    ui.album_link_button.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| handle_album_link_click(ui.clone())
    ));
    ui.album_sync_button.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| handle_album_sync_click(ui.clone())
    ));
}

fn handle_album_sync_click(ui: Rc<LibraryWindowUi>) {
    let source = ui.ctx.library_state.lock().source.clone();
    let LibrarySource::Album {
        id: album_id,
        name: album_name,
    } = source
    else {
        return;
    };
    let entries = ui.ctx.live_watch_paths.lock().clone();
    let Some(entry) = crate::config::watch_entry_for_album(&album_name, &entries) else {
        return;
    };
    let watch_path = std::path::PathBuf::from(entry.path());
    // Sync from the album view should preview every operation regardless of
    // per-folder gates — the user gets explicit checkboxes for each direction
    // in the confirmation dialog and can opt in even when the folder's rules
    // would otherwise suppress them.
    let rules = crate::config::FolderRules {
        delete_folder_to_album: true,
        delete_album_to_folder: true,
        sync_method: crate::config::FolderSyncMethod::Full,
        ..entry.rules()
    };

    let ui_for_async = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let diff = match crate::library::album_sync::diff_album_vs_folder(
            ui_for_async.ctx.clone(),
            &album_id,
            &watch_path,
            &rules,
            true,
        )
        .await
        {
            Ok(d) => d,
            Err(err) => {
                log::error!("Album diff failed: {}", err);
                return;
            }
        };
        present_sync_dialog(ui_for_async, album_id, album_name, watch_path, diff);
    });
}

fn present_sync_dialog(
    ui: Rc<LibraryWindowUi>,
    album_id: String,
    album_name: String,
    watch_path: std::path::PathBuf,
    diff: AlbumDiff,
) {
    if diff.to_upload.is_empty()
        && diff.to_download.is_empty()
        && diff.to_delete_remote.is_empty()
        && diff.to_delete_local.is_empty()
    {
        show_in_sync(&ui, diff.remote_unhashed);
        return;
    }

    let (dialog, checks) = build_sync_dialog(&diff);

    let ui_for_apply = ui.clone();
    dialog.connect_response(None, move |dlg, response| {
        if response != "apply" {
            return;
        }
        let choices = SyncChoices {
            upload: checks[0].is_active(),
            download: checks[1].is_active(),
            delete_remote: checks[2].is_active(),
            delete_local: checks[3].is_active(),
        };
        if choices.any() {
            glib::MainContext::default().spawn_local(execute_sync_selections(
                ui_for_apply.clone(),
                album_id.clone(),
                album_name.clone(),
                watch_path.clone(),
                filtered_diff(&diff, choices),
            ));
        }
        dlg.close();
    });
    dialog.present(Some(&ui.window));
}

/// Alert with one checkbox per sync direction that has work: upload, download,
/// album trash, local trash (in that order).
fn build_sync_dialog(diff: &AlbumDiff) -> (libadwaita::AlertDialog, [gtk::CheckButton; 4]) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading("Sync album")
        .body(format!(
            "Pick which directions to apply.{}",
            unmatched_note(diff.remote_unhashed)
        ))
        .build();
    let body_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    let checks = [
        direction_check(
            &body_box,
            "Upload {} item(s) to album",
            diff.to_upload.len(),
        ),
        direction_check(
            &body_box,
            "Download {} item(s) to folder",
            diff.to_download.len(),
        ),
        direction_check(
            &body_box,
            "Move {} album item(s) to trash",
            diff.to_delete_remote.len(),
        ),
        direction_check(
            &body_box,
            "Move {} local item(s) to trash",
            diff.to_delete_local.len(),
        ),
    ];
    dialog.set_extra_child(Some(&body_box));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("apply", "Apply");
    dialog.set_response_appearance("apply", libadwaita::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("apply"));
    dialog.set_close_response("cancel");
    (dialog, checks)
}

fn show_in_sync(ui: &LibraryWindowUi, remote_unhashed: usize) {
    let info = libadwaita::AlertDialog::builder()
        .heading("Album sync")
        .body(in_sync_message(remote_unhashed))
        .build();
    info.add_response("ok", "OK");
    info.set_default_response(Some("ok"));
    info.set_close_response("ok");
    info.present(Some(&ui.window));
}

/// Checkbox for one sync direction, pre-checked and shown only when it has work to do.
fn direction_check(body_box: &gtk::Box, label: &str, count: usize) -> gtk::CheckButton {
    let check = gtk::CheckButton::builder()
        .label(label.replace("{}", &count.to_string()))
        .active(count > 0)
        .sensitive(count > 0)
        .build();
    if count > 0 {
        body_box.append(&check);
    }
    check
}

/// Which directions the user chose to apply.
#[derive(Debug, Clone, Copy)]
struct SyncChoices {
    upload: bool,
    download: bool,
    delete_remote: bool,
    delete_local: bool,
}

impl SyncChoices {
    fn any(self) -> bool {
        self.upload || self.download || self.delete_remote || self.delete_local
    }
}

/// The diff restricted to the chosen directions.
fn filtered_diff(diff: &AlbumDiff, choices: SyncChoices) -> AlbumDiff {
    fn keep<T: Clone>(items: &[T], chosen: bool) -> Vec<T> {
        if chosen { items.to_vec() } else { Vec::new() }
    }
    AlbumDiff {
        to_upload: keep(&diff.to_upload, choices.upload),
        to_download: keep(&diff.to_download, choices.download),
        to_delete_remote: keep(&diff.to_delete_remote, choices.delete_remote),
        to_delete_local: keep(&diff.to_delete_local, choices.delete_local),
        remote_unhashed: 0,
    }
}

fn in_sync_message(remote_unhashed: usize) -> String {
    if remote_unhashed > 0 {
        format!(
            "Already in sync. ({} remote item(s) couldn't be matched — missing checksum.)",
            remote_unhashed
        )
    } else {
        "Already in sync.".to_string()
    }
}

fn unmatched_note(remote_unhashed: usize) -> String {
    if remote_unhashed > 0 {
        format!(
            "\n\n{} remote item(s) couldn't be matched (missing checksum).",
            remote_unhashed
        )
    } else {
        String::new()
    }
}

async fn execute_sync_selections(
    ui: Rc<LibraryWindowUi>,
    album_id: String,
    album_name: String,
    watch_path: std::path::PathBuf,
    diff: crate::library::album_sync::AlbumDiff,
) {
    let queued =
        execute_selected_uploads(&ui, &album_id, &album_name, &watch_path, diff.to_upload).await;
    let (downloaded, failed) =
        execute_selected_downloads(&ui, &album_id, &album_name, watch_path, diff.to_download).await;
    let remote_deleted =
        execute_selected_remote_deletes(&ui, &album_id, diff.to_delete_remote).await;
    let (local_deleted, local_delete_failed) =
        execute_selected_local_deletes(&ui, diff.to_delete_local).await;

    log::info!(
        "Album sync done: {} queued for upload, {} downloaded, {} download failures, {} moved to Immich trash, {} local trashed, {} local trash failures",
        queued,
        downloaded,
        failed,
        remote_deleted,
        local_deleted,
        local_delete_failed
    );
    if queued > 0 || downloaded > 0 || remote_deleted > 0 || local_deleted > 0 {
        super::refresh_library_after_mutation(ui.clone(), true);
    }
}

async fn execute_selected_uploads(
    ui: &Rc<LibraryWindowUi>,
    album_id: &str,
    album_name: &str,
    watch_path: &std::path::Path,
    assets: Vec<crate::library::album_sync::LocalEntry>,
) -> usize {
    if assets.is_empty() {
        return 0;
    }
    crate::library::album_sync::execute_uploads(
        ui.ctx.clone(),
        album_id.to_string(),
        album_name.to_string(),
        watch_path.to_path_buf(),
        assets,
    )
    .await
}

async fn execute_selected_downloads(
    ui: &Rc<LibraryWindowUi>,
    album_id: &str,
    album_name: &str,
    watch_path: std::path::PathBuf,
    assets: Vec<crate::api_client::LibraryAsset>,
) -> (usize, usize) {
    if assets.is_empty() {
        return (0, 0);
    }
    crate::library::album_sync::execute_downloads(
        ui.ctx.clone(),
        watch_path,
        Some(album_id.to_string()),
        Some(album_name.to_string()),
        assets,
    )
    .await
}

async fn execute_selected_remote_deletes(
    ui: &Rc<LibraryWindowUi>,
    album_id: &str,
    assets: Vec<crate::api_client::LibraryAsset>,
) -> usize {
    if assets.is_empty() {
        return 0;
    }
    crate::library::album_sync::execute_remote_deletions(ui.ctx.clone(), album_id, assets).await
}

async fn execute_selected_local_deletes(
    ui: &Rc<LibraryWindowUi>,
    assets: Vec<crate::library::album_sync::LocalEntry>,
) -> (usize, usize) {
    if assets.is_empty() {
        return (0, 0);
    }
    crate::library::album_sync::execute_local_deletions(ui.ctx.clone(), assets).await
}

fn handle_album_link_click(ui: Rc<LibraryWindowUi>) {
    let source = ui.ctx.library_state.lock().source.clone();
    let LibrarySource::Album {
        id: album_id,
        name: album_name,
    } = source
    else {
        return;
    };

    let entries = ui.ctx.live_watch_paths.lock().clone();
    let already_linked = crate::config::watch_entry_for_album(&album_name, &entries).is_some();

    if already_linked {
        unlink_album(ui.clone(), &album_name);
        return;
    }

    let dialog = gtk::FileDialog::builder()
        .title(format!("Link folder for album '{}'", album_name))
        .build();
    let ui_for_pick = ui.clone();
    let album_name_for_pick = album_name.clone();
    let album_id_for_pick = album_id.clone();
    dialog.select_folder(Some(&ui.window), gtk::gio::Cancellable::NONE, move |res| {
        let Ok(folder) = res else { return };
        let Some(path) = folder.path() else { return };
        link_album_to_path(
            ui_for_pick.clone(),
            album_id_for_pick.clone(),
            album_name_for_pick.clone(),
            path,
        );
    });
}

fn unlink_album(ui: Rc<LibraryWindowUi>, album_name: &str) {
    {
        let mut config = ui.ctx.config.write();
        config
            .data
            .watch_paths
            .retain(|entry| entry.album_name() != Some(album_name));
        if !config.save() {
            log::error!("Failed to save config after unlink");
            return;
        }
        *ui.ctx.live_watch_paths.lock() = config.data.watch_paths.clone();
    }
    let source_after = ui.ctx.library_state.lock().source.clone();
    refresh_album_link_row(&ui, &source_after);
}

fn link_album_to_path(
    ui: Rc<LibraryWindowUi>,
    album_id: String,
    album_name: String,
    path: std::path::PathBuf,
) {
    let path_string = path.to_string_lossy().to_string();
    {
        let mut config = ui.ctx.config.write();
        config
            .data
            .watch_paths
            .retain(|entry| entry.album_name() != Some(album_name.as_str()));
        config
            .data
            .watch_paths
            .push(crate::config::WatchPathEntry::WithConfig {
                path: path_string,
                album_id: Some(album_id),
                album_name: Some(album_name),
                rules: crate::config::FolderRules::default(),
            });
        if !config.save() {
            log::error!("Failed to save config after link");
            return;
        }
        *ui.ctx.live_watch_paths.lock() = config.data.watch_paths.clone();
    }
    let source_after = ui.ctx.library_state.lock().source.clone();
    refresh_album_link_row(&ui, &source_after);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::LibraryAsset;

    fn asset(id: &str) -> LibraryAsset {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "originalFileName": format!("{id}.jpg"),
            "originalMimeType": "image/jpeg",
            "fileCreatedAt": "2024-01-15T14:25:15.000Z",
            "type": "IMAGE"
        }))
        .unwrap()
    }

    #[test]
    fn filtered_diff_keeps_only_chosen_directions() {
        let diff = AlbumDiff {
            to_download: vec![asset("d1"), asset("d2")],
            to_delete_remote: vec![asset("r1")],
            remote_unhashed: 3,
            ..AlbumDiff::default()
        };
        let choices = SyncChoices {
            upload: true,
            download: true,
            delete_remote: false,
            delete_local: true,
        };
        let filtered = filtered_diff(&diff, choices);
        assert_eq!(filtered.to_download.len(), 2);
        assert!(filtered.to_delete_remote.is_empty());
        assert_eq!(filtered.remote_unhashed, 0);
    }

    #[test]
    fn sync_choices_any_needs_one_direction() {
        let none = SyncChoices {
            upload: false,
            download: false,
            delete_remote: false,
            delete_local: false,
        };
        assert!(!none.any());
        assert!(
            SyncChoices {
                delete_local: true,
                ..none
            }
            .any()
        );
    }

    #[test]
    fn messages_mention_unmatched_items_only_when_present() {
        assert_eq!(in_sync_message(0), "Already in sync.");
        assert!(in_sync_message(2).contains("2 remote item(s)"));
        assert_eq!(unmatched_note(0), "");
        assert!(unmatched_note(1).contains("1 remote item(s)"));
    }
}
