//! Exports a support-friendly diagnostics bundle with sensitive local data redacted for privacy.
//!
//! Copies recent log files, state snapshots, and environment summaries
//! into a temporary directory. API keys, URLs, and filesystem paths are
//! scrubbed before the bundle is presented to the user for attachment
//! to issue reports.

use crate::config::Config;
use crate::queue_manager::FileTask;
use crate::state_manager::AppState;
use serde::Serialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Primary entrypoint to create and export a redacted diagnostics zip/bundle folder.
pub fn export_bundle(
    destination_root: &Path,
    state: &AppState,
    config: &Config,
) -> io::Result<PathBuf> {
    let cache_root = crate::profile::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp").join(crate::profile::dir_segment()));
    let data_root = crate::profile::data_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp").join(crate::profile::dir_segment()));
    export_bundle_with_paths(destination_root, state, config, &cache_root, &data_root)
}

/// Helper that generates redactions by specifying explicit caches and data roots for testing.
fn export_bundle_with_paths(
    destination_root: &Path,
    state: &AppState,
    config: &Config,
    cache_root: &Path,
    data_root: &Path,
) -> io::Result<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let bundle_dir = destination_root.join(format!("mimick-diagnostics-{}", timestamp));
    fs::create_dir_all(&bundle_dir)?;

    let summary_path = bundle_dir.join("summary.txt");
    fs::write(summary_path, build_summary(config, state))?;
    fs::write(
        bundle_dir.join("privacy-note.txt"),
        "This bundle intentionally omits API keys, raw logs, server URLs, and full local paths.\n",
    )?;

    write_json_pretty(
        &bundle_dir.join("config.redacted.json"),
        &build_config_export(config),
    )?;
    write_json_pretty(
        &bundle_dir.join("status.redacted.json"),
        &build_state_export(state),
    )?;
    write_json_pretty(
        &bundle_dir.join("retries.redacted.json"),
        &build_retry_export(&cache_path(cache_root, "retries.json"))?,
    )?;
    write_json_pretty(
        &bundle_dir.join("synced_index.redacted.json"),
        &build_sync_index_export(&cache_path(data_root, "synced_index.json"))?,
    )?;

    Ok(bundle_dir)
}

/// Format system environment and configuration state as plain text.
fn build_summary(config: &Config, state: &AppState) -> String {
    let mut lines = vec![
        "Mimick diagnostics export".to_string(),
        format!("Version: {}", env!("CARGO_PKG_VERSION")),
    ];
    lines.extend(state_lines(state));
    lines.extend(config_lines(config));
    lines.push(
        "Sensitive data policy: URLs, API key, logs, and full local paths omitted".to_string(),
    );
    lines.push(String::new());
    lines.push("Recent queue events:".to_string());
    lines.extend(event_lines(state));
    lines.join("\n")
}

fn or_none(value: Option<&str>) -> &str {
    value.unwrap_or("none")
}

/// File names only: full local paths are omitted from the export.
fn redacted_or_none(path: Option<&str>) -> String {
    path.map(redact_path_hint)
        .unwrap_or_else(|| "none".to_string())
}

fn hour_or_disabled(hour: Option<u8>) -> String {
    hour.map(|h| h.to_string())
        .unwrap_or_else(|| "disabled".to_string())
}

fn state_lines(state: &AppState) -> Vec<String> {
    vec![
        format!("App status: {}", state.status),
        format!("Paused: {}", state.paused),
        format!("Pause reason: {}", or_none(state.pause_reason.as_deref())),
        format!("Watched folder count: {}", state.watched_folder_count),
        format!(
            "Active server route: {}",
            or_none(state.active_server_route.as_deref())
        ),
        format!("Queue size: {}", state.queue_size),
        format!("Processed count: {}", state.processed_count),
        format!("Failed count: {}", state.failed_count),
        format!(
            "Current file: {}",
            redacted_or_none(state.current_file.as_deref())
        ),
        format!(
            "Last completed file: {}",
            redacted_or_none(state.last_completed_file.as_deref())
        ),
        format!("Last error: {}", or_none(state.last_error.as_deref())),
        format!(
            "Suggested fix: {}",
            or_none(state.last_error_guidance.as_deref())
        ),
    ]
}

fn config_lines(config: &Config) -> Vec<String> {
    let data = &config.data;
    vec![
        format!("Configured watch paths: {}", data.watch_paths.len()),
        format!(
            "Pause on metered network: {}",
            data.pause_on_metered_network
        ),
        format!("Pause on battery power: {}", data.pause_on_battery_power),
        format!("Background sync enabled: {}", data.background_sync_enabled),
        format!("Notifications enabled: {}", data.notifications_enabled),
        format!("Startup catchup mode: {:?}", data.startup_catchup_mode),
        format!("Upload concurrency: {}", data.upload_concurrency),
        format!(
            "Quiet hours start: {}",
            hour_or_disabled(data.quiet_hours_start)
        ),
        format!(
            "Quiet hours end: {}",
            hour_or_disabled(data.quiet_hours_end)
        ),
    ]
}

