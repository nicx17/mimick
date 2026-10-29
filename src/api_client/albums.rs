//! Album management: list, create, cache, and add assets to albums.
//!
//! Provides the `get_all_albums`, `get_or_create_album`, and
//! `add_asset_to_album` methods that the upload and library flows
//! depend on. Album metadata is cached in-memory after the first fetch.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use super::errors::{RequestContext, classify_http_issue, classify_network_issue};
use super::{AlbumSummary, AlbumUpdate, ApiIssue, ImmichApiClient, LibraryAlbum};

mod asset_lookup;

/// One asset's result from adding to or removing from an album.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct BulkIdResult {
    pub id: String,
    pub success: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BulkOutcome {
    pub done: usize,
    pub duplicate: usize,
    pub denied: usize,
}

impl BulkOutcome {
    pub fn from_results(results: &[BulkIdResult]) -> Self {
        results.iter().fold(Self::default(), |mut outcome, result| {
            match (result.success, result.error.as_deref()) {
                (true, _) => outcome.done += 1,
                (false, Some("duplicate")) => outcome.duplicate += 1,
                (false, _) => outcome.denied += 1,
            }
            outcome
        })
    }
}

impl ImmichApiClient {
    /// Retrieve all albums from the Immich server, populating the local in-memory cache.
    async fn fetch_all_albums(&self) {
        let _fetch_guard = self.album_fetch_lock.lock().await;
        if *self.albums_fetched.lock().await {
            return;
        }
        self.fetch_all_albums_locked().await;
    }

    /// Inner fetch implementation. Assumes `album_fetch_lock` is held by the
    /// caller and `albums_fetched` is already known to be false.
    async fn fetch_all_albums_locked(&self) {
        let base_url = match self.get_active_url().await {
            Some(u) => u,
            None => {
                log::warn!("Cannot fetch albums: no active URL.");
                self.set_issue(ApiIssue {
                    summary: "Album list is unavailable".to_string(),
                    guidance: "Reconnect to the Immich server before refreshing albums."
                        .to_string(),
                })
                .await;
                return;
            }
        };

        let url = format!("{}/api/albums", base_url);
        let api_key = self.settings.read().api_key.clone();
        log::info!("Fetching album list...");

        let result = self
            .client
            .get(&url)
            .header("x-api-key", &api_key)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(10))
            .send()
            .await;

