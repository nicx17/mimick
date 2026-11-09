//! Albums landing page with Recent / Owned / Shared sections.
//!
//! Fetches album cover thumbnails and renders them as clickable tiles
//! in a responsive flow layout. Selecting a tile navigates the main
//! grid to that album's contents.

use crate::i18n::gettext_noop;
use gettextrs::gettext;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;

use crate::api_client::LibraryAlbum;
use crate::app_context::AppContext;

use super::context_menu::{Menu, connect_context_menu, menu_key};

mod stats;
mod tile;
pub(super) use stats::refresh_stats;
pub(super) use tile::spawn_thumbnail;

pub type AlbumClick = Rc<dyn Fn(&str, String)>;
/// Context-menu actions on album tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumAction {
    /// Manage which users the album is shared with.
    Share,
    /// Create a public link to the album.
    CreateLink,
    Rename,
    EditDescription,
    /// Stop being a member of an album someone else shared.
    Leave,
    /// Save the whole album as zip files.
    Download,
    /// Delete one of the user's own albums; its photos stay in the library.
    Delete,
}

/// Each action's icon in the tile menu.
const ACTION_ICONS: [(AlbumAction, &str); 7] = [
    (AlbumAction::Share, "mimick-share-symbolic"),
    (AlbumAction::CreateLink, "insert-link-symbolic"),
    (AlbumAction::Rename, "document-edit-symbolic"),
    (AlbumAction::EditDescription, "format-justify-left-symbolic"),
    (AlbumAction::Leave, "system-log-out-symbolic"),
    (AlbumAction::Download, "mimick-download-symbolic"),
    (AlbumAction::Delete, "user-trash-symbolic"),
];

impl AlbumAction {
    fn icon(self) -> &'static str {
        ACTION_ICONS
            .iter()
            .find(|(action, _)| *action == self)
            .map_or("view-more-symbolic", |(_, icon)| icon)
    }
}

const OWN_ALBUM_ACTIONS: &[(&str, AlbumAction)] = &[
    (gettext_noop("Share…"), AlbumAction::Share),
    (gettext_noop("Create Share Link…"), AlbumAction::CreateLink),
    (gettext_noop("Rename…"), AlbumAction::Rename),
    (
        gettext_noop("Edit Description…"),
        AlbumAction::EditDescription,
    ),
    (gettext_noop("Download"), AlbumAction::Download),
    (gettext_noop("Delete Album…"), AlbumAction::Delete),
];

const SHARED_ALBUM_ACTIONS: &[(&str, AlbumAction)] = &[
    (gettext_noop("Download"), AlbumAction::Download),
    (gettext_noop("Leave Album…"), AlbumAction::Leave),
];

type TileMenu = (AlbumShare, &'static [(&'static str, AlbumAction)]);

/// Runs a context-menu action on an album tile.
pub type AlbumShare = Rc<dyn Fn(LibraryAlbum, AlbumAction)>;

/// Sort modes available for the albums landing page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlbumsSort {
    #[default]
    Newest,
    Name,
    MostAssets,
}

/// Contains references to individual grid widgets of the albums overview display.
pub struct AlbumsViewParts {
    pub root: gtk::ScrolledWindow,
    pub populated: Rc<Cell<bool>>,
    pub create_button: gtk::Button,
    /// Owned, shared, and not-shared album counts, above the filter.
    stats: gtk::Label,
    owned_title: gtk::Label,
    /// Shared Albums mode: only "Shared by you" and "Shared with you".
    shared_only: Rc<Cell<bool>>,
    cached_share: Rc<RefCell<Option<AlbumShare>>>,
    recent_grid: gtk::FlowBox,
    owned_grid: gtk::FlowBox,
    shared_grid: gtk::FlowBox,
    recent_section: gtk::Box,
    owned_section: gtk::Box,
    shared_section: gtk::Box,
    /// Shown on the Shared Albums page when there is nothing to list.
    empty: libadwaita::StatusPage,
    cached_albums: Rc<RefCell<Vec<LibraryAlbum>>>,
    cached_click: Rc<RefCell<Option<AlbumClick>>>,
    pub search_query: Rc<RefCell<String>>,
    pub filter_entry: gtk::SearchEntry,
    pub sort_mode: Rc<Cell<AlbumsSort>>,
    cached_ctx: Rc<RefCell<Option<Arc<AppContext>>>>,
}

