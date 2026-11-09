//! The menu's actions that change a photo on the server: albums, info, location, favorite,
//! archive, and trash, or restore and delete while the Trash view is open.

use gettextrs::gettext;
use std::rc::Rc;

use gtk::prelude::*;

use super::{AssetMenuHooks, Menu};
use crate::library::LibraryWindowUi;
use crate::library::actions::{TrashTarget, confirm_trash};
use crate::library::archive::{archive_icon, archive_label, set_archived};
use crate::library::asset_edit::show_edit_dialog;
use crate::library::asset_object::AssetObject;
use crate::library::favorites::{FavoriteHooks, heart_icon, toggle_favorite};
use crate::library::trash_view;

/// "Edit Info…" and "Move to Trash" for assets that exist on the server;
/// "Restore" and "Delete Permanently" instead while the Trash view is open.
pub(super) fn append_manage_buttons(
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
    crate::library::memories::append_remove_button(menu, ui, &remote_id, hooks.on_trashed.clone());
    append_share_buttons(menu, ui, &remote_id);
    crate::library::lightbox::append_unstack_button(menu, ui, item);
    if crate::library::actions::is_mine(ui, item) {
        // A new stack cover regroups the grid, as a changed date does.
        let on_edited = hooks.on_edited.clone();
        let on_stacked: Rc<dyn Fn()> = Rc::new(move || on_edited(true));
        crate::library::upload_stack::append_button(menu, ui, item, remote_id.clone(), on_stacked);
        append_edit_buttons(menu, ui, item, remote_id.clone(), hooks.on_edited);
        append_profile_button(menu, ui, item, remote_id);
        crate::library::asset_jobs::append_refresh_buttons(menu, ui, item);
    }
    append_trash_button(menu, ui, target, hooks.on_trashed);
}

/// "Create Share Link…" for a new link, and "Add to Share Link…" for one made before.
fn append_share_buttons(menu: &Menu, ui: &Rc<LibraryWindowUi>, id: &str) {
    let (ui_link, link_id) = (ui.clone(), id.to_string());
    menu.add_with_icon(
        "mimick-share-symbolic",
        &gettext("Create Share Link…"),
        false,
        move || {
            crate::library::shared_link_dialog::present(
                ui_link.clone(),
                crate::library::shared_link_dialog::LinkDialogMode::Create(
                    crate::api_client::SharedLinkTarget::Assets(vec![link_id.clone()]),
                ),
                Rc::new(|| {}),
            )
        },
    );
    let (ui_add, add_id) = (ui.clone(), id.to_string());
    menu.add_with_icon(
        "insert-link-symbolic",
        &gettext("Add to Share Link…"),
        false,
        move || crate::library::shared_link_photos::add_to_link(&ui_add, vec![add_id.clone()]),
    );
}

/// "Add to Album…"; inside an album also "Set as Album Cover" (owner) and "Remove from Album".
fn append_album_buttons(menu: &Menu, ui: &Rc<LibraryWindowUi>, id: &str, hooks: &AssetMenuHooks) {
    let (ui_add, ids) = (ui.clone(), vec![id.to_string()]);
    menu.add_with_icon(
        "list-add-symbolic",
        &gettext("Add to Album…"),
        false,
        move || crate::library::album_picker::present(&ui_add, ids.clone(), Rc::new(|| {})),
    );
    if let Some(album) = crate::library::album_assets::owned_current_album(ui) {
        let (ui_cover, asset_id) = (ui.clone(), id.to_string());
        menu.add_with_icon(
            "image-x-generic-symbolic",
            &gettext("Set as Album Cover"),
            false,
            move || {
                crate::library::album_assets::set_cover(
                    &ui_cover,
                    album.id.clone(),
                    asset_id.clone(),
                )
            },
        );
    }
    if crate::library::album_assets::can_remove_here(ui) {
        let (ui_remove, ids, on_removed) =
            (ui.clone(), vec![id.to_string()], hooks.on_trashed.clone());
        menu.add_with_icon(
            "list-remove-symbolic",
            &gettext("Remove from Album"),
            false,
            move || {
                crate::library::album_assets::remove(&ui_remove, ids.clone(), on_removed.clone())
            },
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
            move || crate::library::lightbox::edit_photo(&ui, &item),
        );
    }
    if is_image && ui.features.get().people {
        let (ui, item) = (ui.clone(), item.clone());
        menu.add_with_icon(
            "avatar-default-symbolic",
            &gettext("Tag People…"),
            false,
            move || crate::library::lightbox::tag_people(&ui, &item),
        );
    }
}

/// "Set as Profile Picture" on your own photos, as in Immich's viewer menu.
fn append_profile_button(menu: &Menu, ui: &Rc<LibraryWindowUi>, item: &AssetObject, id: String) {
    let is_image = item
        .property::<String>("asset-type")
        .eq_ignore_ascii_case("IMAGE");
    if !is_image {
        return;
    }
    let ui = ui.clone();
    menu.add_with_icon(
        "avatar-default-symbolic",
        &gettext("Set as Profile Picture"),
        false,
        move || crate::library::account::set_from_photo(&ui, id.clone()),
    );
}

fn append_location_button(menu: &Menu, ui: &Rc<LibraryWindowUi>, item: &AssetObject, id: String) {
    let (ui, filename) = (ui.clone(), item.property::<String>("filename"));
    menu.add_with_icon(
        "find-location-symbolic",
        &gettext("Set Location…"),
        false,
        move || {
            crate::library::map::set_location_for_asset(
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
