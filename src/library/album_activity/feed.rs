use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk::prelude::*;

use crate::api_client::{Activity, ActivityKind, LibraryAlbum, NewActivity};
use crate::library::LibraryWindowUi;
use crate::library::album_picker::asset_cover;
use crate::library::context_menu::show_error_toast;
use crate::library::explore_view::format_relative_date;
use crate::library::favorites::heart_icon;

use super::{apply_change, can_delete, like_text, my_like, user_name};

const AVATAR_SIZE: i32 = 32;

pub(in crate::library) struct ActivityFeed {
    pub list: gtk::ListBox,
    pub composer: gtk::Box,
    placeholder: gtk::Label,
    like_btn: gtk::Button,
    entry: gtk::Entry,
    send_btn: gtk::Button,
    ui: Rc<LibraryWindowUi>,
    album_id: String,
    owner_id: String,
    asset_id: Option<String>,
    me: RefCell<String>,
    activities: RefCell<Vec<Activity>>,
    on_change: Box<dyn Fn()>,
}

impl ActivityFeed {
    pub(in crate::library) fn new(
        ui: &Rc<LibraryWindowUi>,
        album: &LibraryAlbum,
        asset_id: Option<&str>,
        on_change: Box<dyn Fn()>,
    ) -> Rc<Self> {
        let (list, placeholder) = build_list();
        let like_btn = round_button("mimick-heart-outline-symbolic", "Like");
        let send_btn = round_button("mail-send-symbolic", "Send");
        send_btn.set_sensitive(false);
        let entry = gtk::Entry::builder()
            .placeholder_text("Add a comment…")
            .hexpand(true)
            .build();
        let composer = gtk::Box::builder().spacing(6).sensitive(false).build();
        composer.append(&like_btn);
        composer.append(&entry);
        composer.append(&send_btn);
        let feed = Rc::new(Self {
            list,
            composer,
            placeholder,
            like_btn,
            entry,
            send_btn,
            ui: ui.clone(),
            album_id: album.id.clone(),
            owner_id: album.owner_id().to_string(),
            asset_id: asset_id.map(str::to_string),
            me: RefCell::default(),
            activities: RefCell::default(),
            on_change,
        });
        feed.connect_composer();
        feed
    }

    fn connect_composer(self: &Rc<Self>) {
        let feed = Rc::downgrade(self);
        self.like_btn
            .connect_clicked(move |_| with(&feed, |f| f.toggle_like()));
        let feed = Rc::downgrade(self);
        self.send_btn
            .connect_clicked(move |_| with(&feed, |f| f.send_comment()));
        let feed = Rc::downgrade(self);
        self.entry
            .connect_activate(move |_| with(&feed, |f| f.send_comment()));
        let send_btn = self.send_btn.clone();
        self.entry.connect_changed(move |entry| {
            send_btn.set_sensitive(!entry.text().trim().is_empty());
        });
    }

