//! Widget construction for the library window.
//!
//! Builds the header, controls, content stack, transfer and selection bars, and
//! the responsive breakpoints. `build_library_window` in the parent module wires
//! signal handlers onto the result.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use glib::clone;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::app_context::AppContext;
use crate::library::albums_view::{AlbumsViewParts, build_albums_view};
use crate::library::explore_view::{ExploreViewParts, build_explore_view};
use crate::library::masonry::{GridViewParts, build_grid_view};
use crate::library::search_view::{SearchViewParts, build_search_view};
use crate::library::sidebar::{SidebarParts, build_sidebar};

use super::trash_view::{self, TrashControls};
use super::{LibraryWindowUi, build_drop_overlay, build_loading_view, build_status_view};

pub(super) struct HeaderParts {
    header: libadwaita::HeaderBar,
    sidebar_toggle: gtk::ToggleButton,
    back_button: gtk::Button,
    select_toggle: gtk::ToggleButton,
}

/// Source/timeline and sort/upload controls above the grid.
pub(super) struct ControlsParts {
    root: gtk::Box,
    source_mode: gtk::DropDown,
    source_revealer: gtk::Revealer,
    timeline_toggle: gtk::ToggleButton,
    sort_mode: gtk::DropDown,
    upload_button: gtk::Button,
}

pub(super) struct TransferParts {
    bar: gtk::Box,
    progress: gtk::ProgressBar,
    icon: gtk::Image,
    label: gtk::Label,
}

pub(super) struct AlbumLinkParts {
    pub listbox: gtk::ListBox,
    row: libadwaita::ActionRow,
    link_button: gtk::Button,
    sync_button: gtk::Button,
}

/// Bottom selection bar; `delete`, `download`, and `clear` are wired by `connect_bulk_actions`.
pub(super) struct BulkParts {
    bar: gtk::Revealer,
    count_label: gtk::Label,
    pub delete: gtk::Button,
    pub download: gtk::Button,
    pub clear: gtk::Button,
    trash: TrashControls,
}

/// Everything shown in the main content column, plus the sidebar.
pub(super) struct ContentParts {
    sidebar: SidebarParts,
    grid: GridViewParts,
    explore: ExploreViewParts,
    albums: AlbumsViewParts,
    search_view: SearchViewParts,
    controls: ControlsParts,
    banner: gtk::Label,
    stack: gtk::Stack,
    error_label: gtk::Label,
    transfer: TransferParts,
    pub album_link: AlbumLinkParts,
    pub bulk: BulkParts,
}

/// The whole library window before signal handlers are connected.
pub(super) struct WindowParts {
    pub window: libadwaita::ApplicationWindow,
    nav: libadwaita::NavigationView,
    header: HeaderParts,
    pub content: ContentParts,
    split: libadwaita::OverlaySplitView,
    drop_overlay: gtk::Revealer,
}

impl WindowParts {
    pub(super) fn build(app: &libadwaita::Application, ctx: &Arc<AppContext>) -> Self {
        let window = libadwaita::ApplicationWindow::builder()
            .application(app)
            .title("Mimick Library")
            .name("mimick-library-window")
            .default_width(1480)
            .default_height(780)
            .width_request(360)
            .height_request(480)
            .build();
        let header = build_header();
        // Shared with the grid canvas so masonry layout knows when the window is narrow.
        let narrow = Rc::new(Cell::new(false));
        let content = ContentParts::build(ctx, header.select_toggle.clone(), narrow.clone());
        let (content_with_drop, drop_overlay) = build_drop_overlay(content.column());
        let split = build_split(
            &content.sidebar.root,
            &content_with_drop,
            &header.sidebar_toggle,
        );
        let nav = build_nav(&window, &header.header, &split);

        add_narrow_breakpoint(&window, narrow, &content, &header.back_button);
        add_desktop_breakpoint(&window, &content, &split);
        add_sidebar_shortcut(&window, &split);
        Self {
            window,
            nav,
            header,
            content,
            split,
            drop_overlay,
        }
    }
}

impl ContentParts {
    fn build(
        ctx: &Arc<AppContext>,
        select_toggle: gtk::ToggleButton,
        narrow: Rc<Cell<bool>>,
    ) -> Self {
        let grid = build_grid_view(ctx.clone(), select_toggle, narrow);
        let explore = build_explore_view();
        let albums = build_albums_view();
        let (stack, error_label) = build_content_stack(&grid, &explore, &albums);
        Self {
            sidebar: build_sidebar(),
            grid,
            explore,
            albums,
            search_view: build_search_view(),
            controls: build_controls(),
            banner: build_timeline_banner(),
            stack,
            error_label,
            transfer: build_transfer_bar(),
            album_link: build_album_link(),
            bulk: build_bulk_bar(),
        }
    }

