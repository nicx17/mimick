use std::cell::RefCell;
use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;

use crate::api_client::{LibraryAsset, MetadataSearchFilters, SortOrder};
use crate::library::album_assets;
use crate::library::album_link::refresh_album_link_row;
use crate::library::content_state::{
    linked_entry_path_for_album, local_to_library_asset, merge_album_unified_page,
    merge_unified_page, reload_sidebar, show_library_permission_error, sync_content_state,
    update_footer,
};
use crate::library::local_source::{
    enumerate_local, enumerate_local_for_entry, filter_by_filename,
};
use crate::library::state::{LibrarySource, Location};
use crate::library::{
    LibraryWindowUi, MAP_AREA_PAGE_SIZE, PAGE_SIZE, archive, explore_view, folders_view,
    lightbox::open_lightbox, navigation, source_header, timeline, trash_view,
};

pub(super) fn apply_source_ui_state(ui: &LibraryWindowUi, source: &LibrarySource) {
    let is_local = matches!(
        source,
        LibrarySource::LocalAll
            | LibrarySource::LocalSearch { .. }
            | LibrarySource::AlbumLocal { .. }
    );
    let is_unified = matches!(
        source,
        LibrarySource::Unified
            | LibrarySource::UnifiedSearch { .. }
            | LibrarySource::AlbumUnified { .. }
    );
    let in_album = matches!(
        source,
        LibrarySource::Album { .. }
            | LibrarySource::AlbumLocal { .. }
            | LibrarySource::AlbumUnified { .. }
    );

    // Source-mode (Remote/Local/Unified) only relevant inside an album.
    ui.source_revealer.set_reveal_child(in_album);
    trash_view::set_trash_mode(ui, matches!(source, LibrarySource::Trash));
    archive::sync_bulk_button(&ui.bulk.archive, source);

    if in_album {
        ui.upload_button.set_tooltip_text(Some("Upload to album"));
    } else {
        ui.upload_button.set_tooltip_text(Some("Upload to library"));
    }

    // Keep source dropdown visually consistent with the active source so
    // sidebar selections don't leave it showing the wrong tab.
    let target = if is_local {
        1
    } else if is_unified {
        2
    } else {
        0
    };
    if ui.source_mode.selected() != target {
        ui.interaction.source_mode_suppressed.set(true);
        ui.source_mode.set_selected(target);
        ui.interaction.source_mode_suppressed.set(false);
    }

    refresh_album_link_row(ui, source);
    album_assets::sync_bulk_remove(ui);
}

/// Retrieve albums from the API and update the side navigation sidebar entries.
pub(super) fn load_albums(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(clone!(
        #[strong]
        ui,
        async move {
            match ui.ctx.api_client.fetch_library_albums().await {
                Ok(albums) => {
                    ui.ctx.library_state.lock().load_albums(albums);
                    reload_sidebar(&ui);
                }
                Err(err) => {
                    // Sidebar-list refresh: never hijack the user's current
                    // tab with an error page just because the albums list
                    // couldn't reload. Only surface the error view when the
                    // user is actively waiting on something.
                    log::warn!("Albums sidebar refresh failed: {}", err);
                    if ui.content_stack.visible_child_name().as_deref() == Some("loading") {
                        ui.error_label
                            .set_label(&format!("Could not load albums: {}", err));
                        ui.content_stack.set_visible_child_name("error");
                    }
                }
            }
        }
    ));
}

/// Fetch and populate active server version details and stats into the UI.
pub(super) fn load_status(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(clone!(
        #[strong]
        ui,
        async move {
            let stats = ui.ctx.api_client.fetch_server_stats().await.ok();
            let about = ui.ctx.api_client.fetch_server_about().await.ok();
            let route = ui.ctx.api_client.active_route_label().await;

            {
                let mut state = ui.ctx.library_state.lock();
                state.set_status(stats, about);
            }
            update_footer(&ui, route);
            if matches!(
                navigation::current(&ui),
                Location::Source(LibrarySource::AllAssets | LibrarySource::Unified)
            ) {
                source_header::refresh(&ui);
            }
        }
    ));
}

