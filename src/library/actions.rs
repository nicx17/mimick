//! Selection-mode wiring and bulk batch operations.
//!
//! Manages multi-select mode toggling, selection-count tracking, and
//! batch actions such as bulk download, bulk delete-to-trash, and
//! bulk add-to-album for the library grid.

use std::cell::Cell;
use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::SharedLinkTarget;
use crate::library::asset_object::AssetObject;

use super::LOCAL_ID_PREFIX;
use super::LibraryWindowUi;
use super::context_menu::show_alert_dialog;
use super::download::start_download_group;

pub(super) fn connect_select_mode(ui: Rc<LibraryWindowUi>, select_toggle: gtk::ToggleButton) {
    let selection = ui.grid.selection.clone();
    let bulk_bar = ui.bulk.bar.clone();
    let count_label = ui.bulk.count_label.clone();

    let refresh = {
        let selection = selection.clone();
        let bulk_bar = bulk_bar.clone();
        let count_label = count_label.clone();
        let select_toggle = select_toggle.clone();
        Rc::new(move || {
            let n = selection_count(&selection);
            bulk_bar.set_reveal_child(select_toggle.is_active() && n > 0);
            count_label.set_label(&format!("{} selected", n));
        })
    };

    selection.connect_selection_changed({
        let refresh = refresh.clone();
        move |_, _, _| (*refresh)()
    });

    select_toggle.connect_toggled({
        let selection = selection.clone();
        let refresh = refresh.clone();
        move |toggle| {
            if !toggle.is_active() {
                selection.unselect_all();
            }
            (*refresh)();
        }
    });

    connect_transient_select_keys(ui, select_toggle, selection);
}

fn connect_transient_select_keys(
    ui: Rc<LibraryWindowUi>,
    select_toggle: gtk::ToggleButton,
    selection: gtk::MultiSelection,
) {
    // Ctrl-hold transient checkboxes: pressing Ctrl reveals the selection UI;
    // releasing Ctrl without having selected anything dismisses it again.
    // Ctrl+click then commits a selection (handled separately in grid_view)
    // and the release no longer collapses select mode.
    let transient = Rc::new(Cell::new(false));
    let key_controller = gtk::EventControllerKey::new();
    key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    key_controller.connect_key_pressed({
        let select_toggle = select_toggle.clone();
        let transient = transient.clone();
        move |_, keyval, _, _| {
            if keyval == gtk::gdk::Key::Escape && select_toggle.is_active() {
                select_toggle.set_active(false);
                transient.set(false);
                return glib::Propagation::Stop;
            }
            if matches!(keyval, gtk::gdk::Key::Control_L | gtk::gdk::Key::Control_R)
                && !select_toggle.is_active()
            {
                select_toggle.set_active(true);
                transient.set(true);
            }
            glib::Propagation::Proceed
        }
    });
    key_controller.connect_key_released({
        let select_toggle = select_toggle.clone();
        let selection = selection.clone();
        let transient = transient.clone();
        move |_, keyval, _, _| {
            if !matches!(keyval, gtk::gdk::Key::Control_L | gtk::gdk::Key::Control_R)
                || !transient.get()
            {
                return;
            }
            if selection.selection().size() == 0 {
                select_toggle.set_active(false);
            }
            transient.set(false);
        }
    });
    ui.window.add_controller(key_controller);
}

pub(super) fn connect_bulk_actions(
    ui: Rc<LibraryWindowUi>,
    delete_btn: gtk::Button,
    download_btn: gtk::Button,
    clear_btn: gtk::Button,
) {
    clear_btn.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| {
            ui.grid.selection.unselect_all();
            ui.select_toggle.set_active(false);
        }
    ));

    download_btn.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| {
            let downloads: Vec<(String, String)> = collect_selected_assets(&ui)
                .into_iter()
                .filter(|(asset_id, _)| !asset_id.starts_with(LOCAL_ID_PREFIX))
                .collect();
            if !downloads.is_empty() {
                start_download_group(ui.clone(), downloads);
            }
        }
    ));

    delete_btn.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| {
            confirm_and_bulk_delete(ui.clone());
        }
    ));

    connect_delete_key(ui);
}

fn connect_delete_key(ui: Rc<LibraryWindowUi>) {
    // On the grid's scroller (not the window) so Delete never fires while typing
    // in the search box or while the lightbox page is showing.
    let delete_key = gtk::EventControllerKey::new();
    delete_key.connect_key_pressed(clone!(
        #[strong]
        ui,
        move |_, keyval, _, _| {
            if keyval != gtk::gdk::Key::Delete || selection_count(&ui.grid.selection) == 0 {
                return glib::Propagation::Proceed;
            }
            if super::trash_view::is_trash_active(&ui) {
                ui.bulk.trash.delete_selected.emit_clicked();
            } else {
                confirm_and_bulk_delete(ui.clone());
            }
            glib::Propagation::Stop
        }
    ));
    ui.grid.scrolled.add_controller(delete_key);
}

