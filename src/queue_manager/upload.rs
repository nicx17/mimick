use std::fs;
use std::path::PathBuf;

use crate::api_client::{ImmichApiClient, TransferProgressCallback};
use crate::sync_index::SyncTarget;

use super::FileTask;

/// Upload or reassociate a file, then ensure the resulting asset is present in the target album.
pub(super) async fn handle_upload(
    api: &ImmichApiClient,
    task: &FileTask,
    progress: TransferProgressCallback,
) -> Option<SyncTarget> {
    let uploaded = upload_or_reuse_asset(api, task, progress).await;
    let asset_id = match resolve_final_asset_id(api, task, uploaded).await {
        Ok(id) => id,
        Err(early_return) => return early_return,
    };

    if task.skip_album {
        log::debug!(
            "Skipping album association for library upload: asset_id={}",
            asset_id
        );
        return Some(SyncTarget {
            album_name: None,
            album_id: None,
        });
    }

    assign_upload_to_album(api, task, &asset_id).await
}

async fn upload_or_reuse_asset(
    api: &ImmichApiClient,
    task: &FileTask,
    progress: TransferProgressCallback,
) -> Option<String> {
    // Pre-flight: ask the server whether this checksum already exists. When it
    // does, we skip streaming the file body entirely and reuse the existing
    // asset id for album assignment. The 409 DUPLICATE path below still acts
    // as a safety net for races (e.g. concurrent upload by another client).
    match api.find_existing_asset_id(&task.checksum).await {
        Some(existing) => {
            log::debug!("Asset already on server, skipping upload: {}", task.path);
            Some(existing)
        }
        None => {
            api.upload_asset(
                &task.path,
                &task.checksum,
                task.sidecar_path.as_deref(),
                Some(progress.clone()),
            )
            .await
        }
    }
}

async fn assign_upload_to_album(
    api: &ImmichApiClient,
    task: &FileTask,
    asset_id: &str,
) -> Option<SyncTarget> {
    // Fall back to the parent directory name when no explicit album name is configured.
    let album_name = match (&task.album_name, &task.album_id) {
        (Some(name), _) if !name.is_empty() && name != "Default (Folder Name)" => name.clone(),
        _ => std::path::Path::new(&task.path)
            .parent()
            .and_then(std::path::Path::file_name)
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Mimick".to_string()),
    };

    log::info!("Adding '{}' to album '{}'", task.path, album_name);

    let mut final_album_id = match resolve_album_id(api, task, &album_name).await {
        Some(id) => id,
        None => {
            log::warn!(
                "Could not resolve album '{}'. Asset uploaded but not added to album.",
                album_name
            );
            return None;
        }
    };

    final_album_id = match add_to_album_with_retry(api, task, asset_id, final_album_id).await {
        Some(id) => id,
        None => return None,
    };

    Some(SyncTarget {
        album_name: Some(album_name),
        album_id: Some(final_album_id),
    })
}

/// Resolve the album ID from either an explicit task ID or by name lookup.
async fn resolve_album_id(
    api: &ImmichApiClient,
    task: &FileTask,
    album_name: &str,
) -> Option<String> {
    if let Some(ref id) = task.album_id
        && !id.is_empty()
    {
        return Some(id.clone());
    }
    match api.get_or_create_album(album_name).await {
        Ok(id) => id,
        Err(e) => {
            log::warn!("Failed to resolve album '{}': {}", album_name, e);
            None
        }
    }
}

/// Infer target album name from directory structure segment.
fn infer_album_name(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .parent()
        .and_then(std::path::Path::file_name)
        .map(|n| n.to_string_lossy().to_string())
}

/// Persist current list of failed items to retries cache.
pub(super) fn save_retries(path: &PathBuf, tasks: &[FileTask]) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(content) = serde_json::to_string(tasks) {
        let unique_ext = format!(
            "tmp.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let tmp = path.with_extension(unique_ext);
        if fs::write(&tmp, content).is_ok()
            && let Err(e) = fs::rename(&tmp, path)
        {
            let _ = fs::remove_file(&tmp);
            log::warn!("Failed to save retries: {}", e);
        }
    }
}

/// Load historical list of failed tasks from retries cache.
pub(super) fn load_retries(path: &PathBuf) -> Vec<FileTask> {
    if !path.exists() {
        return Vec::new();
    }
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(e) => {
            log::error!("Failed to load retries: {}", e);
            Vec::new()
        }
    }
}

async fn resolve_final_asset_id(
    api: &ImmichApiClient,
    task: &FileTask,
    asset_id: Option<String>,
) -> Result<String, Option<SyncTarget>> {
    let id = match asset_id {
        None => return Err(None),
        Some(ref id) if id == "DUPLICATE" => match api.find_existing_asset_id(&task.checksum).await
        {
            Some(existing) => existing,
            None => {
                log::info!("Asset already on server: {}", task.path);
                return Err(Some(SyncTarget {
                    album_name: task
                        .album_name
                        .clone()
                        .or_else(|| infer_album_name(&task.path)),
                    album_id: task.album_id.clone(),
                }));
            }
        },
        Some(id) => id,
    };
    Ok(id)
}

async fn add_to_album_with_retry(
    api: &ImmichApiClient,
    task: &FileTask,
    asset_id: &str,
    mut final_album_id: String,
) -> Option<String> {
    let asset_id_str = asset_id.to_string();
    if !api
        .add_assets_to_album(&final_album_id, std::slice::from_ref(&asset_id_str))
        .await
    {
        let name = task
            .album_name
            .clone()
            .or_else(|| infer_album_name(&task.path))?;
        log::warn!(
            "Album '{}' may be stale or deleted. Refreshing album resolution.",
            final_album_id
        );
        final_album_id = match api.resolve_album_by_name(&name, true).await {
            Ok(Some(id)) => id,
            Ok(None) | Err(_) => return None,
        };
        if !api
            .add_assets_to_album(&final_album_id, std::slice::from_ref(&asset_id_str))
            .await
        {
            return None;
        }
    }
    Some(final_album_id)
}
