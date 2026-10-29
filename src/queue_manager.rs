//! Manages upload queue orchestration, retry persistence, and sync-index updates.
//!
//! A configurable pool of async workers drains the upload queue in parallel.
//! Failed tasks are moved to a retry queue with exponential back-off. The
//! queue pauses automatically when environment policies (metered network,
//! battery power, quiet hours) are active, and resumes when conditions clear.

use crate::api_client::{ImmichApiClient, TransferProgressCallback};
use crate::runtime_env;
use crate::state_manager::{AppState, TransferDirection};
use crate::sync_index::ShardedSyncIndex;
use chrono::Timelike;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, Notify, mpsc};
use tokio_util::sync::CancellationToken;

mod upload;
#[cfg(test)]
use upload::load_retries;
use upload::save_retries;
mod batch_notifications;
use batch_notifications::BatchNotifyState;
mod progress;
use batch_notifications::activate_batch_if_needed;
use progress::{
    MAX_WORKERS, WorkerRuntime, current_attempt_count, load_and_clear_retries, pending_paths_of,
    retry_file_path, run_worker, spawn_retry_requeue,
};

#[derive(Clone)]
struct WorkerContext {
    api: Arc<ImmichApiClient>,
    state_ref: Arc<parking_lot::Mutex<AppState>>,
    retry_ref: Arc<parking_lot::Mutex<Vec<FileTask>>>,
    pending_ref: Arc<parking_lot::Mutex<HashSet<String>>>,
    sync_index: Arc<ShardedSyncIndex>,
    batch_notify: Arc<parking_lot::Mutex<BatchNotifyState>>,
    connectivity_notified: Arc<parking_lot::Mutex<bool>>,
    consecutive_failures: Arc<parking_lot::Mutex<usize>>,
    sender: mpsc::Sender<FileTask>,
}

/// Helper to generate progress callback callbacks that update global transfer metrics.
fn upload_progress_callback(
    shared_state: &Arc<parking_lot::Mutex<AppState>>,
    item_id: String,
) -> TransferProgressCallback {
    let state_ref = shared_state.clone();
    Arc::new(move |bytes_done, total_bytes| {
        let mut state = state_ref.lock();
        let route = state.active_server_route.clone();
        if let Some(total_bytes) = total_bytes {
            let current = state
                .transfer
                .active_item_totals
                .get(&item_id)
                .copied()
                .unwrap_or(0);
            if current == 0 {
                state.transfer.update_item_total(&item_id, total_bytes);
            }
        }
        state
            .transfer
            .update_item_bytes(TransferDirection::Upload, &item_id, bytes_done, route);
    })
}

/// Represents a unit of work for the upload queue.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileTask {
    /// Absolute local file path.
    pub path: String,
    /// Absolute watch path prefix under which the file resides.
    #[serde(default)]
    pub watch_path: String,
    /// Precomputed file SHA-1 checksum.
    pub checksum: String,
    /// Target Immich album identifier if already resolved.
    #[serde(default)]
    pub album_id: Option<String>,
    /// Mapped Immich album name.
    #[serde(default)]
    pub album_name: Option<String>,
    /// True if the asset is already on the server and only needs album addition.
    #[serde(default)]
    pub reassociate_only: bool,
    /// Uploads that must land in the library without being pinned to an album.
    /// This is used by manual uploads and explicit library-only watch folders.
    #[serde(default)]
    pub skip_album: bool,
    /// Absolute path to an XMP sidecar file to attach during upload.
    /// `None` when no companion sidecar exists or sidecar upload is disabled.
    #[serde(default)]
    pub sidecar_path: Option<String>,
}

