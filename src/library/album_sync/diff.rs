use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use crate::api_client::LibraryAsset;
use crate::app_context::AppContext;
use crate::config::{FolderRules, FolderSyncMethod};
use crate::library::local_source::{LocalAsset, enumerate_local};
use crate::monitor::compute_sha1_chunked;
use crate::sync_index::SyncTarget;

use super::{
    ALBUM_TO_FOLDER_TRASH_AVAILABLE, clear_pending_deletions, compute_orphan_records,
    compute_remote_deletions, confirm_pending_deletions, fetch_remote_album_assets,
};

#[derive(Debug, Default, Clone)]
pub struct AlbumDiff {
    pub to_upload: Vec<LocalEntry>,
    pub to_download: Vec<LibraryAsset>,
    pub to_delete_remote: Vec<LibraryAsset>,
    pub to_delete_local: Vec<LocalEntry>,
    pub remote_unhashed: usize,
}

#[derive(Debug, Clone)]
pub struct LocalEntry {
    pub local: LocalAsset,
    pub checksum: String,
}

pub async fn diff_album_vs_folder(
    ctx: Arc<AppContext>,
    album_id: &str,
    watch_path: &Path,
    rules: &FolderRules,
    manual_sync: bool,
) -> Result<AlbumDiff, String> {
    let mut prepared = prepare_diff(&ctx, album_id, watch_path, rules).await?;

    let classify_ctx = ClassifyContext {
        album_id,
        remote_unhashed: prepared.remote_unhashed,
        rules,
        manual_sync,
    };
    let (mut to_upload, mut to_delete_local) = classify_local_entries(
        &ctx,
        prepared.local_entries,
        &prepared.remote_set,
        &mut prepared.orphan_by_checksum,
        &classify_ctx,
    );

    let mut to_delete_remote = compute_remote_deletions(
        &ctx,
        album_id,
        watch_path,
        &prepared.local_paths,
        &prepared.remote_by_checksum,
        rules,
    );

    // Two-tick confirmation (#7) for mass deletes
    confirm_pending_deletions(&ctx, album_id, &mut to_delete_local, &mut to_delete_remote);

    // Clear pending-deletion confirmations for items that came back
    clear_pending_deletions(
        &ctx,
        album_id,
        &prepared.remote_set,
        &to_upload,
        &prepared.local_paths,
    );

    if !to_upload.is_empty()
        || !prepared.to_download.is_empty()
        || !to_delete_local.is_empty()
        || !to_delete_remote.is_empty()
    {
        log::info!(
            "Album sync diff: upload={} download={} trash_local={} trash_remote={}",
            to_upload.len(),
            prepared.to_download.len(),
            to_delete_local.len(),
            to_delete_remote.len()
        );
    }

    if rules.sync_method == FolderSyncMethod::UploadOnly {
        prepared.to_download.clear();
    } else if rules.sync_method == FolderSyncMethod::DownloadOnly {
        to_upload.clear();
    }

    Ok(AlbumDiff {
        to_upload,
        to_download: prepared.to_download,
        to_delete_remote,
        to_delete_local,
        remote_unhashed: prepared.remote_unhashed,
    })
}

struct PreparedDiff {
    local_entries: Vec<LocalEntry>,
    local_paths: HashSet<String>,
    orphan_by_checksum: std::collections::HashMap<String, String>,
    remote_by_checksum: std::collections::HashMap<String, LibraryAsset>,
    remote_set: HashSet<String>,
    to_download: Vec<LibraryAsset>,
    remote_unhashed: usize,
}

async fn prepare_diff(
    ctx: &Arc<AppContext>,
    album_id: &str,
    watch_path: &Path,
    rules: &FolderRules,
) -> Result<PreparedDiff, String> {
    let remote = fetch_remote_album_assets(ctx, album_id).await?;
    let watch_root = watch_path.to_path_buf();
    let locals: Vec<LocalAsset> = enumerate_local(ctx.clone())
        .await
        .into_iter()
        .filter(|asset| asset.path.starts_with(&watch_root))
        .collect();
    let local_entries = resolve_local_checksums(ctx.clone(), locals).await;
    let local_set = local_entries.iter().map(|e| e.checksum.clone()).collect();
    let local_paths = local_entries
        .iter()
        .map(|e| e.local.path.to_string_lossy().to_string())
        .collect();
    let orphan_by_checksum = compute_orphan_records(ctx, album_id, watch_path, &local_paths);
    Ok(classify_remote_assets(
        remote,
        local_entries,
        local_set,
        local_paths,
        orphan_by_checksum,
        rules,
    ))
}

fn classify_remote_assets(
    remote: Vec<LibraryAsset>,
    local_entries: Vec<LocalEntry>,
    local_set: HashSet<String>,
    local_paths: HashSet<String>,
    orphan_by_checksum: std::collections::HashMap<String, String>,
    rules: &FolderRules,
) -> PreparedDiff {
    let mut prepared = PreparedDiff {
        local_entries,
        local_paths,
        orphan_by_checksum,
        remote_by_checksum: std::collections::HashMap::new(),
        remote_set: HashSet::new(),
        to_download: Vec::new(),
        remote_unhashed: 0,
    };
    for asset in remote {
        match asset.checksum.as_deref().filter(|c| !c.is_empty()) {
            Some(checksum) => {
                prepared.remote_set.insert(checksum.to_string());
                prepared
                    .remote_by_checksum
                    .entry(checksum.to_string())
                    .or_insert_with(|| asset.clone());
                if !local_set.contains(checksum)
                    && !(rules.delete_folder_to_album
                        && prepared.orphan_by_checksum.contains_key(checksum))
                {
                    prepared.to_download.push(asset);
                }
            }
            None => prepared.remote_unhashed += 1,
        }
    }
    prepared
}

