//! "Appears In": the albums holding the photo, each opening that album.

use std::rc::Rc;

use libadwaita::prelude::*;

use crate::api_client::LibraryAlbum;
use crate::library::LibraryWindowUi;
use crate::library::album_assets::open_album;
use crate::library::album_picker::album_cover;
use crate::library::header_format::count_label;

const REVEAL_MS: u32 = 200;

pub(super) struct AlbumsSection {
    pub root: gtk::Revealer,
    list: gtk::ListBox,
}

impl AlbumsSection {
    pub(super) fn new() -> Self {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        let group = libadwaita::PreferencesGroup::builder()
            .title("Appears In")
            .build();
        group.add(&list);
        let root = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .transition_duration(REVEAL_MS)
            .child(&group)
            .build();
        Self { root, list }
    }

    /// Hide at once, so the next photo never shows this photo's albums.
    pub(super) fn clear(&self) {
        self.root.set_transition_duration(0);
        self.root.set_reveal_child(false);
        self.root.set_transition_duration(REVEAL_MS);
        self.list.remove_all();
    }

    pub(super) fn show(&self, ui: &Rc<LibraryWindowUi>, albums: &[LibraryAlbum]) {
        self.list.remove_all();
        for album in albums {
            self.list.append(&album_row(ui, album));
        }
        self.root.set_reveal_child(!albums.is_empty());
    }
}

fn album_row(ui: &Rc<LibraryWindowUi>, album: &LibraryAlbum) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .title(&album.album_name)
        .subtitle(count_label(album.asset_count.into()))
        .use_markup(false)
        .title_lines(1)
        .activatable(true)
        .tooltip_text(format!("Open “{}”", album.album_name))
        .build();
    row.add_prefix(&album_cover(ui, album));
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let (ui, id, name) = (ui.clone(), album.id.clone(), album.album_name.clone());
    row.connect_activated(move |_| open_album(&ui, &id, &name));
    row
}