    pub(in crate::library) fn load(self: &Rc<Self>) {
        let feed = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let me = crate::library::ensure_current_user_id(&feed.ui.ctx).await;
            *feed.me.borrow_mut() = me.unwrap_or_default();
            let result = feed
                .ui
                .ctx
                .api_client
                .fetch_activities(&feed.album_id, feed.asset_id.as_deref())
                .await;
            match result {
                Ok(activities) => {
                    *feed.activities.borrow_mut() = activities;
                    feed.render();
                    feed.composer.set_sensitive(true);
                }
                Err(err) => {
                    log::warn!("Could not load album activity: {err}");
                    feed.placeholder
                        .set_label("Couldn't load likes and comments");
                }
            }
        });
    }

    fn render(self: &Rc<Self>) {
        self.list.remove_all();
        self.placeholder.set_label("No likes or comments yet");
        for activity in self.activities.borrow().iter() {
            self.list.append(&self.row(activity));
        }
        let liked = self.my_like_id().is_some();
        self.like_btn.set_icon_name(heart_icon(liked));
        self.like_btn
            .set_tooltip_text(Some(if liked { "Unlike" } else { "Like" }));
    }

    fn my_like_id(&self) -> Option<String> {
        let activities = self.activities.borrow();
        my_like(&activities, &self.me.borrow(), self.asset_id.as_deref()).map(|a| a.id.clone())
    }

    fn new_activity<'a>(&'a self, kind: ActivityKind, comment: Option<&'a str>) -> NewActivity<'a> {
        NewActivity {
            album_id: &self.album_id,
            asset_id: self.asset_id.as_deref(),
            kind,
            comment,
        }
    }

    fn apply(self: &Rc<Self>, removed: Option<&str>, added: Option<Activity>) {
        apply_change(&mut self.activities.borrow_mut(), removed, added);
        self.render();
        (self.on_change)();
    }

    fn toggle_like(self: &Rc<Self>) {
        let existing = self.my_like_id();
        self.like_btn.set_sensitive(false);
        let feed = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let api = &feed.ui.ctx.api_client;
            let result = match &existing {
                Some(id) => api.delete_activity(id).await.map(|()| None),
                None => api
                    .add_activity(&feed.new_activity(ActivityKind::Like, None))
                    .await
                    .map(Some),
            };
            feed.like_btn.set_sensitive(true);
            match result {
                Ok(added) => feed.apply(existing.as_deref(), added),
                Err(err) => show_error_toast(&feed.ui, "Couldn't update the like", &err),
            }
        });
    }

    fn send_comment(self: &Rc<Self>) {
        let text = self.entry.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.composer.set_sensitive(false);
        let feed = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = feed
                .ui
                .ctx
                .api_client
                .add_activity(&feed.new_activity(ActivityKind::Comment, Some(&text)))
                .await;
            feed.composer.set_sensitive(true);
            match result {
                Ok(added) => {
                    feed.entry.set_text("");
                    feed.apply(None, Some(added));
                }
                Err(err) => show_error_toast(&feed.ui, "Couldn't post the comment", &err),
            }
            feed.entry.grab_focus();
        });
    }

    fn delete(self: &Rc<Self>, activity_id: String) {
        let feed = self.clone();
        glib::MainContext::default().spawn_local(async move {
            match feed.ui.ctx.api_client.delete_activity(&activity_id).await {
                Ok(()) => feed.apply(Some(&activity_id), None),
                Err(err) => show_error_toast(&feed.ui, "Couldn't delete", &err),
            }
        });
    }

    fn row(self: &Rc<Self>, activity: &Activity) -> gtk::ListBoxRow {
        let content = gtk::Box::builder()
            .spacing(10)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(10)
            .margin_end(6)
            .build();
        let name = user_name(&activity.user);
        content.append(
            &libadwaita::Avatar::builder()
                .size(AVATAR_SIZE)
                .text(name)
                .show_initials(true)
                .valign(gtk::Align::Start)
                .build(),
        );
        content.append(&activity_text(activity, name));
        if let Some(asset_id) = self.thumbnail_for(activity) {
            content.append(&asset_cover(&self.ui.ctx, asset_id));
        }
        if can_delete(activity, &self.me.borrow(), &self.owner_id) {
            content.append(&self.delete_button(activity));
        }
        gtk::ListBoxRow::builder()
            .child(&content)
            .activatable(false)
            .build()
    }

    fn thumbnail_for(&self, activity: &Activity) -> Option<String> {
        self.asset_id
            .is_none()
            .then(|| activity.asset_id.clone())
            .flatten()
    }

    fn delete_button(self: &Rc<Self>, activity: &Activity) -> gtk::Button {
        let button = round_button("user-trash-symbolic", "Delete");
        let (feed, id) = (Rc::downgrade(self), activity.id.clone());
        button.connect_clicked(move |_| with(&feed, |f| f.delete(id.clone())));
        button
    }
}

fn with(feed: &Weak<ActivityFeed>, action: impl FnOnce(&Rc<ActivityFeed>)) {
    if let Some(feed) = feed.upgrade() {
        action(&feed);
    }
}

fn build_list() -> (gtk::ListBox, gtk::Label) {
    let placeholder = gtk::Label::builder()
        .label("Loading…")
        .wrap(true)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .css_classes(["dim-label"])
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .valign(gtk::Align::Start)
        .css_classes(["boxed-list"])
        .build();
    list.set_placeholder(Some(&placeholder));
    (list, placeholder)
}

fn round_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular"])
        .build()
}

fn activity_text(activity: &Activity, name: &str) -> gtk::Box {
    let header = gtk::Label::builder()
        .use_markup(true)
        .label(format!(
            "<b>{}</b> · <span alpha=\"70%\">{}</span>",
            glib::markup_escape_text(name),
            glib::markup_escape_text(&format_relative_date(&activity.created_at))
        ))
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let body = match activity.comment.as_deref() {
        Some(comment) => gtk::Label::builder()
            .label(comment)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .xalign(0.0)
            .build(),
        None => gtk::Label::builder()
            .label(like_text(activity))
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build(),
    };
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    text.append(&header);
    text.append(&body);
    text
}