/// Construct the hierarchical panels and containers for the albums listing page.
pub fn build_albums_view() -> AlbumsViewParts {
    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    let (header_row, filter_entry, create_button) = build_header();
    let stats = stats::label();
    header_row.prepend(&stats);
    outer.append(&header_row);

    let (recent_section, _, recent_grid) = build_section(&gettext("Recent"));
    let (owned_section, owned_title, owned_grid) = build_section(&gettext("Your albums"));
    let (shared_section, _, shared_grid) = build_section(&gettext("Shared with you"));
    outer.append(&recent_section);
    outer.append(&owned_section);
    outer.append(&shared_section);
    let empty = build_empty_state();
    outer.append(&empty);

    let root = gtk::ScrolledWindow::builder()
        .child(&outer)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    AlbumsViewParts {
        root,
        populated: Rc::new(Cell::new(false)),
        create_button,
        stats,
        owned_title,
        shared_only: Rc::default(),
        cached_share: Rc::default(),
        recent_grid,
        owned_grid,
        shared_grid,
        recent_section,
        owned_section,
        shared_section,
        empty,
        cached_albums: Rc::new(RefCell::new(Vec::new())),
        cached_click: Rc::new(RefCell::new(None)),
        search_query: Rc::new(RefCell::new(String::new())),
        filter_entry,
        sort_mode: Rc::new(Cell::new(AlbumsSort::default())),
        cached_ctx: Rc::new(RefCell::new(None)),
    }
}

/// The filter entry and "Create album"; the page's name is only in the title bar.
fn build_header() -> (gtk::Box, gtk::SearchEntry, gtk::Button) {
    let header_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    let create_button = gtk::Button::builder()
        .label(gettext("Create album"))
        .css_classes(vec!["suggested-action".to_string()])
        .build();
    let filter_entry = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Filter albums"))
        .hexpand(true)
        .max_width_chars(20)
        .build();
    let filter_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    filter_row.append(&filter_entry);
    filter_row.append(&create_button);
    header_row.append(&filter_row);
    (header_row, filter_entry, create_button)
}

/// Populate the album list grids grouped by ownership status.
pub fn populate_albums(
    parts: &AlbumsViewParts,
    ctx: Arc<AppContext>,
    albums: Vec<LibraryAlbum>,
    on_click: AlbumClick,
) {
    *parts.cached_albums.borrow_mut() = albums;
    *parts.cached_click.borrow_mut() = Some(on_click);
    *parts.cached_ctx.borrow_mut() = Some(ctx);
    render_albums(parts);
}

/// Switch between the full Albums page and the Shared Albums page, then re-render.
pub fn set_shared_only(parts: &AlbumsViewParts, shared_only: bool) {
    parts.shared_only.set(shared_only);
    parts.owned_title.set_label(&if shared_only {
        gettext("Shared by you")
    } else {
        gettext("Your albums")
    });
    parts.create_button.set_visible(!shared_only);
    render_albums(parts);
}

/// Handler for "Share…" on the user's own album tiles.
pub fn set_share_handler(parts: &AlbumsViewParts, share: AlbumShare) {
    *parts.cached_share.borrow_mut() = Some(share);
}

/// The user's albums that have at least one other member.
fn shared_by_you(owned: Vec<&LibraryAlbum>) -> Vec<&LibraryAlbum> {
    owned.into_iter().filter(|a| a.is_shared()).collect()
}

/// Set the current search filter and re-render. Empty string clears the filter.
pub fn set_search_filter(parts: &AlbumsViewParts, query: &str) {
    *parts.search_query.borrow_mut() = query.to_string();
    render_albums(parts);
}

