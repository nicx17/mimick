//! Library view module -- browse, search, and download assets from an Immich server.
//!
//! Constructs the main GTK window with a split-pane sidebar, paginated
//! grid view, timeline scrubber, and search bar. Submodules handle the
//! explore dashboard, album grids, lightbox, sidebar, and thumbnail
//! caching independently.

use gettextrs::gettext;
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
mod bulk_more;
mod bulk_overflow;
pub mod explore_view;
mod favorites;
pub mod local_exif;
pub mod local_source;
pub mod masonry;
pub mod sidebar;
pub mod state;
pub mod style;
pub mod theme;
pub mod thumbnail_cache;
mod thumbnail_loader;

mod about_dialog;
mod account;
mod actions;
mod album_activity;
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
mod asset_jobs;
mod asset_tags;
mod color;
mod content_state;
mod context_menu;
mod controls;
mod day_row;
mod dialogs;
mod download;
mod drop_target;
mod duplicates_view;
mod features;
mod folders_view;
mod header_format;
mod inbox;
mod lightbox;
mod map;
mod memories;
mod nav_history;
mod navigation;
mod people_view;
mod person_merge_dialog;
mod person_picker;
mod photo_canvas;
mod photo_editor;
mod related;
pub mod search_filters;
mod search_options;
mod search_panel;
pub mod search_view;
mod search_wiring;
mod select_all;
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
mod clipboard;
mod server_stats_dialog;
mod shared_link_dialog;
mod shared_link_photos;
mod shared_link_qr;
mod shared_links_view;
mod sharing_view;
mod shortcuts_dialog;
mod source_header;
pub mod staging_view;
mod tags_view;
mod trash_view;
mod tree_page;
mod upload_picker;
mod upload_stack;
mod utilities;
mod video_player;
mod window_layout;
use search_wiring::{connect_lightweight_filters, connect_search_view};

const PAGE_SIZE: u32 = 50;
/// Map areas load each photo by ID, so smaller pages show the first ones sooner.
const MAP_AREA_PAGE_SIZE: u32 = 32;
/// Large Files is one list from Immich, with no further pages; this many is plenty to clear.
const LARGE_FILES_LIMIT: u32 = 250;

mod texture_decode;
mod time_zones;
mod timeline;
use texture_decode::{
    decode_heif_thumbnail, decode_raw_thumbnail_texture, develops_raw, is_heif_path,
    load_texture_blocking, load_texture_fitted_blocking, load_texture_like,
};
pub use texture_decode::{set_raw_cache_enabled, set_raw_full_decode};

/// Register application custom icons with the Gtk icon theme system, once per process.
/// Startup calls it, so the Settings window has them before any library window exists.
pub(crate) fn register_app_icons() {
    static REGISTERED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    REGISTERED.get_or_init(|| {
        if let Some(display) = gtk::gdk::Display::default() {
            let theme = gtk::IconTheme::for_display(&display);
            theme.add_resource_path("/dev/nicx/mimick/icons");
        }
    });
}

/// Load a local texture, preferring decoders that cover formats outside the
/// GDK-Pixbuf runtime loader set before falling back to pixbuf and image-rs.
pub(crate) async fn load_texture_oriented(path: &std::path::Path) -> Option<gdk4::Texture> {
    load_texture_oriented_like(path, None).await
}

