//! The Profile page: your picture, name, and email, and changing or removing the picture.

use crate::i18n::fill;
use gettextrs::gettext;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{Account, ThumbnailSize};
use crate::library::LibraryWindowUi;
use crate::library::context_menu::show_error_toast;

const AVATAR_SIZE: i32 = 96;

/// The page's widgets, filled in once the account loads.
struct Profile {
    avatar: libadwaita::Avatar,
    name: gtk::Label,
    email: gtk::Label,
    change: gtk::Button,
    remove: gtk::Button,
}

pub(super) fn page(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
) -> libadwaita::PreferencesPage {
    let profile = Rc::new(Profile::new());
    let group = libadwaita::PreferencesGroup::new();
    group.add(&profile.header());
    group.add(&profile.buttons());
    group.add(&hint());
    let page = libadwaita::PreferencesPage::builder()
        .title(gettext("Profile"))
        .icon_name("avatar-default-symbolic")
        .build();
    page.add(&group);
    profile.connect(ui, dialog);
    profile.load(ui);
    page
}

impl Profile {
    fn new() -> Self {
        Self {
            avatar: libadwaita::Avatar::new(AVATAR_SIZE, None, true),
            name: gtk::Label::builder()
                .css_classes(["title-2"])
                .wrap(true)
                .justify(gtk::Justification::Center)
                .build(),
            email: gtk::Label::builder()
                .css_classes(["dim-label"])
                .selectable(true)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .build(),
            change: gtk::Button::builder()
                .label(gettext("Change Picture…"))
                .css_classes(["pill"])
                .build(),
            remove: gtk::Button::builder()
                .label(gettext("Remove Picture"))
                .css_classes(["pill", "destructive-action"])
                .visible(false)
                .build(),
        }
    }

    fn header(&self) -> gtk::Box {
        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_bottom(12)
            .halign(gtk::Align::Center)
            .build();
        header.append(&self.avatar);
        header.append(&self.name);
        header.append(&self.email);
        header
    }

    /// Side by side on a wide window, one above the other on a phone.
    fn buttons(&self) -> libadwaita::WrapBox {
        let buttons = libadwaita::WrapBox::builder()
            .child_spacing(12)
            .line_spacing(8)
            .halign(gtk::Align::Center)
            .justify(libadwaita::JustifyMode::None)
            .build();
        buttons.append(&self.change);
        buttons.append(&self.remove);
        buttons
    }

    fn connect(self: &Rc<Self>, ui: &Rc<LibraryWindowUi>, dialog: &libadwaita::PreferencesDialog) {
        let (profile, ui_change, dialog_ref) = (self.clone(), ui.clone(), dialog.clone());
        self.change.connect_clicked(move |_| {
            pick_and_upload(&ui_change, &dialog_ref, profile.clone());
        });
        let (profile, ui_remove, dialog_ref) = (self.clone(), ui.clone(), dialog.clone());
        self.remove.connect_clicked(move |_| {
            let (profile, ui) = (profile.clone(), ui_remove.clone());
            super::confirm(
                &dialog_ref,
                (
                    &gettext("Remove your profile picture?"),
                    &gettext("Immich shows your initials instead."),
                    &gettext("Remove"),
                ),
                move || remove(&ui, profile.clone()),
            );
        });
    }

    fn load(self: &Rc<Self>, ui: &Rc<LibraryWindowUi>) {
        let (profile, ui) = (self.clone(), ui.clone());
        glib::MainContext::default().spawn_local(async move {
            match ui.ctx.api_client.fetch_account().await {
                Ok(account) => profile.show(&ui, account).await,
                Err(err) => profile.name.set_label(&fill(
                    gettext("Couldn't load your account: {err}"),
                    &[("err", &err)],
                )),
            }
        });
    }

    async fn show(&self, ui: &LibraryWindowUi, account: Account) {
        self.name.set_label(&account.name);
        self.email.set_label(&account.email);
        self.avatar.set_text(Some(&account.name));
        let has_picture = !account.profile_image_path.is_empty();
        self.remove.set_visible(has_picture);
        self.avatar.set_custom_image(None::<&gtk::gdk::Paintable>);
        if !has_picture {
            return;
        }
        let bytes = ui.ctx.api_client.fetch_profile_image(&account.id).await;
        let texture = bytes
            .ok()
            .and_then(|bytes| gtk::gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).ok());
        self.avatar.set_custom_image(texture.as_ref());
    }
}

fn hint() -> gtk::Label {
    gtk::Label::builder()
        .label(gettext(
            "To use one of your photos, choose Set as Profile Picture in its menu.",
        ))
        .wrap(true)
        .justify(gtk::Justification::Center)
        .margin_top(12)
        .css_classes(["dim-label", "caption"])
        .build()
}