/// Set the current sort mode and re-render.
pub fn set_sort_mode(parts: &AlbumsViewParts, mode: AlbumsSort) {
    parts.sort_mode.set(mode);
    render_albums(parts);
}

/// Pure projection over the cached album list. Returns the (recent, owned,
/// shared) buckets that drive the three Albums-view rows after applying the
/// active search filter and sort mode. Extracted from `render_albums` so the
/// ordering rules can be unit-tested without GTK.
pub(crate) fn project_albums<'a>(
    albums: &'a [LibraryAlbum],
    current_user: &str,
    query: &str,
    sort: AlbumsSort,
) -> (
    Vec<&'a LibraryAlbum>,
    Vec<&'a LibraryAlbum>,
    Vec<&'a LibraryAlbum>,
) {
    let query = query.to_ascii_lowercase();
    let filtered: Vec<&LibraryAlbum> = albums
        .iter()
        .filter(|a| query.is_empty() || a.album_name.to_ascii_lowercase().contains(&query))
        .collect();

    let mut sorted: Vec<&LibraryAlbum> = filtered.clone();
    match sort {
        AlbumsSort::Newest => sorted.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
        AlbumsSort::Name => sorted.sort_by(|a, b| {
            a.album_name
                .to_ascii_lowercase()
                .cmp(&b.album_name.to_ascii_lowercase())
        }),
        AlbumsSort::MostAssets => sorted.sort_by_key(|a| std::cmp::Reverse(a.asset_count)),
    }

    // The "Recent" bucket always reflects creation order, even when the user
    // picks a different sort for the main owned/shared rows.
    let recent: Vec<&LibraryAlbum> = if matches!(sort, AlbumsSort::Newest) {
        sorted.iter().copied().take(8).collect()
    } else {
        let mut by_date: Vec<&LibraryAlbum> = filtered.clone();
        by_date.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        by_date.into_iter().take(8).collect()
    };

    let owned: Vec<&LibraryAlbum> = sorted
        .iter()
        .copied()
        .filter(|a| !current_user.is_empty() && a.owner_id() == current_user)
        .collect();
    let shared: Vec<&LibraryAlbum> = sorted
        .iter()
        .copied()
        .filter(|a| !current_user.is_empty() && a.owner_id() != current_user)
        .collect();

    (recent, owned, shared)
}

fn render_albums(parts: &AlbumsViewParts) {
    let ctx_opt = parts.cached_ctx.borrow().clone();
    let on_click_opt = parts.cached_click.borrow().clone();
    let (Some(ctx), Some(on_click)) = (ctx_opt, on_click_opt) else {
        return;
    };

    clear(&parts.recent_grid);
    clear(&parts.owned_grid);
    clear(&parts.shared_grid);

    let current_user = ctx.current_user_id.lock().clone().unwrap_or_default();
    let query = parts.search_query.borrow().clone();
    let sort = parts.sort_mode.get();

    let albums = parts.cached_albums.borrow();
    let (mut recent, mut owned, shared) = project_albums(&albums, &current_user, &query, sort);
    if parts.shared_only.get() {
        recent.clear();
        owned = shared_by_you(owned);
    }
    let share = parts.cached_share.borrow().clone();
    let menu = |album: &LibraryAlbum| {
        let actions = tile_actions(album, &current_user)?;
        share.clone().map(|share| (share, actions))
    };
    insert_tiles(&parts.recent_grid, &recent, &ctx, &on_click, &menu);
    insert_tiles(&parts.owned_grid, &owned, &ctx, &on_click, &menu);
    insert_tiles(&parts.shared_grid, &shared, &ctx, &on_click, &menu);

    parts.recent_section.set_visible(!recent.is_empty());
    parts.owned_section.set_visible(!owned.is_empty());
    parts.shared_section.set_visible(!shared.is_empty());
    let nothing_shared = parts.shared_only.get() && owned.is_empty() && shared.is_empty();
    show_empty_state(parts, nothing_shared, current_user.is_empty());
    parts.populated.set(true);
}

