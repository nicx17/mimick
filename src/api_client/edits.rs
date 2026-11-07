//! Non-destructive photo edits (crop, rotate, mirror) that Immich renders on top of the original.

use super::ImmichApiClient;

/// One edit, as `{ "action": …, "parameters": … }`. Immich crops first, in the original's
/// pixels, then applies the mirrors and rotation in list order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", content = "parameters", rename_all = "lowercase")]
pub enum AssetEdit {
    Crop {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    /// Clockwise degrees: 90, 180, or 270.
    Rotate {
        angle: u32,
    },
    Mirror {
        axis: MirrorAxis,
    },
}

/// `Horizontal` swaps left and right; `Vertical` swaps top and bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MirrorAxis {
    Horizontal,
    Vertical,
}

/// An edited copy to upload as a new photo.
pub struct CopyUpload {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub mime: &'static str,
    /// The original's `fileCreatedAt`.
    pub created_at: String,
    pub favorite: bool,
}

#[derive(serde::Deserialize)]
struct EditsResponse {
    #[serde(default)]
    edits: Vec<AssetEdit>,
}

impl ImmichApiClient {
    pub async fn fetch_edits(&self, asset_id: &str) -> Result<Vec<AssetEdit>, String> {
        let resp = self
            .send_json(
                reqwest::Method::GET,
                &format!("/api/assets/{asset_id}/edits"),
                None,
            )
            .await?;
        let edits: EditsResponse = resp.json().await.map_err(|err| err.to_string())?;
        Ok(edits.edits)
    }

    /// Replaces the photo's edits with `edits`.
    pub async fn save_edits(&self, asset_id: &str, edits: &[AssetEdit]) -> Result<(), String> {
        let body = serde_json::json!({ "edits": edits });
        self.send_json(
            reqwest::Method::PUT,
            &format!("/api/assets/{asset_id}/edits"),
            Some(body),
        )
        .await
        .map(|_| ())
    }

    /// Uploads an edited copy of a photo as a new one and returns its id. `created_at` keeps
    /// it beside the original; the timezone fix-up sync uploads get is left out.
    pub async fn upload_copy(&self, copy: CopyUpload) -> Result<String, String> {
        let base_url = self.require_active_url().await?;
        let part = reqwest::multipart::Part::bytes(copy.bytes)
            .file_name(copy.filename.clone())
            .mime_str(copy.mime)
            .map_err(|err| err.to_string())?;
        let form = reqwest::multipart::Form::new()
            .part("assetData", part)
            .text("fileCreatedAt", copy.created_at)
            .text("fileModifiedAt", chrono::Utc::now().to_rfc3339())
            .text("filename", copy.filename)
            .text("isFavorite", copy.favorite.to_string());
        let resp = self
            .client
            .post(format!("{base_url}/api/assets"))
            .header("x-api-key", self.settings_snapshot().api_key)
            .header("Accept", "application/json")
            .timeout(std::time::Duration::from_secs(300))
            .multipart(form)
            .send()
            .await
            .map_err(|err| err.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let created: serde_json::Value = resp.json().await.map_err(|err| err.to_string())?;
        created["id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| "Immich didn't say which photo it made".to_string())
    }

    /// Drops every edit, back to the original.
    pub async fn revert_edits(&self, asset_id: &str) -> Result<(), String> {
        self.send_json(
            reqwest::Method::DELETE,
            &format!("/api/assets/{asset_id}/edits"),
            None,
        )
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_use_immichs_action_and_parameters_shape() {
        let edits = [
            AssetEdit::Crop {
                x: 1,
                y: 2,
                width: 30,
                height: 40,
            },
            AssetEdit::Mirror {
                axis: MirrorAxis::Horizontal,
            },
            AssetEdit::Rotate { angle: 90 },
        ];
        assert_eq!(
            serde_json::to_value(edits).unwrap(),
            serde_json::json!([
                { "action": "crop", "parameters": { "x": 1, "y": 2, "width": 30, "height": 40 } },
                { "action": "mirror", "parameters": { "axis": "horizontal" } },
                { "action": "rotate", "parameters": { "angle": 90 } }
            ])
        );
    }

    #[test]
    fn saved_edits_read_back_despite_their_ids() {
        let response: EditsResponse = serde_json::from_value(serde_json::json!({
            "assetId": "a",
            "edits": [{ "id": "e1", "action": "rotate", "parameters": { "angle": 270 } }]
        }))
        .unwrap();
        assert_eq!(response.edits, [AssetEdit::Rotate { angle: 270 }]);
    }
}