/// Load smart/metadata sections onto the Explore dashboard view.
///
/// Switches to the Explore stack child immediately so the user sees the
/// section headers + per-section spinners while data is in flight. Each of
/// the three fetches runs as an independent task, so a slow endpoint never
/// blocks the others — first contentful paint happens as soon as the
/// fastest section returns.
pub(super) fn load_explore_landing(ui: Rc<LibraryWindowUi>) {
    if ui.explore.populated.get() {
        log::debug!("Explore: populated=true, reusing cached widgets");
        ui.content_stack.set_visible_child_name("explore");
        return;
    }
    ui.explore.populated.set(true);
    explore_view::wire_people_filter(&ui.explore, ui.ctx.clone(), || {});
    explore_view::show_loading(&ui.explore);
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
pub(super) fn unwrap_or_report_permission<T: Default>(
    ui: &LibraryWindowUi,
    result: Result<T, String>,
) -> T {
    if let Err(e) = &result
        && (e.contains("HTTP 401") || e.contains("HTTP 403"))
    {
        show_library_permission_error(&ui.window);
    }
    result.unwrap_or_default()
}

pub(super) fn load_explore_people(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(async move {
        let people = unwrap_or_report_permission(&ui, ui.ctx.api_client.fetch_people(false).await);
        let click_ui = ui.clone();
        explore_view::populate_people(&ui.explore, ui.ctx.clone(), people, move |id, name| {
            navigation::navigate_source(&click_ui, LibrarySource::Person { id, name });
        });
    });
}

pub(super) fn load_explore_places(ui: Rc<LibraryWindowUi>) {
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

pub(super) fn load_explore_sections(ui: Rc<LibraryWindowUi>) {
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
pub(super) fn open_asset_in_lightbox(ui: Rc<LibraryWindowUi>, asset_id: String) {
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

/// Load a paginated subset of assets for the current source.
pub(super) fn load_source_page(
    ui: Rc<LibraryWindowUi>,
    request: (u64, LibrarySource, u32),
    append: bool,
) {
    if matches!(request.1, LibrarySource::Explore) {
        load_explore_landing(ui);
        return;
    }
    if !append && !navigation::on_page(&ui) {
        source_header::set_status(&ui, "loading", None);
        ui.content_stack.set_visible_child_name("grid");
    }
    if !append {
        let sort = ui.ctx.library_state.lock().sort_mode.clone();
        if timeline::eligible(&request.1, &sort) {
            timeline::open(&ui, request, None);
            return;
        }
        timeline::close(&ui);
    }
    load_pages(ui, request, append);
}

pub(super) fn load_pages(
    ui: Rc<LibraryWindowUi>,
    request: (u64, LibrarySource, u32),
    append: bool,
) {
    log::debug!("Loading library source {:?} page={}", request.1, request.2);
    glib::MainContext::default().spawn_local(clone!(
        #[strong]
        ui,
        async move {
            let (generation, source, page) = request;
            let order = ui.ctx.library_state.lock().sort_mode.server_order();
            let result = fetch_source_page(&ui, source, page, order).await;
            apply_page_result(&ui, generation, append, result);
        }
    ));
}

/// One page of assets plus whether more pages follow.
type PageResult = Result<(Vec<LibraryAsset>, bool), String>;

pub(super) async fn fetch_source_page(
    ui: &Rc<LibraryWindowUi>,
    source: LibrarySource,
    page: u32,
    order: Option<SortOrder>,
) -> PageResult {
    match source {
        LibrarySource::LocalAll
        | LibrarySource::LocalSearch { .. }
        | LibrarySource::AlbumLocal { .. } => fetch_local_page(ui, source, page, order).await,
        LibrarySource::Unified
        | LibrarySource::UnifiedSearch { .. }
        | LibrarySource::AlbumUnified { .. } => fetch_unified_page(ui, source, page, order).await,
        remote => fetch_remote_page(&ui.ctx.api_client, remote, page, order).await,
    }
}

pub(super) async fn fetch_remote_page(
    api: &crate::api_client::ImmichApiClient,
    source: LibrarySource,
    page: u32,
    order: Option<SortOrder>,
) -> PageResult {
    if let Some(filters) = remote_filters(&source, order) {
        return api
            .search_metadata_with_filters(&filters, page, PAGE_SIZE)
            .await;
    }
    match source {
        LibrarySource::Folder { path } => {
            if page > 1 {
                return Ok((Vec::new(), false));
            }
            let mut assets = api.fetch_folder_assets(&path).await?;
            folders_view::sort_by_date(&mut assets, order);
            Ok((assets, false))
        }
        LibrarySource::Album { id, .. } => {
            api.fetch_album_assets(&id, page, PAGE_SIZE, order).await
        }
        LibrarySource::SmartSearch { query } | LibrarySource::Thing { query } => {
            api.search_smart(&query, page, PAGE_SIZE).await
        }
        LibrarySource::OcrSearch { query } => api.search_ocr(&query, page, PAGE_SIZE, order).await,
        LibrarySource::Partner { id, .. } => api.fetch_user_month_page(&id, page).await,
        LibrarySource::MetadataSearch { query } => {
            api.search_metadata(&query, page, PAGE_SIZE, order).await
        }
        LibrarySource::MapArea { ids, .. } => {
            let (page_ids, more) = ids.page(page, MAP_AREA_PAGE_SIZE);
            let assets = api.fetch_assets_by_ids(page_ids).await?;
            Ok((assets, more))
        }
        other => unreachable!("{other:?} is not a remote page source"),
    }
}

pub(super) fn remote_filters(
    source: &LibrarySource,
    order: Option<SortOrder>,
) -> Option<MetadataSearchFilters> {
    let base = match source {
        LibrarySource::AllAssets => MetadataSearchFilters {
            visibility: Some("timeline".to_string()),
            ..MetadataSearchFilters::default()
        },
        LibrarySource::Archive => MetadataSearchFilters {
            visibility: Some("archive".to_string()),
            ..MetadataSearchFilters::default()
        },
        LibrarySource::Favorites => MetadataSearchFilters {
            is_favorite: Some(true),
            ..MetadataSearchFilters::default()
        },
        LibrarySource::Trash => MetadataSearchFilters::trashed(),
        LibrarySource::Person { id, .. } => MetadataSearchFilters {
            person_ids: Some(vec![id.clone()]),
            ..MetadataSearchFilters::default()
        },
        LibrarySource::Tag { id, .. } => MetadataSearchFilters {
            tag_ids: Some(vec![id.clone()]),
            ..MetadataSearchFilters::default()
        },
        LibrarySource::Place { city } => MetadataSearchFilters {
            city: Some(city.clone()),
            ..MetadataSearchFilters::default()
        },
        LibrarySource::AdvancedSearch { filters } => (**filters).clone(),
        _ => return None,
    };
    Some(MetadataSearchFilters { order, ..base })
}

/// Local enumeration is bounded, so local sources return everything as one page.
pub(super) async fn fetch_local_page(
    ui: &Rc<LibraryWindowUi>,
    source: LibrarySource,
    page: u32,
    order: Option<SortOrder>,
) -> PageResult {
    if page > 1 {
        return Ok((Vec::new(), false));
    }
    let locals = match source {
        LibrarySource::LocalAll => enumerate_local(ui.ctx.clone()).await,
        LibrarySource::LocalSearch { query } => {
            filter_by_filename(enumerate_local(ui.ctx.clone()).await, &query)
        }
        LibrarySource::AlbumLocal { name, .. } => match linked_entry_path_for_album(ui, &name) {
            Some(path) => enumerate_local_for_entry(ui.ctx.clone(), path).await,
            None => Vec::new(),
        },
        other => unreachable!("{other:?} is not a local page source"),
    };
    let mut assets: Vec<LibraryAsset> = locals.into_iter().map(local_to_library_asset).collect();
    if order == Some(SortOrder::Asc) {
        assets.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    }
    Ok((assets, false))
}

/// Remote assets overlayed with local sync state.
pub(super) async fn fetch_unified_page(
    ui: &Rc<LibraryWindowUi>,
    source: LibrarySource,
    page: u32,
    order: Option<SortOrder>,
) -> PageResult {
    let api = &ui.ctx.api_client;
    match source {
        LibrarySource::Unified => {
            let remote = api
                .search_metadata_with_filters(
                    &remote_filters(&LibrarySource::AllAssets, order).unwrap_or_default(),
                    page,
                    PAGE_SIZE,
                )
                .await;
            merge_unified_page(remote, page, ui, None).await
        }
        LibrarySource::UnifiedSearch { query } => {
            let remote = api.search_metadata(&query, page, PAGE_SIZE, order).await;
            merge_unified_page(remote, page, ui, Some(&query)).await
        }
        LibrarySource::AlbumUnified { id, name } => {
            let remote = api.fetch_album_assets(&id, page, PAGE_SIZE, order).await;
            merge_album_unified_page(remote, page, ui, &name).await
        }
        other => unreachable!("{other:?} is not a unified page source"),
    }
}

pub(super) fn apply_page_result(
    ui: &Rc<LibraryWindowUi>,
    generation: u64,
    append: bool,
    result: PageResult,
) {
    let (items, has_more) = match result {
        Ok(page) => page,
        Err(err) => {
            let current = {
                let mut state = ui.ctx.library_state.lock();
                state.mark_error(generation, err.clone());
                state.generation == generation
            };
            if current && !append && !navigation::on_page(ui) {
                let message = format!("Could not load library assets: {err}");
                source_header::set_status(ui, "error", Some(&message));
                ui.content_stack.set_visible_child_name("grid");
            }
            return;
        }
    };
    {
        let mut state = ui.ctx.library_state.lock();
        let applied = if append {
            state.append_assets_with_more(generation, items, has_more)
        } else {
            state.replace_assets_with_more(generation, items, has_more)
        };
        if !applied {
            return;
        }
        if append {
            ui.grid
                .model
                .extend(&ui.ctx, &state.assets, &state.sort_mode);
        } else {
            ui.grid
                .model
                .reset(&ui.ctx, &state.assets, &state.sort_mode);
        }
    }
    // Lock is released before touching GTK widgets so that signal handlers
    // triggered by the stack transition can safely re-acquire library_state.
    sync_content_state(ui);
    reload_sidebar(ui);
}