        self.handle_fetch_albums_response(result).await;
    }

    async fn handle_fetch_albums_response(
        &self,
        result: Result<reqwest::Response, reqwest::Error>,
    ) {
        match result {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(albums) = resp.json::<Vec<AlbumSummary>>().await {
                    let total = albums.len();
                    let (fresh, unique, duplicates) = Self::process_album_summaries(albums);
                    {
                        let mut cache = self.album_cache.lock().await;
                        *cache = fresh;
                    }
                    *self.albums_fetched.lock().await = true;
                    self.clear_issue().await;
                    log::info!(
                        "Cached {} unique album(s) from {} server entries ({} duplicate(s) ignored).",
                        unique,
                        total,
                        duplicates
                    );
                }
            }
            Ok(resp) => {
                log::error!("Failed to fetch albums: {}", resp.status());
                self.set_issue(classify_http_issue(
                    RequestContext::Albums,
                    resp.status().as_u16(),
                    None,
                ))
                .await;
            }
            Err(e) => {
                log::error!("Network error fetching albums: {}", e);
                let mut active = self.active_url.lock().await;
                *active = None;
                self.set_issue(classify_network_issue(RequestContext::Albums, &e))
                    .await;
            }
        }
    }

    fn process_album_summaries(
        albums: Vec<AlbumSummary>,
    ) -> (HashMap<String, String>, usize, usize) {
        let mut fresh: HashMap<String, String> = HashMap::with_capacity(albums.len());
        let mut duplicates = 0usize;
        for album in albums {
            match fresh.get(&album.album_name) {
                Some(existing_id) if existing_id != &album.id => {
                    duplicates += 1;
                    log::warn!(
                        "Duplicate album on server: '{}' has both id {} and {} (keeping first). Future syncs will use the first.",
                        album.album_name,
                        existing_id,
                        album.id
                    );
                }
                Some(_) => {
                    // Same name + same id: server returned an entry twice; ignore silently.
                }
                None => {
                    fresh.insert(album.album_name, album.id);
                }
            }
        }
        let unique = fresh.len();
        (fresh, unique, duplicates)
    }

    pub async fn refresh_album_cache(&self) {
        let _fetch_guard = self.album_fetch_lock.lock().await;
        {
            let mut cache = self.album_cache.lock().await;
            cache.clear();
        }
        *self.albums_fetched.lock().await = false;
        self.fetch_all_albums_locked().await;
    }

    /// Return a snapshot of all cached albums as a list of (albumName, id)
    pub async fn get_all_albums(&self) -> Result<Vec<(String, String)>, String> {
        if !*self.albums_fetched.lock().await {
            self.fetch_all_albums().await;
        }
        if !*self.albums_fetched.lock().await {
            return Err("Failed to fetch albums".to_string());
        }
        let cache = self.album_cache.lock().await;
        Ok(cache
            .iter()
            .map(|(n, id)| (n.clone(), id.clone()))
            .collect())
    }

    /// Create a new album. Returns the new album ID.
    pub async fn create_album(&self, album_name: &str) -> Result<Option<String>, String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let url = format!("{}/api/albums", base_url);
        let api_key = self.settings.read().api_key.clone();

        log::info!("Creating album: '{}'", album_name);

        let body = serde_json::json!({
            "albumName": album_name,
            "description": "Created by Mimick"
        });

        let result = self
            .client
            .post(&url)
            .header("x-api-key", &api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        self.handle_create_album_response(result, album_name).await
    }

    async fn handle_create_album_response(
        &self,
        result: Result<reqwest::Response, reqwest::Error>,
        album_name: &str,
    ) -> Result<Option<String>, String> {
        match result {
            Ok(resp) if resp.status().is_success() => {
                let id = resp
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .and_then(|json| json["id"].as_str().map(String::from));
                if let Some(id_str) = id.as_ref() {
                    let mut cache = self.album_cache.lock().await;
                    cache.insert(album_name.to_string(), id_str.clone());
                }
                self.clear_issue().await;
                log::info!("Album created: '{}' ({:?})", album_name, id);
                Ok(id)
            }
            Ok(resp) => {
                log::error!("Failed to create album '{}': {}", album_name, resp.status());
                self.set_issue(classify_http_issue(
                    RequestContext::AlbumCreate,
                    resp.status().as_u16(),
                    Some(album_name),
                ))
                .await;
                Err(format!("HTTP {}", resp.status()))
            }
            Err(e) => {
                log::error!("Network error creating album '{}': {}", album_name, e);
                self.set_issue(classify_network_issue(RequestContext::AlbumCreate, &e))
                    .await;
                Err(e.to_string())
            }
        }
    }

    /// Return an existing album ID or create a new one.
    pub async fn get_or_create_album(&self, album_name: &str) -> Result<Option<String>, String> {
        if !*self.albums_fetched.lock().await {
            self.fetch_all_albums().await;
        }
        {
            let cache = self.album_cache.lock().await;
            if let Some(id) = cache.get(album_name) {
                log::debug!("Album found in cache: '{}' ({})", album_name, id);
                return Ok(Some(id.clone()));
            }
        }
        if !*self.albums_fetched.lock().await {
            return Err("Cannot fetch albums to verify existence".to_string());
        }

        let create_lock = {
            let mut locks = self.album_create_locks.lock().await;
            locks
                .entry(album_name.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = create_lock.lock().await;

        {
            let cache = self.album_cache.lock().await;
            if let Some(id) = cache.get(album_name) {
                return Ok(Some(id.clone()));
            }
        }

        self.create_album(album_name).await
    }

    /// Return an existing album ID without creating a new album as a side effect.
    pub async fn get_album_id_if_exists(&self, album_name: &str) -> Result<Option<String>, String> {
        if !*self.albums_fetched.lock().await {
            self.fetch_all_albums().await;
        }
        if !*self.albums_fetched.lock().await {
            return Err("Cannot fetch albums to verify existence".to_string());
        }

        let cache = self.album_cache.lock().await;
        Ok(cache.get(album_name).cloned())
    }

    pub async fn resolve_album_by_name(
        &self,
        album_name: &str,
        force_refresh: bool,
    ) -> Result<Option<String>, String> {
        if force_refresh {
            self.refresh_album_cache().await;
        }
        self.get_or_create_album(album_name).await
    }

    /// Add a list of asset IDs to an album.
    pub async fn add_assets_to_album(&self, album_id: &str, asset_ids: &[String]) -> bool {
        if album_id.is_empty() || asset_ids.is_empty() {
            log::warn!("Skipping add_assets_to_album: missing ID or assets.");
            return false;
        }

        let base_url = match self.get_active_url().await {
            Some(u) => u,
            None => return false,
        };

        let url = format!("{}/api/albums/{}/assets", base_url, album_id);
        let api_key = self.settings.read().api_key.clone();
        let body = serde_json::json!({ "ids": asset_ids });

        log::info!(
            "Adding {} asset(s) to album '{}'",
            asset_ids.len(),
            album_id
        );

        let result = self
            .client
            .put(&url)
            .header("x-api-key", &api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        self.handle_add_assets_response(result, album_id).await
    }

    async fn handle_add_assets_response(
        &self,
        result: Result<reqwest::Response, reqwest::Error>,
        album_id: &str,
    ) -> bool {
        match result {
            Ok(resp) if resp.status().is_success() => {
                log::info!("Assets added to album successfully.");
                self.clear_issue().await;
                true
            }
            Ok(resp) => {
                log::error!("Failed to add assets to album: {}", resp.status());
                self.set_issue(classify_http_issue(
                    RequestContext::AlbumAssign,
                    resp.status().as_u16(),
                    Some(album_id),
                ))
                .await;
                false
            }
            Err(e) => {
                log::error!("Network error adding assets to album: {}", e);
                self.set_issue(classify_network_issue(RequestContext::AlbumAssign, &e))
                    .await;
                false
            }
        }
    }

    /// Count the albums that currently contain the given asset on the
    /// server. Drives the trash-vs-remove-from-album decision when mirroring
    /// a local deletion: an asset in multiple albums should only be unlinked
    /// from the linked album, never destroyed.
    pub async fn count_albums_for_asset(&self, asset_id: &str) -> Option<usize> {
        let base_url = self.get_active_url().await?;
        let url = format!("{}/api/albums?assetId={}", base_url, asset_id);
        let api_key = self.settings.read().api_key.clone();
        match self
            .client
            .get(&url)
            .header("x-api-key", &api_key)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let albums: Vec<serde_json::Value> = resp.json().await.ok()?;
                Some(albums.len())
            }
            Ok(resp) => {
                log::warn!(
                    "count_albums_for_asset({}) returned {}",
                    asset_id,
                    resp.status()
                );
                None
            }
            Err(err) => {
                log::warn!("count_albums_for_asset({}) failed: {}", asset_id, err);
                None
            }
        }
    }

    /// Remove assets from an album without trashing them on the server. Used
    /// when an asset is referenced from more than one watch folder — we want
    /// to mirror the local deletion's album side, not destroy the asset.
    pub async fn remove_assets_from_album(&self, album_id: &str, asset_ids: &[String]) -> bool {
        if album_id.is_empty() || asset_ids.is_empty() {
            return false;
        }
        let base_url = match self.get_active_url().await {
            Some(u) => u,
            None => return false,
        };
        let url = format!("{}/api/albums/{}/assets", base_url, album_id);
        let api_key = self.settings.read().api_key.clone();
        let body = serde_json::json!({ "ids": asset_ids });
        match self
            .client
            .delete(&url)
            .header("x-api-key", &api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                log::info!(
                    "Removed {} asset(s) from album '{}' (asset preserved on server)",
                    asset_ids.len(),
                    album_id
                );
                true
            }
            Ok(resp) => {
                log::warn!("Remove-from-album returned {}", resp.status());
                false
            }
            Err(err) => {
                log::warn!("Remove-from-album request failed: {}", err);
                false
            }
        }
    }

    pub async fn update_album(&self, album_id: &str, update: &AlbumUpdate) -> Result<(), String> {
        let body = serde_json::to_value(update).map_err(|err| err.to_string())?;
        let path = format!("/api/albums/{album_id}");
        self.send_json(reqwest::Method::PATCH, &path, Some(body))
            .await
            .map(|_| ())
    }
}

