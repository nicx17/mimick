use gettextrs::gettext;
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::LibraryAlbum;
use crate::library::LibraryWindowUi;
use crate::library::album_activity::ActivityFeed;

pub(super) struct ActivitySection {
    pub root: libadwaita::PreferencesGroup,
    holder: libadwaita::Bin,
    feed: RefCell<Option<Rc<ActivityFeed>>>,
}

impl ActivitySection {
    pub(super) fn new() -> Self {
        let holder = libadwaita::Bin::new();
        let root = libadwaita::PreferencesGroup::builder()
            .title(gettext("Activity"))
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
        self.holder.set_child(None::<&gtk::Widget>);
        self.feed.take();
    }

    pub(super) fn show(&self, ui: &Rc<LibraryWindowUi>, album: &LibraryAlbum, asset_id: &str) {
        let feed = ActivityFeed::new(ui, album, Some(asset_id), Box::new(|| {}));
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        content.append(&feed.list);
        content.append(&feed.composer);
        self.holder.set_child(Some(&content));
        feed.load();
        self.feed.replace(Some(feed));
        self.root.set_visible(true);
    }
}
