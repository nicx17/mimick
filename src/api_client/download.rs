//! Album downloads: how Immich splits an album into zip archives, and streaming each one.

use std::path::Path;
use std::time::Duration;

use super::library::stream_to_file;
use super::{ImmichApiClient, TransferProgressCallback};

const ARCHIVE_TIMEOUT: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct DownloadInfo {
    #[serde(rename = "totalSize")]
    pub total_size: u64,
    pub archives: Vec<DownloadArchive>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct DownloadArchive {
    #[serde(rename = "assetIds")]
    pub asset_ids: Vec<String>,
    pub size: u64,
}

impl ImmichApiClient {
    pub async fn album_download_info(&self, album_id: &str) -> Result<DownloadInfo, String> {
        let body = serde_json::json!({ "albumId": album_id });
        let resp = self
            .send_json(reqwest::Method::POST, "/api/download/info", Some(body))
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    /// Stream one zip into `output`; a failed download leaves nothing behind.
    pub async fn download_archive_to_file(
        &self,
        asset_ids: &[String],
        output: &Path,
        progress: Option<TransferProgressCallback>,
    ) -> Result<(), String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let resp = self
            .client
            .post(format!("{base_url}/api/download/archive"))
            .header("x-api-key", self.settings_snapshot().api_key)
            .header("Accept", "application/octet-stream")
            .json(&serde_json::json!({ "assetIds": asset_ids }))
            .timeout(ARCHIVE_TIMEOUT)
            .send()
            .await
            .map_err(|err| err.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let partial = partial_path(output);
        let result = stream_to_file(resp, &partial, progress.as_ref()).await;
        match result {
            Ok(()) => tokio::fs::rename(&partial, output)
                .await
                .map_err(|err| err.to_string()),
            Err(err) => {
                let _ = tokio::fs::remove_file(&partial).await;
                Err(err)
            }
        }
    }
}

fn partial_path(output: &Path) -> std::path::PathBuf {
    let mut name = output.as_os_str().to_owned();
    name.push(".part");
    name.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_info_lists_archives() {
        let info: DownloadInfo = serde_json::from_value(serde_json::json!({
            "totalSize": 30,
            "archives": [
                { "assetIds": ["a", "b"], "size": 20 },
                { "assetIds": ["c"], "size": 10 }
            ]
        }))
        .unwrap();
        assert_eq!(info.total_size, 30);
        assert_eq!(info.archives.len(), 2);
        assert_eq!(info.archives[0].asset_ids, ["a", "b"]);
    }
}
