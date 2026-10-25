//! Marker widgets for the Map page: count bubbles, photo thumbnails, and the "you are here" dot.

use std::rc::Rc;

use gtk::prelude::*;
use shumate::prelude::*;

use crate::api_client::{MapMarker, ThumbnailSize};
use crate::library::LibraryWindowUi;

use super::cluster::{self, Cluster};
use super::coords::GeoPoint;

const PHOTO_PX: i32 = 44;

/// A round button showing a cluster's photo count, larger for bigger clusters.
pub fn bubble(count: usize) -> gtk::Button {
    let button = gtk::Button::builder()
        .label(compact_count(count))
        .css_classes(["mimick-map-cluster"])
        .build();
    if let Some(size) = size_class(count) {
        button.add_css_class(size);
    }
    button
}

/// A round thumbnail of one photo; the image loads in the background.
pub fn photo(ui: &LibraryWindowUi, asset_id: &str) -> gtk::Button {
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .width_request(PHOTO_PX)
        .height_request(PHOTO_PX)
        .build();
    load_thumbnail(ui, asset_id, &picture);
    gtk::Button::builder()
        .child(&picture)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["mimick-map-photo"])
        .build()
}

fn load_thumbnail(ui: &LibraryWindowUi, asset_id: &str, picture: &gtk::Picture) {
    let cache = ui.ctx.thumbnail_cache.clone();
    if let Some(texture) = cache.peek_cached(asset_id, ThumbnailSize::Thumbnail) {
        picture.set_paintable(Some(&texture));
        return;
    }
    let (id, weak) = (asset_id.to_string(), picture.downgrade());
    glib::MainContext::default().spawn_local(async move {
        if let Ok(texture) = cache.load_thumbnail(&id, ThumbnailSize::Thumbnail).await
            && let Some(picture) = weak.upgrade()
        {
            picture.set_paintable(Some(&texture));
        }
    });
}

/// Tooltip naming the place, worked out only when the pointer rests on the marker.
pub fn place_tooltip(
    button: &gtk::Button,
    markers: Rc<Vec<MapMarker>>,
    clusters: Rc<Vec<Cluster>>,
    index: usize,
) {
    button.set_has_tooltip(true);
    button.connect_query_tooltip(move |_, _, _, _, tooltip| {
        let members = &clusters[index].members;
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

fn size_class(count: usize) -> Option<&'static str> {
    match count {
        0..=9 => None,
        10..=99 => Some("mimick-map-cluster-m"),
        100..=999 => Some("mimick-map-cluster-l"),
        _ => Some("mimick-map-cluster-xl"),
    }
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
    fn bigger_clusters_get_bigger_bubbles() {
        assert_eq!(size_class(3), None);
        assert_eq!(size_class(42), Some("mimick-map-cluster-m"));
        assert_eq!(size_class(467), Some("mimick-map-cluster-l"));
        assert_eq!(size_class(3_900), Some("mimick-map-cluster-xl"));
    }

    #[test]
    fn tooltips_name_the_place() {
        assert_eq!(marker_tooltip(1, Some("Lisbon")), "1 photo in Lisbon");
        assert_eq!(marker_tooltip(12, None), "12 photos");
    }
}
