//! The card over the map while markers load, when there are none, or when loading failed.

use gettextrs::gettext;
use gtk::prelude::*;

use super::view::MapView;

/// What the card says; `Hidden` takes it away.
pub(super) enum Status<'a> {
    Loading,
    Empty,
    Error(&'a str),
    Hidden,
}

pub(super) fn status_page() -> libadwaita::StatusPage {
    libadwaita::StatusPage::builder()
        .icon_name("mark-location-symbolic")
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .can_target(false)
        .css_classes(["card", "compact", "mimick-map-status"])
        .visible(false)
        .build()
}

pub(super) fn show_status(view: &MapView, status: Status<'_>) {
    let (title, description) = match status {
        Status::Loading => (&gettext("Loading Map…"), String::new()),
        Status::Empty => (
            &gettext("No Photos With a Location"),
            gettext("Photos with GPS data, or a location you set, appear here"),
        ),
        Status::Error(err) => (&gettext("Could Not Load the Map"), err.to_string()),
        Status::Hidden => {
            view.status.set_visible(false);
            return;
        }
    };
    view.status.set_title(title);
    view.status
        .set_description((!description.is_empty()).then_some(description.as_str()));
    view.status.set_visible(true);
}
