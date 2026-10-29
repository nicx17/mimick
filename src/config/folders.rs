use serde::{Deserialize, Serialize};
use std::path::Path;

/// Defines per-folder filters and guardrails applied before a file is queued for upload.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderRules {
    /// True if hidden files/folders should be ignored during sync.
    #[serde(default)]
    pub ignore_hidden: bool,
    /// Maximum file size allowed for upload, in Megabytes.
    #[serde(default)]
    pub max_file_size_mb: Option<u64>,
    /// List of file extensions permitted for sync.
    #[serde(default)]
    pub allowed_extensions: Vec<String>,
    /// Selected synchronization direction method.
    #[serde(default)]
    pub sync_method: FolderSyncMethod,
    /// Override startup catch-up mode, or None to use global setting.
    #[serde(default)]
    pub startup_catchup_mode: Option<StartupCatchupMode>,
    /// Delete local file when its corresponding remote asset is removed.
    #[serde(default)]
    pub delete_folder_to_album: bool,
    /// Delete remote asset when its corresponding local file is removed.
    #[serde(default)]
    pub delete_album_to_folder: bool,
    /// Per-folder override for XMP sidecar upload. `None` inherits the global
    /// `upload_xmp_sidecars` setting; `Some(true/false)` overrides it.
    #[serde(default)]
    pub include_xmp_sidecar: Option<bool>,
}

impl FolderRules {
    /// Restrict this rule set to operations that are safe without an album target.
    ///
    /// A library-only watch folder has no remote collection to reconcile, so it
    /// can upload files but cannot safely mirror downloads or deletions.
    pub fn restrict_to_library_uploads(&mut self) {
        self.sync_method = FolderSyncMethod::UploadOnly;
        self.delete_folder_to_album = false;
        self.delete_album_to_folder = false;
    }

    /// Return the list of allowed extensions trimmed and lowercased.
    pub fn normalized_extensions(&self) -> Vec<String> {
        self.allowed_extensions
            .iter()
            .map(|ext| ext.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|ext| !ext.is_empty())
            .collect()
    }

    /// Check if a specific file path meets all active folder validation rules.
    pub fn matches(&self, path: &Path) -> bool {
        if self.ignore_hidden
            && path.components().any(|component| {
                component
                    .as_os_str()
                    .to_str()
                    .map(|part| part.starts_with('.') && part.len() > 1)
                    .unwrap_or(false)
            })
        {
            return false;
        }

        if let Some(limit_mb) = self.max_file_size_mb
            && let Ok(metadata) = std::fs::metadata(path)
            && metadata.len() > limit_mb.saturating_mul(1024 * 1024)
        {
            return false;
        }

        let normalized = self.normalized_extensions();
        if !normalized.is_empty() {
            let ext = path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .map(str::to_ascii_lowercase);
            if ext
                .as_deref()
                .is_none_or(|ext| !normalized.iter().any(|allowed| allowed == ext))
            {
                return false;
            }
        }

        true
    }

    /// Resolve whether XMP sidecar upload is enabled for this folder.
    ///
    /// Returns the per-folder override when set, otherwise falls back to the
    /// caller-supplied global default.
    pub fn xmp_sidecar_enabled(&self, global_default: bool) -> bool {
        self.include_xmp_sidecar.unwrap_or(global_default)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub enum FolderSyncMethod {
    /// Upload folder-only assets and download album-only assets.
    Full,
    /// Only upload assets found in the folder.
    #[default]
    UploadOnly,
    /// Only download assets found in the album.
    DownloadOnly,
}

/// A watch path entry stored in config.
///
/// Older configs may contain plain strings, while newer entries can also store per-folder
/// album targeting metadata.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum WatchPathEntry {
    /// Legacy, simple watched directory path without specific rules or targets.
    Simple(String),
    /// Configured watched directory path containing custom sync targets and filters.
    WithConfig {
        /// Absolute directory path on the local filesystem.
        path: String,
        /// Target Immich album unique identifier.
        #[serde(default)]
        album_id: Option<String>,
        /// Target Immich album name.
        #[serde(default)]
        album_name: Option<String>,
        /// Specific validation and sync rules applied to this path.
        #[serde(default)]
        rules: FolderRules,
    },
}

impl WatchPathEntry {
    /// Retrieve the base path of the watched directory.
    pub fn path(&self) -> &str {
        match self {
            WatchPathEntry::Simple(p) => p,
            WatchPathEntry::WithConfig { path, .. } => path,
        }
    }
    /// Retrieve the target album name, if any.
    pub fn album_name(&self) -> Option<&str> {
        match self {
            WatchPathEntry::Simple(_) => None,
            WatchPathEntry::WithConfig { album_name, .. } => album_name.as_deref(),
        }
    }

