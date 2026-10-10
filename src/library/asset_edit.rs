//! "Edit Info" dialog: change an asset's description, capture date, and favorite flag.
//!
//! Only fields the user actually changed are sent to Immich, so an untouched
//! date never gets rewritten with a lossy local-time round trip.

use std::rc::Rc;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{AssetDetails, AssetUpdate};

use super::LibraryWindowUi;
use super::context_menu::show_alert_dialog;

/// Format shown in and accepted by the "Date taken" field (local time).
const DATE_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// Field values as shown in the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EditValues {
    description: String,
    date_text: String,
    favorite: bool,
}

impl EditValues {
    fn from_details(details: &AssetDetails) -> Self {
        let exif = details.exif_info.as_ref();
        Self {
            description: exif.and_then(|e| e.description.clone()).unwrap_or_default(),
            date_text: exif
                .and_then(|e| e.date_time_original.as_deref())
                .and_then(format_local_datetime)
                .unwrap_or_default(),
            favorite: details.is_favorite,
        }
    }
}

/// Immich stores capture times in UTC; show them in the user's local timezone.
fn format_local_datetime(iso: &str) -> Option<String> {
    let local = DateTime::parse_from_rfc3339(iso)
        .map(|dt| dt.with_timezone(&Local))
        .or_else(|_| {
            iso.parse::<DateTime<Utc>>()
                .map(|dt| dt.with_timezone(&Local))
        })
        .ok()?;
    Some(local.format(DATE_FORMAT).to_string())
}

/// Read `text` as local time and return RFC3339 with its UTC offset, which Immich accepts.
/// `None` for malformed input or a time skipped by a DST change.
fn parse_local_datetime(text: &str) -> Option<String> {
    let naive = NaiveDateTime::parse_from_str(text.trim(), DATE_FORMAT).ok()?;
    let local = Local.from_local_datetime(&naive).earliest()?;
    Some(local.to_rfc3339())
}

/// The update to send, holding only changed fields; `None` if the edited date is invalid.
fn update_from(original: &EditValues, edited: &EditValues) -> Option<AssetUpdate> {
    let date_time_original = if edited.date_text == original.date_text {
        None
    } else {
        Some(parse_local_datetime(&edited.date_text)?)
    };
    Some(AssetUpdate {
        description: (edited.description != original.description)
            .then(|| edited.description.clone()),
        date_time_original,
        is_favorite: (edited.favorite != original.favorite).then_some(edited.favorite),
    })
}

/// Fetch the asset's current info and open the edit dialog.
/// `on_saved` receives whether the capture date changed (the timeline order may move).
pub(super) fn show_edit_dialog(
    ui: Rc<LibraryWindowUi>,
    asset_id: String,
    filename: String,
    on_saved: Rc<dyn Fn(bool)>,
) {
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.fetch_asset_details(&asset_id).await {
            Ok(details) => {
                let original = EditValues::from_details(&details);
                present_edit_dialog(ui, asset_id, &filename, original, on_saved);
            }
            Err(err) => show_alert_dialog(&ui, "Could Not Load Info", &err),
        }
    });
}

/// The editable rows inside the dialog.
struct EditForm {
    group: libadwaita::PreferencesGroup,
    description_row: libadwaita::EntryRow,
    date_row: libadwaita::EntryRow,
    favorite_row: libadwaita::SwitchRow,
}

impl EditForm {
    fn new(original: &EditValues, filename: &str) -> Self {
        let description_row = libadwaita::EntryRow::builder()
            .title("Description")
            .text(&original.description)
            .build();
        let date_row = libadwaita::EntryRow::builder()
            .title("Date taken (YYYY-MM-DD HH:MM:SS)")
            .text(&original.date_text)
            .build();
        let favorite_row = libadwaita::SwitchRow::builder()
            .title("Favorite")
            .active(original.favorite)
            .build();
        let group = libadwaita::PreferencesGroup::builder()
            .description(filename)
            .build();
        group.add(&description_row);
        group.add(&date_row);
        group.add(&favorite_row);
        Self {
            group,
            description_row,
            date_row,
            favorite_row,
        }
    }

    fn values(&self) -> EditValues {
        EditValues {
            description: self.description_row.text().to_string(),
            date_text: self.date_row.text().to_string(),
            favorite: self.favorite_row.is_active(),
        }
    }
}

/// Dialog with Cancel/Save in the header bar around `form`; returns the dialog and both buttons.
fn dialog_shell(form: &EditForm) -> (libadwaita::Dialog, gtk::Button, gtk::Button) {
    let cancel_btn = gtk::Button::builder().label("Cancel").build();
    let save_btn = gtk::Button::builder()
        .label("Save")
        .css_classes(["suggested-action"])
        .build();
    let header = libadwaita::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&cancel_btn);
    header.pack_end(&save_btn);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&form.group);
    let toolbar = libadwaita::ToolbarView::builder().content(&content).build();
    toolbar.add_top_bar(&header);
    let dialog = libadwaita::Dialog::builder()
        .title("Edit Info")
        .content_width(440)
        .child(&toolbar)
        .build();
    (dialog, cancel_btn, save_btn)
}

