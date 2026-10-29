use std::rc::Rc;

use crate::api_client::{AlbumUpdate, LibraryAlbum};

use super::LibraryWindowUi;
use super::context_menu::show_error_toast;
use super::dialogs::{TextPrompt, prompt_multiline, prompt_text};
use super::state::LibrarySource;

pub(super) fn album_menu_items(
    ui: &Rc<LibraryWindowUi>,
    album: &LibraryAlbum,
) -> Vec<(String, Box<dyn Fn()>)> {
    let (ui_rename, album_rename) = (ui.clone(), album.clone());
    let (ui_describe, album_describe) = (ui.clone(), album.clone());
    let (ui_options, album_options) = (ui.clone(), album.clone());
    let (ui_delete, album_delete) = (ui.clone(), album.clone());
    vec![
        (
            "Rename…".to_string(),
            Box::new(move || rename(&ui_rename, &album_rename)),
        ),
        (
            "Edit Description…".to_string(),
            Box::new(move || edit_description(&ui_describe, &album_describe)),
        ),
        (
            "Options…".to_string(),
            Box::new(move || super::album_options::present(&ui_options, album_options.clone())),
        ),
        (
            "Delete Album…".to_string(),
            Box::new(move || super::album_delete::confirm_delete(&ui_delete, album_delete.clone())),
        ),
    ]
}

pub(super) fn rename(ui: &Rc<LibraryWindowUi>, album: &LibraryAlbum) {
    let (ui_save, album_id, old_name) = (ui.clone(), album.id.clone(), album.album_name.clone());
    prompt_text(
        ui,
        TextPrompt {
            heading: "Rename Album",
            body: "Folders linked to this album stay linked.",
            initial: &album.album_name,
            placeholder: "Album name",
            accept: "Rename",
        },
        move |name| {
            let update = AlbumUpdate {
                album_name: Some(name.clone()),
                ..AlbumUpdate::default()
            };
            let (ui, album_id, old_name) = (ui_save.clone(), album_id.clone(), old_name.clone());
            save(ui, album_id.clone(), update, move |ui| {
                after_rename(ui, &album_id, &old_name, &name)
            });
        },
    );
}

pub(super) fn edit_description(ui: &Rc<LibraryWindowUi>, album: &LibraryAlbum) {
    let (ui_save, album_id) = (ui.clone(), album.id.clone());
    let on_save = Rc::new(move |text: String| {
        let update = AlbumUpdate {
            description: Some(text),
            ..AlbumUpdate::default()
        };
        save(ui_save.clone(), album_id.clone(), update, |ui| {
            super::source_header::refresh(ui)
        });
    });
    prompt_multiline(ui, "Album Description", &album.description, on_save);
}

fn save(
    ui: Rc<LibraryWindowUi>,
    album_id: String,
    update: AlbumUpdate,
    after: impl FnOnce(&Rc<LibraryWindowUi>) + 'static,
) {
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.update_album(&album_id, &update).await {
            Ok(()) => after(&ui),
            Err(err) => show_error_toast(&ui, "Couldn't update the album", &err),
        }
    });
}

fn after_rename(ui: &Rc<LibraryWindowUi>, album_id: &str, old_name: &str, new_name: &str) {
    {
        let mut config = ui.ctx.config.write();
        if crate::config::rename_album_in_watch_paths(
            &mut config.data.watch_paths,
            album_id,
            old_name,
            new_name,
        ) {
            if !config.save() {
                log::error!("Failed to save config after renaming album");
            }
            *ui.ctx.live_watch_paths.lock() = config.data.watch_paths.clone();
        }
    }
    ui.ctx.library_state.lock().rename_album(album_id, new_name);
    let source: LibrarySource = ui.ctx.library_state.lock().source.clone();
    super::refresh_album_link_row(ui, &source);
    super::source_header::refresh(ui);
    ui.albums.populated.set(false);
    super::refresh_library_after_mutation(ui.clone(), false);
}
