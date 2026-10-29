//! Sectioned landing page mirroring Immich web's Explore tab.
//!
//! Four rows: People (round avatars), Recently Added (date tiles),
//! Places (city tiles), Things (tag tiles). Tile clicks invoke
//! caller-provided closures so dispatch lives in `mod.rs`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;

use crate::api_client::{ExploreSection, Person, PlaceItem};
use crate::app_context::AppContext;

type ExploreClick = Rc<dyn Fn(&str, String, String)>;
type PersonClick = Rc<dyn Fn(String, String)>;

const INITIAL_TILE_COUNT: usize = 16;
const RECENTS_EXPANDED_COUNT: usize = 30;

mod people;
mod places;
mod tiles;
pub use people::{populate_people, set_people_search, wire_people_filter};
pub use places::{has_cached_places, populate_places, render_cached_places};
use tiles::{explore_tile, format_relative_date};
pub(super) use tiles::{person_tile, spawn_person_thumbnail};

/// Contains references to individual grid widgets of the explore tab dashboard display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExploreSections {
    pub people: bool,
    pub places: bool,
    pub things: bool,
}

impl Default for ExploreSections {
    fn default() -> Self {
        Self {
            people: true,
            places: true,
            things: true,
        }
    }
}

pub struct ExploreViewParts {
    pub root: gtk::ScrolledWindow,
    pub sections: Rc<Cell<ExploreSections>>,
    pub populated: Rc<Cell<bool>>,
    people_row: gtk::Box,
    recents_grid: gtk::FlowBox,
    places_grid: gtk::FlowBox,
    things_grid: gtk::FlowBox,
    people_section: gtk::Box,
    recents_section: gtk::Box,
    places_section: gtk::Box,
    things_section: gtk::Box,
    people_spinner: gtk::Spinner,
    recents_spinner: gtk::Spinner,
    places_spinner: gtk::Spinner,
    things_spinner: gtk::Spinner,
    pub people_filter_button: gtk::MenuButton,
    cached_people: Rc<RefCell<Vec<Person>>>,
    cached_people_click: Rc<RefCell<Option<PersonClick>>>,
    cached_places: Rc<RefCell<Vec<PlaceItem>>>,
    cached_places_click: Rc<RefCell<Option<ExploreClick>>>,
    pub search_query: Rc<RefCell<String>>,
    pub filter_entry: gtk::SearchEntry,
    cached_ctx: Rc<RefCell<Option<Arc<AppContext>>>>,
}

/// Construct the hierarchical panels and containers for the explore dashboard view.
pub fn build_explore_view() -> ExploreViewParts {
    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    let widgets = build_sections(&outer);
    let root = gtk::ScrolledWindow::builder()
        .child(&outer)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .build();
    ExploreViewParts {
        root,
        populated: Rc::new(Cell::new(false)),
        people_row: widgets.people_row,
        recents_grid: widgets.recents_grid,
        places_grid: widgets.places_grid,
        things_grid: widgets.things_grid,
        people_section: widgets.people_section,
        recents_section: widgets.recents_section,
        places_section: widgets.places_section,
        things_section: widgets.things_section,
        people_spinner: widgets.people_spinner,
        recents_spinner: widgets.recents_spinner,
        places_spinner: widgets.places_spinner,
        things_spinner: widgets.things_spinner,
        people_filter_button: widgets.people_filter_button,
        cached_people: Rc::new(RefCell::new(Vec::new())),
        cached_people_click: Rc::new(RefCell::new(None)),
        cached_places: Rc::new(RefCell::new(Vec::new())),
        cached_places_click: Rc::new(RefCell::new(None)),
        search_query: Rc::new(RefCell::new(String::new())),
        filter_entry: widgets.filter_entry,
        cached_ctx: Rc::new(RefCell::new(None)),
        sections: Rc::default(),
    }
}

