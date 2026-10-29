//! Library view module -- browse, search, and download assets from an Immich server.
//!
//! Constructs the main GTK window with a split-pane sidebar, paginated
//! grid view, timeline scrubber, and search bar. Submodules handle the
//! explore dashboard, album grids, lightbox, sidebar, and thumbnail
//! caching independently.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use glib::clone;
use gtk::prelude::*;

use crate::api_client::SharedLinkTarget;
use crate::app_context::AppContext;
use crate::library::albums_view::AlbumsViewParts;
use crate::library::explore_view::ExploreViewParts;
use crate::library::masonry::GridViewParts;
use crate::library::search_view::SearchViewParts;
use crate::library::sidebar::SidebarParts;
use crate::library::state::Location;

use self::actions::{connect_bulk_actions, connect_select_mode};
use self::album_link::{connect_album_link_row, refresh_album_link_row};
use self::context_menu::{AssetMenuHooks, show_asset_context_menu};
use self::controls::{
    connect_controls, connect_grid_handlers, connect_sidebar_handlers,
    refresh_library_after_mutation,
};
use self::lightbox::open_lightbox;

pub(super) const LOCAL_ID_PREFIX: &str = "local::";

pub mod album_sync;
pub mod albums_view;
mod archive;
pub mod asset_model;
pub mod asset_object;
mod bottom_nav;
mod bulk_date;
mod bulk_overflow;
pub mod explore_view;
mod favorites;
pub mod local_exif;
pub mod local_source;
pub mod masonry;
pub mod sidebar;
pub mod state;
pub mod style;
pub mod thumbnail_cache;
mod thumbnail_loader;

mod about_dialog;
mod actions;
mod album_assets;
mod album_delete;
mod album_download;
mod album_edit;
mod album_link;
mod album_options;
mod album_picker;
mod album_share_dialog;
mod album_wiring;
mod asset_edit;
mod asset_tags;
mod content_state;
mod context_menu;
mod controls;
mod dialogs;
mod download;
mod drop_target;
mod features;
mod folders_view;
mod header_format;
mod lightbox;
mod map;
mod nav_history;
mod navigation;
mod people_view;
mod person_merge_dialog;
pub mod search_filters;
mod search_panel;
pub mod search_view;
mod search_wiring;
mod source_loading;
use album_wiring::{
    connect_albums_create, refresh_albums_view, refresh_search_tags, search_by_tag,
};
use content_state::{
    build_loading_view, build_status_view, immich_checksum_to_hex, sync_content_state,
    update_footer, update_transfer_ui,
};
use drop_target::{connect_drop_target, handle_album_drop_upload, show_unsupported_drop_toast};
use source_loading::{
    apply_source_ui_state, load_albums, load_explore_landing, load_pages, load_source_page,
    load_status,
};
mod server_stats_dialog;
mod shared_link_dialog;
mod shared_links_view;
mod sharing_view;
mod shortcuts_dialog;
mod source_header;
pub mod staging_view;
mod tags_view;
mod trash_view;
mod tree_page;
mod upload_picker;
mod video_player;
mod window_layout;
use search_wiring::{connect_lightweight_filters, connect_search_view};

const PAGE_SIZE: u32 = 50;
/// Map areas load each photo by ID, so smaller pages show the first ones sooner.
const MAP_AREA_PAGE_SIZE: u32 = 32;

mod texture_decode;
mod time_zones;
mod timeline;
use texture_decode::{decode_raw_thumbnail_texture, load_texture_blocking};
pub use texture_decode::{set_raw_cache_enabled, set_raw_full_decode};

/// Register application custom icons with the Gtk icon theme system.
pub(crate) fn register_app_icons() {
    if let Some(display) = gtk::gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        theme.add_resource_path("/dev/nicx/mimick/icons");
    }
}

/// Load a local texture, preferring decoders that cover formats outside the
/// GDK-Pixbuf runtime loader set before falling back to pixbuf and image-rs.
pub(crate) async fn load_texture_oriented(path: &std::path::Path) -> Option<gdk4::Texture> {
    let path_buf = path.to_path_buf();
    tokio::task::spawn_blocking(move || load_texture_blocking(&path_buf))
        .await
        .ok()
        .flatten()
}

