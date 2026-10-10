//! Trash view controls: restore or permanently delete trashed assets.
//!
//! The Trash sidebar destination lists trashed assets in the normal grid. This
//! module adds the "Restore All" / "Empty Trash" bar above the grid and swaps the
//! selection bar's actions for "Restore" / "Delete Permanently" while it's active.

use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use super::LibraryWindowUi;
use super::actions::{TrashTarget, remove_assets_from_grid, selected_items};
use super::context_menu::show_alert_dialog;
use super::state::LibrarySource;

/// Trash-only widgets, plus the selection-bar buttons they temporarily replace.
pub(super) struct TrashControls {
    /// Bar above the grid; revealed only while the Trash source is active.
    pub bar: gtk::Revealer,
    restore_all: gtk::Button,
    empty: gtk::Button,
    /// Selection-bar actions shown in the trash.
    pub restore_selected: gtk::Button,
    pub delete_selected: gtk::Button,
    /// Selection-bar actions hidden in the trash (download, move to trash).
    live_only: Vec<gtk::Button>,
}

pub(super) fn build_trash_controls(live_only: Vec<gtk::Button>) -> TrashControls {
    let note = gtk::Label::builder()
        .label("Immich permanently deletes trashed items after its retention period (30 days by default).")
        .xalign(0.0)
        .hexpand(true)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();
    let restore_all = gtk::Button::builder().label("Restore All").build();
    let empty = gtk::Button::builder()
        .label("Empty Trash")
        .css_classes(["destructive-action"])
        .build();
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .build();
    inner.append(&note);
    inner.append(&restore_all);
    inner.append(&empty);
    let bar = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .reveal_child(false)
        .child(&inner)
        .build();
    let restore_selected = gtk::Button::builder()
        .icon_name("edit-undo-symbolic")
        .tooltip_text("Restore selected")
        .visible(false)
        .build();
    let delete_selected = gtk::Button::builder()
        .icon_name("edit-delete-symbolic")
        .tooltip_text("Delete selected permanently")
        .css_classes(["destructive-action"])
        .visible(false)
        .build();
    TrashControls {
        bar,
        restore_all,
        empty,
        restore_selected,
        delete_selected,
        live_only,
    }
}

pub(super) fn is_trash_active(ui: &LibraryWindowUi) -> bool {
    matches!(ui.ctx.library_state.lock().source, LibrarySource::Trash)
}

/// Show the trash bar and trash selection actions only while the Trash source is active.
pub(super) fn set_trash_mode(ui: &LibraryWindowUi, active: bool) {
    let trash = &ui.trash;
    trash.bar.set_reveal_child(active);
    trash.restore_selected.set_visible(active);
    trash.delete_selected.set_visible(active);
    for button in &trash.live_only {
        button.set_visible(!active);
    }
}

pub(super) fn connect_trash_controls(ui: Rc<LibraryWindowUi>) {
    let ui_restore_all = ui.clone();
    ui.trash
        .restore_all
        .connect_clicked(move |_| restore_all(ui_restore_all.clone()));
    let ui_empty = ui.clone();
    ui.trash
        .empty
        .connect_clicked(move |_| confirm_empty_trash(ui_empty.clone()));
    let ui_restore = ui.clone();
    ui.trash
        .restore_selected
        .connect_clicked(move |_| restore_selected(ui_restore.clone()));
    let ui_delete = ui.clone();
    ui.trash
        .delete_selected
        .connect_clicked(move |_| confirm_delete_selected(ui_delete.clone()));
}

fn selected_ids(ui: &Rc<LibraryWindowUi>) -> Vec<String> {
    selected_items(ui)
        .iter()
        .filter_map(TrashTarget::from_item)
        .map(|target| target.remote_id().to_string())
        .collect()
}

fn clear_selection(ui: &LibraryWindowUi) {
    ui.grid.selection.unselect_all();
    ui.select_toggle.set_active(false);
}

