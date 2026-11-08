use super::*;

fn album(id: &str, name: &str, role: &str, updated: &str) -> LibraryAlbum {
    let owner = if role == "owner" { "me" } else { "boss" };
    let mut users = vec![serde_json::json!({ "role": "owner", "user": { "id": owner } })];
    if role != "owner" {
        users.push(serde_json::json!({ "role": role, "user": { "id": "me" } }));
    }
    serde_json::from_value(serde_json::json!({
        "id": id,
        "albumName": name,
        "assetCount": 1,
        "albumThumbnailAssetId": null,
        "createdAt": "2024-01-01T00:00:00.000Z",
        "updatedAt": updated,
        "albumUsers": users
    }))
    .unwrap()
}

fn fixture() -> Vec<LibraryAlbum> {
    vec![
        album("a", "Beach Trip", "owner", "2024-01-01T00:00:00.000Z"),
        album("b", "Family", "editor", "2024-03-01T00:00:00.000Z"),
        album("c", "Wedding", "viewer", "2024-04-01T00:00:00.000Z"),
        album("d", "Garden", "owner", "2024-02-01T00:00:00.000Z"),
    ]
}

#[test]
fn picker_lists_owned_and_editor_albums_newest_first() {
    let albums = fixture();
    let ids: Vec<&str> = pickable_albums(&albums, "me", "")
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(ids, ["b", "d", "a"]);
}

#[test]
fn picker_filter_ignores_case() {
    let albums = fixture();
    let names: Vec<&str> = pickable_albums(&albums, "me", "TRIP")
        .iter()
        .map(|a| a.album_name.as_str())
        .collect();
    assert_eq!(names, ["Beach Trip"]);
}

#[test]
fn added_message_variants() {
    let o = |done, duplicate, denied| BulkOutcome {
        done,
        duplicate,
        denied,
    };
    assert_eq!(added_message(o(1, 0, 0), "Trip"), "Added 1 item to “Trip”");
    assert_eq!(
        added_message(o(3, 2, 0), "Trip"),
        "Added 3 items to “Trip” · 2 already there"
    );
    assert_eq!(added_message(o(0, 2, 0), "Trip"), "Already in “Trip”");
    assert_eq!(
        added_message(o(2, 0, 1), "Trip"),
        "Added 2 items to “Trip” · 1 not allowed"
    );
    assert_eq!(added_message(o(0, 0, 3), "Trip"), "Couldn't add to “Trip”");
}
