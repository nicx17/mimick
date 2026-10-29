/// Detailed Immich library album representation.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct LibraryAlbum {
    /// Unique album identifier.
    pub id: String,
    /// Name of the album.
    #[serde(rename = "albumName")]
    pub album_name: String,
    /// Count of assets contained in the album.
    #[serde(rename = "assetCount")]
    pub asset_count: u32,
    /// Asset ID used as the album's cover thumbnail.
    #[serde(rename = "albumThumbnailAssetId")]
    pub thumbnail_asset_id: Option<String>,
    /// ISO 8601 creation timestamp.
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// ISO 8601 modification timestamp.
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    #[serde(rename = "startDate", default)]
    pub start_date: Option<String>,
    #[serde(rename = "endDate", default)]
    pub end_date: Option<String>,
    /// User description of the album.
    #[serde(default)]
    pub description: String,
    /// Users associated with this album (owner + editors + viewers).
    /// In Immich v3 the top-level `ownerId` was removed; ownership is now
    /// represented as an entry in this list with `role == "owner"`.
    #[serde(rename = "albumUsers", default)]
    pub album_users: Vec<AlbumUser>,
    /// Immich's flag: shared with other users or through a shared link.
    #[serde(default)]
    pub shared: bool,
    #[serde(default)]
    pub order: Option<SortOrder>,
    #[serde(rename = "isActivityEnabled", default)]
    pub is_activity_enabled: bool,
    #[serde(rename = "hasSharedLink", default)]
    pub has_shared_link: bool,
}

impl LibraryAlbum {
    /// Extract the owner's user ID from the `albumUsers` list.
    /// Returns an empty string when no owner entry is present.
    pub fn owner_id(&self) -> &str {
        self.album_users
            .iter()
            .find(|u| u.role == "owner")
            .map(|u| u.user.id.as_str())
            .unwrap_or("")
    }

    /// Whether the album is shared with other users or through a shared link.
    pub fn is_shared(&self) -> bool {
        self.shared || self.album_users.len() > 1
    }

    pub fn role_of(&self, user_id: &str) -> Option<&str> {
        self.album_users
            .iter()
            .find(|u| u.user.id == user_id)
            .map(|u| u.role.as_str())
    }

    /// Owners and editors can add and remove photos; viewers can't.
    pub fn can_edit_assets(&self, user_id: &str) -> bool {
        self.owner_id().is_empty() || matches!(self.role_of(user_id), Some("owner" | "editor"))
    }
}

/// A user entry within an album's user list.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct AlbumUser {
    /// User details.
    pub user: AlbumUserInfo,
    /// Role in the album ("owner", "editor", or "viewer").
    pub role: String,
}

/// Minimal user identity nested inside an `AlbumUser`.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct AlbumUserInfo {
    /// Unique user identifier.
    pub id: String,
    /// Display name (absent in some older album list responses).
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
}

/// A user on the server, as listed by `GET /api/users` (for sharing).
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct UserSummary {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
}

/// Detailed Immich library asset representation.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct LibraryAsset {
    /// Unique asset identifier on the server.
    pub id: String,
    /// Original file name.
    #[serde(rename = "originalFileName")]
    pub filename: String,
    /// Original MIME type.
    #[serde(rename = "originalMimeType")]
    pub mime_type: String,
    /// Asset file creation timestamp.
    #[serde(rename = "fileCreatedAt")]
    pub created_at: String,
    /// Asset kind (e.g. `"IMAGE"` or `"VIDEO"`).
    #[serde(rename = "type")]
    pub asset_type: String,
    /// Thumbhash representation for preview blur effects.
    pub thumbhash: Option<String>,
    /// Display width in pixels.
    pub width: Option<u32>,
    /// Display height in pixels.
    pub height: Option<u32>,
    /// Canonical lowercase SHA-1 checksum.
    #[serde(default, deserialize_with = "deserialize_checksum_to_hex")]
    pub checksum: Option<String>,
    /// EXIF block; Immich keeps pixel dimensions here, not at the top level.
    #[serde(rename = "exifInfo", default)]
    pub exif_info: Option<ExifInfo>,
    /// Whether the asset is marked as a favorite.
    #[serde(rename = "isFavorite", default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub visibility: String,
    /// Whether the asset was cropped or rotated in Immich.
    #[serde(rename = "isEdited", default)]
    pub is_edited: bool,
    /// The owner's user id; only the owner can edit an asset's info or location.
    #[serde(rename = "ownerId", default)]
    pub owner_id: String,
    #[serde(rename = "localDateTime", default)]
    pub local_date_time: Option<String>,
}

