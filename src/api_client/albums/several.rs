//! Adding photos to several albums at once with `PUT /api/albums/assets`.

use super::super::ImmichApiClient;

/// What `PUT /api/albums/assets` did: Immich answers for all the albums at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeveralAdded {
    /// Some photos went into at least one album.
    Added,
    /// Every album already had every photo.
    AlreadyThere,
    NotAllowed,
}

#[derive(serde::Deserialize)]
struct AlbumsAddAssetsResponse {
    success: bool,
    #[serde(default)]
    error: Option<String>,
}

impl SeveralAdded {
    fn from_response(response: &AlbumsAddAssetsResponse) -> Self {
        match (response.success, response.error.as_deref()) {
            (true, _) => SeveralAdded::Added,
            (false, Some("duplicate")) => SeveralAdded::AlreadyThere,
            (false, _) => SeveralAdded::NotAllowed,
        }
    }
}

impl ImmichApiClient {
    /// Add `asset_ids` to every album in `album_ids` with one request.
    pub async fn add_to_albums(
        &self,
        album_ids: &[String],
        asset_ids: &[String],
    ) -> Result<SeveralAdded, String> {
        let body = serde_json::json!({ "albumIds": album_ids, "assetIds": asset_ids });
        let resp = self
            .send_json(reqwest::Method::PUT, "/api/albums/assets", Some(body))
            .await?;
        let response: AlbumsAddAssetsResponse = resp.json().await.map_err(|e| e.to_string())?;
        Ok(SeveralAdded::from_response(&response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_albums_answer_once_for_all() {
        let read = |json| {
            let response: AlbumsAddAssetsResponse = serde_json::from_value(json).unwrap();
            SeveralAdded::from_response(&response)
        };
        assert_eq!(
            read(serde_json::json!({ "success": true })),
            SeveralAdded::Added
        );
        assert_eq!(
            read(serde_json::json!({ "success": false, "error": "duplicate" })),
            SeveralAdded::AlreadyThere
        );
        assert_eq!(
            read(serde_json::json!({ "success": false, "error": "no_permission" })),
            SeveralAdded::NotAllowed
        );
    }
}
