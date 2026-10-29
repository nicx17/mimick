//! Stores persistent status snapshots to restore basic UI state across application launches.
//!
//! Tracks the current sync progress, folder-level status, error history,
//! and recent queue events. The state file is written atomically so a
//! crash mid-write never corrupts the snapshot.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

mod transfer_snapshot;

/// Represents a rolling queue/event status used by the settings window inspector.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct QueueEvent {
    /// File path where the event occurred.
    pub path: String,
    /// Outcome label of this synchronization event.
    pub status: String,
    /// Detailed warning or cause description if failed.
    #[serde(default)]
    pub detail: Option<String>,
    /// Number of sync attempts performed.
    #[serde(default)]
    pub attempts: u32,
    /// Epoch timestamp of this event.
    pub timestamp: f64,
}

/// Represents the status of an individual watch folder.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct FolderSyncStatus {
    /// Epoch timestamp of the last folder synchronization sweep.
    pub last_sync_at: Option<f64>,
    /// Number of local files waiting in queue for this folder.
    pub pending_count: usize,
    /// Target Immich album mapped to this folder.
    pub target_album: Option<String>,
    /// Error warning recorded on last folder sync failure.
    pub last_error: Option<String>,
}

/// Target movement direction of active transfers.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub enum TransferDirection {
    /// Files uploaded from local directories to Immich server.
    #[default]
    Upload,
    /// Files downloaded from Immich server to local directories.
    Download,
}

/// Snapshot describing currently executing and active file transfers.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct TransferSnapshot {
    /// True if a transfer batch session is currently running.
    #[serde(default)]
    pub active: bool,
    /// Direction of current active transfer session.
    #[serde(default)]
    pub direction: TransferDirection,
    /// Total bytes processed in current transfer task.
    #[serde(default)]
    pub current_bytes: u64,
    /// Total bytes target across current session.
    #[serde(default)]
    pub total_bytes: Option<u64>,
    /// Estimated sum of all item file sizes in the active batch.
    #[serde(default)]
    pub session_total_bytes: u64,
    /// Sum of all bytes successfully sent or received in this session.
    #[serde(default)]
    pub session_transferred_bytes: u64,
    /// Total target bytes of previously finished items in the batch.
    #[serde(default)]
    pub completed_total_bytes: u64,
    /// Total bytes completed of previously finished items in the batch.
    #[serde(default)]
    pub completed_transferred_bytes: u64,
    /// Instant speed in bytes per second.
    #[serde(default)]
    pub instant_bps: f64,
    /// Average session speed in bytes per second.
    #[serde(default)]
    pub session_avg_bps: f64,
    /// Epoch timestamp when the transfer session was started.
    #[serde(default)]
    pub session_started_at: Option<f64>,
    /// Snapshot count of completed session bytes.
    #[serde(default)]
    pub session_bytes_done: u64,
    /// Label representing the file currently being processed.
    #[serde(default)]
    pub active_item_label: Option<String>,
    /// Active network route segment used for connection.
    #[serde(default)]
    pub active_route: Option<String>,
    /// Average bytes per second achieved on the last upload batch.
    #[serde(default)]
    pub last_upload_avg_bps: f64,
    /// Average bytes per second achieved on the last download batch.
    #[serde(default)]
    pub last_download_avg_bps: f64,
    /// Tick epoch timestamp used to recalculate instant speed.
    #[serde(skip)]
    pub last_tick_at: Option<f64>,
    /// Bytes processed on the last instant speed tick check.
    #[serde(skip)]
    pub last_tick_bytes: u64,
    /// Number of active parallel upload workers.
    #[serde(skip)]
    pub active_uploads: usize,
    /// Number of active parallel download workers.
    #[serde(skip)]
    pub active_downloads: usize,
    /// Track of active item transferred bytes maps.
    #[serde(skip)]
    pub active_item_bytes: HashMap<String, u64>,
    /// Track of active item total sizes maps.
    #[serde(skip)]
    pub active_item_totals: HashMap<String, u64>,
}

