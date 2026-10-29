//! "Shared Links" page: every public link the user has made, with copy, edit, and delete.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::SharedLink;

use super::LibraryWindowUi;
use super::albums_view::spawn_thumbnail;
use super::context_menu::show_alert_dialog;
use super::shared_link_dialog::{self, LinkDialogMode, copy_to_clipboard, format_expiry};

const COPY_ICON: &str = "edit-copy-symbolic";

/// Widgets for the Shared Links content page.
pub(super) struct SharedLinksView {
    pub root: gtk::ScrolledWindow,
    group: libadwaita::PreferencesGroup,
    list: gtk::ListBox,
    empty: libadwaita::StatusPage,
    links: RefCell<Vec<SharedLink>>,
}

pub(super) fn build_shared_links_view() -> SharedLinksView {
    let group = libadwaita::PreferencesGroup::builder()
        .title("Shared Links")
        .description("Anyone with a link can view what it shares.")
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    group.add(&list);
    let empty = libadwaita::StatusPage::builder()
        .icon_name("mimick-share-symbolic")
        .title("No Shared Links")
        .description(
            "Create one from an album's or a photo's right-click menu, or from a selection.",
        )
        .visible(false)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    content.append(&group);
    content.append(&empty);
    let clamp = libadwaita::Clamp::builder()
        .maximum_size(820)
        .child(&content)
        .build();
    let root = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    SharedLinksView {
        root,
        group,
        list,
        empty,
        links: RefCell::default(),
    }
}

pub(super) fn connect_shared_links_view(ui: Rc<LibraryWindowUi>) {
    let ui_edit = ui.clone();
    ui.shared_links.list.connect_row_activated(move |_, row| {
        let link = usize::try_from(row.index())
            .ok()
            .and_then(|index| ui_edit.shared_links.links.borrow().get(index).cloned());
        if let Some(link) = link {
            edit(&ui_edit, link);
        }
    });
}

/// Show the Shared Links page and (re)load its links.
pub(super) fn show(ui: &Rc<LibraryWindowUi>) {
    ui.content_stack.set_visible_child_name("shared-links");
    reload(ui.clone());
}

fn reload(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.fetch_shared_links().await {
            Ok(links) => render(&ui, links),
            Err(err) => show_alert_dialog(
                &ui,
                "Could Not Load Shared Links",
                &format!("{err}\n\nYour API key needs the 'sharedLink.read' permission."),
            ),
        }
    });
}

fn render(ui: &Rc<LibraryWindowUi>, links: Vec<SharedLink>) {
    let view = &ui.shared_links;
    view.list.remove_all();
    for link in &links {
        view.list.append(&link_row(ui, link));
    }
    view.group.set_visible(!links.is_empty());
    view.empty.set_visible(links.is_empty());
    view.links.replace(links);
}

fn edit(ui: &Rc<LibraryWindowUi>, link: SharedLink) {
    let ui_after = ui.clone();
    shared_link_dialog::present(
        ui.clone(),
        LinkDialogMode::Edit(Box::new(link)),
        Rc::new(move || reload(ui_after.clone())),
    );
}

fn link_row(ui: &Rc<LibraryWindowUi>, link: &SharedLink) -> gtk::ListBoxRow {
    let content = gtk::Box::builder()
        .spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    content.append(&link_cover(ui, link));
    content.append(&link_text(link));
    content.append(&copy_button(ui, link));
    content.append(&delete_button(ui, link));
    gtk::ListBoxRow::builder().child(&content).build()
}