impl ImmichApiClient {
    pub async fn add_to_album(
        &self,
        album_id: &str,
        asset_ids: &[String],
    ) -> Result<Vec<BulkIdResult>, String> {
        self.album_assets_request(reqwest::Method::PUT, album_id, asset_ids)
            .await
    }

    pub async fn remove_from_album(
        &self,
        album_id: &str,
        asset_ids: &[String],
    ) -> Result<Vec<BulkIdResult>, String> {
        self.album_assets_request(reqwest::Method::DELETE, album_id, asset_ids)
            .await
    }

    async fn album_assets_request(
        &self,
        method: reqwest::Method,
        album_id: &str,
        asset_ids: &[String],
    ) -> Result<Vec<BulkIdResult>, String> {
        let path = format!("/api/albums/{album_id}/assets");
        let body = serde_json::json!({ "ids": asset_ids });
        let resp = self.send_json(method, &path, Some(body)).await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    /// Delete the album; its photos stay in the library. Forgets it in the name cache too.
    pub async fn delete_album(&self, album_id: &str) -> Result<(), String> {
        let path = format!("/api/albums/{album_id}");
        self.send_json(reqwest::Method::DELETE, &path, None).await?;
        self.refresh_album_cache().await;
        Ok(())
    }

    pub async fn albums_containing(&self, asset_id: &str) -> Result<Vec<LibraryAlbum>, String> {
        let path = format!("/api/albums?assetId={asset_id}");
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        let mut albums: Vec<LibraryAlbum> = resp.json().await.map_err(|err| err.to_string())?;
        albums.sort_by_key(|album| album.album_name.to_lowercase());
        Ok(albums)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulk_outcome_counts_each_reason() {
        let results: Vec<BulkIdResult> = serde_json::from_value(serde_json::json!([
            { "id": "a", "success": true },
            { "id": "b", "success": false, "error": "duplicate" },
            { "id": "c", "success": false, "error": "no_permission" },
            { "id": "d", "success": false, "error": "not_found" }
        ]))
        .unwrap();
        assert_eq!(
            BulkOutcome::from_results(&results),
            BulkOutcome {
                done: 1,
                duplicate: 1,
                denied: 2
            }
        );
    }
}
