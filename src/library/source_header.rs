use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use super::LibraryWindowUi;
use super::asset_tags::color_dot;
use super::explore_view::spawn_person_thumbnail;
use super::header_format::{capitalize, count_label, count_noun};
use super::state::{LibrarySource, Location};
use super::tree_page::leaf_name;

mod enrichment;

const AVATAR_SIZE: i32 = 64;

pub(super) struct SourceHeader {
    pub root: gtk::Box,
    pub extra: gtk::Box,
    pub status: gtk::Stack,
    leading: gtk::Box,
    title: gtk::Label,
    facts: gtk::Label,
    description: gtk::Label,
    actions: gtk::Box,
    status_error: gtk::Label,
    count: RefCell<Option<String>>,
    details: RefCell<Vec<String>>,
}

enum Leading {
    Icon(&'static str),
    Person { id: String, name: String },
    TagColor(Option<String>),
}

struct HeaderTop {
    root: gtk::Box,
    leading: gtk::Box,
    title: gtk::Label,
    facts: gtk::Label,
    actions: gtk::Box,
}

pub(super) fn build_source_header() -> SourceHeader {
    let top = build_header_top();
    let description = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .visible(false)
        .build();
    let extra = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(16)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    root.append(&top.root);
    root.append(&description);
    root.append(&extra);
    let (status, status_error) = build_status_stack();
    SourceHeader {
        root,
        extra,
        status,
        leading: top.leading,
        title: top.title,
        facts: top.facts,
        description,
        actions: top.actions,
        status_error,
        count: RefCell::default(),
        details: RefCell::default(),
    }
}

fn build_header_top() -> HeaderTop {
    let leading = gtk::Box::builder().valign(gtk::Align::Center).build();
    let title = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["title-1"])
        .build();
    let facts = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .css_classes(["dim-label"])
        .visible(false)
        .build();
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    text.append(&title);
    text.append(&facts);
    let actions = gtk::Box::builder()
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    let root = gtk::Box::builder().spacing(12).build();
    root.append(&leading);
    root.append(&text);
    root.append(&actions);
    HeaderTop {
        root,
        leading,
        title,
        facts,
        actions,
    }
}

fn build_status_stack() -> (gtk::Stack, gtk::Label) {
    let error_view = super::build_status_view(
        "dialog-warning-symbolic",
        "Library data unavailable",
        "Could not load library assets",
    );
    let status_error = error_view
        .last_child()
        .and_downcast::<gtk::Label>()
        .expect("status-view subtitle label");
    let status = gtk::Stack::builder()
        .vexpand(true)
        .vhomogeneous(false)
        .build();
    status.add_named(&gtk::Box::new(gtk::Orientation::Vertical, 0), Some("none"));
    status.add_named(&super::build_loading_view(), Some("loading"));
    status.add_named(
        &super::build_status_view(
            "image-x-generic-symbolic",
            "Nothing Here",
            "There are no photos or videos to show.",
        ),
        Some("empty"),
    );
    status.add_named(&error_view, Some("error"));
    (status, status_error)
}

pub(super) fn set_status(ui: &LibraryWindowUi, status: &str, error: Option<&str>) {
    if let Some(error) = error {
        ui.header.status_error.set_label(error);
    }
    ui.header.status.set_visible_child_name(status);
    ui.header.status.set_visible(status != "none");
    ui.grid.canvas.set_visible(status == "none");
}

pub(super) fn refresh(ui: &Rc<LibraryWindowUi>) {
    if let Location::Source(source) = super::navigation::current(ui) {
        show(ui, &source);
    }
}

pub(super) fn set_loaded_count(ui: &LibraryWindowUi, count: usize) {
    *ui.header.count.borrow_mut() = Some(count_label(count as u64));
    render_facts(&ui.header);
}

pub(super) fn show(ui: &Rc<LibraryWindowUi>, source: &LibrarySource) {
    let header = &ui.header;
    *header.count.borrow_mut() = None;
    header.details.borrow_mut().clear();
    render_facts(header);
    set_description(header, None);
    clear_actions(header);
    let (leading, title) = basics(ui, source);
    set_leading(ui, leading);
    set_title(ui, &title);
    enrich(ui, source);
}