impl LibraryAsset {
    pub fn is_archived(&self) -> bool {
        self.visibility == "archive"
    }
}

/// Immich returns asset checksums as base64-encoded SHA1, while Mimick computes
/// and stores them as lowercase hex. Normalize on deserialization so every
/// comparison site (album diff, deletion lookup, sync index) sees the same
/// canonical form.
fn deserialize_checksum_to_hex<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.as_deref().and_then(normalize_checksum_to_hex))
}

pub fn normalize_checksum_to_hex(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() == 40 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(trimmed.to_ascii_lowercase());
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(trimmed.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(trimmed.as_bytes()))
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed.as_bytes()))
        .ok()?;
    if bytes.len() != 20 {
        return None;
    }
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Structured filters passed down search operations.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataSearchFilters {
    /// Inferred or original file name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_file_name: Option<String>,
    /// User description or caption search query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Match against OCR-extracted text inside images. Distinct from
    /// `description` (user-set caption) — Immich indexes recognised text
    /// during ML processing and exposes it as its own filter dimension on
    /// both `MetadataSearchDto` and `SmartSearchDto`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr: Option<String>,
    /// `"IMAGE"` or `"VIDEO"`; `None` returns both.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub asset_type: Option<String>,
    /// ISO 8601 inclusive lower bound on `fileCreatedAt`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taken_after: Option<String>,
    /// ISO 8601 inclusive upper bound on `fileCreatedAt`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taken_before: Option<String>,
    /// Camera make.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub make: Option<String>,
    /// Camera model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Camera lens model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens_model: Option<String>,
    /// City location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// State location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// City location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    /// True to only return favorited assets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_favorite: Option<bool>,
    /// True to only return motion photos.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_motion: Option<bool>,
    /// True to only return assets not associated with any albums.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_not_in_album: Option<bool>,
    /// True to only return assets having valid EXIF.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_exif: Option<bool>,
    /// True to also return deleted assets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_deleted: Option<bool>,
    /// List of person identifiers to match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_ids: Option<Vec<String>>,
    /// List of tag identifiers to match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_ids: Option<Vec<String>>,
    /// Desired sorting order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<SortOrder>,
    /// Star rating (1-5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<u32>,
    /// Restrict results to specific albums.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_ids: Option<Vec<String>>,
    /// True to only return encoded (transcoded) assets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_encoded: Option<bool>,
    /// True to only return offline assets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_offline: Option<bool>,
    /// Restrict to a specific external library.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_id: Option<String>,
    /// ISO 8601 lower bound on asset creation timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_after: Option<String>,
    /// ISO 8601 upper bound on asset creation timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_before: Option<String>,
    /// ISO 8601 lower bound on asset update timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_after: Option<String>,
    /// ISO 8601 upper bound on asset update timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_before: Option<String>,
    /// ISO 8601 lower bound on trashed timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trashed_after: Option<String>,
    /// ISO 8601 upper bound on trashed timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trashed_before: Option<String>,
    /// Visibility filter: "archive", "timeline", "hidden", "locked".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    /// True to include person face data in results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_people: Option<bool>,
    /// True to include stacked asset data in results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_stacked: Option<bool>,
}

/// Direction of sort results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    /// Ascending order.
    Asc,
    /// Descending order.
    Desc,
}

/// Immich server asset count statistics.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct ServerStats {
    /// Number of image assets.
    pub images: u64,
    /// Number of video assets.
    pub videos: u64,
    /// Total assets.
    pub total: u64,
}

