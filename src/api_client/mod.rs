//! Integrates with the Immich API, handles connectivity failover, and provides album/cache helpers.
//!
//! The client probes both an internal (LAN) and external (WAN) URL and
//! locks onto whichever responds first. Submodules split the API surface
//! by domain: `albums`, `library`, `search`, `upload`, and `errors`.
//! All HTTP requests share a single `reqwest::Client` connection pool.

use parking_lot::RwLock;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};

mod albums;
pub use albums::{BulkIdResult, BulkOutcome};
mod download;
pub use download::DownloadArchive;
mod errors;
mod features;
pub use features::LibraryFeatures;
mod folders;
mod library;
pub use library::DateChange;
mod map;
pub use map::{MAP_USER_AGENT, MapMarker, Place};
mod ocr;
pub use ocr::OcrBox;
mod partners;
pub use partners::{Partner, PartnerDirection};
mod people;
pub use people::PersonUpdate;
mod search;
mod sharing;
pub use sharing::AlbumRole;
mod shared_links;
pub use shared_links::{SharedLink, SharedLinkOptions, SharedLinkTarget, share_url};
mod models;
mod suggestions;
mod tags;
mod timeline;
mod trash;
mod upload;
pub(super) use models::SearchResponse;
pub use models::*;
mod upload_helpers;
pub(crate) use upload_helpers::local_timezone_name;

#[cfg(test)]
use errors::{RequestContext, classify_http_issue};
#[cfg(test)]
use upload_helpers::{
    looks_like_iana_timezone, mime_for_path, normalize_file_timestamps, unix_to_utc_iso8601,
};

pub type TransferProgressCallback = Arc<dyn Fn(u64, Option<u64>) + Send + Sync>;

/// Represents an actionable API or connection issue encountered during operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiIssue {
    /// Concise summary of the encountered issue.
    pub summary: String,
    /// Guidance text showing the user how to resolve it.
    pub guidance: String,
}

/// In-memory API client connection URLs and configuration settings.
#[derive(Debug, Clone)]
pub(super) struct ApiClientSettings {
    /// Server URL for local network connections.
    pub(super) internal_url: String,
    /// Server URL for external network connections.
    pub(super) external_url: String,
    /// Immich authorization API key.
    pub(super) api_key: String,
}

/// Simplified album summary response from Immich.
#[derive(Debug, serde::Deserialize)]
pub(super) struct AlbumSummary {
    /// Album identifier.
    pub(super) id: String,
    /// Name of the album.
    #[serde(rename = "albumName")]
    pub(super) album_name: String,
}

/// Asynchronous Immich API client with failover and request serialization.
pub struct ImmichApiClient {
    /// Internal HTTP client instance.
    pub client: Client,
    /// Configuration settings wrapper.
    settings: RwLock<ApiClientSettings>,
    /// The currently active base URL, selected by the last successful connectivity check.
    pub active_url: Mutex<Option<String>>,
    /// Most recent actionable API/client problem, used for the dashboard and diagnostics.
    last_issue: Mutex<Option<ApiIssue>>,
    /// Caches album names to album IDs to avoid repeated list/create API calls.
    album_cache: Mutex<HashMap<String, String>>,
    /// Per-album-name async locks that serialize concurrent get-or-create calls
    /// for the *same* name, preventing duplicate-album creation under load.
    album_create_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// Serializes `fetch_all_albums` so concurrent callers don't all hit the
    /// network and race writes into the cache.
    album_fetch_lock: Mutex<()>,
    /// Serializes `check_connection` so concurrent callers collapse into one
    /// connectivity probe instead of each issuing their own LAN+WAN pings.
    connection_check_lock: Mutex<()>,
    /// Timestamp of the most recent successful connectivity probe. Used to
    /// coalesce concurrent burst callers without suppressing periodic re-checks.
    last_successful_check: Mutex<Option<Instant>>,
    /// Flag indicating whether the album list has been successfully fetched in this session.
    albums_fetched: Mutex<bool>,
    /// Semaphore guarding maximum concurrent thumbnail downloads.
    thumbnail_semaphore: Arc<Semaphore>,
    preview_semaphore: Arc<Semaphore>,
}

const THUMBNAIL_DOWNLOADS: usize = 16;
const PREVIEW_DOWNLOADS: usize = 8;

impl ImmichApiClient {
    /// Initialize a new ImmichApiClient.
    pub fn new(internal_url: String, external_url: String, api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .pool_max_idle_per_host(THUMBNAIL_DOWNLOADS + PREVIEW_DOWNLOADS) // reuse connections across thumbnail bursts
            .pool_idle_timeout(Duration::from_secs(90)) // drop idle connections after 90s
            .build()
            .unwrap_or_default();

        let int = internal_url.trim_end_matches('/').to_string();
        let ext = external_url.trim_end_matches('/').to_string();

        log::debug!(
            "ImmichApiClient created: internal={}, external={}",
            int,
            ext
        );

        Self {
            client,
            settings: RwLock::new(ApiClientSettings {
                internal_url: int,
                external_url: ext,
                api_key,
            }),
            active_url: Mutex::new(None),
            last_issue: Mutex::new(None),
            album_cache: Mutex::new(HashMap::new()),
            album_create_locks: Mutex::new(HashMap::new()),
            album_fetch_lock: Mutex::new(()),
            connection_check_lock: Mutex::new(()),
            last_successful_check: Mutex::new(None),
            albums_fetched: Mutex::new(false),
            thumbnail_semaphore: Arc::new(Semaphore::new(THUMBNAIL_DOWNLOADS)),
            preview_semaphore: Arc::new(Semaphore::new(PREVIEW_DOWNLOADS)),
        }
    }