    /// Whether this configured folder explicitly uploads directly to the
    /// Immich library instead of targeting an album.
    ///
    /// Legacy simple paths intentionally return false: they preserve their
    /// established folder-name-as-album behaviour.
    pub fn uploads_to_library(&self) -> bool {
        matches!(
            self,
            WatchPathEntry::WithConfig {
                album_id: None,
                album_name: None,
                ..
            }
        )
    }

    /// Retrieve the folder rules or a default set for simple paths.
    pub fn rules(&self) -> FolderRules {
        match self {
            WatchPathEntry::Simple(_) => FolderRules::default(),
            WatchPathEntry::WithConfig { rules, .. } => rules.clone(),
        }
    }

    /// Retrieve the sync direction configured for this directory.
    pub fn sync_method(&self) -> FolderSyncMethod {
        self.rules().sync_method
    }

    /// Retrieve the catchup strategy override or fallback to global configuration.
    pub fn startup_catchup_mode(&self, fallback: &StartupCatchupMode) -> StartupCatchupMode {
        self.rules()
            .startup_catchup_mode
            .unwrap_or_else(|| fallback.clone())
    }
}

/// Find the most specific configured watch entry that contains `path`.
///
/// Matching is path-aware rather than string-prefix-based so sibling paths like
/// `/home/user/Pictures` and `/home/user/Pictures-backup` are treated correctly.
pub fn best_matching_watch_entry<'a>(
    path: &Path,
    entries: &'a [WatchPathEntry],
) -> Option<&'a WatchPathEntry> {
    entries
        .iter()
        .filter(|entry| path.starts_with(Path::new(entry.path())))
        .max_by_key(|entry| entry.path().len())
}

/// Find a configured watch path entry by its target album name.
pub fn watch_entry_for_album<'a>(
    album_name: &str,
    entries: &'a [WatchPathEntry],
) -> Option<&'a WatchPathEntry> {
    entries
        .iter()
        .find(|entry| entry.album_name() == Some(album_name))
}

pub fn rename_album_in_watch_paths(
    entries: &mut [WatchPathEntry],
    album_id: &str,
    old_name: &str,
    new_name: &str,
) -> bool {
    let mut changed = false;
    for entry in entries.iter_mut() {
        if let WatchPathEntry::WithConfig {
            album_id: id,
            album_name: Some(name),
            ..
        } = entry
            && (id.as_deref() == Some(album_id) || (id.is_none() && name == old_name))
        {
            *name = new_name.to_string();
            changed = true;
        }
    }
    changed
}

/// Album choice that names each file's album after its parent folder.
const FOLDER_NAME_ALBUM: &str = "Default (Folder Name)";

/// Watch folders that keep an album alive: re-created by name if deleted while they upload to it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AlbumFolders {
    /// Watch folders that target the album, or whose own name is the album.
    pub linked: Vec<String>,
    /// Subfolders of folder-name watch folders whose photos went into the album.
    pub named_after: Vec<String>,
}

impl AlbumFolders {
    pub fn is_empty(&self) -> bool {
        self.linked.is_empty() && self.named_after.is_empty()
    }
}

