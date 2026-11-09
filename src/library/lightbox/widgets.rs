use gettextrs::gettext;
use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::library::LibraryWindowUi;
use crate::library::asset_tags::TagSection;

use super::details::truncate_filename;
use super::{activity, albums, ocr, people, rating};
use crate::library::archive::{archive_icon, archive_tooltip};
use crate::library::favorites::{heart_icon, heart_tooltip};

pub(crate) fn lightbox_picture() -> gtk::Picture {
    gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Contain)
        .vexpand(true)
        .hexpand(true)
        .css_classes(["mimick-lightbox-picture"])
        .build()
}

pub(crate) fn crossfade_overlay(child: &impl IsA<gtk::Widget>, align: gtk::Align) -> gtk::Revealer {
    gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(180)
        .reveal_child(false)
        .halign(align)
        .valign(align)
        .child(child)
        .can_target(false)
        .build()
}

pub(crate) fn details_text_label() -> gtk::Label {
    gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(28)
        .build()
}

/// Title length that still fits in the header beside the nav buttons.
pub(crate) fn lightbox_title_cap(ui: &LibraryWindowUi) -> usize {
    if ui.split.is_collapsed() { 14 } else { 24 }
}

/// Navigation page with its header bar (back, prev/next, details toggle).
pub(crate) struct LightboxHeader {
    pub(super) page: libadwaita::NavigationPage,
    pub(super) toolbar: libadwaita::ToolbarView,
    pub(super) bar: libadwaita::HeaderBar,
    pub(super) prev_btn: gtk::Button,
    pub(super) next_btn: gtk::Button,
    pub(super) details_btn: gtk::ToggleButton,
    pub(super) menu_btn: gtk::Button,
}

/// The photo strip, its scroller, and the overlay holding the spinner.
pub(crate) struct PictureArea {
    pub(super) strip: super::strip::PhotoStrip,
    pub(super) scrolled: gtk::ScrolledWindow,
    pub(super) overlay: gtk::Overlay,
    pub(super) loader: gtk::Revealer,
    pub(super) ocr: ocr::OcrLayer,
    pub(super) ocr_copy: gtk::Button,
    /// Face boxes: all of them in face mode, otherwise the one pointed at. They pan and zoom
    /// with the photo, like the OCR boxes.
    pub(super) faces: crate::library::photo_canvas::BoxLayer,
    /// The name of the face pointed at, by its box.
    pub(super) face_label: gtk::Label,
    /// The crop box in edit mode.
    pub(super) crop: crate::library::photo_canvas::BoxLayer,
    /// The areas edit mode's Redact hides.
    pub(super) redact: crate::library::photo_canvas::BoxLayer,
}

/// "Preview unavailable" card shown over the picture when nothing can be decoded.
pub(crate) struct UnavailableCard {
    pub(super) overlay: gtk::Revealer,
    pub(super) filename: gtk::Label,
    pub(super) mime: gtk::Label,
    pub(super) open: gtk::Button,
}

/// Zoom controls, resolution toggle, and download button under the picture.
pub(crate) struct ActionBar {
    pub(super) bar: gtk::Box,
    pub(super) ocr_toggle: gtk::ToggleButton,
    /// Shown per photo; `edit_slot` hides it on narrow windows, where the ⋯ menu has it.
    pub(super) edit: gtk::Button,
    pub(super) edit_slot: gtk::Box,
    pub(super) resolution_toggle: gtk::ToggleButton,
    pub(super) download: gtk::Button,
    pub(super) open_with: gtk::Button,
    pub(super) favorite: gtk::Button,
    pub(super) archive: gtk::Button,
    pub(super) trash: gtk::Button,
    pub(super) zoom_group: gtk::Box,
    pub(super) zoom_in: gtk::Button,
    pub(super) zoom_out: gtk::Button,
    pub(super) zoom_reset: gtk::Button,
}

/// Sidebar with the filename, sync summary, and EXIF groups.
pub(crate) struct DetailsPane {
    pub(super) pane: gtk::ScrolledWindow,
    pub(super) filename: gtk::Label,
    pub(super) summary: gtk::Label,
    pub(super) created: gtk::Label,
    pub(super) description: gtk::Box,
    pub(super) edit_btn: gtk::Button,
    pub(super) album_btn: gtk::Button,
    pub(super) rating: rating::RatingSection,
    pub(super) people: people::PeopleSection,
    pub(super) tags: TagSection,
    pub(super) albums: albums::AlbumsSection,
    pub(super) activity: activity::ActivitySection,
    pub(super) loading: gtk::Label,
    pub(super) exif: gtk::Box,
}