struct ClassifyContext<'a> {
    album_id: &'a str,
    remote_unhashed: usize,
    rules: &'a FolderRules,
    manual_sync: bool,
}

fn classify_local_entries(
    ctx: &Arc<AppContext>,
    local_entries: Vec<LocalEntry>,
    remote_set: &HashSet<String>,
    orphan_by_checksum: &mut std::collections::HashMap<String, String>,
    cc: &ClassifyContext<'_>,
) -> (Vec<LocalEntry>, Vec<LocalEntry>) {
    let mut to_upload = Vec::new();
    let mut to_delete_local = Vec::new();

    for entry in local_entries {
        match classify_local_entry(ctx, entry, remote_set, orphan_by_checksum, cc) {
            LocalDecision::Upload(entry) => to_upload.push(entry),
            LocalDecision::Delete(entry) => to_delete_local.push(entry),
            LocalDecision::Ignore => {}
        }
    }

    (to_upload, to_delete_local)
}

enum LocalDecision {
    Upload(LocalEntry),
    Delete(LocalEntry),
    Ignore,
}

fn classify_local_entry(
    ctx: &Arc<AppContext>,
    entry: LocalEntry,
    remote_set: &HashSet<String>,
    orphan_by_checksum: &mut std::collections::HashMap<String, String>,
    cc: &ClassifyContext<'_>,
) -> LocalDecision {
    let path_str = entry.local.path.to_string_lossy().to_string();
    if remote_set.contains(&entry.checksum) {
        migrate_orphan_if_needed(ctx, orphan_by_checksum, &entry, &path_str);
        return LocalDecision::Ignore;
    }
    if !was_previously_synced_to(ctx, &path_str, &entry.checksum, cc.album_id) {
        return LocalDecision::Upload(entry);
    }
    synced_entry_decision(entry, cc)
}

fn migrate_orphan_if_needed(
    ctx: &Arc<AppContext>,
    orphan_by_checksum: &mut std::collections::HashMap<String, String>,
    entry: &LocalEntry,
    path_str: &str,
) {
    if let Some(old_path) = orphan_by_checksum.remove(&entry.checksum) {
        migrate_renamed_record(ctx, &old_path, path_str, &entry.checksum);
    }
}

fn synced_entry_decision(entry: LocalEntry, cc: &ClassifyContext<'_>) -> LocalDecision {
    if cc.remote_unhashed > 0 {
        log::debug!(
            "Skipping local delete decision for {} because {} remote album item(s) have no checksum",
            entry.local.path.display(),
            cc.remote_unhashed
        );
        LocalDecision::Ignore
    } else if cc.rules.delete_album_to_folder && ALBUM_TO_FOLDER_TRASH_AVAILABLE {
        LocalDecision::Delete(entry)
    } else if cc.manual_sync {
        LocalDecision::Upload(entry)
    } else {
        LocalDecision::Ignore
    }
}

fn was_previously_synced_to(
    ctx: &Arc<AppContext>,
    path: &str,
    checksum: &str,
    album_id: &str,
) -> bool {
    ctx.sync_index.record_for_path(path).is_some_and(|record| {
        record.checksum == checksum && record.album_id.as_deref().is_none_or(|id| id == album_id)
    })
}

async fn resolve_local_checksums(ctx: Arc<AppContext>, locals: Vec<LocalAsset>) -> Vec<LocalEntry> {
    let mut out = Vec::with_capacity(locals.len());
    let mut to_compute: Vec<LocalAsset> = Vec::new();

    {
        for asset in locals {
            match ctx.sync_index.fresh_checksum(&asset.path) {
                Some(c) => out.push(LocalEntry {
                    local: asset,
                    checksum: c,
                }),
                None => to_compute.push(asset),
            }
        }
    }

    for asset in to_compute {
        let path_str = asset.path.to_string_lossy().to_string();
        let hashed = tokio::task::spawn_blocking(move || compute_sha1_chunked(&path_str))
            .await
            .map_err(|err| err.to_string())
            .and_then(|r| r.map_err(|err| err.to_string()));
        match hashed {
            Ok(checksum) => out.push(LocalEntry {
                local: asset,
                checksum,
            }),
            Err(err) => log::warn!("Skipping {} during diff: {}", asset.path.display(), err),
        }
    }

    out
}

fn migrate_renamed_record(ctx: &Arc<AppContext>, old_path: &str, new_path: &str, checksum: &str) {
    let target = ctx
        .sync_index
        .record_for_path(old_path)
        .map(|record| SyncTarget {
            album_name: record.album_name,
            album_id: record.album_id,
        })
        .unwrap_or_else(|| SyncTarget {
            album_name: None,
            album_id: None,
        });

    if let Err(err) = ctx.sync_index.remove_path(old_path) {
        log::warn!(
            "Could not migrate sync record from {} during rename: {}",
            old_path,
            err
        );
        return;
    }
    if let Err(err) = ctx.sync_index.record_synced(new_path, checksum, &target) {
        log::warn!(
            "Could not record sync entry for renamed file {}: {}",
            new_path,
            err
        );
        return;
    }
    log::debug!("Renamed: {} -> {}", old_path, new_path);
}
