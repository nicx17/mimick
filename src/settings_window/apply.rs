//! Saving settings: validate, write config, then push the changes into the running app.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::app_context::AppContext;
use crate::autostart;
use crate::config::{ApiKeyStorage, WatchPathEntry};

use super::form::{
    ConnectionValues, SettingsForm, SettingsValues, apply_to_config, validate_connection,
};
use super::{FolderRowData, api_key_file_notice, show_alert, watch_path_entry};

/// Non-widget state shared by the save path and the watch-folder rows.
#[derive(Clone)]
pub(super) struct ApplyState {
    pub ctx: Arc<AppContext>,
    pub tracked_rows: Rc<RefCell<Vec<FolderRowData>>>,
    /// `(name, id)` pairs for the album picker; refreshed after a connection change.
    pub albums: Rc<RefCell<Vec<(String, String)>>>,
    /// Last applied startup / background-sync values, to detect changes.
    startup: Rc<Cell<bool>>,
    background_sync: Rc<Cell<bool>>,
    /// Auto-apply fires on every control change; drop saves while one is running.
    in_flight: Rc<Cell<bool>>,
}

impl ApplyState {
    pub(super) fn new(ctx: &Arc<AppContext>, run_on_startup: bool, background_sync: bool) -> Self {
        Self {
            ctx: ctx.clone(),
            tracked_rows: Rc::default(),
            albums: Rc::default(),
            startup: Rc::new(Cell::new(run_on_startup)),
            background_sync: Rc::new(Cell::new(background_sync)),
            in_flight: Rc::default(),
        }
    }

    fn watch_paths(&self) -> Vec<WatchPathEntry> {
        let albums: HashMap<String, String> = self.albums.borrow().iter().cloned().collect();
        self.tracked_rows
            .borrow()
            .iter()
            .map(|row| {
                watch_path_entry(
                    row.path.clone(),
                    &row.album_name.borrow(),
                    row.uploads_to_library.get(),
                    row.rules.borrow().clone(),
                    &albums,
                )
            })
            .collect()
    }
}

/// Everything one save needs, captured before the async part starts.
struct ApplyRequest {
    conn: ConnectionValues,
    values: SettingsValues,
    watch_paths: Vec<WatchPathEntry>,
    include_connectivity: bool,
    show_success_ack: bool,
    previous_startup: bool,
    previous_background_sync: bool,
}

/// `apply(include_connectivity, show_success_ack)`. Connection fields are only saved from
/// the explicit Save button; auto-apply keeps the stored connection.
pub(super) fn make_apply_settings(
    form: &SettingsForm,
    state: ApplyState,
) -> Rc<dyn Fn(bool, bool)> {
    let weak = form.downgrade();
    Rc::new(move |include_connectivity, show_success_ack| {
        if let Some(form) = weak.upgrade() {
            apply_settings(&form, &state, include_connectivity, show_success_ack);
        }
    })
}

fn apply_settings(
    form: &SettingsForm,
    state: &ApplyState,
    include_connectivity: bool,
    show_success_ack: bool,
) {
    if state.in_flight.replace(true) {
        return;
    }
    let conn = if include_connectivity {
        form.connection_values()
    } else {
        ConnectionValues::from_config(&state.ctx.config.read())
    };
    if include_connectivity && let Err((heading, err)) = validate_connection(&conn) {
        show_alert(&form.window, heading, &err);
        state.in_flight.set(false);
        return;
    }
    let request = ApplyRequest {
        conn,
        values: form.values(),
        watch_paths: state.watch_paths(),
        include_connectivity,
        show_success_ack,
        previous_startup: state.startup.get(),
        previous_background_sync: state.background_sync.get(),
    };
    let weak = form.downgrade();
    let state = state.clone();
    glib::MainContext::default().spawn_local(async move {
        if let Some(form) = weak.upgrade() {
            save_and_apply(&form, &state, request).await;
        }
    });
}

async fn save_and_apply(form: &SettingsForm, state: &ApplyState, request: ApplyRequest) {
    if !update_autostart(form, &request).await {
        state.in_flight.set(false);
        return;
    }
    let Some(key_file_notice) = write_config(form, state, &request) else {
        state.in_flight.set(false);
        return;
    };
    state.startup.set(request.values.run_on_startup);
    state
        .background_sync
        .set(request.values.background_sync_enabled);
    update_api_client(state, &request).await;
    apply_sync_runtime(state, &request);

    state.in_flight.set(false);
    if let Some(notice) = key_file_notice {
        show_alert(&form.window, "API Key Stored in a File", &notice);
    } else if request.show_success_ack {
        show_alert(
            &form.window,
            "Settings Saved",
            "Mimick saved the updated settings successfully.",
        );
    }
}

