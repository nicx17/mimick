//! Place search through Photon (photon.komoot.io), an OpenStreetMap geocoder outside Immich.

use std::time::Duration;

use super::ImmichApiClient;
use super::map::{MAP_USER_AGENT, retry_after};

const PHOTON_URL: &str = "https://photon.komoot.io/api/";
const PHOTON_LIMIT: &str = "8";

/// Where results should be preferred: the map's centre and its web-map zoom level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchBias {
    pub lat: f64,
    pub lon: f64,
    pub zoom: u32,
}

#[derive(Debug, PartialEq)]
pub enum PhotonReply {
    Places(Vec<PhotonPlace>),
    RetryAfter(Duration),
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct PhotonPlace {
    pub geometry: PhotonPoint,
    pub properties: PhotonProperties,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct PhotonPoint {
    /// Longitude, then latitude.
    pub coordinates: [f64; 2],
}

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct PhotonProperties {
    pub name: Option<String>,
    /// Photon's layer: house, street, locality, district, city, county, state, country, or other.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub osm_value: Option<String>,
    pub housenumber: Option<String>,
    pub street: Option<String>,
    pub district: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
    /// West, north, east, south.
    pub extent: Option<[f64; 4]>,
}

#[derive(serde::Deserialize)]
struct PhotonResponse {
    features: Vec<PhotonPlace>,
}

impl ImmichApiClient {
    pub async fn search_photon(
        &self,
        query: &str,
        lang: &str,
        bias: Option<SearchBias>,
    ) -> Result<PhotonReply, String> {
        let resp = self
            .client
            .get(photon_url(query, lang, bias))
            .header(reqwest::header::USER_AGENT, MAP_USER_AGENT)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|err| err.to_string())?;
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let wait = resp.headers().get(reqwest::header::RETRY_AFTER);
            return Ok(PhotonReply::RetryAfter(retry_after(
                wait.and_then(|value| value.to_str().ok()),
            )));
        }
        if !status.is_success() {
            return Err(format!("Photon: HTTP {status}"));
        }
        let body: PhotonResponse = resp.json().await.map_err(|err| err.to_string())?;
        Ok(PhotonReply::Places(body.features))
    }
}

fn photon_url(query: &str, lang: &str, bias: Option<SearchBias>) -> String {
    let mut params = url::form_urlencoded::Serializer::new(String::new());
    params
        .append_pair("q", query)
        .append_pair("limit", PHOTON_LIMIT)
        .append_pair("lang", lang);
    if let Some(bias) = bias {
        params
            .append_pair("lat", &format!("{:.4}", bias.lat))
            .append_pair("lon", &format!("{:.4}", bias.lon))
            .append_pair("zoom", &bias.zoom.to_string());
    }
    format!("{PHOTON_URL}?{}", params.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photon_features_parse_with_missing_fields() {
        let body: PhotonResponse = serde_json::from_value(serde_json::json!({
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "geometry": { "type": "Point", "coordinates": [1.888, 46.603] },
                    "properties": {
                        "osm_value": "country", "type": "country", "name": "France",
                        "country": "France", "extra": { "admin_level": "2" },
                        "extent": [-178.38, 51.30, 172.30, -50.21]
                    }
                },
                {
                    "type": "Feature",
                    "geometry": { "type": "Point", "coordinates": [-9.137, 38.710] },
                    "properties": { "type": "house", "housenumber": "100", "street": "Rua Augusta" }
                }
            ]
        }))
        .unwrap();
        let [france, house] = body.features.as_slice() else {
            panic!("two features");
        };
        assert_eq!(france.geometry.coordinates, [1.888, 46.603]);
        assert_eq!(france.properties.kind.as_deref(), Some("country"));
        assert_eq!(
            france.properties.extent,
            Some([-178.38, 51.30, 172.30, -50.21])
        );
        assert_eq!(house.properties.name, None);
        assert_eq!(house.properties.street.as_deref(), Some("Rua Augusta"));
    }

    #[test]
    fn url_carries_language_and_bias() {
        let bias = SearchBias {
            lat: 38.72231,
            lon: -9.13934,
            zoom: 11,
        };
        assert_eq!(
            photon_url("rua augusta & co", "en", Some(bias)),
            format!(
                "{PHOTON_URL}?q=rua+augusta+%26+co&limit=8&lang=en&lat=38.7223&lon=-9.1393&zoom=11"
            )
        );
        assert_eq!(
            photon_url("berlin", "de", None),
            format!("{PHOTON_URL}?q=berlin&limit=8&lang=de")
        );
    }
}
