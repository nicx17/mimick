//! Handles application bootstrap, single-instance wiring, and daemon startup flow.
//!
//! Initialises GTK/Libadwaita, registers the D-Bus application name for
//! single-instance enforcement, and decides whether to present the library
//! window or settings window based on user configuration. Background sync,
//! tray icon, and filesystem monitor are wired up before entering the
//! main event loop.

use gtk::prelude::*;
use libadwaita as adw;

use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[path = "main/file_events.rs"]
mod file_events;
#[path = "main/ui_lifecycle.rs"]
mod ui_lifecycle;
use file_events::{
    refresh_connection_status, run_catchup_scan, run_file_event_loop, run_manual_sync_listener,
};
use tokio::sync::mpsc;
use ui_lifecycle::{
    handle_command_line, install_termination_handler, listen_for_ui_flags, persist_on_shutdown,
    run_tray,
};

mod api_client;
mod app_context;
mod autostart;
mod cache_manager;
mod config;
mod debug_width;
mod diagnostics;
mod library;
mod logging;
mod media_kinds;
mod monitor;
mod notifications;
mod profile;
mod queue_manager;
mod remote_sync;
mod runtime_env;
mod sanitize;
mod settings_window;
mod sidecar;
mod startup_scan;
mod state_manager;
mod sync_index;
mod tray_icon;
mod util;
mod watch_path_display;

use api_client::ImmichApiClient;
use app_context::AppContext;
use config::Config;
use library::state::LibraryState;
use library::thumbnail_cache::ThumbnailCache;
use monitor::{Monitor, MonitorEvent};
use queue_manager::{EnvironmentPolicy, QueueManager};
use state_manager::{AppState, StateManager};
use sync_index::ShardedSyncIndex;

use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming, WriteMode};

/// Shared application context reused by UI entry points and the shutdown path.
static APP_CONTEXT: std::sync::OnceLock<Arc<AppContext>> = std::sync::OnceLock::new();

/// Atomically check and clear a cross-thread boolean flag.
/// Returns true if the flag was set, clearing it in the process.
fn consume_flag(flag: &parking_lot::Mutex<bool>) -> bool {
    let mut f = flag.lock();
    if *f {
        *f = false;
        true
    } else {
        false
    }
}

/// Cross-thread UI requests: the tray task sets them and wakes the GTK thread to consume them.
/// `Arc<Mutex<bool>>` is Send + Sync, so the flags can cross into the tray's Tokio
/// task while the application handle itself stays on the GTK thread.
#[derive(Clone, Default)]
struct UiFlags {
    settings: Arc<Mutex<bool>>,
    library: Arc<Mutex<bool>>,
    quit: Arc<Mutex<bool>>,
    pause: Arc<Mutex<bool>>,
    sync_now: Arc<Mutex<bool>>,
    wake: Arc<tokio::sync::Notify>,
}

