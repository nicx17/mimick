use super::*;

use crate::api_client::{AlbumUser, AlbumUserInfo};

fn album(id: &str, name: &str, owner: &str, count: u32, created: &str) -> LibraryAlbum {
    LibraryAlbum {
        id: id.into(),
        album_name: name.into(),
        asset_count: count,
        thumbnail_asset_id: None,
        created_at: created.into(),
        updated_at: created.into(),
        start_date: None,
        end_date: None,
        description: String::new(),
        album_users: vec![AlbumUser {
            user: AlbumUserInfo {
                id: owner.into(),
                name: String::new(),
                email: String::new(),
            },
            role: "owner".into(),
        }],
        shared: false,
        order: None,
        is_activity_enabled: true,
        has_shared_link: false,
    }
}

fn fixture() -> Vec<LibraryAlbum> {
    vec![
        album("a1", "Beach Trip", "me", 200, "2024-06-01T00:00:00Z"),
        album("a2", "Family", "friend", 50, "2025-01-10T00:00:00Z"),
        album("a3", "Archive 2020", "me", 1000, "2020-12-31T00:00:00Z"),
        album("a4", "beach-volleyball", "me", 12, "2023-09-15T00:00:00Z"),
    ]
}

#[test]
fn project_filters_by_case_insensitive_substring() {
    let items = fixture();
    let (_, owned, _) = project_albums(&items, "me", "beach", AlbumsSort::Name);
    let names: Vec<&str> = owned.iter().map(|a| a.album_name.as_str()).collect();
    assert_eq!(names, vec!["Beach Trip", "beach-volleyball"]);
}

#[test]
fn project_sort_newest_orders_by_created_desc() {
    let items = fixture();
    let (_, owned, _) = project_albums(&items, "me", "", AlbumsSort::Newest);
    let ids: Vec<&str> = owned.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec!["a1", "a4", "a3"]);
}

#[test]
fn project_sort_name_is_case_insensitive() {
    let items = fixture();
    let (_, owned, _) = project_albums(&items, "me", "", AlbumsSort::Name);
    let names: Vec<&str> = owned.iter().map(|a| a.album_name.as_str()).collect();
    // "Archive 2020", "Beach Trip", "beach-volleyball" — lowercase-sorted.
    assert_eq!(
        names,
        vec!["Archive 2020", "Beach Trip", "beach-volleyball"]
    );
}

#[test]
fn project_sort_most_assets_descends() {
    let items = fixture();
    let (_, owned, _) = project_albums(&items, "me", "", AlbumsSort::MostAssets);
    let counts: Vec<u32> = owned.iter().map(|a| a.asset_count).collect();
    assert_eq!(counts, vec![1000, 200, 12]);
}

#[test]
fn shared_by_you_keeps_only_albums_with_other_members() {
    let mut items = fixture();
    items[0].album_users.push(AlbumUser {
        user: AlbumUserInfo {
            id: "friend".into(),
            name: "Friend".into(),
            email: String::new(),
        },
        role: "viewer".into(),
    });
    let (_, owned, _) = project_albums(&items, "me", "", AlbumsSort::Newest);
    let shared: Vec<&str> = shared_by_you(owned).iter().map(|a| a.id.as_str()).collect();
    assert_eq!(shared, ["a1"]);
}

#[test]
fn shared_by_you_includes_albums_shared_only_by_link() {
    let mut items = fixture();
    items[2].shared = true;
    let (_, owned, _) = project_albums(&items, "me", "", AlbumsSort::Newest);
    let shared: Vec<&str> = shared_by_you(owned).iter().map(|a| a.id.as_str()).collect();
    assert_eq!(shared, ["a3"]);
}

#[test]
fn project_buckets_by_owner() {
    let items = fixture();
    let (_, owned, shared) = project_albums(&items, "me", "", AlbumsSort::Newest);
    assert_eq!(owned.len(), 3);
    assert_eq!(shared.len(), 1);
    assert_eq!(shared[0].id, "a2");
}

#[test]
fn project_empty_current_user_excludes_from_both_buckets() {
    // Without a known user id we can't safely classify owner vs shared,
    // so both buckets stay empty to avoid mis-labeling.
    let items = fixture();
    let (recent, owned, shared) = project_albums(&items, "", "", AlbumsSort::Newest);
    assert!(owned.is_empty());
    assert!(shared.is_empty());
    // Recent still populates regardless of user identity.
    assert_eq!(recent.len(), 4);
}

#[test]
fn project_recent_always_by_date_even_under_alternate_sort() {
    let items = fixture();
    let (recent, _, _) = project_albums(&items, "me", "", AlbumsSort::MostAssets);
    let ids: Vec<&str> = recent.iter().map(|a| a.id.as_str()).collect();
    // Recent is creation-order, not asset-count, so the "Family" album
    // (newest) leads even though it's not the largest.
    assert_eq!(ids[0], "a2");
}

#[test]
fn project_recent_caps_at_eight() {
    let mut items = Vec::new();
    for i in 0..20 {
        items.push(album(
            &format!("id{i}"),
            &format!("Album {i}"),
            "me",
            i,
            &format!("2024-01-{:02}T00:00:00Z", i + 1),
        ));
    }
    let (recent, _, _) = project_albums(&items, "me", "", AlbumsSort::Newest);
    assert_eq!(recent.len(), 8);
}

#[test]
fn tile_actions_follow_album_owner() {
    let items = fixture();
    assert_eq!(tile_actions(&items[0], "me"), Some(OWN_ALBUM_ACTIONS));
    assert_eq!(tile_actions(&items[1], "me"), Some(SHARED_ALBUM_ACTIONS));
    assert_eq!(tile_actions(&items[0], ""), None);
}
