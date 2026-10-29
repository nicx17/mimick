//! Album selection dialog and upload result handling for staged files.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::LibraryAlbum;
use crate::app_context::AppContext;
use crate::library::upload_picker;

/// Present an album picker dialog and upload files to the chosen album.
pub(crate) fn show_album_picker(
    parent: libadwaita::ApplicationWindow,
    ctx: Arc<AppContext>,
    paths: Vec<PathBuf>,
) {
    let dialog = simple_alert("Upload to Album", "Loading albums...", "cancel", "Cancel");
    dialog.present(Some(&parent));

    let ctx_c = ctx.clone();
    let parent_c = parent.clone();
    glib::MainContext::default().spawn_local(async move {
        let albums = match ctx_c.api_client.fetch_library_albums().await {
            Ok(a) => a,
            Err(err) => {
                dialog.force_close();
                simple_alert("Failed to Load Albums", &err, "ok", "OK").present(Some(&parent_c));
                return;
            }
        };
        dialog.force_close();
        present_album_list(parent_c, ctx_c, albums, paths);
    });
}

/// Build and show the album selection list dialog.
fn present_album_list(
    parent: libadwaita::ApplicationWindow,
    ctx: Arc<AppContext>,
    albums: Vec<LibraryAlbum>,
    paths: Vec<PathBuf>,
) {
    let parent_for_result = parent.clone();
    let dialog = libadwaita::AlertDialog::builder()
        .heading("Select Album")
        .body("Choose an album for the upload:")
        .build();
    dialog.add_response("cancel", "Cancel");

    let (scroll, listbox) = build_album_listbox(&albums);
    dialog.set_extra_child(Some(&scroll));

    let albums_rc = Rc::new(albums);
    let paths_rc = Rc::new(paths);
    let ctx_c = ctx.clone();
    dialog.connect_response(None, move |_dialog, response| {
        if response == "cancel" {
            return;
        }
        handle_album_upload(&listbox, &albums_rc, &ctx_c, &paths_rc, &parent_for_result);
    });

    dialog.add_response("upload", "Upload");
    dialog.set_response_appearance("upload", libadwaita::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("upload"));
    dialog.present(Some(&parent));
}

/// Handle the album upload response: look up selected row and enqueue.
fn handle_album_upload(
    listbox: &gtk::ListBox,
    albums: &[LibraryAlbum],
    ctx: &Arc<AppContext>,
    paths: &Rc<Vec<PathBuf>>,
    parent: &libadwaita::ApplicationWindow,
) {
    let Some(row) = listbox.selected_row() else {
        return;
    };
    let idx = row.index() as usize;
    if let Some(album) = albums.get(idx) {
        let album_arg = Some((album.id.clone(), album.album_name.clone()));
        let parent_c = parent.clone();
        upload_picker::spawn_enqueue_with_callback(
            ctx.clone(),
            album_arg,
            paths.to_vec(),
            move |queued, skipped| show_upload_result(&parent_c, queued, skipped),
        );
    }
}

/// Build the scrollable album list for the picker dialog.
fn build_album_listbox(albums: &[LibraryAlbum]) -> (gtk::ScrolledWindow, gtk::ListBox) {
    let listbox = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(["boxed-list"])
        .build();
    for album in albums {
        let row = libadwaita::ActionRow::builder()
            .title(&album.album_name)
            .subtitle(format!("{} assets", album.asset_count))
            .activatable(true)
            .build();
        listbox.append(&row);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .child(&listbox)
        .min_content_height(200)
        .max_content_height(400)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    (scroll, listbox)
}

/// Show a result dialog after upload enqueue completes.
pub(super) fn show_upload_result(
    parent: &libadwaita::ApplicationWindow,
    queued: usize,
    skipped: usize,
) {
    let (heading, body) = if skipped == 0 {
        (
            "Upload Queued",
            format!(
                "{} file(s) queued for upload. Progress is visible in the queue inspector.",
                queued
            ),
        )
    } else if queued == 0 {
        (
            "Upload Failed",
            format!(
                "All {} file(s) failed to enqueue. Check the logs for details.",
                skipped
            ),
        )
    } else {
        (
            "Upload Partially Queued",
            format!(
                "{} file(s) queued, {} skipped (hash or path errors).",
                queued, skipped
            ),
        )
    };

    simple_alert(heading, &body, "ok", "OK").present(Some(parent));
}

/// Create a one-button `AlertDialog` (shared pattern used by multiple dialogs).
fn simple_alert(
    heading: &str,
    body: &str,
    response_id: &str,
    response_label: &str,
) -> libadwaita::AlertDialog {
    let dialog = libadwaita::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response(response_id, response_label);
    dialog
}