fn event_lines(state: &AppState) -> Vec<String> {
    state
        .recent_events
        .iter()
        .map(|event| {
            format!(
                "- {} [{}] attempts={} detail={}",
                redact_path_hint(&event.path),
                event.status,
                event.attempts,
                or_none(event.detail.as_deref())
            )
        })
        .collect()
}

/// Resolve a named file within the system cache root.
fn cache_path(cache_root: &Path, name: &str) -> PathBuf {
    cache_root.join(name)
}

/// Safe, fully redacted structural copy of Config for user privacy.
#[derive(Serialize)]
struct RedactedConfigExport {
    /// True if internal URL is actively configured.
    internal_url_enabled: bool,
    /// True if external URL is actively configured.
    external_url_enabled: bool,
    /// Number of configured watch paths.
    watch_path_count: usize,
    /// Number of watch paths targeting custom albums.
    watch_paths_with_custom_album: usize,
    /// Number of watch paths using extension or size filters.
    watch_paths_with_rules: usize,
    /// True if autostart/startup is enabled.
    run_on_startup: bool,
    /// True if transfer pauses on metered network.
    pause_on_metered_network: bool,
    /// True if transfer pauses when running on battery.
    pause_on_battery_power: bool,
    /// True if filesystem watcher is active in background.
    background_sync_enabled: bool,
    /// True if user notifications are enabled.
    notifications_enabled: bool,
    /// Current catchup strategy mode string representation.
    startup_catchup_mode: String,
    /// Number of parallel uploads allowed.
    upload_concurrency: u8,
    /// Hour of the day when quiet window begins, if any.
    quiet_hours_start: Option<u8>,
    /// Hour of the day when quiet window ends, if any.
    quiet_hours_end: Option<u8>,
}

/// Redacted view of a transfer event to avoid exposing private filenames.
#[derive(Serialize)]
struct RedactedQueueEvent {
    /// Redacted filename suffix without path components.
    path_hint: String,
    /// Operation outcome status string.
    status: String,
    /// Detailed diagnostic warning or cause if failed.
    detail: Option<String>,
    /// Number of transfer attempts.
    attempts: u32,
    /// Timestamp when this event took place.
    timestamp: f64,
}

/// Redacted snapshot of persistent app context status.
#[derive(Serialize)]
struct RedactedStateExport {
    /// App status label.
    status: String,
    /// True if transfers are currently paused.
    paused: bool,
    /// User or environmental cause of pause.
    pause_reason: Option<String>,
    /// Number of directories being watched.
    watched_folder_count: usize,
    /// Connection route label.
    active_server_route: Option<String>,
    /// Number of active items in transmission queue.
    queue_size: usize,
    /// Total number of items queued this session.
    total_queued: usize,
    /// Number of successfully processed items.
    processed_count: usize,
    /// Number of items that failed transmission.
    failed_count: usize,
    /// Percent progress of active batch.
    progress: u8,
    /// Unix timestamp of last successful remote sync.
    last_successful_sync_at: Option<f64>,
    /// File currently being processed.
    current_file: Option<String>,
    /// Last processed file.
    last_completed_file: Option<String>,
    /// Last recorded network or logic error string.
    last_error: Option<String>,
    /// Troubleshooting instructions for the last error.
    last_error_guidance: Option<String>,
    /// Total count of diagnostic exports generated.
    diagnostics_exports: usize,
    /// Log of recent queue event summaries.
    recent_events: Vec<RedactedQueueEvent>,
}

/// Quantitative summaries of files waiting to be retried.
#[derive(Serialize)]
struct RedactedRetryExport {
    /// Total number of tasks pending retry.
    total_retry_items: usize,
    /// Number of tasks requiring only metadata linking.
    reassociate_only_items: usize,
    /// Number of tasks destined for specific albums.
    album_targeted_items: usize,
}

/// Redacted metrics from the local synced database index.
#[derive(Serialize)]
struct RedactedSyncIndexExport {
    /// Total synced records.
    total_entries: usize,
    /// Synced records belonging to named albums.
    album_named_entries: usize,
    /// Synced records with verified album IDs.
    album_id_entries: usize,
}

