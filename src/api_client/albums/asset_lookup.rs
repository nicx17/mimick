//! Check whether assets already exist on the server by checksum.

use std::collections::HashMap;
use std::time::Duration;

use super::super::ImmichApiClient;

impl ImmichApiClient {
    /// Check whether an asset already exists on the server by checksum and return its asset ID.
    /// Batch checksum existence check. Returns a map from checksum to asset id
    /// for every checksum the server already has. Missing checksums are absent
    /// from the result. The Immich endpoint accepts arbitrary batch sizes, but
    /// we chunk to keep request bodies modest.
    pub async fn bulk_existing_asset_ids(&self, checksums: &[String]) -> HashMap<String, String> {
        const CHUNK: usize = 500;
        let mut out: HashMap<String, String> = HashMap::new();
        let Some(base_url) = self.get_active_url().await else {
            return out;
        };
        let url = format!("{}/api/assets/bulk-upload-check", base_url);
        let api_key = self.settings.read().api_key.clone();

        for chunk in checksums.chunks(CHUNK) {
            let assets: Vec<_> = chunk
                .iter()
                .map(|c| serde_json::json!({ "id": c, "checksum": c }))
                .collect();
            let body = serde_json::json!({ "assets": assets });

            match self
                .client
                .post(&url)
                .header("x-api-key", &api_key)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .json(&body)
                .timeout(Duration::from_secs(30))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(json) = resp.json::<serde_json::Value>().await
                        && let Some(results) = json["results"].as_array()
                    {
                        for item in results {
                            let id = item["id"].as_str();
                            let asset_id = item["assetId"].as_str();
                            if let (Some(checksum), Some(asset_id)) = (id, asset_id) {
                                out.insert(checksum.to_string(), asset_id.to_string());
                            }
                        }
                    }
                }
                Ok(resp) => log::warn!("Bulk upload check returned {}", resp.status()),
                Err(err) => log::warn!("Bulk upload check request failed: {}", err),
            }
        }

        out
    }

    pub async fn find_existing_asset_id(&self, checksum: &str) -> Option<String> {
        let base_url = self.get_active_url().await?;
        let url = format!("{}/api/assets/bulk-upload-check", base_url);
        let api_key = self.settings.read().api_key.clone();
        let body = serde_json::json!({
            "assets": [
                {
                    "id": checksum,
                    "checksum": checksum
                }
            ]
        });

        match self
            .client
            .post(&url)
            .header("x-api-key", &api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let json = resp.json::<serde_json::Value>().await.ok()?;
                json["results"]
                    .as_array()
                    .and_then(|results| results.first())
                    .and_then(|item| item["assetId"].as_str())
                    .map(ToString::to_string)
            }
            Ok(resp) => {
                log::warn!(
                    "Bulk upload check failed for checksum {}: {}",
                    checksum,
                    resp.status()
                );
                None
            }
            Err(err) => {
                log::warn!(
                    "Bulk upload check request failed for checksum {}: {}",
                    checksum,
                    err
                );
                None
            }
        }
    }
}
