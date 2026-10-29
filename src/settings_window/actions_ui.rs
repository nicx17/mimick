//! Settings page "Troubleshooting" section, shown whether or not the library is on.

use adw::prelude::*;
use gtk::Button;
use libadwaita as adw;

/// Adds the section and returns its "Export…" button.
pub fn build_troubleshooting_group(settings_page: &adw::PreferencesPage) -> Button {
    let group = adw::PreferencesGroup::builder()
        .title("Troubleshooting")
        .build();
    settings_page.add(&group);
    let export_btn = Button::builder()
        .label("Export…")
        .valign(gtk::Align::Center)
        .build();
    let row = adw::ActionRow::builder()
        .title("Export Diagnostics")
        .subtitle("Save logs, status, and settings with private details removed, to attach to a bug report.")
        .build();
    row.add_suffix(&export_btn);
    group.add(&row);
    export_btn
}
