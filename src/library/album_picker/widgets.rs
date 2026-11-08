//! The picker's dialog, its "no matches" row, and album covers.

use std::sync::Arc;

use gettextrs::gettext;
use gtk::prelude::*;

use crate::api_client::LibraryAlbum;
use crate::app_context::AppContext;
use crate::library::albums_view::spawn_thumbnail;
use crate::library::header_format::count_label;

const COVER_SIZE: i32 = 40;

/// The dialog around the filter and list; `add` sits in a bar along the bottom.
pub(super) fn picker_dialog(
    filter: &gtk::SearchEntry,
    list: &gtk::ListBox,
    (title, count): (&str, usize),
    add: Option<&gtk::Button>,
) -> libadwaita::Dialog {
    let header = libadwaita::HeaderBar::builder()
        .title_widget(&libadwaita::WindowTitle::new(
            title,
            &count_label(count as u64),
        ))
        .build();
    let search = gtk::Box::builder()
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(6)
        .build();
    search.append(filter);
    let content = gtk::Box::builder()
        .margin_top(6)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(list);
    let clamp = libadwaita::Clamp::builder()
        .maximum_size(600)
        .child(&content)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    let toolbar = libadwaita::ToolbarView::builder()
        .content(&scroller)
        .build();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&search);
    if let Some(add) = add {
        toolbar.add_bottom_bar(&add_bar(add));
    }
    libadwaita::Dialog::builder()
        .title(title)
        .content_width(400)
        .content_height(560)
        .child(&toolbar)
        .build()
}

/// A wide button on its own bar, so it's easy to reach on a phone and doesn't crowd the
/// title on narrow windows; the bar shows only while the button does.
fn add_bar(add: &gtk::Button) -> gtk::Box {
    add.set_halign(gtk::Align::Center);
    add.add_css_class("pill");
    let bar = gtk::Box::builder()
        .margin_top(6)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .halign(gtk::Align::Center)
        .build();
    bar.append(add);
    add.bind_property("visible", &bar, "visible")
        .sync_create()
        .build();
    bar
}

pub(super) fn no_matches_row() -> gtk::ListBoxRow {
    let label = gtk::Label::builder()
        .label(gettext("No matching albums"))
        .margin_top(12)
        .margin_bottom(12)
        .css_classes(["dim-label"])
        .build();
    gtk::ListBoxRow::builder()
        .child(&label)
        .activatable(false)
        .selectable(false)
        .build()
}

fn cover_frame() -> gtk::Overlay {
    let spacer = gtk::Box::builder()
        .width_request(COVER_SIZE)
        .height_request(COVER_SIZE)
        .build();
    gtk::Overlay::builder()
        .child(&spacer)
        .valign(gtk::Align::Center)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["mimick-picker-cover"])
        .build()
}

pub(super) fn cover_placeholder(icon: &str) -> gtk::Overlay {
    let frame = cover_frame();
    frame.add_overlay(&gtk::Image::from_icon_name(icon));
    frame
}

pub(in crate::library) fn album_cover(ctx: &Arc<AppContext>, album: &LibraryAlbum) -> gtk::Overlay {
    match album.thumbnail_asset_id.clone() {
        Some(thumb_id) => asset_cover(ctx, thumb_id),
        None => cover_placeholder("folder-pictures-symbolic"),
    }
}

pub(in crate::library) fn asset_cover(ctx: &Arc<AppContext>, asset_id: String) -> gtk::Overlay {
    let frame = cover_frame();
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .build();
    frame.add_overlay(&picture);
    spawn_thumbnail(ctx.clone(), asset_id, picture);
    frame
}
