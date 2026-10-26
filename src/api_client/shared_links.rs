//! Shared links: public URLs for an album or a set of assets.

use super::ImmichApiClient;

/// A shared link as returned by `GET /api/shared-links`.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SharedLink {
    pub id: String,
    /// Secret part of the public URL (`/share/{key}`).
    pub key: String,
    /// Optional custom path (`/s/{slug}`), used instead of the key when set.
    #[serde(default)]
    pub slug: Option<String>,
    /// `"ALBUM"` or `"INDIVIDUAL"`.
    #[serde(rename = "type")]
    pub link_type: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Present only when the link is password protected.
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub allow_download: bool,
    #[serde(default)]
    pub allow_upload: bool,
    #[serde(default)]
    pub show_metadata: bool,
    #[serde(default)]
    pub album: Option<SharedLinkAlbum>,
    #[serde(default)]
    pub assets: Vec<SharedLinkAsset>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SharedLinkAlbum {
    pub id: String,
    pub album_name: String,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct SharedLinkAsset {
    pub id: String,
}

/// Settings shared by creating and editing a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedLinkOptions {
    pub description: String,
    /// New password; empty keeps no password (create) or the current one (edit).
    pub password: String,
    /// RFC3339 expiry, or `None` for a link that never expires.
    pub expires_at: Option<String>,
    pub allow_download: bool,
    pub allow_upload: bool,
    pub show_metadata: bool,
}

/// What a new link shares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedLinkTarget {
    Album(String),
    Assets(Vec<String>),
}

/// Body for `POST /api/shared-links`.
fn create_body(target: &SharedLinkTarget, options: &SharedLinkOptions) -> serde_json::Value {
    let mut body = options_body(options);
    match target {
        SharedLinkTarget::Album(album_id) => {
            body["type"] = "ALBUM".into();
            body["albumId"] = album_id.clone().into();
        }
        SharedLinkTarget::Assets(asset_ids) => {
            body["type"] = "INDIVIDUAL".into();
            body["assetIds"] = asset_ids.clone().into();
        }
    }
    if let Some(expires_at) = &options.expires_at {
        body["expiresAt"] = expires_at.clone().into();
    }
    body
}

/// Body for `PATCH /api/shared-links/{id}`; `null` clears the expiry.
/// Immich clears the slug when it's left out, so the current one is always sent.
fn edit_body(options: &SharedLinkOptions, slug: Option<&str>) -> serde_json::Value {
    let mut body = options_body(options);
    body["expiresAt"] = options.expires_at.clone().into();
    body["slug"] = slug.into();
    body
}

fn options_body(options: &SharedLinkOptions) -> serde_json::Value {
    let mut body = serde_json::json!({
        "description": options.description,
        "allowDownload": options.allow_download,
        "allowUpload": options.allow_upload,
        "showMetadata": options.show_metadata,
    });
    if !options.password.is_empty() {
        body["password"] = options.password.clone().into();
    }
    body
}

/// Public URL for a link: `{base}/s/{slug}` when it has a slug, `{base}/share/{key}` otherwise.
pub fn share_url(base: &str, link: &SharedLink) -> String {
    let base = base.trim_end_matches('/');
    match link.slug.as_deref().filter(|s| !s.is_empty()) {
        Some(slug) => format!("{base}/s/{slug}"),
        None => format!("{base}/share/{}", link.key),
    }
}

impl ImmichApiClient {
    /// The user's shared links, newest first.
    pub async fn fetch_shared_links(&self) -> Result<Vec<SharedLink>, String> {
        let resp = self
            .send_json(reqwest::Method::GET, "/api/shared-links", None)
            .await?;
        let mut links: Vec<SharedLink> = resp.json().await.map_err(|err| err.to_string())?;
        links.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(links)
    }

