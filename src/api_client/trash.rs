//! Trash: list trashed assets, restore them, or empty the trash.

use std::time::Duration;

use super::{ImmichApiClient, MetadataSearchFilters};

/// Immich returns how many assets a trash operation affected.
#[derive(Debug, serde::Deserialize)]
struct TrashResponse {
    count: u64,
}

impl MetadataSearchFilters {
    /// Filters that match only trashed assets. Immich hides trashed assets unless
    /// `withDeleted` is set, and `trashedAfter` then drops the live ones.
    pub fn trashed() -> Self {
        Self {
            with_deleted: Some(true),
            trashed_after: Some("1970-01-01T00:00:00.000Z".to_string()),
            ..Self::default()
        }
    }
}

impl ImmichApiClient {
    /// Restore specific assets from the trash. Returns how many were restored.
    pub async fn restore_assets(&self, asset_ids: &[String]) -> Result<u64, String> {
        if asset_ids.is_empty() {
            return Ok(0);
        }
        self.trash_request(
            "/api/trash/restore/assets",
            Some(serde_json::json!({ "ids": asset_ids })),
        )
        .await
    }

    /// Restore everything in the trash. Returns how many assets were restored.
    pub async fn restore_all_trash(&self) -> Result<u64, String> {
        self.trash_request("/api/trash/restore", None).await
    }

    /// Permanently delete everything in the trash. Returns how many assets were removed.
    pub async fn empty_trash(&self) -> Result<u64, String> {
        self.trash_request("/api/trash/empty", None).await
    }

    async fn trash_request(
        &self,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<u64, String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let mut request = self
            .client
            .post(format!("{}{}", base_url, path))
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/json")
            // Emptying a large trash can take a while server-side.
            .timeout(Duration::from_secs(60));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let resp = request.send().await.map_err(|err| err.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let parsed: TrashResponse = resp.json().await.map_err(|err| err.to_string())?;
        self.clear_issue().await;
        Ok(parsed.count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trashed_filters_request_only_deleted_assets() {
        let json = serde_json::to_value(MetadataSearchFilters::trashed()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "withDeleted": true,
                "trashedAfter": "1970-01-01T00:00:00.000Z",
            })
        );
    }

    #[test]
    fn trash_response_reads_count() {
        let parsed: TrashResponse =
            serde_json::from_value(serde_json::json!({ "count": 7 })).unwrap();
        assert_eq!(parsed.count, 7);
    }
}
