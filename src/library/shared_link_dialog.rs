//! Create or edit a shared link: description, expiry, password, and what visitors may do.

use std::rc::Rc;

use chrono::{DateTime, Duration, Utc};
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{SharedLink, SharedLinkOptions, SharedLinkTarget, share_url};

use super::LibraryWindowUi;
use super::context_menu::show_alert_dialog;

/// Expiry presets after the optional "Keep current" entry.
const EXPIRY_PRESETS: [(&str, Option<i64>); 5] = [
    ("Never", None),
    ("1 hour", Some(1)),
    ("1 day", Some(24)),
    ("7 days", Some(24 * 7)),
    ("30 days", Some(24 * 30)),
];

/// Labels for the expiry dropdown; editing a link that expires adds "Keep current" first.
fn expiry_labels(current: Option<&str>) -> Vec<String> {
    let keep = current.map(|at| format!("Keep current ({})", format_expiry(at)));
    keep.into_iter()
        .chain(EXPIRY_PRESETS.iter().map(|(label, _)| label.to_string()))
        .collect()
}

/// Expiry for the selected dropdown entry: RFC3339, or `None` for never.
fn expiry_for_choice(index: u32, current: Option<&str>, now: DateTime<Utc>) -> Option<String> {
    let preset_index = match current {
        Some(at) if index == 0 => return Some(at.to_string()),
        Some(_) => index as usize - 1,
        None => index as usize,
    };
    let hours = EXPIRY_PRESETS
        .get(preset_index)
        .and_then(|(_, hours)| *hours)?;
    Some((now + Duration::hours(hours)).to_rfc3339())
}

/// Local short date for an expiry timestamp ("9 Oct 2026, 14:30").
pub(super) fn format_expiry(at: &str) -> String {
    DateTime::parse_from_rfc3339(at)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%-d %b %Y, %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| at.to_string())
}

/// What the dialog is for: a new link, or editing an existing one.
pub(super) enum LinkDialogMode {
    Create(SharedLinkTarget),
    Edit(Box<SharedLink>),
}

/// The dialog's editable rows.
struct LinkForm {
    description: libadwaita::EntryRow,
    expiry: libadwaita::ComboRow,
    password: libadwaita::PasswordEntryRow,
    allow_download: libadwaita::SwitchRow,
    allow_upload: libadwaita::SwitchRow,
    show_metadata: libadwaita::SwitchRow,
    /// Expiry of the link being edited, if it has one ("Keep current").
    current_expiry: Option<String>,
}

fn switch(title: &str, subtitle: &str, active: bool) -> libadwaita::SwitchRow {
    libadwaita::SwitchRow::builder()
        .title(title)
        .subtitle(subtitle)
        .active(active)
        .build()
}

fn targets_album(mode: &LinkDialogMode) -> bool {
    match mode {
        LinkDialogMode::Create(target) => matches!(target, SharedLinkTarget::Album(_)),
        LinkDialogMode::Edit(link) => link.link_type == "ALBUM",
    }
}

impl LinkForm {
    fn new(mode: &LinkDialogMode) -> Self {
        let existing = match mode {
            LinkDialogMode::Edit(link) => Some(link),
            LinkDialogMode::Create(_) => None,
        };
        let current_expiry = existing.and_then(|l| l.expires_at.clone());
        let is_album = targets_album(mode);
        let labels = expiry_labels(current_expiry.as_deref());
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let form = Self {
            description: libadwaita::EntryRow::builder()
                .title("Description")
                .text(
                    existing
                        .and_then(|l| l.description.clone())
                        .unwrap_or_default(),
                )
                .build(),
            expiry: libadwaita::ComboRow::builder()
                .title("Expires")
                .model(&gtk::StringList::new(&label_refs))
                .build(),
            password: libadwaita::PasswordEntryRow::builder()
                .title(if existing.is_some_and(|l| l.password.is_some()) {
                    "Password (leave empty to keep)"
                } else {
                    "Password (optional)"
                })
                .build(),
            allow_download: switch(
                "Allow downloads",
                "Visitors can download originals",
                existing.is_none_or(|l| l.allow_download),
            ),
            allow_upload: switch(
                "Allow uploads",
                "Visitors can add photos to the album",
                existing.is_some_and(|l| l.allow_upload),
            ),
            show_metadata: switch(
                "Show metadata",
                "Visitors can see EXIF details and location",
                existing.is_none_or(|l| l.show_metadata),
            ),
            current_expiry,
        };
        // Uploading only makes sense into an album.
        form.allow_upload.set_visible(is_album);
        form
    }

    fn group(&self) -> libadwaita::PreferencesGroup {
        let group = libadwaita::PreferencesGroup::new();
        group.add(&self.description);
        group.add(&self.expiry);
        group.add(&self.password);
        group.add(&self.allow_download);
        group.add(&self.allow_upload);
        group.add(&self.show_metadata);
        group
    }

    fn options(&self) -> SharedLinkOptions {
        SharedLinkOptions {
            description: self.description.text().trim().to_string(),
            password: self.password.text().to_string(),
            expires_at: expiry_for_choice(
                self.expiry.selected(),
                self.current_expiry.as_deref(),
                Utc::now(),
            ),
            allow_download: self.allow_download.is_active(),
            allow_upload: self.allow_upload.is_visible() && self.allow_upload.is_active(),
            show_metadata: self.show_metadata.is_active(),
        }
    }
}