/// The complete UI widgets state wrapper for the library window interface.
struct LibraryWindowUi {
    ctx: Arc<AppContext>,
    app: libadwaita::Application,
    window: libadwaita::ApplicationWindow,
    nav: libadwaita::NavigationView,
    toasts: libadwaita::ToastOverlay,
    sidebar: SidebarParts,
    grid: GridViewParts,
    explore: ExploreViewParts,
    albums: AlbumsViewParts,
    shared_links: shared_links_view::SharedLinksView,
    people: people_view::PeopleView,
    tags: tags_view::TagsView,
    folders: folders_view::FoldersView,
    sharing: sharing_view::SharingView,
    map: map::MapView,
    header: source_header::SourceHeader,
    bottom_nav: bottom_nav::BottomNav,
    features: Cell<crate::api_client::LibraryFeatures>,
    content_stack: gtk::Stack,
    error_label: gtk::Label,
    transfer: window_layout::TransferParts,
    search_view: SearchViewParts,
    filters_split: libadwaita::OverlaySplitView,
    sort_mode: gtk::DropDown,
    source_mode: gtk::DropDown,
    source_revealer: gtk::Revealer,
    upload_button: gtk::Button,
    controls_row: gtk::Box,
    back_button: gtk::Button,
    select_toggle: gtk::ToggleButton,
    bulk: window_layout::BulkParts,
    album_link: window_layout::AlbumLinkParts,
    interaction: WindowInteractionState,
    split: libadwaita::OverlaySplitView,
    drop_overlay: gtk::Revealer,
    scrubber: timeline::scrubber::TimelineScrubber,
}

#[derive(Default)]
struct WindowInteractionState {
    source_mode_suppressed: Cell<bool>,
    sidebar_suppressed: Cell<bool>,
    sort_suppressed: Cell<bool>,
    album_sort_applied: Cell<bool>,
    last_seen_upload_batch: Cell<u64>,
    timeline: RefCell<Option<Rc<timeline::Timeline>>>,
}

/// Build and display the main library window application layout.
pub fn build_library_window(app: &libadwaita::Application, ctx: Arc<AppContext>) {
    style::ensure_registered();
    register_app_icons();
    let (ui, window) = build_window_ui(app, ctx);
    bootstrap_window(ui);
    window.present();
}

fn build_window_ui(
    app: &libadwaita::Application,
    ctx: Arc<AppContext>,
) -> (Rc<LibraryWindowUi>, libadwaita::ApplicationWindow) {
    let parts = window_layout::WindowParts::build(app, &ctx);
    let window = parts.window.clone();
    crate::debug_width::install(&window);
    crate::debug_width::trace_overflow_warnings(&window);
    let bulk = &parts.content.bulk;
    let controls = WindowControls {
        album_link_listbox: parts.content.album_link.listbox.clone(),
        bulk_delete: bulk.delete.clone(),
        bulk_download: bulk.download.clone(),
        bulk_clear: bulk.clear.clone(),
        bulk_share_link: bulk.share_link.clone(),
        bulk_location: bulk.location.clone(),
        bulk_date: bulk.date.clone(),
    };
    let ui = Rc::new(LibraryWindowUi::from_parts(ctx, app, parts));
    connect_window_controls(&ui, controls);
    install_close_handler(&ui, &window);
    (ui, window)
}

struct WindowControls {
    album_link_listbox: gtk::ListBox,
    bulk_delete: gtk::Button,
    bulk_download: gtk::Button,
    bulk_clear: gtk::Button,
    bulk_share_link: gtk::Button,
    bulk_location: gtk::Button,
    bulk_date: gtk::Button,
}

fn connect_window_controls(ui: &Rc<LibraryWindowUi>, controls: WindowControls) {
    connect_menus(ui.clone());
    connect_album_link_row(ui.clone(), controls.album_link_listbox);
    connect_select_mode(ui.clone(), ui.select_toggle.clone());
    connect_bulk_actions(
        ui.clone(),
        controls.bulk_delete,
        controls.bulk_download,
        controls.bulk_clear,
    );
    actions::connect_bulk_share_link(ui.clone(), &controls.bulk_share_link);
    actions::connect_bulk_archive(ui.clone());
    actions::connect_bulk_location(ui.clone(), &controls.bulk_location);
    actions::connect_bulk_date(ui.clone(), &controls.bulk_date);
    actions::connect_bulk_add_to_album(ui.clone());
    actions::connect_bulk_remove_from_album(ui.clone());
    trash_view::connect_trash_controls(ui.clone());
    people_view::connect_people_view(ui.clone());
    tags_view::connect_tags_view(ui.clone());
    sharing_view::connect_sharing_view(ui.clone());
    shared_links_view::connect_shared_links_view(ui.clone());
    map::connect_map_view(ui);
    timeline::connect(ui);
    bottom_nav::connect_bottom_nav(ui);
    connect_sidebar_handlers(ui.clone());
    connect_controls(ui.clone());
    connect_grid_handlers(ui.clone());
    connect_search_view(ui.clone());
    connect_lightweight_filters(ui.clone());
    connect_drop_target(ui.clone());
}

