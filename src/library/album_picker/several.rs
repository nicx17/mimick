//! Ticking several albums in the picker; its Add button puts the photos in all of them at once.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::api_client::{LibraryAlbum, SeveralAdded};
use crate::library::LibraryWindowUi;
use crate::library::album_assets::{refresh_albums, showing_album};
use crate::library::context_menu::show_error_toast;
use crate::library::header_format::count_label;

/// Receives the ticked albums as `(id, name)`, in the order they were ticked.
pub(super) type OnSeveral = Rc<dyn Fn(Vec<(String, String)>)>;

/// The ticked albums and the header button that adds to them.
pub(super) struct Several {
    pub(super) add: gtk::Button,
    ticked: Rc<RefCell<Vec<(String, String)>>>,
    on_add: OnSeveral,
}

impl Several {
    pub(super) fn new(on_add: OnSeveral) -> Self {
        let add = gtk::Button::builder()
            .css_classes(["suggested-action"])
            .visible(false)
            .build();
        Self {
            add,
            ticked: Rc::default(),
            on_add,
        }
    }

    pub(super) fn any(&self) -> bool {
        !self.ticked.borrow().is_empty()
    }

    /// The check box for `album`'s row; it remembers its state across filtering.
    pub(super) fn tick(&self, album: &LibraryAlbum) -> gtk::CheckButton {
        let ticked = self.ticked.borrow().iter().any(|(id, _)| *id == album.id);
        let check = gtk::CheckButton::builder()
            .active(ticked)
            .valign(gtk::Align::Center)
            .tooltip_text(gettext("Add to this album too"))
            .build();
        let (list, add) = (self.ticked.clone(), self.add.clone());
        let (id, name) = (album.id.clone(), album.album_name.clone());
        check.connect_toggled(move |check| {
            let mut list = list.borrow_mut();
            list.retain(|(ticked, _)| *ticked != id);
            if check.is_active() {
                list.push((id.clone(), name.clone()));
            }
            show_count(&add, list.len());
        });
        check
    }

    pub(super) fn commit(&self) {
        (self.on_add)(self.ticked.take());
    }
}

fn show_count(add: &gtk::Button, albums: usize) {
    add.set_visible(albums > 0);
    add.set_label(&fill(
        ngettext("Add to {n} Album", "Add to {n} Albums", plural_n(albums)),
        &[("n", &albums.to_string())],
    ));
}

/// Add `asset_ids` to every album in `albums` and say how it went.
pub(super) fn add_and_report(
    ui: &Rc<LibraryWindowUi>,
    albums: Vec<(String, String)>,
    asset_ids: Vec<String>,
    on_added: Rc<dyn Fn()>,
) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let ids: Vec<String> = albums.iter().map(|(id, _)| id.clone()).collect();
        match ui.ctx.api_client.add_to_albums(&ids, &asset_ids).await {
            Ok(outcome) => {
                let message = added_message(outcome, asset_ids.len(), albums.len());
                ui.toasts.add_toast(libadwaita::Toast::new(&message));
                on_added();
                refresh_albums(&ui);
                if ids.iter().any(|id| showing_album(&ui, id)) {
                    crate::library::navigation::reload_current(&ui);
                }
            }
            Err(err) => show_error_toast(
                &ui,
                &gettext("Couldn't add to the albums"),
                &fill(
                    gettext("Immich rejected the request: {err}"),
                    &[("err", &err)],
                ),
            ),
        }
    });
}

/// Immich answers once for all the albums, so the message can't say which got what.
fn added_message(outcome: SeveralAdded, items: usize, albums: usize) -> String {
    match outcome {
        SeveralAdded::Added => fill(
            ngettext(
                "Added {items} to {n} album",
                "Added {items} to {n} albums",
                plural_n(albums),
            ),
            &[
                ("items", &count_label(items as u64)),
                ("n", &albums.to_string()),
            ],
        ),
        SeveralAdded::AlreadyThere => gettext("Already in those albums"),
        SeveralAdded::NotAllowed => gettext("You can't add these to those albums"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_albums_get_one_message() {
        assert_eq!(
            added_message(SeveralAdded::Added, 3, 2),
            "Added 3 items to 2 albums"
        );
        assert_eq!(
            added_message(SeveralAdded::AlreadyThere, 3, 2),
            "Already in those albums"
        );
    }
}
