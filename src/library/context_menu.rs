//! Right-click context menu popover and its action handlers.
//!
//! Builds a GTK popover menu with per-asset actions: copy, download, open in
//! an external app, edit info, and move to trash. Actions delegate to the
//! shared library helpers.

use std::path::PathBuf;
use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::library::asset_object::AssetObject;

use super::LOCAL_ID_PREFIX;
use super::LibraryWindowUi;
use super::actions::{TrashTarget, confirm_trash};
use super::asset_edit::show_edit_dialog;
use super::download::{
    begin_download_session, finish_download_item, open_local_with_default_app, start_download,
    track_download_item,
};
use super::load_texture_oriented;
use super::trash_view;

/// What the view that opened the menu does after its asset is trashed or edited.
pub(super) struct AssetMenuHooks {
    pub on_trashed: Rc<dyn Fn()>,
    /// Receives whether the capture date changed.
    pub on_edited: Rc<dyn Fn(bool)>,
}

impl AssetMenuHooks {
    /// Grid behaviour: trashed items are already gone from the model, and a changed
    /// capture date reloads the source because the item's timeline position moves.
    pub(super) fn for_grid(ui: &Rc<LibraryWindowUi>) -> Self {
        let ui = ui.clone();
        Self {
            on_trashed: Rc::new(|| {}),
            on_edited: Rc::new(move |date_changed| {
                if date_changed {
                    super::refresh_library_after_mutation(ui.clone(), true);
                }
            }),
        }
    }
}

pub(super) fn show_asset_context_menu(
    ui: Rc<LibraryWindowUi>,
    parent: &impl gtk::prelude::IsA<gtk::Widget>,
    position: u32,
    x: f64,
    y: f64,
    hooks: AssetMenuHooks,
) {
    let Some(item) = ui.grid.model.item(position).and_downcast::<AssetObject>() else {
        return;
    };
    let asset_id = item.property::<String>("id");
    let remote_id = item.property::<String>("remote-id");
    let local_path = item.property::<String>("local-path");
    let filename = item.property::<String>("filename");
    let asset_type = item.property::<String>("asset-type");
    let is_image = asset_type.eq_ignore_ascii_case("IMAGE");
    let can_download = !remote_id.is_empty() && !asset_id.starts_with(LOCAL_ID_PREFIX);
    let can_open = can_download || !local_path.is_empty();

    let popover = gtk::Popover::builder()
        .has_arrow(true)
        .autohide(true)
        .build();
    popover.set_parent(parent);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();

    if is_image {
        let copy_btn = gtk::Button::builder()
            .label("Copy")
            .halign(gtk::Align::Fill)
            .build();
        copy_btn.connect_clicked(clone!(
            #[strong]
            ui,
            #[strong]
            popover,
            #[strong]
            asset_id,
            #[strong]
            remote_id,
            #[strong]
            local_path,
            #[strong]
            filename,
            move |_| {
                popover.popdown();
                copy_asset_to_clipboard(
                    ui.clone(),
                    asset_id.clone(),
                    remote_id.clone(),
                    local_path.clone(),
                    filename.clone(),
                );
            }
        ));
        content.append(&copy_btn);
    }

    if can_download {
        let download_btn = gtk::Button::builder()
            .label("Download")
            .halign(gtk::Align::Fill)
            .build();
        download_btn.connect_clicked(clone!(
            #[strong]
            ui,
            #[strong]
            popover,
            #[strong]
            remote_id,
            #[strong]
            filename,
            move |_| {
                popover.popdown();
                start_download(ui.clone(), remote_id.clone(), filename.clone());
            }
        ));
        content.append(&download_btn);
    }

    if can_open {
        let open_btn = gtk::Button::builder()
            .label("Open In")
            .halign(gtk::Align::Fill)
            .build();
        open_btn.connect_clicked(clone!(
            #[strong]
            ui,
            #[strong]
            popover,
            #[strong]
            asset_id,
            #[strong]
            remote_id,
            #[strong]
            local_path,
            #[strong]
            filename,
            move |_| {
                popover.popdown();
                open_asset_in_default_app(
                    ui.clone(),
                    asset_id.clone(),
                    remote_id.clone(),
                    local_path.clone(),
                    filename.clone(),
                );
            }
        ));
        content.append(&open_btn);
    }

    if can_download {
        append_manage_buttons(&content, &ui, &popover, &item, hooks);
    }

    popover.set_child(Some(&content));
    popover.popup();
}

/// "Edit Info…" and "Move to Trash" for assets that exist on the server.
fn append_manage_buttons(
    content: &gtk::Box,
    ui: &Rc<LibraryWindowUi>,
    popover: &gtk::Popover,
    item: &AssetObject,
    hooks: AssetMenuHooks,
) {
    let Some(target) = TrashTarget::from_item(item) else {
        return;
    };
    if trash_view::is_trash_active(ui) {
        let remote_id = target.remote_id().to_string();
        content.append(&restore_button(
            ui,
            popover,
            remote_id.clone(),
            hooks.on_trashed.clone(),
        ));
        content.append(&delete_forever_button(
            ui,
            popover,
            remote_id,
            hooks.on_trashed,
        ));
        return;
    }
    let filename = item.property::<String>("filename");
    content.append(&edit_info_button(
        ui,
        popover,
        target.remote_id().to_string(),
        filename,
        hooks.on_edited,
    ));
    content.append(&trash_button(ui, popover, target, hooks.on_trashed));
}

