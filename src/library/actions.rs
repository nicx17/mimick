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

use crate::library::asset_object::AssetObject;

use super::LOCAL_ID_PREFIX;
use super::LibraryWindowUi;
use super::download::start_download_group;

pub(super) fn connect_select_mode(ui: Rc<LibraryWindowUi>, select_toggle: gtk::ToggleButton) {
    let selection = ui.grid.selection.clone();
    let bulk_bar = ui.bulk_bar.clone();
    let count_label = ui.bulk_count_label.clone();

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
                ui.trash.delete_selected.emit_clicked();
            } else {
                confirm_and_bulk_delete(ui.clone());
            }
            glib::Propagation::Stop
        }
    ));
    ui.grid.scrolled.add_controller(delete_key);
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
fn trash_dialog_text(targets: &[TrashTarget], skipped_local: usize) -> (String, String) {
    let heading = match targets {
        [single] => format!("Move “{}” to trash?", single.filename),
        _ => format!("Move {} items to trash?", targets.len()),
    };
    let mut body = "Items can be restored from the Immich trash.".to_string();
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
    let (heading, body) = trash_dialog_text(&targets, skipped_local);
    let dialog = libadwaita::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Move to trash");
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
    let (assets, sort_mode) = {
        let mut state = ui.ctx.library_state.lock();
        state.assets.retain(|asset| !ids.contains(&asset.id));
        (state.assets.clone(), state.sort_mode.clone())
    };
    // Reset outside the lock: items_changed handlers may read the library state.
    ui.grid.model.reset(&ui.ctx, &assets, &sort_mode);
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
        let (heading, body) = trash_dialog_text(&[target("IMG_1.jpg", false)], 0);
        assert_eq!(heading, "Move “IMG_1.jpg” to trash?");
        assert_eq!(body, "Items can be restored from the Immich trash.");
    }

    #[test]
    fn body_mentions_kept_local_copies_and_skipped_items() {
        let targets = [target("a.jpg", true), target("b.jpg", false)];
        let (heading, body) = trash_dialog_text(&targets, 2);
        assert_eq!(heading, "Move 2 items to trash?");
        assert!(body.contains("Local copies in your watch folders are kept."));
        assert!(body.contains("2 local-only item(s)"));
    }
}
