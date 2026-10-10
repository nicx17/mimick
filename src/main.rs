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
use tokio::sync::mpsc;

mod api_client;
mod app_context;
mod autostart;
mod cache_manager;
mod config;
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
use config::{Config, best_matching_watch_entry};
use library::state::LibraryState;
use library::thumbnail_cache::ThumbnailCache;
use monitor::{Monitor, MonitorEvent};
use queue_manager::{EnvironmentPolicy, FileTask, QueueManager};
use settings_window::build_settings_window;
use startup_scan::queue_unsynced_files;
use state_manager::{AppState, StateManager};
use sync_index::{ShardedSyncIndex, SyncDecision, SyncTarget};
use tray_icon::build_tray;

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

/// Cross-thread UI requests: the tray task sets them and a GTK timer consumes them.
/// `Arc<Mutex<bool>>` is Send + Sync, so the flags can cross into the tray's Tokio
/// task while the application handle itself stays on the GTK thread.
#[derive(Clone, Default)]
struct UiFlags {
    settings: Arc<Mutex<bool>>,
    library: Arc<Mutex<bool>>,
    quit: Arc<Mutex<bool>>,
    pause: Arc<Mutex<bool>>,
    sync_now: Arc<Mutex<bool>>,
}

#[tokio::main]
async fn main() {
    let _logger = init_logging();

    gtk::gio::resources_register_include!("mimick.gresource")
        .expect("Failed to register bundled GResource");

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

    let config = Config::new();
    log::info!(
        "Config: internal={} external={} paths={:?}",
        config.data.internal_url,
        config.data.external_url,
        config.watch_path_strings(),
    );
    shared_state.lock().watched_folder_count = config.data.watch_paths.len();

    let background_sync_enabled = config.data.background_sync_enabled;
    let api_client = Arc::new(build_api_client(&config));
    let sync_index = Arc::new(ShardedSyncIndex::new());
    spawn_sync_index_flusher(sync_index.clone());

    let qm = Arc::new(QueueManager::new(
        api_client.clone(),
        config.data.upload_concurrency.max(1) as usize,
        shared_state.clone(),
        sync_index.clone(),
        EnvironmentPolicy {
            pause_on_metered_network: config.data.pause_on_metered_network,
            pause_on_battery_power: config.data.pause_on_battery_power,
            quiet_hours_start: config.data.quiet_hours_start,
            quiet_hours_end: config.data.quiet_hours_end,
        },
    ));

    // Apply the user's notification preference before any notification can fire.
    crate::notifications::set_enabled(config.data.notifications_enabled);

    // Sync the RAW decode cache flag with the user's persisted preference.
    crate::library::set_raw_cache_enabled(config.data.raw_decode_cache_enabled);
    crate::library::set_raw_full_decode(config.data.raw_full_decode);

    let (tx, rx) = mpsc::channel(32);
    let monitor_handle = Arc::new(start_monitor(&config, tx));
    let (manual_sync_tx, manual_sync_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    spawn_cache_prune(config.data.cache_disk_cap_mb);

    let tray_library_enabled = config.data.library_view_enabled;
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
        library_timeline_active: std::sync::atomic::AtomicBool::new(false),
        current_user_id: Arc::new(parking_lot::Mutex::new(None)),
        expected_self_deletions: Arc::new(app_context::RecentSelfPaths::default()),
        expected_self_downloads: Arc::new(app_context::RecentSelfPaths::default()),
        reconcile_locks: Arc::new(app_context::ReconcileLocks::default()),
        pending_deletions: Arc::new(app_context::PendingDeletions::default()),
    });
    let _ = APP_CONTEXT.set(ctx.clone());

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

    let flags = UiFlags::default();
    poll_ui_flags(app.clone(), flags.clone());
    tokio::spawn(run_tray(tray_library_enabled, flags));
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
            let _ = sync_index.flush();
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

/// Queue new files and mirror local deletions reported by the folder monitor.
async fn run_file_event_loop(mut rx: mpsc::Receiver<MonitorEvent>, ctx: Arc<AppContext>) {
    while let Some(event) = rx.recv().await {
        match event {
            MonitorEvent::Ready {
                path,
                checksum,
                sidecar_path,
            } => queue_ready_file(&ctx, path, checksum, sidecar_path).await,
            MonitorEvent::Deleted { path } => handle_local_deletion(&ctx, path).await,
        }
    }
}

