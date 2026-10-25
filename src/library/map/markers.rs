//! Marker widgets for the Map page: count bubbles, photo thumbnails, and the "you are here" dot.

use std::sync::Arc;

use gtk::prelude::*;
use shumate::prelude::*;

use crate::api_client::{MapMarker, ThumbnailSize};
use crate::library::LibraryWindowUi;

use super::cluster::{self, Cluster};
use super::coords::GeoPoint;

const PHOTO_PX: i32 = 48;

/// The photos a marker stands for, without holding a whole zoom level for a single photo.
#[derive(Clone)]
pub enum Members {
    Photo([usize; 1]),
    Cluster(Arc<Vec<Cluster>>, usize),
}

impl Members {
    pub fn as_slice(&self) -> &[usize] {
        match self {
            Self::Photo(single) => single,
            Self::Cluster(clusters, index) => &clusters[*index].members,
        }
    }
}

/// A round button showing a cluster's photo count, the same size for every cluster.
pub fn bubble(count: usize) -> gtk::Button {
    gtk::Button::builder()
        .label(compact_count(count))
        .css_classes(["mimick-map-cluster"])
        .build()
}

/// A round thumbnail of one photo, cropped to a circle; the image loads in the background.
pub fn photo(ui: &LibraryWindowUi, asset_id: &str) -> gtk::Button {
    let avatar = libadwaita::Avatar::builder()
        .size(PHOTO_PX)
        .icon_name("image-x-generic-symbolic")
        .build();
    load_thumbnail(ui, asset_id, &avatar);
    gtk::Button::builder()
        .child(&avatar)
        .css_classes(["mimick-map-photo"])
        .build()
}

fn load_thumbnail(ui: &LibraryWindowUi, asset_id: &str, avatar: &libadwaita::Avatar) {
    let cache = ui.ctx.thumbnail_cache.clone();
    if let Some(texture) = cache.peek_cached(asset_id, ThumbnailSize::Thumbnail) {
        avatar.set_custom_image(Some(&texture));
        return;
    }
    let (id, weak) = (asset_id.to_string(), avatar.downgrade());
    glib::MainContext::default().spawn_local(async move {
        let gone = || weak.upgrade().is_none();
        let loaded = cache
            .load_thumbnail_cancellable(&id, ThumbnailSize::Thumbnail, gone)
            .await;
        if let (Ok(texture), Some(avatar)) = (loaded, weak.upgrade()) {
            avatar.set_custom_image(Some(&texture));
        }
    });
}

/// Tooltip naming the place, worked out only when the pointer rests on the marker.
pub fn place_tooltip(button: &gtk::Button, markers: Arc<Vec<MapMarker>>, members: Members) {
    button.set_has_tooltip(true);
    button.connect_query_tooltip(move |_, _, _, _, tooltip| {
        let members = members.as_slice();
        let place = cluster::area_label(&markers, members);
        tooltip.set_text(Some(&marker_tooltip(members.len(), place.as_deref())));
        true
    });
}

/// Blue dot for the position from the location portal.
pub fn here(point: GeoPoint) -> shumate::Marker {
    let dot = gtk::Box::builder()
        .css_classes(["mimick-map-here"])
        .tooltip_text("You are here")
        .build();
    let marker = shumate::Marker::builder().child(&dot).build();
    marker.set_location(point.lat, point.lon);
    marker
}

fn marker_tooltip(count: usize, place: Option<&str>) -> String {
    let noun = if count == 1 { "photo" } else { "photos" };
    match place {
        Some(place) => format!("{count} {noun} in {place}"),
        None => format!("{count} {noun}"),
    }
}

/// Short count for a cluster badge: 999, 1.2k, 25k.
fn compact_count(count: usize) -> String {
    match count {
        0..=999 => count.to_string(),
        1_000..=9_999 => format!("{:.1}k", (count / 100) as f64 / 10.0),
        _ => format!("{}k", count / 1_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_compact_on_badges() {
        assert_eq!(compact_count(7), "7");
        assert_eq!(compact_count(999), "999");
        assert_eq!(compact_count(1_250), "1.2k");
        assert_eq!(compact_count(9_999), "9.9k");
        assert_eq!(compact_count(25_400), "25k");
    }

    #[test]
    fn tooltips_name_the_place() {
        assert_eq!(marker_tooltip(1, Some("Lisbon")), "1 photo in Lisbon");
        assert_eq!(marker_tooltip(12, None), "12 photos");
    }
}
