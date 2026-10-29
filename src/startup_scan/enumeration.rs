use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::api_client::ImmichApiClient;
use crate::config::{FolderSyncMethod, StartupCatchupMode, WatchPathEntry};
use crate::monitor::{compute_sha1_chunked, is_supported_media_path, is_temporary_file};
use crate::queue_manager::FileTask;
use crate::state_manager::AppState;
use crate::sync_index::{ShardedSyncIndex, SyncTarget};
use futures_util::stream::{self, StreamExt};
use parking_lot::Mutex;
use rayon::prelude::*;

use super::ScanCandidate;

pub(super) fn enumerate_candidates(
    watch_paths: &[WatchPathEntry],
    fallback_catchup_mode: StartupCatchupMode,
    last_sync: f64,
    shared_state: &Arc<Mutex<AppState>>,
    global_xmp_enabled: bool,
) -> (Vec<ScanCandidate>, HashSet<String>, usize, usize) {
    let skipped = AtomicUsize::new(0);
    let errors = AtomicUsize::new(0);
    let seen_paths = Mutex::new(HashSet::new());

    let candidates: Vec<ScanCandidate> = watch_paths
        .par_iter()
        .flat_map(|entry| {
            enumerate_entry_candidates(
                entry,
                &fallback_catchup_mode,
                last_sync,
                shared_state,
                global_xmp_enabled,
                &seen_paths,
                &skipped,
                &errors,
            )
        })
        .collect();

    (
        candidates,
        seen_paths.into_inner(),
        skipped.into_inner(),
        errors.into_inner(),
    )
}

#[allow(clippy::too_many_arguments)]
fn enumerate_entry_candidates(
    entry: &WatchPathEntry,
    fallback_catchup_mode: &StartupCatchupMode,
    last_sync: f64,
    shared_state: &Arc<Mutex<AppState>>,
    global_xmp_enabled: bool,
    seen_paths: &Mutex<HashSet<String>>,
    skipped: &AtomicUsize,
    errors: &AtomicUsize,
) -> Vec<ScanCandidate> {
    if entry.sync_method() == FolderSyncMethod::DownloadOnly {
        return Vec::new();
    }
    let watch_path = entry.path().to_string();
    let root = Path::new(&watch_path);
    if !root.exists() {
        mark_missing_watch_path(root, &watch_path, shared_state);
        return Vec::new();
    }

    let catchup_mode = entry.startup_catchup_mode(fallback_catchup_mode);
    let sidecar_enabled = entry.rules().xmp_sidecar_enabled(global_xmp_enabled);
    let ctx = ScanContext {
        entry,
        watch_path: &watch_path,
        catchup_mode: &catchup_mode,
        last_sync,
        sidecar_enabled,
        seen_paths,
        skipped,
        errors,
    };

    let mut candidates = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        scan_candidate_directory(&dir, &ctx, &mut stack, &mut candidates);
    }
    candidates
}

struct ScanContext<'a> {
    entry: &'a WatchPathEntry,
    watch_path: &'a str,
    catchup_mode: &'a StartupCatchupMode,
    last_sync: f64,
    sidecar_enabled: bool,
    seen_paths: &'a Mutex<HashSet<String>>,
    skipped: &'a AtomicUsize,
    errors: &'a AtomicUsize,
}

fn mark_missing_watch_path(root: &Path, watch_path: &str, shared_state: &Arc<Mutex<AppState>>) {
    log::warn!(
        "Startup scan skipped missing watch path: {}",
        root.display()
    );
    if let Some(mut state) = shared_state.try_lock() {
        let status = state
            .folder_statuses
            .entry(watch_path.to_string())
            .or_default();
        status.last_error = Some("Permission lost or folder missing".to_string());
    }
}

fn scan_candidate_directory(
    dir: &Path,
    ctx: &ScanContext,
    stack: &mut Vec<PathBuf>,
    candidates: &mut Vec<ScanCandidate>,
) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(iter) => iter,
        Err(err) => {
            ctx.errors.fetch_add(1, Ordering::Relaxed);
            log::warn!("Startup scan could not read '{}': {}", dir.display(), err);
            return;
        }
    };
    for child in read_dir {
        process_candidate_file(child, ctx, stack, candidates);
    }
}