/// Selection-bar button: create one public link for every selected server asset.
pub(super) fn connect_bulk_share_link(ui: Rc<LibraryWindowUi>, button: &gtk::Button) {
    button.connect_clicked(move |_| {
        let ids = selected_server_ids(&ui);
        if ids.is_empty() {
            return;
        }
        let ui_after = ui.clone();
        super::shared_link_dialog::present(
            ui.clone(),
            super::shared_link_dialog::LinkDialogMode::Create(SharedLinkTarget::Assets(ids)),
            Rc::new(move || {
                ui_after.grid.selection.unselect_all();
                ui_after.select_toggle.set_active(false);
            }),
        );
    });
}

/// Selection-bar button: give every selected server asset one location.
/// Whether the current user owns `item`, as Immich only lets owners edit an asset.
/// Local files, and assets checked before the user id is known, count as yours.
pub(super) fn is_mine(ui: &LibraryWindowUi, item: &AssetObject) -> bool {
    owned_by(&item.owner_id(), ui.ctx.current_user_id.lock().as_deref())
}

fn owned_by(owner: &str, me: Option<&str>) -> bool {
    owner.is_empty() || me.is_none_or(|me| me == owner)
}

/// Dialog subtitle for a batch; photos other people own are left out of the change.
fn batch_title(owned: usize, selected: usize) -> String {
    if owned == selected {
        format!("{selected} selected")
    } else {
        format!("{owned} of {selected} selected, only yours change")
    }
}

/// The selection's server assets and, of those, the ids the user owns.
fn owned_server_selection(ui: &Rc<LibraryWindowUi>) -> (usize, Vec<String>) {
    let on_server: Vec<AssetObject> = selected_items(ui)
        .into_iter()
        .filter(|item| TrashTarget::from_item(item).is_some())
        .collect();
    let ids = on_server
        .iter()
        .filter(|item| is_mine(ui, item))
        .filter_map(TrashTarget::from_item)
        .map(|target| target.remote_id)
        .collect();
    (on_server.len(), ids)
}

fn clear_selection_after(ui: &Rc<LibraryWindowUi>) -> Rc<dyn Fn()> {
    let ui = ui.clone();
    Rc::new(move || {
        ui.grid.selection.unselect_all();
        ui.select_toggle.set_active(false);
    })
}

/// Selection-bar button: give every selected server asset the user owns one location.
pub(super) fn connect_bulk_location(ui: Rc<LibraryWindowUi>, button: &gtk::Button) {
    button.connect_clicked(move |_| {
        let (on_server, ids) = owned_server_selection(&ui);
        if on_server == 0 {
            return;
        }
        if ids.is_empty() {
            show_alert_dialog(
                &ui,
                "Not Your Photos",
                "Only the owner can change where a photo was taken, and the selected photos belong to other people.",
            );
            return;
        }
        let target = super::map::LocationTarget {
            title: batch_title(ids.len(), on_server),
            ids,
            initial: None,
        };
        super::map::set_location(ui.clone(), target, clear_selection_after(&ui));
    });
}

/// Selection-bar button: set or shift the capture date of every selected asset the user owns.
pub(super) fn connect_bulk_date(ui: Rc<LibraryWindowUi>, button: &gtk::Button) {
    button.connect_clicked(move |_| {
        let (on_server, ids) = owned_server_selection(&ui);
        if on_server == 0 {
            return;
        }
        if ids.is_empty() {
            show_alert_dialog(
                &ui,
                "Not Your Photos",
                "Only the owner can change when a photo was taken, and the selected photos belong to other people.",
            );
            return;
        }
        let title = batch_title(ids.len(), on_server);
        super::bulk_date::present(&ui, ids, title, clear_selection_after(&ui));
    });
}

/// Selection-bar button: add every selected server asset to an album.
pub(super) fn connect_bulk_add_to_album(ui: Rc<LibraryWindowUi>) {
    ui.bulk.add_to_album.clone().connect_clicked(move |_| {
        let ids = selected_server_ids(&ui);
        super::album_picker::present(&ui, ids, clear_selection_after(&ui));
    });
}

/// Selection-bar button: take every selected server asset out of the open album.
pub(super) fn connect_bulk_remove_from_album(ui: Rc<LibraryWindowUi>) {
    ui.bulk.remove_from_album.clone().connect_clicked(move |_| {
        let ids = selected_server_ids(&ui);
        super::album_assets::remove(&ui, ids, clear_selection_after(&ui));
    });
}

fn selected_server_ids(ui: &Rc<LibraryWindowUi>) -> Vec<String> {
    selected_items(ui)
        .iter()
        .filter_map(TrashTarget::from_item)
        .map(|target| target.remote_id)
        .collect()
}

pub(super) fn connect_bulk_archive(ui: Rc<LibraryWindowUi>) {
    ui.bulk.archive.clone().connect_clicked(move |_| {
        let archived = !matches!(
            ui.ctx.library_state.lock().source,
            super::state::LibrarySource::Archive
        );
        let ui_done = ui.clone();
        super::archive::set_archived(&ui, selected_items(&ui), archived, move |_| {
            ui_done.grid.selection.unselect_all();
            ui_done.select_toggle.set_active(false);
        });
    });
}

