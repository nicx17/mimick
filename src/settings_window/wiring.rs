//! Settings window construction helpers and signal wiring.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use glib::clone;
use gtk::prelude::*;
use gtk::{Button, FileDialog, ListBox, ScrolledWindow};
use libadwaita as adw;

use crate::app_context::AppContext;
use crate::config::{Config, WatchPathEntry};
use crate::diagnostics;

use super::actions_ui::ActionsWidgets;
use super::apply::ApplyState;
use super::behavior::BehaviorWidgets;
use super::connectivity::ConnectivityWidgets;
use super::form::SettingsForm;
use super::library::LibraryWidgets;
use super::queue_inspector::show_about_dialog;
use super::watch_folders::add_folder_row;
use super::{show_alert, show_queue_inspector};

/// Window with Status / Settings pages behind a header view switcher.
pub(super) fn build_window_shell(
    app: &adw::Application,
    parent: Option<&adw::ApplicationWindow>,
) -> (
    adw::ApplicationWindow,
    adw::PreferencesPage,
    adw::PreferencesPage,
) {
    let mut window_builder = adw::ApplicationWindow::builder()
        .application(app)
        .title("Mimick")
        .name("mimick-settings-window")
        .default_width(520)
        .default_height(780);
    if let Some(parent) = parent {
        window_builder = window_builder
            .transient_for(parent)
            .modal(true)
            .destroy_with_parent(true);
    }
    let window = window_builder.build();
    window.set_size_request(360, 480);

    let view_stack = adw::ViewStack::builder()
        .vexpand(true)
        .hexpand(true)
        .build();
    let toolbar_view = adw::ToolbarView::builder().build();
    toolbar_view.add_top_bar(&build_header_bar(&window, &view_stack));
    toolbar_view.set_content(Some(&view_stack));
    window.set_content(Some(&toolbar_view));

    let status_page = add_page(
        &view_stack,
        "status",
        "Status",
        "dialog-information-symbolic",
    );
    let settings_page = add_page(
        &view_stack,
        "settings",
        "Settings",
        "emblem-system-symbolic",
    );
    (window, status_page, settings_page)
}

/// Header with the Status / Settings switcher and an About button.
fn build_header_bar(
    window: &adw::ApplicationWindow,
    view_stack: &adw::ViewStack,
) -> adw::HeaderBar {
    let header_bar = adw::HeaderBar::builder()
        .title_widget(&adw::ViewSwitcher::builder().stack(view_stack).build())
        .build();
    let about_header_btn = Button::builder()
        .icon_name("help-about-symbolic")
        .tooltip_text("About Mimick")
        .build();
    let window = window.clone();
    about_header_btn.connect_clicked(move |_| show_about_dialog(&window));
    header_bar.pack_start(&about_header_btn);
    header_bar
}

fn add_page(stack: &adw::ViewStack, name: &str, title: &str, icon: &str) -> adw::PreferencesPage {
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .build();
    stack.add_titled_with_icon(&scroll, Some(name), title, icon);
    let page = adw::PreferencesPage::builder()
        .title(title)
        .icon_name(icon)
        .build();
    scroll.set_child(Some(&page));
    page
}

/// First-run help shown until an API key is configured.
pub(super) fn add_welcome_group(settings_page: &adw::PreferencesPage) {
    let welcome_group = adw::PreferencesGroup::builder()
        .title("Welcome to Mimick!")
        .description("Start by adding your API key, testing the connection, and choosing at least one folder. The key needs Asset (read, view, upload, update, download, delete), Album (read, create, update), and albumAsset (create, delete) permissions.")
        .build();
    let help_row = adw::ActionRow::builder()
        .title("How to get an API Key")
        .subtitle("Base sync: user.read, asset.upload/update, album.read/create, albumAsset.create. See docs for Library/deletion scopes.")
        .activatable(true)
        .build();
    help_row.connect_activated(|_| {
        let uri = "https://immich.app/docs/features/command-line-interface/#api-key";
        if let Err(e) =
            gtk::gio::AppInfo::launch_default_for_uri(uri, None::<&gtk::gio::AppLaunchContext>)
        {
            log::error!("Failed to open browser: {}", e);
        }
    });
    welcome_group.add(&help_row);
    settings_page.add(&welcome_group);
}

