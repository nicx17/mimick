use std::sync::Arc;

use glib::clone;
use gtk::FileDialog;
use gtk::prelude::*;
use libadwaita as adw;

use crate::app_context::AppContext;
use crate::diagnostics;
use crate::settings_window::show_alert;

pub(super) fn choose_diagnostics_folder(window: &adw::ApplicationWindow, ctx: Arc<AppContext>) {
    let dialog = FileDialog::builder()
        .title("Choose Diagnostics Export Folder")
        .build();
    dialog.select_folder(
        Some(window),
        gtk::gio::Cancellable::NONE,
        clone!(
            #[weak]
            window,
            move |res| {
                if let Some(path) = res.ok().and_then(|folder| folder.path()) {
                    export_diagnostics(&window, &ctx, path);
                }
            }
        ),
    );
}

/// Write the diagnostics bundle off the GTK thread and report where it went.
fn export_diagnostics(window: &adw::ApplicationWindow, ctx: &AppContext, path: std::path::PathBuf) {
    let state_snapshot = ctx.state.lock().clone();
    let config_snapshot = ctx.config.read().clone();
    glib::MainContext::default().spawn_local(clone!(
        #[weak]
        window,
        async move {
            let export_result = tokio::task::spawn_blocking(move || {
                diagnostics::export_bundle(&path, &state_snapshot, &config_snapshot)
            })
            .await;
            let (heading, body) = match export_result {
                Ok(Ok(bundle_dir)) => (
                    "Diagnostics Exported",
                    format!(
                        "Saved diagnostics bundle to {}",
                        crate::watch_path_display::display_watch_path_inline(
                            &bundle_dir.parent().unwrap_or(&bundle_dir).to_string_lossy()
                        )
                    ),
                ),
                Ok(Err(err)) => (
                    "Diagnostics Export Failed",
                    format!("Could not write diagnostics bundle: {}", err),
                ),
                Err(err) => (
                    "Diagnostics Export Failed",
                    format!("Diagnostics task could not complete: {}", err),
                ),
            };
            show_alert(&window, heading, &body);
        }
    ));
}