/// Apply a changed "Run on Startup" choice; on refusal or error, revert the switch and say why.
async fn update_autostart(form: &SettingsForm, request: &ApplyRequest) -> bool {
    let wanted = request.values.run_on_startup;
    if wanted == request.previous_startup {
        return true;
    }
    let failure = match autostart::apply(&form.window, wanted).await {
        Ok(granted) if granted == wanted => return true,
        Ok(_) => (
            "Startup Permission Needed",
            "Mimick was not allowed to start automatically at login.".to_string(),
        ),
        Err(err) => ("Could Not Update Startup Setting", err),
    };
    form.startup_row.set_active(request.previous_startup);
    show_alert(&form.window, failure.0, &failure.1);
    false
}

/// Write config (and the API key, from the Save button). `None` after showing an error;
/// otherwise the file-fallback notice to show, if the key couldn't go in the keyring.
fn write_config(
    form: &SettingsForm,
    state: &ApplyState,
    request: &ApplyRequest,
) -> Option<Option<String>> {
    let values = &request.values;
    let mut config = state.ctx.config.write();
    apply_to_config(
        &mut config.data,
        &request.conn,
        values,
        request.watch_paths.clone(),
    );
    crate::library::set_raw_cache_enabled(values.raw_decode_cache_enabled);
    crate::library::set_raw_full_decode(values.raw_full_decode);

    let mut key_file_notice = None;
    if request.include_connectivity && !request.conn.api_key.is_empty() {
        match config.set_api_key(&request.conn.api_key) {
            Ok(ApiKeyStorage::File(path)) => key_file_notice = Some(api_key_file_notice(&path)),
            Ok(ApiKeyStorage::Keyring) => {}
            Err(detail) => {
                show_alert(&form.window, "Could Not Save API Key", &detail);
                return None;
            }
        }
    }
    if !config.save() {
        show_alert(
            &form.window,
            "Could Not Save Settings",
            "Mimick could not write the updated configuration to disk.",
        );
        return None;
    }
    Some(key_file_notice)
}

/// Point the API client at the new addresses; refresh the album list after a key change.
async fn update_api_client(state: &ApplyState, request: &ApplyRequest) {
    let (internal, external) = request.conn.runtime_urls();
    let api_client = &state.ctx.api_client;
    api_client
        .update_settings(internal, external, request.conn.api_key.clone())
        .await;
    if request.include_connectivity && !request.conn.api_key.is_empty() {
        match api_client.get_all_albums().await {
            Ok(fetched) => *state.albums.borrow_mut() = fetched,
            Err(err) => log::warn!("Could not fetch albums after saving settings: {}", err),
        }
    }
}

/// Queue limits and pause policy, notifications, folder watches, and status bookkeeping.
fn apply_sync_runtime(state: &ApplyState, request: &ApplyRequest) {
    let ctx = &state.ctx;
    let values = &request.values;
    ctx.queue_manager
        .set_worker_limit(values.upload_concurrency);
    ctx.queue_manager
        .update_environment_policy(crate::queue_manager::EnvironmentPolicy {
            pause_on_metered_network: values.pause_on_metered_network,
            pause_on_battery_power: values.pause_on_battery_power,
            quiet_hours_start: values.quiet_hours_start,
            quiet_hours_end: values.quiet_hours_end,
        });
    crate::notifications::set_enabled(values.notifications_enabled);

    let background_sync = values.background_sync_enabled;
    let background_sync_changed = request.previous_background_sync != background_sync;
    if background_sync_changed && !background_sync {
        let mut app_state = ctx.state.lock();
        if app_state.status != "uploading" && !app_state.paused {
            app_state.status = "idle".to_string();
            app_state.pause_reason = None;
        }
    }

    let monitor_paths = if background_sync {
        request.watch_paths.clone()
    } else {
        Vec::new()
    };
    ctx.monitor_handle
        .replace_watch_paths(monitor_paths, background_sync);
    *ctx.live_watch_paths.lock() = request.watch_paths.clone();
    // Turning background sync on catches up on anything missed while it was off.
    if background_sync && background_sync_changed {
        let _ = ctx.sync_now_tx.send(());
    }
    forget_removed_folders(state, &request.watch_paths);
}

/// Update the folder count and drop status entries for folders that were removed.
fn forget_removed_folders(state: &ApplyState, watch_paths: &[WatchPathEntry]) {
    let mut app_state = state.ctx.state.lock();
    app_state.watched_folder_count = watch_paths.len();
    let current_paths = watch_paths
        .iter()
        .map(|entry| entry.path().to_string())
        .collect::<std::collections::HashSet<_>>();
    app_state
        .folder_statuses
        .retain(|path, _| current_paths.contains(path));
}