/// Alert heading and body for a connection test result.
fn connection_report(internal_ok: bool, external_ok: bool) -> (&'static str, String) {
    if !internal_ok && !external_ok {
        return (
            "Connection Failed",
            "Could not connect to Immich at either address.".to_string(),
        );
    }
    let label = |ok: bool| if ok { "OK" } else { "FAILED" };
    let mode = if internal_ok { "LAN" } else { "WAN" };
    (
        "Connection Successful",
        format!(
            "Internal: {}\nExternal: {}\n\nActive Mode: {}",
            label(internal_ok),
            label(external_ok),
            mode
        ),
    )
}

/// Ping the enabled addresses off the GTK thread and report the result.
pub(super) fn connect_test_button(
    window: &adw::ApplicationWindow,
    conn: &ConnectivityWidgets,
    api_client: Arc<crate::api_client::ImmichApiClient>,
) {
    let window = window.downgrade();
    conn.test_btn.connect_clicked(clone!(
        #[weak(rename_to = internal_switch)]
        conn.internal_switch,
        #[weak(rename_to = external_switch)]
        conn.external_switch,
        #[weak(rename_to = internal_entry)]
        conn.internal_entry,
        #[weak(rename_to = external_entry)]
        conn.external_entry,
        move |btn| {
            btn.set_sensitive(false);
            // Collect only plain values: no GTK types cross into the Tokio task.
            let enabled_url = |switch: &gtk::Switch, entry: &gtk::Entry| {
                if switch.is_active() {
                    entry.text().to_string()
                } else {
                    String::new()
                }
            };
            let internal = enabled_url(&internal_switch, &internal_entry);
            let external = enabled_url(&external_switch, &external_entry);
            let rx = spawn_ping(api_client.clone(), internal, external);
            let (window, btn) = (window.clone(), btn.clone());
            glib::MainContext::default().spawn_local(async move {
                let Ok((int_ok, ext_ok)) = rx.await else {
                    return;
                };
                btn.set_sensitive(true);
                if let Some(window) = window.upgrade() {
                    let (heading, report) = connection_report(int_ok, ext_ok);
                    show_alert(&window, heading, &report);
                }
            });
        }
    ));
}

/// Ping both addresses on Tokio; resolves to `(internal_ok, external_ok)`.
/// Reuses the app-wide API client: a new one per click opens a connection pool
/// that lingers ~30s after the test completes.
fn spawn_ping(
    api_client: Arc<crate::api_client::ImmichApiClient>,
    internal: String,
    external: String,
) -> tokio::sync::oneshot::Receiver<(bool, bool)> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let ping = |url: String| {
            let client = api_client.clone();
            async move { !url.is_empty() && client.ping_url(&url).await }
        };
        let int_ok = ping(internal).await;
        let ext_ok = ping(external).await;
        let _ = tx.send((int_ok, ext_ok));
    });
    rx
}

/// Show the hour spinners only when quiet hours are enabled.
pub(super) fn connect_quiet_hours_sensitivity(behavior: &BehaviorWidgets) {
    behavior.quiet_hours_row.connect_active_notify(clone!(
        #[weak(rename_to = start)]
        behavior.quiet_start_row,
        #[weak(rename_to = end)]
        behavior.quiet_end_row,
        move |row| {
            start.set_sensitive(row.is_active());
            end.set_sensitive(row.is_active());
        }
    ));
}

/// Save a config flag immediately (only when it changed).
fn save_flag(ctx: &AppContext, update: impl FnOnce(&mut crate::config::ConfigData) -> bool) {
    let mut cfg = ctx.config.write();
    if update(&mut cfg.data) {
        cfg.save();
    }
}

fn grid_quality_index(value: &str) -> u32 {
    match value {
        "thumbnail" => 1,
        "preview" => 2,
        "fullsize" => 3,
        _ => 0,
    }
}

fn grid_quality_value(index: u32) -> &'static str {
    match index {
        1 => "thumbnail",
        2 => "preview",
        3 => "fullsize",
        _ => "auto",
    }
}