/// Populate redacted configuration statistics from live config values.
fn build_config_export(config: &Config) -> RedactedConfigExport {
    let watch_paths_with_custom_album = config
        .data
        .watch_paths
        .iter()
        .filter(|entry| entry.album_name().is_some_and(|name| !name.is_empty()))
        .count();
    let watch_paths_with_rules = config
        .data
        .watch_paths
        .iter()
        .filter(|entry| {
            let rules = entry.rules();
            rules.ignore_hidden
                || rules.max_file_size_mb.is_some()
                || !rules.allowed_extensions.is_empty()
        })
        .count();

    RedactedConfigExport {
        internal_url_enabled: config.data.internal_url_enabled,
        external_url_enabled: config.data.external_url_enabled,
        watch_path_count: config.data.watch_paths.len(),
        watch_paths_with_custom_album,
        watch_paths_with_rules,
        run_on_startup: config.data.run_on_startup,
        pause_on_metered_network: config.data.pause_on_metered_network,
        pause_on_battery_power: config.data.pause_on_battery_power,
        background_sync_enabled: config.data.background_sync_enabled,
        notifications_enabled: config.data.notifications_enabled,
        startup_catchup_mode: format!("{:?}", config.data.startup_catchup_mode),
        upload_concurrency: config.data.upload_concurrency,
        quiet_hours_start: config.data.quiet_hours_start,
        quiet_hours_end: config.data.quiet_hours_end,
    }
}

/// Map live state values into redacted serialization format.
fn build_state_export(state: &AppState) -> RedactedStateExport {
    RedactedStateExport {
        status: state.status.clone(),
        paused: state.paused,
        pause_reason: state.pause_reason.clone(),
        watched_folder_count: state.watched_folder_count,
        active_server_route: state.active_server_route.clone(),
        queue_size: state.queue_size,
        total_queued: state.total_queued,
        processed_count: state.processed_count,
        failed_count: state.failed_count,
        progress: state.progress,
        last_successful_sync_at: state.last_successful_sync_at,
        current_file: state.current_file.as_deref().map(redact_path_hint),
        last_completed_file: state.last_completed_file.as_deref().map(redact_path_hint),
        last_error: state.last_error.clone(),
        last_error_guidance: state.last_error_guidance.clone(),
        diagnostics_exports: state.diagnostics_exports,
        recent_events: state
            .recent_events
            .iter()
            .map(|event| RedactedQueueEvent {
                path_hint: redact_path_hint(&event.path),
                status: event.status.clone(),
                detail: event.detail.clone(),
                attempts: event.attempts,
                timestamp: event.timestamp,
            })
            .collect(),
    }
}

/// Summarize items in the retry log without revealing their names.
fn build_retry_export(retry_path: &Path) -> io::Result<RedactedRetryExport> {
    let tasks = if retry_path.exists() {
        let content = fs::read_to_string(retry_path)?;
        serde_json::from_str::<Vec<FileTask>>(&content).unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok(RedactedRetryExport {
        total_retry_items: tasks.len(),
        reassociate_only_items: tasks.iter().filter(|task| task.reassociate_only).count(),
        album_targeted_items: tasks
            .iter()
            .filter(|task| task.album_id.is_some() || task.album_name.is_some())
            .count(),
    })
}

/// Summarize database index composition to avoid detailing private folder contents.
fn build_sync_index_export(sync_index_path: &Path) -> io::Result<RedactedSyncIndexExport> {
    if !sync_index_path.exists() {
        return Ok(RedactedSyncIndexExport {
            total_entries: 0,
            album_named_entries: 0,
            album_id_entries: 0,
        });
    }

    let content = fs::read_to_string(sync_index_path)?;
    let json = serde_json::from_str::<serde_json::Value>(&content).unwrap_or_default();
    let files = json
        .get("files")
        .and_then(|files| files.as_object())
        .cloned()
        .unwrap_or_default();

    let mut album_named_entries = 0usize;
    let mut album_id_entries = 0usize;
    for record in files.values() {
        if record
            .get("album_name")
            .and_then(|value| value.as_str())
            .is_some_and(|name| !name.is_empty())
        {
            album_named_entries += 1;
        }
        if record
            .get("album_id")
            .and_then(|value| value.as_str())
            .is_some_and(|id| !id.is_empty())
        {
            album_id_entries += 1;
        }
    }

    Ok(RedactedSyncIndexExport {
        total_entries: files.len(),
        album_named_entries,
        album_id_entries,
    })
}

/// Helper to write formatted, indented JSON structures to disk.
fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let content = serde_json::to_string_pretty(value)?;
    fs::write(path, content)
}

/// Redact the absolute directory path, preserving only the trailing filename segment.
fn redact_path_hint(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "[path hidden]".to_string())
}

