//! Header bar button wiring, search, sort, source-mode, timeline, and sidebar/grid handlers.
//!
//! Connects signal handlers for the library header bar controls including
//! the search entry, sort-mode dropdown, view-source toggle, timeline
//! scrubber, and sidebar collapse button.

use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::library::state::{LibrarySortMode, LibrarySource, Location};
use crate::settings_window::{build_settings_window_with_parent, show_queue_inspector};

use super::navigation;
use super::{LibraryWindowUi, load_albums, load_source_page, load_status, open_lightbox};

pub(super) fn connect_controls(ui: Rc<LibraryWindowUi>) {
    connect_window_actions(&ui);
    connect_shortcuts_action(&ui);
    connect_navigation_controls(&ui);
    connect_sort_controls(&ui);
    connect_source_controls(&ui);
}

/// `win.settings`, `win.queue`, `win.refresh`, and the server-stats rows.
fn connect_window_actions(ui: &Rc<LibraryWindowUi>) {
    let action_settings = gtk::gio::SimpleAction::new("settings", None);
    action_settings.connect_activate(clone!(
        #[strong]
        ui,
        move |_, _| {
            build_settings_window_with_parent(&ui.app, ui.ctx.clone(), Some(&ui.window));
        }
    ));
    ui.window.add_action(&action_settings);

    let action_queue = gtk::gio::SimpleAction::new("queue", None);
    action_queue.connect_activate(clone!(
        #[strong]
        ui,
        move |_, _| {
            show_queue_inspector(&ui.window, ui.ctx.queue_manager.clone());
        }
    ));
    ui.window.add_action(&action_queue);

    let action_refresh = gtk::gio::SimpleAction::new("refresh", None);
    action_refresh.connect_activate(clone!(
        #[strong]
        ui,
        move |_, _| {
            refresh_library_surfaces(ui.clone(), true);
        }
    ));
    ui.window.add_action(&action_refresh);

    // Connection row: usage statistics. Server row: version/build details.
    ui.sidebar.connection_row.connect_activated(clone!(
        #[strong]
        ui,
        move |_| {
            super::server_stats_dialog::present(ui.ctx.clone(), &ui.window);
        }
    ));
    ui.sidebar.server_row.connect_activated(clone!(
        #[strong]
        ui,
        move |_| {
            super::about_dialog::present(ui.ctx.clone(), &ui.window);
        }
    ));
}

fn connect_shortcuts_action(ui: &Rc<LibraryWindowUi>) {
    let action = gtk::gio::SimpleAction::new("shortcuts", None);
    action.connect_activate(clone!(
        #[strong]
        ui,
        move |_, _| super::shortcuts_dialog::present(&ui.window)
    ));
    ui.window.add_action(&action);
    ui.app
        .set_accels_for_action("win.shortcuts", &["<Control>question"]);
}

/// Upload button and the F5 shortcut; Back is wired in `connect_back_controls`.
fn connect_navigation_controls(ui: &Rc<LibraryWindowUi>) {
    ui.upload_button.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| {
            super::upload_picker::pick_and_upload(&ui.window, ui.ctx.clone(), current_album(&ui));
        }
    ));
    connect_back_controls(ui);

    let f5_controller = gtk::EventControllerKey::new();
    f5_controller.connect_key_pressed(clone!(
        #[strong]
        ui,
        move |_, keyval, _, _| {
            if keyval != gtk::gdk::Key::F5 {
                return glib::Propagation::Proceed;
            }
            let _ = ui
                .window
                .upcast_ref::<gtk::Widget>()
                .activate_action("win.refresh", None);
            glib::Propagation::Stop
        }
    ));
    ui.window.add_controller(f5_controller);
}

type NavAction = fn(&Rc<LibraryWindowUi>) -> bool;

fn connect_back_controls(ui: &Rc<LibraryWindowUi>) {
    ui.back_button.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| {
            navigation::go_back(&ui);
        }
    ));
    add_mouse_button(ui, 8, navigation::go_back);
    add_mouse_button(ui, 9, navigation::go_forward);
    add_shortcut(ui, "<Alt>Left", navigation::go_back);
    add_shortcut(ui, "<Alt>Right", navigation::go_forward);
    connect_swipe_back(ui);
}

