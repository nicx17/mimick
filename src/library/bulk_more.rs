//! Selection-bar actions beyond the basics: Favorite, Edit Description, and Immich's refresh
//! jobs. They act on the selected photos this user owns; Immich refuses anyone else's.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::rc::Rc;

use gtk::prelude::*;

use super::LibraryWindowUi;
use super::actions::{is_mine, owned_selection_ids, selected_items};
use super::asset_jobs;
use super::asset_object::AssetObject;
use super::context_menu::show_error_toast;
use super::dialogs::{TextPrompt, prompt_text};
use super::favorites;

pub(super) fn connect(ui: &Rc<LibraryWindowUi>) {
    connect_favorite(ui);
    let ui_description = ui.clone();
    ui.bulk
        .description
        .connect_clicked(move |_| edit_description(&ui_description));
    for (job, button) in &ui.bulk.jobs {
        let (ui, job) = (ui.clone(), *job);
        button.connect_clicked(move |_| {
            if let Some(ids) = owned_ids_or_explain(&ui) {
                asset_jobs::run_bulk(&ui, job, ids);
            }
        });
    }
}

/// The selected photos' server ids, or `None` after a toast when none of them are yours.
fn owned_ids_or_explain(ui: &Rc<LibraryWindowUi>) -> Option<Vec<String>> {
    let ids = owned_selection_ids(ui);
    if ids.is_empty() {
        ui.toasts.add_toast(libadwaita::Toast::new(&gettext(
            "Only the owner can change these photos",
        )));
        return None;
    }
    Some(ids)
}

/// Whether every selected photo you own is already a favorite, so the button unfavorites.
fn all_favorite(ui: &Rc<LibraryWindowUi>) -> bool {
    let owned: Vec<_> = selected_items(ui)
        .into_iter()
        .filter(|item| is_mine(ui, item))
        .collect();
    !owned.is_empty() && owned.iter().all(AssetObject::is_favorite)
}

fn connect_favorite(ui: &Rc<LibraryWindowUi>) {
    let ui_sync = ui.clone();
    ui.grid
        .selection
        .connect_selection_changed(move |_, _, _| sync_favorite(&ui_sync));
    let ui_click = ui.clone();
    ui.bulk
        .favorite
        .connect_clicked(move |_| toggle_favorite(&ui_click));
}

/// A filled heart when the selection is all favorites already, an outline otherwise.
fn sync_favorite(ui: &Rc<LibraryWindowUi>) {
    let (icon, tooltip) = if all_favorite(ui) {
        (
            "mimick-heart-filled-symbolic",
            gettext("Unfavorite selected"),
        )
    } else {
        (
            "mimick-heart-outline-symbolic",
            gettext("Favorite selected"),
        )
    };
    ui.bulk.favorite.set_icon_name(icon);
    ui.bulk.favorite.set_tooltip_text(Some(&tooltip));
}

fn toggle_favorite(ui: &Rc<LibraryWindowUi>) {
    let Some(ids) = owned_ids_or_explain(ui) else {
        return;
    };
    let favorite = !all_favorite(ui);
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        if let Err(err) = ui.ctx.api_client.set_assets_favorite(&ids, favorite).await {
            show_error_toast(&ui, &gettext("Couldn't update favorites"), &err);
            return;
        }
        for id in &ids {
            favorites::apply_saved_favorite(&ui, id, favorite);
            if !favorite {
                favorites::drop_from_favorites_view(&ui, id);
            }
        }
        sync_favorite(&ui);
        let message = if favorite {
            ngettext(
                "Added {n} photo to favorites",
                "Added {n} photos to favorites",
                plural_n(ids.len()),
            )
        } else {
            ngettext(
                "Removed {n} photo from favorites",
                "Removed {n} photos from favorites",
                plural_n(ids.len()),
            )
        };
        let message = fill(message, &[("n", &ids.len().to_string())]);
        ui.toasts.add_toast(libadwaita::Toast::new(&message));
    });
}

fn edit_description(ui: &Rc<LibraryWindowUi>) {
    let Some(ids) = owned_ids_or_explain(ui) else {
        return;
    };
    let heading = fill(
        ngettext(
            "Describe {n} Photo",
            "Describe {n} Photos",
            plural_n(ids.len()),
        ),
        &[("n", &ids.len().to_string())],
    );
    let ui_save = ui.clone();
    prompt_text(
        ui,
        TextPrompt {
            heading: &heading,
            body: &gettext("Each photo gets this description in place of its own."),
            initial: "",
            placeholder: &gettext("Description"),
            accept: &gettext("Save"),
        },
        move |text| save_description(&ui_save, ids.clone(), text),
    );
}

fn save_description(ui: &Rc<LibraryWindowUi>, ids: Vec<String>, text: String) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.set_assets_description(&ids, &text).await {
            Ok(()) => ui
                .toasts
                .add_toast(libadwaita::Toast::new(&gettext("Description saved"))),
            Err(err) => show_error_toast(&ui, &gettext("Couldn't save the description"), &err),
        }
    });
}