pub struct QueueManager {
    /// Internal task sender channel.
    sender: mpsc::Sender<FileTask>,
    /// Shared in-memory state accessed by both workers and the UI.
    shared_state: Arc<parking_lot::Mutex<AppState>>,
    /// Failed tasks accumulated in memory and flushed during graceful shutdown.
    retry_list: Arc<parking_lot::Mutex<Vec<FileTask>>>,
    /// Paths already queued or awaiting retry to avoid duplication.
    pending_paths: Arc<parking_lot::Mutex<HashSet<String>>>,
    /// Path of persistent retries file.
    retry_path: PathBuf,
    /// Shared current transmission limits and environmental constraints.
    policy: Arc<parking_lot::Mutex<EnvironmentPolicy>>,
    /// Active worker concurrency limit wrapper.
    worker_limit: Arc<AtomicUsize>,
    limit_changed: Arc<Notify>,
    /// Sync summary notification batch states tracker.
    batch_notify_state: Arc<parking_lot::Mutex<BatchNotifyState>>,
    /// Flag indicating whether new tasks are accepted.
    accepting_new: Arc<AtomicBool>,
    /// Internal shutdown cancellation coordinator.
    shutdown_token: CancellationToken,
}

#[derive(Debug, Clone, Copy)]
pub struct EnvironmentPolicy {
    /// True if syncing should pause when metered connections are detected.
    pub pause_on_metered_network: bool,
    /// True if syncing should pause when battery power is detected.
    pub pause_on_battery_power: bool,
    /// First local clock hour (0–23) of the quiet window. `None` = disabled.
    pub quiet_hours_start: Option<u8>,
    /// Last local clock hour (0–23, exclusive) of the quiet window.
    pub quiet_hours_end: Option<u8>,
}

impl QueueManager {
    /// Initialize a new QueueManager and kick off worker threads.
    pub fn new(
        api_client: Arc<ImmichApiClient>,
        workers: usize,
        shared_state: Arc<parking_lot::Mutex<AppState>>,
        sync_index: Arc<ShardedSyncIndex>,
        policy: EnvironmentPolicy,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<FileTask>(64);
        let retry_path = retry_file_path();
        let loaded_retries = load_and_clear_retries(&retry_path, &shared_state);

        // Retry state stays in memory during the session to avoid per-failure disk writes.
        let retry_list = Arc::new(parking_lot::Mutex::new(Vec::<FileTask>::new()));
        let pending_paths = Arc::new(parking_lot::Mutex::new(pending_paths_of(&loaded_retries)));
        let batch_notify_state = Arc::new(parking_lot::Mutex::new(BatchNotifyState::default()));
        let runtime = WorkerRuntime {
            ctx: WorkerContext {
                api: api_client,
                state_ref: shared_state.clone(),
                retry_ref: retry_list.clone(),
                pending_ref: pending_paths.clone(),
                sync_index,
                batch_notify: batch_notify_state.clone(),
                connectivity_notified: Arc::new(parking_lot::Mutex::new(false)),
                consecutive_failures: Arc::new(parking_lot::Mutex::new(0usize)),
                sender: tx.clone(),
            },
            rx: Arc::new(Mutex::new(rx)),
            worker_limit: Arc::new(AtomicUsize::new(workers.clamp(1, MAX_WORKERS))),
            limit_changed: Arc::new(Notify::new()),
            policy: Arc::new(parking_lot::Mutex::new(policy)),
            cancel: CancellationToken::new(),
        };

        let qm = Self {
            sender: tx,
            shared_state,
            retry_list,
            pending_paths,
            retry_path,
            policy: runtime.policy.clone(),
            worker_limit: runtime.worker_limit.clone(),
            limit_changed: runtime.limit_changed.clone(),
            batch_notify_state,
            accepting_new: Arc::new(AtomicBool::new(true)),
            shutdown_token: runtime.cancel.clone(),
        };

        for i in 0..MAX_WORKERS {
            tokio::spawn(run_worker(i, runtime.clone()));
        }
        spawn_retry_requeue(&qm, loaded_retries);
        qm
    }

    /// Add a file task to the upload queue and return whether it was accepted.
    pub async fn add_to_queue(&self, task: FileTask) -> bool {
        if !self.accepting_new.load(Ordering::Relaxed) {
            log::debug!("Refusing new task during shutdown: {}", task.path);
            return false;
        }
        log::debug!("Queuing: {}", task.path);
        {
            let mut pending = self.pending_paths.lock();
            if pending.contains(&task.path) {
                log::debug!("Skipping already pending task: {}", task.path);
                return false;
            }
            pending.insert(task.path.clone());
        }

        self.record_queued_task(&task);
        if let Err(e) = self.sender.send(task).await {
            log::error!("Failed to send task to queue: {}", e);
            self.rollback_queued_task(&e.0);
            return false;
        }

        true
    }

