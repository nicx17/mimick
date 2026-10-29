use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tokio::sync::{Mutex, Notify, mpsc};
use tokio_util::sync::CancellationToken;

use crate::notifications;
use crate::state_manager::{AppState, TransferDirection};
use crate::sync_index::{ShardedSyncIndex, SyncTarget};

use super::batch_notifications::{
    BatchNotifyState, activate_batch_if_needed, check_batch_notification,
    schedule_batch_notification,
};
use super::upload::{handle_upload, load_retries};
use super::{
    EnvironmentPolicy, FileTask, QueueManager, WorkerContext, unix_timestamp_now,
    upload_progress_callback, wait_until_allowed,
};

pub(super) async fn complete_worker_task(
    ctx: &WorkerContext,
    task: &FileTask,
    sync_target: Option<SyncTarget>,
    started: Instant,
) {
    let success = sync_target.is_some();
    let elapsed = started.elapsed().as_secs_f32();
    let active_route = ctx.api.active_route_label().await;
    let latest_issue = ctx.api.latest_issue().await;

    if let Some(target) = sync_target.as_ref() {
        complete_successful_upload(ctx, task, target, active_route, elapsed).await;
    } else {
        record_upload_failure(ctx, task, active_route, latest_issue.as_ref(), elapsed);
    }

    track_consecutive_failures(
        success,
        &ctx.consecutive_failures,
        &ctx.connectivity_notified,
    );
    if let Some(batch_id) = finalize_upload_progress(
        &ctx.state_ref,
        &ctx.batch_notify,
        &ctx.sync_index,
        &task.path,
        success,
    ) {
        schedule_batch_notification(ctx.state_ref.clone(), ctx.batch_notify.clone(), batch_id);
    }
}

async fn complete_successful_upload(
    ctx: &WorkerContext,
    task: &FileTask,
    target: &SyncTarget,
    active_route: Option<String>,
    elapsed: f32,
) {
    log::info!("Upload SUCCESS: {} ({:.2}s)", task.path, elapsed);
    ctx.pending_ref.lock().remove(&task.path);
    if let Err(err) = ctx
        .sync_index
        .record_synced(&task.path, &task.checksum, target)
    {
        log::warn!("Failed to update sync index for '{}': {}", task.path, err);
    }
    requeue_retries_after_success(
        &ctx.state_ref,
        &ctx.retry_ref,
        &ctx.batch_notify,
        &ctx.sender,
    )
    .await;
    record_upload_success(&ctx.state_ref, task, Some(target), active_route, elapsed);
}

async fn requeue_retries_after_success(
    state_ref: &Arc<parking_lot::Mutex<AppState>>,
    retry_ref: &Arc<parking_lot::Mutex<Vec<FileTask>>>,
    batch_notify: &Arc<parking_lot::Mutex<BatchNotifyState>>,
    sender: &mpsc::Sender<FileTask>,
) {
    let retries = std::mem::take(&mut *retry_ref.lock());
    if retries.is_empty() {
        return;
    }
    log::info!(
        "Network active. Re-queuing {} retry item(s).",
        retries.len()
    );
    {
        let mut state = state_ref.lock();
        let mut batch_state = batch_notify.lock();
        activate_batch_if_needed(&mut batch_state, &state);
        state.failed_count = state.failed_count.saturating_sub(retries.len());
        state.total_queued += retries.len();
    }
    for retry in retries {
        let _ = sender.send(retry).await;
    }
}

/// Upper bound on upload workers; workers above the configured limit idle until it rises.
pub(super) const MAX_WORKERS: usize = 10;

/// Everything a worker task needs: shared upload state plus queue plumbing.
#[derive(Clone)]
pub(super) struct WorkerRuntime {
    pub(super) ctx: WorkerContext,
    pub(super) rx: Arc<Mutex<mpsc::Receiver<FileTask>>>,
    pub(super) worker_limit: Arc<AtomicUsize>,
    pub(super) limit_changed: Arc<Notify>,
    pub(super) policy: Arc<parking_lot::Mutex<EnvironmentPolicy>>,
    pub(super) cancel: CancellationToken,
}

pub(super) fn retry_file_path() -> PathBuf {
    let mut path = crate::profile::cache_dir()
        .unwrap_or_else(|| PathBuf::from("~/.cache").join(crate::profile::dir_segment()));
    path.push("retries.json");
    path
}

/// Load persisted retries and clear the file so only current-session failures are kept.
pub(super) fn load_and_clear_retries(
    retry_path: &PathBuf,
    shared_state: &Arc<parking_lot::Mutex<AppState>>,
) -> Vec<FileTask> {
    let loaded_retries = load_retries(retry_path);
    if !loaded_retries.is_empty() {
        log::info!(
            "Loaded {} item(s) from retry queue. Clearing file.",
            loaded_retries.len()
        );
        let _ = fs::write(retry_path, "[]");
        shared_state.lock().failed_count = loaded_retries.len();
    }
    loaded_retries
}

