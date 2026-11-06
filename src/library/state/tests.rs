use super::*;

fn asset(id: &str, filename: &str) -> LibraryAsset {
    LibraryAsset {
        id: id.into(),
        filename: filename.into(),
        mime_type: "image/jpeg".into(),
        created_at: format!("2024-01-0{}T00:00:00.000Z", id),
        asset_type: "IMAGE".into(),
        thumbhash: None,
        width: Some(10),
        height: Some(10),
        checksum: None,
        exif_info: None,
        is_favorite: false,
        visibility: String::new(),
        is_edited: false,
        owner_id: String::new(),
        local_date_time: None,
        live_photo_video_id: None,
        stack: None,
    }
}

#[test]
fn test_switch_source_resets_pagination_and_assets() {
    let mut state = LibraryState::new();
    state.assets.push(asset("1", "a.jpg"));
    state.next_page = 9;
    state.has_more = false;

    let (_, source, page) = state.switch_source(LibrarySource::Album {
        id: "album-1".into(),
        name: "Trips".into(),
    });

    assert!(matches!(source, LibrarySource::Album { .. }));
    assert_eq!(page, 1);
    assert!(state.assets.is_empty());
    assert_eq!(state.next_page, 1);
    assert!(state.has_more);
    assert!(state.page_in_flight);
}

#[test]
fn test_stale_generation_results_are_ignored() {
    let mut state = LibraryState::new();
    let (generation, _, _) = state.switch_source(LibrarySource::AllAssets);
    let newer_generation = state.switch_source(LibrarySource::MetadataSearch {
        query: "cats".into(),
    });

    assert!(!state.replace_assets(generation, vec![asset("1", "a.jpg")]));
    assert!(state.replace_assets(newer_generation.0, vec![asset("2", "b.jpg")]));
    assert_eq!(state.assets.len(), 1);
    assert_eq!(state.assets[0].id, "2");
}

#[test]
fn test_duplicate_page_requests_are_suppressed() {
    let mut state = LibraryState::new();
    let (_, _, _) = state.switch_source(LibrarySource::AllAssets);
    assert!(state.load_next_page_if_needed().is_none());
    state.page_in_flight = false;
    assert!(state.load_next_page_if_needed().is_some());
    assert!(state.load_next_page_if_needed().is_none());
}

#[test]
fn test_dedup_drops_duplicates_across_appends() {
    let mut state = LibraryState::new();
    let (generation, _, _) = state.switch_source(LibrarySource::AllAssets);
    let mk_page = |start: u32| -> Vec<LibraryAsset> {
        (0..50)
            .map(|i| asset(&format!("{}", start + i), "a.jpg"))
            .collect()
    };
    state.append_assets(generation, mk_page(0));
    state.append_assets(generation, mk_page(50));
    // Re-append the second page; ids must not appear twice.
    state.append_assets(generation, mk_page(50));
    let unique: std::collections::HashSet<_> = state.assets.iter().map(|a| a.id.clone()).collect();
    assert_eq!(unique.len(), state.assets.len());
    assert_eq!(state.assets.len(), 100);
}

#[test]
fn set_album_order_updates_the_cached_album() {
    let mut state = LibraryState::default();
    state.load_albums(vec![
        serde_json::from_value(serde_json::json!({
            "id": "a1",
            "albumName": "Trip",
            "assetCount": 1,
            "albumThumbnailAssetId": null,
            "createdAt": "2024-01-01T00:00:00.000Z",
            "updatedAt": "2024-01-01T00:00:00.000Z"
        }))
        .unwrap(),
    ]);
    state.set_album_order("a1", crate::api_client::SortOrder::Asc);
    assert_eq!(
        state.albums[0].order,
        Some(crate::api_client::SortOrder::Asc)
    );
}

#[test]
fn test_sort_mode_server_order_mapping() {
    assert!(matches!(
        LibrarySortMode::NewestFirst.server_order(),
        Some(crate::api_client::SortOrder::Desc)
    ));
    assert!(matches!(
        LibrarySortMode::OldestFirst.server_order(),
        Some(crate::api_client::SortOrder::Asc)
    ));
    assert!(LibrarySortMode::Filename.server_order().is_none());
    assert!(LibrarySortMode::FileType.server_order().is_none());
}

#[test]
fn test_clear_search_restores_previous_non_search_source() {
    let mut state = LibraryState::new();
    state.switch_source(LibrarySource::Album {
        id: "album-1".into(),
        name: "Trips".into(),
    });
    state.switch_source(LibrarySource::SmartSearch {
        query: "sunset".into(),
    });

    let (_, source, page) = state.clear_search_restore_previous_source().unwrap();
    assert!(matches!(source, LibrarySource::Album { .. }));
    assert_eq!(page, 1);
}

#[test]
fn asset_ids_page_through_in_order() {
    let ids = AssetIds((0..5).map(|i| i.to_string()).collect());
    assert_eq!(ids.page(1, 2), (&ids.0[0..2], true));
    assert_eq!(ids.page(3, 2), (&ids.0[4..5], false));
    assert_eq!(ids.page(4, 2).0, &[] as &[String]);
    assert_eq!(format!("{ids:?}"), "5 assets");
}

#[test]
fn timeline_months_insert_in_place_and_stop_paging() {
    let mut state = LibraryState::new();
    let (generation, _, _) = state.switch_source(LibrarySource::AllAssets);
    assert!(state.enter_timeline(generation, false));
    assert!(state.load_next_page_if_needed().is_none());
    assert!(state.insert_assets_at(
        generation,
        0,
        vec![asset("1", "a.jpg"), asset("3", "c.jpg")]
    ));
    assert!(state.insert_assets_at(generation, 1, vec![asset("2", "b.jpg")]));
    let ids: Vec<&str> = state.assets.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["1", "2", "3"]);
    assert!(!state.insert_assets_at(generation + 1, 0, vec![asset("4", "d.jpg")]));
}
