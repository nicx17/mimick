//! Favorite toggling shared by the lightbox, the context menu, and Edit Info.
//!
//! The heart flips immediately (item, library state, grid badge) and Immich is
//! updated in the background; if Immich refuses, the change is rolled back.

use std::rc::Rc;

use gtk::prelude::*;

use crate::api_client::{AssetUpdate, LibraryAsset};
use crate::library::asset_object::AssetObject;

use super::LibraryWindowUi;
use super::actions::{TrashTarget, remove_assets_from_grid};
use super::context_menu::show_error_toast;
use super::state::LibrarySource;

/// Icon for a heart button showing `favorite`.
pub(super) fn heart_icon(favorite: bool) -> &'static str {
    if favorite {
        "mimick-heart-filled-symbolic"
    } else {
        "mimick-heart-outline-symbolic"
    }
}

pub(super) fn heart_tooltip(favorite: bool) -> &'static str {
    if favorite {
        "Remove from favorites (F)"
    } else {
        "Add to favorites (F)"
    }
}

/// Callbacks for the view that started the toggle.
pub(super) struct FavoriteHooks {
    /// Runs with the new value right away, and again with the old value on rollback.
    pub on_change: Rc<dyn Fn(bool)>,
    /// Runs after an un-favorited asset is dropped from the Favorites view.
    pub on_removed: Rc<dyn Fn()>,
}

/// Flip `item`'s favorite flag. Local-only assets have nothing on the server and are ignored.
pub(super) fn toggle_favorite(ui: &Rc<LibraryWindowUi>, item: &AssetObject, hooks: FavoriteHooks) {
    let Some(target) = TrashTarget::from_item(item) else {
        return;
    };
    let asset_id = target.remote_id().to_string();
    let favorite = !item.is_favorite();
    show_favorite(ui, item, &asset_id, favorite);
    (hooks.on_change)(favorite);

    let (ui, item) = (ui.clone(), item.clone());
    glib::MainContext::default().spawn_local(async move {
        let update = AssetUpdate {
            is_favorite: Some(favorite),
            ..AssetUpdate::default()
        };
        if let Err(err) = ui.ctx.api_client.update_asset(&asset_id, &update).await {
            show_favorite(&ui, &item, &asset_id, !favorite);
            (hooks.on_change)(!favorite);
            show_error_toast(&ui, "Couldn't update the favorite", &err);
            return;
        }
        if !favorite && drop_from_favorites_view(&ui, &asset_id) {
            (hooks.on_removed)();
        }
    });
}

/// Reflect a favorite change saved elsewhere (Edit Info) on the asset's grid item, if shown.
pub(super) fn apply_saved_favorite(ui: &LibraryWindowUi, asset_id: &str, favorite: bool) {
    let model = &ui.grid.model;
    let item = (0..model.n_items())
        .filter_map(|i| model.item(i).and_downcast::<AssetObject>())
        .find(|item| {
            item.property::<String>("id") == asset_id
                || item.property::<String>("remote-id") == asset_id
        });
    match item {
        Some(item) => show_favorite(ui, &item, asset_id, favorite),
        None => {
            set_favorite_in(&mut ui.ctx.library_state.lock().assets, asset_id, favorite);
        }
    }
}

/// Drop an un-favorited asset from the Favorites view; returns whether it was dropped.
pub(super) fn drop_from_favorites_view(ui: &Rc<LibraryWindowUi>, asset_id: &str) -> bool {
    let in_favorites = matches!(ui.ctx.library_state.lock().source, LibrarySource::Favorites);
    if in_favorites {
        remove_assets_from_grid(ui, &[asset_id.to_string()]);
    }
    in_favorites
}

/// Show `favorite` everywhere the asset appears: its grid item, the cached page, and the badge.
fn show_favorite(ui: &LibraryWindowUi, item: &AssetObject, asset_id: &str, favorite: bool) {
    item.set_is_favorite(favorite);
    set_favorite_in(&mut ui.ctx.library_state.lock().assets, asset_id, favorite);
    ui.grid.canvas.queue_draw();
}

/// Update the cached asset so a re-sort or re-render keeps the new value.
fn set_favorite_in(assets: &mut [LibraryAsset], asset_id: &str, favorite: bool) -> bool {
    match assets.iter_mut().find(|asset| asset.id == asset_id) {
        Some(asset) => {
            asset.is_favorite = favorite;
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: &str) -> LibraryAsset {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "originalFileName": "a.jpg",
            "originalMimeType": "image/jpeg",
            "fileCreatedAt": "2024-01-15T14:25:15.000Z",
            "type": "IMAGE",
            "isFavorite": true
        }))
        .unwrap()
    }

    #[test]
    fn asset_json_reads_is_favorite() {
        assert!(asset("a").is_favorite);
    }

    #[test]
    fn set_favorite_in_updates_only_the_matching_asset() {
        let mut assets = vec![asset("a"), asset("b")];
        assert!(set_favorite_in(&mut assets, "b", false));
        assert!(assets[0].is_favorite);
        assert!(!assets[1].is_favorite);
        assert!(!set_favorite_in(&mut assets, "missing", false));
    }

    #[test]
    fn heart_icon_and_tooltip_follow_the_state() {
        assert_eq!(heart_icon(true), "mimick-heart-filled-symbolic");
        assert_eq!(heart_icon(false), "mimick-heart-outline-symbolic");
        assert!(heart_tooltip(false).starts_with("Add"));
    }
}
