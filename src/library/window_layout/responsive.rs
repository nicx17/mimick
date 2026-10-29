//! Responsive split views, breakpoints, and navigation shell.

use std::cell::Cell;
use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::library::search_view::SearchViewParts;

use super::ContentParts;

/// Search filters beside the results: docked on wide windows, sliding over them on phones.
pub(super) fn build_filters_split(
    results: &gtk::Paned,
    search_view: &SearchViewParts,
) -> libadwaita::OverlaySplitView {
    let split = libadwaita::OverlaySplitView::builder()
        .sidebar_position(gtk::PackType::End)
        .show_sidebar(false)
        .enable_show_gesture(false)
        .enable_hide_gesture(true)
        .sidebar_width_unit(libadwaita::LengthUnit::Px)
        .min_sidebar_width(280.0)
        .max_sidebar_width(360.0)
        .sidebar_width_fraction(0.3)
        .content(results)
        .sidebar(&search_view.filter_panel)
        .build();
    search_view
        .filters_toggle
        .bind_property("active", &split, "show-sidebar")
        .sync_create()
        .bidirectional()
        .build();
    let form = search_view.root.clone();
    split.connect_show_sidebar_notify(move |split| {
        if split.shows_sidebar() && !form.reveals_child() {
            split.set_show_sidebar(false);
        }
    });
    split
}

pub(super) fn build_split(
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

/// Navigation view whose root page holds the header and split view, under the window's toasts.
pub(super) fn build_nav(
    window: &libadwaita::ApplicationWindow,
    header: &libadwaita::HeaderBar,
    split: &libadwaita::OverlaySplitView,
) -> (libadwaita::NavigationView, libadwaita::ToastOverlay) {
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
    let toasts = libadwaita::ToastOverlay::new();
    toasts.set_child(Some(&nav));
    window.set_content(Some(&toasts));
    (nav, toasts)
}

fn breakpoint(condition: &str) -> libadwaita::Breakpoint {
    libadwaita::Breakpoint::new(
        libadwaita::BreakpointCondition::parse(condition).expect("valid breakpoint condition"),
    )
}

/// Phone width: show the bottom navigation bar and switch the grid to narrow layout.
pub(super) fn add_narrow_breakpoint(
    window: &libadwaita::ApplicationWindow,
    narrow: Rc<Cell<bool>>,
    content: &ContentParts,
) {
    let narrow_bp = breakpoint("max-width: 600px");
    narrow_bp.add_setter(&content.bottom_nav.root, "visible", Some(&true.to_value()));
    narrow_bp.add_setter(&content.bulk.secondary, "visible", Some(&false.to_value()));
    narrow_bp.add_setter(
        &content.bulk.overflow_box,
        "visible",
        Some(&true.to_value()),
    );
    narrow_bp.add_setter(
        &content.map.split,
        "orientation",
        Some(&gtk::Orientation::Vertical.to_value()),
    );
    let (narrow_apply, canvas_apply) = (narrow.clone(), content.grid.canvas.clone());
    let scrubber_apply = content.scrubber.clone();
    narrow_bp.connect_apply(move |_| {
        log::info!("NARROW breakpoint APPLIED: setting narrow=true");
        narrow_apply.set(true);
        canvas_apply.set_narrow(true);
        scrubber_apply.set_compact(true);
    });
    let (canvas_unapply, scrubber_unapply) =
        (content.grid.canvas.clone(), content.scrubber.clone());
    narrow_bp.connect_unapply(move |_| {
        log::info!("NARROW breakpoint UNAPPLIED: setting narrow=false");
        narrow.set(false);
        canvas_unapply.set_narrow(false);
        scrubber_unapply.set_compact(false);
    });
    window.add_breakpoint(narrow_bp);
}

/// Desktop width: horizontal controls, docked sidebar, and longer album-link labels.
pub(super) fn add_desktop_breakpoint(
    window: &libadwaita::ApplicationWindow,
    content: &ContentParts,
    split: &libadwaita::OverlaySplitView,
) {
    let desktop_bp = breakpoint("min-width: 600px");
    let window_apply = window.clone();
    desktop_bp.connect_apply(move |_| window_apply.add_css_class("mimick-wide"));
    let window_unapply = window.clone();
    desktop_bp.connect_unapply(move |_| window_unapply.remove_css_class("mimick-wide"));
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
    window.add_breakpoint(breakpoint("min-width: 600px and max-width: 1000px"));
}

/// F9 toggles the sidebar.
pub(super) fn add_sidebar_shortcut(
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
