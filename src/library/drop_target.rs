use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::library::staging_view;

use super::{LibraryWindowUi, state::LibrarySource, upload_picker};

pub(super) fn connect_drop_target(ui: Rc<LibraryWindowUi>) {
    let drop_target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY,
    );

    let ui_enter = ui.clone();
    drop_target.connect_enter(move |target, _x, _y| {
        // Reject internal drags (from our own DragSource).
        if target.current_drop().is_some_and(|d| d.drag().is_some()) {
            return gtk::gdk::DragAction::empty();
        }
        ui_enter.drop_overlay.set_reveal_child(true);
        gtk::gdk::DragAction::COPY
    });

    let ui_leave = ui.clone();
    drop_target.connect_leave(move |_target| {
        ui_leave.drop_overlay.set_reveal_child(false);
    });

    let ui_drop = ui.clone();
    drop_target.connect_drop(move |target, value, _x, _y| {
        ui_drop.drop_overlay.set_reveal_child(false);

        // Reject internal drags.
        if target.current_drop().is_some_and(|d| d.drag().is_some()) {
            return false;
        }

        handle_drop(&ui_drop, value)
    });

    ui.window.add_controller(drop_target);
}

fn handle_drop(ui: &LibraryWindowUi, value: &gtk::glib::Value) -> bool {
    let file_list = match value.get::<gtk::gdk::FileList>() {
        Ok(fl) => fl,
        Err(_) => return false,
    };

    let paths: Vec<std::path::PathBuf> = file_list
        .files()
        .iter()
        .filter_map(gtk::gio::prelude::FileExt::path)
        .filter(|p| crate::media_kinds::is_supported_path(p))
        .collect();

    if paths.is_empty() {
        show_unsupported_drop_toast(ui);
        return true;
    }

    let album = match ui.ctx.library_state.lock().source.clone() {
        LibrarySource::Album { id, name }
        | LibrarySource::AlbumLocal { id, name }
        | LibrarySource::AlbumUnified { id, name } => Some((id, name)),
        _ => None,
    };

    if let Some(album) = album {
        handle_album_drop_upload(ui, album, paths);
    } else {
        handle_library_drop_upload(ui, paths);
    }

    true
}

pub(super) fn show_unsupported_drop_toast(ui: &LibraryWindowUi) {
    let toast = libadwaita::Toast::new("No supported media files in drop");
    if let Some(overlay) = ui
        .window
        .content()
        .and_then(|w| w.first_child())
        .and_downcast::<libadwaita::ToastOverlay>()
    {
        overlay.add_toast(toast);
    } else {
        log::info!("No supported media files in drop");
    }
}

pub(super) fn handle_album_drop_upload(
    ui: &LibraryWindowUi,
    album: (String, String),
    paths: Vec<std::path::PathBuf>,
) {
    let count = paths.len();
    upload_picker::spawn_enqueue_with_callback(
        ui.ctx.clone(),
        Some(album.clone()),
        paths,
        move |queued, _skipped| {
            log::info!(
                "Drop upload to album '{}': queued {}/{} file(s)",
                album.1,
                queued,
                count
            );
        },
    );
}

fn handle_library_drop_upload(ui: &LibraryWindowUi, paths: Vec<std::path::PathBuf>) {
    let count = paths.len();
    let ctx_for_album = ui.ctx.clone();
    let window_for_album = ui.window.clone();
    let paths_for_album = paths.clone();

    upload_picker::spawn_enqueue_with_callback(
        ui.ctx.clone(),
        None,
        paths,
        move |queued, _skipped| {
            log::info!(
                "Drop upload to library: queued {}/{} file(s)",
                queued,
                count
            );
            if queued > 0 {
                staging_view::show_album_picker(window_for_album, ctx_for_album, paths_for_album);
            }
        },
    );
}