/// Library rows save straight to config (no Save button needed).
pub(super) fn connect_library_rows(
    ctx: &Arc<AppContext>,
    library: &LibraryWidgets,
    library_view_row: &adw::SwitchRow,
) {
    let (c, group) = (ctx.clone(), library.library_group.clone());
    library_view_row.connect_active_notify(move |row| {
        let active = row.is_active();
        group.set_visible(active);
        save_flag(&c, |d| {
            std::mem::replace(&mut d.library_view_enabled, active) != active
        });
    });
    let c = ctx.clone();
    library.preview_full_row.connect_active_notify(move |row| {
        let active = row.is_active();
        save_flag(&c, |d| {
            std::mem::replace(&mut d.library_preview_full_resolution, active) != active
        });
    });
    library.grid_quality_row.set_selected(grid_quality_index(
        &ctx.config.read().data.library_grid_quality,
    ));
    let c = ctx.clone();
    library
        .grid_quality_row
        .connect_selected_notify(move |row| {
            let value = grid_quality_value(row.selected());
            save_flag(&c, |d| {
                let changed = d.library_grid_quality != value;
                d.library_grid_quality = value.to_string();
                changed
            });
        });
    connect_raw_rows(ctx, library);
    connect_disk_cache_row(ctx, &library.disk_cache_row);
}

fn connect_raw_rows(ctx: &Arc<AppContext>, library: &LibraryWidgets) {
    library
        .raw_cache_row
        .set_sensitive(library.raw_full_decode_row.is_active());
    let (c, cache_row) = (ctx.clone(), library.raw_cache_row.clone());
    library
        .raw_full_decode_row
        .connect_active_notify(move |row| {
            let active = row.is_active();
            cache_row.set_sensitive(active);
            crate::library::set_raw_full_decode(active);
            save_flag(&c, |d| {
                std::mem::replace(&mut d.raw_full_decode, active) != active
            });
        });
    let c = ctx.clone();
    library.raw_cache_row.connect_active_notify(move |row| {
        let active = row.is_active();
        crate::library::set_raw_cache_enabled(active);
        save_flag(&c, |d| {
            std::mem::replace(&mut d.raw_decode_cache_enabled, active) != active
        });
    });
}

/// Debounced: the spin row fires on every step, so save 400ms after the last change.
fn connect_disk_cache_row(ctx: &Arc<AppContext>, row: &adw::SpinRow) {
    let pending: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
    let ctx = ctx.clone();
    row.connect_value_notify(move |row| {
        let new_value = row.value() as u32;
        if let Some(id) = pending.take() {
            id.remove();
        }
        let (ctx, pending_for_save) = (ctx.clone(), pending.clone());
        let id = glib::timeout_add_local_once(Duration::from_millis(400), move || {
            pending_for_save.set(None);
            save_flag(&ctx, |d| {
                std::mem::replace(&mut d.cache_disk_cap_mb, new_value) != new_value
            });
        });
        pending.set(Some(id));
    });
}

/// Choose / clear the folder library downloads are saved to.
pub(super) fn connect_download_folder(
    ctx: &Arc<AppContext>,
    library: &LibraryWidgets,
    window: &adw::ApplicationWindow,
) {
    if let Some(path) = ctx.config.read().data.download_target_path.as_deref() {
        library.download_folder_row.set_subtitle(path);
        library.download_clear_btn.set_visible(true);
    }
    let (c, row, clear) = (
        ctx.clone(),
        library.download_folder_row.clone(),
        library.download_clear_btn.clone(),
    );
    library.download_change_btn.connect_clicked(clone!(
        #[weak]
        window,
        move |_| {
            let (ctx, row, clear) = (c.clone(), row.clone(), clear.clone());
            let dialog = FileDialog::builder()
                .title("Choose Download Folder")
                .build();
            dialog.select_folder(Some(&window), gtk::gio::Cancellable::NONE, move |res| {
                let Some(path) = res.ok().and_then(|f| f.path()) else {
                    return;
                };
                let path_str = path.to_string_lossy().to_string();
                row.set_subtitle(&path_str);
                clear.set_visible(true);
                let mut cfg = ctx.config.write();
                cfg.data.download_target_path = Some(path_str);
                cfg.save();
            });
        }
    ));
    let c = ctx.clone();
    library.download_clear_btn.connect_clicked(clone!(
        #[weak(rename_to = row)]
        library.download_folder_row,
        move |btn| {
            row.set_subtitle("Not set");
            btn.set_visible(false);
            let mut cfg = c.config.write();
            cfg.data.download_target_path = None;
            cfg.save();
        }
    ));
}

