//! Upload-related helper functions: MIME detection, timezone fixup, file timestamps.
//!
//! Pure utility functions shared by the upload pipeline. Includes IANA
//! timezone resolution from `/etc/localtime` or `$TZ`, ISO 8601
//! formatting, and filesystem timestamp normalization.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{SecondsFormat, TimeZone, Utc};
use reqwest::Client;

/// Resolve the standard MIME type for a given local file path.
pub(super) fn mime_for_path(path: &Path) -> &'static str {
    crate::media_kinds::mime_for_path(path)
}

/// Make a PUT request to the server to update the timezone offset of a specific asset.
pub(super) async fn apply_asset_timezone_fixup(
    client: &Client,
    api_key: &str,
    base_url: &str,
    asset_id: &str,
    time_zone: &str,
) {
    let url = format!("{}/api/assets", base_url);
    let body = serde_json::json!({
        "ids": [asset_id],
        "timeZone": time_zone,
    });

    match client
        .put(&url)
        .header("x-api-key", api_key)
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            log::debug!("Updated timezone for asset {} to {}", asset_id, time_zone);
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            log::warn!(
                "Uploaded asset {} but failed to update timezone [{}]: {}",
                asset_id,
                status,
                body
            );
        }
        Err(e) => {
            log::warn!(
                "Uploaded asset {} but timezone update request failed: {}",
                asset_id,
                e
            );
        }
    }
}

/// Retrieve and normalize the creation and modification timestamps of a local file.
pub(super) fn file_timestamps(meta: &std::fs::Metadata) -> (u64, u64) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let created = meta.created().ok().and_then(system_time_to_unix_secs);
    let modified = meta.modified().ok().and_then(system_time_to_unix_secs);
    let (created, modified) = normalize_file_timestamps(created, modified, now);

    (created, modified)
}

/// Convert a standard `SystemTime` to standard unix epoch seconds.
pub(super) fn system_time_to_unix_secs(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// Coalesce and normalize creation/modification timestamps using standard rules.
pub(super) fn normalize_file_timestamps(
    created: Option<u64>,
    modified: Option<u64>,
    now: u64,
) -> (u64, u64) {
    // Birth time is frequently the copy/import time on Linux and for moved files.
    // Use the earliest available filesystem timestamp as the asset creation time so
    // Immich's timeline is closer to the media's original timestamp.
    let created = match (created, modified) {
        (Some(created), Some(modified)) => created.min(modified),
        (Some(created), None) => created,
        (None, Some(modified)) => modified,
        (None, None) => now,
    };

    let modified = modified.unwrap_or(created);

    (created, modified)
}

/// Convert standard unix epoch seconds to standard UTC ISO 8601 formatted string.
pub(super) fn unix_to_utc_iso8601(secs: u64) -> String {
    Utc.timestamp_opt(secs as i64, 0)
        .single()
        .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00.000+00:00".to_string())
}

/// Resolve the user's current local timezone name (IANA format) from system files or environment.
pub(crate) fn local_timezone_name() -> Option<String> {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim().trim_start_matches(':');
        if looks_like_iana_timezone(tz) {
            return Some(tz.to_string());
        }
    }

    if let Ok(target) = std::fs::read_link("/etc/localtime")
        && let Some(path) = target.to_str()
        && let Some((_, tz)) = path.split_once("/zoneinfo/")
        && looks_like_iana_timezone(tz)
    {
        return Some(tz.to_string());
    }

    if let Ok(tz) = std::fs::read_to_string("/etc/timezone") {
        let tz = tz.trim();
        if looks_like_iana_timezone(tz) {
            return Some(tz.to_string());
        }
    }

    None
}

/// Check if the timezone name string strictly matches IANA zone style.
pub(super) fn looks_like_iana_timezone(value: &str) -> bool {
    !value.is_empty() && value.contains('/') && !value.contains(' ')
}