    /// Send a JSON request to `path` and fail on a non-success status.
    pub(super) async fn send_json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<reqwest::Response, String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let mut request = self
            .client
            .request(method, format!("{}{}", base_url, path))
            .header("x-api-key", self.settings_snapshot().api_key)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(15));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let resp = request.send().await.map_err(|err| err.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        self.clear_issue().await;
        Ok(resp)
    }

    /// Retrieve the route label (LAN/WAN) for the currently active connection URL.
    pub async fn active_route_label(&self) -> Option<String> {
        let active = self.active_url.lock().await.clone()?;
        Some(self.route_label_for_url(&active))
    }

    /// Retrieve the most recently recorded API issue.
    pub async fn latest_issue(&self) -> Option<ApiIssue> {
        self.last_issue.lock().await.clone()
    }

    /// Update in-memory configuration settings and reset connection state.
    pub async fn update_settings(
        &self,
        internal_url: String,
        external_url: String,
        api_key: String,
    ) {
        {
            let mut settings = self.settings.write();
            settings.internal_url = internal_url.trim_end_matches('/').to_string();
            settings.external_url = external_url.trim_end_matches('/').to_string();
            settings.api_key = api_key;
        }

        *self.active_url.lock().await = None;
        self.refresh_album_cache().await;
        self.clear_issue().await;
    }

    /// Set the last encountered API issue.
    pub(super) async fn set_issue(&self, issue: ApiIssue) {
        *self.last_issue.lock().await = Some(issue);
    }

    /// Clear the active API issue.
    pub(super) async fn clear_issue(&self) {
        *self.last_issue.lock().await = None;
    }

    /// Retrieve a snapshot copy of the current configuration settings.
    pub(super) fn settings_snapshot(&self) -> ApiClientSettings {
        self.settings.read().clone()
    }

    /// Retrieve the LAN/WAN/Custom route label matching a specific URL.
    pub(super) fn route_label_for_url(&self, url: &str) -> String {
        let settings = self.settings.read().clone();
        let trimmed = url.trim_end_matches('/');
        if !settings.internal_url.is_empty() && trimmed == settings.internal_url {
            "LAN".to_string()
        } else if !settings.external_url.is_empty() && trimmed == settings.external_url {
            "WAN".to_string()
        } else {
            "Custom".to_string()
        }
    }

    /// Determine which base URL to use, preferring the internal address when reachable.
    pub async fn check_connection(&self) -> bool {
        let _check_guard = self.connection_check_lock.lock().await;

        if self.active_url.lock().await.is_some()
            && let Some(when) = *self.last_successful_check.lock().await
            && when.elapsed() < Duration::from_secs(1)
        {
            return true;
        }

        log::debug!("Checking connectivity...");
        let settings = self.settings.read().clone();
        let was_active = self.active_url.lock().await.clone();

        for url in [&settings.internal_url, &settings.external_url] {
            if self.try_activate_url(url, was_active.is_none()).await {
                return true;
            }
        }

        *self.active_url.lock().await = None;
        if was_active.is_some() {
            log::error!("Could not connect to Immich server.");
        } else {
            log::debug!("Server still unreachable.");
        }
        self.set_issue(ApiIssue {
            summary: "Could not reach the Immich server".to_string(),
            guidance: "Check the LAN/WAN URLs, confirm the server is running, and verify your network connection."
                .to_string(),
        })
        .await;
        false
    }

    async fn try_activate_url(&self, url: &str, was_offline: bool) -> bool {
        if !self.ping_url(url).await {
            return false;
        }
        *self.active_url.lock().await = Some(url.to_string());
        *self.last_successful_check.lock().await = Some(Instant::now());
        self.clear_issue().await;
        let label = self.route_label_for_url(url);
        if was_offline {
            log::info!("Connected via {}: {}", label, url);
        } else {
            log::debug!("Connected via {}: {}", label, url);
        }
        true
    }

    /// Ping a specific Immich base URL and validate that it returns a real `pong` response.
    pub async fn ping_url(&self, url: &str) -> bool {
        if url.is_empty() {
            return false;
        }
        let endpoint = format!("{}/api/server/ping", url.trim_end_matches('/'));
        log::debug!("Pinging: {}", endpoint);

        match self
            .client
            .get(&endpoint)
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            Ok(resp) if resp.status().as_u16() == 200 => {
                match resp.json::<serde_json::Value>().await {
                    Ok(json)
                        if json["res"].as_str().map(str::to_lowercase) == Some("pong".into()) =>
                    {
                        log::debug!("Ping success: {}", endpoint);
                        true
                    }
                    _ => {
                        log::warn!("Ping failed (not a valid Immich response): {}", endpoint);
                        false
                    }
                }
            }
            Ok(resp) => {
                log::warn!("Ping failed ({}): {}", resp.status(), endpoint);
                false
            }
            Err(e) => {
                log::warn!("Ping error ({}): {}", e, endpoint);
                false
            }
        }
    }

    /// Return the cached active base URL, resolving connectivity first if needed.
    pub(super) async fn get_active_url(&self) -> Option<String> {
        {
            let active = self.active_url.lock().await;
            if active.is_some() {
                return active.clone();
            }
        }
        if self.check_connection().await {
            let active = self.active_url.lock().await;
            return active.clone();
        }
        None
    }
}

#[cfg(test)]
mod tests;