fn build_empty_state() -> libadwaita::StatusPage {
    libadwaita::StatusPage::builder()
        .icon_name("folder-publicshare-symbolic")
        .visible(false)
        .build()
}

/// Explain an empty Shared Albums page; without the user id albums can't be sorted by owner.
fn show_empty_state(parts: &AlbumsViewParts, visible: bool, user_unknown: bool) {
    let (title, description) = if user_unknown {
        (
            &gettext("Couldn't Tell Which Albums Are Yours"),
            &gettext("Your API key needs the 'user.read' permission."),
        )
    } else {
        (
            &gettext("No Shared Albums"),
            &gettext("Share one of your albums, or ask someone to share one with you."),
        )
    };
    parts.empty.set_title(title);
    parts.empty.set_description(Some(description));
    parts.empty.set_visible(visible);
}

/// Own-album actions, member actions, or none while the current user is unknown.
fn tile_actions(
    album: &LibraryAlbum,
    current_user: &str,
) -> Option<&'static [(&'static str, AlbumAction)]> {
    if current_user.is_empty() {
        None
    } else if album.owner_id() == current_user {
        Some(OWN_ALBUM_ACTIONS)
    } else {
        Some(SHARED_ALBUM_ACTIONS)
    }
}

/// Add a tile per album, with the context menu `menu` picks for it.
fn insert_tiles(
    grid: &gtk::FlowBox,
    albums: &[&LibraryAlbum],
    ctx: &Arc<AppContext>,
    on_click: &AlbumClick,
    menu: &dyn Fn(&LibraryAlbum) -> Option<TileMenu>,
) {
    for album in albums {
        let tile = tile::album_tile(ctx.clone(), album, on_click.clone());
        if let Some((share, actions)) = menu(album) {
            attach_tile_menu(&tile, album, share, actions);
        }
        grid.insert(&tile, -1);
    }
}

fn attach_tile_menu(
    tile: &gtk::Button,
    album: &LibraryAlbum,
    share: AlbumShare,
    actions: &'static [(&'static str, AlbumAction)],
) {
    let (weak, album) = (tile.downgrade(), album.clone());
    let open = Rc::new(move |x, y| {
        if let Some(tile) = weak.upgrade() {
            show_tile_menu(&tile, &album, &share, actions, x, y);
        }
    });
    let on_secondary = open.clone();
    connect_context_menu(tile, move |x, y| on_secondary(x, y));
    let weak = tile.downgrade();
    tile.add_controller(menu_key(move || {
        if let Some(tile) = weak.upgrade() {
            open(
                f64::from(tile.width()) / 2.0,
                f64::from(tile.height()) / 2.0,
            );
        }
    }));
}

fn show_tile_menu(
    tile: &gtk::Button,
    album: &LibraryAlbum,
    share: &AlbumShare,
    actions: &'static [(&'static str, AlbumAction)],
    x: f64,
    y: f64,
) {
    let menu = Menu::new(tile, x, y);
    for &(label, action) in actions {
        let destructive = matches!(action, AlbumAction::Delete | AlbumAction::Leave);
        let (share, album) = (share.clone(), album.clone());
        menu.add_with_icon(action.icon(), &gettext(label), destructive, move || {
            share(album.clone(), action)
        });
    }
    menu.popup();
}

/// Build a single category grid section with a text label and a flow box layout.
fn build_section(title: &str) -> (gtk::Box, gtk::Label, gtk::FlowBox) {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    let label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(vec!["title-3".to_string()])
        .build();
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(20)
        .min_children_per_line(2)
        .row_spacing(12)
        .column_spacing(12)
        .homogeneous(true)
        .halign(gtk::Align::Start)
        .build();
    section.append(&label);
    section.append(&grid);
    (section, label, grid)
}

/// Remove all child widgets from the specified flow box.
fn clear(flow: &gtk::FlowBox) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
}

#[cfg(test)]
mod tests;
