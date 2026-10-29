//! Handles persistent configuration loading and provides desktop keyring access helpers.
//!
//! Configuration lives in a JSON file under the XDG config directory.
//! Watch-path entries support both simple paths and extended per-folder
//! rules (sync method, extension filters, size limits). The API key is
//! stored in the desktop keyring via `oo7` when available, with an
//! automatic file-based fallback (base64-encoded, `0600` permissions)
//! for desktops where no keyring service is running.

mod folders;
pub use folders::*;
mod schema;
pub use schema::{ConfigData, DateHeadings};
mod keyring;
pub use keyring::ApiKeyStorage;
#[cfg(test)]
use keyring::{
    KeyringError, api_key_fallback_path, keyring_error_message, pick_api_key,
    read_api_key_file_fallback, remove_api_key_file_fallback, write_api_key_file_fallback,
};
use std::fs;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

/// Persistent config container wrapping loaded schema and file source info.
#[derive(Clone)]
pub struct Config {
    /// Active live configuration switches.
    pub data: ConfigData,
    /// Path to config file source.
    pub config_file: PathBuf,
}

impl Config {
    /// Load the config from the standard Mimick config path, creating a default file if missing.
    pub fn new() -> Self {
        let config_dir = crate::profile::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config").join(crate::profile::dir_segment()));

        let config_file = config_dir.join("config.json");

        let mut config = Config {
            data: ConfigData::default(),
            config_file,
        };

        config.load();
        config
    }

    /// Load or parse configuration state from standard path source.
    pub fn load(&mut self) -> bool {
        if self.config_file.exists() {
            if let Ok(content) = fs::read_to_string(&self.config_file) {
                if let Ok(data) = serde_json::from_str(&content) {
                    self.data = data;
                    log::info!("Config loaded from: {}", self.config_file.display());
                    return true;
                }
                log::warn!("Config parse failed: {}", self.config_file.display());
            }
        } else {
            log::info!(
                "No config found, creating default at: {}",
                self.config_file.display()
            );
            self.save();
        }
        false
    }

    /// Atomically write current configuration values back to disk.
    pub fn save(&self) -> bool {
        if let Ok(content) = serde_json::to_string_pretty(&self.data) {
            match crate::util::atomic_write(&self.config_file, content.as_bytes()) {
                Ok(()) => {
                    log::info!("Config saved to: {}", self.config_file.display());
                    true
                }
                Err(err) => {
                    log::error!(
                        "Failed to write config {}: {}",
                        self.config_file.display(),
                        err
                    );
                    false
                }
            }
        } else {
            false
        }
    }