fn edit_info_button(
    ui: &Rc<LibraryWindowUi>,
    popover: &gtk::Popover,
    remote_id: String,
    filename: String,
    on_edited: Rc<dyn Fn(bool)>,
) -> gtk::Button {
    let button = gtk::Button::builder()
        .label("Edit Info…")
        .halign(gtk::Align::Fill)
        .build();
    button.connect_clicked(clone!(
        #[strong]
        ui,
        #[strong]
        popover,
        move |_| {
            popover.popdown();
            show_edit_dialog(
                ui.clone(),
                remote_id.clone(),
                filename.clone(),
                on_edited.clone(),
            );
        }
    ));
    button
}

/// Trash view: put the asset back in the library.
fn restore_button(
    ui: &Rc<LibraryWindowUi>,
    popover: &gtk::Popover,
    remote_id: String,
    on_removed: Rc<dyn Fn()>,
) -> gtk::Button {
    let button = gtk::Button::builder()
        .label("Restore")
        .halign(gtk::Align::Fill)
        .build();
    button.connect_clicked(clone!(
        #[strong]
        ui,
        #[strong]
        popover,
        move |_| {
            popover.popdown();
            let on_removed = on_removed.clone();
            trash_view::restore(ui.clone(), vec![remote_id.clone()], move || on_removed());
        }
    ));
    button
}

/// Trash view: delete the asset for good, after confirmation.
fn delete_forever_button(
    ui: &Rc<LibraryWindowUi>,
    popover: &gtk::Popover,
    remote_id: String,
    on_removed: Rc<dyn Fn()>,
) -> gtk::Button {
    let button = gtk::Button::builder()
        .label("Delete Permanently")
        .halign(gtk::Align::Fill)
        .css_classes(["destructive-action"])
        .build();
    button.connect_clicked(clone!(
        #[strong]
        ui,
        #[strong]
        popover,
        move |_| {
            popover.popdown();
            let on_removed = on_removed.clone();
            trash_view::confirm_delete_permanently(
                ui.clone(),
                vec![remote_id.clone()],
                move || on_removed(),
            );
        }
    ));
    button
}

fn trash_button(
    ui: &Rc<LibraryWindowUi>,
    popover: &gtk::Popover,
    target: TrashTarget,
    on_trashed: Rc<dyn Fn()>,
) -> gtk::Button {
    let button = gtk::Button::builder()
        .label("Move to Trash")
        .halign(gtk::Align::Fill)
        .css_classes(["destructive-action"])
        .build();
    // The popover is single-use, so the target is handed to the dialog on the first click.
    let target = std::cell::RefCell::new(Some(target));
    button.connect_clicked(clone!(
        #[strong]
        ui,
        #[strong]
        popover,
        move |_| {
            popover.popdown();
            if let Some(target) = target.borrow_mut().take() {
                let on_trashed = on_trashed.clone();
                confirm_trash(ui.clone(), vec![target], 0, move || on_trashed());
            }
        }
    ));
    button
}

fn copy_asset_to_clipboard(
    ui: Rc<LibraryWindowUi>,
    asset_id: String,
    remote_id: String,
    local_path: String,
    filename: String,
) {
    glib::MainContext::default().spawn_local(async move {
        let path =
            match ensure_original_asset_path(&ui, &asset_id, &remote_id, &local_path, &filename)
                .await
            {
                Ok(path) => path,
                Err(err) => {
                    show_alert_dialog(&ui, "Copy Failed", &err);
                    return;
                }
            };
        let Some(texture) = load_texture_oriented(&path).await else {
            show_alert_dialog(&ui, "Copy Failed", "Could not decode the original image.");
            return;
        };
        if let Some(display) = gdk4::Display::default() {
            display.clipboard().set_texture(&texture);
        }
    });
}

fn open_asset_in_default_app(
    ui: Rc<LibraryWindowUi>,
    asset_id: String,
    remote_id: String,
    local_path: String,
    filename: String,
) {
    glib::MainContext::default().spawn_local(async move {
        let path =
            match ensure_original_asset_path(&ui, &asset_id, &remote_id, &local_path, &filename)
                .await
            {
                Ok(path) => path,
                Err(err) => {
                    show_alert_dialog(&ui, "Open Failed", &err);
                    return;
                }
            };
        open_local_with_default_app(&path.display().to_string());
    });
}

pub(super) async fn ensure_original_asset_path(
    ui: &LibraryWindowUi,
    asset_id: &str,
    remote_id: &str,
    local_path: &str,
    filename: &str,
) -> Result<PathBuf, String> {
    if !local_path.is_empty() {
        return Ok(PathBuf::from(local_path));
    }
    let remote_asset_id = if !remote_id.is_empty() {
        remote_id
    } else {
        asset_id
    };
    let cache_dir = crate::profile::cache_dir()
        .ok_or_else(|| "Could not locate a cache directory.".to_string())?
        .join("open-in");
    let _ = tokio::fs::create_dir_all(&cache_dir).await;
    let safe_name =
        crate::sanitize::safe_filename(filename).unwrap_or_else(|| asset_id.to_string());
    let path = cache_dir.join(&safe_name);
    if path.exists() {
        return Ok(path);
    }
    begin_download_session(&ui.ctx, filename.to_string());
    let progress = track_download_item(
        &ui.ctx,
        remote_asset_id.to_string(),
        Some(filename.to_string()),
        None,
    );
    let result = ui
        .ctx
        .api_client
        .download_original_to_file(remote_asset_id, &path, Some(progress))
        .await;
    finish_download_item(&ui.ctx, remote_asset_id);
    result.map(|_| path).map_err(|err| err.to_string())
}

pub(super) fn show_alert_dialog(ui: &LibraryWindowUi, heading: &str, body: &str) {
    let alert = libadwaita::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    alert.add_response("ok", "OK");
    alert.present(Some(&ui.window));
}