/// Paths already waiting in the queue, so duplicates of retried files aren't queued twice.
pub(super) fn pending_paths_of(tasks: &[FileTask]) -> HashSet<String> {
    tasks.iter().map(|task| task.path.clone()).collect()
}

/// Re-queue persisted retries after startup so the main daemon can settle first.
pub(super) fn spawn_retry_requeue(qm: &QueueManager, loaded_retries: Vec<FileTask>) {
    if loaded_retries.is_empty() {
        return;
    }
    let sender = qm.sender.clone();
    let state_ref = qm.shared_state.clone();
    let batch_notify_state = qm.batch_notify_state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        {
            let mut s = state_ref.lock();
            let mut batch_state = batch_notify_state.lock();
            activate_batch_if_needed(&mut batch_state, &s);
            // Retry items are now being actively queued — reset failed_count.
            s.failed_count = 0;
            s.total_queued += loaded_retries.len();
        }
        for task in loaded_retries {
            log::info!("Re-queuing from retry: {}", task.path);
            let _ = sender.send(task).await;
        }
    });
}

pub(super) async fn run_worker(i: usize, rt: WorkerRuntime) {
    log::debug!("Worker {} started", i);
    loop {
        if !wait_for_worker_slot(i, &rt).await {
            log::debug!("Worker {} cancelled while throttled, exiting.", i);
            return;
        }
        let Some(file_task) = next_task(&rt).await else {
            log::debug!("Worker {} channel closed, exiting.", i);
            break;
        };
        process_task(i, &rt, file_task).await;
    }
}

/// Idle while this worker's index is above the configured limit; `false` once shutting down.
async fn wait_for_worker_slot(i: usize, rt: &WorkerRuntime) -> bool {
    loop {
        let limit_changed = rt.limit_changed.notified();
        tokio::pin!(limit_changed);
        limit_changed.as_mut().enable();
        if i < rt.worker_limit.load(Ordering::Relaxed) {
            return true;
        }
        tokio::select! {
            _ = rt.cancel.cancelled() => return false,
            _ = limit_changed => {}
        }
    }
}

/// Next queued task, or `None` on shutdown or when the channel closes.
async fn next_task(rt: &WorkerRuntime) -> Option<FileTask> {
    tokio::select! {
        biased;
        _ = rt.cancel.cancelled() => None,
        msg = async {
            let mut receiver = rt.rx.lock().await;
            receiver.recv().await
        } => msg,
    }
}

async fn process_task(i: usize, rt: &WorkerRuntime, file_task: FileTask) {
    let state_ref = &rt.ctx.state_ref;
    wait_until_allowed(state_ref, &rt.policy).await;
    let file_size = tokio::fs::metadata(&file_task.path)
        .await
        .ok()
        .map(|metadata| metadata.len());
    let (processed, total) = mark_upload_started(state_ref, &file_task, file_size);
    log::info!(
        "Worker {} uploading [{}/{}]: {}",
        i,
        processed + 1,
        total,
        file_task.path
    );

    let t_start = std::time::Instant::now();
    let sync_target = tokio::select! {
        biased;
        _ = rt.cancel.cancelled() => {
            log::warn!("Upload cancelled by shutdown: {}", file_task.path);
            None
        }
        res = handle_upload(
            &rt.ctx.api,
            &file_task,
            upload_progress_callback(state_ref, file_task.path.clone()),
        ) => res,
    };
    complete_worker_task(&rt.ctx, &file_task, sync_target, t_start).await;
}

