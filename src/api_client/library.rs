//! Library browsing: fetch albums, thumbnails, asset details, download originals, delete.
//!
//! Implements the read-side API surface used by the library window:
//! paginated asset lists, thumbnail downloads (with semaphore-limited
//! concurrency), EXIF metadata, and original-file downloads. Asset
//! deletion sends items to Immich trash rather than permanent removal.

use std::time::Duration;

use super::errors::{RequestContext, classify_http_issue, classify_network_issue};
use super::{
    AssetDetails, AssetUpdate, ImmichApiClient, LibraryAlbum, LibraryAsset, SortOrder,
    ThumbnailSize, TransferProgressCallback,
};

/// Body for `DELETE /api/assets`; `force` skips the trash.
fn delete_request_body(asset_ids: &[String], force: bool) -> serde_json::Value {
    serde_json::json!({
        "ids": asset_ids,
        "force": force,
    })
}

impl ImmichApiClient {
    /// Retrieve the complete list of albums from the Immich server for library display.
    pub async fn fetch_library_albums(&self) -> Result<Vec<LibraryAlbum>, String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let url = format!("{}/api/albums", base_url);

        match self
            .client
            .get(&url)
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let albums = resp
                    .json::<Vec<LibraryAlbum>>()
                    .await
                    .map_err(|err| err.to_string())?;
                self.clear_issue().await;
                Ok(albums)
            }
            Ok(resp) => {
                self.set_issue(classify_http_issue(
                    RequestContext::Albums,
                    resp.status().as_u16(),
                    None,
                ))
                .await;
                Err(format!("HTTP {}", resp.status()))
            }
            Err(err) => {
                *self.active_url.lock().await = None;
                self.set_issue(classify_network_issue(RequestContext::Albums, &err))
                    .await;
                Err(err.to_string())
            }
        }
    }

    /// Retrieve paginated assets contained in a specific album.
    pub async fn fetch_album_assets(
        &self,
        album_id: &str,
        page: u32,
        size: u32,
        order: Option<SortOrder>,
    ) -> Result<(Vec<LibraryAsset>, bool), String> {
        let mut body = serde_json::json!({
            "albumIds": [album_id],
            "page": page,
            "size": size.max(1),
        });
        if let Some(order) = order
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert(
                "order".into(),
                serde_json::to_value(order).unwrap_or(serde_json::Value::Null),
            );
        }
        self.fetch_search_assets(
            "/api/search/metadata",
            body,
            RequestContext::AssetList,
            Some(album_id),
        )
        .await
    }

    /// Retrieve raw thumbnail/preview image byte array for a given asset ID.
    pub async fn fetch_thumbnail(
        &self,
        asset_id: &str,
        size: ThumbnailSize,
    ) -> Result<Vec<u8>, String> {
        let _permit = self
            .thumbnail_semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|err| err.to_string())?;
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let url = format!(
            "{}/api/assets/{}/thumbnail?size={}",
            base_url,
            asset_id,
            size.as_str()
        );

        match self
            .client
            .get(&url)
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/octet-stream")
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let bytes = resp.bytes().await.map_err(|err| err.to_string())?;
                self.clear_issue().await;
                Ok(bytes.to_vec())
            }
            Ok(resp) => {
                self.set_issue(classify_http_issue(
                    RequestContext::ThumbnailFetch,
                    resp.status().as_u16(),
                    Some(asset_id),
                ))
                .await;
                Err(format!("HTTP {}", resp.status()))
            }
            Err(err) => {
                *self.active_url.lock().await = None;
                self.set_issue(classify_network_issue(RequestContext::ThumbnailFetch, &err))
                    .await;
                Err(err.to_string())
            }
        }
    }

    /// Generic helper to fetch asset JSON details.
    async fn fetch_asset_generic<T: serde::de::DeserializeOwned>(
        &self,
        asset_id: &str,
    ) -> Result<T, String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let url = format!("{}/api/assets/{}", base_url, asset_id);
        match self
            .client
            .get(&url)
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                resp.json::<T>().await.map_err(|err| err.to_string())
            }
            Ok(resp) => Err(format!("HTTP {}", resp.status())),
            Err(err) => Err(err.to_string()),
        }
    }

    /// Retrieve full EXIF metadata and details for a given asset ID.
    pub async fn fetch_asset_details(&self, asset_id: &str) -> Result<AssetDetails, String> {
        self.fetch_asset_generic(asset_id).await
    }

    /// Fetch a single asset as a `LibraryAsset` by its ID.
    pub async fn fetch_asset_by_id(&self, asset_id: &str) -> Result<LibraryAsset, String> {
        self.fetch_asset_generic(asset_id).await
    }

    /// Download original source file of a given asset ID and save it locally.
    pub async fn download_original_to_file(
        &self,
        asset_id: &str,
        output_path: &std::path::Path,
        progress: Option<TransferProgressCallback>,
    ) -> Result<(), String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let url = format!("{}/api/assets/{}/original", base_url, asset_id);

        match self
            .client
            .get(&url)
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/octet-stream")
            .timeout(Duration::from_secs(300))
            .send()
            .await
        {
            Ok(mut resp) if resp.status().is_success() => {
                use tokio::io::AsyncWriteExt;
                let total_bytes = resp.content_length();
                let mut written = 0_u64;
                let mut file = tokio::fs::File::create(output_path)
                    .await
                    .map_err(|e| e.to_string())?;
                while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
                    file.write_all(&chunk).await.map_err(|e| e.to_string())?;
                    written = written.saturating_add(chunk.len() as u64);
                    if let Some(callback) = &progress {
                        callback(written, total_bytes);
                    }
                }
                self.clear_issue().await;
                Ok(())
            }
            Ok(resp) => {
                self.set_issue(classify_http_issue(
                    RequestContext::AssetDownload,
                    resp.status().as_u16(),
                    Some(asset_id),
                ))
                .await;
                Err(format!("HTTP {}", resp.status()))
            }
            Err(err) => {
                *self.active_url.lock().await = None;
                self.set_issue(classify_network_issue(RequestContext::AssetDownload, &err))
                    .await;
                Err(err.to_string())
            }
        }
    }

    /// Fetch unique user ID of the logged-in API user.
    pub async fn fetch_current_user_id(&self) -> Result<String, String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let url = format!("{}/api/users/me", base_url);
        match self
            .client
            .get(&url)
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let json: serde_json::Value = resp.json().await.map_err(|err| err.to_string())?;
                json.get("id")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string)
                    .ok_or_else(|| "Missing id in /users/me response".to_string())
            }
            Ok(resp) => Err(format!("HTTP {}", resp.status())),
            Err(err) => Err(err.to_string()),
        }
    }

    /// Update an asset's description, capture date, or favorite flag.
    pub async fn update_asset(&self, asset_id: &str, update: &AssetUpdate) -> Result<(), String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let url = format!("{}/api/assets/{}", base_url, asset_id);
        match self
            .client
            .put(&url)
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(15))
            .json(update)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                self.clear_issue().await;
                Ok(())
            }
            Ok(resp) => Err(format!("HTTP {}", resp.status())),
            Err(err) => Err(err.to_string()),
        }
    }

    /// Soft-delete specified assets from the Immich server.
    pub async fn delete_assets(&self, asset_ids: &[String]) -> Result<(), String> {
        self.delete_assets_with(asset_ids, false).await
    }

    /// Permanently delete assets, skipping the Immich trash. Cannot be undone.
    pub async fn delete_assets_permanently(&self, asset_ids: &[String]) -> Result<(), String> {
        self.delete_assets_with(asset_ids, true).await
    }

    async fn delete_assets_with(&self, asset_ids: &[String], force: bool) -> Result<(), String> {
        if asset_ids.is_empty() {
            return Ok(());
        }
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let url = format!("{}/api/assets", base_url);
        let body = delete_request_body(asset_ids, force);
        match self
            .client
            .delete(&url)
            .header("x-api-key", &settings.api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(15))
            .body(body.to_string())
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                self.clear_issue().await;
                Ok(())
            }
            Ok(resp) => Err(format!("HTTP {}", resp.status())),
            Err(err) => Err(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::delete_request_body;

    #[test]
    fn delete_body_moves_to_trash_unless_forced() {
        let ids = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            delete_request_body(&ids, false),
            serde_json::json!({ "ids": ["a", "b"], "force": false })
        );
        assert_eq!(delete_request_body(&ids, true)["force"], true);
    }
}
