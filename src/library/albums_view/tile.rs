//! Album cover tile and thumbnail loading.

use std::sync::Arc;

use gtk::prelude::*;

use crate::api_client::{LibraryAlbum, ThumbnailSize};
use crate::app_context::AppContext;

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
    let (thumb, picture) = build_album_cover();
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

fn build_album_cover() -> (gtk::Overlay, gtk::Picture) {
    // The spacer fixes the overlay height so portrait covers cannot expand a row.
    let thumb = gtk::Overlay::builder()
        .overflow(gtk::Overflow::Hidden)
        .css_classes(vec!["mimick-explore-tile".to_string()])
        .build();
    let spacer = gtk::Box::builder()
        .css_classes(vec!["mimick-explore-spacer".to_string()])
        .build();
    let picture = gtk::Picture::builder()
        .can_shrink(true)
        .content_fit(gtk::ContentFit::Cover)
        .build();
    thumb.set_child(Some(&spacer));
    thumb.add_overlay(&picture);
    (thumb, picture)
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
        .label(format!(
            "{} item{}",
            album.asset_count,
            if album.asset_count == 1 { "" } else { "s" }
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
    if let Some(texture) = ctx
        .thumbnail_cache
        .get_cached(&asset_id, ThumbnailSize::Thumbnail)
    {
        picture.set_paintable(Some(&texture));
        return;
    }
    picture.add_css_class("mimick-thumb-reveal");
    picture.add_css_class("mimick-thumb-pending");
    glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
        glib::MainContext::default().spawn_local(async move {
            if let Ok(texture) = ctx
                .thumbnail_cache
                .load_thumbnail(&asset_id, ThumbnailSize::Thumbnail)
                .await
            {
                picture.set_paintable(Some(&texture));
            }
            picture.remove_css_class("mimick-thumb-pending");
        });
    });
}