/// Update the shared progress snapshot before handing off to the API.
/// Returns `(processed_count, total_queued)` for the progress log line.
fn mark_upload_started(
    state_ref: &Arc<parking_lot::Mutex<AppState>>,
    file_task: &FileTask,
    file_size: Option<u64>,
) -> (usize, usize) {
    let mut s = state_ref.lock();
    s.active_workers += 1;
    s.status = "uploading".to_string();
    s.pause_reason = None;
    s.current_file = Some(file_task.path.clone());
    s.queue_size = s.total_queued.saturating_sub(s.processed_count);
    s.progress = if s.total_queued > 0 {
        ((s.processed_count as f32 / s.total_queued as f32) * 100.0) as u8
    } else {
        0
    };
    let attempts = current_attempt_count(&s, &file_task.path);
    s.record_event(file_task.path.clone(), "uploading", None, attempts);
    let item_label = std::path::Path::new(&file_task.path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .or_else(|| Some(file_task.path.clone()));
    let route = s.active_server_route.clone();
    s.transfer.register_item(
        TransferDirection::Upload,
        file_task.path.clone(),
        file_size,
        item_label,
        route,
    );
    (s.processed_count, s.total_queued)
}

/// Get current sync attempt count for a path based on recent events history.
pub(super) fn current_attempt_count(state: &AppState, path: &str) -> u32 {
    state
        .recent_events
        .iter()
        .find(|event| event.path == path)
        .map(|event| event.attempts)
        .unwrap_or(1)
}

/// Activate a batch tracking window if none is currently active.
/// Record state updates after a successful upload.
fn record_upload_success(
    state_ref: &Arc<parking_lot::Mutex<AppState>>,
    task: &FileTask,
    sync_target: Option<&SyncTarget>,
    active_route: Option<String>,
    elapsed: f32,
) {
    let mut s = state_ref.lock();
    let attempts = current_attempt_count(&s, &task.path);
    s.active_server_route = active_route;
    s.last_successful_sync_at = Some(unix_timestamp_now());
    s.last_completed_file = Some(task.path.clone());
    s.last_error = None;
    s.last_error_guidance = None;
    s.record_event(
        task.path.clone(),
        "completed",
        Some(format!("Finished in {:.2}s", elapsed)),
        attempts,
    );
    if let Some(target) = sync_target {
        let status = s
            .folder_statuses
            .entry(task.watch_path.clone())
            .or_default();
        status.pending_count = status.pending_count.saturating_sub(1);
        status.last_sync_at = Some(unix_timestamp_now());
        status.last_error = None;
        status.target_album = target.album_name.clone();
    }
}

/// Record state updates after a failed upload.
fn record_upload_failure(
    ctx: &WorkerContext,
    task: &FileTask,
    active_route: Option<String>,
    latest_issue: Option<&crate::api_client::ApiIssue>,
    elapsed: f32,
) {
    log::warn!(
        "Upload FAILED: {} ({:.2}s). Adding to retry queue.",
        task.path,
        elapsed
    );
    ctx.retry_ref.lock().push(task.clone());
    let mut s = ctx.state_ref.lock();
    s.failed_count += 1;
    s.active_server_route = active_route;
    let error_text = latest_issue
        .map(|issue| issue.summary.clone())
        .unwrap_or_else(|| format!("Upload failed for {}", task.path));
    s.last_error = Some(error_text.clone());
    s.last_error_guidance = latest_issue
        .map(|issue| issue.guidance.clone())
        .or_else(|| {
            Some(
                "Review the latest server and permission settings, then retry the failed item."
                    .to_string(),
            )
        });
    let status = s
        .folder_statuses
        .entry(task.watch_path.clone())
        .or_default();
    status.pending_count = status.pending_count.saturating_sub(1);
    status.last_error = Some(error_text);
    let attempts = current_attempt_count(&s, &task.path);
    s.record_event(
        task.path.clone(),
        "failed",
        Some("Queued for retry".to_string()),
        attempts,
    );
}

/// Track consecutive failures and fire a connectivity-lost notification after 3+.
fn track_consecutive_failures(
    success: bool,
    consec_fail_ref: &Arc<parking_lot::Mutex<usize>>,
    connectivity_notified_ref: &Arc<parking_lot::Mutex<bool>>,
) {
    if success {
        *consec_fail_ref.lock() = 0;
        return;
    }
    let mut cf = consec_fail_ref.lock();
    *cf += 1;
    if *cf >= 3 {
        let mut notified = connectivity_notified_ref.lock();
        if !*notified {
            *notified = true;
            notifications::send_connectivity_lost();
        }
    }
}

/// Finalize upload progress: update counters, detect queue idle, return batch ID if
/// a summary notification should be scheduled.
fn finalize_upload_progress(
    state_ref: &Arc<parking_lot::Mutex<AppState>>,
    batch_notify_ref: &Arc<parking_lot::Mutex<BatchNotifyState>>,
    sync_index_ref: &Arc<ShardedSyncIndex>,
    path: &str,
    success: bool,
) -> Option<u64> {
    let mut s = state_ref.lock();
    if success {
        s.processed_count += 1;
    }
    s.active_workers -= 1;
    s.current_file = None;
    let route = s.active_server_route.clone();
    let completed_batch = s
        .transfer
        .finish_item(TransferDirection::Upload, path, route);
    if completed_batch {
        s.completed_upload_batches = s.completed_upload_batches.saturating_add(1);
    }
    let total_handled = s.processed_count + s.failed_count;
    if total_handled >= s.total_queued && s.active_workers == 0 {
        apply_idle_state(&mut s);
        let completed = check_batch_notification(&mut batch_notify_ref.lock());
        drop(s);
        if let Err(err) = sync_index_ref.flush() {
            log::warn!("Failed to flush sync index on idle: {}", err);
        }
        completed
    } else {
        apply_active_state(&mut s, total_handled);
        None
    }
}

fn apply_idle_state(s: &mut AppState) {
    s.queue_size = 0;
    s.status = if s.paused {
        "paused".to_string()
    } else {
        "idle".to_string()
    };
    s.progress = 100;
    log::info!("All {} file(s) processed. Idle.", s.total_queued);
}

fn apply_active_state(s: &mut AppState, total_handled: usize) {
    s.queue_size = s.total_queued.saturating_sub(total_handled);
    s.progress = if s.total_queued > 0 {
        ((total_handled as f32 / s.total_queued as f32) * 100.0) as u8
    } else {
        0
    };
    s.status = "uploading".to_string();
}
