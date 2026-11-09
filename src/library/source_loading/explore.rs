//! Explore's landing sections, and opening one of its photos in the lightbox.

use gettextrs::gettext;
use std::cell::RefCell;
use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;

use crate::library::content_state::show_library_permission_error;
use crate::library::state::LibrarySource;
use crate::library::{LibraryWindowUi, explore_view, lightbox::open_lightbox, navigation};

use super::load_source_page;

/// Load smart/metadata sections onto the Explore dashboard view.
///
/// Switches to the Explore stack child immediately so the user sees the
/// section headers + per-section spinners while data is in flight. Each of
/// the three fetches runs as an independent task, so a slow endpoint never
/// blocks the others — first contentful paint happens as soon as the
/// fastest section returns.
pub(in crate::library) fn load_explore_landing(ui: Rc<LibraryWindowUi>) {
    if ui.explore.populated.get() {
        log::debug!("Explore: populated=true, reusing cached widgets");
        ui.content_stack.set_visible_child_name("explore");
        return;
    }
    ui.explore.populated.set(true);
    explore_view::wire_people_filter(&ui.explore, ui.ctx.clone(), || {});
    explore_view::show_loading(&ui.explore);
    explore_view::calendar::load(&ui.explore.calendar, &ui);
    ui.content_stack.set_visible_child_name("explore");

    let sections = ui.explore.sections.get();
    if sections.people {
        load_explore_people(ui.clone());
    }
    // Fetch places (slow paginated scan) only if not cached.
    if sections.places && explore_view::has_cached_places(&ui.explore) {
        log::debug!("Explore: rendering places from cache");
        explore_view::render_cached_places(&ui.explore, ui.ctx.clone());
    } else if sections.places {
        log::debug!("Explore: places cache empty, fetching from server");
        load_explore_places(ui.clone());
    }
    load_explore_sections(ui);
}

/// The fetched value, or its default after telling the user if the API key lacks permission.
pub(in crate::library) fn unwrap_or_report_permission<T: Default>(
    ui: &LibraryWindowUi,
    result: Result<T, String>,
) -> T {
    if let Err(e) = &result
        && (e.contains("HTTP 401") || e.contains(&gettext("HTTP 403")))
    {
        show_library_permission_error(&ui.window);
    }
    result.unwrap_or_default()
}

pub(in crate::library) fn load_explore_people(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(async move {
        let people = unwrap_or_report_permission(&ui, ui.ctx.api_client.fetch_people(false).await);
        let click_ui = ui.clone();
        explore_view::populate_people(&ui.explore, ui.ctx.clone(), people, move |id, name| {
            navigation::navigate_source(&click_ui, LibrarySource::Person { id, name });
        });
    });
}

pub(in crate::library) fn load_explore_places(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(async move {
        let places = unwrap_or_report_permission(&ui, ui.ctx.api_client.fetch_all_places().await);
        let click_ui = ui.clone();
        explore_view::populate_places(
            &ui.explore,
            ui.ctx.clone(),
            places,
            move |_kind, value, _asset_id| {
                navigation::navigate_source(&click_ui, LibrarySource::Place { city: value });
            },
        );
    });
}

pub(in crate::library) fn load_explore_sections(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(async move {
        let sections = unwrap_or_report_permission(&ui, ui.ctx.api_client.fetch_explore().await);
        let click_ui = ui.clone();
        explore_view::populate_explore(
            &ui.explore,
            ui.ctx.clone(),
            sections,
            move |kind, value, asset_id| {
                if kind == "recent" {
                    open_asset_in_lightbox(click_ui.clone(), asset_id);
                    return;
                }
                let source = if kind == "place" {
                    LibrarySource::Place { city: value }
                } else {
                    LibrarySource::Thing { query: value }
                };
                navigation::navigate_source(&click_ui, source);
            },
        );
    });
}

/// Fetch a single asset by ID and open it in lightbox without leaving explore.
///
/// Temporarily loads the asset into the grid model so the lightbox can display
/// it, then restores the previous library state when the lightbox page is popped.
pub(in crate::library) fn open_asset_in_lightbox(ui: Rc<LibraryWindowUi>, asset_id: String) {
    glib::MainContext::default().spawn_local(clone!(
        #[strong]
        ui,
        async move {
            let asset = match ui.ctx.api_client.fetch_asset_by_id(&asset_id).await {
                Ok(a) => a,
                Err(e) => {
                    log::warn!("Failed to fetch asset {} for lightbox: {}", asset_id, e);
                    return;
                }
            };

            // Snapshot the current state so we can restore it after lightbox.
            let prev_source = ui.ctx.library_state.lock().source.clone();

            // Temporarily load just this asset so open_lightbox can read it.
            {
                let mut state = ui.ctx.library_state.lock();
                let generation = state.generation;
                state.replace_assets_with_more(generation, vec![asset], false);
                ui.grid
                    .model
                    .reset(&ui.ctx, &state.assets, &state.sort_mode);
            }

            // Open lightbox at position 0 (the single asset).
            open_lightbox(ui.clone(), 0);

            // After the lightbox page is popped, restore the explore state.
            let restore_ui = ui.clone();
            let handler_id = Rc::new(RefCell::new(None::<glib::SignalHandlerId>));
            let handler_id_clone = handler_id.clone();
            let id = ui.nav.connect_popped(move |nav, _page| {
                let request = restore_ui
                    .ctx
                    .library_state
                    .lock()
                    .switch_source(prev_source.clone());
                load_source_page(restore_ui.clone(), request, false);
                // Disconnect this handler so it only fires once.
                if let Some(id) = handler_id_clone.borrow_mut().take() {
                    nav.disconnect(id);
                }
            });
            *handler_id.borrow_mut() = Some(id);
        }
    ));
}
