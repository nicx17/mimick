//! Right-click context menu popover and its action handlers.
//!
//! Builds a GTK popover menu with per-asset actions: copy, download, open in
//! an external app, edit info, and move to trash. Actions delegate to the
//! shared library helpers.

use gettextrs::gettext;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::library::asset_object::AssetObject;

use super::LOCAL_ID_PREFIX;
use super::LibraryWindowUi;
use super::actions::{TrashTarget, confirm_trash};
use super::archive::{archive_icon, archive_label, set_archived};
use super::asset_edit::show_edit_dialog;
use super::download::{
    begin_download_session, finish_download_item, open_local_with_default_app, start_download,
    track_download_item,
};
use super::favorites::{FavoriteHooks, heart_icon, toggle_favorite};
use super::load_texture_oriented;
use super::trash_view;

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

/// Popover content plus a helper for full-width buttons that close the menu first.
pub(super) struct Menu {
    content: gtk::Box,
    popover: gtk::Popover,
    entries: RefCell<Vec<Entry>>,
}

/// One choice, kept so the menu can open as a popover or as a bottom sheet.
struct Entry {
    icon: Option<&'static str>,
    label: String,
    destructive: bool,
    action: Rc<dyn Fn()>,
}

impl Menu {
    pub(super) fn new(parent: &impl gtk::prelude::IsA<gtk::Widget>, x: f64, y: f64) -> Self {
        let popover = gtk::Popover::builder()
            .has_arrow(true)
            .autohide(true)
            .build();
        popover.set_parent(parent);
        popover.connect_closed(|popover| {
            let popover = popover.clone();
            glib::idle_add_local_once(move || popover.unparent());
        });
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        Self {
            content,
            popover,
            entries: RefCell::default(),
        }
    }

    /// Every choice as an icon tile in a sheet that slides up from the bottom of the window.
    /// The tiles wrap into rows on narrow windows, so all of them show without scrolling.
    pub(super) fn present_sheet(self, parent: &impl IsA<gtk::Widget>) {
        self.popover.unparent();
        let dialog = libadwaita::Dialog::builder()
            .title(gettext("Photo Actions"))
            .presentation_mode(libadwaita::DialogPresentationMode::BottomSheet)
            .follows_content_size(true)
            .build();
        // Five 64 px tiles fit a 360 px phone, so a typical menu takes about three rows.
        let tiles = libadwaita::WrapBox::builder()
            .child_spacing(4)
            .line_spacing(10)
            .halign(gtk::Align::Center)
            .margin_top(18)
            .margin_bottom(12)
            .margin_start(8)
            .margin_end(8)
            .build();
        for entry in self.entries.take() {
            tiles.append(&sheet_tile(&dialog, entry));
        }
        // Scrolls only when even the rows don't fit, as on a phone held sideways.
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_width(true)
            .propagate_natural_height(true)
            .child(&tiles)
            .build();
        dialog.set_child(Some(&scroller));
        dialog.add_controller(close_on_f10(&dialog));
        dialog.present(Some(parent));
    }

    /// Scrollable, so GTK lets the compositor shrink a menu taller than the screen.
    pub(super) fn popup(self) {
        let scroller = gtk::ScrolledWindow::builder()
            .child(&self.content)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(true)
            .propagate_natural_height(true)
            .build();
        self.popover.set_child(Some(&scroller));
        self.popover.popup();
    }

    /// Runs `f` when the menu closes, however it was dismissed.
    pub(super) fn on_closed(&self, f: impl Fn() + 'static) {
        self.popover.connect_closed(move |_| f());
    }

    /// A bold line above the buttons naming what the menu acts on.
    pub(super) fn heading(&self, text: &str) {
        let label = gtk::Label::builder()
            .label(text)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(28)
            .margin_start(6)
            .margin_end(6)
            .margin_top(2)
            .margin_bottom(2)
            .css_classes(["heading"])
            .build();
        self.content.append(&label);
    }

    pub(super) fn add(&self, label: &str, destructive: bool, on_click: impl Fn() + 'static) {
        self.add_entry(None, label, destructive, Rc::new(on_click));
    }

    /// Like `add`, with the icon the bottom sheet shows; the popover stays text-only.
    pub(super) fn add_with_icon(
        &self,
        icon: &'static str,
        label: &str,
        destructive: bool,
        on_click: impl Fn() + 'static,
    ) {
        self.add_entry(Some(icon), label, destructive, Rc::new(on_click));
    }

