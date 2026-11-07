//! Stacks: bursts and RAW+JPEG pairs grouped under one cover photo.

use super::{ImmichApiClient, LibraryAsset};

/// A stack and all its photos, from `GET /api/stacks/{id}`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stack {
    pub id: String,
    pub primary_asset_id: String,
    #[serde(default)]
    pub assets: Vec<LibraryAsset>,
}

impl ImmichApiClient {
    pub async fn fetch_stack(&self, id: &str) -> Result<Stack, String> {
        let resp = self
            .send_json(reqwest::Method::GET, &format!("/api/stacks/{id}"), None)
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    /// Stacks `asset_ids`; Immich makes the first one the cover.
    pub async fn create_stack(&self, asset_ids: &[String]) -> Result<Stack, String> {
        let body = serde_json::json!({ "assetIds": asset_ids });
        let resp = self
            .send_json(reqwest::Method::POST, "/api/stacks", Some(body))
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn set_stack_cover(&self, id: &str, asset_id: &str) -> Result<(), String> {
        let body = serde_json::json!({ "primaryAssetId": asset_id });
        self.send_json(
            reqwest::Method::PUT,
            &format!("/api/stacks/{id}"),
            Some(body),
        )
        .await
        .map(|_| ())
    }

    /// Takes one photo out; Immich deletes the stack once fewer than two remain.
    pub async fn remove_from_stack(&self, id: &str, asset_id: &str) -> Result<(), String> {
        let path = format!("/api/stacks/{id}/assets/{asset_id}");
        self.send_json(reqwest::Method::DELETE, &path, None)
            .await
            .map(|_| ())
    }

    /// Unstacks: every photo goes back to being its own tile.
    pub async fn delete_stack(&self, id: &str) -> Result<(), String> {
        self.send_json(reqwest::Method::DELETE, &format!("/api/stacks/{id}"), None)
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_reads_cover_and_photos() {
        let stack: Stack = serde_json::from_value(serde_json::json!({
            "id": "s1",
            "primaryAssetId": "a1",
            "assets": [{
                "id": "a1",
                "originalFileName": "IMG_1.CR3",
                "originalMimeType": "image/x-canon-cr3",
                "fileCreatedAt": "2026-03-15T12:00:00.000Z",
                "type": "IMAGE"
            }]
        }))
        .unwrap();
        assert_eq!(stack.primary_asset_id, "a1");
        assert_eq!(stack.assets.len(), 1);
    }
}