/// Queue a stable file for upload unless Mimick wrote it itself or it is already synced.
async fn queue_ready_file(
    ctx: &Arc<AppContext>,
    path: String,
    checksum: String,
    sidecar_path: Option<String>,
) {
    if ctx.expected_self_downloads.consume(&path) {
        return;
    }

    let (album_id, album_name, watch_path, folder_rules, skip_album) =
        watch_target_for(&path, &ctx.live_watch_paths.lock());

    // Resolve XMP: per-folder override -> global default.
    let global_xmp = ctx.config.read().data.upload_xmp_sidecars;
    let sidecar_path = if folder_rules.xmp_sidecar_enabled(global_xmp) {
        sidecar_path
    } else {
        None
    };

    let target = SyncTarget {
        album_name: album_name.clone(),
        album_id: album_id.clone(),
    };
    let Some((reassociate_only, task_checksum)) = upload_plan(ctx, &path, &target, checksum) else {
        return;
    };

    log::info!("Queuing: {} (sha1={})", path, task_checksum);

    let _ = ctx
        .queue_manager
        .add_to_queue(FileTask {
            path,
            watch_path,
            checksum: task_checksum,
            album_id,
            album_name,
            reassociate_only,
            skip_album,
            sidecar_path,
        })
        .await;
}

/// Album target, watch root, rules, and library-only flag of the folder containing `path`.
fn watch_target_for(
    path: &str,
    entries: &[config::WatchPathEntry],
) -> (
    Option<String>,
    Option<String>,
    String,
    config::FolderRules,
    bool,
) {
    let Some(entry) = best_matching_watch_entry(std::path::Path::new(path), entries) else {
        return (None, None, String::new(), Default::default(), false);
    };
    match entry {
        config::WatchPathEntry::WithConfig {
            album_id,
            album_name,
            rules,
            ..
        } => (
            album_id.clone(),
            album_name.clone(),
            entry.path().to_string(),
            rules.clone(),
            entry.uploads_to_library(),
        ),
        config::WatchPathEntry::Simple(_) => (
            None,
            None,
            entry.path().to_string(),
            Default::default(),
            false,
        ),
    }
}

/// `(reassociate_only, checksum)` for the queue, or `None` if the file is up to date.
fn upload_plan(
    ctx: &AppContext,
    path: &str,
    target: &SyncTarget,
    checksum: String,
) -> Option<(bool, String)> {
    match ctx
        .sync_index
        .sync_decision(std::path::Path::new(path), target)
    {
        Ok(SyncDecision::UpToDate) => {
            log::debug!("Skipping unchanged file: {}", path);
            None
        }
        // Already uploaded but linked to a different album: reuse the stored
        // checksum and only re-link instead of uploading again.
        Ok(SyncDecision::NeedsReassociate) => Some((
            true,
            ctx.sync_index.stored_checksum(path).unwrap_or(checksum),
        )),
        Ok(SyncDecision::NeedsUpload) => Some((false, checksum)),
        Err(err) => {
            log::warn!(
                "Could not inspect sync index for '{}': {}; queuing anyway",
                path,
                err
            );
            Some((false, checksum))
        }
    }
}

/// Trash the remote copy of a locally deleted file, unless Mimick deleted it itself.
async fn handle_local_deletion(ctx: &Arc<AppContext>, path: String) {
    if ctx.expected_self_deletions.consume(&path) {
        return;
    }
    if let Some(request) = remote_sync::build_local_deletion_request(ctx.clone(), path).await {
        remote_sync::trash_remote_after_local_delete(ctx.clone(), request).await;
    }
}

/// Queue anything in the watch folders that isn't synced yet.
async fn run_catchup_scan(ctx: Arc<AppContext>) {
    let (watch_paths, catchup_mode) = {
        let cfg = ctx.config.read();
        (
            cfg.data.watch_paths.clone(),
            cfg.data.startup_catchup_mode.clone(),
        )
    };
    queue_unsynced_files(
        watch_paths,
        ctx.queue_manager.clone(),
        ctx.sync_index.clone(),
        ctx.api_client.clone(),
        catchup_mode,
        ctx.state.clone(),
        ctx.clone(),
    )
    .await;
}

/// Run a catch-up scan for each "Sync Now" request.
async fn run_manual_sync_listener(
    mut manual_sync_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    ctx: Arc<AppContext>,
) {
    while manual_sync_rx.recv().await.is_some() {
        run_catchup_scan(ctx.clone()).await;
    }
}

/// Record the initial server route and any connection error for the status page.
async fn refresh_connection_status(ctx: Arc<AppContext>) {
    let connected = ctx.api_client.check_connection().await;
    let route = ctx.api_client.active_route_label().await;
    let latest_issue = ctx.api_client.latest_issue().await;

    let mut state = ctx.state.lock();
    state.active_server_route = route;
    if connected {
        state.last_error = None;
        state.last_error_guidance = None;
    } else if let Some(issue) = latest_issue {
        state.last_error = Some(issue.summary);
        state.last_error_guidance = Some(issue.guidance);
    }
}

/// GTK-side: poll the flags every 250ms on the main thread.
/// The application handle stays on the GTK thread and never enters Tokio tasks.
fn poll_ui_flags(app: adw::Application, flags: UiFlags) {
    glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        handle_ui_flags(&app, &flags)
    });
}

