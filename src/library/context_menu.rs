//! Right-click context menu popover and its action handlers.
//!
//! Builds a GTK popover menu with per-asset actions: copy, download, open in
//! an external app, edit info, and move to trash. Actions delegate to the
//! shared library helpers.

use gettextrs::gettext;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::library::asset_object::AssetObject;

use super::LOCAL_ID_PREFIX;
use super::LibraryWindowUi;
use super::download::{
    begin_download_session, finish_download_item, open_local_with_default_app, start_download,
    track_download_item,
};
use super::load_texture_oriented;
use super::trash_view;

mod manage;
mod menu;

use manage::append_manage_buttons;
pub(super) use menu::Menu;

/// What the view that opened the menu does after its asset is trashed or edited.
pub(super) struct AssetMenuHooks {
    pub on_trashed: Rc<dyn Fn()>,
    /// Receives whether the capture date changed.
    pub on_edited: Rc<dyn Fn(bool)>,
    /// Only the lightbox offers Fullscreen.
    pub toggle_fullscreen: Option<Rc<dyn Fn()>>,
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
            toggle_fullscreen: None,
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

/// The photo a menu acts on, and the grid position slideshows start from.
pub(super) struct MenuTarget {
    pub item: AssetObject,
    pub position: u32,
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
    show_item_context_menu(ui, parent, MenuTarget { item, position }, x, y, hooks);
}

pub(super) fn show_item_context_menu(
    ui: Rc<LibraryWindowUi>,
    parent: &impl gtk::prelude::IsA<gtk::Widget>,
    target: MenuTarget,
    x: f64,
    y: f64,
    hooks: AssetMenuHooks,
) {
    item_menu(ui, parent, target, x, y, hooks).popup();
}

/// The photo's menu, ready to pop up at (`x`, `y`) on `parent` or to present as a sheet.
pub(super) fn item_menu(
    ui: Rc<LibraryWindowUi>,
    parent: &impl gtk::prelude::IsA<gtk::Widget>,
    target: MenuTarget,
    x: f64,
    y: f64,
    hooks: AssetMenuHooks,
) -> Menu {
    let MenuTarget { item, position } = target;
    let asset = MenuAsset::from_item(&item);
    let is_image = item
        .property::<String>("asset-type")
        .eq_ignore_ascii_case("IMAGE");

    let menu = Menu::new(parent, x, y);

    if is_image {
        let (ui, asset) = (ui.clone(), asset.clone());
        menu.add_with_icon("edit-copy-symbolic", &gettext("Copy"), false, move || {
            copy_asset_to_clipboard(ui.clone(), asset.clone())
        });
    }
    if asset.on_server() {
        let (ui, asset) = (ui.clone(), asset.clone());
        menu.add_with_icon(
            "mimick-download-symbolic",
            &gettext("Download"),
            false,
            move || start_download(ui.clone(), asset.remote_id.clone(), asset.filename.clone()),
        );
    }
    if asset.on_server() || !asset.local_path.is_empty() {
        let (ui, asset) = (ui.clone(), asset.clone());
        menu.add_with_icon(
            "document-open-symbolic",
            &gettext("Open In"),
            false,
            move || open_asset_in_default_app(ui.clone(), asset.clone()),
        );
    }
    append_viewing_buttons(&menu, &ui, position, &hooks);
    if asset.on_server() && !trash_view::is_trash_active(&ui) {
        super::related::append_related_buttons(&menu, &ui, &item);
    }
    if asset.on_server() {
        append_manage_buttons(&menu, &ui, &item, hooks);
    }
    menu
}

/// Slideshow from this photo, and Fullscreen when the menu belongs to the lightbox.
fn append_viewing_buttons(
    menu: &Menu,
    ui: &Rc<LibraryWindowUi>,
    position: u32,
    hooks: &AssetMenuHooks,
) {
    if !trash_view::is_trash_active(ui) {
        let ui = ui.clone();
        menu.add_with_icon(
            "mimick-slideshow-symbolic",
            &gettext("Slideshow"),
            false,
            move || super::lightbox::start_slideshow_at(&ui, position, None),
        );
    }
    if let Some(toggle) = hooks.toggle_fullscreen.clone() {
        let (icon, label) = if ui.window.is_fullscreen() {
            ("view-restore-symbolic", gettext("Leave Fullscreen"))
        } else {
            ("view-fullscreen-symbolic", gettext("Fullscreen"))
        };
        menu.add_with_icon(icon, &label, false, move || toggle());
    }
}

/// Calls `open` on right-click and on touch long-press, which also swallows the tap.
pub(super) fn connect_context_menu(
    widget: &impl IsA<gtk::Widget>,
    open: impl Fn(f64, f64) + 'static,
) {
    let open = Rc::new(open);
    let click = gtk::GestureClick::builder()
        .button(gtk::gdk::BUTTON_SECONDARY)
        .build();
    let on_click = open.clone();
    click.connect_pressed(move |_, _, x, y| on_click(x, y));
    widget.add_controller(click);

    let press = gtk::GestureLongPress::builder().touch_only(true).build();
    press.connect_pressed(move |gesture, x, y| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        open(x, y);
    });
    widget.add_controller(press);
}