fn add_mouse_button(ui: &Rc<LibraryWindowUi>, button: u32, action: NavAction) {
    let gesture = gtk::GestureClick::builder()
        .button(button)
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    gesture.connect_pressed(clone!(
        #[strong]
        ui,
        move |gesture, _, _, _| {
            if action(&ui) {
                gesture.set_state(gtk::EventSequenceState::Claimed);
            }
        }
    ));
    ui.window.add_controller(gesture);
}

fn add_shortcut(ui: &Rc<LibraryWindowUi>, trigger: &str, action: NavAction) {
    let shortcut = gtk::Shortcut::builder()
        .trigger(&gtk::ShortcutTrigger::parse_string(trigger).expect("valid shortcut"))
        .action(&gtk::CallbackAction::new(clone!(
            #[strong]
            ui,
            move |_, _| {
                if action(&ui) {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        )))
        .build();
    let controller = gtk::ShortcutController::new();
    controller.add_shortcut(shortcut);
    ui.window.add_controller(controller);
}

fn connect_swipe_back(ui: &Rc<LibraryWindowUi>) {
    let swipe = gtk::GestureSwipe::builder().touch_only(true).build();
    let start_x = Rc::new(std::cell::Cell::new(0.0));
    let start_for_begin = start_x.clone();
    swipe.connect_begin(move |gesture, sequence| {
        if let Some((x, _)) = gesture.point(sequence) {
            start_for_begin.set(x);
        }
    });
    swipe.connect_swipe(clone!(
        #[strong]
        ui,
        move |_, vx, vy| {
            if is_back_swipe(start_x.get(), vx, vy) {
                navigation::go_back(&ui);
            }
        }
    ));
    ui.content_stack.add_controller(swipe);
}

fn is_back_swipe(start_x: f64, vx: f64, vy: f64) -> bool {
    start_x > 32.0 && vx > 600.0 && vx > vy.abs() * 2.0
}

/// The sort dropdown changes options per view and sorts the visible content.
fn connect_sort_controls(ui: &Rc<LibraryWindowUi>) {
    // Rebuild the sort dropdown model when the visible content view changes:
    // each view has its own sort taxonomy.
    ui.content_stack.connect_visible_child_notify(clone!(
        #[strong]
        ui,
        move |stack| {
            let view = stack.visible_child_name();
            let model = gtk::StringList::new(sort_labels_for_view(view.as_deref()));
            ui.sort_mode.set_model(Some(&model));
            ui.sort_mode.set_selected(0);
            // Switching views resets in-view filters: Albums resets its album filter,
            // Explore its people search, and the grid views reset both.
            if view.as_deref() != Some("explore") {
                super::albums_view::set_search_filter(&ui.albums, "");
            }
            if view.as_deref() != Some("albums") {
                super::explore_view::set_people_search(&ui.explore, "");
            }
        }
    ));

    ui.sort_mode.connect_selected_notify(clone!(
        #[strong]
        ui,
        move |dropdown| apply_sort_selection(&ui, dropdown.selected())
    ));
}

/// Sort options for the visible content view.
fn sort_labels_for_view(view: Option<&str>) -> &'static [&'static str] {
    match view {
        Some("albums") => &["Newest", "Name", "Most assets"],
        Some("explore") => &["Default"],
        _ => &["Newest", "Filename", "File Type"],
    }
}

/// Route a sort choice by active view. Albums uses an album-specific sort
/// taxonomy; Explore intentionally doesn't sort the people row (curated server order).
fn apply_sort_selection(ui: &Rc<LibraryWindowUi>, selected: u32) {
    let view = ui.content_stack.visible_child_name();
    if view.as_deref() == Some("albums") {
        let mode = match selected {
            1 => super::albums_view::AlbumsSort::Name,
            2 => super::albums_view::AlbumsSort::MostAssets,
            _ => super::albums_view::AlbumsSort::Newest,
        };
        super::albums_view::set_sort_mode(&ui.albums, mode);
        return;
    }
    let sort_mode = match selected {
        1 => LibrarySortMode::Filename,
        2 => LibrarySortMode::FileType,
        _ => LibrarySortMode::NewestFirst,
    };
    let mut state = ui.ctx.library_state.lock();
    state.apply_sort(sort_mode);
    if super::timeline::eligible(&state.source, &state.sort_mode) != super::timeline::is_active(ui)
    {
        drop(state);
        navigation::reload_current(ui);
        return;
    }
    ui.grid
        .model
        .reset(&ui.ctx, &state.assets, &state.sort_mode);
}

/// Remote/Local/Unified dropdown.
fn connect_source_controls(ui: &Rc<LibraryWindowUi>) {
    ui.source_mode.connect_selected_notify(clone!(
        #[strong]
        ui,
        move |dropdown| {
            if ui.source_mode_suppressed.get() {
                return;
            }
            let source = source_for_selection(dropdown.selected(), current_album(&ui));
            navigation::navigate_source(&ui, source);
        }
    ));
}

/// The album being browsed, if any, as `(id, name)`.
fn current_album(ui: &LibraryWindowUi) -> Option<(String, String)> {
    match ui.ctx.library_state.lock().source.clone() {
        LibrarySource::Album { id, name }
        | LibrarySource::AlbumLocal { id, name }
        | LibrarySource::AlbumUnified { id, name } => Some((id, name)),
        _ => None,
    }
}

/// Source for a Remote (0) / Local (1) / Unified (2) choice, scoped to the open album if any.
fn source_for_selection(selected: u32, album: Option<(String, String)>) -> LibrarySource {
    match (selected, album) {
        (1, Some((id, name))) => LibrarySource::AlbumLocal { id, name },
        (1, None) => LibrarySource::LocalAll,
        (2, Some((id, name))) => LibrarySource::AlbumUnified { id, name },
        (2, None) => LibrarySource::Unified,
        (_, Some((id, name))) => LibrarySource::Album { id, name },
        (_, None) => LibrarySource::AllAssets,
    }
}

pub(super) fn refresh_library_surfaces(ui: Rc<LibraryWindowUi>, include_current_source: bool) {
    load_albums(ui.clone());
    load_status(ui.clone());
    if include_current_source {
        super::features::load(&ui);
    }

    // Clearing `populated` marks the cached explore/albums grids as stale
    // but, by itself, won't repaint a visible tab. Re-trigger the loader
    // for whichever grid is currently on-screen so the user sees fresh
    // content (with spinners) instead of silently-stale tiles.
    let visible = ui.content_stack.visible_child_name();
    ui.explore.populated.set(false);
    ui.map.populated.set(false);
    ui.map.tiles.retry_failed();
    match visible.as_deref() {
        Some("explore") => super::load_explore_landing(ui.clone()),
        Some("albums") => super::refresh_albums_view(ui.clone()),
        _ => {}
    }

    if !include_current_source {
        return;
    }
    if navigation::on_page(&ui) {
        navigation::reload_current(&ui);
        return;
    }
    if super::timeline::is_active(&ui) {
        super::timeline::reload(&ui);
        return;
    }
    let request = {
        let source = ui.ctx.library_state.lock().source.clone();
        ui.ctx.library_state.lock().switch_source(source)
    };
    super::source_header::show(&ui, &request.1);
    load_source_page(ui, request, false);
}

pub(super) fn refresh_library_after_mutation(ui: Rc<LibraryWindowUi>, prefer_current_source: bool) {
    refresh_library_surfaces(ui, prefer_current_source);
}

pub(super) fn connect_sidebar_handlers(ui: Rc<LibraryWindowUi>) {
    for list in &ui.sidebar.fixed_lists {
        connect_fixed_list(&ui, list);
    }

    ui.sidebar.albums_list.connect_row_selected(clone!(
        #[strong]
        ui,
        move |_, row| {
            if ui.sidebar_suppressed.get() {
                return;
            }
            let Some(tooltip) = row.and_then(|row| row.tooltip_text()) else {
                return;
            };
            let (id, name) = tooltip
                .split_once(':')
                .unwrap_or((tooltip.as_str(), "Album"));
            navigation::navigate_source(
                &ui,
                LibrarySource::Album {
                    id: id.to_string(),
                    name: name.to_string(),
                },
            );
            auto_hide_sidebar(&ui);
        }
    ));
}

fn connect_fixed_list(ui: &Rc<LibraryWindowUi>, list: &gtk::ListBox) {
    list.connect_row_selected(clone!(
        #[strong]
        ui,
        move |_, row| {
            if ui.sidebar_suppressed.get() {
                return;
            }
            let Some(row) = row else {
                return;
            };
            let location = match super::sidebar::recent_album_key(row) {
                Some((id, name)) => Some(Location::Source(LibrarySource::Album { id, name })),
                None => navigation::location_for_key(&row.tooltip_text().unwrap_or_default()),
            };
            if let Some(location) = location {
                navigation::navigate(&ui, location);
            }
            auto_hide_sidebar(&ui);
        }
    ));
}

fn auto_hide_sidebar(ui: &Rc<LibraryWindowUi>) {
    if ui.split.is_collapsed() {
        ui.split.set_show_sidebar(false);
    }
}

pub(super) fn connect_grid_handlers(ui: Rc<LibraryWindowUi>) {
    let ui_for_activate = ui.clone();
    ui.grid.canvas.set_activate_handler(move |position| {
        open_lightbox(ui_for_activate.clone(), position);
    });

    let scroll_pending = Rc::new(std::cell::Cell::new(false));
    ui.grid.scrolled.vadjustment().connect_value_changed(clone!(
        #[strong]
        ui,
        #[strong]
        scroll_pending,
        move |_adj| {
            if scroll_pending.replace(true) {
                return;
            }
            let ui = ui.clone();
            let scroll_pending = scroll_pending.clone();
            glib::idle_add_local_once(move || {
                scroll_pending.set(false);
                let adj = ui.grid.scrolled.vadjustment();

                let threshold = (adj.upper() - adj.page_size()) * 0.50;
                if adj.value() < threshold {
                    return;
                }

                let next = ui.ctx.library_state.lock().load_next_page_if_needed();
                if let Some(request) = next {
                    load_source_page(ui.clone(), request, true);
                }
            });
        }
    ));
}

#[cfg(test)]
mod tests {
    use super::{sort_labels_for_view, source_for_selection};
    use crate::library::state::LibrarySource;

    fn album() -> Option<(String, String)> {
        Some(("a1".to_string(), "Trips".to_string()))
    }

    #[test]
    fn back_swipe_needs_a_fast_mostly_horizontal_swipe_away_from_the_edge() {
        assert!(super::is_back_swipe(100.0, 900.0, 100.0));
        assert!(!super::is_back_swipe(10.0, 900.0, 100.0));
        assert!(!super::is_back_swipe(100.0, 300.0, 0.0));
        assert!(!super::is_back_swipe(100.0, 900.0, 800.0));
        assert!(!super::is_back_swipe(100.0, -900.0, 0.0));
    }

    #[test]
    fn source_for_selection_scopes_to_the_open_album() {
        assert!(matches!(
            source_for_selection(1, album()),
            LibrarySource::AlbumLocal { .. }
        ));
        assert!(matches!(
            source_for_selection(2, album()),
            LibrarySource::AlbumUnified { .. }
        ));
        assert!(matches!(
            source_for_selection(0, album()),
            LibrarySource::Album { .. }
        ));
    }

    #[test]
    fn source_for_selection_without_album() {
        assert_eq!(source_for_selection(1, None), LibrarySource::LocalAll);
        assert_eq!(source_for_selection(2, None), LibrarySource::Unified);
        assert_eq!(source_for_selection(0, None), LibrarySource::AllAssets);
    }

    #[test]
    fn sort_labels_follow_the_visible_view() {
        assert_eq!(sort_labels_for_view(Some("albums"))[2], "Most assets");
        assert_eq!(sort_labels_for_view(Some("explore")), &["Default"]);
        assert_eq!(sort_labels_for_view(Some("grid"))[1], "Filename");
        assert_eq!(sort_labels_for_view(None)[0], "Newest");
    }
}
