//! Map: geotagged asset markers, place search, map styles, and bulk location edits.

use std::time::Duration;

use futures_util::stream::{self, StreamExt};

use super::{ImmichApiClient, LibraryAsset};

const ASSET_FETCH_CONCURRENCY: usize = 16;

/// Identifies Mimick to tile, style, and sprite hosts.
pub const MAP_USER_AGENT: &str = concat!(
    "Mimick/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/nicx17/mimick)"
);

/// A geotagged asset from `GET /api/map/markers`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct MapMarker {
    pub id: String,
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
}

/// A named place from Immich's geodata, from `GET /api/search/places`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub admin1name: Option<String>,
    #[serde(default)]
    pub admin2name: Option<String>,
}

/// Map style URLs the server gives its clients; `None` when unset or on older servers.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct MapStyleUrls {
    #[serde(default, rename = "mapLightStyleUrl")]
    pub light: Option<String>,
    #[serde(default, rename = "mapDarkStyleUrl")]
    pub dark: Option<String>,
}

impl ImmichApiClient {
    pub async fn fetch_map_markers(&self) -> Result<Vec<MapMarker>, String> {
        let resp = self
            .send_json(reqwest::Method::GET, "/api/map/markers", None)
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn search_places(&self, name: &str) -> Result<Vec<Place>, String> {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("name", name)
            .finish();
        let resp = self
            .send_json(
                reqwest::Method::GET,
                &format!("/api/search/places?{query}"),
                None,
            )
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn fetch_map_style_urls(&self) -> Result<MapStyleUrls, String> {
        let resp = self
            .send_json(reqwest::Method::GET, "/api/server/config", None)
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    /// Move every asset in `ids` to one GPS position in a single request.
    pub async fn set_assets_location(
        &self,
        ids: &[String],
        latitude: f64,
        longitude: f64,
    ) -> Result<(), String> {
        let body = serde_json::json!({ "ids": ids, "latitude": latitude, "longitude": longitude });
        self.send_json(reqwest::Method::PUT, "/api/assets", Some(body))
            .await
            .map(|_| ())
    }

    /// Fetch assets in `ids` order, skipping any the server no longer has.
    pub async fn fetch_assets_by_ids(&self, ids: &[String]) -> Result<Vec<LibraryAsset>, String> {
        let results = stream::iter(ids)
            .map(|id| self.fetch_asset_by_id(id))
            .buffered(ASSET_FETCH_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        keep_found(results)
    }

    /// GET a public URL without the Immich API key (map styles, TileJSON, sprites).
    pub async fn fetch_public_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let resp = self
            .client
            .get(url)
            .header(reqwest::header::USER_AGENT, MAP_USER_AGENT)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|err| err.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("{url}: HTTP {}", resp.status()));
        }
        resp.bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|err| err.to_string())
    }
}

/// Assets that loaded; an error only when nothing did.
fn keep_found(results: Vec<Result<LibraryAsset, String>>) -> Result<Vec<LibraryAsset>, String> {
    let mut first_error = None;
    let mut found = Vec::with_capacity(results.len());
    for result in results {
        match result {
            Ok(asset) => found.push(asset),
            Err(err) => {
                first_error.get_or_insert(err);
            }
        }
    }
    match first_error {
        Some(err) if found.is_empty() => Err(err),
        _ => Ok(found),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: &str) -> LibraryAsset {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "originalFileName": "a.jpg",
            "originalMimeType": "image/jpeg",
            "fileCreatedAt": "2024-01-01T00:00:00.000Z",
            "type": "IMAGE"
        }))
        .unwrap()
    }

    #[test]
    fn markers_and_places_parse_immich_responses() {
        let markers: Vec<MapMarker> = serde_json::from_value(serde_json::json!([
            { "id": "a", "lat": 38.7, "lon": -9.1, "city": "Lisbon", "state": null, "country": "Portugal" },
            { "id": "b", "lat": 1.0, "lon": 2.0, "city": null, "state": null, "country": null }
        ]))
        .unwrap();
        assert_eq!(markers[0].city.as_deref(), Some("Lisbon"));
        assert_eq!(markers[1].country, None);
        let places: Vec<Place> = serde_json::from_value(serde_json::json!([
            { "name": "Porto", "latitude": 41.1, "longitude": -8.6, "admin1name": "Porto" }
        ]))
        .unwrap();
        assert_eq!(places[0].admin2name, None);
    }

    #[test]
    fn style_urls_come_from_server_config() {
        let urls: MapStyleUrls = serde_json::from_value(serde_json::json!({
            "mapLightStyleUrl": "https://tiles.example/light.json",
            "mapDarkStyleUrl": "https://tiles.example/dark.json",
            "trashDays": 30
        }))
        .unwrap();
        assert_eq!(
            urls.light.as_deref(),
            Some("https://tiles.example/light.json")
        );
        let old: MapStyleUrls = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(old, MapStyleUrls::default());
    }

    #[test]
    fn missing_assets_are_skipped_unless_all_fail() {
        let some = keep_found(vec![Ok(asset("a")), Err("HTTP 404".into()), Ok(asset("c"))]);
        let ids: Vec<String> = some.unwrap().into_iter().map(|a| a.id).collect();
        assert_eq!(ids, ["a", "c"]);
        assert_eq!(
            keep_found(vec![Err("offline".into()), Err("HTTP 404".into())]),
            Err("offline".to_string())
        );
        assert_eq!(keep_found(Vec::new()), Ok(Vec::new()));
    }
}