/// Load albums for the folder-row picker.
pub(super) fn fetch_albums_for_picker(window: &adw::ApplicationWindow, state: &ApplyState) {
    // Hold the window weakly across the fetch so closing it mid-request frees the
    // row widgets instead of accumulating them over open/close cycles.
    let weak_win = window.downgrade();
    // Reuse the app-wide API client: a new reqwest client per window open keeps
    // its connection pool alive ~30s, so RAM grows with each open/close.
    let client = state.ctx.api_client.clone();
    let albums_ref = state.albums.clone();
    glib::MainContext::default().spawn_local(async move {
        let fetched = client.get_all_albums().await.unwrap_or_default();
        // Window closed during the fetch; returning drops albums_ref right away.
        if weak_win.upgrade().is_none() {
            log::debug!("Settings window closed during album fetch — discarding result.");
            return;
        }
        // The album picker reads albums_ref when opened, so rows need no update.
        *albums_ref.borrow_mut() = fetched;
    });
}

/// "Watch Folders" group: one row per configured folder plus an Add button.
pub(super) fn add_folders_group(
    settings_page: &adw::PreferencesPage,
    window: &adw::ApplicationWindow,
    config: &Config,
    state: &ApplyState,
    on_changed: Rc<dyn Fn()>,
) {
    let folders_group = adw::PreferencesGroup::builder()
        .title("Watch Folders")
        .description("Pick folders to sync.")
        .build();
    settings_page.add(&folders_group);
    // Folder list first, then the Add button below it.
    let folders_list = ListBox::builder()
        .margin_top(12)
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(vec!["boxed-list".to_string()])
        .build();
    folders_group.add(&folders_list);
    let add_folder_btn = Button::builder().label("Add Folder").margin_top(12).build();
    folders_group.add(&add_folder_btn);

    let catchup = config.data.startup_catchup_mode.clone();
    for entry in &config.data.watch_paths {
        add_folder_row(
            &folders_list,
            entry,
            catchup.clone(),
            state.albums.clone(),
            &state.tracked_rows,
            on_changed.clone(),
        );
    }
    let state = state.clone();
    let window = window.clone();
    add_folder_btn.connect_clicked(move |_| {
        pick_new_folder(
            &window,
            &folders_list,
            &state,
            catchup.clone(),
            on_changed.clone(),
        );
    });
}

/// Ask for a folder and add it as a row (ignoring folders already listed).
fn pick_new_folder(
    window: &adw::ApplicationWindow,
    folders_list: &ListBox,
    state: &ApplyState,
    catchup: crate::config::StartupCatchupMode,
    on_changed: Rc<dyn Fn()>,
) {
    let dialog = FileDialog::builder().title("Select Watch Folder").build();
    let (list, state) = (folders_list.clone(), state.clone());
    dialog.select_folder(Some(window), gtk::gio::Cancellable::NONE, move |res| {
        let Some(path) = res.ok().and_then(|file| file.path()) else {
            return;
        };
        let path_str = path.to_string_lossy().to_string();
        if state
            .tracked_rows
            .borrow()
            .iter()
            .any(|r| r.path == path_str)
        {
            return;
        }
        add_folder_row(
            &list,
            &WatchPathEntry::Simple(path_str),
            catchup,
            state.albums.clone(),
            &state.tracked_rows,
            on_changed.clone(),
        );
        on_changed();
    });
}

/// Queue inspector, pause/resume, sync now, diagnostics export, cache clearing, and quit.
pub(super) fn connect_action_buttons(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    ctx: &Arc<AppContext>,
    actions: &ActionsWidgets,
) {
    let queue_manager = ctx.queue_manager.clone();
    actions.pause_btn.set_label(if queue_manager.is_paused() {
        "Resume"
    } else {
        "Pause"
    });
    let qm = queue_manager.clone();
    actions.queue_btn.connect_clicked(clone!(
        #[weak]
        window,
        move |_| show_queue_inspector(&window, qm.clone())
    ));
    actions.pause_btn.connect_clicked(move |btn| {
        let paused = !queue_manager.is_paused();
        queue_manager.set_paused(paused, paused.then(|| "Paused by user".to_string()));
        btn.set_label(if paused { "Resume" } else { "Pause" });
    });
    let sync_now_tx = ctx.sync_now_tx.clone();
    actions.sync_now_btn.connect_clicked(move |_| {
        let _ = sync_now_tx.send(());
    });
    let c = ctx.clone();
    actions.export_btn.connect_clicked(clone!(
        #[weak]
        window,
        move |_| choose_diagnostics_folder(&window, c.clone())
    ));
    let thumbnail_cache = ctx.thumbnail_cache.clone();
    actions.clear_cache_btn.connect_clicked(clone!(
        #[weak]
        window,
        move |_| clear_caches(&window, &thumbnail_cache)
    ));
    let app = app.clone();
    actions.quit_btn.connect_clicked(move |_| app.quit());
}