struct ExploreSectionWidgets {
    people_section: gtk::Box,
    people_row: gtk::Box,
    people_spinner: gtk::Spinner,
    people_filter_button: gtk::MenuButton,
    recents_section: gtk::Box,
    recents_grid: gtk::FlowBox,
    recents_spinner: gtk::Spinner,
    places_section: gtk::Box,
    places_grid: gtk::FlowBox,
    places_spinner: gtk::Spinner,
    things_section: gtk::Box,
    things_grid: gtk::FlowBox,
    things_spinner: gtk::Spinner,
    filter_entry: gtk::SearchEntry,
}

fn build_sections(outer: &gtk::Box) -> ExploreSectionWidgets {
    let (people_section, people_row, people_spinner, people_filter_button) = build_people_section();
    let (recents_section, recents_grid, recents_spinner) = build_tile_section("Recently Added");
    let (places_section, places_grid, places_spinner) = build_tile_section("Places");
    let (things_section, things_grid, things_spinner) = build_tile_section("Things");

    let filter_entry = gtk::SearchEntry::builder()
        .placeholder_text("Filter people and places")
        .hexpand(true)
        .max_width_chars(20)
        .build();
    outer.append(&filter_entry);

    outer.append(&people_section);
    outer.append(&places_section);
    outer.append(&recents_section);
    outer.append(&things_section);
    ExploreSectionWidgets {
        people_row,
        people_section,
        people_filter_button,
        people_spinner,
        recents_section,
        recents_grid,
        recents_spinner,
        places_section,
        places_grid,
        places_spinner,
        things_section,
        things_grid,
        things_spinner,
        filter_entry,
    }
}

/// Reveal each section with its spinner active, so the user gets immediate
/// visual feedback that data is on the way. Each `populate_*` call clears
/// its own spinner when results arrive.
pub fn show_loading(parts: &ExploreViewParts) {
    let enabled = parts.sections.get();
    for (section, spinner, shown) in [
        (&parts.people_section, &parts.people_spinner, enabled.people),
        (&parts.recents_section, &parts.recents_spinner, true),
        (&parts.places_section, &parts.places_spinner, enabled.places),
        (&parts.things_section, &parts.things_spinner, enabled.things),
    ] {
        if !shown {
            section.set_visible(false);
            continue;
        }
        section.set_visible(true);
        spinner.set_visible(true);
        spinner.start();
    }
}

fn stop_spinner(spinner: &gtk::Spinner) {
    spinner.stop();
    spinner.set_visible(false);
}

/// Build a horizontal scrolled gallery row dedicated to recognized people circles.
fn build_people_section() -> (gtk::Box, gtk::Box, gtk::Spinner, gtk::MenuButton) {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .visible(false)
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let title = heading("People");
    title.set_hexpand(true);
    header.append(&title);
    let spinner = gtk::Spinner::builder()
        .visible(false)
        .valign(gtk::Align::Center)
        .build();
    header.append(&spinner);
    let filter_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("Filter people")
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    header.append(&filter_button);
    section.append(&header);

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .child(&row)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .height_request(140)
        .build();
    section.append(&scroller);
    (section, row, spinner, filter_button)
}

/// Build a flow grid section mapping image tiles for Places or Things category.
fn build_tile_section(title: &str) -> (gtk::Box, gtk::FlowBox, gtk::Spinner) {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .visible(false)
        .build();
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let title_label = heading(title);
    title_label.set_hexpand(true);
    header.append(&title_label);
    let spinner = gtk::Spinner::builder()
        .visible(false)
        .valign(gtk::Align::Center)
        .build();
    header.append(&spinner);
    section.append(&header);
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .row_spacing(8)
        .column_spacing(8)
        .min_children_per_line(2)
        .max_children_per_line(20)
        .homogeneous(true)
        .halign(gtk::Align::Start)
        .build();
    section.append(&grid);
    (section, grid, spinner)
}

/// Helper to create a styled section heading label.
fn heading(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(vec!["title-2".to_string()])
        .build()
}

