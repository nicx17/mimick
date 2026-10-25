//! Right-click context menu popover and its action handlers.
//!
//! Builds a GTK popover menu with per-asset actions: copy, download, open in
//! an external app, edit info, and move to trash. Actions delegate to the
//! shared library helpers.

use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::library::asset_object::AssetObject;

use super::LOCAL_ID_PREFIX;
use super::LibraryWindowUi;
use super::actions::{TrashTarget, confirm_trash};
use super::archive::{archive_label, set_archived};
use super::asset_edit::show_edit_dialog;
use super::download::{
    begin_download_session, finish_download_item, open_local_with_default_app, start_download,
    track_download_item,
};
use super::favorites::{FavoriteHooks, toggle_favorite};
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

/// The menu's asset, captured once so each button can clone what it needs.
#[derive(Clone)]
struct MenuAsset {
    asset_id: String,
    remote_id: String,
    local_path: String,
    filename: String,
}

impl MenuAsset {
    fn from_item(item: &AssetObject) -> Self {
        Self {
            asset_id: item.property("id"),
            remote_id: item.property("remote-id"),
            local_path: item.property("local-path"),
            filename: item.property("filename"),
        }
    }

    fn on_server(&self) -> bool {
        !self.remote_id.is_empty() && !self.asset_id.starts_with(LOCAL_ID_PREFIX)
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
    let asset = MenuAsset::from_item(&item);
    let is_image = item
        .property::<String>("asset-type")
        .eq_ignore_ascii_case("IMAGE");

    let menu = Menu::new(parent, x, y);

    if is_image {
        let (ui, asset) = (ui.clone(), asset.clone());
        menu.add("Copy", false, move || {
            copy_asset_to_clipboard(ui.clone(), asset.clone())
        });
    }
    if asset.on_server() {
        let (ui, asset) = (ui.clone(), asset.clone());
        menu.add("Download", false, move || {
            start_download(ui.clone(), asset.remote_id.clone(), asset.filename.clone())
        });
    }
    if asset.on_server() || !asset.local_path.is_empty() {
        let (ui, asset) = (ui.clone(), asset.clone());
        menu.add("Open In", false, move || {
            open_asset_in_default_app(ui.clone(), asset.clone())
        });
    }
    if asset.on_server() {
        append_manage_buttons(&menu, &ui, &item, hooks);
    }

    menu.popover.set_child(Some(&menu.content));
    menu.popover.popup();
}

/// Popover content plus a helper for full-width buttons that close the menu first.
struct Menu {
    content: gtk::Box,
    popover: gtk::Popover,
}

impl Menu {
    fn new(parent: &impl gtk::prelude::IsA<gtk::Widget>, x: f64, y: f64) -> Self {
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
        Self { content, popover }
    }

