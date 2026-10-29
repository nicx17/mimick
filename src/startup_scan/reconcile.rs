use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app_context::AppContext;
use crate::config::{FolderSyncMethod, WatchPathEntry};
use crate::library::album_sync;

use super::configured_album_id;

/// Parallel reconcile for each watch path entry against their remote albums.
pub(super) async fn sync_album_to_folder_entries(
    watch_paths: &[WatchPathEntry],
    app_ctx: Arc<AppContext>,
) {
    for entry in watch_paths {
        reconcile_entry(app_ctx.clone(), entry).await;
    }
}

/// Reconcile a single watch entry against its remote album: download new
/// items, trash local files removed from the album, and trash remote items
/// missing locally — gated by the entry's per-folder rules.
pub async fn reconcile_entry(app_ctx: Arc<AppContext>, entry: &WatchPathEntry) {
    if entry.uploads_to_library() {
        return;
    }

    let rules = entry.rules();
    let download_enabled = rules.sync_method != FolderSyncMethod::UploadOnly;
    if !download_enabled && !rules.delete_album_to_folder && !rules.delete_folder_to_album {
        return;
    }

    let watch_path = Path::new(entry.path()).to_path_buf();
    if !watch_path.is_dir() {
        return;
    }

    let album_name = entry
        .album_name()
        .map(ToString::to_string)
        .or_else(|| {
            watch_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "Mimick".to_string());

    let album_id = resolve_entry_album_id(&app_ctx, entry, &album_name).await;
    let Some(album_id) = album_id else {
        return;
    };
    reconcile_album_folder(app_ctx, album_name, album_id, watch_path, rules).await;
}

async fn resolve_entry_album_id(
    app_ctx: &AppContext,
    entry: &WatchPathEntry,
    album_name: &str,
) -> Option<String> {
    if let Some(id) = configured_album_id(entry) {
        return Some(id);
    }
    match app_ctx.api_client.get_album_id_if_exists(album_name).await {
        Ok(id) => id,
        Err(err) => {
            log::warn!(
                "Album-to-folder sync could not resolve album '{}': {}",
                album_name,
                err
            );
            None
        }
    }
}

async fn reconcile_album_folder(
    app_ctx: Arc<AppContext>,
    album_name: String,
    album_id: String,
    watch_path: PathBuf,
    rules: crate::config::FolderRules,
) {
    // Per-album lock: drop the reconcile if another instance (periodic or
    // manual) is already running for this album.
    let _guard = match app_ctx.reconcile_locks.try_acquire(album_id.clone()) {
        Some(g) => g,
        None => {
            log::debug!(
                "Reconcile already in progress for album {}, skipping",
                album_id
            );
            return;
        }
    };

    let diff = match album_sync::diff_album_vs_folder(
        app_ctx.clone(),
        &album_id,
        &watch_path,
        &rules,
        false,
    )
    .await
    {
        Ok(diff) => diff,
        Err(err) => {
            log::warn!(
                "Album-to-folder sync diff failed for '{}': {}",
                album_name,
                err
            );
            return;
        }
    };

    execute_reconcile_diff(app_ctx.clone(), &album_name, &album_id, watch_path, diff).await;
}

async fn execute_reconcile_diff(
    app_ctx: Arc<AppContext>,
    album_name: &str,
    album_id: &str,
    watch_path: std::path::PathBuf,
    diff: crate::library::album_sync::AlbumDiff,
) {
    if !diff.to_download.is_empty() {
        let (downloaded, failed) = album_sync::execute_downloads(
            app_ctx.clone(),
            watch_path.clone(),
            Some(album_id.to_string()),
            Some(album_name.to_string()),
            diff.to_download,
        )
        .await;
        log::info!(
            "Album-to-folder sync for '{}' downloaded {} item(s), {} failure(s)",
            album_name,
            downloaded,
            failed
        );
    }

    if !diff.to_delete_local.is_empty() {
        let (trashed, failed) =
            album_sync::execute_local_deletions(app_ctx.clone(), diff.to_delete_local).await;
        log::info!(
            "Album-to-folder deletion sync for '{}' moved {} local item(s) to trash, {} failure(s)",
            album_name,
            trashed,
            failed
        );
    }

    if !diff.to_delete_remote.is_empty() {
        let count = diff.to_delete_remote.len();
        let trashed =
            album_sync::execute_remote_deletions(app_ctx.clone(), album_id, diff.to_delete_remote)
                .await;
        log::info!(
            "Folder-to-album deletion sync for '{}' moved {} of {} remote item(s) to Immich trash",
            album_name,
            trashed,
            count
        );
    }
}