pub(super) fn selection_count(selection: &gtk::MultiSelection) -> u32 {
    let bitset = selection.selection();
    bitset.size() as u32
}

pub(super) fn collect_selected_assets(ui: &Rc<LibraryWindowUi>) -> Vec<(String, String)> {
    selected_items(ui)
        .iter()
        .map(|item| {
            (
                item.property::<String>("id"),
                item.property::<String>("filename"),
            )
        })
        .collect()
}

pub(super) fn selected_items(ui: &Rc<LibraryWindowUi>) -> Vec<AssetObject> {
    let bitset = ui.grid.selection.selection();
    let mut out = Vec::new();
    let Some((mut iter, first)) = gtk::BitsetIter::init_first(&bitset) else {
        return out;
    };
    let mut pos = Some(first);
    while let Some(p) = pos {
        if let Some(item) = ui.grid.model.item(p).and_downcast::<AssetObject>() {
            out.push(item);
        }
        pos = iter.next();
    }
    out
}

/// A server asset the user asked to move to the Immich trash.
pub(super) struct TrashTarget {
    remote_id: String,
    filename: String,
    has_local_copy: bool,
}

impl TrashTarget {
    pub(super) fn remote_id(&self) -> &str {
        &self.remote_id
    }

    /// `None` for local-only items, which have nothing on the server to trash.
    pub(super) fn from_item(item: &AssetObject) -> Option<Self> {
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
    let heading = match (targets, trash) {
        ([single], true) => format!("Move “{}” to trash?", single.filename),
        (_, true) => format!("Move {} items to trash?", targets.len()),
        ([single], false) => format!("Delete “{}” permanently?", single.filename),
        (_, false) => format!("Delete {} items permanently?", targets.len()),
    };
    let mut body = if trash {
        "Items can be restored from the Immich trash.".to_string()
    } else {
        "The Immich trash is turned off, so this can't be undone.".to_string()
    };
    if targets.iter().any(|t| t.has_local_copy) {
        body.push_str(" Local copies in your watch folders are kept.");
    }
    if skipped_local > 0 {
        body.push_str(&format!(
            "\n\n{} local-only item(s) are not on the server and will be skipped.",
            skipped_local
        ));
    }
    (heading, body)
}

/// Ask before moving `targets` to the Immich trash; `on_trashed` runs after the server confirms.
pub(super) fn confirm_trash(
    ui: Rc<LibraryWindowUi>,
    targets: Vec<TrashTarget>,
    skipped_local: usize,
    on_trashed: impl Fn() + 'static,
) {
    if targets.is_empty() {
        return;
    }
    let trash = ui.features.get().trash;
    let (heading, body) = trash_dialog_text(&targets, skipped_local, trash);
    let dialog = libadwaita::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", if trash { "Move to trash" } else { "Delete" });
    dialog.set_response_appearance("delete", libadwaita::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

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
                    super::context_menu::show_alert_dialog(
                        &ui,
                        "Could Not Move to Trash",
                        &format!("Immich rejected the request: {err}"),
                    );
                }
            }
        });
    });
    dialog.present(Some(&ui.window));
}

/// Trash on the server, then drop the items from the grid.
async fn trash_assets(ui: &Rc<LibraryWindowUi>, ids: &[String]) -> Result<(), String> {
    ui.ctx.api_client.delete_assets(ids).await?;
    remove_assets_from_grid(ui, ids);
    Ok(())
}

/// Drop assets from the grid in place, so open views (like the lightbox) keep their
/// position instead of reloading from the first page; then refresh album/stat counts.
pub(super) fn remove_assets_from_grid(ui: &Rc<LibraryWindowUi>, ids: &[String]) {
    if !super::timeline::remove(ui, ids) {
        ui.ctx
            .library_state
            .lock()
            .assets
            .retain(|asset| !ids.contains(&asset.id));
        // Outside the lock: items_changed handlers may read the library state.
        ui.grid.model.remove_ids(ids);
    }
    super::refresh_library_after_mutation(ui.clone(), false);
}

fn confirm_and_bulk_delete(ui: Rc<LibraryWindowUi>) {
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
    use super::{TrashTarget, batch_title, owned_by, trash_dialog_text};
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
        assert!(body.contains("2 local-only item(s)"));
    }

    #[test]
    fn without_trash_the_dialog_warns_deletion_is_permanent() {
        let (heading, body) = trash_dialog_text(&[target("IMG_1.jpg", false)], 0, false);
        assert_eq!(heading, "Delete “IMG_1.jpg” permanently?");
        assert!(body.contains("can't be undone"));
    }

    #[test]
    fn only_the_owner_counts_as_mine() {
        assert!(owned_by("me", Some("me")));
        assert!(!owned_by("partner", Some("me")));
        assert!(owned_by("", Some("me")));
        assert!(owned_by("partner", None));
    }

    #[test]
    fn batch_titles_say_when_others_photos_are_left_out() {
        assert_eq!(batch_title(3, 3), "3 selected");
        assert_eq!(batch_title(2, 5), "2 of 5 selected, only yours change");
    }
}
