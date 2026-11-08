//! Moving photos to the Immich trash: the confirmation, the request, and the grid update.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::rc::Rc;

use glib::prelude::*;
use libadwaita::prelude::*;

use crate::library::asset_object::AssetObject;
use crate::library::{LOCAL_ID_PREFIX, LibraryWindowUi};

use super::selected_items;

/// A server asset the user asked to move to the Immich trash.
pub(in crate::library) struct TrashTarget {
    /// Read directly by the bulk actions, which take the ids out.
    pub(super) remote_id: String,
    filename: String,
    has_local_copy: bool,
}

impl TrashTarget {
    pub(in crate::library) fn remote_id(&self) -> &str {
        &self.remote_id
    }

    /// `None` for local-only items, which have nothing on the server to trash.
    pub(in crate::library) fn from_item(item: &AssetObject) -> Option<Self> {
        let id = item.property::<String>("id");
        let remote_id = item.property::<String>("remote-id");
        let remote_id = if remote_id.is_empty() { id } else { remote_id };
        if remote_id.is_empty() || remote_id.starts_with(LOCAL_ID_PREFIX) {
            return None;
        }
        Some(Self {
            remote_id,
            filename: item.property("filename"),
            has_local_copy: !item.property::<String>("local-path").is_empty(),
        })
    }
}

/// Heading and body for the trash confirmation dialog.
fn trash_dialog_text(
    targets: &[TrashTarget],
    skipped_local: usize,
    trash: bool,
) -> (String, String) {
    (
        trash_heading(targets, trash),
        trash_body(targets, skipped_local, trash),
    )
}

/// Names the file when there's one, else counts them.
fn trash_heading(targets: &[TrashTarget], trash: bool) -> String {
    match (targets, trash) {
        ([single], true) => fill(
            gettext("Move “{filename}” to trash?"),
            &[("filename", &single.filename.to_string())],
        ),
        (_, true) => fill(
            ngettext(
                "Move {count} item to trash?",
                "Move {count} items to trash?",
                plural_n(targets.len()),
            ),
            &[("count", &targets.len().to_string())],
        ),
        ([single], false) => fill(
            gettext("Delete “{filename}” permanently?"),
            &[("filename", &single.filename.to_string())],
        ),
        (_, false) => fill(
            ngettext(
                "Delete {count} item permanently?",
                "Delete {count} items permanently?",
                plural_n(targets.len()),
            ),
            &[("count", &targets.len().to_string())],
        ),
    }
}

/// Whether it can be undone, that local copies stay, and how many local-only items are skipped.
fn trash_body(targets: &[TrashTarget], skipped_local: usize, trash: bool) -> String {
    let mut body = if trash {
        gettext("Items can be restored from the Immich trash.")
    } else {
        gettext("The Immich trash is turned off, so this can't be undone.")
    };
    if targets.iter().any(|t| t.has_local_copy) {
        body.push(' ');
        body.push_str(&gettext("Local copies in your watch folders are kept."));
    }
    if skipped_local > 0 {
        body.push_str("\n\n");
        body.push_str(&fill(
            ngettext(
                "{n} local-only item is not on the server and will be skipped.",
                "{n} local-only items are not on the server and will be skipped.",
                plural_n(skipped_local),
            ),
            &[("n", &skipped_local.to_string())],
        ));
    }
    body
}

/// Ask before moving `targets` to the Immich trash; `on_trashed` runs after the server confirms.
pub(in crate::library) fn confirm_trash(
    ui: Rc<LibraryWindowUi>,
    targets: Vec<TrashTarget>,
    skipped_local: usize,
    on_trashed: impl Fn() + 'static,
) {
    if targets.is_empty() {
        return;
    }
    let trash = ui.features.get().trash;
    let dialog = trash_dialog(&targets, skipped_local, trash);
    let ids: Vec<String> = targets.into_iter().map(|t| t.remote_id).collect();
    let on_trashed = Rc::new(on_trashed);
    let ui_for_choice = ui.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "delete" {
            return;
        }
        let ui = ui_for_choice.clone();
        let ids = ids.clone();
        let on_trashed = on_trashed.clone();
        glib::MainContext::default().spawn_local(async move {
            match trash_assets(&ui, &ids).await {
                Ok(()) => on_trashed(),
                Err(err) => {
                    log::error!("Move to trash failed: {}", err);
                    crate::library::context_menu::show_alert_dialog(
                        &ui,
                        &gettext("Could Not Move to Trash"),
                        &fill(
                            gettext("Immich rejected the request: {err}"),
                            &[("err", &err.to_string())],
                        ),
                    );
                }
            }
        });
    });
    dialog.present(Some(&ui.window));
}

