//! Settings window construction helpers and signal wiring.

use gettextrs::gettext;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use glib::clone;
use gtk::prelude::*;
use gtk::{Button, ScrolledWindow};
use libadwaita as adw;

use crate::app_context::AppContext;

use super::behavior::BehaviorWidgets;
use super::form::SettingsForm;
use super::show_queue_inspector;
use super::status::StatusWidgets;

mod connectivity_wiring;
mod diagnostics_wiring;
mod folder_wiring;
mod library_wiring;
pub(super) use connectivity_wiring::{connect_test_button, connect_url_switches};
pub(super) use folder_wiring::{
    add_folders_group, connect_download_folder, fetch_albums_for_picker, folders_group,
};
pub(super) use library_wiring::connect_library_rows;

/// Window with Status / Settings pages behind a header view switcher, showing `start_page`
/// ("status" or "settings") first.
pub(super) fn build_window_shell(
    app: &adw::Application,
    parent: Option<&adw::ApplicationWindow>,
    start_page: &str,
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
    configure_toolbar_view(app, &window, &view_stack);

    let status_page = add_page(
        &view_stack,
        "status",
        &gettext("Status"),
        "dialog-information-symbolic",
    );
    let settings_page = add_page(
        &view_stack,
        "settings",
        &gettext("Settings"),
        "emblem-system-symbolic",
    );
    view_stack.set_visible_child_name(start_page);
    (window, status_page, settings_page)
}

fn configure_toolbar_view(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    view_stack: &adw::ViewStack,
) {
    let toolbar_view = adw::ToolbarView::builder().build();
    let (header, switcher) = build_header_bar(app, window, view_stack);
    toolbar_view.add_top_bar(&header);
    toolbar_view.set_content(Some(view_stack));
    let switcher_bar = adw::ViewSwitcherBar::builder().stack(view_stack).build();
    toolbar_view.add_bottom_bar(&switcher_bar);
    window.set_content(Some(&toolbar_view));
    add_narrow_switcher(window, &switcher, &switcher_bar);
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
    menu.append(Some(&gettext("About Mimick")), Some("win.about"));
    menu.append(Some(&gettext("Quit Mimick")), Some("win.quit"));
    let menu_btn = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text(gettext("Main Menu"))
        .menu_model(&menu)
        .build();
    header_bar.pack_end(&menu_btn);
    let about = gtk::gio::SimpleAction::new("about", None);
    let window_for_about = window.clone();
    about.connect_activate(move |_, _| crate::about::present(&window_for_about));
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
        .title(gettext("Get Started"))
        .description(gettext(
            "Three steps to connect Mimick to your Immich server.",
        ))
        .build();
    let steps = [
        (
            &gettext("1. Enter your server"),
            &gettext("Your Immich address under Server, below."),
        ),
        (
            &gettext("2. Add an API key"),
            &gettext("In Immich: Account Settings → API Keys. The guide lists the permissions."),
        ),
        (
            &gettext("3. Save and Connect"),
            &gettext(
                "Your library opens next. To back up a folder too, add it under Watch Folders.",
            ),
        ),
    ];
    for (index, (title, subtitle)) in steps.into_iter().enumerate() {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .subtitle_lines(3)
            .build();
        if index == 1 {
            row.add_suffix(&build_guide_button());
        }
        welcome_group.add(&row);
    }
    settings_page.add(&welcome_group);
}

fn build_guide_button() -> Button {
    let guide = Button::builder()
        .label(gettext("Guide"))
        .valign(gtk::Align::Center)
        .build();
    guide.connect_clicked(|_| {
        let uri = "https://github.com/nicx17/mimick/wiki/Configuration-and-First-Run";
        if let Err(e) =
            gtk::gio::AppInfo::launch_default_for_uri(uri, None::<&gtk::gio::AppLaunchContext>)
        {
            log::error!("Failed to open browser: {}", e);
        }
    });
    guide
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
    status.pause_btn.set_label(&if queue_manager.is_paused() {
        gettext("Resume")
    } else {
        gettext("Pause")
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
        queue_manager.set_paused(paused, paused.then(|| gettext("Paused by you")));
        btn.set_label(&if paused {
            gettext("Resume")
        } else {
            gettext("Pause")
        });
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

/// Detailed Logs takes effect at once and is remembered across restarts.
pub(super) fn connect_detailed_logs_row(ctx: &Arc<AppContext>, row: &adw::SwitchRow) {
    row.set_active(ctx.config.read().data.detailed_logging);
    let ctx = ctx.clone();
    row.connect_active_notify(move |row| {
        let detailed = row.is_active();
        crate::logging::set_detailed(detailed);
        save_flag(&ctx, |d| {
            std::mem::replace(&mut d.detailed_logging, detailed) != detailed
        });
    });
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
        // Reads the config each time, so a change from any window counts.
        if crate::ui_lifecycle::quits_on_close(&app, &ctx) {
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
    fn grid_quality_round_trips() {
        for value in ["auto", "thumbnail", "preview", "fullsize"] {
            assert_eq!(grid_quality_value(grid_quality_index(value)), value);
        }
        assert_eq!(grid_quality_index("unknown"), 0);
    }
}
