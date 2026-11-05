use super::*;
use crate::api_client::{Activity, ActivityKind, ActivityStatistics, LibraryAlbum, UserSummary};

fn activity(id: &str, kind: ActivityKind, user: &str, asset: Option<&str>) -> Activity {
    Activity {
        id: id.into(),
        kind,
        comment: (kind == ActivityKind::Comment).then(|| "Nice".into()),
        asset_id: asset.map(Into::into),
        created_at: "2024-01-01T00:00:00.000Z".into(),
        user: UserSummary {
            id: user.into(),
            name: String::new(),
            email: format!("{user}@example.com"),
        },
    }
}

#[test]
fn my_like_matches_user_and_level() {
    let all = [
        activity("1", ActivityKind::Like, "me", Some("p1")),
        activity("2", ActivityKind::Comment, "me", None),
        activity("3", ActivityKind::Like, "ana", None),
        activity("4", ActivityKind::Like, "me", None),
    ];
    assert_eq!(my_like(&all, "me", None).map(|a| a.id.as_str()), Some("4"));
    assert_eq!(
        my_like(&all, "me", Some("p1")).map(|a| a.id.as_str()),
        Some("1")
    );
    assert!(my_like(&all, "me", Some("p2")).is_none());
    assert!(my_like(&all, "", None).is_none());
}

#[test]
fn authors_and_album_owners_can_delete() {
    let comment = activity("1", ActivityKind::Comment, "ana", None);
    assert!(can_delete(&comment, "ana", "owner"));
    assert!(can_delete(&comment, "owner", "owner"));
    assert!(!can_delete(&comment, "ben", "owner"));
    assert!(!can_delete(&comment, "", ""));
}

#[test]
fn apply_change_removes_and_skips_duplicate_likes() {
    let like = activity("1", ActivityKind::Like, "me", None);
    let mut all = vec![like.clone()];
    apply_change(&mut all, None, Some(like));
    assert_eq!(all.len(), 1);
    apply_change(&mut all, Some("1"), None);
    assert!(all.is_empty());
}

#[test]
fn user_name_falls_back_to_email() {
    let mut user = activity("1", ActivityKind::Like, "ana", None).user;
    assert_eq!(user_name(&user), "ana@example.com");
    user.name = "Ana".into();
    assert_eq!(user_name(&user), "Ana");
}

#[test]
fn like_text_names_the_target() {
    let photo = activity("1", ActivityKind::Like, "ana", Some("p1"));
    let album = activity("2", ActivityKind::Like, "ana", None);
    assert_eq!(like_text(&photo), "Liked this photo");
    assert_eq!(like_text(&album), "Liked the album");
}

#[test]
fn stats_summary_counts_both_kinds() {
    assert_eq!(
        stats_summary(ActivityStatistics::default()),
        "0 likes · 0 comments"
    );
    let stats = ActivityStatistics {
        likes: 1,
        comments: 3,
    };
    assert_eq!(stats_summary(stats), "1 like · 3 comments");
}

#[test]
fn album_needs_sharing_and_activity_enabled() {
    let mut album: LibraryAlbum = serde_json::from_value(serde_json::json!({
        "id": "a1",
        "albumName": "Trip",
        "assetCount": 1,
        "albumThumbnailAssetId": null,
        "createdAt": "2024-01-01T00:00:00.000Z",
        "updatedAt": "2024-01-01T00:00:00.000Z",
        "shared": true,
        "isActivityEnabled": true
    }))
    .unwrap();
    assert!(has_activity(&album));
    album.is_activity_enabled = false;
    assert!(!has_activity(&album));
    album.is_activity_enabled = true;
    album.shared = false;
    assert!(!has_activity(&album));
}