/// Shift+F10 and the Menu key while focus is inside the widget it's added to.
pub(super) fn menu_key(open: impl Fn() + 'static) -> gtk::ShortcutController {
    let shortcut = gtk::Shortcut::builder()
        .trigger(&gtk::ShortcutTrigger::parse_string("<Shift>F10|Menu").expect("valid shortcut"))
        .action(&gtk::CallbackAction::new(move |_, _| {
            open();
            glib::Propagation::Stop
        }))
        .build();
    let controller = gtk::ShortcutController::new();
    controller.add_shortcut(shortcut);
    controller
}

fn copy_asset_to_clipboard(ui: Rc<LibraryWindowUi>, asset: MenuAsset) {
    glib::MainContext::default().spawn_local(async move {
        let path = match original_path(&ui, &asset).await {
            Ok(path) => path,
            Err(err) => {
                show_error_toast(&ui, &gettext("Couldn't copy the photo"), &err);
                return;
            }
        };
        let Some(texture) = load_texture_oriented(&path).await else {
            show_error_toast(
                &ui,
                &gettext("Couldn't copy the photo"),
                &gettext("Could not decode the original image."),
            );
            return;
        };
        let provider = gdk4::ContentProvider::for_value(&texture.to_value());
        let photo = super::clipboard::CopiedPhoto::new(&asset.remote_id, Some(path));
        super::clipboard::set_copied(&ui, provider, vec![photo]);
    });
}

/// Ctrl+C on one photo: its picture, as the menu's Copy gives.
pub(super) fn copy_picture(ui: &Rc<LibraryWindowUi>, item: &AssetObject) {
    copy_asset_to_clipboard(ui.clone(), MenuAsset::from_item(item));
}

fn open_asset_in_default_app(ui: Rc<LibraryWindowUi>, asset: MenuAsset) {
    glib::MainContext::default().spawn_local(async move {
        match original_path(&ui, &asset).await {
            Ok(path) => open_local_with_default_app(&path.display().to_string()),
            Err(err) => show_error_toast(&ui, &gettext("Couldn't open the file"), &err),
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
        .ok_or_else(|| gettext("Could not locate a cache directory."))?
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

/// A failed quick action as a toast; "Details" opens the full message.
pub(super) fn show_error_toast(ui: &LibraryWindowUi, title: &str, details: &str) {
    let (window, title_text, details) = (ui.window.clone(), title.to_string(), details.to_string());
    let toast = super::dialogs::toast_with_action(title, &gettext("Details"), move || {
        let alert = libadwaita::AlertDialog::builder()
            .heading(&title_text)
            .body(&details)
            .build();
        alert.add_response("ok", &gettext("OK"));
        alert.present(Some(&window));
    });
    ui.toasts.add_toast(toast);
}

pub(super) fn show_alert_dialog(ui: &LibraryWindowUi, heading: &str, body: &str) {
    show_alert_on(&ui.window, heading, body);
}

pub(super) fn show_alert_on(parent: &impl IsA<gtk::Widget>, heading: &str, body: &str) {
    let alert = libadwaita::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    alert.add_response("ok", &gettext("OK"));
    alert.present(Some(parent));
}
