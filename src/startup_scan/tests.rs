use super::{effective_album_name, hash_to_task};
use crate::config::{FolderRules, WatchPathEntry};
use crate::monitor::is_supported_media_path;
use std::path::PathBuf;

#[test]
fn test_supported_media_path_filter() {
    assert!(is_supported_media_path(&PathBuf::from("image.avif")));
    assert!(is_supported_media_path(&PathBuf::from("image.jpg")));
    assert!(is_supported_media_path(&PathBuf::from("movie.mkv")));
    assert!(is_supported_media_path(&PathBuf::from("movie.mp4")));
    assert!(!is_supported_media_path(&PathBuf::from("notes.txt")));
}

#[test]
fn library_only_entry_has_no_effective_album_name() {
    let entry = WatchPathEntry::WithConfig {
        path: "/home/user/Camera".into(),
        album_id: None,
        album_name: None,
        rules: FolderRules::default(),
    };

    assert_eq!(
        effective_album_name(&entry, std::path::Path::new("/home/user/Camera/photo.jpg")),
        None
    );
}

#[tokio::test]
async fn library_only_startup_task_skips_album_association() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg");
    std::fs::write(&path, b"photo").unwrap();

    let task = hash_to_task(
        path.to_string_lossy().into_owned(),
        dir.path().to_string_lossy().into_owned(),
        None,
        None,
        false,
        Some("checksum".into()),
        false,
    )
    .await
    .unwrap();

    assert!(task.skip_album);
    assert_eq!(task.album_id, None);
    assert_eq!(task.album_name, None);
}
