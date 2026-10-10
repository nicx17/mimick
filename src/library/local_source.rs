//! Local file enumeration for the library view.
//!
//! Walks the user's currently configured watch paths, applies the same
//! filtering rules used by the sync engine (`FolderRules` + supported-media
//! extensions), and produces a list of `LocalAsset` rows that the library
//! grid can display alongside (or instead of) remote Immich assets.
//!
//! Unlike the sync path, this module does NOT compute checksums on enumeration
//! — that would be too expensive for browsing. Sync state is matched by
//! looking up `SyncIndex.stored_checksum(path)` for paths the engine has
//! already hashed; assets the user hasn't synced yet show as "Local only".

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, SecondsFormat, Utc};

use crate::app_context::AppContext;
use crate::config::WatchPathEntry;
use crate::monitor::{is_supported_media_path, is_temporary_file};
// `crate::sync_index::SyncIndex` is referenced only in `local_sync_state`'s
// signature and is imported there inline to keep the public surface narrow.

/// A single file enumerated from the user's watched folders.
#[derive(Clone, Debug)]
pub struct LocalAsset {
    /// Absolute path on disk (used as the synthetic row id).
    pub path: PathBuf,
    /// Display name (file_name component).
    pub filename: String,
    /// MIME type, derived from the extension.
    pub mime: String,
    /// "IMAGE" or "VIDEO".
    pub asset_type: &'static str,
    /// ISO-8601 modification time, used as `created_at` for sort consistency
    /// with remote `LibraryAsset`.
    pub created_at: String,
}

/// Walk every configured watch path and return matching media files.
///
/// Runs the synchronous walk on a Tokio blocking thread so the UI thread
/// stays responsive even on libraries with tens of thousands of files.
pub async fn enumerate_local(ctx: Arc<AppContext>) -> Vec<LocalAsset> {
    let watch_paths = ctx.live_watch_paths.lock().clone();

    tokio::task::spawn_blocking(move || enumerate_blocking(&watch_paths))
        .await
        .unwrap_or_default()
}

/// Enumerate only the watch entries matching `entry_path`. Used by the
/// album-scoped Local/Unified views so a linked album's view stays bounded
/// to its own folder instead of spilling assets from sibling albums.
pub async fn enumerate_local_for_entry(
    ctx: Arc<AppContext>,
    entry_path: String,
) -> Vec<LocalAsset> {
    let watch_paths = ctx.live_watch_paths.lock().clone();

    let scoped: Vec<WatchPathEntry> = watch_paths
        .into_iter()
        .filter(|e| e.path() == entry_path)
        .collect();

    tokio::task::spawn_blocking(move || enumerate_blocking(&scoped))
        .await
        .unwrap_or_default()
}

/// Walk directories on a thread pool thread to search for media files.
fn enumerate_blocking(watch_paths: &[WatchPathEntry]) -> Vec<LocalAsset> {
    let mut out = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for entry in watch_paths {
        enumerate_entry(entry, &mut seen, &mut out);
    }
    out
}

/// Traverse one configured root, adding each eligible file once across roots.
fn enumerate_entry(entry: &WatchPathEntry, seen: &mut HashSet<PathBuf>, out: &mut Vec<LocalAsset>) {
    let root = PathBuf::from(entry.path());
    if !root.is_dir() {
        return;
    }
    let rules = entry.rules();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for child in read_dir.flatten() {
            let path = child.path();
            if path.is_dir() {
                stack.push(path);
            } else if is_eligible_path(&path, &rules) {
                let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if seen.insert(key)
                    && let Some(asset) = build_asset(&path)
                {
                    out.push(asset);
                }
            }
        }
    }
}

fn is_eligible_path(path: &Path, rules: &crate::config::FolderRules) -> bool {
    is_supported_media_path(path) && !is_temporary_file(path) && rules.matches(path)
}

/// Parse metadata details and construct a typed `LocalAsset` for a file path.
fn build_asset(path: &Path) -> Option<LocalAsset> {
    let filename = path.file_name()?.to_string_lossy().into_owned();
    let metadata = std::fs::metadata(path).ok()?;
    let created_at = format_modified(&metadata);
    let mime = crate::media_kinds::mime_for_path(path);
    let asset_type = match crate::media_kinds::asset_kind(mime) {
        crate::media_kinds::AssetKind::Video => "VIDEO",
        _ => "IMAGE",
    };
    Some(LocalAsset {
        path: path.to_path_buf(),
        filename,
        mime: mime.into(),
        asset_type,
        created_at,
    })
}

/// Convert a file modification timestamp into standard ISO-8601 formatting.
fn format_modified(meta: &std::fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .or_else(|_| meta.created())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let datetime: DateTime<Utc> = mtime.into();
    datetime.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Apply a case-insensitive filename substring filter, matching the spec's
/// "Local Search: file name based" requirement.
pub fn filter_by_filename(items: Vec<LocalAsset>, query: &str) -> Vec<LocalAsset> {
    let needle = query.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|a| a.filename.to_ascii_lowercase().contains(&needle))
        .collect()
}

/// Decide the sync-state badge for a local asset.
///
/// Takes a borrowed `&SyncIndex` rather than `&AppContext` because callers
/// like `asset_objects_from_state` already hold the `sync_index` mutex
/// guard while iterating assets. Re-acquiring it inside this function
/// would deadlock against the outer guard (`parking_lot::Mutex` is
/// non-reentrant). Returns 2 when the file's path is recorded as synced
/// (badge "Both"), 1 otherwise (LocalOnly).
pub fn local_sync_state(idx: &crate::sync_index::ShardedSyncIndex, path: &Path) -> u32 {
    if idx.stored_checksum(&path.display().to_string()).is_some() {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn make(name: &str) -> LocalAsset {
        LocalAsset {
            path: PathBuf::from(name),
            filename: name.into(),
            mime: "image/jpeg".into(),
            asset_type: "IMAGE",
            created_at: String::new(),
        }
    }

    /// Synthetic identity used by the GridView model. Using the path keeps
    /// dedup logic stable across enumerations even when modification times
    /// change.
    fn synthetic_id(asset: &LocalAsset) -> String {
        format!("local::{}", asset.path.display())
    }

    #[test]
    fn filter_by_filename_is_case_insensitive_substring() {
        let items = vec![make("Beach.JPG"), make("city.jpg"), make("forest.png")];
        let filtered = filter_by_filename(items, "JpG");
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn filter_by_filename_empty_query_returns_all() {
        let items = vec![make("a.jpg"), make("b.jpg")];
        assert_eq!(filter_by_filename(items, "  ").len(), 2);
    }

    #[test]
    fn synthetic_id_is_stable_across_clones() {
        let a = make("/tmp/a.jpg");
        assert_eq!(synthetic_id(&a), synthetic_id(&a.clone()));
    }

    #[test]
    fn enumeration_filters_non_media_and_temporary_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("photo.jpg"), b"jpg").unwrap();
        fs::write(dir.path().join("unfinished.jpg.part"), b"partial").unwrap();
        fs::write(dir.path().join("notes.txt"), b"text").unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        fs::write(dir.path().join("nested/video.mp4"), b"video").unwrap();

        let entries = vec![WatchPathEntry::Simple(
            dir.path().to_string_lossy().into_owned(),
        )];
        let assets = enumerate_blocking(&entries);
        let mut names: Vec<_> = assets.into_iter().map(|asset| asset.filename).collect();
        names.sort();

        assert_eq!(names, ["photo.jpg", "video.mp4"]);
    }
}