fn install_close_handler(ui: &Rc<LibraryWindowUi>, window: &libadwaita::ApplicationWindow) {
    let close_ctx = ui.ctx.clone();
    window.connect_close_request(move |_| {
        close_ctx.thumbnail_cache.clear_memory();
        glib::timeout_add_seconds_local_once(3, release_freed_memory);
        glib::Propagation::Proceed
    });
}

#[cfg(target_env = "gnu")]
fn release_freed_memory() {
    unsafe extern "C" {
        safe fn malloc_trim(pad: usize) -> i32;
    }
    malloc_trim(0);
}

#[cfg(not(target_env = "gnu"))]
fn release_freed_memory() {}

/// Wire the grid right-click menu and the album tile sharing actions.
fn connect_menus(ui: Rc<LibraryWindowUi>) {
    *ui.grid.context_menu_handler.borrow_mut() = Some(Box::new(clone!(
        #[strong]
        ui,
        move |position, x, y| {
            show_asset_context_menu(
                ui.clone(),
                &ui.grid.canvas,
                position,
                x,
                y,
                AssetMenuHooks::for_grid(&ui),
            );
        }
    )));

    let ui_for_share = ui.clone();
    albums_view::set_share_handler(
        &ui.albums,
        Rc::new(move |album, action| match action {
            albums_view::AlbumAction::Share => {
                album_share_dialog::present(ui_for_share.clone(), album)
            }
            albums_view::AlbumAction::Rename => album_edit::rename(&ui_for_share, &album),
            albums_view::AlbumAction::EditDescription => {
                album_edit::edit_description(&ui_for_share, &album)
            }
            albums_view::AlbumAction::Leave => album_assets::confirm_leave(&ui_for_share, album),
            albums_view::AlbumAction::Delete => album_delete::confirm_delete(&ui_for_share, album),
            albums_view::AlbumAction::Download => {
                album_download::download_album(ui_for_share.clone(), album.id, album.album_name)
            }
            albums_view::AlbumAction::CreateLink => shared_link_dialog::present(
                ui_for_share.clone(),
                shared_link_dialog::LinkDialogMode::Create(SharedLinkTarget::Album(album.id)),
                Rc::new(|| {}),
            ),
        }),
    );
}

/// Bootstrap initial data loading and background tasks for the library window.
fn bootstrap_window(ui: Rc<LibraryWindowUi>) {
    let initial_request = {
        let mut state = ui.ctx.library_state.lock();
        state.load_initial_source()
    };

    apply_source_ui_state(&ui, &initial_request.1);
    source_header::show(&ui, &initial_request.1);
    navigation::sync_controls(&ui, &Location::Source(initial_request.1.clone()));
    load_albums(ui.clone());
    load_status(ui.clone());
    features::load(&ui);
    fetch_current_user(ui.clone());
    connect_albums_create(ui.clone());
    let timers = vec![
        spawn_server_ping_loop(ui.clone()),
        spawn_transfer_poll_loop(ui.clone()),
    ];
    stop_timers_on_close(&ui.window, timers);
    load_source_page(ui, initial_request, false);
}

/// Build a drop overlay widget and wrap `content` in a `gtk::Overlay`.
///
/// Returns the overlay wrapper (to be used as the content widget) and the
/// revealer handle for toggling visibility from the `DropTarget` handlers.
fn build_drop_overlay(content: gtk::Box) -> (gtk::Overlay, gtk::Revealer) {
    let drop_icon = gtk::Image::builder()
        .icon_name("document-send-symbolic")
        .pixel_size(48)
        .halign(gtk::Align::Center)
        .build();
    let drop_label = gtk::Label::builder()
        .label("Drop files to upload")
        .halign(gtk::Align::Center)
        .build();
    // Inner box centers the icon+label; outer box provides the full-window tint.
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build();
    inner.append(&drop_icon);
    inner.append(&drop_label);

    let drop_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .vexpand(true)
        .hexpand(true)
        .css_classes(["mimick-drop-overlay"])
        .build();
    drop_box.append(&inner);

    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(180)
        .reveal_child(false)
        .can_target(false)
        .vexpand(true)
        .hexpand(true)
        .child(&drop_box)
        .build();

    let overlay = gtk::Overlay::builder().build();
    overlay.set_child(Some(&content));
    overlay.add_overlay(&revealer);

    (overlay, revealer)
}

