//! Map: geotagged asset markers, place search, map styles, and bulk location edits.

use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
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

/// Which photos `GET /api/map/markers` returns; the default is Immich's: your own, not archived.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarkerFilter {
    pub favorites_only: bool,
    pub include_archived: bool,
    pub with_partners: bool,
    pub with_shared_albums: bool,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
}

impl MarkerFilter {
    /// Only switched-on flags are sent: `isFavorite=false` would hide favorites instead.
    fn query(&self) -> String {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        let flags = [
            ("isFavorite", self.favorites_only),
            ("isArchived", self.include_archived),
            ("withPartners", self.with_partners),
            ("withSharedAlbums", self.with_shared_albums),
        ];
        for (name, _) in flags.iter().filter(|(_, on)| *on) {
            query.append_pair(name, "true");
        }
        let dates = [
            ("fileCreatedAfter", self.created_after),
            ("fileCreatedBefore", self.created_before),
        ];
        for (name, date) in dates {
            if let Some(date) = date {
                query.append_pair(name, &date.to_rfc3339_opts(SecondsFormat::Millis, true));
            }
        }
        query.finish()
    }
}

/// The place around a point, from `GET /api/map/reverse-geocode`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct PlaceName {
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
}

impl PlaceName {
    /// "City, State, Country", skipping the parts Immich doesn't know.
    pub fn display(&self) -> Option<String> {
        let parts: Vec<&str> = [&self.city, &self.state, &self.country]
            .into_iter()
            .filter_map(|part| part.as_deref().map(str::trim))
            .filter(|part| !part.is_empty())
            .collect();
        (!parts.is_empty()).then(|| parts.join(", "))
    }
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
    pub async fn fetch_map_markers(&self, filter: &MarkerFilter) -> Result<Vec<MapMarker>, String> {
        let query = filter.query();
        let path = if query.is_empty() {
            "/api/map/markers".to_string()
        } else {
            format!("/api/map/markers?{query}")
        };
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    /// Geotagged photos in one album, from `GET /api/albums/{id}/map-markers` (Immich v3).
    pub async fn fetch_album_map_markers(&self, album_id: &str) -> Result<Vec<MapMarker>, String> {
        let path = format!("/api/albums/{album_id}/map-markers");
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    /// The place Immich's geodata puts at a point; `None` out at sea or far from towns.
    pub async fn reverse_geocode(&self, lat: f64, lon: f64) -> Result<Option<PlaceName>, String> {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("lat", &lat.to_string())
            .append_pair("lon", &lon.to_string())
            .finish();
        let path = format!("/api/map/reverse-geocode?{query}");
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        let places: Vec<PlaceName> = resp.json().await.map_err(|err| err.to_string())?;
        Ok(places.into_iter().next())
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

    /// GET a public map tile, revalidating a cached copy when its `etag` is given.
    pub async fn fetch_map_tile(&self, url: &str, etag: Option<&str>) -> Result<TileFetch, String> {
        let mut request = self
            .client
            .get(url)
            .header(reqwest::header::USER_AGENT, MAP_USER_AGENT)
            .timeout(Duration::from_secs(15));
        if let Some(etag) = etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        let resp = request.send().await.map_err(|err| err.to_string())?;
        let headers = resp.headers();
        match resp.status() {
            reqwest::StatusCode::NOT_MODIFIED => return Ok(TileFetch::Unchanged),
            reqwest::StatusCode::NOT_FOUND => return Ok(TileFetch::Missing),
            reqwest::StatusCode::TOO_MANY_REQUESTS => {
                let wait = header_text(headers, reqwest::header::RETRY_AFTER);
                return Ok(TileFetch::RetryAfter(retry_after(wait)));
            }
            status if !status.is_success() => return Err(format!("{url}: HTTP {status}")),
            _ => {}
        }
        let etag = header_text(headers, reqwest::header::ETAG).map(str::to_owned);
        let bytes = resp.bytes().await.map_err(|err| err.to_string())?;
        Ok(TileFetch::Found {
            bytes: bytes.to_vec(),
            etag,
        })
    }
}

/// A tile server's answer for one tile.
#[derive(Debug, PartialEq, Eq)]
pub enum TileFetch {
    Found {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
    Unchanged,
    Missing,
    RetryAfter(Duration),
}

fn header_text(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// The `Retry-After` delay in seconds; an HTTP date or a missing header waits a minute.
pub(super) fn retry_after(value: Option<&str>) -> Duration {
    let seconds = value.and_then(|value| value.trim().parse().ok());
    Duration::from_secs(seconds.unwrap_or(60))
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
    fn retry_after_reads_seconds_and_defaults_to_a_minute() {
        assert_eq!(retry_after(Some(" 30 ")), Duration::from_secs(30));
        assert_eq!(
            retry_after(Some("Wed, 21 Oct 2026 07:28:00 GMT")),
            Duration::from_secs(60)
        );
        assert_eq!(retry_after(None), Duration::from_secs(60));
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
    fn marker_filter_sends_only_what_is_switched_on() {
        assert_eq!(MarkerFilter::default().query(), "");
        let after = DateTime::parse_from_rfc3339("2025-01-02T03:04:05Z")
            .unwrap()
            .with_timezone(&Utc);
        let filter = MarkerFilter {
            favorites_only: true,
            with_shared_albums: true,
            created_after: Some(after),
            ..MarkerFilter::default()
        };
        assert_eq!(
            filter.query(),
            "isFavorite=true&withSharedAlbums=true&fileCreatedAfter=2025-01-02T03%3A04%3A05.000Z"
        );
    }

    #[test]
    fn place_names_skip_unknown_parts() {
        let place: Vec<PlaceName> = serde_json::from_value(serde_json::json!([
            { "city": "Lisbon", "state": null, "country": "Portugal" }
        ]))
        .unwrap();
        assert_eq!(place[0].display().as_deref(), Some("Lisbon, Portugal"));
        assert_eq!(PlaceName::default().display(), None);
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
