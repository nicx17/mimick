use super::*;
use std::path::Path;

fn album_with_role(role: &str) -> LibraryAlbum {
    let owner = if role == "owner" { "me" } else { "boss" };
    let mut users = vec![serde_json::json!({ "role": "owner", "user": { "id": owner } })];
    if role != "owner" {
        users.push(serde_json::json!({ "role": role, "user": { "id": "me" } }));
    }
    serde_json::from_value(serde_json::json!({
        "id": "a1",
        "albumName": "Trip",
        "assetCount": 1,
        "albumThumbnailAssetId": null,
        "createdAt": "2024-01-01T00:00:00.000Z",
        "updatedAt": "2024-01-01T00:00:00.000Z",
        "albumUsers": users
    }))
    .unwrap()
}

#[test]
fn album_reads_order_and_activity() {
    let album: LibraryAlbum = serde_json::from_value(serde_json::json!({
        "id": "a1",
        "albumName": "Trip",
        "assetCount": 1,
        "albumThumbnailAssetId": null,
        "createdAt": "2024-01-01T00:00:00.000Z",
        "updatedAt": "2024-01-01T00:00:00.000Z",
        "order": "asc",
        "isActivityEnabled": false
    }))
    .unwrap();
    assert_eq!(album.order, Some(SortOrder::Asc));
    assert!(!album.is_activity_enabled);
}

#[test]
fn editors_and_owners_can_edit_assets_viewers_cannot() {
    assert!(album_with_role("owner").can_edit_assets("me"));
    assert!(album_with_role("editor").can_edit_assets("me"));
    assert!(!album_with_role("viewer").can_edit_assets("me"));
    assert_eq!(album_with_role("editor").role_of("me"), Some("editor"));
    assert_eq!(album_with_role("editor").role_of("nobody"), None);
}

#[test]
fn album_update_sends_only_the_cover() {
    let update = AlbumUpdate {
        album_thumbnail_asset_id: Some("x".into()),
        ..AlbumUpdate::default()
    };
    assert_eq!(
        serde_json::to_value(&update).unwrap(),
        serde_json::json!({ "albumThumbnailAssetId": "x" })
    );
}

#[test]
fn album_update_sends_order_and_activity() {
    let update = AlbumUpdate {
        order: Some(SortOrder::Asc),
        is_activity_enabled: Some(false),
        ..AlbumUpdate::default()
    };
    assert_eq!(
        serde_json::to_value(&update).unwrap(),
        serde_json::json!({ "order": "asc", "isActivityEnabled": false })
    );
}

#[test]
fn normalize_checksum_passes_through_lowercase_hex() {
    let hex = "823cb0e790d643a90f61f07e5c2bdd588cf8f230";
    assert_eq!(normalize_checksum_to_hex(hex), Some(hex.to_string()));
}

#[test]
fn normalize_checksum_lowercases_uppercase_hex() {
    let hex_up = "823CB0E790D643A90F61F07E5C2BDD588CF8F230";
    assert_eq!(
        normalize_checksum_to_hex(hex_up),
        Some(hex_up.to_ascii_lowercase())
    );
}

#[test]
fn normalize_checksum_decodes_base64_to_hex() {
    // Base64 of the 20-byte SHA1 0x82 0x3c ... 0x30
    let b64 = "gjyw55DWQ6kPYfB+XCvdWIz48jA=";
    let expected = "823cb0e790d643a90f61f07e5c2bdd588cf8f230";
    assert_eq!(normalize_checksum_to_hex(b64), Some(expected.to_string()));
}

#[test]
fn normalize_checksum_rejects_empty_and_unknown() {
    assert_eq!(normalize_checksum_to_hex(""), None);
    assert_eq!(normalize_checksum_to_hex("not-a-checksum"), None);
}

