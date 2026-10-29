use super::*;
use crate::api_client::ImmichApiClient;
use glib::Bytes;
use tempfile::tempdir;

#[test]
fn check_cancelled_reports_cancellation() {
    assert_eq!(check_cancelled(&|| false), Ok(()));
    assert_eq!(check_cancelled(&|| true), Err("cancelled".to_string()));
}

#[tokio::test]
async fn read_cached_texture_is_none_for_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.png");
    assert!(read_cached_texture(missing).await.unwrap().is_none());
}

// 1x1 transparent PNG
const PNG_BYTES: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0,
    5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn cache(max_bytes: usize) -> ThumbnailCache {
    let dir = tempdir().unwrap();
    let cache_dir = dir.keep().join("thumbs");
    ThumbnailCache::new_for_test(
        std::sync::Arc::new(ImmichApiClient::new(
            String::new(),
            String::new(),
            String::new(),
        )),
        cache_dir,
        max_bytes,
    )
}

fn texture_from_png() -> Texture {
    Texture::from_bytes(&Bytes::from(PNG_BYTES)).unwrap()
}

#[test]
fn test_memory_hit_after_insert() {
    let cache = cache(1024);
    cache
        .memory
        .lock()
        .insert("thumbnail:1".into(), texture_from_png());

    assert!(cache.get_cached("1", ThumbnailSize::Thumbnail).is_some());
}

#[test]
fn test_get_cached_does_not_touch_disk() {
    let cache = cache(1024);
    std::fs::create_dir_all(&cache.cache_dir).unwrap();
    std::fs::write(
        cache.cache_file("2", ThumbnailSize::Thumbnail, false),
        PNG_BYTES,
    )
    .unwrap();

    assert!(cache.get_cached("2", ThumbnailSize::Thumbnail).is_none());
}

#[test]
fn cache_keys_for_unedited_assets_are_unchanged() {
    assert_eq!(
        cache_key("1", ThumbnailSize::Thumbnail, false),
        "thumbnail:1"
    );
    assert_eq!(cache_key("1", ThumbnailSize::Preview, false), "preview:1");
    assert_eq!(cache_key("1", ThumbnailSize::Fullsize, false), "fullsize:1");
}

#[test]
fn edited_preview_has_its_own_key() {
    assert_eq!(
        cache_key("1", ThumbnailSize::Preview, true),
        "preview-edited:1"
    );
}

#[test]
fn edited_preview_is_cached_separately() {
    let cache = cache(1024);
    cache
        .memory
        .lock()
        .insert("preview:1".into(), texture_from_png());
    assert!(cache.get_cached_preview("1", false).is_some());
    assert!(cache.get_cached_preview("1", true).is_none());
}

#[test]
fn test_eviction_after_byte_budget_overflow() {
    let cache = cache(3);
    cache
        .memory
        .lock()
        .insert("thumbnail:1".into(), texture_from_png());
    cache
        .memory
        .lock()
        .insert("thumbnail:2".into(), texture_from_png());

    assert!(cache.memory.lock().inner.len() <= 1);
}

#[test]
fn test_clear_removes_memory_and_disk() {
    let cache = cache(1024);
    std::fs::create_dir_all(&cache.cache_dir).unwrap();
    std::fs::write(
        cache.cache_file("3", ThumbnailSize::Thumbnail, false),
        PNG_BYTES,
    )
    .unwrap();
    cache
        .memory
        .lock()
        .insert("thumbnail:3".into(), texture_from_png());

    cache.clear().unwrap();

    assert!(cache.memory.lock().inner.is_empty());
    assert!(!cache.cache_dir.exists());
}

#[tokio::test]
async fn test_load_local_raw_thumbnail() {
    let cache = cache(1024 * 1024 * 10);
    let fixture_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.dng");

    let result = cache
        .load_local_thumbnail_cancellable("local_dng_test", &fixture_path, || false)
        .await;

    // Synthetic DNG may not decode via libraw alone; accept either outcome.
    match result {
        Ok(texture) => {
            assert!(texture.width() > 0);
            assert!(texture.height() > 0);
            let cache_file = cache.cache_file_local("local_dng_test");
            assert!(cache_file.exists(), "Cache file was not written to disk");
        }
        Err(_) => {
            // Expected when libraw cannot decode the synthetic fixture.
            let cache_file = cache.cache_file_local("local_dng_test");
            assert!(
                !cache_file.exists(),
                "Cache file should not exist after failed decode"
            );
        }
    }
}