    fn record_queued_task(&self, task: &FileTask) {
        let mut s = self.shared_state.lock();
        let mut batch_state = self.batch_notify_state.lock();
        activate_batch_if_needed(&mut batch_state, &s);
        s.total_queued += 1;
        s.queue_size = s.total_queued.saturating_sub(s.processed_count);
        let status = s
            .folder_statuses
            .entry(task.watch_path.clone())
            .or_default();
        status.pending_count += 1;
        status.target_album = task.album_name.clone();
        let attempts = current_attempt_count(&s, &task.path);
        s.record_event(
            task.path.clone(),
            "pending",
            task.album_name
                .clone()
                .map(|name| format!("Target album: {}", name)),
            attempts,
        );
    }

    fn rollback_queued_task(&self, task: &FileTask) {
        self.pending_paths.lock().remove(&task.path);
        let mut s = self.shared_state.lock();
        s.total_queued = s.total_queued.saturating_sub(1);
        s.queue_size = s.total_queued.saturating_sub(s.processed_count);
        let route = s.active_server_route.clone();
        let completed_batch = s
            .transfer
            .finish_item(TransferDirection::Upload, &task.path, route);
        if completed_batch {
            s.completed_upload_batches = s.completed_upload_batches.saturating_add(1);
        }
        let status = s
            .folder_statuses
            .entry(task.watch_path.clone())
            .or_default();
        status.pending_count = status.pending_count.saturating_sub(1);
    }

    pub fn set_paused(&self, paused: bool, reason: Option<String>) {
        let mut state = self.shared_state.lock();
        state.paused = paused;
        state.pause_reason = reason;
        state.status = if paused {
            "paused".to_string()
        } else if state.active_workers > 0 {
            "uploading".to_string()
        } else {
            "idle".to_string()
        };
    }

    pub fn is_paused(&self) -> bool {
        self.shared_state.lock().paused
    }

    pub fn set_worker_limit(&self, workers: u8) {
        self.worker_limit
            .store((workers as usize).clamp(1, 10), Ordering::Relaxed);
        self.limit_changed.notify_waiters();
    }

    pub fn update_environment_policy(&self, policy: EnvironmentPolicy) {
        *self.policy.lock() = policy;
    }

    pub fn recent_events(&self) -> Vec<crate::state_manager::QueueEvent> {
        self.shared_state.lock().recent_events.clone()
    }

    pub fn failed_tasks(&self) -> Vec<FileTask> {
        self.retry_list.lock().clone()
    }

    pub fn clear_failed(&self) -> usize {
        let tasks = {
            let mut retries = self.retry_list.lock();
            std::mem::take(&mut *retries)
        };
        if tasks.is_empty() {
            return 0;
        }

        {
            let mut pending = self.pending_paths.lock();
            for task in &tasks {
                pending.remove(&task.path);
            }
        }

        let mut state = self.shared_state.lock();
        state.failed_count = state.failed_count.saturating_sub(tasks.len());
        for task in &tasks {
            let attempts = current_attempt_count(&state, &task.path);
            state.record_event(
                task.path.clone(),
                "cleared",
                Some("Removed from retry queue".to_string()),
                attempts,
            );
        }

        tasks.len()
    }

    pub async fn retry_all_failed(&self) -> usize {
        let tasks = {
            let mut retries = self.retry_list.lock();
            std::mem::take(&mut *retries)
        };
        self.requeue_failed(tasks, "Manual retry".to_string()).await
    }

    pub async fn retry_failed_path(&self, path: &str) -> bool {
        let task = {
            let mut retries = self.retry_list.lock();
            let index = retries.iter().position(|task| task.path == path);
            index.map(|index| retries.remove(index))
        };

        if let Some(task) = task {
            self.requeue_failed(vec![task], "Manual retry".to_string())
                .await
                > 0
        } else {
            false
        }
    }