pub(crate) fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build()
}

pub(crate) fn build_header(ui: &Rc<LibraryWindowUi>, initial_filename: &str) -> LightboxHeader {
    let page = libadwaita::NavigationPage::builder()
        .title(truncate_filename(initial_filename, lightbox_title_cap(ui)))
        .can_pop(true)
        .build();
    let toolbar = libadwaita::ToolbarView::builder().build();
    let header = libadwaita::HeaderBar::builder()
        .show_back_button(false)
        .build();
    let back_btn = icon_button("mimick-library-symbolic", &gettext("Back to library"));
    back_btn.connect_clicked(clone!(
        #[strong]
        ui,
        move |_| {
            ui.nav.pop();
        }
    ));
    let prev_btn = icon_button("go-previous-symbolic", &gettext("Previous (Left)"));
    let next_btn = icon_button("go-next-symbolic", &gettext("Next (Right)"));
    let details_btn = gtk::ToggleButton::builder()
        .icon_name("dialog-information-symbolic")
        .tooltip_text(gettext("Toggle details (I)"))
        .active(false)
        .build();
    header.pack_start(&back_btn);
    header.pack_start(&prev_btn);
    header.pack_start(&next_btn);
    // The same menu as right-click and long-press, so its actions have a visible entry point.
    let menu_btn = icon_button("view-more-symbolic", &gettext("More (Shift+F10)"));
    header.pack_end(&menu_btn);
    header.pack_end(&details_btn);
    toolbar.add_top_bar(&header);
    page.set_child(Some(&toolbar));
    LightboxHeader {
        page,
        toolbar,
        bar: header,
        prev_btn,
        next_btn,
        details_btn,
        menu_btn,
    }
}

type BoxLayer = crate::library::photo_canvas::BoxLayer;

/// The face, crop, and redaction boxes over the photo. They take input through the scroller,
/// as OCR does, and only while their mode is on. Faces stay up to outline the one pointed at;
/// edit mode's boxes show only while editing.
fn box_layers(content: &gtk::Overlay) -> (BoxLayer, BoxLayer, BoxLayer) {
    let (faces, crop, redact) = (BoxLayer::new(), BoxLayer::new(), BoxLayer::new());
    for layer in [&faces, &crop, &redact] {
        layer.area.set_can_target(false);
        content.add_overlay(&layer.area);
    }
    crop.area.set_visible(false);
    redact.area.set_visible(false);
    (faces, crop, redact)
}

pub(crate) fn build_picture_area() -> PictureArea {
    let strip = super::strip::PhotoStrip::new();
    let ocr = ocr::OcrLayer::new();
    let content = gtk::Overlay::builder().child(&strip).build();
    content.add_overlay(&ocr.area);
    let (faces, crop, redact) = box_layers(&content);
    let face_label = super::face_people::face_label(&content, &faces);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&content)
        .vexpand(true)
        .hexpand(true)
        .kinetic_scrolling(false)
        .min_content_width(120)
        .build();

    // Spinner overlay: a centered Mimick app icon that rotates while a
    // full-resolution texture is being fetched / decoded. Hidden by default;
    // `load_into_picture` reveals it after a short delay.
    let loader_icon = gtk::Image::builder()
        .icon_name("dev.nicx.mimick")
        .pixel_size(72)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["mimick-loader-icon"])
        .build();
    let loader = crossfade_overlay(&loader_icon, gtk::Align::Center);
    let overlay = gtk::Overlay::builder().build();
    overlay.set_child(Some(&scrolled));
    overlay.add_overlay(&loader);
    let ocr_copy = ocr::copy_button();
    overlay.add_overlay(&ocr_copy);
    PictureArea {
        strip,
        scrolled,
        overlay,
        loader,
        ocr,
        ocr_copy,
        faces,
        face_label,
        crop,
        redact,
    }
}