    /// Main column, top to bottom.
    fn column(&self) -> gtk::Box {
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        column.append(&self.controls.root);
        column.append(&self.search_view.root);
        column.append(&self.album_link.listbox);
        column.append(&self.bulk.trash.bar);
        column.append(&self.banner);
        column.append(&self.stack);
        column.append(&self.bulk.bar);
        column.append(&self.transfer.bar);
        column
    }
}

impl LibraryWindowUi {
    pub(super) fn from_parts(
        ctx: Arc<AppContext>,
        app: &libadwaita::Application,
        parts: WindowParts,
    ) -> Self {
        let (header, content) = (parts.header, parts.content);
        let controls = content.controls;
        Self {
            ctx,
            app: app.clone(),
            window: parts.window,
            nav: parts.nav,
            sidebar: content.sidebar,
            grid: content.grid,
            explore: content.explore,
            albums: content.albums,
            content_stack: content.stack,
            error_label: content.error_label,
            transfer_bar: content.transfer.bar,
            transfer_progress: content.transfer.progress,
            transfer_icon: content.transfer.icon,
            transfer_label: content.transfer.label,
            search_view: content.search_view,
            sort_mode: controls.sort_mode,
            source_mode: controls.source_mode,
            source_revealer: controls.source_revealer,
            upload_button: controls.upload_button,
            timeline_toggle: controls.timeline_toggle,
            timeline_banner: content.banner,
            source_mode_suppressed: Cell::new(false),
            sidebar_suppressed: Cell::new(false),
            back_button: header.back_button,
            select_toggle: header.select_toggle,
            bulk_bar: content.bulk.bar,
            trash: content.bulk.trash,
            bulk_count_label: content.bulk.count_label,
            album_link_row: content.album_link.row,
            album_link_button: content.album_link.link_button,
            album_sync_button: content.album_link.sync_button,
            last_seen_upload_batch: Cell::new(0),
            split: parts.split,
            drop_overlay: parts.drop_overlay,
        }
    }
}

fn pressable_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .css_classes(["mimick-pressable"])
        .build()
}

fn build_header() -> HeaderParts {
    let header = libadwaita::HeaderBar::builder()
        .show_start_title_buttons(true)
        .show_end_title_buttons(true)
        .build();
    let sidebar_toggle = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-symbolic")
        .tooltip_text("Toggle sidebar (F9)")
        .active(true)
        .css_classes(["mimick-pressable"])
        .build();
    let back_button = pressable_button("go-previous-symbolic", "Back (Alt+Left)");
    back_button.set_sensitive(false);
    let menu = gtk::gio::Menu::new();
    menu.append(Some("Refresh"), Some("win.refresh"));
    menu.append(Some("Queue Inspector"), Some("win.queue"));
    menu.append(Some("Settings"), Some("win.settings"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Menu")
        .css_classes(["mimick-pressable"])
        .build();
    header.pack_start(&sidebar_toggle);
    header.pack_start(&back_button);
    header.pack_end(&menu_button);
    let select_toggle = gtk::ToggleButton::builder()
        .icon_name("checkbox-symbolic")
        .tooltip_text("Select assets (Esc to exit)")
        .build();
    HeaderParts {
        header,
        sidebar_toggle,
        back_button,
        select_toggle,
    }
}

fn hbox(children: &[&gtk::Widget]) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    for child in children {
        row.append(*child);
    }
    row
}

fn build_controls() -> ControlsParts {
    let source_mode = gtk::DropDown::builder()
        .model(&gtk::StringList::new(&["Remote", "Local", "Unified"]))
        .selected(0)
        .tooltip_text("Asset source")
        .build();
    let timeline_toggle = gtk::ToggleButton::builder()
        .label("Timeline")
        .tooltip_text("Timeline view (all assets only)")
        .build();
    // Source mode (Remote/Local/Unified) is meaningful only inside a linked
    // album; `apply_timeline_ui_state` reveals it per source kind.
    let source_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideRight)
        .transition_duration(180)
        .reveal_child(false)
        .child(&source_mode)
        .build();
    let sort_mode = gtk::DropDown::builder()
        .model(&gtk::StringList::new(&["Newest", "Filename", "File Type"]))
        .selected(0)
        .build();
    let upload_button = pressable_button("document-send-symbolic", "Upload to library");
    upload_button.add_css_class("suggested-action");

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(8)
        .margin_end(8)
        .build();
    root.append(&hbox(&[
        source_revealer.upcast_ref(),
        timeline_toggle.upcast_ref(),
    ]));
    root.append(&hbox(&[sort_mode.upcast_ref(), upload_button.upcast_ref()]));
    ControlsParts {
        root,
        source_mode,
        source_revealer,
        timeline_toggle,
        sort_mode,
        upload_button,
    }
}

