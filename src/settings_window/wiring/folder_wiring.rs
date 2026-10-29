use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use glib::clone;
use gtk::prelude::*;
use gtk::{Button, FileDialog, ListBox};
use libadwaita as adw;

use crate::app_context::AppContext;
use crate::config::{Config, WatchPathEntry};
use crate::settings_window::apply::ApplyState;
use crate::settings_window::library::LibraryWidgets;
use crate::settings_window::watch_folders::add_folder_row;

/// Choose / clear the folder library downloads are saved to.
pub(crate) fn connect_download_folder(
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
pub(crate) fn fetch_albums_for_picker(window: &adw::ApplicationWindow, state: &ApplyState) {
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
pub(crate) fn folders_group(settings_page: &adw::PreferencesPage) -> adw::PreferencesGroup {
    let folders_group = adw::PreferencesGroup::builder()
        .title("Watch Folders")
        .description("Folders Mimick backs up. Expand one to pick its album and rules.")
        .build();
    settings_page.add(&folders_group);
    folders_group
}

/// Fill the "Watch Folders" section: one row per configured folder plus an Add button.
pub(crate) fn add_folders_group(
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
