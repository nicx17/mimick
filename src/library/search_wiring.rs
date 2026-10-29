use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;

use crate::api_client::MetadataSearchFilters;
use crate::library::state::LibrarySource;

use super::{LibraryWindowUi, albums_view, explore_view, navigation, search_view};

pub(super) fn connect_lightweight_filters(ui: Rc<LibraryWindowUi>) {
    // Albums: live-filter tiles as the user types.
    ui.albums.filter_entry.connect_search_changed(clone!(
        #[strong]
        ui,
        move |entry| {
            let query = entry.text().to_string();
            albums_view::set_search_filter(&ui.albums, &query);
        }
    ));

    // Explore: live-filter people tiles as the user types.
    ui.explore.filter_entry.connect_search_changed(clone!(
        #[strong]
        ui,
        move |entry| {
            let query = entry.text().to_string();
            explore_view::set_people_search(&ui.explore, &query);
        }
    ));
}

/// Wire the dedicated search view's interactive handlers.
pub(super) fn connect_search_view(ui: Rc<LibraryWindowUi>) {
    connect_filter_panel_actions(ui.clone());
    connect_search_submit(&ui);
}

fn connect_filter_panel_actions(ui: Rc<LibraryWindowUi>) {
    // Clear button resets all fields.
    ui.search_view.clear_button.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| {
            search_view::clear_all_filters(&ui.search_view);
        }
    ));

    ui.search_view.panel_clear.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| search_view::clear_filter_rows(&ui.search_view)
    ));
    ui.search_view.panel_search.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| {
            if ui.filters_split.is_collapsed() {
                ui.search_view.filters_toggle.set_active(false);
            }
            dispatch_search_from_view(&ui);
        }
    ));
    ui.search_view.filters_toggle.connect_toggled(clone!(
        #[strong]
        ui,
        move |toggle| {
            if !toggle.is_active() {
                search_view::refresh_filter_count(&ui.search_view);
            }
        }
    ));
}

fn connect_search_submit(ui: &Rc<LibraryWindowUi>) {
    // Search button (or Enter in the entry) dispatches the search.
    let do_search = {
        let ui = ui.clone();
        move || {
            dispatch_search_from_view(&ui);
        }
    };

    let do_search_btn = do_search.clone();
    ui.search_view.search_button.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| {
            let _ = &ui;
            do_search_btn();
        }
    ));

    ui.search_view.search_entry.connect_activate(clone!(
        #[strong]
        ui,
        move |_| {
            let _ = &ui;
            do_search();
        }
    ));
}

/// Read the search view widgets and dispatch the appropriate search source.
fn dispatch_search_from_view(ui: &Rc<LibraryWindowUi>) {
    search_view::refresh_filter_count(&ui.search_view);
    let query = ui.search_view.search_entry.text().trim().to_string();
    let filters = search_view::collect_filters(&ui.search_view);
    let has_filters = filters != MetadataSearchFilters::default();
    let mode = search_view::selected_mode(&ui.search_view); // 0=Smart, 1=Filename, 2=OCR

    let source = build_search_source(mode, &query, has_filters, filters);
    let Some(source) = source else {
        return;
    };

    navigation::navigate_source(ui, source);
}

/// Decide which `LibrarySource` variant to use based on mode, query, and filters.
fn build_search_source(
    mode: u32,
    query: &str,
    has_filters: bool,
    filters: MetadataSearchFilters,
) -> Option<LibrarySource> {
    if query.is_empty() && !has_filters {
        return None;
    }

    match mode {
        // Smart Search
        0 => {
            if query.is_empty() {
                // Filters only -> metadata search
                Some(LibrarySource::AdvancedSearch {
                    filters: Box::new(filters),
                })
            } else {
                Some(LibrarySource::SmartSearch {
                    query: query.to_string(),
                })
            }
        }
        // Filename
        1 => {
            let mut f = filters;
            if !query.is_empty() {
                f.original_file_name = Some(query.to_string());
            }
            Some(LibrarySource::AdvancedSearch {
                filters: Box::new(f),
            })
        }
        // OCR
        2 => {
            let mut f = filters;
            if !query.is_empty() {
                f.ocr = Some(query.to_string());
            }
            Some(LibrarySource::AdvancedSearch {
                filters: Box::new(f),
            })
        }
        _ => None,
    }
}