fn build_timeline_banner() -> gtk::Label {
    gtk::Label::builder()
        .xalign(0.0)
        .css_classes(vec!["mimick-timeline-banner".to_string()])
        .visible(false)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(20)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(12)
        .build()
}

/// Stack of loading/empty/error states and the grid, Explore, and Albums views.
/// Also returns the error view's subtitle label, which shows the load error.
fn build_content_stack(
    grid: &GridViewParts,
    explore: &ExploreViewParts,
    albums: &AlbumsViewParts,
) -> (gtk::Stack, gtk::Label) {
    let stack = gtk::Stack::builder()
        .vexpand(true)
        .hexpand(true)
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(180)
        .build();
    let empty_view = build_status_view(
        "image-x-generic-symbolic",
        "Nothing to show",
        "No assets match the current view",
    );
    let error_view = build_status_view(
        "dialog-warning-symbolic",
        "Library data unavailable",
        "Could not load library assets",
    );
    let error_label = error_view
        .last_child()
        .and_downcast::<gtk::Label>()
        .expect("status-view subtitle label");
    stack.add_named(&build_loading_view(), Some("loading"));
    stack.add_named(&empty_view, Some("empty"));
    stack.add_named(&error_view, Some("error"));
    stack.add_named(&grid.scrolled, Some("grid"));
    stack.add_named(&explore.root, Some("explore"));
    stack.add_named(&albums.root, Some("albums"));
    (stack, error_label)
}

fn build_transfer_bar() -> TransferParts {
    let progress = gtk::ProgressBar::builder()
        .hexpand(true)
        .valign(gtk::Align::Center)
        .css_classes(vec!["mimick-transfer-progress".to_string()])
        .build();
    let icon = gtk::Image::builder()
        .icon_size(gtk::IconSize::Normal)
        .css_classes(vec!["dim-label".to_string()])
        .visible(false)
        .build();
    let label = gtk::Label::builder()
        .xalign(0.0)
        .hexpand(true)
        .wrap(true)
        .max_width_chars(24)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(vec!["caption".to_string(), "dim-label".to_string()])
        .build();
    let bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(8)
        .margin_bottom(16)
        .margin_start(12)
        .margin_end(12)
        .css_classes(vec!["mimick-transfer-shell".to_string()])
        .build();
    bar.append(&progress);
    bar.append(&icon);
    bar.append(&label);
    TransferParts {
        bar,
        progress,
        icon,
        label,
    }
}

fn build_album_link() -> AlbumLinkParts {
    let row = libadwaita::ActionRow::builder()
        .title("No local folder linked")
        .subtitle("Drop files in the linked folder to sync this album")
        .title_lines(1)
        .subtitle_lines(2)
        .build();
    let sync_button = gtk::Button::builder()
        .label("Sync")
        .valign(gtk::Align::Center)
        .css_classes(vec!["suggested-action".to_string()])
        .visible(false)
        .build();
    let link_button = gtk::Button::builder()
        .label("Link")
        .valign(gtk::Align::Center)
        .build();
    row.add_suffix(&sync_button);
    row.add_suffix(&link_button);
    let listbox = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(vec!["boxed-list".to_string()])
        .margin_start(12)
        .margin_end(12)
        .margin_top(4)
        .margin_bottom(4)
        .visible(false)
        .build();
    listbox.append(&row);
    AlbumLinkParts {
        listbox,
        row,
        link_button,
        sync_button,
    }
}

fn build_bulk_bar() -> BulkParts {
    let count_label = gtk::Label::builder().xalign(0.0).hexpand(true).build();
    let delete = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("Delete selected")
        .css_classes(vec!["destructive-action".to_string()])
        .build();
    let download = gtk::Button::builder()
        .icon_name("mimick-download-symbolic")
        .tooltip_text("Download selected")
        .build();
    let clear = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .tooltip_text("Clear selection")
        .css_classes(vec!["flat".to_string()])
        .build();
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(16)
        .margin_start(12)
        .margin_end(12)
        .css_classes(vec!["toolbar".to_string()])
        .build();
    // Download and Move to trash are swapped for Restore / Delete Permanently in the Trash view.
    let trash = trash_view::build_trash_controls(vec![delete.clone(), download.clone()]);
    inner.append(&count_label);
    inner.append(&clear);
    inner.append(&download);
    inner.append(&delete);
    inner.append(&trash.restore_selected);
    inner.append(&trash.delete_selected);
    let bar = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideUp)
        .reveal_child(false)
        .child(&inner)
        .build();
    BulkParts {
        bar,
        count_label,
        delete,
        download,
        clear,
        trash,
    }
}