/// Contains shared progress counters exposed to the settings window.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppState {
    /// Number of items remaining in background queue.
    pub queue_size: usize,
    /// Total number of items scheduled this session.
    pub total_queued: usize,
    /// Number of processed items during current session.
    pub processed_count: usize,
    /// Number of items failing transmission.
    #[serde(default)]
    pub failed_count: usize,
    /// In-flight worker count — not persisted to disk.
    #[serde(skip)]
    pub active_workers: usize,
    /// Relative path of file currently undergoing transmission.
    pub current_file: Option<String>,
    /// Global application status label.
    pub status: String,
    /// Percentage progress indicator of active batch.
    pub progress: u8,
    /// Epoch timestamp of this state snapshot.
    pub timestamp: f64,
    /// True if all processing queues are manually paused.
    #[serde(default)]
    pub paused: bool,
    /// User or system reason why transmission is suspended.
    #[serde(default)]
    pub pause_reason: Option<String>,
    /// Number of watched folders configured.
    #[serde(default)]
    pub watched_folder_count: usize,
    /// Connection route label of the current session.
    #[serde(default)]
    pub active_server_route: Option<String>,
    /// Epoch timestamp when the last sweep successfully finished.
    #[serde(default)]
    pub last_successful_sync_at: Option<f64>,
    /// Diagnostic warning of the last critical error.
    #[serde(default)]
    pub last_error: Option<String>,
    /// Troubleshooting guidance instructions for last_error.
    #[serde(default)]
    pub last_error_guidance: Option<String>,
    /// Filename of the last successfully processed asset.
    #[serde(default)]
    pub last_completed_file: Option<String>,
    /// Count of diagnostic bundles exported.
    #[serde(default)]
    pub diagnostics_exports: usize,
    /// Historic rolling logs of recent queue event results.
    #[serde(default)]
    pub recent_events: Vec<QueueEvent>,
    /// Current sync states mapped by watched folder paths.
    #[serde(default)]
    pub folder_statuses: std::collections::HashMap<String, FolderSyncStatus>,
    /// Active metrics representing ongoing network transfers.
    #[serde(default)]
    pub transfer: TransferSnapshot,
    /// Total completed batches of uploads executed.
    #[serde(skip)]
    pub completed_upload_batches: u64,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            queue_size: 0,
            total_queued: 0,
            processed_count: 0,
            failed_count: 0,
            active_workers: 0,
            current_file: None,
            status: "idle".to_string(),
            progress: 0,
            timestamp: 0.0,
            paused: false,
            pause_reason: None,
            watched_folder_count: 0,
            active_server_route: None,
            last_successful_sync_at: None,
            last_error: None,
            last_error_guidance: None,
            last_completed_file: None,
            diagnostics_exports: 0,
            recent_events: Vec::new(),
            folder_statuses: std::collections::HashMap::new(),
            transfer: TransferSnapshot::default(),
            completed_upload_batches: 0,
        }
    }
}

impl AppState {
    const MAX_EVENTS: usize = 80;

    /// Add or update a rolling queue event entry in history.
    pub fn record_event(
        &mut self,
        path: impl Into<String>,
        status: impl Into<String>,
        detail: Option<String>,
        attempts: u32,
    ) {
        let path = path.into();
        let status = status.into();
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        if let Some(existing) = self.recent_events.iter_mut().find(|evt| evt.path == path) {
            existing.status = status;
            existing.detail = detail;
            existing.attempts = attempts;
            existing.timestamp = timestamp;
        } else {
            self.recent_events.push(QueueEvent {
                path,
                status,
                detail,
                attempts,
                timestamp,
            });
        }

        self.recent_events
            .sort_by(|a, b| b.timestamp.total_cmp(&a.timestamp));
        self.recent_events.truncate(Self::MAX_EVENTS);
    }

    /// Discard in-progress transfer states at application boot time.
    pub fn reset_runtime_state(&mut self) {
        self.transfer.reset_runtime();
    }
}

/// Return current epoch timestamp.
fn unix_timestamp_now() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Helper that reads and writes redacted app states to disk.
pub struct StateManager {
    /// Resolved absolute path of status JSON cache.
    state_file: PathBuf,
}

impl StateManager {
    /// Point at the standard `status.json` cache path used by Mimick.
    pub fn new() -> Self {
        let cache_dir = crate::profile::cache_dir().unwrap_or_else(|| {
            std::path::PathBuf::from("~/.cache").join(crate::profile::dir_segment())
        });

        let state_file = cache_dir.join("status.json");
        Self { state_file }
    }