#[test]
fn library_asset_deserializes_base64_checksum_as_hex() {
    let json = serde_json::json!({
        "id": "asset-1",
        "originalFileName": "a.png",
        "originalMimeType": "image/png",
        "fileCreatedAt": "2024-01-01T00:00:00.000Z",
        "type": "IMAGE",
        "checksum": "gjyw55DWQ6kPYfB+XCvdWIz48jA="
    });
    let asset: LibraryAsset = serde_json::from_value(json).unwrap();
    assert_eq!(
        asset.checksum.as_deref(),
        Some("823cb0e790d643a90f61f07e5c2bdd588cf8f230")
    );
}

#[test]
fn library_asset_reads_is_edited_flag() {
    let json = serde_json::json!({
        "id": "asset-1",
        "originalFileName": "a.jpg",
        "originalMimeType": "image/jpeg",
        "fileCreatedAt": "2024-01-01T00:00:00.000Z",
        "type": "IMAGE",
        "isEdited": true
    });
    let asset: LibraryAsset = serde_json::from_value(json).unwrap();
    assert!(asset.is_edited);
}

#[test]
fn library_asset_defaults_is_edited_to_false() {
    let json = serde_json::json!({
        "id": "asset-1",
        "originalFileName": "a.jpg",
        "originalMimeType": "image/jpeg",
        "fileCreatedAt": "2024-01-01T00:00:00.000Z",
        "type": "IMAGE"
    });
    let asset: LibraryAsset = serde_json::from_value(json).unwrap();
    assert!(!asset.is_edited);
}