/// Act on pending tray requests; returns `Break` after quitting to stop the timer.
fn handle_ui_flags(app: &adw::Application, flags: &UiFlags) -> glib::ControlFlow {
    if consume_flag(&flags.settings) {
        let ctx = APP_CONTEXT
            .get()
            .cloned()
            .expect("App context should be initialized before opening settings");
        open_settings_window_now(app, ctx);
    }

    if consume_flag(&flags.library) {
        let ctx = APP_CONTEXT
            .get()
            .cloned()
            .expect("App context should be initialized before opening library");
        open_library_window_now(app, ctx);
    }

    if consume_flag(&flags.quit) {
        app.quit();
        return glib::ControlFlow::Break;
    }

    if consume_flag(&flags.pause) {
        let qm = &APP_CONTEXT
            .get()
            .expect("App context should be initialized before pause handling")
            .queue_manager;
        let paused = !qm.is_paused();
        qm.set_paused(paused, paused.then(|| "Paused by user".to_string()));
    }

    if consume_flag(&flags.sync_now) {
        let tx = &APP_CONTEXT
            .get()
            .expect("App context should be initialized before manual sync handling")
            .sync_now_tx;
        let _ = tx.send(());
    }

    glib::ControlFlow::Continue
}

/// Tokio-side: build the tray and forward its signals into the UI flags.
async fn run_tray(library_enabled: bool, flags: UiFlags) {
    log::info!("Starting system tray");
    let handles = match build_tray(library_enabled).await {
        Ok(handles) => handles,
        Err(e) => {
            log::warn!("System tray failed to start: {:?}", e);
            return;
        }
    };
    let crate::tray_icon::TrayHandles {
        handle: _handle,
        mut settings_rx,
        mut library_rx,
        mut quit_rx,
        mut pause_rx,
        mut sync_now_rx,
    } = handles;
    loop {
        let forwarded = tokio::select! {
            res = settings_rx.changed() => forward_tray_signal(res, &settings_rx, &flags.settings),
            res = library_rx.changed() => forward_tray_signal(res, &library_rx, &flags.library),
            res = quit_rx.changed() => forward_tray_signal(res, &quit_rx, &flags.quit),
            res = pause_rx.changed() => forward_tray_signal(res, &pause_rx, &flags.pause),
            res = sync_now_rx.changed() => forward_tray_signal(res, &sync_now_rx, &flags.sync_now),
        };
        if !forwarded {
            break;
        }
    }
}

/// Raise `flag` when the tray signal fired; `false` once the tray channel is closed.
fn forward_tray_signal(
    changed: Result<(), tokio::sync::watch::error::RecvError>,
    rx: &tokio::sync::watch::Receiver<bool>,
    flag: &Mutex<bool>,
) -> bool {
    if changed.is_err() {
        return false;
    }
    if *rx.borrow() {
        *flag.lock() = true;
    }
    true
}

/// Handle command line from both the primary and secondary instances.
fn handle_command_line(
    app: &adw::Application,
    cmdline: &gtk::gio::ApplicationCommandLine,
) -> glib::ExitCode {
    let argv: Vec<String> = cmdline
        .arguments()
        .iter()
        .filter_map(|a| a.to_str().map(ToString::to_string))
        .collect();

    let quit_requested = argv.contains(&"--quit".to_string());
    if quit_requested {
        app.quit();
        return 0.into();
    }

    let ctx_early = APP_CONTEXT.get().cloned();
    let want_settings = argv.contains(&"--settings".to_string());
    let want_library = argv.contains(&"--library".to_string());
    let want_upload = argv.contains(&"--upload".to_string());
    let setup_required = ctx_early
        .as_ref()
        .map(|c| c.config.read().get_api_key().unwrap_or_default().is_empty())
        .unwrap_or(true);
    let secondary_activation = cmdline.is_remote();

    let ctx_lookup = || {
        APP_CONTEXT
            .get()
            .cloned()
            .expect("App context should be initialized before command-line activation")
    };

    // Collect file path arguments (positional args that are not flags).
    let file_args: Vec<std::path::PathBuf> = argv
        .iter()
        .skip(1) // skip binary name
        .filter(|a| !a.starts_with("--"))
        .map(std::path::PathBuf::from)
        .filter(|p| crate::media_kinds::is_supported_path(p))
        .collect();

    if !file_args.is_empty() && !setup_required {
        // Files were passed -- open the staging view.
        crate::library::staging_view::build_staging_window(
            app,
            ctx_lookup(),
            file_args,
            want_upload,
        );
    } else if want_settings || setup_required {
        open_settings_window_now(app, ctx_lookup());
    } else if want_library {
        open_library_window_now(app, ctx_lookup());
    } else if secondary_activation
        || !ctx_early
            .as_ref()
            .map(|c| c.config.read().data.background_sync_enabled)
            .unwrap_or(false)
    {
        open_default_window(app, ctx_lookup());
    }

    app.activate();
    0.into()
}

