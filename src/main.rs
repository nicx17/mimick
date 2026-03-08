use gtk::prelude::*;
use libadwaita as adw;
use tokio::sync::mpsc;
use std::sync::Arc;

mod config;
mod api_client;
mod state_manager;
mod queue_manager;
mod monitor;
mod notifications;
mod settings_window;
mod tray_icon;

use config::Config;
use api_client::ImmichApiClient;
use queue_manager::{QueueManager, FileTask};
use monitor::Monitor;
use settings_window::build_settings_window;
use tray_icon::build_tray;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    ).init();

    log::info!("Mimick starting up");

    let app = adw::Application::builder()
        .application_id("com.github.nicx17.mimick")
        .flags(gtk::gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    let args: Vec<String> = std::env::args().collect();
    let is_settings = args.contains(&"--settings".to_string());

    app.connect_command_line(|app, _cmdline| {
        app.activate();
        0.into()
    });

    // Load config
    let config = Config::new();
    log::info!(
        "Config: internal={} external={} paths={:?}",
        config.data.internal_url,
        config.data.external_url,
        config.watch_path_strings(),
    );

    let api_key = config.get_api_key().unwrap_or_default();

    // Pass both URLs — failover is handled inside ImmichApiClient
    let api_client = Arc::new(ImmichApiClient::new(
        config.data.internal_url.clone(),
        config.data.external_url.clone(),
        api_key,
    ));

    let qm = Arc::new(QueueManager::new(api_client, 10));

    // Start file monitor using plain path strings
    let (tx, mut rx) = mpsc::channel(200);
    let watch_paths = config.watch_path_strings();
    let monitor = Monitor::new(watch_paths);
    monitor.start(tx);
    log::info!("File monitor started");

    // Feed monitor events into the upload queue, preserving per-path album config
    let qm_clone = qm.clone();
    // Build a map from path -> album config for the event handler
    let path_configs: Vec<_> = config.data.watch_paths.clone();
    tokio::spawn(async move {
        while let Some((path, checksum)) = rx.recv().await {
            log::info!("Queuing: {} (sha1={})", path, checksum);

            // Find album config for this path's parent
            let mut album_id = None;
            let mut album_name = None;
            for entry in &path_configs {
                use config::WatchPathEntry;
                if let WatchPathEntry::WithConfig { path: base, album_id: aid, album_name: aname } = entry {
                    if path.starts_with(base.as_str()) {
                        album_id = aid.clone();
                        album_name = aname.clone();
                        break;
                    }
                }
            }

            qm_clone.add_to_queue(FileTask { path, checksum, album_id, album_name }).await;
        }
    });

    app.connect_activate(move |app| {
        if is_settings {
            log::debug!("Opening settings window");
            build_settings_window(app);
        }
    });

    tokio::spawn(async move {
        log::info!("Starting system tray");
        if let Err(e) = build_tray().await {
            log::warn!("System tray failed to start: {:?}", e);
        }
    });

    log::info!("GTK main loop starting");
    app.run();
    log::info!("Mimick exiting");
}