/// Open the create/edit dialog. `on_saved` runs after Immich accepts the change.
pub(super) fn present(ui: Rc<LibraryWindowUi>, mode: LinkDialogMode, on_saved: Rc<dyn Fn()>) {
    let form = Rc::new(LinkForm::new(&mode));
    let title = match &mode {
        LinkDialogMode::Create(_) => "Create Share Link",
        LinkDialogMode::Edit(_) => "Edit Share Link",
    };
    let (dialog, save_btn) = dialog_shell(title, &form.group());
    let mode = Rc::new(mode);
    let (ui_save, dialog_ref) = (ui.clone(), dialog.clone());
    save_btn.connect_clicked(move |btn| {
        btn.set_sensitive(false);
        save(
            ui_save.clone(),
            mode.clone(),
            form.options(),
            dialog_ref.clone(),
            btn.clone(),
            on_saved.clone(),
        );
    });
    dialog.present(Some(&ui.window));
}

fn dialog_shell(
    title: &str,
    group: &libadwaita::PreferencesGroup,
) -> (libadwaita::Dialog, gtk::Button) {
    let save_btn = gtk::Button::builder()
        .label("Save")
        .css_classes(["suggested-action"])
        .build();
    let header = libadwaita::HeaderBar::new();
    header.pack_end(&save_btn);
    let page = libadwaita::PreferencesPage::new();
    page.add(group);
    let toolbar = libadwaita::ToolbarView::builder().content(&page).build();
    toolbar.add_top_bar(&header);
    let dialog = libadwaita::Dialog::builder()
        .title(title)
        .content_width(460)
        .child(&toolbar)
        .build();
    (dialog, save_btn)
}

fn save(
    ui: Rc<LibraryWindowUi>,
    mode: Rc<LinkDialogMode>,
    options: SharedLinkOptions,
    dialog: libadwaita::Dialog,
    save_btn: gtk::Button,
    on_saved: Rc<dyn Fn()>,
) {
    glib::MainContext::default().spawn_local(async move {
        let api = &ui.ctx.api_client;
        let result = match mode.as_ref() {
            LinkDialogMode::Create(target) => api.create_shared_link(target, &options).await,
            LinkDialogMode::Edit(link) => api.edit_shared_link(link, &options).await,
        };
        match result {
            Ok(link) => {
                dialog.close();
                if matches!(mode.as_ref(), LinkDialogMode::Create(_)) {
                    copy_link_url(&ui, &link, "Share Link Created").await;
                }
                on_saved();
            }
            Err(err) => {
                save_btn.set_sensitive(true);
                show_alert_dialog(&ui, "Could Not Save Share Link", &err);
            }
        }
    });
}

/// Copy a link's public URL to the clipboard and confirm with `heading`.
pub(super) async fn copy_link_url(ui: &LibraryWindowUi, link: &SharedLink, heading: &str) {
    match copy_to_clipboard(ui, link).await {
        Ok(url) => show_alert_dialog(ui, heading, &format!("Copied to the clipboard:\n{url}")),
        Err(err) => show_alert_dialog(ui, "Could Not Build Link", &err),
    }
}

pub(super) async fn copy_to_clipboard(
    ui: &LibraryWindowUi,
    link: &SharedLink,
) -> Result<String, String> {
    let url = share_url(&ui.ctx.api_client.shared_link_base().await?, link);
    if let Some(display) = gdk4::Display::default() {
        display.clipboard().set_text(&url);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn expiry_labels_add_keep_current_only_when_editing_an_expiring_link() {
        assert_eq!(expiry_labels(None)[0], "Never");
        assert_eq!(expiry_labels(None).len(), 5);
        let labels = expiry_labels(Some("2026-11-01T00:00:00Z"));
        assert!(labels[0].starts_with("Keep current ("));
        assert_eq!(labels.len(), 6);
    }

    #[test]
    fn expiry_for_choice_maps_presets_to_timestamps() {
        assert_eq!(expiry_for_choice(0, None, now()), None);
        assert_eq!(
            expiry_for_choice(2, None, now()).as_deref(),
            Some("2026-10-02T12:00:00+00:00")
        );
        assert_eq!(expiry_for_choice(99, None, now()), None);
    }

    #[test]
    fn expiry_for_choice_keeps_or_shifts_when_editing() {
        let current = Some("2026-11-01T00:00:00Z");
        assert_eq!(
            expiry_for_choice(0, current, now()).as_deref(),
            Some("2026-11-01T00:00:00Z")
        );
        assert_eq!(expiry_for_choice(1, current, now()), None);
        assert_eq!(
            expiry_for_choice(2, current, now()).as_deref(),
            Some("2026-10-01T13:00:00+00:00")
        );
    }

    #[test]
    fn format_expiry_falls_back_to_raw_text() {
        assert_eq!(format_expiry("soon"), "soon");
        assert!(format_expiry("2026-10-09T12:00:00Z").contains("Oct 2026"));
    }
}
