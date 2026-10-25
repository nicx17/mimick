use std::rc::Rc;

use gtk::prelude::*;

use super::Lightbox;
use crate::api_client::ExifInfo;
use crate::library::actions::TrashTarget;
use crate::library::map::{self, GeoPoint};

impl Lightbox {
    pub(super) fn set_location_current(self: &Rc<Self>) {
        let Some(item) = self.current_item() else {
            return;
        };
        let Some(target) = TrashTarget::from_item(&item) else {
            return;
        };
        let lb = self.clone();
        map::set_location_for_asset(
            self.ui.clone(),
            target.remote_id().to_string(),
            item.property("filename"),
            Rc::new(move || lb.render()),
        );
    }

    /// Mini map under the location rows when the asset has GPS coordinates.
    pub(super) fn show_location_map(
        &self,
        group: Option<libadwaita::PreferencesGroup>,
        exif: &ExifInfo,
    ) {
        if let (Some(group), Some(point)) = (group, exif_point(exif)) {
            map::attach_minimap(&self.ui, &group, point);
        }
    }
}

fn exif_point(exif: &ExifInfo) -> Option<GeoPoint> {
    GeoPoint::new(exif.latitude?, exif.longitude?)
}

/// Copies "lat, lon" in the format the Set Location dialog pastes.
pub(super) fn copy_coordinates_button(lat: f64, lon: f64) -> gtk::Button {
    let text = GeoPoint { lat, lon }.display();
    let button = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("Copy coordinates")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    button.connect_clicked(move |button| button.clipboard().set_text(&text));
    button
}