fn link_cover(ui: &LibraryWindowUi, link: &SharedLink) -> gtk::Overlay {
    let spacer = gtk::Box::builder()
        .width_request(64)
        .height_request(64)
        .build();
    let cover = gtk::Overlay::builder()
        .child(&spacer)
        .overflow(gtk::Overflow::Hidden)
        .valign(gtk::Align::Center)
        .css_classes(["mimick-link-cover"])
        .build();
    match cover_asset_id(link) {
        Some(asset_id) => {
            let picture = gtk::Picture::builder()
                .can_shrink(true)
                .content_fit(gtk::ContentFit::Cover)
                .build();
            cover.add_overlay(&picture);
            spawn_thumbnail(ui.ctx.clone(), asset_id.to_string(), picture);
        }
        None => cover.add_overlay(
            &gtk::Image::builder()
                .icon_name("image-x-generic-symbolic")
                .pixel_size(24)
                .css_classes(["dim-label"])
                .build(),
        ),
    }
    cover
}

fn link_text(link: &SharedLink) -> gtk::Box {
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let now = Utc::now();
    let expiry_class = if is_expired(link, now) {
        "error"
    } else {
        "dim-label"
    };
    text.append(&text_label(
        &expiry_text(link, now),
        &["caption", expiry_class],
    ));
    text.append(&text_label(&link_title(link), &["heading", "accent"]));
    if let Some(description) = link.description.as_deref().filter(|d| !d.is_empty()) {
        let label = text_label(description, &[]);
        label.set_wrap(false);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&label);
    }
    let capabilities = link_capabilities(link);
    if !capabilities.is_empty() {
        text.append(&text_label(&capabilities, &["caption", "accent"]));
    }
    text
}

fn text_label(text: &str, classes: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .css_classes(classes)
        .build()
}

fn copy_button(ui: &Rc<LibraryWindowUi>, link: &SharedLink) -> gtk::Button {
    let button = icon_button(COPY_ICON, "Copy link");
    let (ui, link) = (ui.clone(), link.clone());
    button.connect_clicked(move |button| {
        let (ui, link, button) = (ui.clone(), link.clone(), button.clone());
        glib::MainContext::default().spawn_local(async move {
            match copy_to_clipboard(&ui, &link).await {
                Ok(_) => show_copied(&button),
                Err(err) => show_alert_dialog(&ui, "Could Not Build Link", &err),
            }
        });
    });
    button
}

fn show_copied(button: &gtk::Button) {
    button.set_icon_name("object-select-symbolic");
    button.add_css_class("mimick-copied");
    let button = button.downgrade();
    glib::timeout_add_local_once(Duration::from_millis(1500), move || {
        if let Some(button) = button.upgrade() {
            button.remove_css_class("mimick-copied");
            button.set_icon_name(COPY_ICON);
        }
    });
}

fn delete_button(ui: &Rc<LibraryWindowUi>, link: &SharedLink) -> gtk::Button {
    let button = icon_button("user-trash-symbolic", "Delete link");
    button.add_css_class("error");
    let (ui, link_id) = (ui.clone(), link.id.clone());
    button.connect_clicked(move |_| confirm_delete(ui.clone(), link_id.clone()));
    button
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build()
}

fn confirm_delete(ui: Rc<LibraryWindowUi>, link_id: String) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading("Delete this link?")
        .body("Anyone using it will lose access. This can't be undone.")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete");
    dialog.set_response_appearance("delete", libadwaita::ResponseAppearance::Destructive);
    dialog.set_close_response("cancel");
    let ui_for_choice = ui.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "delete" {
            return;
        }
        let (ui, link_id) = (ui_for_choice.clone(), link_id.clone());
        glib::MainContext::default().spawn_local(async move {
            match ui.ctx.api_client.delete_shared_link(&link_id).await {
                Ok(()) => reload(ui),
                Err(err) => show_alert_dialog(&ui, "Could Not Delete Link", &err),
            }
        });
    });
    dialog.present(Some(&ui.window));
}

/// Album name, or "Individual share": Immich lists at most one asset per link.
fn link_title(link: &SharedLink) -> String {
    match &link.album {
        Some(album) => album.album_name.clone(),
        None => "Individual share".to_string(),
    }
}

