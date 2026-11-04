use std::cell::RefCell;
use std::rc::Rc;

use libadwaita::prelude::*;

use crate::api_client::LibraryAlbum;
use crate::library::LibraryWindowUi;
use crate::library::album_activity::ActivityFeed;

pub(super) struct ActivitySection {
    pub root: libadwaita::PreferencesGroup,
    holder: gtk::Box,
    feed: RefCell<Option<Rc<ActivityFeed>>>,
}

impl ActivitySection {
    pub(super) fn new() -> Self {
        let holder = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        let root = libadwaita::PreferencesGroup::builder()
            .title("Activity")
            .visible(false)
            .build();
        root.add(&holder);
        Self {
            root,
            holder,
            feed: RefCell::default(),
        }
    }

    pub(super) fn clear(&self) {
        self.root.set_visible(false);
        while let Some(child) = self.holder.first_child() {
            self.holder.remove(&child);
        }
        self.feed.take();
    }

    pub(super) fn show(&self, ui: &Rc<LibraryWindowUi>, album: &LibraryAlbum, asset_id: &str) {
        self.clear();
        let feed = ActivityFeed::new(ui, album, Some(asset_id), Box::new(|| {}));
        self.holder.append(&feed.list);
        self.holder.append(&feed.composer);
        feed.load();
        self.feed.replace(Some(feed));
        self.root.set_visible(true);
    }
}