    fn add(&self, label: &str, destructive: bool, on_click: impl Fn() + 'static) {
        let button = gtk::Button::builder()
            .label(label)
            .halign(gtk::Align::Fill)
            .build();
        if destructive {
            button.add_css_class("destructive-action");
        }
        let popover = self.popover.clone();
        button.connect_clicked(move |_| {
            popover.popdown();
            on_click();
        });
        self.content.append(&button);
    }
}

/// "Edit Info…" and "Move to Trash" for assets that exist on the server;
/// "Restore" and "Delete Permanently" instead while the Trash view is open.
fn append_manage_buttons(
    menu: &Menu,
    ui: &Rc<LibraryWindowUi>,
    item: &AssetObject,
    hooks: AssetMenuHooks,
) {
    let Some(target) = TrashTarget::from_item(item) else {
        return;
    };
    let remote_id = target.remote_id().to_string();
    if trash_view::is_trash_active(ui) {
        append_trash_mode_buttons(menu, ui, remote_id, hooks.on_trashed);
        return;
    }

    append_favorite_button(menu, ui, item, &hooks);
    append_archive_button(menu, ui, item, &hooks);
    let (ui_link, link_id) = (ui.clone(), remote_id.clone());
    menu.add("Create Share Link…", false, move || {
        super::shared_link_dialog::present(
            ui_link.clone(),
            super::shared_link_dialog::LinkDialogMode::Create(
                crate::api_client::SharedLinkTarget::Assets(vec![link_id.clone()]),
            ),
            Rc::new(|| {}),
        )
    });
    let (ui_edit, filename, on_edited) = (
        ui.clone(),
        item.property::<String>("filename"),
        hooks.on_edited,
    );
    let edit_id = remote_id.clone();
    menu.add("Edit Info…", false, move || {
        show_edit_dialog(
            ui_edit.clone(),
            edit_id.clone(),
            filename.clone(),
            on_edited.clone(),
        )
    });
    append_location_button(menu, ui, item, remote_id);
    append_trash_button(menu, ui, target, hooks.on_trashed);
}

fn append_location_button(menu: &Menu, ui: &Rc<LibraryWindowUi>, item: &AssetObject, id: String) {
    let (ui, filename) = (ui.clone(), item.property::<String>("filename"));
    menu.add("Set Location…", false, move || {
        super::map::set_location_for_asset(ui.clone(), id.clone(), filename.clone(), Rc::new(|| {}))
    });
}

fn append_trash_button(
    menu: &Menu,
    ui: &Rc<LibraryWindowUi>,
    target: TrashTarget,
    on_trashed: Rc<dyn Fn()>,
) {
    // The popover is single-use, so the target is handed to the dialog on the first click.
    let target = std::cell::RefCell::new(Some(target));
    let label = if ui.features.get().trash {
        "Move to Trash"
    } else {
        "Delete…"
    };
    let ui = ui.clone();
    menu.add(label, true, move || {
        if let Some(target) = target.borrow_mut().take() {
            let on_trashed = on_trashed.clone();
            confirm_trash(ui.clone(), vec![target], 0, move || on_trashed());
        }
    });
}

/// "Add to / Remove from Favorites". The opening view is told through `on_edited` so a
/// lightbox can refresh its heart, and through `on_trashed` when the Favorites view drops it.
fn append_favorite_button(
    menu: &Menu,
    ui: &Rc<LibraryWindowUi>,
    item: &AssetObject,
    hooks: &AssetMenuHooks,
) {
    let label = if item.is_favorite() {
        "Remove from Favorites"
    } else {
        "Add to Favorites"
    };
    let (ui, item) = (ui.clone(), item.clone());
    let (on_edited, on_trashed) = (hooks.on_edited.clone(), hooks.on_trashed.clone());
    menu.add(label, false, move || {
        let on_edited = on_edited.clone();
        let hooks = FavoriteHooks {
            on_change: Rc::new(move |_| on_edited(false)),
            on_removed: on_trashed.clone(),
        };
        toggle_favorite(&ui, &item, hooks);
    });
}

fn append_archive_button(
    menu: &Menu,
    ui: &Rc<LibraryWindowUi>,
    item: &AssetObject,
    hooks: &AssetMenuHooks,
) {
    let archived = item.is_archived();
    let (ui, item) = (ui.clone(), item.clone());
    let (on_edited, on_trashed) = (hooks.on_edited.clone(), hooks.on_trashed.clone());
    menu.add(archive_label(archived), false, move || {
        let (on_edited, on_trashed) = (on_edited.clone(), on_trashed.clone());
        set_archived(&ui, vec![item.clone()], !archived, move |removed| {
            if removed {
                on_trashed()
            } else {
                on_edited(false)
            }
        });
    });
}

/// Trash view: put the asset back, or delete it for good after confirmation.
fn append_trash_mode_buttons(
    menu: &Menu,
    ui: &Rc<LibraryWindowUi>,
    remote_id: String,
    on_removed: Rc<dyn Fn()>,
) {
    let (ui_restore, id, on_restored) = (ui.clone(), remote_id.clone(), on_removed.clone());
    menu.add("Restore", false, move || {
        let on_restored = on_restored.clone();
        trash_view::restore(ui_restore.clone(), vec![id.clone()], move || on_restored());
    });
    let ui_delete = ui.clone();
    menu.add("Delete Permanently", true, move || {
        let on_removed = on_removed.clone();
        trash_view::confirm_delete_permanently(
            ui_delete.clone(),
            vec![remote_id.clone()],
            move || on_removed(),
        );
    });
}

fn copy_asset_to_clipboard(ui: Rc<LibraryWindowUi>, asset: MenuAsset) {
    glib::MainContext::default().spawn_local(async move {
        let path = match original_path(&ui, &asset).await {
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

fn open_asset_in_default_app(ui: Rc<LibraryWindowUi>, asset: MenuAsset) {
    glib::MainContext::default().spawn_local(async move {
        match original_path(&ui, &asset).await {
            Ok(path) => open_local_with_default_app(&path.display().to_string()),
            Err(err) => show_alert_dialog(&ui, "Open Failed", &err),
        }
    });
}

async fn original_path(ui: &LibraryWindowUi, asset: &MenuAsset) -> Result<PathBuf, String> {
    ensure_original_asset_path(
        ui,
        &asset.asset_id,
        &asset.remote_id,
        &asset.local_path,
        &asset.filename,
    )
    .await
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