    /// Stop accepting new tasks, wait up to `deadline` for active uploads
    /// to finish, then hard-cancel anything still in flight.
    ///
    /// In-flight uploads that get cancelled at the deadline land in the retry
    /// list (treated as failures) so `flush_retries` will persist them for the
    /// next session.
    pub async fn shutdown(&self, deadline: Duration) {
        self.accepting_new.store(false, Ordering::Relaxed);
        let active_now = self.shared_state.lock().active_workers;
        if active_now == 0 {
            self.shutdown_token.cancel();
            return;
        }
        log::info!(
            "Draining {} active upload(s); waiting up to {:?}...",
            active_now,
            deadline
        );

        let start = Instant::now();
        while start.elapsed() < deadline {
            if self.shared_state.lock().active_workers == 0 {
                log::info!("All active uploads finished within deadline.");
                self.shutdown_token.cancel();
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let stuck = self.shared_state.lock().active_workers;
        log::warn!(
            "Shutdown deadline exceeded; cancelling {} in-flight upload(s).",
            stuck
        );
        self.shutdown_token.cancel();
        // Brief grace so cancelled futures can record themselves as failed before flush.
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    /// Persist any in-memory retry items so they survive a clean shutdown.
    pub fn flush_retries(&self) {
        let retries = self.retry_list.lock();
        if !retries.is_empty() {
            save_retries(&self.retry_path, &retries);
            log::info!(
                "Flushed {} unfinished retry item(s) to disk.",
                retries.len()
            );
        }
    }

    /// Requeue failed task lists with specified details string.
    async fn requeue_failed(&self, tasks: Vec<FileTask>, detail: String) -> usize {
        if tasks.is_empty() {
            return 0;
        }

        {
            let mut state = self.shared_state.lock();
            let mut batch_state = self.batch_notify_state.lock();
            activate_batch_if_needed(&mut batch_state, &state);
            state.failed_count = state.failed_count.saturating_sub(tasks.len());
            state.total_queued += tasks.len();
            state.queue_size = state.total_queued.saturating_sub(state.processed_count);
            for task in &tasks {
                let attempts = current_attempt_count(&state, &task.path).saturating_add(1);
                state.record_event(task.path.clone(), "pending", Some(detail.clone()), attempts);
                let status = state
                    .folder_statuses
                    .entry(task.watch_path.clone())
                    .or_default();
                status.pending_count += 1;
            }
        }

        let mut queued = 0usize;
        for task in tasks {
            if self.sender.send(task).await.is_ok() {
                queued += 1;
            }
        }
        queued
    }
}

/// Return current epoch timestamp in seconds.
fn unix_timestamp_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Returns true if the current local clock hour falls inside the configured quiet window.
///
/// Wrapping windows (e.g. 23:00 to 06:00) are supported.
fn is_quiet_hour(start: Option<u8>, end: Option<u8>) -> bool {
    let (Some(start), Some(end)) = (start, end) else {
        return false;
    };
    let h = chrono::Local::now().hour() as u8;
    if start <= end {
        h >= start && h < end
    } else {
        // Wrapping window: e.g. 23 → 06
        h >= start || h < end
    }
}

/// Pause worker processing loops until environment checks and manual pauses permit resume.
async fn wait_until_allowed(
    state_ref: &Arc<parking_lot::Mutex<AppState>>,
    policy_ref: &Arc<parking_lot::Mutex<EnvironmentPolicy>>,
) {
    loop {
        let policy = *policy_ref.lock();
        let defer_reason = {
            let state = state_ref.lock();
            if state.paused {
                Some(
                    state
                        .pause_reason
                        .clone()
                        .unwrap_or_else(|| "Paused by user".to_string()),
                )
            } else if policy.pause_on_metered_network && runtime_env::is_metered_connection() {
                Some("Deferred on metered network".to_string())
            } else if policy.pause_on_battery_power && runtime_env::is_on_battery_power() {
                Some("Deferred while on battery power".to_string())
            } else if is_quiet_hour(policy.quiet_hours_start, policy.quiet_hours_end) {
                Some("Deferred during quiet hours".to_string())
            } else {
                None
            }
        };

        if let Some(reason) = defer_reason {
            {
                let mut state = state_ref.lock();
                state.status = "paused".to_string();
                state.pause_reason = Some(reason);
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            continue;
        }

        let mut state = state_ref.lock();
        if state.status == "paused" && !state.paused {
            state.status = "idle".to_string();
            state.pause_reason = None;
        }
        break;
    }
}

#[cfg(test)]
mod tests;