/// Quit gracefully on SIGINT/SIGTERM, polling the signal flag from the GTK loop.
fn install_termination_handler(app: &adw::Application) {
    let quit_requested = Arc::new(AtomicBool::new(false));
    let qr_signal = quit_requested.clone();
    tokio::spawn(async move {
        if wait_for_termination_signal().await {
            qr_signal.store(true, Ordering::SeqCst);
        }
    });

    let app_for_quit = app.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        if quit_requested.load(Ordering::SeqCst) {
            app_for_quit.quit();
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

/// Wait for SIGINT or SIGTERM; `false` if a handler couldn't be installed.
async fn wait_for_termination_signal() -> bool {
    let mut sigterm = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        Ok(s) => s,
        Err(err) => {
            log::warn!("Could not install SIGTERM handler: {}", err);
            return false;
        }
    };
    tokio::select! {
        res = tokio::signal::ctrl_c() => {
            if let Err(err) = res {
                log::warn!("SIGINT handler error: {}", err);
                return false;
            }
            log::info!("Received SIGINT; requesting graceful shutdown.");
        }
        _ = sigterm.recv() => {
            log::info!("Received SIGTERM; requesting graceful shutdown.");
        }
    }
    true
}

/// Persist final state and any pending retries on graceful shutdown.
async fn persist_on_shutdown(shared_state: &Arc<Mutex<AppState>>) {
    if let Some(ctx) = APP_CONTEXT.get() {
        ctx.queue_manager
            .shutdown(std::time::Duration::from_secs(5))
            .await;
        ctx.queue_manager.flush_retries();
        if let Err(err) = ctx.sync_index.flush() {
            log::warn!("Failed to flush sync index on shutdown: {}", err);
        }
    }
    let state = APP_CONTEXT
        .get()
        .map(|ctx| ctx.state.lock().clone())
        .unwrap_or_else(|| shared_state.lock().clone());
    StateManager::new().write_state(state);
    log::info!("Mimick exiting");
}

/// Open whichever window the user prefers, presenting an existing instance if available.
fn open_default_window(app: &adw::Application, ctx: Arc<AppContext>) {
    if let Some(win) = find_window(app, "mimick-library-window")
        .or_else(|| find_window(app, "mimick-settings-window"))
    {
        win.present();
        return;
    }
    if ctx.config.read().data.library_view_enabled {
        open_library_window_now(app, ctx);
    } else {
        open_settings_window_now(app, ctx);
    }
}

/// Open Settings window or present existing settings instance.
fn open_settings_window_now(app: &adw::Application, ctx: Arc<AppContext>) {
    if let Some(win) = find_window(app, "mimick-settings-window") {
        win.present();
        return;
    }
    log::debug!("Opening settings window");
    build_settings_window(app, ctx);
}

/// Open Library window, falling back to Settings if disabled.
fn open_library_window_now(app: &adw::Application, ctx: Arc<AppContext>) {
    if let Some(win) = find_window(app, "mimick-library-window") {
        win.present();
        return;
    }
    if !ctx.config.read().data.library_view_enabled {
        log::info!("Library view is disabled in settings; opening Settings instead");
        open_settings_window_now(app, ctx);
        return;
    }
    log::debug!("Opening library window");
    library::build_library_window(app, ctx);
}

/// Helper to look up active GTK window instances by widget name.
fn find_window(app: &adw::Application, name: &str) -> Option<gtk::Window> {
    app.windows().into_iter().find(|w| w.widget_name() == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FolderRules, WatchPathEntry};

    #[test]
    fn test_live_queue_matching_prefers_most_specific_watch_path() {
        let entries = vec![
            WatchPathEntry::WithConfig {
                path: "/home/user/Pictures".into(),
                album_id: Some("root-album".into()),
                album_name: Some("Pictures".into()),
                rules: FolderRules::default(),
            },
            WatchPathEntry::WithConfig {
                path: "/home/user/Pictures/Trips".into(),
                album_id: Some("trips-album".into()),
                album_name: Some("Trips".into()),
                rules: FolderRules::default(),
            },
        ];

        let matched = best_matching_watch_entry(
            std::path::Path::new("/home/user/Pictures/Trips/day1/photo.jpg"),
            &entries,
        )
        .unwrap();

        let config::WatchPathEntry::WithConfig { album_id, .. } = matched else {
            panic!("expected configured watch entry");
        };
        assert_eq!(album_id.as_deref(), Some("trips-album"));
        assert_eq!(matched.album_name(), Some("Trips"));
    }
}
