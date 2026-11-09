//! Settings page "Troubleshooting" section, shown whether or not the library is on.

use adw::prelude::*;
use gettextrs::gettext;
use gtk::Button;
use libadwaita as adw;

/// The section's controls, wired in `wiring`.
pub struct TroubleshootingWidgets {
    pub export_btn: Button,
    pub detailed_logs_row: adw::SwitchRow,
}

/// Adds the section: detailed logs to switch on before reproducing a problem, then Export.
pub fn build_troubleshooting_group(settings_page: &adw::PreferencesPage) -> TroubleshootingWidgets {
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Troubleshooting"))
        .build();
    settings_page.add(&group);
    let detailed_logs_row = adw::SwitchRow::builder()
        .title(gettext("Detailed Logs"))
        .subtitle(gettext(
            "Record more while you reproduce a problem, then export. The log grows faster.",
        ))
        .build();
    group.add(&detailed_logs_row);
    let export_btn = Button::builder()
        .label(gettext("Export…"))
        .valign(gtk::Align::Center)
        .build();
    let row = adw::ActionRow::builder()
        .title(gettext("Export Diagnostics"))
        .subtitle(gettext(
            "For bug reports: settings, status, and the end of the log, with private details removed.",
        ))
        .build();
    row.add_suffix(&export_btn);
    group.add(&row);
    TroubleshootingWidgets {
        export_btn,
        detailed_logs_row,
    }
}