#[test]
fn test_unix_to_utc_iso8601() {
    assert_eq!(unix_to_utc_iso8601(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(unix_to_utc_iso8601(1704067200), "2024-01-01T00:00:00.000Z");
}

#[test]
fn test_mime_for_path() {
    assert_eq!(mime_for_path(Path::new("test.avif")), "image/avif");
    assert_eq!(mime_for_path(Path::new("test.jpg")), "image/jpeg");
    assert_eq!(mime_for_path(Path::new("test.jpe")), "image/jpeg");
    assert_eq!(mime_for_path(Path::new("test.heif")), "image/heif");
    assert_eq!(mime_for_path(Path::new("test.jp2")), "image/jp2");
    assert_eq!(mime_for_path(Path::new("test.jxl")), "image/jxl");
    assert_eq!(mime_for_path(Path::new("test.PNG")), "image/png");
    assert_eq!(
        mime_for_path(Path::new("test.psd")),
        "image/vnd.adobe.photoshop"
    );
    assert_eq!(mime_for_path(Path::new("test.svg")), "image/svg+xml");
    assert_eq!(mime_for_path(Path::new("test.mp4")), "video/mp4");
    assert_eq!(mime_for_path(Path::new("test.insv")), "video/mp4");
    assert_eq!(mime_for_path(Path::new("test.mkv")), "video/x-matroska");
    assert_eq!(mime_for_path(Path::new("test.mxf")), "application/mxf");
    assert_eq!(
        mime_for_path(Path::new("test.unknown")),
        "application/octet-stream"
    );
}

#[test]
fn test_mime_for_path_covers_immich_spec() {
    // Every extension from pv_docs/Library_view_feature.md must map to a
    // non-fallback MIME so uploads/downloads pick the right pipeline.
    const SPEC_EXTENSIONS: &[&str] = &[
        // RAW
        "3fr", "ari", "arw", "cap", "cin", "cr2", "cr3", "crw", "dcr", "dng", "erf", "fff", "iiq",
        "k25", "kdc", "mrw", "nef", "nrw", "orf", "ori", "pef", "psd", "raf", "raw", "rw2", "rwl",
        "sr2", "srf", "srw", "x3f", // Web image
        "avif", "bmp", "gif", "jpeg", "jpg", "png", "webp", // Other image
        "heic", "heif", "hif", "insp", "jp2", "jpe", "jxl", "svg", "tif", "tiff",
        // Video
        "3gp", "3gpp", "avi", "flv", "insv", "m2t", "m2ts", "m4v", "mkv", "mov", "mp4", "mpe",
        "mpeg", "mpg", "mts", "mxf", "ts", "vob", "webm", "wmv",
    ];
    for ext in SPEC_EXTENSIONS {
        let p = std::path::PathBuf::from(format!("a.{}", ext));
        let mime = mime_for_path(&p);
        assert_ne!(
            mime, "application/octet-stream",
            "extension `.{}` falls through to octet-stream",
            ext
        );
    }
}

#[test]
fn test_classify_http_issue_for_invalid_api_key() {
    let issue = classify_http_issue(RequestContext::Upload, 401, Some("photo.jpg"));
    assert_eq!(issue.summary, "Immich rejected the API key");
    assert!(issue.guidance.contains("API key"));
}

#[test]
fn test_classify_http_issue_for_album_assign_404() {
    let issue = classify_http_issue(RequestContext::AlbumAssign, 404, Some("album-1"));
    assert_eq!(issue.summary, "An album reference is no longer valid");
}

#[test]
fn test_classify_http_issue_404_outside_albums_uses_context_summary() {
    let issue = classify_http_issue(RequestContext::ThumbnailFetch, 404, None);
    assert_eq!(issue.summary, "Immich could not load a library thumbnail");
    assert!(issue.guidance.contains("HTTP 404"));
}

#[test]
fn test_classify_http_issue_for_gateway_errors() {
    let issue = classify_http_issue(RequestContext::AssetList, 503, None);
    assert_eq!(issue.summary, "Immich is temporarily unavailable");
}

#[test]
fn test_classify_http_issue_names_the_upload_subject() {
    let named = classify_http_issue(RequestContext::Upload, 500, Some("photo.jpg"));
    assert_eq!(named.summary, "Immich could not accept photo.jpg");
    let unnamed = classify_http_issue(RequestContext::Upload, 500, None);
    assert_eq!(unnamed.summary, "Immich could not accept the upload");
}

#[test]
fn test_library_album_deserializes_from_immich_shape() {
    let album: LibraryAlbum = serde_json::from_value(serde_json::json!({
        "id": "album-1",
        "albumName": "Trips",
        "assetCount": 42,
        "albumThumbnailAssetId": "asset-9",
        "createdAt": "2024-01-01T00:00:00.000Z",
        "updatedAt": "2024-01-02T00:00:00.000Z",
        "description": "Vacation",
        "shared": true
    }))
    .unwrap();

    assert_eq!(album.id, "album-1");
    assert_eq!(album.album_name, "Trips");
    assert_eq!(album.asset_count, 42);
    assert_eq!(album.thumbnail_asset_id.as_deref(), Some("asset-9"));
    assert_eq!(album.description, "Vacation");
    assert!(album.shared && album.is_shared());
}

#[test]
fn test_library_asset_deserializes_from_search_result_shape() {
    let asset: LibraryAsset = serde_json::from_value(serde_json::json!({
        "id": "asset-1",
        "originalFileName": "IMG_0001.JPG",
        "originalMimeType": "image/jpeg",
        "fileCreatedAt": "2024-01-01T12:00:00.000Z",
        "type": "IMAGE",
        "thumbhash": "abcd",
        "width": 4032,
        "height": 3024
    }))
    .unwrap();

    assert_eq!(asset.id, "asset-1");
    assert_eq!(asset.filename, "IMG_0001.JPG");
    assert_eq!(asset.mime_type, "image/jpeg");
    assert_eq!(asset.asset_type, "IMAGE");
    assert_eq!(asset.thumbhash.as_deref(), Some("abcd"));
    assert_eq!(asset.width, Some(4032));
    assert_eq!(asset.height, Some(3024));
    assert!(asset.checksum.is_none());
    assert!(asset.local_date_time.is_none());
}

#[test]
fn library_asset_reads_local_date_time() {
    let asset: LibraryAsset = serde_json::from_value(serde_json::json!({
        "id": "asset-1",
        "originalFileName": "IMG_0001.JPG",
        "originalMimeType": "image/jpeg",
        "fileCreatedAt": "2026-03-31T18:00:00.000Z",
        "localDateTime": "2026-03-31T23:30:00.000Z",
        "type": "IMAGE"
    }))
    .unwrap();
    assert_eq!(
        asset.local_date_time.as_deref(),
        Some("2026-03-31T23:30:00.000Z")
    );
}

#[test]
fn test_search_response_deserializes_items() {
    let response: SearchResponse = serde_json::from_value(serde_json::json!({
        "assets": {
            "items": [
                {
                    "id": "asset-1",
                    "originalFileName": "IMG_0001.JPG",
                    "originalMimeType": "image/jpeg",
                    "fileCreatedAt": "2024-01-01T12:00:00.000Z",
                    "type": "IMAGE",
                    "thumbhash": null,
                    "width": 12,
                    "height": 10
                }
            ]
        }
    }))
    .unwrap();

    assert_eq!(response.assets.items.len(), 1);
    assert_eq!(response.assets.items[0].filename, "IMG_0001.JPG");
    assert!(response.assets.next_page.is_none());
}

#[test]
fn test_search_response_parses_next_page() {
    let response: SearchResponse = serde_json::from_value(serde_json::json!({
        "assets": {
            "items": [],
            "nextPage": "2"
        }
    }))
    .unwrap();
    assert_eq!(response.assets.next_page.as_deref(), Some("2"));
}

#[test]
fn test_server_structs_deserialize() {
    let stats: ServerStats = serde_json::from_value(serde_json::json!({
        "images": 100,
        "videos": 25,
        "total": 125
    }))
    .unwrap();
    let about: ServerAbout = serde_json::from_value(serde_json::json!({
        "version": "1.132.0"
    }))
    .unwrap();

    assert_eq!(stats.total, 125);
    assert_eq!(about.version, "1.132.0");
}

#[test]
fn test_thumbnail_size_serialization_values() {
    assert_eq!(ThumbnailSize::Thumbnail.as_str(), "thumbnail");
    assert_eq!(ThumbnailSize::Preview.as_str(), "preview");
}

#[test]
fn test_normalize_file_timestamps_prefers_earliest_available_time_for_created_at() {
    let (created, modified) =
        normalize_file_timestamps(Some(1_704_153_600), Some(1_704_067_200), 99);

    assert_eq!(created, 1_704_067_200);
    assert_eq!(modified, 1_704_067_200);
}

#[test]
fn test_normalize_file_timestamps_falls_back_to_created_time_when_modified_is_missing() {
    let (created, modified) = normalize_file_timestamps(Some(1_704_067_200), None, 99);

    assert_eq!(created, 1_704_067_200);
    assert_eq!(modified, 1_704_067_200);
}

#[test]
fn test_looks_like_iana_timezone() {
    assert!(looks_like_iana_timezone("Asia/Kolkata"));
    assert!(looks_like_iana_timezone("America/New_York"));
    assert!(!looks_like_iana_timezone("UTC"));
    assert!(!looks_like_iana_timezone("Africa Abidjan"));
}

#[tokio::test]
async fn test_active_route_label_tracks_selected_url() {
    let client = ImmichApiClient::new(
        "http://lan.example".into(),
        "https://wan.example".into(),
        "token".into(),
    );
    *client.active_url.lock().await = Some("https://wan.example".into());

    assert_eq!(client.active_route_label().await.as_deref(), Some("WAN"));
}

#[test]
fn person_deserializes_with_is_hidden_field() {
    let person: Person = serde_json::from_value(serde_json::json!({
        "id": "p1",
        "name": "Alice",
        "isHidden": true
    }))
    .expect("person json");
    assert_eq!(person.id, "p1");
    assert_eq!(person.name, "Alice");
    assert!(person.is_hidden);
}

#[test]
fn person_defaults_is_hidden_when_missing() {
    let person: Person = serde_json::from_value(serde_json::json!({
        "id": "p2",
        "name": ""
    }))
    .expect("person json");
    assert!(!person.is_hidden, "missing isHidden must default to false");
}

#[test]
fn server_statistics_parses_usage_by_user() {
    let stats: ServerStatistics = serde_json::from_value(serde_json::json!({
        "photos": 42,
        "videos": 7,
        "usage": 1024,
        "usageByUser": [
            {
                "userId": "u1",
                "userName": "alice",
                "photos": 10,
                "videos": 2,
                "usage": 512,
                "quotaSizeInBytes": 4096
            }
        ]
    }))
    .expect("server statistics json");
    assert_eq!(stats.photos, 42);
    assert_eq!(stats.videos, 7);
    assert_eq!(stats.usage, 1024);
    assert_eq!(stats.usage_by_user.len(), 1);
    let user = &stats.usage_by_user[0];
    assert_eq!(user.user_name, "alice");
    assert_eq!(user.photos, 10);
    assert_eq!(user.quota_size_in_bytes, Some(4096));
}

#[test]
fn server_statistics_tolerates_missing_quota() {
    let stats: ServerStatistics = serde_json::from_value(serde_json::json!({
        "photos": 0,
        "videos": 0,
        "usage": 0,
        "usageByUser": [
            { "userId": "u1", "userName": "anon", "photos": 0, "videos": 0, "usage": 0 }
        ]
    }))
    .expect("server statistics json");
    assert_eq!(stats.usage_by_user[0].quota_size_in_bytes, None);
}

#[test]
fn asset_update_serializes_only_set_fields_in_camel_case() {
    let update = AssetUpdate {
        date_time_original: Some("2024-01-15T19:55:15+05:30".into()),
        is_favorite: Some(true),
        ..AssetUpdate::default()
    };
    assert_eq!(
        serde_json::to_value(&update).unwrap(),
        serde_json::json!({
            "dateTimeOriginal": "2024-01-15T19:55:15+05:30",
            "isFavorite": true,
        })
    );
    assert_eq!(
        serde_json::to_value(AssetUpdate::default()).unwrap(),
        serde_json::json!({})
    );
}

#[test]
fn asset_details_reads_favorite_flag() {
    let details: AssetDetails =
        serde_json::from_value(serde_json::json!({ "isFavorite": true })).unwrap();
    assert!(details.is_favorite);
    assert!(details.exif_info.is_none());
}

#[test]
fn server_about_reads_full_and_minimal_payloads() {
    let full: ServerAbout = serde_json::from_value(serde_json::json!({
        "version": "v3.3.1",
        "versionUrl": "https://github.com/immich-app/immich/releases/tag/v3.3.1",
        "licensed": false,
        "nodejs": "v24.21.0",
        "exiftool": "13.59",
        "sourceRef": "v3.3.1",
        "sourceCommit": "db6e11ecc"
    }))
    .unwrap();
    assert_eq!(full.nodejs.as_deref(), Some("v24.21.0"));
    assert_eq!(full.source_commit.as_deref(), Some("db6e11ecc"));
    let minimal: ServerAbout =
        serde_json::from_value(serde_json::json!({ "version": "v1.0.0" })).unwrap();
    assert_eq!(minimal.version, "v1.0.0");
    assert!(minimal.ffmpeg.is_none());
}

#[test]
fn version_history_entry_deserializes() {
    let entries: Vec<VersionHistoryEntry> = serde_json::from_value(serde_json::json!([
        { "id": "a", "createdAt": "2026-10-09T08:00:00.000Z", "version": "3.3.1" }
    ]))
    .unwrap();
    assert_eq!(entries[0].version, "3.3.1");
}
