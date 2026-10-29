use gtk::prelude::*;
use libadwaita as adw;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app_context::AppContext;
use crate::library;
use crate::settings_window::build_settings_window;
use crate::state_manager::{AppState, StateManager};
use crate::tray_icon::build_tray;

use super::{APP_CONTEXT, UiFlags, consume_flag};

pub(super) fn listen_for_ui_flags(app: adw::Application, flags: UiFlags) {
    glib::MainContext::default().spawn_local(async move {
        loop {
            flags.wake.notified().await;
            if matches!(handle_ui_flags(&app, &flags), glib::ControlFlow::Break) {
                break;
            }
        }
    });
}

/// Act on pending tray requests; returns `Break` after quitting to stop listening.
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
pub(super) async fn run_tray(library_enabled: bool, flags: UiFlags) {
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
        flags.wake.notify_one();
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
pub(super) fn handle_command_line(
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
    let setup_required = api_key_missing(ctx_early.as_deref());
    let secondary_activation = cmdline.is_remote();
    let ctx_lookup = command_line_context;
    let file_args = media_file_arguments(&argv);

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

/// True until an API key is configured (or before the app context exists).
fn api_key_missing(ctx: Option<&AppContext>) -> bool {
    ctx.map(|c| c.config.read().get_api_key().unwrap_or_default().is_empty())
        .unwrap_or(true)
}

fn command_line_context() -> Arc<AppContext> {
    APP_CONTEXT
        .get()
        .cloned()
        .expect("App context should be initialized before command-line activation")
}

/// Collect file path arguments (positional args that are not flags) that Mimick can upload.
fn media_file_arguments(argv: &[String]) -> Vec<std::path::PathBuf> {
    argv.iter()
        .skip(1) // skip binary name
        .filter(|a| !a.starts_with("--"))
        .map(std::path::PathBuf::from)
        .filter(|p| crate::media_kinds::is_supported_path(p))
        .collect()
}

/// Quit gracefully on SIGINT/SIGTERM, polling the signal flag from the GTK loop.
pub(super) fn install_termination_handler(app: &adw::Application) {
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
pub(super) async fn persist_on_shutdown(shared_state: &Arc<Mutex<AppState>>) {
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
pub(super) fn open_default_window(app: &adw::Application, ctx: Arc<AppContext>) {
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
pub(super) fn open_settings_window_now(app: &adw::Application, ctx: Arc<AppContext>) {
    if let Some(win) = find_window(app, "mimick-settings-window") {
        win.present();
        return;
    }
    log::debug!("Opening settings window");
    build_settings_window(app, ctx);
}

/// Open Library window, falling back to Settings if disabled.
pub(super) fn open_library_window_now(app: &adw::Application, ctx: Arc<AppContext>) {
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
    use crate::config::{self, FolderRules, WatchPathEntry, best_matching_watch_entry};

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
