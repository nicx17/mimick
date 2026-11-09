//! Album cover tile and thumbnail loading.
use crate::i18n::{fill, plural_n};
use gettextrs::ngettext;

use std::sync::Arc;

use gtk::prelude::*;

use crate::api_client::LibraryAlbum;
use crate::app_context::AppContext;
use crate::library::explore_view::cover_frame;

use super::AlbumClick;

/// Build a clickable tile representing an individual album complete with cover art.
pub(super) fn album_tile(
    ctx: Arc<AppContext>,
    album: &LibraryAlbum,
    on_click: AlbumClick,
) -> gtk::Button {
    let tile_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    let (thumb, picture) = cover_frame();
    tile_box.append(&thumb);
    tile_box.append(&build_album_metadata(album));

    let button = gtk::Button::builder()
        .child(&tile_box)
        .css_classes(vec!["flat".to_string()])
        .build();
    if let Some(thumb_id) = album.thumbnail_asset_id.clone() {
        spawn_thumbnail(ctx, thumb_id, picture);
    }
    let id = album.id.clone();
    let name = album.album_name.clone();
    button.connect_clicked(move |_| on_click(&id, name.clone()));
    button
}

fn build_album_metadata(album: &LibraryAlbum) -> gtk::Box {
    let meta_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let title_label = gtk::Label::builder()
        .label(&album.album_name)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .width_chars(1)
        .max_width_chars(1)
        .css_classes(vec!["caption-heading".to_string()])
        .build();
    let count_label = gtk::Label::builder()
        .label(fill(
            ngettext("{n} item", "{n} items", plural_n(album.asset_count)),
            &[("n", &album.asset_count.to_string())],
        ))
        .xalign(1.0)
        .css_classes(vec!["caption".to_string(), "dim-label".to_string()])
        .build();
    meta_row.append(&title_label);
    meta_row.append(&count_label);
    meta_row
}

/// Asynchronously load and set the thumbnail for an album cover art picture widget.
pub fn spawn_thumbnail(ctx: Arc<AppContext>, asset_id: String, picture: gtk::Picture) {
    super::super::thumbnail_loader::load_into_picture(ctx, asset_id, picture, true);
}
