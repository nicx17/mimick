//! Tags: list, create, and apply/remove tags on assets.

use std::time::Duration;

use super::{ImmichApiClient, Tag};

/// Per-asset outcome of a tag/untag request.
#[derive(Debug, serde::Deserialize)]
struct BulkIdResult {
    success: bool,
    #[serde(default)]
    error: Option<String>,
}

/// Fail on the first rejected asset. "duplicate" (already tagged) and "not_found"
/// on untag (already removed) leave the asset in the requested state, so they count as success.
fn check_bulk_results(results: &[BulkIdResult]) -> Result<(), String> {
    match results.iter().find(|r| {
        !r.success && !matches!(r.error.as_deref(), Some("duplicate") | Some("not_found"))
    }) {
        Some(failed) => Err(failed
            .error
            .clone()
            .unwrap_or_else(|| "unknown error".to_string())),
        None => Ok(()),
    }
}

impl ImmichApiClient {
    /// All tags on the server, sorted by their full path.
    pub async fn fetch_tags(&self) -> Result<Vec<Tag>, String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let resp = self
            .client
            .get(format!("{}/api/tags", base_url))
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|err| err.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let mut tags: Vec<Tag> = resp.json().await.map_err(|err| err.to_string())?;
        tags.sort_by_key(|tag| tag.value.to_lowercase());
        Ok(tags)
    }

    /// Create a tag (and any missing parents for a `Parent/Child` path), or return it if it exists.
    pub async fn create_tag(&self, value: &str) -> Result<Tag, String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let resp = self
            .client
            .put(format!("{}/api/tags", base_url))
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/json")
            .json(&serde_json::json!({ "tags": [value] }))
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|err| err.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let tags: Vec<Tag> = resp.json().await.map_err(|err| err.to_string())?;
        // The response includes created parents too; pick the requested leaf.
        tags.into_iter()
            .find(|tag| tag.value == value)
            .ok_or_else(|| format!("Immich did not return the tag \"{value}\""))
    }

    /// Apply a tag to an asset.
    pub async fn tag_asset(&self, tag_id: &str, asset_id: &str) -> Result<(), String> {
        self.change_tag_assets(reqwest::Method::PUT, tag_id, asset_id)
            .await
    }

    /// Remove a tag from an asset.
    pub async fn untag_asset(&self, tag_id: &str, asset_id: &str) -> Result<(), String> {
        self.change_tag_assets(reqwest::Method::DELETE, tag_id, asset_id)
            .await
    }

    async fn change_tag_assets(
        &self,
        method: reqwest::Method,
        tag_id: &str,
        asset_id: &str,
    ) -> Result<(), String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let resp = self
            .client
            .request(method, format!("{}/api/tags/{}/assets", base_url, tag_id))
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/json")
            .json(&serde_json::json!({ "ids": [asset_id] }))
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|err| err.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let results: Vec<BulkIdResult> = resp.json().await.map_err(|err| err.to_string())?;
        check_bulk_results(&results)?;
        self.clear_issue().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn results(json: serde_json::Value) -> Vec<BulkIdResult> {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn bulk_results_accept_success_and_no_op_errors() {
        let ok = results(serde_json::json!([
            { "id": "a", "success": true },
            { "id": "b", "success": false, "error": "duplicate" },
            { "id": "c", "success": false, "error": "not_found" },
        ]));
        assert_eq!(check_bulk_results(&ok), Ok(()));
    }

    #[test]
    fn bulk_results_report_real_failures() {
        let failed = results(serde_json::json!([
            { "id": "a", "success": false, "error": "no_permission" },
        ]));
        assert_eq!(
            check_bulk_results(&failed),
            Err("no_permission".to_string())
        );
    }

    #[test]
    fn tag_deserializes_from_immich_response() {
        let tag: Tag = serde_json::from_value(serde_json::json!({
            "id": "t1",
            "name": "2024",
            "value": "Trips/2024",
            "parentId": "t0",
            "createdAt": "2024-01-01T00:00:00.000Z",
            "updatedAt": "2024-01-01T00:00:00.000Z"
        }))
        .unwrap();
        assert_eq!(tag.value, "Trips/2024");
        assert_eq!(tag.name, "2024");
    }
}