fn image_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&gettext("Images")));
    for mime in ["image/jpeg", "image/png", "image/webp"] {
        filter.add_mime_type(mime);
    }
    filter
}

fn pick_and_upload(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
    profile: Rc<Profile>,
) {
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&image_filter());
    let chooser = gtk::FileDialog::builder()
        .title(gettext("Choose a Profile Picture"))
        .filters(&filters)
        .build();
    let (ui, dialog) = (ui.clone(), dialog.clone());
    glib::MainContext::default().spawn_local(async move {
        let Ok(file) = chooser.open_future(Some(&ui.window)).await else {
            return;
        };
        let bytes = match file.load_contents_future().await {
            Ok((bytes, _)) => bytes.to_vec(),
            Err(err) => return toast(&dialog, &err.to_string()),
        };
        if sniff_image(&bytes).is_none() {
            return toast(
                &dialog,
                &gettext("That file isn't a JPEG, PNG, or WebP picture."),
            );
        }
        let cropped = match super::avatar_crop::crop(&dialog, bytes).await {
            Ok(Some(cropped)) => cropped,
            Ok(None) => return,
            Err(err) => {
                return toast(
                    &dialog,
                    &fill(
                        gettext("Couldn't open the picture: {err}"),
                        &[("err", &err)],
                    ),
                );
            }
        };
        upload(&ui, &dialog, profile, cropped).await;
    });
}

async fn upload(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
    profile: Rc<Profile>,
    bytes: Vec<u8>,
) {
    let Some((mime, name)) = sniff_image(&bytes) else {
        return toast(
            dialog,
            &gettext("That file isn't a JPEG, PNG, or WebP picture."),
        );
    };
    profile.change.set_sensitive(false);
    let result = ui.ctx.api_client.set_profile_image(bytes, name, mime).await;
    profile.change.set_sensitive(true);
    match result {
        Ok(()) => profile.load(ui),
        Err(err) => toast(
            dialog,
            &fill(gettext("Couldn't change it: {err}"), &[("err", &err)]),
        ),
    }
}

fn remove(ui: &Rc<LibraryWindowUi>, profile: Rc<Profile>) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.remove_profile_image().await {
            Ok(()) => profile.load(&ui),
            Err(err) => show_error_toast(&ui, &gettext("Couldn't remove the picture"), &err),
        }
    });
}

fn toast(dialog: &libadwaita::PreferencesDialog, message: &str) {
    dialog.add_toast(libadwaita::Toast::new(message));
}

/// The type Immich needs to be told, read from the file's first bytes.
fn sniff_image(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some(("image/jpeg", "profile.jpg")),
        [0x89, b'P', b'N', b'G', ..] => Some(("image/png", "profile.png")),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(("image/webp", "profile.webp")),
        _ => None,
    }
}

/// "Set as Profile Picture" in the photo menu: the photo's preview becomes your picture.
pub(in crate::library) fn set_from_photo(ui: &Rc<LibraryWindowUi>, asset_id: String) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match crop_and_set(&ui, &asset_id).await {
            Ok(true) => ui
                .toasts
                .add_toast(libadwaita::Toast::new(&gettext("Profile picture changed"))),
            Ok(false) => {}
            Err(err) => show_error_toast(&ui, &gettext("Couldn't change your picture"), &err),
        }
    });
}

/// Crop Immich's preview of the photo, as Change Picture does, then upload it.
/// False when the crop was cancelled.
async fn crop_and_set(ui: &Rc<LibraryWindowUi>, asset_id: &str) -> Result<bool, String> {
    let api = &ui.ctx.api_client;
    let bytes = api
        .fetch_thumbnail(asset_id, ThumbnailSize::Preview, true)
        .await?;
    if sniff_image(&bytes).is_none() {
        return Err(gettext(
            "Immich's preview of this photo isn't a JPEG or WebP.",
        ));
    }
    let Some(cropped) = super::avatar_crop::crop(&ui.window, bytes).await? else {
        return Ok(false);
    };
    let (mime, name) = sniff_image(&cropped).unwrap_or(("image/png", "profile.png"));
    api.set_profile_image(cropped, name, mime).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_known_by_their_first_bytes() {
        assert_eq!(
            sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap().0,
            "image/jpeg"
        );
        assert_eq!(sniff_image(b"\x89PNG\r\n").unwrap().0, "image/png");
        assert_eq!(
            sniff_image(b"RIFF\0\0\0\0WEBPVP8 ").unwrap().0,
            "image/webp"
        );
        assert_eq!(sniff_image(b"GIF89a"), None);
    }
}
