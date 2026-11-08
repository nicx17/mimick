use crate::i18n::fill;
use gettextrs::gettext;
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
        .title(gettext("Choose Diagnostics Export Folder"))
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
    // Read here, on the GTK thread, like the rest of Settings: the log is scrubbed of it.
    let api_key = config_snapshot.get_api_key().unwrap_or_default();
    glib::MainContext::default().spawn_local(clone!(
        #[weak]
        window,
        async move {
            let export_result = tokio::task::spawn_blocking(move || {
                diagnostics::export_bundle(&path, &state_snapshot, &config_snapshot, &api_key)
            })
            .await;
            let (heading, body) = match export_result {
                Ok(Ok(bundle_dir)) => (
                    gettext("Diagnostics Exported"),
                    fill(
                        gettext("Saved diagnostics bundle to {path}"),
                        &[(
                            "path",
                            &crate::watch_path_display::display_watch_path_inline(
                                &bundle_dir.parent().unwrap_or(&bundle_dir).to_string_lossy(),
                            ),
                        )],
                    ),
                ),
                Ok(Err(err)) => (
                    gettext("Diagnostics Export Failed"),
                    fill(
                        gettext("Could not write diagnostics bundle: {err}"),
                        &[("err", &err.to_string())],
                    ),
                ),
                Err(err) => (
                    gettext("Diagnostics Export Failed"),
                    fill(
                        gettext("Diagnostics task could not complete: {err}"),
                        &[("err", &err.to_string())],
                    ),
                ),
            };
            show_alert(&window, &heading, &body);
        }
    ));
}