/// Wire the lightweight filter entries embedded in Albums and Explore views.
fn stop_timers_on_close(window: &libadwaita::ApplicationWindow, timers: Vec<glib::SourceId>) {
    let timers = RefCell::new(timers);
    window.connect_close_request(move |_| {
        for timer in timers.borrow_mut().drain(..) {
            timer.remove();
        }
        glib::Propagation::Proceed
    });
}

fn spawn_server_ping_loop(ui: Rc<LibraryWindowUi>) -> glib::SourceId {
    glib::timeout_add_seconds_local(5, move || {
        let ui_for_tick = ui.clone();
        glib::MainContext::default().spawn_local(async move {
            let _ = ui_for_tick.ctx.api_client.check_connection().await;
            let route = ui_for_tick.ctx.api_client.active_route_label().await;

            // If we are online but missing stats (e.g. from an initial network failure), re-fetch them.
            if route.is_some() {
                let missing_stats = {
                    let state = ui_for_tick.ctx.library_state.lock();
                    state.status.stats.is_none() || state.status.about.is_none()
                };
                if missing_stats {
                    let stats = ui_for_tick.ctx.api_client.fetch_server_stats().await.ok();
                    let about = ui_for_tick.ctx.api_client.fetch_server_about().await.ok();
                    if stats.is_some() || about.is_some() {
                        let mut state = ui_for_tick.ctx.library_state.lock();
                        state.set_status(stats, about);
                    }
                }
            }

            update_footer(&ui_for_tick, route);
        });
        glib::ControlFlow::Continue
    })
}

/// Start a quick periodic polling loop updating the transfer progress bar UI.
fn spawn_transfer_poll_loop(ui: Rc<LibraryWindowUi>) -> glib::SourceId {
    glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        let completed_batches = ui.ctx.state.lock().completed_upload_batches;
        if completed_batches != ui.interaction.last_seen_upload_batch.get() {
            ui.interaction.last_seen_upload_batch.set(completed_batches);
            refresh_library_after_mutation(ui.clone(), true);
        }
        update_transfer_ui(&ui);
        glib::ControlFlow::Continue
    })
}

/// Fetch current logged-in API user ID and store it in app context.
fn fetch_current_user(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(async move {
        ensure_current_user_id(&ui.ctx).await;
    });
}

/// The API key's user id, fetched and cached on first use.
async fn ensure_current_user_id(ctx: &AppContext) -> Option<String> {
    if let Some(id) = ctx.current_user_id.lock().clone() {
        return Some(id);
    }
    match ctx.api_client.fetch_current_user_id().await {
        Ok(id) => {
            *ctx.current_user_id.lock() = Some(id.clone());
            Some(id)
        }
        Err(err) => {
            log::warn!("Could not fetch current user id: {}", err);
            None
        }
    }
}

fn setup_album_drop_target(
    ui: &Rc<LibraryWindowUi>,
    row: &gtk::ListBoxRow,
    album_id: String,
    album_name: String,
) {
    let row_drop = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY,
    );
    let row_ref_enter = row.clone();
    row_drop.connect_enter(move |target, _x, _y| {
        if target.current_drop().is_some_and(|d| d.drag().is_some()) {
            return gtk::gdk::DragAction::empty();
        }
        row_ref_enter.add_css_class("mimick-album-drop-hover");
        gtk::gdk::DragAction::COPY
    });
    let row_ref_leave = row.clone();
    row_drop.connect_leave(move |_target| {
        row_ref_leave.remove_css_class("mimick-album-drop-hover");
    });
    let row_ref_drop = row.clone();
    let ui_drop = ui.clone();
    row_drop.connect_drop(move |target, value, _x, _y| {
        row_ref_drop.remove_css_class("mimick-album-drop-hover");

        if target.current_drop().is_some_and(|d| d.drag().is_some()) {
            return false;
        }
        let file_list = match value.get::<gtk::gdk::FileList>() {
            Ok(fl) => fl,
            Err(_) => return false,
        };
        let paths: Vec<std::path::PathBuf> = file_list
            .files()
            .iter()
            .filter_map(gtk::gio::prelude::FileExt::path)
            .filter(|p| crate::media_kinds::is_supported_path(p))
            .collect();
        if paths.is_empty() {
            show_unsupported_drop_toast(&ui_drop);
            return true;
        }
        handle_album_drop_upload(&ui_drop, (album_id.clone(), album_name.clone()), paths);
        true
    });
    row.add_controller(row_drop);
}
