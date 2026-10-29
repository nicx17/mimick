use std::sync::Arc;
use tokio::sync::mpsc;

use crate::app_context::AppContext;
use crate::config;
use crate::config::best_matching_watch_entry;
use crate::monitor::MonitorEvent;
use crate::queue_manager::FileTask;
use crate::remote_sync;
use crate::startup_scan::queue_unsynced_files;
use crate::sync_index::{SyncDecision, SyncTarget};

pub(super) async fn run_file_event_loop(
    mut rx: mpsc::Receiver<MonitorEvent>,
    ctx: Arc<AppContext>,
) {
    while let Some(event) = rx.recv().await {
        match event {
            MonitorEvent::Ready {
                path,
                checksum,
                sidecar_path,
            } => queue_ready_file(&ctx, path, checksum, sidecar_path).await,
            MonitorEvent::Deleted { path } => handle_local_deletion(&ctx, path).await,
        }
    }
}

/// Queue a stable file for upload unless Mimick wrote it itself or it is already synced.
async fn queue_ready_file(
    ctx: &Arc<AppContext>,
    path: String,
    checksum: String,
    sidecar_path: Option<String>,
) {
    if ctx.expected_self_downloads.consume(&path) {
        return;
    }

    let (album_id, album_name, watch_path, folder_rules, skip_album) =
        watch_target_for(&path, &ctx.live_watch_paths.lock());

    // Resolve XMP: per-folder override -> global default.
    let global_xmp = ctx.config.read().data.upload_xmp_sidecars;
    let sidecar_path = if folder_rules.xmp_sidecar_enabled(global_xmp) {
        sidecar_path
    } else {
        None
    };

    let target = SyncTarget {
        album_name: album_name.clone(),
        album_id: album_id.clone(),
    };
    let Some((reassociate_only, task_checksum)) = upload_plan(ctx, &path, &target, checksum) else {
        return;
    };

    log::info!("Queuing: {} (sha1={})", path, task_checksum);

    let _ = ctx
        .queue_manager
        .add_to_queue(FileTask {
            path,
            watch_path,
            checksum: task_checksum,
            album_id,
            album_name,
            reassociate_only,
            skip_album,
            sidecar_path,
        })
        .await;
}

/// Album target, watch root, rules, and library-only flag of the folder containing `path`.
fn watch_target_for(
    path: &str,
    entries: &[config::WatchPathEntry],
) -> (
    Option<String>,
    Option<String>,
    String,
    config::FolderRules,
    bool,
) {
    let Some(entry) = best_matching_watch_entry(std::path::Path::new(path), entries) else {
        return (None, None, String::new(), Default::default(), false);
    };
    match entry {
        config::WatchPathEntry::WithConfig {
            album_id,
            album_name,
            rules,
            ..
        } => (
            album_id.clone(),
            album_name.clone(),
            entry.path().to_string(),
            rules.clone(),
            entry.uploads_to_library(),
        ),
        config::WatchPathEntry::Simple(_) => (
            None,
            None,
            entry.path().to_string(),
            Default::default(),
            false,
        ),
    }
}

/// `(reassociate_only, checksum)` for the queue, or `None` if the file is up to date.
fn upload_plan(
    ctx: &AppContext,
    path: &str,
    target: &SyncTarget,
    checksum: String,
) -> Option<(bool, String)> {
    match ctx
        .sync_index
        .sync_decision(std::path::Path::new(path), target)
    {
        Ok(SyncDecision::UpToDate) => {
            log::debug!("Skipping unchanged file: {}", path);
            None
        }
        // Already uploaded but linked to a different album: reuse the stored
        // checksum and only re-link instead of uploading again.
        Ok(SyncDecision::NeedsReassociate) => Some((
            true,
            ctx.sync_index.stored_checksum(path).unwrap_or(checksum),
        )),
        Ok(SyncDecision::NeedsUpload) => Some((false, checksum)),
        Err(err) => {
            log::warn!(
                "Could not inspect sync index for '{}': {}; queuing anyway",
                path,
                err
            );
            Some((false, checksum))
        }
    }
}

/// Trash the remote copy of a locally deleted file, unless Mimick deleted it itself.
async fn handle_local_deletion(ctx: &Arc<AppContext>, path: String) {
    if ctx.expected_self_deletions.consume(&path) {
        return;
    }
    if let Some(request) = remote_sync::build_local_deletion_request(ctx.clone(), path).await {
        remote_sync::trash_remote_after_local_delete(ctx.clone(), request).await;
    }
}

/// Queue anything in the watch folders that isn't synced yet.
pub(super) async fn run_catchup_scan(ctx: Arc<AppContext>) {
    let (watch_paths, catchup_mode) = {
        let cfg = ctx.config.read();
        (
            cfg.data.watch_paths.clone(),
            cfg.data.startup_catchup_mode.clone(),
        )
    };
    queue_unsynced_files(
        watch_paths,
        ctx.queue_manager.clone(),
        ctx.sync_index.clone(),
        ctx.api_client.clone(),
        catchup_mode,
        ctx.state.clone(),
        ctx.clone(),
    )
    .await;
}

/// Run a catch-up scan for each "Sync Now" request.
pub(super) async fn run_manual_sync_listener(
    mut manual_sync_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    ctx: Arc<AppContext>,
) {
    while manual_sync_rx.recv().await.is_some() {
        run_catchup_scan(ctx.clone()).await;
    }
}

/// Record the initial server route and any connection error for the status page.
pub(super) async fn refresh_connection_status(ctx: Arc<AppContext>) {
    let connected = ctx.api_client.check_connection().await;
    let route = ctx.api_client.active_route_label().await;
    let latest_issue = ctx.api_client.latest_issue().await;

    let mut state = ctx.state.lock();
    state.active_server_route = route;
    if connected {
        state.last_error = None;
        state.last_error_guidance = None;
    } else if let Some(issue) = latest_issue {
        state.last_error = Some(issue.summary);
        state.last_error_guidance = Some(issue.guidance);
    }
}