fn process_candidate_file(
    child: std::io::Result<std::fs::DirEntry>,
    ctx: &ScanContext,
    stack: &mut Vec<PathBuf>,
    candidates: &mut Vec<ScanCandidate>,
) {
    let entry_fs = match child {
        Ok(entry_fs) => entry_fs,
        Err(err) => {
            ctx.errors.fetch_add(1, Ordering::Relaxed);
            log::warn!("Startup scan directory entry error: {}", err);
            return;
        }
    };
    let path = entry_fs.path();
    if path.is_dir() {
        stack.push(path);
        return;
    }
    if !is_eligible_startup_path(ctx.entry, &path) {
        return;
    }
    if should_skip_for_catchup(ctx.catchup_mode, &entry_fs, ctx.last_sync) {
        ctx.skipped.fetch_add(1, Ordering::Relaxed);
    } else {
        let path_str = path.to_string_lossy().into_owned();
        ctx.seen_paths.lock().insert(path_str.clone());
        candidates.push(ScanCandidate {
            path: path_str,
            watch_path: ctx.watch_path.to_string(),
            album_name: effective_album_name(ctx.entry, &path),
            sidecar_enabled: ctx.sidecar_enabled,
        });
    }
}

fn is_eligible_startup_path(entry: &WatchPathEntry, path: &Path) -> bool {
    is_supported_media_path(path) && !is_temporary_file(path) && entry.rules().matches(path)
}

fn should_skip_for_catchup(
    mode: &StartupCatchupMode,
    entry_fs: &std::fs::DirEntry,
    last_sync: f64,
) -> bool {
    let meta = match entry_fs.metadata() {
        Ok(m) => m,
        Err(_) => return false,
    };

    match mode {
        StartupCatchupMode::RecentOnly => {
            let Ok(modified) = meta.modified() else {
                return false;
            };
            let Ok(duration) = std::time::SystemTime::now().duration_since(modified) else {
                return false;
            };
            duration.as_secs() > 7 * 86400
        }
        StartupCatchupMode::NewFilesOnly => {
            let Ok(created) = meta.created().or_else(|_| meta.modified()) else {
                return false;
            };
            let created_secs = created
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            created_secs < last_sync
        }
        _ => false,
    }
}

/// Hash a candidate file and submit it to the upload queue.
pub(super) async fn hash_to_task(
    path: String,
    watch_path: String,
    album_id: Option<String>,
    album_name: Option<String>,
    reassociate_only: bool,
    checksum: Option<String>,
    sidecar_enabled: bool,
) -> Result<FileTask, ()> {
    let checksum = if let Some(checksum) = checksum {
        checksum
    } else {
        let path_for_hash = path.clone();
        match tokio::task::spawn_blocking(move || compute_sha1_chunked(&path_for_hash)).await {
            Ok(Ok(checksum)) => checksum,
            Ok(Err(err)) => {
                log::warn!("Startup scan could not checksum '{}': {}", path, err);
                return Err(());
            }
            Err(err) => {
                log::warn!("Startup scan checksum task failed for '{}': {}", path, err);
                return Err(());
            }
        }
    };

    let sidecar_path = if sidecar_enabled {
        crate::sidecar::find_sidecar(std::path::Path::new(&path))
            .map(|p| p.to_string_lossy().into_owned())
    } else {
        None
    };
    let skip_album = album_name.is_none();

    Ok(FileTask {
        path,
        watch_path,
        checksum,
        album_id,
        album_name,
        reassociate_only,
        skip_album,
        sidecar_path,
    })
}

/// Resolve the effective album name for a file using per-folder configuration.
pub(super) fn effective_album_name(entry: &WatchPathEntry, path: &Path) -> Option<String> {
    if entry.uploads_to_library() {
        return None;
    }

    match entry.album_name() {
        Some(name) if !name.is_empty() && name != "Default (Folder Name)" => Some(name.to_string()),
        _ => path
            .parent()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().to_string())
            .or_else(|| Some("Mimick".to_string())),
    }
}

