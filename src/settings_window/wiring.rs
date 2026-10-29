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
use crate::config::{Config, DateHeadings, WatchPathEntry};
use crate::diagnostics;

use super::apply::ApplyState;
use super::behavior::BehaviorWidgets;
use super::connectivity::ConnectivityWidgets;
use super::form::SettingsForm;
use super::library::LibraryWidgets;
use super::queue_inspector::show_about_dialog;
use super::status::StatusWidgets;
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
    let (header, switcher) = build_header_bar(app, &window, &view_stack);
    toolbar_view.add_top_bar(&header);
    toolbar_view.set_content(Some(&view_stack));
    let switcher_bar = adw::ViewSwitcherBar::builder().stack(&view_stack).build();
    toolbar_view.add_bottom_bar(&switcher_bar);
    window.set_content(Some(&toolbar_view));
    add_narrow_switcher(&window, &switcher, &switcher_bar);

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

/// Header with the Status / Settings switcher and a main menu (About, Quit).
fn build_header_bar(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    view_stack: &adw::ViewStack,
) -> (adw::HeaderBar, adw::ViewSwitcher) {
    let switcher = adw::ViewSwitcher::builder()
        .stack(view_stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    let header_bar = adw::HeaderBar::builder().title_widget(&switcher).build();
    let menu = gtk::gio::Menu::new();
    menu.append(Some("About Mimick"), Some("win.about"));
    menu.append(Some("Quit Mimick"), Some("win.quit"));
    let menu_btn = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main Menu")
        .menu_model(&menu)
        .build();
    header_bar.pack_end(&menu_btn);
    let about = gtk::gio::SimpleAction::new("about", None);
    let window_for_about = window.clone();
    about.connect_activate(move |_, _| show_about_dialog(&window_for_about));
    window.add_action(&about);
    let quit = gtk::gio::SimpleAction::new("quit", None);
    let app = app.clone();
    quit.connect_activate(move |_, _| app.quit());
    window.add_action(&quit);
    (header_bar, switcher)
}

/// On narrow windows the page switcher moves to a bar at the bottom.
fn add_narrow_switcher(
    window: &adw::ApplicationWindow,
    switcher: &adw::ViewSwitcher,
    bar: &adw::ViewSwitcherBar,
) {
    let breakpoint = adw::Breakpoint::new(
        adw::BreakpointCondition::parse("max-width: 450sp").expect("valid breakpoint condition"),
    );
    breakpoint.add_setter(switcher, "visible", Some(&false.to_value()));
    breakpoint.add_setter(bar, "reveal", Some(&true.to_value()));
    window.add_breakpoint(breakpoint);
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

/// First-run steps shown until an API key is configured.
pub(super) fn add_welcome_group(settings_page: &adw::PreferencesPage) {
    let welcome_group = adw::PreferencesGroup::builder()
        .title("Get Started")
        .description("Three steps to start backing up to Immich.")
        .build();
    let steps = [
        (
            "1. Enter your server",
            "Your Immich address under Server, below.",
        ),
        (
            "2. Add an API key",
            "Create one in Immich under Account Settings → API Keys. Open the guide for the permissions it needs.",
        ),
        (
            "3. Choose a folder",
            "Add a folder under Watch Folders, then Save and Connect.",
        ),
    ];
    for (index, (title, subtitle)) in steps.into_iter().enumerate() {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .subtitle_lines(3)
            .build();
        if index == 1 {
            let guide = Button::builder()
                .label("Guide")
                .valign(gtk::Align::Center)
                .build();
            guide.connect_clicked(|_| {
                let uri = "https://github.com/nicx17/mimick/wiki/Configuration-and-First-Run";
                if let Err(e) = gtk::gio::AppInfo::launch_default_for_uri(
                    uri,
                    None::<&gtk::gio::AppLaunchContext>,
                ) {
                    log::error!("Failed to open browser: {}", e);
                }
            });
            row.add_suffix(&guide);
        }
        welcome_group.add(&row);
    }
    settings_page.add(&welcome_group);
}

/// One line for a connection test: which address worked, and how the other fared.
fn connection_summary(
    internal_ok: bool,
    external_ok: bool,
    internal_on: bool,
    external_on: bool,
) -> String {
    let other = |name: &str, on: bool, ok: bool| match (on, ok) {
        (false, _) => String::new(),
        (true, true) => format!(" · {name} also works"),
        (true, false) => format!(" · {name} unreachable"),
    };
    if internal_ok {
        format!(
            "Connected through LAN{}",
            other("WAN", external_on, external_ok)
        )
    } else if external_ok {
        format!(
            "Connected through WAN{}",
            other("LAN", internal_on, internal_ok)
        )
    } else {
        "Couldn't reach Immich at either address".to_string()
    }
}

/// Ping the enabled addresses off the GTK thread and show the result in the Connection row.
pub(super) fn connect_test_button(
    conn: &ConnectivityWidgets,
    api_client: Arc<crate::api_client::ImmichApiClient>,
) {
    let status_row = conn.status_row.downgrade();
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
            let (internal_on, external_on) = (!internal.is_empty(), !external.is_empty());
            if let Some(row) = status_row.upgrade() {
                row.set_subtitle("Testing…");
            }
            let rx = spawn_ping(api_client.clone(), internal, external);
            let (status_row, btn) = (status_row.clone(), btn.clone());
            glib::MainContext::default().spawn_local(async move {
                let Ok((int_ok, ext_ok)) = rx.await else {
                    return;
                };
                btn.set_sensitive(true);
                if let Some(row) = status_row.upgrade() {
                    row.set_subtitle(&connection_summary(
                        int_ok,
                        ext_ok,
                        internal_on,
                        external_on,
                    ));
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

/// Show the From / Until rows only while quiet hours are on.
pub(super) fn connect_quiet_hours_sensitivity(behavior: &BehaviorWidgets) {
    let rows = [
        behavior.quiet_start_row.clone(),
        behavior.quiet_end_row.clone(),
    ];
    let show = move |active: bool| {
        for row in &rows {
            row.set_visible(active);
        }
    };
    show(behavior.quiet_hours_row.is_active());
    behavior
        .quiet_hours_row
        .connect_active_notify(move |row| show(row.is_active()));
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
    let enabled = ctx.config.read().data.library_view_enabled;
    for group in &library.library_groups {
        group.set_visible(enabled);
    }
    let (c, groups) = (ctx.clone(), library.library_groups.clone());
    library_view_row.connect_active_notify(move |row| {
        let active = row.is_active();
        for group in &groups {
            group.set_visible(active);
        }
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
            c.publish_grid_style();
        });
    connect_date_headings_row(ctx, &library.date_headings_row);
    connect_raw_rows(ctx, library);
    connect_disk_cache_row(ctx, &library.disk_cache_row);
    connect_clear_cache(ctx, library);
    refresh_cache_usage(&library.disk_cache_row);
}

/// Measure the caches off the GTK thread and show it under the cache limit.
fn refresh_cache_usage(row: &adw::SpinRow) {
    let row = row.downgrade();
    glib::MainContext::default().spawn_local(async move {
        let used = tokio::task::spawn_blocking(crate::cache_manager::total_size_blocking)
            .await
            .unwrap_or(0);
        if let Some(row) = row.upgrade() {
            row.set_subtitle(&super::library::cache_usage_label(used, row.value() as u32));
        }
    });
}

fn connect_clear_cache(ctx: &Arc<AppContext>, library: &LibraryWidgets) {
    let (thumbnail_cache, row) = (ctx.thumbnail_cache.clone(), library.disk_cache_row.clone());
    library.clear_cache_btn.connect_clicked(move |btn| {
        let _ = thumbnail_cache.clear();
        btn.set_sensitive(false);
        let (btn, row) = (btn.clone(), row.clone());
        glib::MainContext::default().spawn_local(async move {
            let result = tokio::task::spawn_blocking(crate::cache_manager::clear_all_blocking)
                .await
                .map_err(|err| err.to_string())
                .and_then(|inner| inner);
            btn.set_sensitive(true);
            if let Err(err) = result
                && let Some(window) = btn.root().and_downcast::<gtk::Window>()
            {
                show_alert(&window, "Could Not Clear Cache", &err);
            }
            refresh_cache_usage(&row);
        });
    });
}

fn connect_date_headings_row(ctx: &Arc<AppContext>, row: &adw::ComboRow) {
    let current = ctx.config.read().data.library_date_headings;
    let index = DateHeadings::ALL.iter().position(|mode| *mode == current);
    row.set_selected(index.unwrap_or_default() as u32);
    let ctx = ctx.clone();
    row.connect_selected_notify(move |row| {
        let mode = DateHeadings::ALL
            .get(row.selected() as usize)
            .copied()
            .unwrap_or_default();
        save_flag(&ctx, |d| {
            std::mem::replace(&mut d.library_date_headings, mode) != mode
        });
        ctx.date_headings.send_replace(mode);
    });
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
        let (ctx, pending_for_save, row) = (ctx.clone(), pending.clone(), row.clone());
        let id = glib::timeout_add_local_once(Duration::from_millis(400), move || {
            pending_for_save.set(None);
            refresh_cache_usage(&row);
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
            row.set_subtitle("Ask each time");
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

/// Empty "Watch Folders" section, added early so it sits right after Server.
pub(super) fn folders_group(settings_page: &adw::PreferencesPage) -> adw::PreferencesGroup {
    let folders_group = adw::PreferencesGroup::builder()
        .title("Watch Folders")
        .description("Folders Mimick backs up. Expand one to pick its album and rules.")
        .build();
    settings_page.add(&folders_group);
    folders_group
}

/// Fill the "Watch Folders" section: one row per configured folder plus an Add button.
pub(super) fn add_folders_group(
    folders_group: &adw::PreferencesGroup,
    window: &adw::ApplicationWindow,
    config: &Config,
    state: &ApplyState,
    on_changed: Rc<dyn Fn()>,
) {
    // Folder list first, then the Add button below it.
    let folders_list = ListBox::builder()
        .margin_top(12)
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(vec!["boxed-list".to_string()])
        .build();
    folders_group.add(&folders_list);
    let add_folder_btn = Button::builder()
        .label("Add Folder…")
        .margin_top(12)
        .build();
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

/// Status page buttons (sync now, pause, queue, retry) and the diagnostics export.
pub(super) fn connect_action_buttons(
    window: &adw::ApplicationWindow,
    ctx: &Arc<AppContext>,
    status: &StatusWidgets,
    export_btn: &Button,
) {
    let queue_manager = ctx.queue_manager.clone();
    status.pause_btn.set_label(if queue_manager.is_paused() {
        "Resume"
    } else {
        "Pause"
    });
    let qm = queue_manager.clone();
    status.queue_btn.connect_clicked(clone!(
        #[weak]
        window,
        move |_| show_queue_inspector(&window, qm.clone())
    ));
    let qm = queue_manager.clone();
    status.retry_btn.connect_clicked(move |_| {
        let qm = qm.clone();
        glib::MainContext::default().spawn_local(async move {
            qm.retry_all_failed().await;
        });
    });
    status.pause_btn.connect_clicked(move |btn| {
        let paused = !queue_manager.is_paused();
        queue_manager.set_paused(paused, paused.then(|| "Paused by you".to_string()));
        btn.set_label(if paused { "Resume" } else { "Pause" });
    });
    let sync_now_tx = ctx.sync_now_tx.clone();
    status.sync_now_btn.connect_clicked(move |_| {
        let _ = sync_now_tx.send(());
    });
    let c = ctx.clone();
    export_btn.connect_clicked(clone!(
        #[weak]
        window,
        move |_| choose_diagnostics_folder(&window, c.clone())
    ));
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
        &form.xmp_sidecar_row,
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
    fn connection_summary_names_what_worked() {
        assert_eq!(
            connection_summary(true, false, true, true),
            "Connected through LAN · WAN unreachable"
        );
        assert_eq!(
            connection_summary(true, true, true, true),
            "Connected through LAN · WAN also works"
        );
        assert_eq!(
            connection_summary(false, true, true, true),
            "Connected through WAN · LAN unreachable"
        );
        assert_eq!(
            connection_summary(true, false, true, false),
            "Connected through LAN"
        );
        assert_eq!(
            connection_summary(false, false, true, true),
            "Couldn't reach Immich at either address"
        );
    }

    #[test]
    fn grid_quality_round_trips() {
        for value in ["auto", "thumbnail", "preview", "fullsize"] {
            assert_eq!(grid_quality_value(grid_quality_index(value)), value);
        }
        assert_eq!(grid_quality_index("unknown"), 0);
    }
}