/// Folders tied to an album. `album_name` is `None` when another album shares the name,
/// so name-only matches can't tell them apart; `synced_dirs` are folders of files Mimick
/// uploaded into the album.
pub fn album_folders(
    entries: &[WatchPathEntry],
    album_id: &str,
    album_name: Option<&str>,
    synced_dirs: &[String],
) -> AlbumFolders {
    let mut linked: Vec<String> = entries
        .iter()
        .filter(|entry| {
            targets_album(entry, album_id, album_name) || is_named_root(entry, album_name)
        })
        .map(|entry| entry.path().to_string())
        .collect();
    let mut named_after: Vec<String> = Vec::new();
    for dir in synced_dirs {
        let Some(entry) = best_matching_watch_entry(Path::new(dir), entries) else {
            continue;
        };
        if !names_albums_after_folders(entry) {
            continue;
        }
        let list = if entry.path() == dir {
            &mut linked
        } else {
            &mut named_after
        };
        if !list.contains(dir) {
            list.push(dir.clone());
        }
    }
    named_after.sort();
    AlbumFolders {
        linked,
        named_after,
    }
}

fn targets_album(entry: &WatchPathEntry, album_id: &str, album_name: Option<&str>) -> bool {
    match entry {
        WatchPathEntry::WithConfig {
            album_id: Some(id), ..
        } => id == album_id,
        WatchPathEntry::WithConfig {
            album_id: None,
            album_name: Some(name),
            ..
        } => name != FOLDER_NAME_ALBUM && Some(name.as_str()) == album_name,
        _ => false,
    }
}

fn names_albums_after_folders(entry: &WatchPathEntry) -> bool {
    match entry {
        WatchPathEntry::Simple(_) => true,
        WatchPathEntry::WithConfig {
            album_id: None,
            album_name: Some(name),
            ..
        } => name == FOLDER_NAME_ALBUM,
        _ => false,
    }
}

/// A folder-name watch folder whose own files go into an album named after it.
fn is_named_root(entry: &WatchPathEntry, album_name: Option<&str>) -> bool {
    let folder = Path::new(entry.path())
        .file_name()
        .and_then(std::ffi::OsStr::to_str);
    names_albums_after_folders(entry) && album_name.is_some() && folder == album_name
}

/// Stop `folders` from feeding the album: linked folders upload to the library only, or are
/// no longer watched; subfolders get a library-only entry of their own.
pub fn detach_album_from_watch_paths(
    entries: &mut Vec<WatchPathEntry>,
    folders: &AlbumFolders,
    keep_watching: bool,
) -> bool {
    let is_linked = |entry: &WatchPathEntry| folders.linked.iter().any(|path| path == entry.path());
    let before = entries.len();
    let mut changed = false;
    if keep_watching {
        for entry in entries.iter_mut().filter(|entry| is_linked(entry)) {
            *entry = library_only(entry.path(), entry.rules());
            changed = true;
        }
    } else {
        entries.retain(|entry| !is_linked(entry));
        changed = entries.len() != before;
    }
    for dir in &folders.named_after {
        if entries.iter().any(|entry| entry.path() == dir) {
            continue;
        }
        let rules = best_matching_watch_entry(Path::new(dir), entries)
            .map(WatchPathEntry::rules)
            .unwrap_or_default();
        entries.push(library_only(dir, rules));
        changed = true;
    }
    changed
}

fn library_only(path: &str, rules: FolderRules) -> WatchPathEntry {
    WatchPathEntry::WithConfig {
        path: path.to_string(),
        album_id: None,
        album_name: None,
        rules: FolderRules {
            sync_method: FolderSyncMethod::UploadOnly,
            delete_folder_to_album: false,
            delete_album_to_folder: false,
            ..rules
        },
    }
}

/// Catch-up strategy applied to existing files when the application boots.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub enum StartupCatchupMode {
    /// Perform full, deep comparison of all files on the local filesystem.
    #[default]
    Full,
    /// Fast sync covering only files modified or added in the last 24 hours.
    RecentOnly,
    /// Sync only new files added to folders while daemon was offline.
    NewFilesOnly,
}