/// Resolve an album ID, creating the album if it doesn't exist yet.
/// Caches the result for reuse by other candidates in the same folder.
pub(super) async fn resolve_album(
    api_client: &ImmichApiClient,
    album_name: &str,
    cache: &Arc<Mutex<HashMap<String, Option<String>>>>,
) -> Result<Option<String>, String> {
    // Fast path: already resolved.
    if let Some(Some(cached)) = cache.lock().get(album_name) {
        return Ok(Some(cached.clone()));
    }

    let resolved = api_client.resolve_album_by_name(album_name, false).await?;
    cache
        .lock()
        .insert(album_name.to_string(), resolved.clone());
    Ok(resolved)
}

pub(super) async fn resolve_album_ids_for_candidates(
    candidates: &[ScanCandidate],
    api_client: &Arc<ImmichApiClient>,
) -> Arc<Mutex<HashMap<String, Option<String>>>> {
    let unique_albums: Vec<String> = candidates
        .iter()
        .filter_map(|c| c.album_name.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let album_id_cache: Arc<Mutex<HashMap<String, Option<String>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let cache = album_id_cache.clone();
    stream::iter(unique_albums)
        .for_each_concurrent(8, |name| {
            let api = api_client.clone();
            let cache = cache.clone();
            async move {
                match api.get_album_id_if_exists(&name).await {
                    Ok(id) => {
                        cache.lock().insert(name, id);
                    }
                    Err(err) => {
                        log::warn!("Startup scan: album lookup failed for '{}': {}", name, err);
                    }
                }
            }
        })
        .await;

    album_id_cache
}

pub(super) async fn filter_and_reassociate_existing(
    prepared_tasks: Vec<FileTask>,
    api_client: &Arc<ImmichApiClient>,
    sync_index: &Arc<ShardedSyncIndex>,
) -> (Vec<FileTask>, usize) {
    let unique_checksums: Vec<String> = prepared_tasks
        .iter()
        .map(|t| t.checksum.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let existing_on_server = if unique_checksums.is_empty() {
        HashMap::new()
    } else {
        api_client.bulk_existing_asset_ids(&unique_checksums).await
    };

    let mut to_reassociate: Vec<(FileTask, String)> = Vec::new();
    let mut to_upload: Vec<FileTask> = Vec::new();
    for task in prepared_tasks {
        match existing_on_server.get(&task.checksum) {
            Some(asset_id) => to_reassociate.push((task, asset_id.clone())),
            None => to_upload.push(task),
        }
    }

    let reassociated = Arc::new(AtomicUsize::new(0));
    process_reassociation(api_client, sync_index, to_reassociate, reassociated.clone()).await;

    (to_upload, reassociated.load(Ordering::Relaxed))
}

async fn process_reassociation(
    api_client: &Arc<ImmichApiClient>,
    sync_index: &Arc<ShardedSyncIndex>,
    to_reassociate: Vec<(FileTask, String)>,
    reassociated: Arc<AtomicUsize>,
) {
    stream::iter(to_reassociate)
        .for_each_concurrent(8, |(task, asset_id)| {
            let api = api_client.clone();
            let sync_index = sync_index.clone();
            let reassociated = reassociated.clone();
            async move {
                if let Some(ref album_id) = task.album_id
                    && !album_id.is_empty()
                {
                    let _ = api
                        .add_assets_to_album(album_id, std::slice::from_ref(&asset_id))
                        .await;
                }
                let target = SyncTarget {
                    album_name: task.album_name.clone(),
                    album_id: task.album_id.clone(),
                };
                if let Err(err) = sync_index.record_synced(&task.path, &task.checksum, &target) {
                    log::warn!(
                        "Could not record sync index for pre-existing asset '{}': {}",
                        task.path,
                        err
                    );
                }
                reassociated.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;
}
