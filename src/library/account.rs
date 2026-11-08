//! "Account" in the main menu: your profile picture, the devices signed in to your account,
//! and your API keys, one page each.

use gettextrs::gettext;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use super::LibraryWindowUi;

mod avatar_crop;
mod devices;
mod keys;
mod profile;

pub(super) use profile::set_from_photo;

/// The `win.account` action behind the main menu's Account entry.
pub(super) fn connect(ui: &Rc<LibraryWindowUi>) {
    let action = gtk::gio::SimpleAction::new("account", None);
    let weak = Rc::downgrade(ui);
    action.connect_activate(move |_, _| {
        if let Some(ui) = weak.upgrade() {
            present(&ui);
        }
    });
    ui.window.add_action(&action);
}

/// Each page loads when the dialog opens; a page whose permission the key lacks says so.
fn present(ui: &Rc<LibraryWindowUi>) {
    let dialog = libadwaita::PreferencesDialog::builder()
        .title(gettext("Account"))
        .content_width(520)
        .content_height(620)
        .search_enabled(false)
        .build();
    dialog.add(&profile::page(ui, &dialog));
    dialog.add(&devices::page(ui, &dialog));
    dialog.add(&keys::page(ui, &dialog));
    dialog.present(Some(&ui.window));
}

/// The rows of a page's group, kept in our own list so a reload can empty it.
fn boxed_list() -> gtk::ListBox {
    gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build()
}

/// A row saying why a group is empty, such as a missing API key permission.
fn note_row(text: &str) -> libadwaita::ActionRow {
    libadwaita::ActionRow::builder()
        .title(text)
        .title_lines(0)
        .use_markup(false)
        .css_classes(["dim-label"])
        .build()
}

/// The permission an API key needs for a page, in Immich's own words.
fn needs_permission(permission: &str) -> String {
    crate::i18n::fill(
        gettext("Couldn't load this. Your API key needs the “{permission}” permission."),
        &[("permission", permission)],
    )
}

fn confirm(
    dialog: &libadwaita::PreferencesDialog,
    (heading, body, accept): (&str, &str, &str),
    on_accept: impl Fn() + 'static,
) {
    let alert = libadwaita::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    alert.add_response("cancel", &gettext("Cancel"));
    alert.add_response("accept", accept);
    alert.set_response_appearance("accept", libadwaita::ResponseAppearance::Destructive);
    alert.set_default_response(Some("cancel"));
    alert.set_close_response("cancel");
    alert.connect_response(Some("accept"), move |_, _| on_accept());
    alert.present(Some(dialog));
}
