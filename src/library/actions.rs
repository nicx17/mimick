//! Selection-mode wiring and bulk batch operations.
//!
//! Manages multi-select mode toggling, selection-count tracking, and
//! batch actions such as bulk download, bulk delete-to-trash, and
//! bulk add-to-album for the library grid.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::Cell;
use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;

use crate::api_client::SharedLinkTarget;
use crate::library::asset_object::AssetObject;

use super::LOCAL_ID_PREFIX;
use super::LibraryWindowUi;
use super::context_menu::show_alert_dialog;
use super::download::start_download_group;

mod trash;
use trash::confirm_and_bulk_delete;
pub(super) use trash::{
    TrashTarget, confirm_trash, confirm_trash_leaving_grid, remove_assets_from_grid,
};

pub(super) fn connect_select_mode(ui: Rc<LibraryWindowUi>, select_toggle: gtk::ToggleButton) {
    let selection = ui.grid.selection.clone();
    let bulk_bar = ui.bulk.bar.clone();
    let count_label = ui.bulk.count_label.clone();

    let refresh = {
        let selection = selection.clone();
        let bulk_bar = bulk_bar.clone();
        let count_label = count_label.clone();
        let select_toggle = select_toggle.clone();
        let canvas = ui.grid.canvas.clone();
        Rc::new(move || {
            let n = selection_count(&selection);
            bulk_bar.set_reveal_child(select_toggle.is_active() && n > 0);
            let sizes = canvas.shows_file_sizes();
            count_label.set_label(&super::utilities::selection_text(&selection, n, sizes));
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

/// Escape bubbles, so an open dialog, menu, or path field it's meant for gets it first.
/// On the library page too, because the navigation view around it takes Escape for itself.
fn connect_escape(
    ui: &LibraryWindowUi,
    select_toggle: &gtk::ToggleButton,
    transient: &Rc<Cell<bool>>,
) {
    if let Some(page) = ui
        .nav
        .navigation_stack()
        .item(0)
        .and_downcast::<gtk::Widget>()
    {
        page.add_controller(escape_leaves_selection(select_toggle, transient));
    }
    ui.window
        .add_controller(escape_leaves_selection(select_toggle, transient));
}

fn escape_leaves_selection(
    select_toggle: &gtk::ToggleButton,
    transient: &Rc<Cell<bool>>,
) -> gtk::EventControllerKey {
    let escape = gtk::EventControllerKey::new();
    let (select_toggle, transient) = (select_toggle.clone(), transient.clone());
    escape.connect_key_pressed(move |_, keyval, _, _| {
        if keyval != gtk::gdk::Key::Escape || !select_toggle.is_active() {
            return glib::Propagation::Proceed;
        }
        select_toggle.set_active(false);
        transient.set(false);
        glib::Propagation::Stop
    });
    escape
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
    connect_escape(&ui, &select_toggle, &transient);
    let key_controller = gtk::EventControllerKey::new();
    key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    key_controller.connect_key_pressed({
        let select_toggle = select_toggle.clone();
        let transient = transient.clone();
        move |_, keyval, _, _| {
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

/// Selection-bar button: a menu to make a new link of the selection, or add it to a link.
pub(super) fn connect_bulk_share_link(ui: Rc<LibraryWindowUi>, button: &gtk::Button) {
    button.connect_clicked(move |button| {
        let ids = selected_server_ids(&ui);
        if ids.is_empty() {
            return;
        }
        let (x, y) = (f64::from(button.width()) / 2.0, 0.0);
        let menu = super::context_menu::Menu::new(button, x, y);
        let (ui_new, ids_new) = (ui.clone(), ids.clone());
        menu.add_with_icon(
            "mimick-share-symbolic",
            &gettext("New Link…"),
            false,
            move || create_link_for(&ui_new, ids_new.clone()),
        );
        let ui_add = ui.clone();
        menu.add_with_icon(
            "insert-link-symbolic",
            &gettext("Add to Existing Link…"),
            false,
            move || super::shared_link_photos::add_to_link(&ui_add, ids.clone()),
        );
        // On narrow windows the button sits in "More actions", so there's nothing to point at.
        if button.is_mapped() {
            menu.popup();
        } else {
            menu.present_sheet(&ui.window);
        }
    });
}

/// A new public link for `ids`; the selection clears once it's made.
fn create_link_for(ui: &Rc<LibraryWindowUi>, ids: Vec<String>) {
    let ui_after = ui.clone();
    super::shared_link_dialog::present(
        ui.clone(),
        super::shared_link_dialog::LinkDialogMode::Create(SharedLinkTarget::Assets(ids)),
        Rc::new(move || {
            ui_after.grid.selection.unselect_all();
            ui_after.select_toggle.set_active(false);
        }),
    );
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
        fill(
            ngettext("{n} selected", "{n} selected", plural_n(selected)),
            &[("n", &selected.to_string())],
        )
    } else {
        fill(
            ngettext(
                "{owned} of {n} selected, only yours change",
                "{owned} of {n} selected, only yours change",
                plural_n(selected),
            ),
            &[("owned", &owned.to_string()), ("n", &selected.to_string())],
        )
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
                &gettext("Not Your Photos"),
                &gettext("Only the owner can change where a photo was taken, and the selected photos belong to other people."),
            );
            return;
        }
        let on_saved = super::utilities::after_fixing(&ui, ids.clone(), clear_selection_after(&ui));
        let target = super::map::LocationTarget {
            title: batch_title(ids.len(), on_server),
            around: taken_span(&selected_items(&ui)),
            ids,
            initial: None,
        };
        super::map::set_location(ui.clone(), target, on_saved);
    });
}

/// When the earliest and latest of `items` were taken, as Immich's UTC times.
fn taken_span(items: &[AssetObject]) -> Option<(String, String)> {
    let times = items.iter().map(AssetObject::created_at);
    let earliest = times.clone().min()?;
    Some((earliest, times.max()?))
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
                &gettext("Not Your Photos"),
                &gettext("Only the owner can change when a photo was taken, and the selected photos belong to other people."),
            );
            return;
        }
        let title = batch_title(ids.len(), on_server);
        let on_done = super::utilities::after_fixing(&ui, ids.clone(), clear_selection_after(&ui));
        super::bulk_date::present(&ui, ids, title, on_done);
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

/// "Stack selected": two or more of your own photos, anywhere but the Trash.
pub(super) fn connect_bulk_stack(ui: Rc<LibraryWindowUi>) {
    let sync = {
        let ui = ui.clone();
        move || {
            let owned = owned_selection_ids(&ui).len();
            let trash = super::trash_view::is_trash_active(&ui);
            ui.bulk.stack.set_visible(owned >= 2 && !trash);
        }
    };
    let on_change = sync.clone();
    ui.grid
        .selection
        .connect_selection_changed(move |_, _, _| on_change());
    sync();
    ui.bulk.stack.clone().connect_clicked(move |_| {
        super::lightbox::stack_selected(&ui, owned_selection_ids(&ui));
    });
}

/// Selected photos you own on the server, in grid order; the first becomes a new stack's cover.
pub(super) fn owned_selection_ids(ui: &Rc<LibraryWindowUi>) -> Vec<String> {
    selected_items(ui)
        .iter()
        .filter(|item| is_mine(ui, item))
        .filter_map(|item| TrashTarget::from_item(item).map(|t| t.remote_id().to_string()))
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

#[cfg(test)]
mod tests {
    use super::{batch_title, owned_by};

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