pub(crate) fn build_unavailable_card() -> UnavailableCard {
    let title = gtk::Label::builder()
        .label(gettext("Preview unavailable"))
        .css_classes(["title-3"])
        .build();
    let filename = gtk::Label::builder()
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(42)
        .build();
    let mime = gtk::Label::builder().css_classes(["dim-label"]).build();
    let open = gtk::Button::builder()
        .label(gettext("Open in external app"))
        .css_classes(["suggested-action"])
        .build();
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["mimick-preview-unavailable"])
        .build();
    card.append(&title);
    card.append(&filename);
    card.append(&mime);
    card.append(&open);
    UnavailableCard {
        overlay: crossfade_overlay(&card, gtk::Align::Fill),
        filename,
        mime,
        open,
    }
}

// Video poster badge: clickable play icon shown over the still thumbnail
// when the current asset is a video; clicking starts in-app playback.
pub(crate) fn build_video_badge() -> gtk::Button {
    let icon = gtk::Image::builder()
        .icon_name("mimick-video-symbolic")
        .pixel_size(72)
        .css_classes(vec!["mimick-video-badge".to_string()])
        .build();
    gtk::Button::builder()
        .child(&icon)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("Play video (Space)"))
        .css_classes(vec!["circular".to_string(), "flat".to_string()])
        .visible(false)
        .build()
}

pub(crate) fn build_action_bar(initial_full: bool) -> ActionBar {
    let (resolution_toggle, ocr_toggle) = view_toggles(initial_full);
    let edit = icon_button("mimick-adjust-symbolic", &gettext("Edit Photo"));
    let edit_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    edit_slot.append(&edit);
    let download = icon_button("mimick-download-symbolic", &gettext("Download asset"));
    let open_with = icon_button("document-open-symbolic", &gettext("Open with…"));
    let trash = icon_button("user-trash-symbolic", &gettext("Move to trash (Delete)"));
    let favorite = icon_button(heart_icon(false), &heart_tooltip(false));
    let archive = icon_button(archive_icon(false), &archive_tooltip(false));
    let (zoom_group, zoom_out, zoom_reset, zoom_in) = build_zoom_group();
    let bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .build();
    let spacer = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .build();
    bar.append(&zoom_group);
    bar.append(&spacer);
    bar.append(&ocr_toggle);
    bar.append(&edit_slot);
    bar.append(&resolution_toggle);
    bar.append(&favorite);
    bar.append(&archive);
    bar.append(&download);
    bar.append(&open_with);
    bar.append(&trash);
    ActionBar {
        bar,
        ocr_toggle,
        edit,
        edit_slot,
        resolution_toggle,
        download,
        open_with,
        favorite,
        archive,
        trash,
        zoom_group,
        zoom_in,
        zoom_out,
        zoom_reset,
    }
}

/// The original/preview toggle, and the recognized-text toggle (hidden until there's text).
fn view_toggles(initial_full: bool) -> (gtk::ToggleButton, gtk::ToggleButton) {
    let resolution_toggle = gtk::ToggleButton::builder()
        .label(if initial_full {
            gettext("Raw")
        } else {
            gettext("Prev")
        })
        .tooltip_text(gettext("Toggle preview vs original full-resolution image"))
        .active(initial_full)
        .build();
    let ocr_toggle = gtk::ToggleButton::builder()
        .icon_name("insert-text-symbolic")
        .tooltip_text(gettext("Show recognized text (T)"))
        .visible(false)
        .build();
    (resolution_toggle, ocr_toggle)
}

fn build_zoom_group() -> (gtk::Box, gtk::Button, gtk::Button, gtk::Button) {
    let zoom_out = icon_button("zoom-out-symbolic", &gettext("Zoom out (Ctrl+-)"));
    let zoom_in = icon_button("zoom-in-symbolic", &gettext("Zoom in (Ctrl++)"));
    let zoom_reset = gtk::Button::builder()
        .label("100%")
        .tooltip_text(gettext("Reset zoom (Ctrl+0)"))
        .build();
    let group = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(vec!["linked".to_string()])
        .build();
    group.append(&zoom_out);
    group.append(&zoom_reset);
    group.append(&zoom_in);
    (group, zoom_out, zoom_reset, zoom_in)
}

