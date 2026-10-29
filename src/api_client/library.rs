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

/// Writes a response body to `output_path` chunk by chunk, reporting bytes written.
pub(super) async fn stream_to_file(
    mut resp: reqwest::Response,
    output_path: &std::path::Path,
    progress: Option<&TransferProgressCallback>,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let total_bytes = resp.content_length();
    let mut written = 0_u64;
    let mut file = tokio::fs::File::create(output_path)
        .await
        .map_err(|e| e.to_string())?;
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        written = written.saturating_add(chunk.len() as u64);
        if let Some(callback) = progress {
            callback(written, total_bytes);
        }
    }
    Ok(())
}

/// A new capture date for many assets: one fixed moment, or a shift that keeps their spacing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateChange {
    /// `date_time` is RFC 3339 with `time_zone`'s offset on that date.
    Set {
        date_time: String,
        time_zone: String,
    },
    Shift {
        minutes: i64,
        time_zone: Option<String>,
    },
}

fn date_change_body(ids: &[String], change: &DateChange) -> serde_json::Value {
    let mut body = serde_json::json!({ "ids": ids });
    match change {
        DateChange::Set {
            date_time,
            time_zone,
        } => {
            body["dateTimeOriginal"] = date_time.as_str().into();
            body["timeZone"] = time_zone.as_str().into();
        }
        DateChange::Shift { minutes, time_zone } => {
            if *minutes != 0 {
                body["dateTimeRelative"] = (*minutes).into();
            }
            if let Some(zone) = time_zone {
                body["timeZone"] = zone.as_str().into();
            }
        }
    }
    body
}

/// API path for a thumbnail; `edited=true` is added only for edited assets so existing URLs don't change.
fn thumbnail_path(asset_id: &str, size: ThumbnailSize, edited: bool) -> String {
    let path = format!("/api/assets/{asset_id}/thumbnail?size={}", size.as_str());
    if edited {
        format!("{path}&edited=true")
    } else {
        path
    }
}

/// API path for an original, or for Immich's edited render when `edited` is set.
fn original_path(asset_id: &str, edited: bool) -> String {
    let path = format!("/api/assets/{asset_id}/original");
    if edited {
        format!("{path}?edited=true")
    } else {
        path
    }
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
        edited: bool,
    ) -> Result<Vec<u8>, String> {
        let lane = match size {
            ThumbnailSize::Thumbnail => &self.thumbnail_semaphore,
            ThumbnailSize::Preview | ThumbnailSize::Fullsize => &self.preview_semaphore,
        };
        let _permit = lane
            .clone()
            .acquire_owned()
            .await
            .map_err(|err| err.to_string())?;
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let url = format!("{base_url}{}", thumbnail_path(asset_id, size, edited));

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

    pub async fn video_playback_source(&self, asset_id: &str) -> Option<(String, String)> {
        let base_url = self.get_active_url().await?;
        let url = format!("{}/api/assets/{}/video/playback", base_url, asset_id);
        Some((url, self.settings_snapshot().api_key))
    }

    /// Download original source file of a given asset ID and save it locally.
    pub async fn download_original_to_file(
        &self,
        asset_id: &str,
        output_path: &std::path::Path,
        progress: Option<TransferProgressCallback>,
    ) -> Result<(), String> {
        let path = original_path(asset_id, false);
        self.download_to_file(&path, asset_id, output_path, progress)
            .await
    }

    /// Download Immich's edited render of an asset at full resolution.
    pub async fn download_edited_original_to_file(
        &self,
        asset_id: &str,
        output_path: &std::path::Path,
        progress: Option<TransferProgressCallback>,
    ) -> Result<(), String> {
        let path = original_path(asset_id, true);
        self.download_to_file(&path, asset_id, output_path, progress)
            .await
    }

    /// Download `path` from the active server into `output_path`.
    async fn download_to_file(
        &self,
        path: &str,
        asset_id: &str,
        output_path: &std::path::Path,
        progress: Option<TransferProgressCallback>,
    ) -> Result<(), String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let url = format!("{base_url}{path}");

        match self
            .client
            .get(&url)
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/octet-stream")
            .timeout(Duration::from_secs(300))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                stream_to_file(resp, output_path, progress.as_ref()).await?;
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

    pub async fn set_asset_rating(&self, asset_id: &str, rating: Option<u8>) -> Result<(), String> {
        let path = format!("/api/assets/{asset_id}");
        let body = serde_json::json!({ "rating": rating });
        self.send_json(reqwest::Method::PUT, &path, Some(body))
            .await
            .map(|_| ())
    }

    pub async fn set_assets_visibility(
        &self,
        asset_ids: &[String],
        visibility: &str,
    ) -> Result<(), String> {
        let body = serde_json::json!({ "ids": asset_ids, "visibility": visibility });
        self.send_json(reqwest::Method::PUT, "/api/assets", Some(body))
            .await
            .map(|_| ())
    }

    pub async fn set_assets_date(&self, ids: &[String], change: &DateChange) -> Result<(), String> {
        self.send_json(
            reqwest::Method::PUT,
            "/api/assets",
            Some(date_change_body(ids, change)),
        )
        .await
        .map(|_| ())
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
    use super::{
        DateChange, ThumbnailSize, date_change_body, delete_request_body, original_path,
        thumbnail_path,
    };

    #[test]
    fn date_change_body_sets_or_shifts() {
        let ids = vec!["a".to_string()];
        let set = DateChange::Set {
            date_time: "2024-01-15T19:55:15+05:30".into(),
            time_zone: "Asia/Kolkata".into(),
        };
        assert_eq!(
            date_change_body(&ids, &set),
            serde_json::json!({
                "ids": ["a"],
                "dateTimeOriginal": "2024-01-15T19:55:15+05:30",
                "timeZone": "Asia/Kolkata"
            })
        );
        assert_eq!(
            date_change_body(
                &ids,
                &DateChange::Shift {
                    minutes: -90,
                    time_zone: None
                }
            ),
            serde_json::json!({ "ids": ["a"], "dateTimeRelative": -90 })
        );
        assert_eq!(
            date_change_body(
                &ids,
                &DateChange::Shift {
                    minutes: 0,
                    time_zone: Some("Europe/Lisbon".into())
                }
            ),
            serde_json::json!({ "ids": ["a"], "timeZone": "Europe/Lisbon" })
        );
    }

    #[test]
    fn thumbnail_path_adds_edited_only_when_requested() {
        assert_eq!(
            thumbnail_path("a1", ThumbnailSize::Preview, false),
            "/api/assets/a1/thumbnail?size=preview"
        );
        assert_eq!(
            thumbnail_path("a1", ThumbnailSize::Preview, true),
            "/api/assets/a1/thumbnail?size=preview&edited=true"
        );
    }

    #[test]
    fn original_path_adds_edited_only_when_requested() {
        assert_eq!(original_path("a1", false), "/api/assets/a1/original");
        assert_eq!(
            original_path("a1", true),
            "/api/assets/a1/original?edited=true"
        );
    }

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