/// Immich server version and build details (`GET /api/server/about`).
/// Everything except `version` is optional; older servers omit some fields.
#[derive(Debug, Clone, Default, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerAbout {
    /// Semver string of the Immich instance.
    pub version: String,
    pub version_url: Option<String>,
    pub repository: Option<String>,
    pub repository_url: Option<String>,
    pub source_ref: Option<String>,
    pub source_commit: Option<String>,
    pub source_url: Option<String>,
    pub build: Option<String>,
    pub build_url: Option<String>,
    pub build_image: Option<String>,
    pub build_image_url: Option<String>,
    pub nodejs: Option<String>,
    pub ffmpeg: Option<String>,
    pub imagemagick: Option<String>,
    pub libvips: Option<String>,
    pub exiftool: Option<String>,
}

/// One entry of the server's upgrade history (`GET /api/server/version-history`).
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VersionHistoryEntry {
    pub version: String,
    /// ISO 8601 timestamp of when this version was first started.
    pub created_at: String,
}

/// Per-user usage row from `/api/server/statistics`.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageByUser {
    pub user_id: String,
    pub user_name: String,
    #[serde(default)]
    pub photos: u64,
    #[serde(default)]
    pub videos: u64,
    #[serde(default)]
    pub usage: u64,
    #[serde(default)]
    pub quota_size_in_bytes: Option<u64>,
}

/// Server-wide statistics (admin-only). Returned by `/api/server/statistics`.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatistics {
    #[serde(default)]
    pub photos: u64,
    #[serde(default)]
    pub videos: u64,
    #[serde(default)]
    pub usage: u64,
    #[serde(default)]
    pub usage_by_user: Vec<UsageByUser>,
}

/// A recognized person returned by Immich's facial recognition.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Person {
    /// Unique identifier of the person.
    pub id: String,
    /// Name assigned to the person.
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "isHidden")]
    pub is_hidden: bool,
    #[serde(default, rename = "birthDate")]
    pub birth_date: Option<String>,
    #[serde(default, rename = "isFavorite")]
    pub is_favorite: bool,
}

/// List wrapper of recognized people returned by Immich.
#[derive(Debug, Clone, serde::Deserialize)]
pub(super) struct PeopleResponse {
    /// Internal people list array.
    pub(super) people: Vec<Person>,
}

/// Discovered item in an Immich explore page section.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExploreItem {
    /// Text value representing the item category (e.g. city or tag name).
    pub value: String,
    /// Associated representative asset for preview.
    pub data: LibraryAsset,
}

/// Discovered category section on the Immich explore page (e.g. places or tags).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExploreSection {
    /// Categorisation field name.
    #[serde(rename = "fieldName")]
    pub field_name: String,
    /// Discovered list of explore items.
    pub items: Vec<ExploreItem>,
}

/// Container matching assets returned alongside their EXIF metadata.
#[derive(Debug, Clone, serde::Deserialize)]
pub(super) struct AssetWithExif {
    /// Unique asset ID.
    pub(super) id: String,
    /// EXIF info details payload if populated.
    #[serde(rename = "exifInfo", default)]
    pub(super) exif_info: Option<ExifInfo>,
}

/// Search results response wrapper for EXIF queries.
#[derive(Debug, serde::Deserialize)]
pub(super) struct ExifSearchResponse {
    /// Contained assets list wrapper.
    pub(super) assets: ExifSearchAssets,
}

/// Asset items list returned inside an EXIF search response.
#[derive(Debug, serde::Deserialize)]
pub(super) struct ExifSearchAssets {
    /// Individual items within search page.
    pub(super) items: Vec<AssetWithExif>,
    /// Pagination pointer to the next search page, if any.
    #[serde(rename = "nextPage", default)]
    pub(super) next_page: Option<String>,
}

/// One city with a representative asset ID for thumbnail display.
/// Places display category representation holding the representative thumbnail asset.
pub struct PlaceItem {
    /// Name of the city location.
    pub city: String,
    /// Asset ID mapped as the representative cover image.
    pub asset_id: String,
}