    fn add_entry(
        &self,
        icon: Option<&'static str>,
        label: &str,
        destructive: bool,
        action: Rc<dyn Fn()>,
    ) {
        let button = gtk::Button::builder()
            .label(label)
            .halign(gtk::Align::Fill)
            .build();
        if destructive {
            button.add_css_class("destructive-action");
        }
        let (popover, on_click) = (self.popover.clone(), action.clone());
        button.connect_clicked(move |_| {
            popover.popdown();
            on_click();
        });
        self.content.append(&button);
        self.entries.borrow_mut().push(Entry {
            icon,
            label: label.to_string(),
            destructive,
            action,
        });
    }
}

/// F10 opens the sheet from the lightbox, so pressing it again closes it.
fn close_on_f10(dialog: &libadwaita::Dialog) -> gtk::ShortcutController {
    let dialog = dialog.downgrade();
    let shortcut = gtk::Shortcut::builder()
        .trigger(&gtk::ShortcutTrigger::parse_string("F10").expect("valid shortcut"))
        .action(&gtk::CallbackAction::new(move |_, _| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
            glib::Propagation::Stop
        }))
        .build();
    let controller = gtk::ShortcutController::new();
    controller.add_shortcut(shortcut);
    controller
}

/// An icon in a round well over its name; picking it closes the sheet first.
fn sheet_tile(dialog: &libadwaita::Dialog, entry: Entry) -> gtk::Button {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .valign(gtk::Align::Start)
        .build();
    content.append(
        &gtk::Image::builder()
            .icon_name(entry.icon.unwrap_or("view-more-symbolic"))
            .pixel_size(20)
            .halign(gtk::Align::Center)
            .css_classes(["mimick-sheet-icon"])
            .build(),
    );
    content.append(
        &gtk::Label::builder()
            .label(&entry.label)
            .justify(gtk::Justification::Center)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(3)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(9)
            .css_classes(["caption"])
            .build(),
    );
    let button = gtk::Button::builder()
        .child(&content)
        .css_classes(["flat", "mimick-sheet-tile"])
        .build();
    if entry.destructive {
        button.add_css_class("mimick-sheet-destructive");
    }
    let (dialog, action) = (dialog.clone(), entry.action);
    button.connect_clicked(move |_| {
        dialog.close();
        action();
    });
    button
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
    append_album_buttons(menu, ui, &remote_id, &hooks);
    super::memories::append_remove_button(menu, ui, &remote_id, hooks.on_trashed.clone());
    let (ui_link, link_id) = (ui.clone(), remote_id.clone());
    menu.add_with_icon(
        "mimick-share-symbolic",
        &gettext("Create Share Link…"),
        false,
        move || {
            super::shared_link_dialog::present(
                ui_link.clone(),
                super::shared_link_dialog::LinkDialogMode::Create(
                    crate::api_client::SharedLinkTarget::Assets(vec![link_id.clone()]),
                ),
                Rc::new(|| {}),
            )
        },
    );
    super::lightbox::append_unstack_button(menu, ui, item);
    if super::actions::is_mine(ui, item) {
        append_edit_buttons(menu, ui, item, remote_id, hooks.on_edited);
    }
    append_trash_button(menu, ui, target, hooks.on_trashed);
}

/// "Add to Album…"; inside an album also "Set as Album Cover" (owner) and "Remove from Album".
fn append_album_buttons(menu: &Menu, ui: &Rc<LibraryWindowUi>, id: &str, hooks: &AssetMenuHooks) {
    let (ui_add, ids) = (ui.clone(), vec![id.to_string()]);
    menu.add_with_icon(
        "list-add-symbolic",
        &gettext("Add to Album…"),
        false,
        move || super::album_picker::present(&ui_add, ids.clone(), Rc::new(|| {})),
    );
    if let Some(album) = super::album_assets::owned_current_album(ui) {
        let (ui_cover, asset_id) = (ui.clone(), id.to_string());
        menu.add_with_icon(
            "image-x-generic-symbolic",
            &gettext("Set as Album Cover"),
            false,
            move || super::album_assets::set_cover(&ui_cover, album.id.clone(), asset_id.clone()),
        );
    }
    if super::album_assets::can_remove_here(ui) {
        let (ui_remove, ids, on_removed) =
            (ui.clone(), vec![id.to_string()], hooks.on_trashed.clone());
        menu.add_with_icon(
            "list-remove-symbolic",
            &gettext("Remove from Album"),
            false,
            move || super::album_assets::remove(&ui_remove, ids.clone(), on_removed.clone()),
        );
    }
}

