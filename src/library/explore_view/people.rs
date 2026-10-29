use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;

use crate::api_client::Person;
use crate::app_context::AppContext;

use super::ExploreViewParts;
use super::{person_tile, stop_spinner};

/// Populate the round avatar buttons for recognized people in the dashboard section.
///
/// Stores the full fetched list so face-visibility toggles can re-filter without
/// re-querying the server. Always call with `include_hidden=true` upstream so the
/// `Show hidden` toggle can apply locally.
pub fn populate_people<F>(
    parts: &ExploreViewParts,
    ctx: Arc<AppContext>,
    people: Vec<Person>,
    on_click: F,
) where
    F: Fn(String, String) + 'static,
{
    stop_spinner(&parts.people_spinner);
    *parts.cached_people.borrow_mut() = people;
    *parts.cached_people_click.borrow_mut() = Some(Rc::new(on_click));
    *parts.cached_ctx.borrow_mut() = Some(ctx.clone());
    render_people(parts, ctx);
}

/// Apply (or clear) the search filter on the people row. Pass an empty string
/// to disable filtering. Caller drives this from the header-bar search entry
/// when the Explore view is the active content stack child.
pub fn set_people_search(parts: &ExploreViewParts, query: &str) {
    *parts.search_query.borrow_mut() = query.to_string();
    let Some(ctx) = parts.cached_ctx.borrow().clone() else {
        return;
    };
    render_people(parts, ctx.clone());
    super::places::render_places_filtered(parts, ctx);
}

fn render_people(parts: &ExploreViewParts, ctx: Arc<AppContext>) {
    while let Some(child) = parts.people_row.first_child() {
        parts.people_row.remove(&child);
    }
    let (show_unnamed, show_hidden) = {
        let cfg = ctx.config.read();
        (cfg.data.show_unnamed_faces, cfg.data.show_hidden_faces)
    };
    let cached = parts.cached_people.borrow();
    let query = parts.search_query.borrow().to_ascii_lowercase();
    let filtered: Vec<&Person> = cached
        .iter()
        .filter(|p| show_hidden || !p.is_hidden)
        .filter(|p| show_unnamed || !p.name.is_empty())
        .filter(|p| query.is_empty() || p.name.to_ascii_lowercase().contains(&query))
        .collect();
    parts
        .people_section
        .set_visible(parts.sections.get().people && !filtered.is_empty());
    let on_click = parts.cached_people_click.borrow().clone();
    let Some(on_click) = on_click else {
        return;
    };
    for person in filtered.into_iter().take(40) {
        let tile = person_tile(ctx.clone(), person, on_click.clone());
        parts.people_row.append(&tile);
    }
}

/// Wire the filter MenuButton popover. Called once after the explore view is built.
/// `on_change` is invoked when a toggle flips so the caller can re-fetch with a
/// different `include_hidden` flag if needed.
pub fn wire_people_filter<F>(parts: &ExploreViewParts, ctx: Arc<AppContext>, on_change: F)
where
    F: Fn() + 'static,
{
    let (popover, unnamed_check, hidden_check) = build_people_filter_popover(&ctx);
    parts.people_filter_button.set_popover(Some(&popover));
    let on_change = Rc::new(on_change);
    connect_people_filter(
        unnamed_check,
        ctx.clone(),
        super::clone_parts_handles(parts),
        on_change.clone(),
        PeopleFilter::Unnamed,
    );
    connect_people_filter(
        hidden_check,
        ctx,
        super::clone_parts_handles(parts),
        on_change,
        PeopleFilter::Hidden,
    );
}

fn build_people_filter_popover(
    ctx: &AppContext,
) -> (gtk::Popover, gtk::CheckButton, gtk::CheckButton) {
    let popover = gtk::Popover::builder().build();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();

    let (show_unnamed, show_hidden) = {
        let cfg = ctx.config.read();
        (cfg.data.show_unnamed_faces, cfg.data.show_hidden_faces)
    };

    let unnamed_check = gtk::CheckButton::builder()
        .label("Show unnamed")
        .active(show_unnamed)
        .build();
    let hidden_check = gtk::CheckButton::builder()
        .label("Show hidden")
        .active(show_hidden)
        .build();
    body.append(&unnamed_check);
    body.append(&hidden_check);
    popover.set_child(Some(&body));
    (popover, unnamed_check, hidden_check)
}

#[derive(Clone, Copy)]
enum PeopleFilter {
    Unnamed,
    Hidden,
}

fn connect_people_filter(
    check: gtk::CheckButton,
    ctx: Arc<AppContext>,
    parts: ExploreViewParts,
    on_change: Rc<dyn Fn()>,
    filter: PeopleFilter,
) {
    check.connect_toggled(move |button| {
        let active = button.is_active();
        update_people_filter(&ctx, &parts, filter, active);
        render_people(&parts, ctx.clone());
        on_change();
    });
}

fn update_people_filter(
    ctx: &AppContext,
    parts: &ExploreViewParts,
    filter: PeopleFilter,
    active: bool,
) {
    let mut config = ctx.config.write();
    match filter {
        PeopleFilter::Unnamed => config.data.show_unnamed_faces = active,
        PeopleFilter::Hidden => {
            let hidden_count = parts
                .cached_people
                .borrow()
                .iter()
                .filter(|person| person.is_hidden)
                .count();
            log::debug!(
                "show_hidden_faces toggled to {} ({} hidden people in cache)",
                active,
                hidden_count
            );
            config.data.show_hidden_faces = active;
        }
    }
    if !config.save() {
        let name = match filter {
            PeopleFilter::Unnamed => "show_unnamed_faces",
            PeopleFilter::Hidden => "show_hidden_faces",
        };
        log::error!("Failed to save config after toggling {}", name);
    }
}