#[tokio::main]
async fn main() {
    let _logger = init_logging();

    gtk::gio::resources_register_include!("mimick.gresource")
        .expect("Failed to register bundled GResource");

    if let Err(err) = gstreamer::init() {
        log::warn!("GStreamer init failed, video playback disabled: {}", err);
    }

    let app = adw::Application::builder()
        .application_id(profile::application_id())
        .flags(gtk::gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    let is_primary_instance = Arc::new(AtomicBool::new(false));
    let shared_state = Arc::new(Mutex::new(restored_app_state()));

    // Only the primary instance should initialize background services.
    // Secondary launches remote-control the primary through GTK's single-instance support.
    let is_primary_instance_clone = is_primary_instance.clone();
    let shared_state_startup = shared_state.clone();
    app.connect_startup(move |app| {
        is_primary_instance_clone.store(true, Ordering::SeqCst);
        crate::runtime_env::watch_metered_network();
        start_primary_instance(app, &shared_state_startup);
    });

    app.connect_command_line(handle_command_line);

    app.connect_activate(move |_app| {
        log::debug!("App activated");
    });

    log::info!("GTK application starting up");

    if is_primary_instance.load(Ordering::SeqCst) {
        install_termination_handler(&app);
    }

    app.run();

    if is_primary_instance.load(Ordering::SeqCst) {
        persist_on_shutdown(&shared_state).await;
    }
}

/// Mirror logs to stdout and to a rotating cache file for easier support/debugging.
fn init_logging() -> flexi_logger::LoggerHandle {
    let log_dir = profile::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp").join(profile::dir_segment()));

    // Named profiles (e.g. MIMICK_PROFILE=dev) default to verbose mimick logs
    let default_log_spec = if profile::name().is_some() {
        "mimick=debug,info"
    } else {
        "info"
    };
    let logger = Logger::try_with_env_or_str(default_log_spec)
        .expect("Failed to parse log level")
        .log_to_file(
            FileSpec::default()
                .directory(log_dir)
                .basename("mimick")
                .suppress_timestamp() // "mimick.log" instead of "mimick_2026-03-09_10-33-35.log"
                .suffix("log"),
        )
        .format_for_files(logging::detailed_plain_format)
        .format_for_stdout(logging::detailed_colored_format)
        .rotate(
            Criterion::Size(2_000_000),
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        // Also print to stdout for systemd / terminal users
        .duplicate_to_stdout(Duplicate::All)
        .write_mode(WriteMode::Direct)
        .start()
        .expect("Failed to initialize logger");

    if let Some(name) = profile::name() {
        log::info!(
            "Active profile: {} (state dirs use segment '{}')",
            name,
            profile::dir_segment()
        );
    }
    logger
}

/// Load persisted state, resetting volatile fields that shouldn't survive a restart.
fn restored_app_state() -> AppState {
    let mut saved = StateManager::new().read_state();
    // Any items left in the channel during shutdown were dropped, so we must
    // sync total_queued down to processed_count to clear the stuck queue state.
    saved.total_queued = saved.processed_count;
    saved.queue_size = 0;
    saved.failed_count = 0; // Will be repopulated from retries.json if any
    saved.current_file = None;

    for status in saved.folder_statuses.values_mut() {
        status.pending_count = 0;
    }
    saved.reset_runtime_state();

    AppState {
        status: "idle".to_string(),
        active_workers: 0,
        ..saved
    }
}

/// Set up config, API client, queue, monitor, tray, and background tasks.
/// Runs only in the primary instance, from `connect_startup`.
fn start_primary_instance(app: &adw::Application, shared_state: &Arc<Mutex<AppState>>) {
    log::info!("Mimick primary instance initializing");
    prepare_gtk(app);

    let config = Config::new();
    log_config(&config);
    shared_state.lock().watched_folder_count = config.data.watch_paths.len();

    let background_sync_enabled = config.data.background_sync_enabled;
    let api_client = Arc::new(build_api_client(&config));
    let sync_index = Arc::new(ShardedSyncIndex::new());
    spawn_sync_index_flusher(sync_index.clone());

    let qm = Arc::new(build_queue_manager(
        &config,
        api_client.clone(),
        shared_state.clone(),
        sync_index.clone(),
    ));
    apply_runtime_preferences(&config);

    let (tx, rx) = mpsc::channel(32);
    let monitor_handle = Arc::new(start_monitor(&config, tx));
    let (manual_sync_tx, manual_sync_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    spawn_cache_prune(config.data.cache_disk_cap_mb);

    let tray_library_enabled = config.data.library_view_enabled;
    let date_headings = config.data.library_date_headings;
    let grid_style = app_context::GridStyle::from_config(&config.data);
    let ctx = Arc::new(AppContext {
        live_watch_paths: Arc::new(Mutex::new(config.data.watch_paths.clone())),
        config: Arc::new(parking_lot::RwLock::new(config)),
        state: shared_state.clone(),
        api_client: api_client.clone(),
        queue_manager: qm,
        monitor_handle,
        sync_index,
        sync_now_tx: manual_sync_tx,
        thumbnail_cache: Arc::new(ThumbnailCache::new(api_client)),
        library_state: Arc::new(parking_lot::Mutex::new(LibraryState::new())),
        date_headings: tokio::sync::watch::Sender::new(date_headings),
        grid_style: tokio::sync::watch::Sender::new(grid_style),
        current_user_id: Arc::new(parking_lot::Mutex::new(None)),
        expected_self_deletions: Arc::new(app_context::RecentSelfPaths::default()),
        expected_self_downloads: Arc::new(app_context::RecentSelfPaths::default()),
        reconcile_locks: Arc::new(app_context::ReconcileLocks::default()),
        pending_deletions: Arc::new(app_context::PendingDeletions::default()),
    });
    let _ = APP_CONTEXT.set(ctx.clone());

    spawn_background_tasks(ctx, rx, manual_sync_rx, background_sync_enabled);

    let flags = UiFlags::default();
    listen_for_ui_flags(app.clone(), flags.clone());
    tokio::spawn(run_tray(tray_library_enabled, flags));
}

fn log_config(config: &Config) {
    log::info!(
        "Config: internal={} external={} paths={:?}",
        config.data.internal_url,
        config.data.external_url,
        config.watch_path_strings(),
    );
}

/// Desktop styling, global CSS, and the hold that keeps the app running without windows.
fn prepare_gtk(app: &adw::Application) {
    // Always follow the desktop's light/dark preference.
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::Default);

    // Register global CSS for button animations and UI components
    crate::library::style::ensure_registered();

    thread_local! {
        static APP_HOLD: std::cell::RefCell<Option<gtk::gio::ApplicationHoldGuard>> = const { std::cell::RefCell::new(None) };
    }
    // Keep the application alive with no windows open so background sync keeps running.
    APP_HOLD.with(|hold| {
        *hold.borrow_mut() = Some(app.hold());
    });
}

/// Upload queue using the configured concurrency and pause policy (metered, battery, quiet hours).
fn build_queue_manager(
    config: &Config,
    api_client: Arc<ImmichApiClient>,
    shared_state: Arc<Mutex<AppState>>,
    sync_index: Arc<ShardedSyncIndex>,
) -> QueueManager {
    QueueManager::new(
        api_client,
        config.data.upload_concurrency.max(1) as usize,
        shared_state,
        sync_index,
        EnvironmentPolicy {
            pause_on_metered_network: config.data.pause_on_metered_network,
            pause_on_battery_power: config.data.pause_on_battery_power,
            quiet_hours_start: config.data.quiet_hours_start,
            quiet_hours_end: config.data.quiet_hours_end,
        },
    )
}

fn apply_runtime_preferences(config: &Config) {
    // Apply the user's notification preference before any notification can fire.
    crate::notifications::set_enabled(config.data.notifications_enabled);

    // Sync the RAW decode cache flag with the user's persisted preference.
    crate::library::set_raw_cache_enabled(config.data.raw_decode_cache_enabled);
    crate::library::set_raw_full_decode(config.data.raw_full_decode);
}

/// Start the file-event loop, catch-up scan, album reconciler, status check, and manual sync listener.
fn spawn_background_tasks(
    ctx: Arc<AppContext>,
    rx: mpsc::Receiver<MonitorEvent>,
    manual_sync_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    background_sync_enabled: bool,
) {
    tokio::spawn(run_file_event_loop(rx, ctx.clone()));

    if background_sync_enabled {
        // The startup scan backfills anything that arrived while Mimick was not running.
        tokio::spawn(run_catchup_scan(ctx.clone()));
        tokio::spawn(remote_sync::run_album_reconciler(ctx.clone()));
    } else {
        log::info!("Background sync is disabled; skipping startup catch-up scan");
    }

    tokio::spawn(refresh_connection_status(ctx.clone()));
    tokio::spawn(run_manual_sync_listener(manual_sync_rx, ctx));
}

/// API client for the enabled server URLs; warns when a URL is set without an API key.
fn build_api_client(config: &Config) -> ImmichApiClient {
    let api_key = config.get_api_key().unwrap_or_default();
    let runtime_internal_url = if config.data.internal_url_enabled {
        config.data.internal_url.clone()
    } else {
        String::new()
    };
    let runtime_external_url = if config.data.external_url_enabled {
        config.data.external_url.clone()
    } else {
        String::new()
    };
    if api_key.is_empty() && (!runtime_internal_url.is_empty() || !runtime_external_url.is_empty())
    {
        log::warn!(
            "Server URL is configured but no API key was found. \
             Library data will not load until an API key is set in Settings."
        );
    }
    ImmichApiClient::new(runtime_internal_url, runtime_external_url, api_key)
}

/// Flush the sync index every 10s so a crash loses at most that much progress.
fn spawn_sync_index_flusher(sync_index: Arc<ShardedSyncIndex>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            let index = sync_index.clone();
            let _ = tokio::task::spawn_blocking(move || index.flush()).await;
        }
    });
}

/// Keep the watcher service alive, but optionally disable active folder watches.
fn start_monitor(config: &Config, tx: mpsc::Sender<MonitorEvent>) -> monitor::MonitorHandle {
    let background_sync_enabled = config.data.background_sync_enabled;
    let monitor_paths = if background_sync_enabled {
        config.data.watch_paths.clone()
    } else {
        Vec::new()
    };
    let handle = Monitor::new(monitor_paths, background_sync_enabled).start(tx);
    if background_sync_enabled {
        log::info!("File monitor started");
    } else {
        log::info!("Background sync is disabled; monitor started with no active watches");
    }
    handle
}

/// One-shot startup prune across every cache directory, delayed so it does not
/// contend with window setup or initial sync work.
fn spawn_cache_prune(cache_cap_mb: u32) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let cap_bytes = (cache_cap_mb as u64).saturating_mul(1024 * 1024);
        let _ = tokio::task::spawn_blocking(move || {
            cache_manager::prune_all_blocking(cap_bytes);
        })
        .await;
    });
}
