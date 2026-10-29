use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;

use crate::api_client::PlaceItem;
use crate::app_context::AppContext;

use super::tiles::explore_tile;
use super::{
    ExploreViewParts, INITIAL_TILE_COUNT, append_see_more_button, append_show_less_button,
    clone_parts_handles, stop_spinner,
};

pub(crate) fn render_places_filtered(parts: &ExploreViewParts, ctx: Arc<AppContext>) {
    while let Some(child) = parts.places_grid.first_child() {
        parts.places_grid.remove(&child);
    }
    let places = parts.cached_places.borrow();
    let query = parts.search_query.borrow().to_ascii_lowercase();
    let filtered: Vec<&PlaceItem> = if query.is_empty() {
        places.iter().collect()
    } else {
        places
            .iter()
            .filter(|p| p.city.to_ascii_lowercase().contains(&query))
            .collect()
    };
    parts
        .places_section
        .set_visible(parts.sections.get().places && !filtered.is_empty());
    let on_click = match parts.cached_places_click.borrow().clone() {
        Some(cb) => cb,
        None => return,
    };
    for place in filtered.into_iter().take(INITIAL_TILE_COUNT) {
        let tile = explore_tile(
            ctx.clone(),
            "place",
            &place.city,
            &place.asset_id,
            on_click.clone(),
        );
        parts.places_grid.append(&tile);
    }
}

/// Populate the city tiles representing locations in the places dashboard section.
///
/// Caches places data so subsequent visits don't re-fetch from the server.
/// Shows the first 16 tiles with a "See More" button to expand.
pub fn populate_places<F>(
    parts: &ExploreViewParts,
    ctx: Arc<AppContext>,
    places: Vec<PlaceItem>,
    on_click: F,
) where
    F: Fn(&str, String, String) + 'static,
{
    stop_spinner(&parts.places_spinner);
    *parts.cached_places.borrow_mut() = places;
    *parts.cached_places_click.borrow_mut() = Some(Rc::new(on_click));
    render_places(parts, ctx, false);
}

/// Check if places are already cached and render them without fetching.
pub fn has_cached_places(parts: &ExploreViewParts) -> bool {
    !parts.cached_places.borrow().is_empty()
}

/// Re-render places from cache (used when navigating back to explore).
pub fn render_cached_places(parts: &ExploreViewParts, ctx: Arc<AppContext>) {
    stop_spinner(&parts.places_spinner);
    render_places(parts, ctx, false);
}

fn render_places(parts: &ExploreViewParts, ctx: Arc<AppContext>, expanded: bool) {
    while let Some(child) = parts.places_grid.first_child() {
        parts.places_grid.remove(&child);
    }
    let places = parts.cached_places.borrow();
    parts
        .places_section
        .set_visible(parts.sections.get().places && !places.is_empty());
    let on_click = match parts.cached_places_click.borrow().clone() {
        Some(cb) => cb,
        None => return,
    };
    let limit = if expanded {
        places.len()
    } else {
        INITIAL_TILE_COUNT
    };
    for place in places.iter().take(limit) {
        let tile = explore_tile(
            ctx.clone(),
            "place",
            &place.city,
            &place.asset_id,
            on_click.clone(),
        );
        parts.places_grid.append(&tile);
    }
    if !expanded && places.len() > INITIAL_TILE_COUNT {
        let remaining = places.len() - INITIAL_TILE_COUNT;
        drop(places);
        append_see_more_button(&parts.places_grid, remaining, {
            let parts = clone_parts_handles(parts);
            let ctx = ctx.clone();
            move || render_places(&parts, ctx.clone(), true)
        });
    } else if expanded && places.len() > INITIAL_TILE_COUNT {
        drop(places);
        append_show_less_button(&parts.places_grid, {
            let parts = clone_parts_handles(parts);
            let ctx = ctx.clone();
            move || render_places(&parts, ctx.clone(), false)
        });
    }
}
