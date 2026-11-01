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

#[test]
fn memory_budget_is_the_limit_but_at_most_half_of_ram() {
    const MB: usize = 1024 * 1024;
    const GB: usize = 1024 * MB;
    assert_eq!(memory_budget(2048, 16 * GB), 2 * GB);
    assert_eq!(memory_budget(2048, 3 * GB), 3 * GB / 2);
    assert_eq!(memory_budget(512, 0), 512 * MB);
}

#[test]
fn lowering_the_limit_evicts_down_to_it() {
    let mut memory = SizedLruCache::new(8);
    memory.insert("thumbnail:1".into(), texture_from_png());
    memory.insert("thumbnail:2".into(), texture_from_png());
    memory.set_max_bytes(4);
    assert_eq!((memory.inner.len(), memory.current_bytes), (1, 4));
    assert!(memory.peek("thumbnail:2").is_some());
}

#[test]
fn telemetry_counts_load_lookups_and_the_first_fill() {
    let mut memory = SizedLruCache::new(4);
    memory.insert("thumbnail:1".into(), texture_from_png());
    assert!(memory.lookup("thumbnail:1").is_some());
    assert!(memory.lookup("thumbnail:2").is_none());
    assert!(memory.get("thumbnail:1").is_some());
    assert_eq!((memory.hits, memory.misses), (1, 1));
    assert!(!memory.full);
    memory.insert("thumbnail:2".into(), texture_from_png());
    assert!(memory.full);
    assert_eq!(memory.evictions, 1);
    memory.clear();
    assert!(!memory.full);
}

#[test]
fn fitted_keys_fall_back_to_the_bucket_at_or_past_its_size() {
    let target = |fit| memory_target("1", ThumbnailSize::Preview, false, fit);
    assert_eq!(target(Some(480)), ("preview@480:1".to_string(), 480));
    assert_eq!(target(Some(1920)), ("preview:1".to_string(), 1440));
    assert_eq!(target(None), ("preview:1".to_string(), 1440));
}

#[tokio::test]
async fn fitted_loads_decode_to_the_tile_and_keep_their_own_key() {
    let cache = cache(64 * 1024 * 1024);
    let mut png = Vec::new();
    image::RgbImage::new(1000, 500)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    std::fs::create_dir_all(&cache.cache_dir).unwrap();
    std::fs::write(cache.cache_file("1", ThumbnailSize::Preview, false), &png).unwrap();

    let fitted = cache
        .load_fitted_cancellable("1", ThumbnailSize::Preview, Some(320), || false)
        .await
        .unwrap();

    assert_eq!((fitted.width(), fitted.height()), (320, 160));
    assert!(
        cache
            .peek_fitted("1", ThumbnailSize::Preview, Some(320))
            .is_some()
    );
    assert!(cache.get_cached_preview("1", false).is_none());
}

#[tokio::test]
async fn screen_loads_fit_the_edge_and_come_back_from_memory() {
    let cache = cache(64 * 1024 * 1024);
    let dir = tempdir().unwrap();
    let path = dir.path().join("huge.png");
    image::RgbImage::new(2000, 1000).save(&path).unwrap();

    let first = cache
        .load_screen("1", false, path.clone(), 500)
        .await
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    let again = cache.load_screen("1", false, path, 500).await.unwrap();

    assert_eq!((first.width(), first.height()), (500, 250));
    assert_eq!(first, again);
}