    /// Persist a status snapshot using a write-then-rename pattern.
    pub fn write_state(&self, mut state: AppState) {
        state.timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        if let Some(parent) = self.state_file.parent() {
            let _ = fs::create_dir_all(parent);
        }

        if let Ok(content) = serde_json::to_string(&state) {
            let unique_ext = format!(
                "tmp.{}",
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            let tmp_file = self.state_file.with_extension(unique_ext);
            if fs::write(&tmp_file, &content).is_ok() {
                if fs::rename(&tmp_file, &self.state_file).is_ok() {
                    log::debug!(
                        "State written: status={} progress={} processed={}/{}",
                        state.status,
                        state.progress,
                        state.processed_count,
                        state.total_queued
                    );
                } else {
                    let _ = fs::remove_file(&tmp_file); // cleanup on fail
                    log::warn!("Failed to atomically rename state file");
                }
            } else {
                log::warn!("Failed to write temp state file");
            }
        }
    }

    /// Load the last saved state or return defaults when no cache exists.
    pub fn read_state(&self) -> AppState {
        match fs::read_to_string(&self.state_file) {
            Ok(content) => match serde_json::from_str::<AppState>(&content) {
                Ok(mut state) => {
                    state.reset_runtime_state();
                    log::debug!("State read: status={}", state.status);
                    state
                }
                Err(e) => {
                    log::warn!("Failed to parse state file: {}", e);
                    AppState::default()
                }
            },
            Err(_) => AppState::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_app_state_default() {
        let state = AppState::default();
        assert_eq!(state.queue_size, 0);
        assert_eq!(state.status, "idle");
        assert_eq!(state.progress, 0);
        assert_eq!(state.watched_folder_count, 0);
        assert!(state.active_server_route.is_none());
        assert!(state.last_successful_sync_at.is_none());
        assert!(state.last_error_guidance.is_none());
        assert!(!state.transfer.active);
    }

    #[test]
    fn test_state_manager_write_read() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("status.json");

        // We override the state_file manually for testing
        let manager = StateManager {
            state_file: file_path.clone(),
        };

        let state = AppState {
            status: "syncing".to_string(),
            progress: 50,
            ..AppState::default()
        };

        manager.write_state(state.clone());

        assert!(file_path.exists());

        let read_state = manager.read_state();
        assert_eq!(read_state.status, "syncing");
        assert_eq!(read_state.progress, 50);
        assert!(!read_state.transfer.active);
    }

    #[test]
    fn test_state_manager_preserves_health_dashboard_fields() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("status.json");
        let manager = StateManager {
            state_file: file_path,
        };

        let state = AppState {
            watched_folder_count: 3,
            active_server_route: Some("LAN".into()),
            last_successful_sync_at: Some(1234.5),
            last_error: Some("Immich rejected the API key".into()),
            last_error_guidance: Some("Update the API key in Settings.".into()),
            ..AppState::default()
        };

        manager.write_state(state);
        let read_state = manager.read_state();

        assert_eq!(read_state.watched_folder_count, 3);
        assert_eq!(read_state.active_server_route.as_deref(), Some("LAN"));
        assert_eq!(read_state.last_successful_sync_at, Some(1234.5));
        assert_eq!(
            read_state.last_error.as_deref(),
            Some("Immich rejected the API key")
        );
        assert_eq!(
            read_state.last_error_guidance.as_deref(),
            Some("Update the API key in Settings.")
        );
        assert!(!read_state.transfer.active);
    }

    #[test]
    fn test_record_event_updates_existing_entry() {
        let mut state = AppState::default();
        state.record_event("/tmp/a.jpg", "pending", Some("queued".into()), 1);
        state.record_event("/tmp/a.jpg", "failed", Some("retry".into()), 2);

        assert_eq!(state.recent_events.len(), 1);
        assert_eq!(state.recent_events[0].status, "failed");
        assert_eq!(state.recent_events[0].attempts, 2);
        assert_eq!(state.recent_events[0].detail.as_deref(), Some("retry"));
    }

    #[test]
    fn test_record_event_truncates_history() {
        let mut state = AppState::default();
        for i in 0..100 {
            state.record_event(format!("/tmp/{i}.jpg"), "pending", None, 1);
        }

        assert_eq!(state.recent_events.len(), 80);
    }

    #[test]
    fn test_transfer_snapshot_speed_updates() {
        let mut transfer = TransferSnapshot::default();
        transfer.register_item(
            TransferDirection::Upload,
            "file.jpg",
            Some(1_000),
            Some("file.jpg".into()),
            Some("LAN".into()),
        );
        let start = unix_timestamp_now() - 1.0;
        transfer.session_started_at = Some(start);
        transfer.last_tick_at = Some(start);
        transfer.last_tick_bytes = 0;
        transfer.update_item_bytes(
            TransferDirection::Upload,
            "file.jpg",
            512,
            Some("LAN".into()),
        );

        assert!(transfer.active);
        assert_eq!(transfer.current_bytes, 512);
        assert!(transfer.instant_bps > 0.0);
        assert!(transfer.session_avg_bps > 0.0);

        transfer.finish_item(TransferDirection::Upload, "file.jpg", Some("LAN".into()));
        assert!(!transfer.active);
    }
}
