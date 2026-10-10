//! Integrates StatusNotifier tray functionality and provides GTK-facing control signals.
//!
//! Uses the `ksni` crate to register a system tray icon with menu entries
//! for Settings, Library, Pause/Resume, Sync Now, and Quit. Each action
//! sends a signal over a `tokio::sync::watch` channel that the GTK main
//! loop polls to trigger the corresponding window or operation.

use ksni::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};
use tokio::sync::watch;

/// Represents the tray state shared with ksni menu callbacks and GTK main loop.
#[derive(Debug)]
pub struct MimickTray {
    /// Sender used to signal the GTK main loop to open the settings window.
    /// Sending `true` triggers the open; the receiver is polled via glib::timeout_add.
    pub settings_tx: watch::Sender<bool>,
    /// Sender used to signal the GTK main loop to open the library window.
    pub library_tx: watch::Sender<bool>,
    /// Sender used to request a graceful application quit from the GTK main loop.
    pub quit_tx: watch::Sender<bool>,
    /// Sender used to toggle the paused state from the tray.
    pub pause_tx: watch::Sender<bool>,
    /// Sender used to request an immediate catch-up scan.
    pub sync_now_tx: watch::Sender<bool>,
    /// Cached from `Config` at tray construction to avoid disk I/O per menu open.
    pub library_view_enabled: bool,
}

impl ksni::Tray for MimickTray {
    fn id(&self) -> String {
        "mimick_tray".to_string()
    }

    fn icon_name(&self) -> String {
        "dev.nicx.mimick".to_string()
    }

    fn title(&self) -> String {
        "Mimick Sync".into()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let library_enabled = self.library_view_enabled;
        let mut items: Vec<ksni::MenuItem<Self>> = Vec::new();
        if library_enabled {
            items.push(
                StandardItem {
                    label: "Library".into(),
                    activate: Box::new(|tray: &mut Self| {
                        let _ = tray.library_tx.send(true);
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }
        items.extend(vec![
            StandardItem {
                label: "Settings".into(),
                activate: Box::new(|tray: &mut Self| {
                    // Signal the GTK main loop — no new process spawned.
                    let _ = tray.settings_tx.send(true);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Pause / Resume".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.pause_tx.send(true);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Sync Now".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sync_now_tx.send(true);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.quit_tx.send(true);
                }),
                ..Default::default()
            }
            .into(),
        ]);
        items
    }
}

/// Channels and handles returned by tray initialization to manage signals.
pub struct TrayHandles {
    /// Active ksni tray handle.
    pub handle: ksni::Handle<MimickTray>,
    /// Receiver for the settings-open signal.
    pub settings_rx: watch::Receiver<bool>,
    /// Receiver for the library-open signal.
    pub library_rx: watch::Receiver<bool>,
    /// Receiver for the application quit signal.
    pub quit_rx: watch::Receiver<bool>,
    /// Receiver for the pause/resume signal.
    pub pause_rx: watch::Receiver<bool>,
    /// Receiver for the catch-up sync request signal.
    pub sync_now_rx: watch::Receiver<bool>,
}

/// Asynchronously construct and spawn the system tray icon, returning control channels.
pub async fn build_tray(library_view_enabled: bool) -> Result<TrayHandles, ksni::Error> {
    let (settings_tx, settings_rx) = watch::channel(false);
    let (library_tx, library_rx) = watch::channel(false);
    let (quit_tx, quit_rx) = watch::channel(false);
    let (pause_tx, pause_rx) = watch::channel(false);
    let (sync_now_tx, sync_now_rx) = watch::channel(false);
    let tray = MimickTray {
        settings_tx,
        library_tx,
        quit_tx,
        pause_tx,
        sync_now_tx,
        library_view_enabled,
    };
    let handle = if ashpd::is_sandboxed() {
        // Flatpak sessions already broker the item through the watcher, so we avoid owning
        // an additional D-Bus name and keep the permission request narrower.
        tray.disable_dbus_name(true).spawn().await?
    } else {
        tray.spawn().await?
    };
    Ok(TrayHandles {
        handle,
        settings_rx,
        library_rx,
        quit_rx,
        pause_rx,
        sync_now_rx,
    })
}