/// Cancel is the default, so Enter doesn't trash anything.
fn trash_dialog(
    targets: &[TrashTarget],
    skipped_local: usize,
    trash: bool,
) -> libadwaita::AlertDialog {
    let (heading, body) = trash_dialog_text(targets, skipped_local, trash);
    let dialog = libadwaita::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    let confirm = if trash {
        gettext("Move to trash")
    } else {
        gettext("Delete")
    };
    dialog.add_response("delete", &confirm);
    dialog.set_response_appearance("delete", libadwaita::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog
}

/// Trash on the server, then drop the items from the grid.
async fn trash_assets(ui: &Rc<LibraryWindowUi>, ids: &[String]) -> Result<(), String> {
    ui.ctx.api_client.delete_assets(ids).await?;
    remove_assets_from_grid(ui, ids);
    Ok(())
}

/// Drop assets from the grid in place, so open views (like the lightbox) keep their
/// position instead of reloading from the first page; then refresh album/stat counts.
pub(in crate::library) fn remove_assets_from_grid(ui: &Rc<LibraryWindowUi>, ids: &[String]) {
    if !crate::library::timeline::remove(ui, ids) {
        ui.ctx
            .library_state
            .lock()
            .assets
            .retain(|asset| !ids.contains(&asset.id));
        // Outside the lock: items_changed handlers may read the library state.
        ui.grid.model.remove_ids(ids);
    }
    crate::library::refresh_library_after_mutation(ui.clone(), false);
}

pub(super) fn confirm_and_bulk_delete(ui: Rc<LibraryWindowUi>) {
    let items = selected_items(&ui);
    let targets: Vec<TrashTarget> = items.iter().filter_map(TrashTarget::from_item).collect();
    let skipped_local = items.len() - targets.len();
    let ui_after = ui.clone();
    confirm_trash(ui, targets, skipped_local, move || {
        ui_after.grid.selection.unselect_all();
        ui_after.select_toggle.set_active(false);
    });
}

#[cfg(test)]
mod tests {
    use super::{TrashTarget, trash_dialog_text};
    use crate::library::LOCAL_ID_PREFIX;
    use crate::library::asset_object::{AssetInit, AssetObject};
    use glib::prelude::ObjectExt;

    fn remote_item(id: &str) -> AssetObject {
        AssetObject::new(AssetInit {
            id,
            filename: "IMG_1.jpg",
            mime_type: "image/jpeg",
            created_at: "2024-01-15T14:25:15.000Z",
            asset_type: "IMAGE",
            sync_state: 0,
            thumbhash: None,
            width: 0,
            height: 0,
            is_favorite: false,
            is_archived: false,
            is_edited: false,
            owner_id: "",
            local_date_time: "",
        })
    }

    #[test]
    fn trash_target_uses_remote_id_for_server_assets() {
        let target = TrashTarget::from_item(&remote_item("remote-1")).expect("server asset");
        assert_eq!(target.remote_id(), "remote-1");
        assert_eq!(target.filename, "IMG_1.jpg");
        assert!(!target.has_local_copy);
    }

    #[test]
    fn trash_target_notes_a_local_copy() {
        let item = remote_item("remote-1");
        item.set_property("local-path", "/home/user/Pictures/IMG_1.jpg");
        let target = TrashTarget::from_item(&item).expect("server asset");
        assert!(target.has_local_copy);
    }

    #[test]
    fn trash_target_skips_local_only_assets() {
        let path = "/home/user/Pictures/IMG_2.jpg";
        let item = AssetObject::new_local(
            &format!("{LOCAL_ID_PREFIX}{path}"),
            "IMG_2.jpg",
            "image/jpeg",
            "2024-01-15T14:25:15.000Z",
            "IMAGE",
            path,
        );
        assert!(TrashTarget::from_item(&item).is_none());
    }

    fn target(name: &str, has_local_copy: bool) -> TrashTarget {
        TrashTarget {
            remote_id: format!("id-{name}"),
            filename: name.to_string(),
            has_local_copy,
        }
    }

    #[test]
    fn single_item_heading_names_the_file() {
        let (heading, body) = trash_dialog_text(&[target("IMG_1.jpg", false)], 0, true);
        assert_eq!(heading, "Move “IMG_1.jpg” to trash?");
        assert_eq!(body, "Items can be restored from the Immich trash.");
    }

    #[test]
    fn body_mentions_kept_local_copies_and_skipped_items() {
        let targets = [target("a.jpg", true), target("b.jpg", false)];
        let (heading, body) = trash_dialog_text(&targets, 2, true);
        assert_eq!(heading, "Move 2 items to trash?");
        assert!(body.contains("Local copies in your watch folders are kept."));
        assert!(body.contains("2 local-only items are not on the server"));
    }

    #[test]
    fn without_trash_the_dialog_warns_deletion_is_permanent() {
        let (heading, body) = trash_dialog_text(&[target("IMG_1.jpg", false)], 0, false);
        assert_eq!(heading, "Delete “IMG_1.jpg” permanently?");
        assert!(body.contains("can't be undone"));
    }
}
