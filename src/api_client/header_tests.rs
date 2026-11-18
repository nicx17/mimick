use super::{ActiveConnection, ApiClientSettings, ImmichApiClient};
use crate::config::{CustomHeader, HeaderScope};

fn settings(internal_url: &str, external_url: &str) -> ApiClientSettings {
    ApiClientSettings {
        internal_url: internal_url.into(),
        external_url: external_url.into(),
        api_key: "key".into(),
        custom_headers: Vec::new(),
    }
}

fn route_client(
    internal_url: &str,
    external_url: &str,
    headers: Vec<CustomHeader>,
) -> ImmichApiClient {
    ImmichApiClient::new(
        internal_url.into(),
        external_url.into(),
        "api-key".into(),
        headers,
    )
}

fn request_headers(client: &ImmichApiClient, url: &str) -> reqwest::header::HeaderMap {
    client
        .authorized_request(reqwest::Method::GET, url)
        .build()
        .unwrap()
        .headers()
        .clone()
}

fn standard_headers() -> Vec<CustomHeader> {
    vec![
        CustomHeader::new("CF-Access-Client-Id", "id-123", HeaderScope::Wan),
        CustomHeader::new("CF-Access-Client-Secret", "secret-456", HeaderScope::Wan),
        CustomHeader::new("X-Both", "both-val", HeaderScope::All),
        CustomHeader::new("X-Local-Only", "lan-val", HeaderScope::Lan),
        CustomHeader::new("Invalid Header Name!", "bad", HeaderScope::All),
    ]
}

#[test]
fn server_route_detection() {
    let settings = settings("http://192.168.1.50:2283", "https://photos.example.com");
    assert_eq!(
        settings.server_route("https://photos.example.com"),
        Some(true)
    );
    assert_eq!(
        settings.server_route("https://photos.example.com/api/server/ping"),
        Some(true)
    );
    assert_eq!(
        settings.server_route("http://192.168.1.50:2283"),
        Some(false)
    );
    assert_eq!(
        settings.server_route("http://192.168.1.50:2283/api/albums"),
        Some(false)
    );
    assert_eq!(
        settings.server_route("https://photon.komoot.io/api?q=test"),
        None
    );
    assert_eq!(
        settings.server_route("https://tile.openstreetmap.org/1/2/3.png"),
        None
    );
}

#[test]
fn same_server_url_defaults_to_internal() {
    let settings = settings("https://photos.example.com", "https://photos.example.com");
    assert_eq!(
        settings.server_route("https://photos.example.com/api/server/ping"),
        Some(false)
    );
}

#[test]
fn custom_headers_follow_the_route() {
    let client = route_client(
        "http://192.168.1.50:2283",
        "https://photos.example.com",
        standard_headers(),
    );
    assert_wan_headers(&request_headers(
        &client,
        "https://photos.example.com/api/server/ping",
    ));
    assert_lan_headers(&request_headers(
        &client,
        "http://192.168.1.50:2283/api/albums",
    ));
}

fn assert_wan_headers(headers: &reqwest::header::HeaderMap) {
    assert_eq!(headers.get("x-api-key").unwrap(), "api-key");
    assert_eq!(headers.get("CF-Access-Client-Id").unwrap(), "id-123");
    assert_eq!(
        headers.get("CF-Access-Client-Secret").unwrap(),
        "secret-456"
    );
    assert_eq!(headers.get("X-Both").unwrap(), "both-val");
    assert!(headers.get("X-Local-Only").is_none());
    assert!(headers.get("Invalid Header Name!").is_none());
}

fn assert_lan_headers(headers: &reqwest::header::HeaderMap) {
    assert_eq!(headers.get("x-api-key").unwrap(), "api-key");
    assert!(headers.get("CF-Access-Client-Id").is_none());
    assert!(headers.get("CF-Access-Client-Secret").is_none());
    assert_eq!(headers.get("X-Both").unwrap(), "both-val");
    assert_eq!(headers.get("X-Local-Only").unwrap(), "lan-val");
}

#[test]
fn custom_headers_skip_third_party_hosts() {
    let client = route_client(
        "http://192.168.1.50:2283",
        "https://photos.example.com",
        standard_headers(),
    );
    let request = client.client.get("https://photon.komoot.io/api?q=test");
    let request = client.apply_custom_headers(request, "https://photon.komoot.io/api?q=test");
    let headers = request.build().unwrap().headers().clone();
    assert!(headers.get("CF-Access-Client-Id").is_none());
    assert!(headers.get("CF-Access-Client-Secret").is_none());
    assert!(headers.get("X-Both").is_none());
    assert!(headers.get("X-Local-Only").is_none());
}

#[tokio::test]
async fn custom_headers_follow_active_route_for_identical_urls() {
    let headers = vec![
        CustomHeader::new("CF-Access-Client-Id", "id-wan", HeaderScope::Wan),
        CustomHeader::new("X-Local-Token", "secret-lan", HeaderScope::Lan),
        CustomHeader::new("X-Common", "common-all", HeaderScope::All),
    ];
    let client = route_client(
        "https://immich.home.arpa",
        "https://immich.home.arpa",
        headers,
    );
    assert_headers_for_active_route(&client, false, "LAN", "X-Local-Token", "secret-lan").await;
    assert_headers_for_active_route(&client, true, "WAN", "CF-Access-Client-Id", "id-wan").await;
}

async fn assert_headers_for_active_route(
    client: &ImmichApiClient,
    is_external: bool,
    label: &str,
    expected_name: &str,
    expected_value: &str,
) {
    *client.active_connection.write() = Some(ActiveConnection {
        url: "https://immich.home.arpa".into(),
        is_external,
    });
    assert_eq!(client.active_route_label().await.as_deref(), Some(label));
    let headers = request_headers(client, "https://immich.home.arpa/api/server/ping");
    assert_eq!(headers.get("x-api-key").unwrap(), "api-key");
    assert_eq!(headers.get(expected_name).unwrap(), expected_value);
    assert_eq!(headers.get("X-Common").unwrap(), "common-all");
}
