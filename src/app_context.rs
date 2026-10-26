//! Consolidated application context passed through the UI and background services.
//!
//! Replaces the growing list of individual `Arc<T>` parameters that were previously
//! threaded through `build_settings_window()` and `open_settings_if_needed()`.

use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

use crate::api_client::ImmichApiClient;
use crate::config::{Config, DateHeadings, WatchPathEntry};
use crate::library::state::LibraryState;
use crate::library::thumbnail_cache::ThumbnailCache;
use crate::monitor::MonitorHandle;
use crate::queue_manager::QueueManager;
use crate::state_manager::AppState;
use crate::sync_index::ShardedSyncIndex;

/// TTL'd set of paths Mimick itself just modified, used to suppress the
/// filesystem events those modifications cause.
#[derive(Default)]
pub struct RecentSelfPaths {
    /// Mutex-wrapped map of self-modified paths to the instant they were modified.
    inner: Mutex<HashMap<String, Instant>>,
}

impl RecentSelfPaths {
    /// Time-to-live after which self-modified paths are expired.
    const TTL: Duration = Duration::from_secs(60);

    /// Record a path as being modified by the application itself.
    pub fn mark(&self, path: &str) {
        let mut map = self.inner.lock();
        map.retain(|_, t| t.elapsed() < Self::TTL);
        map.insert(path.to_string(), Instant::now());
    }

    /// Check if a path is within its TTL and consume the record if present.
    pub fn consume(&self, path: &str) -> bool {
        let mut map = self.inner.lock();
        map.retain(|_, t| t.elapsed() < Self::TTL);
        map.remove(path).is_some()
    }
}

/// Per-album reconcile lock. Prevents the periodic poller and a manual sync
/// click from racing each other into duplicate downloads / duplicate trash.
#[derive(Default)]
pub struct ReconcileLocks {
    /// Track of album IDs currently undergoing reconciliation.
    inner: Mutex<std::collections::HashSet<String>>,
}

impl ReconcileLocks {
    /// Attempt to acquire a lock for the given album, returning a guard on success.
    pub fn try_acquire(self: &Arc<Self>, album_id: String) -> Option<ReconcileGuard> {
        let mut set = self.inner.lock();
        if !set.insert(album_id.clone()) {
            return None;
        }
        Some(ReconcileGuard {
            locks: self.clone(),
            album_id,
        })
    }
}

/// Active lock guard that frees the album lock when dropped.
pub struct ReconcileGuard {
    /// Reference back to the parent locks list.
    locks: Arc<ReconcileLocks>,
    /// Unique identifier of the locked album.
    album_id: String,
}

impl Drop for ReconcileGuard {
    fn drop(&mut self) {
        self.locks.inner.lock().remove(&self.album_id);
    }
}

/// Two-tick deletion confirmation. For mass deletions, require the same
/// asset to be missing across two consecutive reconciler observations
/// before trashing — defends against transient server-side stale reads.
#[derive(Default)]
pub struct PendingDeletions {
    /// Number of observations recorded per asset ID.
    inner: Mutex<HashMap<String, u32>>,
}

impl PendingDeletions {
    /// Number of consecutive confirmations required to execute trashing.
    pub const REQUIRED_CONFIRMATIONS: u32 = 2;

    /// Increment the verification count for an asset and return if target threshold is reached.
    pub fn confirm(&self, key: &str) -> bool {
        let mut map = self.inner.lock();
        let entry = map.entry(key.to_string()).or_insert(0);
        *entry += 1;
        *entry >= Self::REQUIRED_CONFIRMATIONS
    }

    /// Clear deletion confirmation history for a specific asset ID.
    pub fn clear(&self, key: &str) {
        self.inner.lock().remove(key);
    }
}

/// Shared application context holding all dependency handles that UI and background
/// tasks need. Wrapped in `Arc` at construction time so it can be cloned cheaply.
pub struct AppContext {
    /// Thread-safe configuration data.
    pub config: Arc<RwLock<Config>>,
    /// Thread-safe application state metrics.
    pub state: Arc<Mutex<AppState>>,
    /// Shared Immich API client handle.
    pub api_client: Arc<ImmichApiClient>,
    /// System upload and download worker queue coordinator.
    pub queue_manager: Arc<QueueManager>,
    /// Native file monitor handle.
    pub monitor_handle: Arc<MonitorHandle>,
    /// Concurrent database state check index.
    pub sync_index: Arc<ShardedSyncIndex>,
    /// Thread-safe active watched paths lists.
    pub live_watch_paths: Arc<Mutex<Vec<WatchPathEntry>>>,
    /// Channel sender to trigger manual synchronization catch-ups.
    pub sync_now_tx: UnboundedSender<()>,
    /// LRU thumbnail image cache coordinator.
    pub thumbnail_cache: Arc<ThumbnailCache>,
    /// GTK library view loading state container.
    pub library_state: Arc<Mutex<LibraryState>>,
    /// Date headings mode; the open library window follows changes.
    pub date_headings: tokio::sync::watch::Sender<DateHeadings>,
    /// Authenticated user unique identifier, fetched at bootstrap.
    pub current_user_id: Arc<Mutex<Option<String>>>,
    /// Tracking lists of deletions requested by Mimick itself.
    pub expected_self_deletions: Arc<RecentSelfPaths>,
    /// Tracking lists of files downloaded by Mimick itself.
    pub expected_self_downloads: Arc<RecentSelfPaths>,
    /// Active locks to prevent multi-sync conflicts.
    pub reconcile_locks: Arc<ReconcileLocks>,
    /// Deletions waiting to be confirmed across sync sweeps.
    pub pending_deletions: Arc<PendingDeletions>,
}
