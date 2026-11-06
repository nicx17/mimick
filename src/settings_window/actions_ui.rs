//! Settings page "Troubleshooting" section, shown whether or not the library is on.

use adw::prelude::*;
use gettextrs::gettext;
use gtk::Button;
use libadwaita as adw;

/// Adds the section and returns its "Export…" button.
pub fn build_troubleshooting_group(settings_page: &adw::PreferencesPage) -> Button {
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Troubleshooting"))
        .build();
    settings_page.add(&group);
    let export_btn = Button::builder()
        .label(gettext("Export…"))
        .valign(gtk::Align::Center)
        .build();
    let row = adw::ActionRow::builder()
        .title(gettext("Export Diagnostics"))
        .subtitle(gettext("For bug reports. Private details are removed."))
        .build();
    row.add_suffix(&export_btn);
    group.add(&row);
    export_btn
}