    /// Return all configured watch paths as plain strings for the live monitor.
    pub fn watch_path_strings(&self) -> Vec<String> {
        self.data
            .watch_paths
            .iter()
            .map(|e| e.path().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album_watch_paths() -> Vec<WatchPathEntry> {
        let configured =
            |path: &str, id: Option<&str>, name: Option<&str>| WatchPathEntry::WithConfig {
                path: path.into(),
                album_id: id.map(Into::into),
                album_name: name.map(Into::into),
                rules: FolderRules {
                    sync_method: FolderSyncMethod::Full,
                    delete_folder_to_album: true,
                    ..FolderRules::default()
                },
            };
        vec![
            configured("/p/by-id", Some("a1"), Some("Trip")),
            configured("/p/by-name", None, Some("Trip")),
            WatchPathEntry::Simple("/p/Trip".into()),
            configured("/p/other", Some("a2"), Some("Other")),
            configured("/p/library", None, None),
            WatchPathEntry::Simple("/p/Pictures".into()),
        ]
    }

    #[test]
    fn album_folders_find_linked_entries() {
        let folders = album_folders(&album_watch_paths(), "a1", Some("Trip"), &[]);
        assert_eq!(folders.linked, ["/p/by-id", "/p/by-name", "/p/Trip"]);
        assert!(folders.named_after.is_empty());
    }

    #[test]
    fn album_folders_skip_name_matches_when_another_album_has_the_name() {
        let folders = album_folders(&album_watch_paths(), "a1", None, &[]);
        assert_eq!(folders.linked, ["/p/by-id"]);
    }

    #[test]
    fn album_folders_find_subfolders_named_after_the_album() {
        let synced = [
            "/p/Pictures/Trip".to_string(),
            "/p/by-id".to_string(),
            "/p/library/sub".to_string(),
        ];
        let folders = album_folders(&album_watch_paths(), "a1", Some("Trip"), &synced);
        assert_eq!(folders.named_after, ["/p/Pictures/Trip"]);
        assert_eq!(folders.linked, ["/p/by-id", "/p/by-name", "/p/Trip"]);
    }

    #[test]
    fn detaching_an_album_keeps_its_folders_uploading_to_the_library() {
        let mut entries = album_watch_paths();
        let folders = AlbumFolders {
            linked: vec!["/p/by-id".into(), "/p/Trip".into()],
            named_after: vec!["/p/Pictures/Trip".into()],
        };
        assert!(detach_album_from_watch_paths(&mut entries, &folders, true));
        let by_path = |path: &str| entries.iter().find(|e| e.path() == path).unwrap();
        for path in ["/p/by-id", "/p/Trip", "/p/Pictures/Trip"] {
            let entry = by_path(path);
            assert!(entry.uploads_to_library(), "{path}");
            assert_eq!(entry.sync_method(), FolderSyncMethod::UploadOnly);
            assert!(!entry.rules().delete_folder_to_album);
        }
        assert_eq!(by_path("/p/other").album_name(), Some("Other"));
        assert!(matches!(by_path("/p/Pictures"), WatchPathEntry::Simple(_)));
    }

    #[test]
    fn detaching_an_album_can_stop_watching_its_folders() {
        let mut entries = album_watch_paths();
        let folders = AlbumFolders {
            linked: vec!["/p/by-id".into(), "/p/by-name".into()],
            named_after: vec!["/p/Pictures/Trip".into()],
        };
        assert!(detach_album_from_watch_paths(&mut entries, &folders, false));
        let paths: Vec<&str> = entries.iter().map(WatchPathEntry::path).collect();
        assert_eq!(
            paths,
            [
                "/p/Trip",
                "/p/other",
                "/p/library",
                "/p/Pictures",
                "/p/Pictures/Trip"
            ]
        );
    }

    #[test]
    fn date_headings_default_to_day_and_read_lowercase() {
        let mut saved = serde_json::to_value(ConfigData::default()).unwrap();
        saved
            .as_object_mut()
            .unwrap()
            .remove("library_date_headings");
        let data: ConfigData = serde_json::from_value(saved).unwrap();
        assert_eq!(data.library_date_headings, DateHeadings::Day);
        let month: DateHeadings = serde_json::from_str("\"month\"").unwrap();
        assert_eq!(month, DateHeadings::Month);
        assert_eq!(
            serde_json::to_string(&DateHeadings::Off).unwrap(),
            "\"off\""
        );
    }

    #[test]
    fn test_watch_path_entry_parsing_simple() {
        let json = r#""/home/user/Pictures""#;
        let entry: WatchPathEntry = serde_json::from_str(json).unwrap();

        assert_eq!(entry.path(), "/home/user/Pictures");
        assert!(matches!(entry, WatchPathEntry::Simple(_)));
    }

    #[test]
    fn test_watch_path_entry_parsing_with_config() {
        let json = r#"{
            "path": "/home/user/Pictures",
            "album_id": "abc-123",
            "album_name": "My Album"
        }"#;
        let entry: WatchPathEntry = serde_json::from_str(json).unwrap();

        assert_eq!(entry.path(), "/home/user/Pictures");
        let WatchPathEntry::WithConfig { album_id, .. } = &entry else {
            panic!("expected configured watch entry");
        };
        assert_eq!(album_id.as_deref(), Some("abc-123"));
        assert_eq!(entry.album_name().unwrap(), "My Album");
        assert!(!entry.uploads_to_library());
    }

    #[test]
    fn test_library_watch_path_is_explicit_and_round_trips() {
        let entry = WatchPathEntry::WithConfig {
            path: "/home/user/Camera".into(),
            album_id: None,
            album_name: None,
            rules: FolderRules::default(),
        };

        assert!(entry.uploads_to_library());
        assert!(!WatchPathEntry::Simple("/home/user/Camera".into()).uploads_to_library());
        assert!(
            !WatchPathEntry::WithConfig {
                path: "/home/user/Camera".into(),
                album_id: Some("album-id".into()),
                album_name: Some("Library (No Album)".into()),
                rules: FolderRules::default(),
            }
            .uploads_to_library()
        );

        let json = serde_json::to_string(&entry).unwrap();
        let restored: WatchPathEntry = serde_json::from_str(&json).unwrap();
        assert!(restored.uploads_to_library());
    }

    #[test]
    fn test_library_rules_are_limited_to_safe_uploads() {
        let mut rules = FolderRules {
            sync_method: FolderSyncMethod::Full,
            delete_folder_to_album: true,
            delete_album_to_folder: true,
            ..FolderRules::default()
        };

        rules.restrict_to_library_uploads();

        assert_eq!(rules.sync_method, FolderSyncMethod::UploadOnly);
        assert!(!rules.delete_folder_to_album);
        assert!(!rules.delete_album_to_folder);
    }

    #[test]
    fn test_config_data_defaults() {
        let data = ConfigData::default();
        assert!(data.internal_url_enabled);
        assert!(data.external_url_enabled);
        assert!(!data.background_sync_enabled);
    }

    #[test]
    fn test_config_data_background_sync_defaults_false_when_missing() {
        let data: ConfigData = serde_json::from_str("{}").unwrap();
        assert!(!data.background_sync_enabled);
    }

    #[test]
    fn test_legacy_folder_rules_default_to_upload_only_and_global_startup_fallback() {
        let entry: WatchPathEntry = serde_json::from_str(
            r#"{
                "path": "/home/nick/Pictures",
                "rules": {
                    "ignore_hidden": true
                }
            }"#,
        )
        .unwrap();

        let rules = entry.rules();
        assert_eq!(rules.sync_method, FolderSyncMethod::UploadOnly);
        assert_eq!(
            entry.startup_catchup_mode(&StartupCatchupMode::RecentOnly),
            StartupCatchupMode::RecentOnly
        );
        assert!(!rules.delete_folder_to_album);
        assert!(!rules.delete_album_to_folder);
    }