/// Re-render the places section applying the current search query.
/// Build a lightweight `ExploreViewParts` snapshot containing only the widget/handle
/// references render_people needs, sharing the same `Rc` data with the original.
fn clone_parts_handles(parts: &ExploreViewParts) -> ExploreViewParts {
    ExploreViewParts {
        root: parts.root.clone(),
        populated: parts.populated.clone(),
        people_row: parts.people_row.clone(),
        recents_grid: parts.recents_grid.clone(),
        places_grid: parts.places_grid.clone(),
        things_grid: parts.things_grid.clone(),
        people_section: parts.people_section.clone(),
        recents_section: parts.recents_section.clone(),
        places_section: parts.places_section.clone(),
        things_section: parts.things_section.clone(),
        people_spinner: parts.people_spinner.clone(),
        recents_spinner: parts.recents_spinner.clone(),
        places_spinner: parts.places_spinner.clone(),
        things_spinner: parts.things_spinner.clone(),
        people_filter_button: parts.people_filter_button.clone(),
        cached_people: parts.cached_people.clone(),
        cached_people_click: parts.cached_people_click.clone(),
        cached_places: parts.cached_places.clone(),
        cached_places_click: parts.cached_places_click.clone(),
        search_query: parts.search_query.clone(),
        filter_entry: parts.filter_entry.clone(),
        cached_ctx: parts.cached_ctx.clone(),
        sections: parts.sections.clone(),
    }
}

/// Populate explore sections: Things tiles + Recently Added tiles.
///
/// Sections by `field_name`:
///   - `exifInfo.city`          -- skipped (populated by `populate_places`)
///   - `createdAt`              -- rendered as "Recently Added" tiles
///   - `smartInfo.objects/tags` -- rendered as "Things" tiles
pub fn populate_explore<F>(
    parts: &ExploreViewParts,
    ctx: Arc<AppContext>,
    sections: Vec<ExploreSection>,
    on_click: F,
) where
    F: Fn(&str, String, String) + 'static,
{
    stop_spinner(&parts.things_spinner);
    stop_spinner(&parts.recents_spinner);
    clear_grid(&parts.things_grid);
    clear_grid(&parts.recents_grid);
    log_explore_sections(&sections);
    let on_click: ExploreClick = Rc::new(on_click);
    let (had_things, had_recents) = render_explore_sections(parts, &ctx, sections, &on_click);

    parts
        .things_section
        .set_visible(parts.sections.get().things && had_things);
    parts.recents_section.set_visible(had_recents);
}

fn clear_grid(grid: &gtk::FlowBox) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }
}