/// "Edit Info…" and "Set Location…", offered only on the user's own assets as in Immich.
fn append_edit_buttons(
    menu: &Menu,
    ui: &Rc<LibraryWindowUi>,
    item: &AssetObject,
    id: String,
    on_edited: Rc<dyn Fn(bool)>,
) {
    let (ui_edit, filename, edit_id) =
        (ui.clone(), item.property::<String>("filename"), id.clone());
    let on_info_edited = on_edited.clone();
    menu.add_with_icon(
        "document-edit-symbolic",
        &gettext("Edit Info…"),
        false,
        move || {
            show_edit_dialog(
                ui_edit.clone(),
                edit_id.clone(),
                filename.clone(),
                on_info_edited.clone(),
            )
        },
    );
    append_location_button(menu, ui, item, id.clone());
    let is_image = item
        .property::<String>("asset-type")
        .eq_ignore_ascii_case("IMAGE");
    if is_image {
        let (ui, item) = (ui.clone(), item.clone());
        menu.add_with_icon(
            "mimick-crop-symbolic",
            &gettext("Edit Photo…"),
            false,
            move || super::lightbox::edit_photo(&ui, &item),
        );
    }
    if is_image && ui.features.get().people {
        let (ui, item) = (ui.clone(), item.clone());
        menu.add_with_icon(
            "avatar-default-symbolic",
            &gettext("Tag People…"),
            false,
            move || super::lightbox::tag_people(&ui, &item),
        );
    }
}

fn append_location_button(menu: &Menu, ui: &Rc<LibraryWindowUi>, item: &AssetObject, id: String) {
    let (ui, filename) = (ui.clone(), item.property::<String>("filename"));
    menu.add_with_icon(
        "find-location-symbolic",
        &gettext("Set Location…"),
        false,
        move || {
            super::map::set_location_for_asset(
                ui.clone(),
                id.clone(),
                filename.clone(),
                Rc::new(|| {}),
            )
        },
    );
}

fn append_trash_button(
    menu: &Menu,
    ui: &Rc<LibraryWindowUi>,
    target: TrashTarget,
    on_trashed: Rc<dyn Fn()>,
) {
    // The popover is single-use, so the target is handed to the dialog on the first click.
    let target = std::cell::RefCell::new(Some(target));
    let (trash_icon, label) = if ui.features.get().trash {
        ("user-trash-symbolic", gettext("Move to Trash"))
    } else {
        ("edit-delete-symbolic", gettext("Delete…"))
    };
    let label = &label;
    let ui = ui.clone();
    menu.add_with_icon(trash_icon, label, true, move || {
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
    let label = &if item.is_favorite() {
        gettext("Remove from Favorites")
    } else {
        gettext("Add to Favorites")
    };
    let (ui, item) = (ui.clone(), item.clone());
    let (on_edited, on_trashed) = (hooks.on_edited.clone(), hooks.on_trashed.clone());
    menu.add_with_icon(heart_icon(item.is_favorite()), label, false, move || {
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
    menu.add_with_icon(
        archive_icon(archived),
        &archive_label(archived),
        false,
        move || {
            let (on_edited, on_trashed) = (on_edited.clone(), on_trashed.clone());
            set_archived(&ui, vec![item.clone()], !archived, move |removed| {
                if removed {
                    on_trashed()
                } else {
                    on_edited(false)
                }
            });
        },
    );
}

/// Trash view: put the asset back, or delete it for good after confirmation.
fn append_trash_mode_buttons(
    menu: &Menu,
    ui: &Rc<LibraryWindowUi>,
    remote_id: String,
    on_removed: Rc<dyn Fn()>,
) {
    let (ui_restore, id, on_restored) = (ui.clone(), remote_id.clone(), on_removed.clone());
    menu.add_with_icon(
        "edit-undo-symbolic",
        &gettext("Restore"),
        false,
        move || {
            let on_restored = on_restored.clone();
            trash_view::restore(ui_restore.clone(), vec![id.clone()], move || on_restored());
        },
    );
    let ui_delete = ui.clone();
    menu.add_with_icon(
        "edit-delete-symbolic",
        &gettext("Delete Permanently"),
        true,
        move || {
            let on_removed = on_removed.clone();
            trash_view::confirm_delete_permanently(
                ui_delete.clone(),
                vec![remote_id.clone()],
                move || on_removed(),
            );
        },
    );
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
        if let Some(display) = gdk4::Display::default() {
            display.clipboard().set_texture(&texture);
        }
    });
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
