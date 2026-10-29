use serde::{Deserialize, Serialize};

use super::{StartupCatchupMode, WatchPathEntry};

/// Date headings between photos in the Photos timeline.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DateHeadings {
    #[default]
    Day,
    Month,
    Off,
}

impl DateHeadings {
    pub const ALL: [DateHeadings; 3] = [DateHeadings::Day, DateHeadings::Month, DateHeadings::Off];
}

/// Configuration schema driving application behavior and connection settings.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConfigData {
    /// Active local Immich server connection address.
    #[serde(default)]
    pub internal_url: String,
    /// Active remote or external connection address.
    #[serde(default)]
    pub external_url: String,
    /// True if internal URL connection is enabled.
    #[serde(default = "default_true")]
    pub internal_url_enabled: bool,
    /// True if external URL connection is enabled.
    #[serde(default = "default_true")]
    pub external_url_enabled: bool,
    /// List of watched local directory entries.
    #[serde(default)]
    pub watch_paths: Vec<WatchPathEntry>,
    /// True if application should launch automatically at user login.
    #[serde(default)]
    pub run_on_startup: bool,
    /// Pause transfers when metered connections are detected.
    #[serde(default)]
    pub pause_on_metered_network: bool,
    /// Pause transfers when system runs on battery.
    #[serde(default)]
    pub pause_on_battery_power: bool,
    /// Whether automatic background monitoring/upload discovery is enabled.
    #[serde(default)]
    pub background_sync_enabled: bool,
    /// Whether desktop notifications (sync summary, connectivity lost, etc.) are shown.
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    /// Default catch-up scanning strategy when background scan starts.
    #[serde(default)]
    pub startup_catchup_mode: StartupCatchupMode,
    /// Number of parallel upload workers (1–10). Defaults to 3.
    #[serde(default = "default_upload_concurrency")]
    pub upload_concurrency: u8,
    /// Quiet-hours window start (local clock hour, 0-23). `None` means disabled.
    #[serde(default)]
    pub quiet_hours_start: Option<u8>,
    /// Quiet-hours window end (local clock hour, 0-23, exclusive).
    #[serde(default)]
    pub quiet_hours_end: Option<u8>,
    /// Whether the built-in library viewer is the primary window.
    #[serde(default)]
    pub library_view_enabled: bool,
    /// Target folder for asset downloads from the library viewer.
    #[serde(default)]
    pub download_target_path: Option<String>,
    /// When true, lightbox loads original full-resolution image instead of preview.
    #[serde(default)]
    pub library_preview_full_resolution: bool,
    /// Grid thumbnail quality: "auto", "thumbnail", "preview". Defaults to auto.
    #[serde(default = "default_grid_quality")]
    pub library_grid_quality: String,
    /// Total on-disk cache cap in megabytes across all subcaches
    /// (thumbnails, raw_decode, exif, video, preview, open-in).
    /// Pruning runs once at startup. Defaults to 2000 MB.
    #[serde(default = "default_cache_disk_cap_mb")]
    pub cache_disk_cap_mb: u32,
    /// When true, decoded RAW textures are cached to disk for faster re-opens.
    /// Disable to save storage; each cached file is a full-resolution PNG.
    #[serde(default)]
    pub raw_decode_cache_enabled: bool,
    /// When true, RAW files are fully demosaiced from sensor data (slow but
    /// highest quality). When false the embedded camera JPEG preview is
    /// extracted instead (near-instant).
    #[serde(default)]
    pub raw_full_decode: bool,
    /// Show people with no assigned name in the Explore view.
    #[serde(default = "default_true")]
    pub show_unnamed_faces: bool,
    /// Include hidden people in the Explore view.
    #[serde(default)]
    pub show_hidden_faces: bool,
    /// Attach XMP sidecar files alongside media during upload. Per-folder
    /// rules can override this global default.
    #[serde(default = "default_true")]
    pub upload_xmp_sidecars: bool,
    /// Border width between grid tiles in pixels (0.0 = edge-to-edge, max 10.0).
    #[serde(default)]
    pub grid_border_width: f32,
    /// Border color as a CSS hex string (e.g. "#ffffff"). Defaults to white.
    #[serde(default = "default_grid_border_color")]
    pub grid_border_color: String,
    /// Lightbox video playback volume (0.0–1.0).
    #[serde(default = "default_video_volume")]
    pub video_volume: f64,
    /// Whether lightbox video playback starts muted.
    #[serde(default)]
    pub video_muted: bool,
    /// Map style id (`immich`, a VersaTiles palette, or `osm`); empty means Immich's own.
    #[serde(default)]
    pub library_map_style: String,
    #[serde(default)]
    pub library_date_headings: DateHeadings,
}

impl Default for ConfigData {
    fn default() -> Self {
        Self {
            internal_url: String::new(),
            external_url: String::new(),
            internal_url_enabled: true,
            external_url_enabled: true,
            watch_paths: Vec::new(),
            run_on_startup: false,
            pause_on_metered_network: false,
            pause_on_battery_power: false,
            background_sync_enabled: false,
            notifications_enabled: true,
            startup_catchup_mode: StartupCatchupMode::default(),
            upload_concurrency: default_upload_concurrency(),
            quiet_hours_start: None,
            quiet_hours_end: None,
            library_view_enabled: false,
            download_target_path: None,
            library_preview_full_resolution: false,
            library_grid_quality: default_grid_quality(),
            cache_disk_cap_mb: default_cache_disk_cap_mb(),
            raw_decode_cache_enabled: false,
            raw_full_decode: false,
            show_unnamed_faces: true,
            show_hidden_faces: false,
            upload_xmp_sidecars: true,
            grid_border_width: 0.0,
            grid_border_color: default_grid_border_color(),
            video_volume: default_video_volume(),
            video_muted: false,
            library_map_style: String::new(),
            library_date_headings: DateHeadings::default(),
        }
    }
}

/// Helper to default boolean fields to true during serialization.
fn default_true() -> bool {
    true
}

fn default_grid_quality() -> String {
    "thumbnail".to_string()
}

/// Helper to default parallel upload worker threads to 3.
fn default_upload_concurrency() -> u8 {
    3
}

/// Default total on-disk cache cap in MB.
fn default_cache_disk_cap_mb() -> u32 {
    2000
}

fn default_grid_border_color() -> String {
    "#ffffff".to_string()
}

fn default_video_volume() -> f64 {
    1.0
}
