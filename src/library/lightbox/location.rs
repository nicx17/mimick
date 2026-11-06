use gettextrs::gettext;
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

    /// The pen on the Coordinates row, shown on the user's own photos.
    pub(super) fn set_location_button(self: &Rc<Self>) -> gtk::Button {
        let button = gtk::Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text(gettext("Set location"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let weak = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(lb) = weak.upgrade() {
                lb.set_location_current();
            }
        });
        button
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
        .tooltip_text(gettext("Copy coordinates"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    button.connect_clicked(move |button| button.clipboard().set_text(&text));
    button
}