    pub async fn create_shared_link(
        &self,
        target: &SharedLinkTarget,
        options: &SharedLinkOptions,
    ) -> Result<SharedLink, String> {
        let body = create_body(target, options);
        let resp = self
            .send_json(reqwest::Method::POST, "/api/shared-links", Some(body))
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn edit_shared_link(
        &self,
        link: &SharedLink,
        options: &SharedLinkOptions,
    ) -> Result<SharedLink, String> {
        let path = format!("/api/shared-links/{}", link.id);
        let body = edit_body(options, link.slug.as_deref());
        let resp = self
            .send_json(reqwest::Method::PATCH, &path, Some(body))
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn delete_shared_link(&self, link_id: &str) -> Result<(), String> {
        let path = format!("/api/shared-links/{link_id}");
        self.send_json(reqwest::Method::DELETE, &path, None)
            .await
            .map(|_| ())
    }

    /// Base URL for public links: the server's configured external domain, or the
    /// address Mimick is connected through when none is set.
    pub async fn shared_link_base(&self) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ServerConfig {
            #[serde(default)]
            external_domain: String,
        }
        let configured = match self
            .send_json(reqwest::Method::GET, "/api/server/config", None)
            .await
        {
            Ok(resp) => resp
                .json::<ServerConfig>()
                .await
                .map(|c| c.external_domain)
                .unwrap_or_default(),
            Err(_) => String::new(),
        };
        if !configured.trim().is_empty() {
            return Ok(configured);
        }
        self.get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> SharedLinkOptions {
        SharedLinkOptions {
            description: "Trip".into(),
            password: String::new(),
            expires_at: None,
            allow_download: true,
            allow_upload: false,
            show_metadata: true,
        }
    }

    fn link(slug: Option<&str>) -> SharedLink {
        serde_json::from_value(serde_json::json!({
            "id": "l1",
            "key": "abc123",
            "slug": slug,
            "type": "ALBUM",
            "createdAt": "2026-10-01T00:00:00.000Z",
            "allowDownload": true,
            "allowUpload": false,
            "showMetadata": true,
            "album": { "id": "a1", "albumName": "Trips" },
            "assets": []
        }))
        .unwrap()
    }

    #[test]
    fn create_body_for_album_and_assets() {
        let album = create_body(&SharedLinkTarget::Album("a1".into()), &options());
        assert_eq!(album["type"], "ALBUM");
        assert_eq!(album["albumId"], "a1");
        assert!(album.get("expiresAt").is_none());
        assert!(album.get("password").is_none());

        let with_expiry = SharedLinkOptions {
            expires_at: Some("2026-11-01T00:00:00Z".into()),
            password: "secret".into(),
            ..options()
        };
        let assets = create_body(&SharedLinkTarget::Assets(vec!["x".into()]), &with_expiry);
        assert_eq!(assets["type"], "INDIVIDUAL");
        assert_eq!(assets["assetIds"], serde_json::json!(["x"]));
        assert_eq!(assets["expiresAt"], "2026-11-01T00:00:00Z");
        assert_eq!(assets["password"], "secret");
    }

    #[test]
    fn edit_body_clears_expiry_with_null() {
        let body = edit_body(&options(), None);
        assert_eq!(body.get("expiresAt"), Some(&serde_json::Value::Null));
        assert!(body.get("changeExpiryTime").is_none());
        assert_eq!(body["allowDownload"], true);
    }

    #[test]
    fn edit_body_keeps_the_slug() {
        assert_eq!(edit_body(&options(), Some("trips"))["slug"], "trips");
        assert_eq!(
            edit_body(&options(), None).get("slug"),
            Some(&serde_json::Value::Null)
        );
    }

    #[test]
    fn share_url_prefers_slug_and_trims_slash() {
        assert_eq!(
            share_url("https://photos.example.com/", &link(None)),
            "https://photos.example.com/share/abc123"
        );
        assert_eq!(
            share_url("https://photos.example.com", &link(Some("trips"))),
            "https://photos.example.com/s/trips"
        );
    }

    #[test]
    fn shared_link_reads_album_name() {
        assert_eq!(link(None).album.unwrap().album_name, "Trips");
    }
}