fn choose_diagnostics_folder(window: &adw::ApplicationWindow, ctx: Arc<AppContext>) {
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

/// Drop the in-memory thumbnail textures immediately, then wipe every on-disk
/// cache subdirectory off the UI thread.
fn clear_caches(
    window: &adw::ApplicationWindow,
    thumbnail_cache: &crate::library::thumbnail_cache::ThumbnailCache,
) {
    let _ = thumbnail_cache.clear();
    let window = window.clone();
    glib::MainContext::default().spawn_local(async move {
        let result = tokio::task::spawn_blocking(crate::cache_manager::clear_all_blocking)
            .await
            .map_err(|err| err.to_string())
            .and_then(|inner| inner);
        let (heading, body) = match result {
            Ok(()) => (
                "Cache Cleared",
                "Removed thumbnails, decoded RAW previews, EXIF, video, and preview caches."
                    .to_string(),
            ),
            Err(err) => ("Could Not Clear Cache", err),
        };
        show_alert(&window, heading, &body);
    });
}

/// At least one server URL must stay enabled; each entry is editable only while enabled.
pub(super) fn connect_url_switches(window: &adw::ApplicationWindow, conn: &ConnectivityWidgets) {
    let pairs = [
        (
            &conn.internal_switch,
            &conn.external_switch,
            &conn.internal_entry,
        ),
        (
            &conn.external_switch,
            &conn.internal_switch,
            &conn.external_entry,
        ),
    ];
    for (switch, other, entry) in pairs {
        switch.connect_active_notify(clone!(
            #[weak]
            other,
            #[weak]
            entry,
            #[weak]
            window,
            move |switch| {
                if !switch.is_active() && !other.is_active() {
                    switch.set_active(true);
                    show_alert(
                        &window,
                        "Invalid Selection",
                        "At least one URL (Internal or External) must be enabled.",
                    );
                }
                entry.set_sensitive(switch.is_active());
            }
        ));
    }
}

/// Save behavior and appearance changes as soon as they're made.
pub(super) fn connect_auto_apply(form: &SettingsForm, auto_apply: Rc<dyn Fn()>) {
    let switches = [
        &form.startup_row,
        &form.metered_row,
        &form.battery_row,
        &form.notifications_row,
        &form.quiet_hours_row,
        &form.background_sync_row,
    ];
    for row in switches {
        let apply = auto_apply.clone();
        row.connect_active_notify(move |_| apply());
    }
    for row in [
        &form.concurrency_row,
        &form.quiet_start_row,
        &form.quiet_end_row,
        &form.border_width_row,
    ] {
        let apply = auto_apply.clone();
        row.connect_value_notify(move |_| apply());
    }
    let apply = auto_apply.clone();
    form.catchup_row.connect_selected_notify(move |_| apply());
    form.border_color_btn
        .connect_rgba_notify(move |_| auto_apply());
}

/// If background sync is disabled AND this is the only open window, closing
/// settings should exit the app. When the library window is also open we
/// must not quit — the user opened settings *from* the library and expects
/// the library to stay around after dismissing settings.
pub(super) fn connect_close_request(
    window: &adw::ApplicationWindow,
    app: &adw::Application,
    ctx: &Arc<AppContext>,
) {
    let (app, ctx) = (app.clone(), ctx.clone());
    window.connect_close_request(move |_| {
        // Read background sync directly from config rather than a shadow
        // RefCell, so any code path that changes the config is reflected here.
        // The closing window is still in app.windows() at this point: a count
        // of 1 means it is the last window, so quit; more means another window
        // (usually the library) is open and should keep running.
        let bg_sync = ctx.config.read().data.background_sync_enabled;
        if !bg_sync && app.windows().len() <= 1 {
            app.quit();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_report_names_the_active_route() {
        let (heading, body) = connection_report(true, false);
        assert_eq!(heading, "Connection Successful");
        assert_eq!(body, "Internal: OK\nExternal: FAILED\n\nActive Mode: LAN");
        assert!(
            connection_report(false, true)
                .1
                .ends_with("Active Mode: WAN")
        );
        assert_eq!(connection_report(false, false).0, "Connection Failed");
    }

    #[test]
    fn grid_quality_round_trips() {
        for value in ["auto", "thumbnail", "preview", "fullsize"] {
            assert_eq!(grid_quality_value(grid_quality_index(value)), value);
        }
        assert_eq!(grid_quality_index("unknown"), 0);
    }
}