/// Like [`load_texture_oriented`], with a RAW that full decoding develops taking the look
/// of `look`, the picture the grid shows for it.
pub(crate) async fn load_texture_oriented_like(
    path: &std::path::Path,
    look: Option<gdk4::Texture>,
) -> Option<gdk4::Texture> {
    let path_buf = path.to_path_buf();
    tokio::task::spawn_blocking(move || load_texture_like(&path_buf, look.as_ref()))
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
    duplicates: duplicates_view::DuplicatesView,
    map: map::MapView,
    header: source_header::SourceHeader,
    memories: memories::MemoriesRow,
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
    /// Its menu holds Paste; shown wherever new photos can go.
    upload_button: libadwaita::SplitButton,
    controls_row: gtk::Box,
    back_button: gtk::Button,
    /// Header search on phones, which have no other one-tap way in; hidden when search is off.
    search_button: gtk::Button,
    /// Notifications from Immich; shown while some are unread.
    bell: inbox::Bell,
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
    lightbox: RefCell<Option<Rc<lightbox::Lightbox>>>,
    /// Photo for the next Photos timeline to scroll to: asset id and its `YYYY-MM`.
    timeline_reveal: RefCell<Option<(String, String)>>,
    /// What Mimick last put on the clipboard, for Paste to recognise its own copy.
    copied: RefCell<Option<clipboard::Copied>>,
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
    let ui = Rc::new(LibraryWindowUi::from_parts(ctx, app, parts));
    connect_window_controls(&ui);
    install_close_handler(&ui, &window);
    (ui, window)
}

fn connect_window_controls(ui: &Rc<LibraryWindowUi>) {
    let bulk = &ui.bulk;
    connect_menus(ui.clone());
    connect_album_link_row(ui.clone(), ui.album_link.listbox.clone());
    connect_select_mode(ui.clone(), ui.select_toggle.clone());
    connect_bulk_actions(
        ui.clone(),
        bulk.delete.clone(),
        bulk.download.clone(),
        bulk.clear.clone(),
    );
    actions::connect_bulk_share_link(ui.clone(), &bulk.share_link);
    actions::connect_bulk_archive(ui.clone());
    actions::connect_bulk_stack(ui.clone());
    actions::connect_bulk_location(ui.clone(), &bulk.location);
    actions::connect_bulk_date(ui.clone(), &bulk.date);
    actions::connect_bulk_add_to_album(ui.clone());
    actions::connect_bulk_remove_from_album(ui.clone());
    bulk_more::connect(ui);
    trash_view::connect_trash_controls(ui.clone());
    people_view::connect_people_view(ui.clone());
    tags_view::connect_tags_view(ui.clone());
    folders_view::connect_folders_view(ui);
    sharing_view::connect_sharing_view(ui.clone());
    duplicates_view::connect_duplicates_view(ui.clone());
    shared_links_view::connect_shared_links_view(ui.clone());
    map::connect_map_view(ui);
    timeline::connect(ui);
    utilities::connect(ui);
    source_header::connect_slideshow(ui);
    bottom_nav::connect_bottom_nav(ui);
    connect_sidebar_handlers(ui.clone());
    connect_controls(ui.clone());
    clipboard::connect_clipboard(ui);
    connect_grid_handlers(ui.clone());
    connect_search_view(ui.clone());
    connect_lightweight_filters(ui.clone());
    connect_drop_target(ui.clone());
}

/// Closing the library frees its memory; as the last window with background sync off, it
/// quits Mimick too, as closing Settings does, instead of leaving it in the tray.
fn install_close_handler(ui: &Rc<LibraryWindowUi>, window: &libadwaita::ApplicationWindow) {
    let (close_ctx, app) = (ui.ctx.clone(), ui.app.clone());
    let weak = Rc::downgrade(ui);
    window.connect_close_request(move |_| {
        log::info!("Library window closing");
        if let Some(ui) = weak.upgrade() {
            lightbox::close_lightbox(&ui);
        }
        if crate::ui_lifecycle::quits_on_close(&app, &close_ctx) {
            app.quit();
            return glib::Propagation::Stop;
        }
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
    load_albums(ui.clone());
    load_status(ui.clone());
    features::load(&ui);
    fetch_current_user(ui.clone());
    connect_albums_create(ui.clone());
    account::connect(&ui);
    let timers = vec![
        spawn_server_ping_loop(ui.clone()),
        spawn_transfer_poll_loop(ui.clone()),
        inbox::connect(&ui),
    ];
    stop_timers_on_close(&ui.window, timers);
    navigation::restore(&ui);
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
        .label(gettext("Drop files to upload"))
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
