//! Album counts from `GET /api/albums/statistics`.

use super::super::ImmichApiClient;

/// How many albums you own, how many are shared either way, and how many of yours aren't.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumStatistics {
    pub owned: u32,
    pub shared: u32,
    pub not_shared: u32,
}

impl ImmichApiClient {
    pub async fn fetch_album_statistics(&self) -> Result<AlbumStatistics, String> {
        let resp = self
            .send_json(reqwest::Method::GET, "/api/albums/statistics", None)
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }
}