#[cfg(test)]
mod tests {
    use super::{build_summary, export_bundle_with_paths};
    use crate::config::{Config, ConfigData, WatchPathEntry};
    use crate::state_manager::{AppState, QueueEvent};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_build_summary_contains_recent_events_and_omits_api_key() {
        let config = Config {
            data: ConfigData {
                watch_paths: vec![WatchPathEntry::Simple("/photos".into())],
                pause_on_metered_network: true,
                pause_on_battery_power: false,
                ..ConfigData::default()
            },
            config_file: PathBuf::from("config.json"),
        };
        let mut state = AppState {
            status: "paused".into(),
            paused: true,
            pause_reason: Some("Paused by user".into()),
            ..AppState::default()
        };
        state.recent_events.push(QueueEvent {
            path: "/photos/a.jpg".into(),
            status: "failed".into(),
            detail: Some("Queued for retry".into()),
            attempts: 2,
            timestamp: 1.0,
        });

        let summary = build_summary(&config, &state);
        assert!(summary.contains("App status: paused"));
        assert!(summary.contains("Watched folder count: 0"));
        assert!(summary.contains("Configured watch paths: 1"));
        assert!(
            summary.contains(
                "Sensitive data policy: URLs, API key, logs, and full local paths omitted"
            )
        );
        assert!(summary.contains("a.jpg [failed] attempts=2"));
        assert!(!summary.contains("/photos/a.jpg [failed] attempts=2"));
    }

    #[test]
    fn test_build_summary_keeps_section_order_and_defaults() {
        let config = Config {
            data: ConfigData::default(),
            config_file: PathBuf::from("config.json"),
        };
        let summary = build_summary(&config, &AppState::default());
        let lines: Vec<&str> = summary.lines().collect();
        let position = |prefix: &str| {
            lines
                .iter()
                .position(|line| line.starts_with(prefix))
                .unwrap_or_else(|| panic!("missing line starting with {prefix:?}"))
        };
        assert_eq!(lines[0], "Mimick diagnostics export");
        assert!(position("Version:") < position("App status:"));
        assert!(position("Suggested fix:") < position("Configured watch paths:"));
        assert!(position("Quiet hours end:") < position("Sensitive data policy:"));
        assert_eq!(lines.last(), Some(&"Recent queue events:"));
        assert!(summary.contains("Pause reason: none"));
        assert!(summary.contains("Current file: none"));
        assert!(summary.contains("Quiet hours start: disabled"));
    }

    #[test]
    fn test_export_bundle_writes_redacted_files_only() {
        let dir = tempdir().unwrap();
        let dest_root = dir.path().join("exports");
        let cache_root = dir.path().join("cache");
        let data_root = dir.path().join("data");
        let config_root = dir.path().join("config");
        fs::create_dir_all(&cache_root).unwrap();
        fs::create_dir_all(&data_root).unwrap();
        fs::create_dir_all(&config_root).unwrap();

        let config_path = config_root.join("config.json");
        fs::write(&config_path, "{\"internal_url\":\"http://localhost\"}").unwrap();
        fs::write(cache_root.join("status.json"), "{\"status\":\"idle\"}").unwrap();
        fs::write(cache_root.join("retries.json"), "[]").unwrap();
        fs::write(data_root.join("synced_index.json"), "{\"files\":{}}").unwrap();
        fs::write(cache_root.join("mimick.log"), "hello log").unwrap();

        let config = Config {
            data: ConfigData::default(),
            config_file: config_path,
        };
        let state = AppState::default();

        let bundle_dir =
            export_bundle_with_paths(&dest_root, &state, &config, &cache_root, &data_root).unwrap();
        assert!(bundle_dir.join("summary.txt").exists());
        assert!(bundle_dir.join("privacy-note.txt").exists());
        assert!(bundle_dir.join("config.redacted.json").exists());
        assert!(bundle_dir.join("status.redacted.json").exists());
        assert!(bundle_dir.join("retries.redacted.json").exists());
        assert!(bundle_dir.join("synced_index.redacted.json").exists());
        assert!(!bundle_dir.join("config.json").exists());
        assert!(!bundle_dir.join("status.json").exists());
        assert!(!bundle_dir.join("retries.json").exists());
        assert!(!bundle_dir.join("synced_index.json").exists());
        assert!(!bundle_dir.join("mimick.log").exists());

        let config_export = fs::read_to_string(bundle_dir.join("config.redacted.json")).unwrap();
        assert!(config_export.contains("\"watch_path_count\": 0"));
        assert!(!config_export.contains("http://localhost"));

        let retry_export = fs::read_to_string(bundle_dir.join("retries.redacted.json")).unwrap();
        assert!(retry_export.contains("\"total_retry_items\": 0"));
    }
}