fn restore_selected(ui: Rc<LibraryWindowUi>) {
    let ids = selected_ids(&ui);
    let ui_done = ui.clone();
    restore(ui, ids, move || clear_selection(&ui_done));
}

fn confirm_delete_selected(ui: Rc<LibraryWindowUi>) {
    let ids = selected_ids(&ui);
    let ui_done = ui.clone();
    confirm_delete_permanently(ui, ids, move || clear_selection(&ui_done));
}

/// Restore assets from the trash and drop them from the trash grid; `on_done` runs after.
pub(super) fn restore(ui: Rc<LibraryWindowUi>, ids: Vec<String>, on_done: impl Fn() + 'static) {
    if ids.is_empty() {
        return;
    }
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.restore_assets(&ids).await {
            Ok(_) => {
                remove_assets_from_grid(&ui, &ids);
                on_done();
            }
            Err(err) => show_alert_dialog(&ui, "Could Not Restore", &err),
        }
    });
}

/// Heading and body for the permanent-delete confirmation.
fn permanent_delete_text(count: usize) -> (String, String) {
    let heading = if count == 1 {
        "Delete this item permanently?".to_string()
    } else {
        format!("Delete {count} items permanently?")
    };
    (
        heading,
        "This can't be undone. Local copies in your watch folders are kept.".to_string(),
    )
}

/// Ask, then permanently delete `ids` and drop them from the grid; `on_done` runs after.
pub(super) fn confirm_delete_permanently(
    ui: Rc<LibraryWindowUi>,
    ids: Vec<String>,
    on_done: impl Fn() + 'static,
) {
    if ids.is_empty() {
        return;
    }
    let (heading, body) = permanent_delete_text(ids.len());
    let on_done = Rc::new(on_done);
    confirm_destructive(&ui, &heading, &body, "Delete Permanently", move |ui| {
        let ids = ids.clone();
        let on_done = on_done.clone();
        glib::MainContext::default().spawn_local(async move {
            match ui.ctx.api_client.delete_assets_permanently(&ids).await {
                Ok(()) => {
                    remove_assets_from_grid(&ui, &ids);
                    on_done();
                }
                Err(err) => show_alert_dialog(&ui, "Could Not Delete", &err),
            }
        });
    });
}

fn restore_all(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.restore_all_trash().await {
            Ok(_) => super::refresh_library_after_mutation(ui.clone(), true),
            Err(err) => show_alert_dialog(&ui, "Could Not Restore", &err),
        }
    });
}

fn confirm_empty_trash(ui: Rc<LibraryWindowUi>) {
    confirm_destructive(
        &ui,
        "Empty the trash?",
        "Everything in the Immich trash is deleted permanently. This can't be undone.",
        "Empty Trash",
        |ui| {
            glib::MainContext::default().spawn_local(async move {
                match ui.ctx.api_client.empty_trash().await {
                    Ok(_) => super::refresh_library_after_mutation(ui.clone(), true),
                    Err(err) => show_alert_dialog(&ui, "Could Not Empty Trash", &err),
                }
            });
        },
    );
}

/// Cancel-by-default confirmation whose destructive button runs `on_confirm`.
fn confirm_destructive(
    ui: &Rc<LibraryWindowUi>,
    heading: &str,
    body: &str,
    action_label: &str,
    on_confirm: impl Fn(Rc<LibraryWindowUi>) + 'static,
) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("confirm", action_label);
    dialog.set_response_appearance("confirm", libadwaita::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let ui_for_choice = ui.clone();
    dialog.connect_response(None, move |_, response| {
        if response == "confirm" {
            on_confirm(ui_for_choice.clone());
        }
    });
    dialog.present(Some(&ui.window));
}

#[cfg(test)]
mod tests {
    use super::permanent_delete_text;

    #[test]
    fn permanent_delete_text_matches_count_and_warns() {
        let (single, body) = permanent_delete_text(1);
        assert_eq!(single, "Delete this item permanently?");
        assert!(body.contains("can't be undone"));
        assert_eq!(permanent_delete_text(3).0, "Delete 3 items permanently?");
    }
}