fn cover_asset_id(link: &SharedLink) -> Option<&str> {
    match &link.album {
        Some(album) => album.album_thumbnail_asset_id.as_deref(),
        None => link.assets.first().map(|asset| asset.id.as_str()),
    }
}

fn is_expired(link: &SharedLink, now: DateTime<Utc>) -> bool {
    link.expires_at
        .as_deref()
        .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
        .is_some_and(|at| at <= now)
}

fn expiry_text(link: &SharedLink, now: DateTime<Utc>) -> String {
    if is_expired(link, now) {
        return "Expired".to_string();
    }
    match link.expires_at.as_deref() {
        Some(at) => format!("Expires {}", format_expiry(at)),
        None => "Never expires".to_string(),
    }
}

fn link_capabilities(link: &SharedLink) -> String {
    [
        (link.allow_upload, "Upload"),
        (link.allow_download, "Download"),
        (link.show_metadata, "Exif"),
        (link.password.is_some(), "Password"),
    ]
    .into_iter()
    .filter_map(|(enabled, name)| enabled.then_some(name))
    .collect::<Vec<_>>()
    .join(" • ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(json: serde_json::Value) -> SharedLink {
        let mut base = serde_json::json!({
            "id": "l1",
            "key": "k",
            "type": "INDIVIDUAL",
            "createdAt": "2026-10-01T00:00:00.000Z",
            "allowDownload": true,
            "allowUpload": false,
            "showMetadata": true,
            "assets": [{ "id": "a" }]
        });
        for (key, value) in json.as_object().unwrap() {
            base[key] = value.clone();
        }
        serde_json::from_value(base).unwrap()
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-10T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn title_uses_album_name_or_individual_share() {
        assert_eq!(link_title(&link(serde_json::json!({}))), "Individual share");
        let album = link(serde_json::json!({
            "type": "ALBUM",
            "album": { "id": "a1", "albumName": "Trips" },
            "assets": []
        }));
        assert_eq!(link_title(&album), "Trips");
    }

    #[test]
    fn cover_is_album_thumbnail_or_first_asset() {
        assert_eq!(cover_asset_id(&link(serde_json::json!({}))), Some("a"));
        let album = link(serde_json::json!({
            "type": "ALBUM",
            "album": { "id": "a1", "albumName": "Trips", "albumThumbnailAssetId": "cover" },
            "assets": []
        }));
        assert_eq!(cover_asset_id(&album), Some("cover"));
        let empty_album = link(serde_json::json!({
            "type": "ALBUM",
            "album": { "id": "a1", "albumName": "Trips", "albumThumbnailAssetId": null },
            "assets": []
        }));
        assert_eq!(cover_asset_id(&empty_album), None);
        assert_eq!(
            cover_asset_id(&link(serde_json::json!({ "assets": [] }))),
            None
        );
    }

    #[test]
    fn expiry_text_marks_past_links_expired() {
        assert_eq!(
            expiry_text(&link(serde_json::json!({})), now()),
            "Never expires"
        );
        let past = link(serde_json::json!({ "expiresAt": "2026-10-09T12:00:00Z" }));
        assert!(is_expired(&past, now()));
        assert_eq!(expiry_text(&past, now()), "Expired");
        let future = link(serde_json::json!({ "expiresAt": "2026-10-11T12:00:00Z" }));
        assert!(!is_expired(&future, now()));
        assert!(expiry_text(&future, now()).starts_with("Expires "));
    }

    #[test]
    fn capabilities_list_what_visitors_may_do() {
        assert_eq!(
            link_capabilities(&link(serde_json::json!({}))),
            "Download • Exif"
        );
        let open = link(serde_json::json!({ "allowUpload": true, "password": "x" }));
        assert_eq!(
            link_capabilities(&open),
            "Upload • Download • Exif • Password"
        );
        let locked = link(serde_json::json!({ "allowDownload": false, "showMetadata": false }));
        assert_eq!(link_capabilities(&locked), "");
    }
}
