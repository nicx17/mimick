//! Duplicate groups that Immich's duplicate-detection job found.

use super::{ImmichApiClient, LibraryAsset};

/// Photos Immich thinks are the same, and the ones it suggests keeping.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub duplicate_id: String,
    pub assets: Vec<LibraryAsset>,
    #[serde(default)]
    pub suggested_keep_asset_ids: Vec<String>,
}

impl ImmichApiClient {
    pub async fn fetch_duplicates(&self) -> Result<Vec<DuplicateGroup>, String> {
        let resp = self
            .send_json(reqwest::Method::GET, "/api/duplicates", None)
            .await?;
        let groups: Vec<DuplicateGroup> = resp.json().await.map_err(|err| err.to_string())?;
        Ok(groups
            .into_iter()
            .filter(|group| group.assets.len() > 1)
            .collect())
    }

    /// Keeps `keep` and trashes `trash`; Immich copies albums, tags and favorites to what's kept.
    pub async fn resolve_duplicate(
        &self,
        id: &str,
        keep: &[String],
        trash: &[String],
    ) -> Result<(), String> {
        let body = serde_json::json!({
            "groups": [{
                "duplicateId": id,
                "keepAssetIds": keep,
                "trashAssetIds": trash,
            }]
        });
        self.send_json(reqwest::Method::POST, "/api/duplicates/resolve", Some(body))
            .await
            .map(|_| ())
    }

    /// "Not duplicates": Immich forgets the group and keeps every photo.
    pub async fn dismiss_duplicate(&self, id: &str) -> Result<(), String> {
        self.send_json(
            reqwest::Method::DELETE,
            &format!("/api/duplicates/{id}"),
            None,
        )
        .await
        .map(|_| ())
    }
}