    #[test]
    fn test_watch_path_strings_helper() {
        let mut data = ConfigData::default();
        data.watch_paths.push(WatchPathEntry::Simple("/a".into()));
        data.watch_paths.push(WatchPathEntry::WithConfig {
            path: "/b".into(),
            album_id: None,
            album_name: None,
            rules: FolderRules::default(),
        });

        let config = Config {
            data,
            config_file: PathBuf::from("dummy.json"),
        };

        let strings = config.watch_path_strings();
        assert_eq!(strings, vec!["/a".to_string(), "/b".to_string()]);
    }

    #[test]
    fn test_folder_rules_match_extension_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.jpg");
        fs::write(&path, vec![0u8; 1024]).unwrap();

        let rules = FolderRules {
            ignore_hidden: false,
            max_file_size_mb: Some(1),
            allowed_extensions: vec!["jpg".into(), "png".into()],
            ..FolderRules::default()
        };

        assert!(rules.matches(&path));

        let restricted = FolderRules {
            ignore_hidden: false,
            max_file_size_mb: Some(0),
            allowed_extensions: vec!["png".into()],
            ..FolderRules::default()
        };

        assert!(!restricted.matches(&path));
    }

    #[test]
    fn test_folder_rules_ignore_hidden_path_components() {
        let dir = tempfile::tempdir().unwrap();
        let hidden_dir = dir.path().join(".hidden");
        fs::create_dir_all(&hidden_dir).unwrap();
        let hidden_file = hidden_dir.join("photo.jpg");
        fs::write(&hidden_file, vec![0u8; 16]).unwrap();

        let rules = FolderRules {
            ignore_hidden: true,
            ..FolderRules::default()
        };

        assert!(!rules.matches(&hidden_file));
    }

    #[test]
    fn test_normalized_extensions_trims_and_lowercases() {
        let rules = FolderRules {
            allowed_extensions: vec![" JPG ".into(), ".PNG".into(), "".into()],
            ..FolderRules::default()
        };

        assert_eq!(rules.normalized_extensions(), vec!["jpg", "png"]);
    }

    #[test]
    fn test_best_matching_watch_entry_prefers_most_specific_path() {
        let entries = vec![
            WatchPathEntry::Simple("/home/user/Pictures".into()),
            WatchPathEntry::WithConfig {
                path: "/home/user/Pictures/Trips".into(),
                album_id: Some("album-1".into()),
                album_name: Some("Trips".into()),
                rules: FolderRules::default(),
            },
        ];

        let matched = best_matching_watch_entry(
            Path::new("/home/user/Pictures/Trips/day1/photo.jpg"),
            &entries,
        )
        .unwrap();
        assert_eq!(matched.path(), "/home/user/Pictures/Trips");
        assert_eq!(matched.album_name(), Some("Trips"));
    }

    #[test]
    fn test_best_matching_watch_entry_does_not_match_string_prefix_siblings() {
        let entries = vec![WatchPathEntry::Simple("/home/user/Pictures".into())];

        assert!(
            best_matching_watch_entry(Path::new("/home/user/Pictures-backup/photo.jpg"), &entries)
                .is_none()
        );
    }

    #[test]
    fn face_visibility_defaults_favour_discoverable_named_only() {
        let data = ConfigData::default();
        assert!(
            data.show_unnamed_faces,
            "unnamed people should be visible by default so users see them"
        );
        assert!(
            !data.show_hidden_faces,
            "hidden people stay hidden by default"
        );
    }

    #[test]
    fn face_visibility_flags_round_trip_through_json() {
        let data = ConfigData {
            show_unnamed_faces: false,
            show_hidden_faces: true,
            ..ConfigData::default()
        };
        let json = serde_json::to_string(&data).expect("serialize");
        let restored: ConfigData = serde_json::from_str(&json).expect("deserialize");
        assert!(!restored.show_unnamed_faces);
        assert!(restored.show_hidden_faces);
    }

    #[test]
    fn video_playback_settings_default_when_missing_from_json() {
        let mut json = serde_json::to_value(ConfigData::default()).expect("serialize");
        let object = json.as_object_mut().expect("object");
        object.remove("video_volume");
        object.remove("video_muted");
        let restored: ConfigData = serde_json::from_value(json).expect("deserialize");
        assert_eq!(restored.video_volume, 1.0);
        assert!(!restored.video_muted);
    }

    #[test]
    fn face_visibility_flags_default_when_absent_in_json() {
        // Older config files written before the flags existed must still load.
        let json = serde_json::to_string(&serde_json::json!({})).unwrap();
        let restored: ConfigData = serde_json::from_str(&json).expect("deserialize legacy config");
        assert!(restored.show_unnamed_faces);
        assert!(!restored.show_hidden_faces);
    }

    #[test]
    fn test_api_key_file_fallback() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let config_file = dir.path().join("config.json");

        let secret_key = "test-secret-key-123";

        // Initially, the key should not exist.
        assert_eq!(read_api_key_file_fallback(&config_file), None);

        // Write the key.
        assert!(write_api_key_file_fallback(&config_file, secret_key).is_ok());

        // Read the key and verify base64 decodes back to original.
        assert_eq!(
            read_api_key_file_fallback(&config_file),
            Some(secret_key.to_string())
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let fallback_path = api_key_fallback_path(&config_file);
            assert_eq!(
                fs::metadata(fallback_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        // Removing the fallback file should delete it.
        remove_api_key_file_fallback(&config_file);
        assert_eq!(read_api_key_file_fallback(&config_file), None);
    }

    #[test]
    fn test_pick_api_key_prefers_file_without_touching_keyring() {
        let key = pick_api_key(Some("file-key".into()), || {
            panic!("keyring must not be consulted when the file has a key")
        });
        assert_eq!(key.as_deref(), Some("file-key"));
    }

    #[test]
    fn test_pick_api_key_uses_keyring_when_no_file() {
        let key = pick_api_key(None, || Some("keyring-key".into()));
        assert_eq!(key.as_deref(), Some("keyring-key"));
    }

    #[cfg(unix)]
    #[test]
    fn test_api_key_file_fallback_tightens_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let config_file = dir.path().join("config.json");
        let path = api_key_fallback_path(&config_file);
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        write_api_key_file_fallback(&config_file, "new-key").unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            read_api_key_file_fallback(&config_file).as_deref(),
            Some("new-key")
        );
    }

    #[test]
    fn test_keyring_timeout_message_points_to_setup_guide() {
        let message = keyring_error_message(&KeyringError::Timeout("Keyring connection"));
        assert!(message.contains("did not respond"));
        assert!(message.contains("wiki/Keyring-Setup"));
    }

    #[test]
    fn renaming_an_album_updates_linked_watch_paths() {
        let entry = |id: Option<&str>, name: &str| WatchPathEntry::WithConfig {
            path: "/p".into(),
            album_id: id.map(str::to_string),
            album_name: Some(name.into()),
            rules: FolderRules::default(),
        };
        let mut entries = vec![
            entry(Some("a1"), "Trips"),
            entry(None, "Trips"),
            entry(Some("a2"), "Trips"),
            WatchPathEntry::Simple("/q".into()),
        ];
        assert!(rename_album_in_watch_paths(
            &mut entries,
            "a1",
            "Trips",
            "Holidays"
        ));
        let names: Vec<Option<&str>> = entries.iter().map(WatchPathEntry::album_name).collect();
        assert_eq!(
            names,
            [Some("Holidays"), Some("Holidays"), Some("Trips"), None]
        );
        assert!(!rename_album_in_watch_paths(
            &mut entries,
            "zz",
            "Nope",
            "X"
        ));
    }
}