pub(crate) fn build_details_pane() -> DetailsPane {
    let inner = details_column();
    let pane = details_scroller(&inner);
    let (filename, summary, created, edit_btn, album_btn) = build_detail_actions(&inner);
    let (loading, exif) = build_exif_area();
    let description = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .visible(false)
        .build();
    let rating = rating::RatingSection::new();
    let people = people::PeopleSection::new();
    let tags = TagSection::new();
    let albums = albums::AlbumsSection::new();
    let activity = activity::ActivitySection::new();
    let sections: [&gtk::Widget; 8] = [
        loading.upcast_ref(),
        description.upcast_ref(),
        rating.root.upcast_ref(),
        people.root.upcast_ref(),
        exif.upcast_ref(),
        albums.root.upcast_ref(),
        tags.root.upcast_ref(),
        activity.root.upcast_ref(),
    ];
    for section in sections {
        inner.append(section);
    }
    DetailsPane {
        pane,
        filename,
        summary,
        created,
        description,
        edit_btn,
        album_btn,
        rating,
        people,
        tags,
        albums,
        activity,
        loading,
        exif,
    }
}

fn build_detail_actions(
    inner: &gtk::Box,
) -> (gtk::Label, gtk::Label, gtk::Label, gtk::Button, gtk::Button) {
    let filename = details_text_label();
    filename.add_css_class("title-3");
    let summary = details_text_label();
    let created = details_text_label();
    let facts = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    facts.append(&summary);
    facts.append(&created);
    let edit_btn = gtk::Button::builder().label(gettext("Edit Info…")).build();
    let album_btn = gtk::Button::builder()
        .label(gettext("Add to Album…"))
        .build();
    let edit_row = libadwaita::WrapBox::builder()
        .child_spacing(6)
        .line_spacing(6)
        .halign(gtk::Align::Start)
        .build();
    edit_row.append(&edit_btn);
    edit_row.append(&album_btn);
    inner.append(&filename);
    inner.append(&facts);
    inner.append(&edit_row);
    (filename, summary, created, edit_btn, album_btn)
}

fn build_exif_area() -> (gtk::Label, gtk::Box) {
    let loading = gtk::Label::builder()
        .xalign(0.0)
        .label(gettext("Loading details…"))
        .css_classes(vec!["dim-label".to_string()])
        .build();
    let exif = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .visible(false)
        .build();
    (loading, exif)
}

/// The details pane's sections, top to bottom.
fn details_column() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .margin_top(14)
        .margin_bottom(14)
        .margin_start(10)
        .margin_end(10)
        .build()
}

fn details_scroller(inner: &gtk::Box) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(inner)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(false)
        .min_content_width(180)
        .max_content_width(320)
        .css_classes(vec!["mimick-details-pane".to_string()])
        .build()
}

pub(crate) fn build_viewer(picture: &PictureArea, actions: &ActionBar) -> gtk::Box {
    let viewer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .build();
    set_viewer_margins(&viewer, false);
    viewer.append(&picture.overlay);
    viewer.append(&actions.bar);
    viewer
}

pub(crate) fn set_viewer_margins(viewer: &gtk::Box, fullscreen: bool) {
    let (edge, bottom) = if fullscreen { (0, 0) } else { (4, 8) };
    viewer.set_margin_top(edge);
    viewer.set_margin_start(edge);
    viewer.set_margin_end(edge);
    viewer.set_margin_bottom(bottom);
}

/// Picture viewer with the details pane as a sidebar that follows the window's narrow layout.
pub(crate) fn build_split_view(
    ui: &LibraryWindowUi,
    viewer: &gtk::Box,
    details: &DetailsPane,
    details_btn: &gtk::ToggleButton,
) -> libadwaita::OverlaySplitView {
    let body = libadwaita::OverlaySplitView::builder()
        .sidebar_position(gtk::PackType::End)
        .show_sidebar(false)
        .collapsed(ui.split.is_collapsed())
        .enable_show_gesture(true)
        .enable_hide_gesture(true)
        .min_sidebar_width(180.0)
        .max_sidebar_width(320.0)
        .sidebar_width_fraction(0.4)
        .build();
    body.set_content(Some(viewer));
    body.set_sidebar(Some(&details.pane));
    details_btn
        .bind_property("active", &body, "show-sidebar")
        .sync_create()
        .bidirectional()
        .build();
    ui.split
        .bind_property("collapsed", &body, "collapsed")
        .sync_create()
        .build();
    body
}
