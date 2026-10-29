//! Performs a startup catch-up scan for files that were missed while Mimick was not running.
//!
//! The scan runs in two stages:
//!   1. **Enumerate (parallel, sync)** -- uses `rayon` to walk all watch paths
//!      concurrently, collecting candidate files with their filesystem fingerprints.
//!   2. **Decide + queue (bounded async)** -- resolves album IDs, checks the sync
//!      index, hashes only when needed, and queues uploads with bounded concurrency.

use crate::api_client::ImmichApiClient;
use crate::app_context::AppContext;
use crate::config::StartupCatchupMode;
use crate::config::WatchPathEntry;
use crate::queue_manager::{FileTask, QueueManager};
use crate::state_manager::AppState;

mod reconcile;
use crate::sync_index::{ShardedSyncIndex, SyncDecision, SyncTarget};
use futures_util::stream::{self, StreamExt};
use parking_lot::Mutex;
pub use reconcile::reconcile_entry;
mod enumeration;
#[cfg(test)]
use enumeration::effective_album_name;
use enumeration::{
    enumerate_candidates, filter_and_reassociate_existing, hash_to_task, resolve_album,
    resolve_album_ids_for_candidates,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A file discovered during the parallel enumeration stage.
struct ScanCandidate {
    /// Absolute local path of the discovered candidate.
    path: String,
    /// Watch path entry prefix that matched the candidate.
    watch_path: String,
    /// Inferred or configured album name. `None` means upload directly to the library.
    album_name: Option<String>,
    /// Whether XMP sidecar attachment is enabled for this candidate.
    sidecar_enabled: bool,
}

/// Scans watch folders at startup and queues new, changed, or retargeted files for upload.
///
/// Stage 1 parallelises the directory walk and filtering via rayon.
/// Stage 2 resolves album IDs and processes candidates with bounded async concurrency.
pub async fn queue_unsynced_files(
    watch_paths: Vec<WatchPathEntry>,
    queue_manager: Arc<QueueManager>,
    sync_index: Arc<ShardedSyncIndex>,
    api_client: Arc<ImmichApiClient>,
    catchup_mode: StartupCatchupMode,
    shared_state: Arc<Mutex<AppState>>,
    app_ctx: Arc<AppContext>,
) {
    if watch_paths.is_empty() {
        return;
    }

    // ── Stage 1: Parallel enumerate + fingerprint ────────────────────────
    //
    // Read last_sync once, before spawning into rayon, to avoid locking
    // shared_state on every file.
    let last_sync = shared_state
        .lock()
        .last_successful_sync_at
        .unwrap_or_default();

    let global_xmp_enabled = app_ctx.config.read().data.upload_xmp_sidecars;

    let (candidates, mut seen_paths, skipped_enum, enum_errors) = enumerate_candidates(
        &watch_paths,
        catchup_mode,
        last_sync,
        &shared_state,
        global_xmp_enabled,
    );

    // ── Stage 2: Async decide + queue ────────────────────────────────────

    let album_id_cache = resolve_album_ids_for_candidates(&candidates, &api_client).await;

    // 2b. Per-candidate: sync_decision -> hash (if needed) -> collect FileTask.
    //     Tasks are NOT yet queued; we batch a server-side checksum check next
    //     so files already on Immich never enter the upload pipeline.
    let (prepared_tasks, skipped, errors) =
        prepare_candidates(candidates, &sync_index, &api_client, &album_id_cache).await;

    // 2c & 2d: Batch pre-flight existing check, split tasks, and inline reassociate hits.
    let (to_upload, reassociated) =
        filter_and_reassociate_existing(prepared_tasks, &api_client, &sync_index).await;
    let queued = queue_tasks(&queue_manager, to_upload).await;

    trash_remote_assets_for_missing_local_files(
        &watch_paths,
        &seen_paths,
        sync_index.clone(),
        api_client.clone(),
    )
    .await;

    reconcile::sync_album_to_folder_entries(&watch_paths, app_ctx).await;

    prune_index_entries_for_missing_files(&watch_paths, &mut seen_paths, &sync_index);

    log_scan_summary(
        queued,
        reassociated,
        skipped_enum + skipped,
        enum_errors + errors,
    );
}

type AlbumIdCache = Arc<Mutex<HashMap<String, Option<String>>>>;

/// What happened to one startup-scan candidate.
enum CandidateOutcome {
    Prepared(Box<FileTask>),
    /// Already synced to the same target.
    UpToDate,
    Failed,
}

/// Run `prepare_candidate` with bounded concurrency; returns the tasks plus skip/error counts.
async fn prepare_candidates(
    candidates: Vec<ScanCandidate>,
    sync_index: &Arc<ShardedSyncIndex>,
    api_client: &Arc<ImmichApiClient>,
    album_cache: &AlbumIdCache,
) -> (Vec<FileTask>, usize, usize) {
    let prepared = Mutex::new(Vec::new());
    let skipped = AtomicUsize::new(0);
    let errors = AtomicUsize::new(0);
    stream::iter(candidates)
        .for_each_concurrent(16, |candidate| async {
            match prepare_candidate(candidate, sync_index, api_client, album_cache).await {
                CandidateOutcome::Prepared(task) => prepared.lock().push(*task),
                CandidateOutcome::UpToDate => {
                    skipped.fetch_add(1, Ordering::Relaxed);
                }
                CandidateOutcome::Failed => {
                    errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        })
        .await;
    (
        prepared.into_inner(),
        skipped.into_inner(),
        errors.into_inner(),
    )
}

/// Decide whether a candidate needs work, resolve its album, and hash it into a `FileTask`.
async fn prepare_candidate(
    candidate: ScanCandidate,
    sync_index: &ShardedSyncIndex,
    api_client: &Arc<ImmichApiClient>,
    album_cache: &AlbumIdCache,
) -> CandidateOutcome {
    let album_name = candidate.album_name.clone();

    // Lookup existing album ID (no lock held across await).
    let existing_album_id = album_name
        .as_ref()
        .and_then(|name| album_cache.lock().get(name).cloned().flatten());
    let target = SyncTarget {
        album_name: album_name.clone(),
        album_id: existing_album_id,
    };

    let (reassociate_only, cached_checksum) =
        match candidate_sync_state(&candidate.path, sync_index, &target) {
            Ok(state) => state,
            Err(outcome) => return outcome,
        };

    let album_id = match album_name.as_deref() {
        Some(name) => match resolve_album(api_client, name, album_cache).await {
            Ok(id) => id,
            Err(err) => {
                log::warn!(
                    "Startup scan skipping '{}': album resolution failed: {}",
                    candidate.path,
                    err
                );
                return CandidateOutcome::Failed;
            }
        },
        None => None,
    };

    match hash_to_task(
        candidate.path,
        candidate.watch_path,
        album_id,
        album_name,
        reassociate_only,
        cached_checksum,
        candidate.sidecar_enabled,
    )
    .await
    {
        Ok(task) => CandidateOutcome::Prepared(Box::new(task)),
        Err(()) => CandidateOutcome::Failed,
    }
}

fn candidate_sync_state(
    path: &str,
    sync_index: &ShardedSyncIndex,
    target: &SyncTarget,
) -> Result<(bool, Option<String>), CandidateOutcome> {
    let decision = match sync_index.sync_decision(Path::new(path), target) {
        Ok(decision) => decision,
        Err(err) => {
            log::warn!("Startup scan could not inspect '{}': {}", path, err);
            return Err(CandidateOutcome::Failed);
        }
    };
    match decision {
        SyncDecision::UpToDate => Err(CandidateOutcome::UpToDate),
        SyncDecision::NeedsUpload => Ok((false, None)),
        SyncDecision::NeedsReassociate => Ok((true, sync_index.stored_checksum(path))),
    }
}

/// Queue each task; returns how many the queue accepted.
async fn queue_tasks(queue_manager: &QueueManager, tasks: Vec<FileTask>) -> usize {
    let mut queued = 0;
    for task in tasks {
        if queue_manager.add_to_queue(task).await {
            queued += 1;
        }
    }
    queued
}

fn log_scan_summary(queued: usize, reassociated: usize, skipped: usize, errors: usize) {
    if queued == 0 && reassociated == 0 {
        log::info!(
            "Startup scan complete: no unsynced files found ({} already current, {} error(s)).",
            skipped,
            errors
        );
        return;
    }
    log::info!(
        "Startup scan: queued={} reassociated={} skipped={} errors={}",
        queued,
        reassociated,
        skipped,
        errors
    );
}

/// Trash remote Immich assets when their corresponding local file has been removed.
async fn trash_remote_assets_for_missing_local_files(
    watch_paths: &[WatchPathEntry],
    seen_paths: &HashSet<String>,
    sync_index: Arc<ShardedSyncIndex>,
    api_client: Arc<ImmichApiClient>,
) {
    for entry in watch_paths {
        sync_missing_files_for_entry(entry, seen_paths, &sync_index, &api_client).await;
    }
}

async fn sync_missing_files_for_entry(
    entry: &WatchPathEntry,
    seen_paths: &HashSet<String>,
    sync_index: &ShardedSyncIndex,
    api_client: &ImmichApiClient,
) {
    if entry.uploads_to_library() || !entry.rules().delete_folder_to_album {
        return;
    }
    for (path, record) in sync_index.records_under_path(Path::new(entry.path())) {
        if seen_paths.contains(&path) {
            continue;
        }
        let album_name = missing_file_album_name(entry, &path, &record.album_name);
        let album_id = configured_album_id(entry).or(record.album_id);
        let album_id = match album_id {
            Some(id) => id,
            None => match api_client.get_album_id_if_exists(&album_name).await {
                Ok(Some(id)) => id,
                Ok(None) => continue,
                Err(err) => {
                    log::warn!(
                        "Startup deletion sync could not resolve album '{}': {}",
                        album_name,
                        err
                    );
                    continue;
                }
            },
        };
        trash_remote_asset_by_checksum(
            api_client,
            sync_index,
            &path,
            &album_id,
            &record.checksum,
            &album_name,
        )
        .await;
    }
}

fn missing_file_album_name(
    entry: &WatchPathEntry,
    path: &str,
    recorded_name: &Option<String>,
) -> String {
    entry
        .album_name()
        .map(ToString::to_string)
        .or(recorded_name.clone())
        .or_else(|| {
            Path::new(path)
                .parent()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "Mimick".to_string())
}

fn configured_album_id(entry: &WatchPathEntry) -> Option<String> {
    match entry {
        WatchPathEntry::WithConfig { album_id, .. } => album_id.clone(),
        WatchPathEntry::Simple(_) => None,
    }
}

fn prune_index_entries_for_missing_files(
    watch_paths: &[WatchPathEntry],
    seen_paths: &mut HashSet<String>,
    sync_index: &ShardedSyncIndex,
) {
    // Prune index entries for files that no longer exist. If a folder is
    // configured to mirror local deletions to the album, keep its missing
    // records so the next album sync can move the remote asset to trash.
    for entry in watch_paths {
        let rules = entry.rules();
        if entry.uploads_to_library() || !rules.delete_folder_to_album {
            continue;
        }
        let root = Path::new(entry.path());
        for (path, _) in sync_index.records_under_path(root) {
            seen_paths.insert(path);
        }
    }
    if let Err(err) = sync_index.prune_missing(seen_paths) {
        log::warn!("Failed to prune sync index after startup scan: {}", err);
    }
}

/// Locate and move a remote asset to Immich trash by looking up its checksum in an album.
async fn trash_remote_asset_by_checksum(
    api_client: &ImmichApiClient,
    sync_index: &ShardedSyncIndex,
    local_path: &str,
    album_id: &str,
    checksum: &str,
    album_name: &str,
) -> bool {
    match find_album_asset_id_by_checksum(api_client, album_id, checksum).await {
        Ok(Some((asset_id, filename))) => {
            trash_remote_asset(
                api_client, sync_index, local_path, album_name, asset_id, filename,
            )
            .await
        }
        Ok(None) => false,
        Err(err) => {
            log::warn!(
                "Startup deletion sync could not inspect album '{}': {}",
                album_name,
                err
            );
            false
        }
    }
}

async fn trash_remote_asset(
    api_client: &ImmichApiClient,
    sync_index: &ShardedSyncIndex,
    local_path: &str,
    album_name: &str,
    asset_id: String,
    filename: String,
) -> bool {
    match api_client.delete_assets(&[asset_id]).await {
        Ok(()) => {
            if let Err(err) = sync_index.remove_path(local_path) {
                log::warn!(
                    "Startup deletion sync trashed '{}' in album '{}' but could not remove sync record for '{}': {}",
                    filename,
                    album_name,
                    local_path,
                    err
                );
            }
            log::info!(
                "Startup deletion sync moved '{}' in album '{}' to Immich trash",
                filename,
                album_name
            );
            true
        }
        Err(err) => {
            log::warn!(
                "Startup deletion sync could not move '{}' in album '{}' to trash: {}",
                filename,
                album_name,
                err
            );
            false
        }
    }
}

/// Find the remote asset ID matching the given checksum inside an album.
async fn find_album_asset_id_by_checksum(
    api_client: &ImmichApiClient,
    album_id: &str,
    checksum: &str,
) -> Result<Option<(String, String)>, String> {
    let mut page = 1;
    loop {
        let (assets, has_more) = api_client
            .fetch_album_assets(album_id, page, 1000, None)
            .await?;
        if let Some(asset) = assets
            .into_iter()
            .find(|asset| asset.checksum.as_deref() == Some(checksum))
        {
            return Ok(Some((asset.id, asset.filename)));
        }
        if !has_more {
            return Ok(None);
        }
        page += 1;
    }
}

/// Stage 1: Walk all watch paths in parallel using rayon.
///
/// Returns `(candidates, seen_paths, skipped_count, error_count)`.
#[cfg(test)]
mod tests;