fn build_split(
    sidebar_root: &gtk::Box,
    content: &impl IsA<gtk::Widget>,
    sidebar_toggle: &gtk::ToggleButton,
) -> libadwaita::OverlaySplitView {
    let split = libadwaita::OverlaySplitView::builder()
        .sidebar(sidebar_root)
        .content(content)
        .show_sidebar(true)
        .collapsed(true)
        .enable_show_gesture(true)
        .enable_hide_gesture(true)
        .min_sidebar_width(180.0)
        .max_sidebar_width(260.0)
        .sidebar_width_fraction(0.3)
        .build();
    split
        .bind_property("show-sidebar", sidebar_toggle, "active")
        .sync_create()
        .bidirectional()
        .build();
    split
}

/// Navigation view whose root page holds the header and split view.
fn build_nav(
    window: &libadwaita::ApplicationWindow,
    header: &libadwaita::HeaderBar,
    split: &libadwaita::OverlaySplitView,
) -> libadwaita::NavigationView {
    let toolbar = libadwaita::ToolbarView::builder().build();
    toolbar.add_top_bar(header);
    toolbar.set_content(Some(split));
    let nav = libadwaita::NavigationView::new();
    let root_page = libadwaita::NavigationPage::builder()
        .child(&toolbar)
        .title("Library")
        .can_pop(false)
        .build();
    nav.add(&root_page);
    window.set_content(Some(&nav));
    nav
}

fn breakpoint(condition: &str) -> libadwaita::Breakpoint {
    libadwaita::Breakpoint::new(
        libadwaita::BreakpointCondition::parse(condition).expect("valid breakpoint condition"),
    )
}

/// Phone width: hide the transfer bar and back button, and switch the grid to narrow layout.
fn add_narrow_breakpoint(
    window: &libadwaita::ApplicationWindow,
    narrow: Rc<Cell<bool>>,
    content: &ContentParts,
    back_button: &gtk::Button,
) {
    let narrow_bp = breakpoint("max-width: 600px");
    narrow_bp.add_setter(&content.transfer.bar, "visible", Some(&false.to_value()));
    narrow_bp.add_setter(back_button, "visible", Some(&false.to_value()));
    let (narrow_apply, canvas_apply) = (narrow.clone(), content.grid.canvas.clone());
    narrow_bp.connect_apply(move |_| {
        log::info!("NARROW breakpoint APPLIED: setting narrow=true");
        narrow_apply.set(true);
        canvas_apply.set_narrow(true);
    });
    let canvas_unapply = content.grid.canvas.clone();
    narrow_bp.connect_unapply(move |_| {
        log::info!("NARROW breakpoint UNAPPLIED: setting narrow=false");
        narrow.set(false);
        canvas_unapply.set_narrow(false);
    });
    window.add_breakpoint(narrow_bp);
}

/// Desktop width: horizontal controls, docked sidebar, and longer album-link labels.
fn add_desktop_breakpoint(
    window: &libadwaita::ApplicationWindow,
    content: &ContentParts,
    split: &libadwaita::OverlaySplitView,
) {
    let desktop_bp = breakpoint("min-width: 600px");
    let window_apply = window.clone();
    desktop_bp.connect_apply(move |_| window_apply.add_css_class("mimick-wide"));
    let window_unapply = window.clone();
    desktop_bp.connect_unapply(move |_| window_unapply.remove_css_class("mimick-wide"));
    desktop_bp.add_setter(
        &content.controls.root,
        "orientation",
        Some(&gtk::Orientation::Horizontal.to_value()),
    );
    desktop_bp.add_setter(split, "collapsed", Some(&false.to_value()));
    desktop_bp.add_setter(
        &content.album_link.sync_button,
        "label",
        Some(&"Sync…".to_value()),
    );
    desktop_bp.add_setter(
        &content.album_link.link_button,
        "label",
        Some(&"Link folder…".to_value()),
    );
    window.add_breakpoint(desktop_bp);
    // Tablet width: reserved for chrome tweaks below typical desktop widths.
    window.add_breakpoint(breakpoint("max-width: 1000px"));
}

/// F9 toggles the sidebar.
fn add_sidebar_shortcut(
    window: &libadwaita::ApplicationWindow,
    split: &libadwaita::OverlaySplitView,
) {
    let f9 = gtk::Shortcut::builder()
        .trigger(&gtk::ShortcutTrigger::parse_string("F9").unwrap())
        .action(&gtk::CallbackAction::new(clone!(
            #[strong]
            split,
            move |_, _| {
                split.set_show_sidebar(!split.shows_sidebar());
                glib::Propagation::Stop
            }
        )))
        .build();
    let shortcut_controller = gtk::ShortcutController::new();
    shortcut_controller.add_shortcut(f9);
    window.add_controller(shortcut_controller);
}