fn basics(ui: &LibraryWindowUi, source: &LibrarySource) -> (Leading, String) {
    match source {
        LibrarySource::AllAssets | LibrarySource::Unified => {
            if let Some(stats) = ui.ctx.library_state.lock().status.stats.clone() {
                *ui.header.details.borrow_mut() = vec![
                    count_noun(stats.images, "photo", "photos"),
                    count_noun(stats.videos, "video", "videos"),
                ];
            }
            (
                Leading::Icon("image-x-generic-symbolic"),
                "Photos".to_string(),
            )
        }
        LibrarySource::Person { id, name } => (
            Leading::Person {
                id: id.clone(),
                name: name.clone(),
            },
            name.clone(),
        ),
        LibrarySource::Tag { name, .. } => (Leading::TagColor(None), leaf_name(name).to_string()),
        other => {
            let (icon, title) = library_icon(other).unwrap_or_else(|| found_icon(other));
            (Leading::Icon(icon), title)
        }
    }
}

/// Icon and title for library places: local files, collections, albums, partners, folders.
fn library_icon(source: &LibrarySource) -> Option<(&'static str, String)> {
    let (icon, title) = match source {
        LibrarySource::LocalAll => ("mimick-computer-symbolic", "On This Computer"),
        LibrarySource::Explore => ("view-grid-symbolic", "Explore"),
        LibrarySource::Favorites => ("mimick-heart-filled-symbolic", "Favorites"),
        LibrarySource::Archive => ("mimick-archive-symbolic", "Archive"),
        LibrarySource::Trash => ("user-trash-symbolic", "Trash"),
        LibrarySource::Album { name, .. }
        | LibrarySource::AlbumLocal { name, .. }
        | LibrarySource::AlbumUnified { name, .. } => ("folder-pictures-symbolic", name.as_str()),
        LibrarySource::Partner { name, .. } => ("system-users-symbolic", name.as_str()),
        LibrarySource::Folder { path } => ("folder-symbolic", leaf_name(path)),
        _ => return None,
    };
    Some((icon, title.to_string()))
}

/// Icon and title for places and searches.
fn found_icon(source: &LibrarySource) -> (&'static str, String) {
    match source {
        LibrarySource::Place { city } => ("mark-location-symbolic", city.clone()),
        LibrarySource::MapArea { label, .. } => ("mark-location-symbolic", label.clone()),
        LibrarySource::Thing { query } => ("system-search-symbolic", capitalize(query)),
        LibrarySource::SmartSearch { query }
        | LibrarySource::MetadataSearch { query }
        | LibrarySource::OcrSearch { query }
        | LibrarySource::LocalSearch { query }
        | LibrarySource::UnifiedSearch { query } => ("system-search-symbolic", query.clone()),
        _ => ("system-search-symbolic", "Search Results".to_string()),
    }
}

fn enrich(ui: &Rc<LibraryWindowUi>, source: &LibrarySource) {
    enrichment::enrich(ui, source);
}

fn set_title(ui: &LibraryWindowUi, title: &str) {
    ui.header.title.set_label(title);
    ui.header.title.set_tooltip_text(Some(title));
    super::navigation::set_view_title(ui, title);
}

fn set_leading(ui: &LibraryWindowUi, leading: Leading) {
    let slot = &ui.header.leading;
    while let Some(child) = slot.first_child() {
        slot.remove(&child);
    }
    let widget: gtk::Widget = match leading {
        Leading::Icon(name) => gtk::Image::builder()
            .icon_name(name)
            .pixel_size(32)
            .css_classes(["dim-label"])
            .build()
            .upcast(),
        Leading::Person { id, name } => {
            let avatar = libadwaita::Avatar::new(AVATAR_SIZE, Some(&name), true);
            let avatar_for_image = avatar.clone();
            spawn_person_thumbnail(ui.ctx.clone(), id, move |texture| {
                avatar_for_image.set_custom_image(Some(texture))
            });
            avatar.upcast()
        }
        Leading::TagColor(color) => color_dot(color.as_deref(), 20).upcast(),
    };
    slot.append(&widget);
}

fn clear_actions(header: &SourceHeader) {
    while let Some(child) = header.actions.first_child() {
        header.actions.remove(&child);
    }
}

fn add_action(header: &SourceHeader, icon: &str, tooltip: &str, on_click: impl Fn() + 'static) {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    button.connect_clicked(move |_| on_click());
    header.actions.append(&button);
}

fn set_details(header: &SourceHeader, details: Vec<String>) {
    *header.details.borrow_mut() = details;
    render_facts(header);
}

fn set_description(header: &SourceHeader, description: Option<&str>) {
    let text = description.map(str::trim).unwrap_or_default();
    header.description.set_label(text);
    header.description.set_visible(!text.is_empty());
}

fn render_facts(header: &SourceHeader) {
    let parts: Vec<String> = header
        .count
        .borrow()
        .iter()
        .cloned()
        .chain(header.details.borrow().iter().cloned())
        .filter(|part| !part.is_empty())
        .collect();
    let text = parts.join(" · ");
    header.facts.set_label(&text);
    header.facts.set_visible(!text.is_empty());
}
