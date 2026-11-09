use super::*;
use crate::api_client::LibraryAsset;

fn asset(id: &str) -> LibraryAsset {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "originalFileName": format!("{id}.jpg"),
        "originalMimeType": "image/jpeg",
        "fileCreatedAt": "2024-01-15T14:25:15.000Z",
        "type": "IMAGE"
    }))
    .unwrap()
}

#[test]
fn filtered_diff_keeps_only_chosen_directions() {
    let diff = AlbumDiff {
        to_download: vec![asset("d1"), asset("d2")],
        to_delete_remote: vec![asset("r1")],
        remote_unhashed: 3,
        ..AlbumDiff::default()
    };
    let choices = SyncChoices {
        upload: true,
        download: true,
        delete_remote: false,
        delete_local: true,
    };
    let filtered = filtered_diff(&diff, choices);
    assert_eq!(filtered.to_download.len(), 2);
    assert!(filtered.to_delete_remote.is_empty());
    assert_eq!(filtered.remote_unhashed, 0);
}

#[test]
fn sync_choices_any_needs_one_direction() {
    let none = SyncChoices {
        upload: false,
        download: false,
        delete_remote: false,
        delete_local: false,
    };
    assert!(!none.any());
    assert!(
        SyncChoices {
            delete_local: true,
            ..none
        }
        .any()
    );
}

#[test]
fn messages_mention_unmatched_items_only_when_present() {
    assert_eq!(in_sync_message(0), "Already in sync.");
    assert!(in_sync_message(2).contains("2 remote items"));
    assert_eq!(unmatched_note(0), "");
    assert!(unmatched_note(1).contains("1 remote item couldn't"));
}