fn log_explore_sections(sections: &[ExploreSection]) {
    log::debug!(
        "Explore sections: [{}]",
        sections
            .iter()
            .map(|section| format!("{}({})", section.field_name, section.items.len()))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn render_explore_sections(
    parts: &ExploreViewParts,
    ctx: &Arc<AppContext>,
    sections: Vec<ExploreSection>,
    on_click: &ExploreClick,
) -> (bool, bool) {
    let mut had_things = false;
    let mut had_recents = false;
    for section in sections {
        match render_explore_section(parts, ctx, section, on_click) {
            SectionKind::Things => had_things = true,
            SectionKind::Recents => had_recents = true,
            SectionKind::Ignored => {}
        }
    }
    (had_things, had_recents)
}

enum SectionKind {
    Things,
    Recents,
    Ignored,
}

fn render_explore_section(
    parts: &ExploreViewParts,
    ctx: &Arc<AppContext>,
    section: ExploreSection,
    on_click: &ExploreClick,
) -> SectionKind {
    if section.field_name.contains("city") {
        return SectionKind::Ignored;
    }
    if section.field_name == "createdAt" || section.field_name == "updatedAt" {
        render_recent_section(parts, ctx, section.items, on_click);
        return SectionKind::Recents;
    }
    if !section.field_name.starts_with("smartInfo") {
        log::debug!("Skipping unknown section '{}'", section.field_name);
        return SectionKind::Ignored;
    }
    for item in section.items.into_iter().take(24) {
        let tile = explore_tile(
            ctx.clone(),
            "thing",
            &item.value,
            &item.data.id,
            on_click.clone(),
        );
        parts.things_grid.append(&tile);
    }
    SectionKind::Things
}

fn render_recent_section(
    parts: &ExploreViewParts,
    ctx: &Arc<AppContext>,
    mut items: Vec<crate::api_client::ExploreItem>,
    on_click: &ExploreClick,
) {
    items.sort_by(|a, b| b.value.cmp(&a.value));
    render_recents_tiles(parts, ctx, &items, on_click, false);
    if items.len() > INITIAL_TILE_COUNT {
        let remaining = items.len().min(RECENTS_EXPANDED_COUNT) - INITIAL_TILE_COUNT;
        let parts = clone_parts_handles(parts);
        let ctx = ctx.clone();
        let on_click = on_click.clone();
        let recents_grid = parts.recents_grid.clone();
        append_see_more_button(&recents_grid, remaining, move || {
            render_recents_tiles(&parts, &ctx, &items, &on_click, true);
        });
    }
}

/// Render recently-added tiles into the recents grid.
fn render_recents_tiles(
    parts: &ExploreViewParts,
    ctx: &Arc<AppContext>,
    items: &[crate::api_client::ExploreItem],
    on_click: &ExploreClick,
    expanded: bool,
) {
    while let Some(child) = parts.recents_grid.first_child() {
        parts.recents_grid.remove(&child);
    }
    let limit = if expanded {
        RECENTS_EXPANDED_COUNT
    } else {
        INITIAL_TILE_COUNT
    };
    for item in items.iter().take(limit) {
        let label = format_relative_date(&item.value);
        let tile = explore_tile(
            ctx.clone(),
            "recent",
            &label,
            &item.data.id,
            on_click.clone(),
        );
        parts.recents_grid.append(&tile);
    }
}

fn append_action_button<F: Fn() + 'static>(
    grid: &gtk::FlowBox,
    icon_name: &str,
    label_text: &str,
    on_click: F,
) {
    let icon = gtk::Image::builder()
        .icon_name(icon_name)
        .pixel_size(24)
        .halign(gtk::Align::Center)
        .build();
    let label = gtk::Label::builder()
        .label(label_text)
        .css_classes(["caption-heading"])
        .halign(gtk::Align::Center)
        .build();
    // Spacer forces the same min-height as explore tiles.
    let spacer = gtk::Box::builder()
        .css_classes(["mimick-explore-spacer"])
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    content.append(&icon);
    content.append(&label);
    // Overlay the centered label on top of the spacer so the button
    // has the same footprint as a regular tile.
    let overlay = gtk::Overlay::builder()
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["mimick-see-more-tile"])
        .build();
    overlay.set_child(Some(&spacer));
    overlay.add_overlay(&content);

    let btn = gtk::Button::builder()
        .child(&overlay)
        .css_classes(["flat"])
        .build();
    let grid_ref = grid.clone();
    btn.connect_clicked(move |button| {
        if let Some(parent) = button.parent() {
            grid_ref.remove(&parent);
        }
        on_click();
    });
    grid.append(&btn);
}

/// Append a card-sized "See More" tile to a FlowBox grid.
///
/// Matches the dimensions of adjacent explore tiles so the button fills a
/// full card slot rather than appearing as a small inline text link.
fn append_see_more_button<F: Fn() + 'static>(grid: &gtk::FlowBox, remaining: usize, on_expand: F) {
    append_action_button(
        grid,
        "view-more-symbolic",
        &format!("See {remaining} more"),
        on_expand,
    );
}

/// Append a card-sized "Show Less" tile to collapse an expanded section.
fn append_show_less_button<F: Fn() + 'static>(grid: &gtk::FlowBox, on_collapse: F) {
    append_action_button(grid, "go-up-symbolic", "Show Less", on_collapse);
}