/// Full EXIF metadata schema properties returned by Immich.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExifInfo {
    /// Camera manufacturer name.
    #[serde(default)]
    pub make: Option<String>,
    /// Camera model.
    #[serde(default)]
    pub model: Option<String>,
    /// Camera lens model name.
    #[serde(default)]
    pub lens_model: Option<String>,
    /// F-number aperture value.
    #[serde(default)]
    pub f_number: Option<f64>,
    /// Focal length in millimeters.
    #[serde(default)]
    pub focal_length: Option<f64>,
    /// ISO speed rating.
    #[serde(default)]
    pub iso: Option<u32>,
    /// Shutter speed string representation.
    #[serde(default)]
    pub exposure_time: Option<String>,
    /// Uncompressed file size in bytes.
    #[serde(default)]
    pub file_size_in_byte: Option<u64>,
    /// Original capture datetime string.
    #[serde(default)]
    pub date_time_original: Option<String>,
    /// IANA zone the photo was taken in, when Immich knows it.
    #[serde(default)]
    pub time_zone: Option<String>,
    /// Discovered city name.
    #[serde(default)]
    pub city: Option<String>,
    /// Discovered state or region name.
    #[serde(default)]
    pub state: Option<String>,
    /// Discovered country name.
    #[serde(default)]
    pub country: Option<String>,
    /// GPS Latitude coordinate in decimal degrees.
    #[serde(default)]
    pub latitude: Option<f64>,
    /// GPS Longitude coordinate in decimal degrees.
    #[serde(default)]
    pub longitude: Option<f64>,
    /// Metadata description text.
    #[serde(default)]
    pub description: Option<String>,
    /// Image width in pixels.
    #[serde(default)]
    pub exif_image_width: Option<u32>,
    /// Image height in pixels.
    #[serde(default)]
    pub exif_image_height: Option<u32>,
    #[serde(default)]
    pub rating: Option<i8>,
}

/// Asset details metadata payload.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDetails {
    /// Extracted EXIF metadata block.
    #[serde(default)]
    pub exif_info: Option<ExifInfo>,
    /// Whether the asset is marked as a favorite in Immich.
    #[serde(default)]
    pub is_favorite: bool,
    /// Tags applied to the asset.
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub people: Vec<Person>,
    #[serde(default)]
    pub local_date_time: Option<String>,
}

/// An Immich tag. `value` is the full path for nested tags (e.g. `Trips/2024`),
/// while `name` is only the last segment.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub value: String,
    /// Optional `#rrggbb` color set in Immich.
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_thumbnail_asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<SortOrder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_activity_enabled: Option<bool>,
}

/// Editable asset fields for `PUT /api/assets/{id}`; `None` fields are left unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// RFC3339 timestamp with offset, e.g. `2024-01-15T19:55:15+05:30`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_time_original: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_favorite: Option<bool>,
}

/// Type of asset thumbnails requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThumbnailSize {
    Thumbnail,
    Preview,
    /// Server-generated full-resolution copy; only present when the server
    /// has the "save full-size image" job enabled. 404s otherwise.
    Fullsize,
}

impl ThumbnailSize {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            ThumbnailSize::Thumbnail => "thumbnail",
            ThumbnailSize::Preview => "preview",
            ThumbnailSize::Fullsize => "fullsize",
        }
    }
}

/// Search response wrapper.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct SearchResponse {
    /// Inner assets block.
    pub(crate) assets: SearchAssetSection,
}

/// Paginated asset list section within a search response.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct SearchAssetSection {
    /// Assets returned in the search.
    pub(crate) items: Vec<LibraryAsset>,
    /// Authoritative pagination signal from Immich. Some(page) when more
    /// results exist, None when the search is exhausted. We only need its
    /// presence — has_more is computed as `next_page.is_some()`.
    #[serde(rename = "nextPage", default)]
    pub(crate) next_page: Option<String>,
}
