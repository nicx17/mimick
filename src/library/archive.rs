use std::rc::Rc;

use gtk::prelude::*;

use crate::api_client::LibraryAsset;
use crate::library::asset_object::AssetObject;

use super::LibraryWindowUi;
use super::actions::{TrashTarget, remove_assets_from_grid};
use super::context_menu::show_alert_dialog;
use super::state::LibrarySource;

pub(super) fn archive_icon(archived: bool) -> &'static str {
    if archived {
        "mimick-unarchive-symbolic"
    } else {
        "mimick-archive-symbolic"
    }
}

pub(super) fn archive_label(archived: bool) -> &'static str {
    if archived { "Unarchive" } else { "Archive" }
}

pub(super) fn archive_tooltip(archived: bool) -> &'static str {
    if archived {
        "Unarchive (A)"
    } else {
        "Archive (A)"
    }
}

pub(super) fn set_archived(
    ui: &Rc<LibraryWindowUi>,
    items: Vec<AssetObject>,
    archived: bool,
    on_done: impl Fn(bool) + 'static,
) {
    let ids: Vec<String> = items
        .iter()
        .filter_map(TrashTarget::from_item)
        .map(|target| target.remote_id().to_string())
        .collect();
    if ids.is_empty() {
        return;
    }
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let visibility = if archived { "archive" } else { "timeline" };
        if let Err(err) = ui
            .ctx
            .api_client
            .set_assets_visibility(&ids, visibility)
            .await
        {
            let heading = if archived {
                "Could Not Archive"
            } else {
                "Could Not Unarchive"
            };
            show_alert_dialog(&ui, heading, &err);
            return;
        }
        for item in &items {
            item.set_is_archived(archived);
        }
        let removed = {
            let mut state = ui.ctx.library_state.lock();
            set_visibility_in(&mut state.assets, &ids, visibility);
            leaves_view(&state.source, archived)
        };
        if removed {
            remove_assets_from_grid(&ui, &ids);
        }
        on_done(removed);
    });
}

pub(super) fn sync_bulk_button(button: &gtk::Button, source: &LibrarySource) {
    let in_archive = matches!(source, LibrarySource::Archive);
    button.set_icon_name(archive_icon(in_archive));
    button.set_tooltip_text(Some(if in_archive {
        "Unarchive selected"
    } else {
        "Archive selected"
    }));
}

fn leaves_view(source: &LibrarySource, archived: bool) -> bool {
    match source {
        LibrarySource::AllAssets | LibrarySource::Unified => archived,
        LibrarySource::Archive => !archived,
        _ => false,
    }
}

fn set_visibility_in(assets: &mut [LibraryAsset], ids: &[String], visibility: &str) {
    for asset in assets.iter_mut().filter(|asset| ids.contains(&asset.id)) {
        asset.visibility = visibility.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: &str, visibility: &str) -> LibraryAsset {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "originalFileName": "a.jpg",
            "originalMimeType": "image/jpeg",
            "fileCreatedAt": "2024-01-15T14:25:15.000Z",
            "type": "IMAGE",
            "visibility": visibility
        }))
        .unwrap()
    }

    #[test]
    fn asset_json_reads_visibility() {
        assert!(asset("a", "archive").is_archived());
        assert!(!asset("a", "timeline").is_archived());
    }

    #[test]
    fn set_visibility_in_updates_only_listed_assets() {
        let mut assets = vec![asset("a", "timeline"), asset("b", "timeline")];
        set_visibility_in(&mut assets, &["b".to_string()], "archive");
        assert!(!assets[0].is_archived());
        assert!(assets[1].is_archived());
    }

    #[test]
    fn archiving_leaves_photos_and_unarchiving_leaves_archive() {
        assert!(leaves_view(&LibrarySource::AllAssets, true));
        assert!(!leaves_view(&LibrarySource::AllAssets, false));
        assert!(leaves_view(&LibrarySource::Archive, false));
        assert!(!leaves_view(&LibrarySource::Archive, true));
        assert!(!leaves_view(&LibrarySource::Favorites, true));
    }

    #[test]
    fn labels_follow_the_state() {
        assert_eq!(archive_label(false), "Archive");
        assert_eq!(archive_icon(true), "mimick-unarchive-symbolic");
        assert!(archive_tooltip(true).starts_with("Unarchive"));
    }
}
