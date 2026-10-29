//! Settings window construction helpers and signal wiring.

use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use glib::clone;
use gtk::prelude::*;
use gtk::{Button, ScrolledWindow};
use libadwaita as adw;

use crate::app_context::AppContext;

use super::behavior::BehaviorWidgets;
use super::connectivity::ConnectivityWidgets;
use super::form::SettingsForm;
use super::queue_inspector::show_about_dialog;
use super::status::StatusWidgets;
use super::{show_alert, show_queue_inspector};

mod diagnostics_wiring;
mod folder_wiring;
mod library_wiring;
pub(super) use folder_wiring::{
    add_folders_group, connect_download_folder, fetch_albums_for_picker, folders_group,
};
pub(super) use library_wiring::connect_library_rows;

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
        move |_| diagnostics_wiring::choose_diagnostics_folder(&window, c.clone())
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