/// Mark the date row as an error and disable Save while the text doesn't parse.
/// The untouched original text stays valid, even if it is empty (no date on record).
fn connect_date_validation(
    date_row: &libadwaita::EntryRow,
    save_btn: &gtk::Button,
    original: String,
) {
    let save_btn = save_btn.clone();
    date_row.connect_changed(move |row| {
        let text = row.text();
        let valid = text == original || parse_local_datetime(&text).is_some();
        if valid {
            row.remove_css_class("error");
        } else {
            row.add_css_class("error");
        }
        save_btn.set_sensitive(valid);
    });
}

fn present_edit_dialog(
    ui: Rc<LibraryWindowUi>,
    asset_id: String,
    filename: &str,
    original: EditValues,
    on_saved: Rc<dyn Fn(bool)>,
) {
    let form = EditForm::new(&original, filename);
    let (dialog, cancel_btn, save_btn) = dialog_shell(&form);
    connect_date_validation(&form.date_row, &save_btn, original.date_text.clone());

    let dialog_for_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dialog_for_cancel.close();
    });

    let dialog_for_save = dialog.clone();
    let ui_for_save = ui.clone();
    save_btn.connect_clicked(move |btn| {
        let Some(update) = update_from(&original, &form.values()) else {
            return;
        };
        if update == AssetUpdate::default() {
            dialog_for_save.close();
            return;
        }
        btn.set_sensitive(false);
        save_update(
            ui_for_save.clone(),
            asset_id.clone(),
            update,
            dialog_for_save.clone(),
            btn.clone(),
            on_saved.clone(),
        );
    });

    dialog.present(Some(&ui.window));
}

fn save_update(
    ui: Rc<LibraryWindowUi>,
    asset_id: String,
    update: AssetUpdate,
    dialog: libadwaita::Dialog,
    save_btn: gtk::Button,
    on_saved: Rc<dyn Fn(bool)>,
) {
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.update_asset(&asset_id, &update).await {
            Ok(()) => {
                dialog.close();
                on_saved(update.date_time_original.is_some());
            }
            Err(err) => {
                // Re-enable Save so the user can retry after fixing permissions or connectivity.
                save_btn.set_sensitive(true);
                show_alert_dialog(
                    &ui,
                    "Could Not Save Info",
                    &format!("Immich rejected the update: {err}"),
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(description: &str, date_text: &str, favorite: bool) -> EditValues {
        EditValues {
            description: description.into(),
            date_text: date_text.into(),
            favorite,
        }
    }

    #[test]
    fn edit_values_prefill_from_asset_details() {
        let details: AssetDetails = serde_json::from_value(serde_json::json!({
            "isFavorite": true,
            "exifInfo": {
                "description": "Beach day",
                "dateTimeOriginal": "2024-01-15T14:25:15.000Z"
            }
        }))
        .unwrap();
        let values = EditValues::from_details(&details);
        assert_eq!(values.description, "Beach day");
        assert!(values.favorite);
        assert_eq!(
            Some(values.date_text.clone()),
            format_local_datetime("2024-01-15T14:25:15.000Z")
        );
    }

    #[test]
    fn edit_values_are_empty_without_exif() {
        let details: AssetDetails = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(EditValues::from_details(&details), values("", "", false));
    }

    #[test]
    fn local_datetime_round_trips() {
        let rfc3339 = parse_local_datetime("2024-01-15 19:55:15").expect("valid date");
        assert_eq!(
            format_local_datetime(&rfc3339).as_deref(),
            Some("2024-01-15 19:55:15")
        );
    }

    #[test]
    fn parse_rejects_malformed_dates() {
        assert_eq!(parse_local_datetime(""), None);
        assert_eq!(parse_local_datetime("yesterday"), None);
        assert_eq!(parse_local_datetime("2024-13-01 00:00:00"), None);
        assert_eq!(parse_local_datetime("2024-01-15"), None);
    }

    #[test]
    fn format_accepts_immich_utc_timestamps() {
        assert!(format_local_datetime("2024-01-15T14:25:15.000Z").is_some());
        assert_eq!(format_local_datetime("not a date"), None);
    }

    #[test]
    fn update_holds_only_changed_fields() {
        let original = values("Old", "2024-01-15 19:55:15", false);
        let edited = values("New", "2024-01-15 19:55:15", true);
        let update = update_from(&original, &edited).expect("valid edit");
        assert_eq!(update.description.as_deref(), Some("New"));
        assert_eq!(update.date_time_original, None);
        assert_eq!(update.is_favorite, Some(true));
    }

    #[test]
    fn update_is_empty_when_nothing_changed() {
        let original = values("Same", "", true);
        assert_eq!(
            update_from(&original, &original.clone()),
            Some(AssetUpdate::default())
        );
    }

    #[test]
    fn update_rejects_an_invalid_new_date() {
        let original = values("", "2024-01-15 19:55:15", false);
        let edited = values("", "2024-01-15", false);
        assert_eq!(update_from(&original, &edited), None);
    }

    #[test]
    fn update_sends_a_changed_date_with_its_offset() {
        let original = values("", "2024-01-15 19:55:15", false);
        let edited = values("", "2024-01-16 08:00:00", false);
        let date = update_from(&original, &edited)
            .and_then(|u| u.date_time_original)
            .expect("date update");
        assert!(date.starts_with("2024-01-16T08:00:00"));
    }
}
