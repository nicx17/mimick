use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{Activity, ActivityKind, ActivityStatistics, LibraryAlbum, UserSummary};

use super::LibraryWindowUi;
use super::context_menu::show_error_toast;
use super::explore_view::format_relative_date;
use super::favorites::heart_icon;
use super::header_format::count_noun;

const AVATAR_SIZE: i32 = 32;
const THUMB_SIZE: i32 = 40;

pub(super) fn has_activity(album: &LibraryAlbum) -> bool {
    album.is_activity_enabled && album.is_shared()
}

fn my_like<'a>(
    activities: &'a [Activity],
    me: &str,
    asset_id: Option<&str>,
) -> Option<&'a Activity> {
    activities.iter().find(|a| {
        a.kind == ActivityKind::Like && a.user.id == me && a.asset_id.as_deref() == asset_id
    })
}

fn can_delete(activity: &Activity, me: &str, owner_id: &str) -> bool {
    !me.is_empty() && (activity.user.id == me || owner_id == me)
}

fn apply_change(activities: &mut Vec<Activity>, removed: Option<&str>, added: Option<Activity>) {
    if let Some(id) = removed {
        activities.retain(|a| a.id != id);
    }
    if let Some(added) = added
        && !activities.iter().any(|a| a.id == added.id)
    {
        activities.push(added);
    }
}

fn user_name(user: &UserSummary) -> &str {
    if user.name.is_empty() {
        &user.email
    } else {
        &user.name
    }
}

fn like_text(activity: &Activity) -> &'static str {
    if activity.asset_id.is_some() {
        "Liked this photo"
    } else {
        "Liked the album"
    }
}

fn stats_summary(stats: ActivityStatistics) -> String {
    if stats.likes == 0 && stats.comments == 0 {
        return "No likes or comments yet".to_string();
    }
    format!(
        "{} · {}",
        count_noun(stats.likes, "like", "likes"),
        count_noun(stats.comments, "comment", "comments")
    )
}

fn like_tooltip(liked: bool, on_photo: bool) -> &'static str {
    match (liked, on_photo) {
        (true, _) => "Unlike",
        (false, true) => "Like this photo",
        (false, false) => "Like the album",
    }
}

pub(super) struct ActivityFeed {
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
    pub(super) fn new(
        ui: &Rc<LibraryWindowUi>,
        album: &LibraryAlbum,
        asset_id: Option<&str>,
        on_change: Box<dyn Fn()>,
    ) -> Rc<Self> {
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

    pub(super) fn load(self: &Rc<Self>) {
        let feed = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let me = super::ensure_current_user_id(&feed.ui.ctx).await;
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
            .set_tooltip_text(Some(like_tooltip(liked, self.asset_id.is_some())));
    }

    fn my_like_id(&self) -> Option<String> {
        let activities = self.activities.borrow();
        my_like(&activities, &self.me.borrow(), self.asset_id.as_deref()).map(|a| a.id.clone())
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
                    .add_activity(
                        &feed.album_id,
                        feed.asset_id.as_deref(),
                        ActivityKind::Like,
                        None,
                    )
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
                .add_activity(
                    &feed.album_id,
                    feed.asset_id.as_deref(),
                    ActivityKind::Comment,
                    Some(&text),
                )
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
        if self.asset_id.is_none()
            && let Some(asset_id) = &activity.asset_id
        {
            content.append(&self.thumbnail(asset_id));
        }
        if can_delete(activity, &self.me.borrow(), &self.owner_id) {
            content.append(&self.delete_button(activity));
        }
        gtk::ListBoxRow::builder()
            .child(&content)
            .activatable(false)
            .build()
    }

    fn thumbnail(&self, asset_id: &str) -> gtk::Overlay {
        let spacer = gtk::Box::builder()
            .width_request(THUMB_SIZE)
            .height_request(THUMB_SIZE)
            .build();
        let frame = gtk::Overlay::builder()
            .child(&spacer)
            .valign(gtk::Align::Center)
            .overflow(gtk::Overflow::Hidden)
            .css_classes(["mimick-picker-cover"])
            .build();
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .build();
        frame.add_overlay(&picture);
        super::thumbnail_loader::load_into_picture(
            self.ui.ctx.clone(),
            asset_id.to_string(),
            picture,
            false,
        );
        frame
    }

    fn delete_button(self: &Rc<Self>, activity: &Activity) -> gtk::Button {
        let tooltip = match activity.kind {
            ActivityKind::Comment => "Delete comment",
            ActivityKind::Like => "Remove like",
        };
        let button = round_button("user-trash-symbolic", tooltip);
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
    let body = match (activity.kind, activity.comment.as_deref()) {
        (ActivityKind::Comment, Some(comment)) => gtk::Label::builder()
            .label(comment)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .xalign(0.0)
            .build(),
        _ => gtk::Label::builder()
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

pub(super) fn header_button(ui: &Rc<LibraryWindowUi>, album: &LibraryAlbum) -> gtk::Button {
    let count = gtk::Label::builder().visible(false).build();
    let content = gtk::Box::builder().spacing(6).build();
    content.append(&gtk::Image::from_icon_name("mimick-comment-symbolic"));
    content.append(&count);
    let button = gtk::Button::builder()
        .child(&content)
        .tooltip_text("Likes and comments")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let refresh = counts_refresher(ui, &album.id, &button, &count);
    refresh();
    let (ui, album) = (ui.clone(), album.clone());
    button.connect_clicked(move |_| {
        let refresh = refresh.clone();
        present(&ui, &album, Box::new(move || refresh()));
    });
    button
}

fn counts_refresher(
    ui: &Rc<LibraryWindowUi>,
    album_id: &str,
    button: &gtk::Button,
    count: &gtk::Label,
) -> Rc<dyn Fn()> {
    let (ui, album_id) = (ui.clone(), album_id.to_string());
    let (button, count) = (button.downgrade(), count.downgrade());
    Rc::new(move || {
        let (ui, album_id) = (ui.clone(), album_id.clone());
        let (button, count) = (button.clone(), count.clone());
        glib::MainContext::default().spawn_local(async move {
            let stats = ui.ctx.api_client.activity_statistics(&album_id, None).await;
            let (Ok(stats), Some(button), Some(count)) = (stats, button.upgrade(), count.upgrade())
            else {
                return;
            };
            let total = stats.likes + stats.comments;
            count.set_label(&total.to_string());
            count.set_visible(total > 0);
            button.set_tooltip_text(Some(&stats_summary(stats)));
        });
    })
}

fn present(ui: &Rc<LibraryWindowUi>, album: &LibraryAlbum, on_change: Box<dyn Fn()>) {
    let feed = ActivityFeed::new(ui, album, None, on_change);
    feed.list.set_margin_top(6);
    feed.list.set_margin_bottom(12);
    feed.list.set_margin_start(12);
    feed.list.set_margin_end(12);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&feed.list)
        .build();
    scroller
        .vadjustment()
        .connect_upper_notify(|adj| adj.set_value(adj.upper()));
    feed.composer.set_margin_top(6);
    feed.composer.set_margin_bottom(6);
    feed.composer.set_margin_start(12);
    feed.composer.set_margin_end(12);
    let toolbar = libadwaita::ToolbarView::new();
    let header = libadwaita::HeaderBar::builder()
        .title_widget(&libadwaita::WindowTitle::new("Activity", &album.album_name))
        .build();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroller));
    toolbar.add_bottom_bar(&feed.composer);
    let dialog = libadwaita::Dialog::builder()
        .content_width(420)
        .content_height(560)
        .child(&toolbar)
        .build();
    feed.load();
    let keep = RefCell::new(Some(feed));
    dialog.connect_closed(move |_| {
        keep.take();
    });
    dialog.present(Some(&ui.window));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activity(id: &str, kind: ActivityKind, user: &str, asset: Option<&str>) -> Activity {
        Activity {
            id: id.into(),
            kind,
            comment: (kind == ActivityKind::Comment).then(|| "Nice".into()),
            asset_id: asset.map(Into::into),
            created_at: "2024-01-01T00:00:00.000Z".into(),
            user: UserSummary {
                id: user.into(),
                name: String::new(),
                email: format!("{user}@example.com"),
            },
        }
    }

    #[test]
    fn my_like_matches_user_and_level() {
        let all = [
            activity("1", ActivityKind::Like, "me", Some("p1")),
            activity("2", ActivityKind::Comment, "me", None),
            activity("3", ActivityKind::Like, "ana", None),
            activity("4", ActivityKind::Like, "me", None),
        ];
        assert_eq!(my_like(&all, "me", None).map(|a| a.id.as_str()), Some("4"));
        assert_eq!(
            my_like(&all, "me", Some("p1")).map(|a| a.id.as_str()),
            Some("1")
        );
        assert!(my_like(&all, "me", Some("p2")).is_none());
        assert!(my_like(&all, "", None).is_none());
    }

    #[test]
    fn authors_and_album_owners_can_delete() {
        let comment = activity("1", ActivityKind::Comment, "ana", None);
        assert!(can_delete(&comment, "ana", "owner"));
        assert!(can_delete(&comment, "owner", "owner"));
        assert!(!can_delete(&comment, "ben", "owner"));
        assert!(!can_delete(&comment, "", ""));
    }

    #[test]
    fn apply_change_removes_and_skips_duplicate_likes() {
        let like = activity("1", ActivityKind::Like, "me", None);
        let mut all = vec![like.clone()];
        apply_change(&mut all, None, Some(like));
        assert_eq!(all.len(), 1);
        apply_change(&mut all, Some("1"), None);
        assert!(all.is_empty());
    }

    #[test]
    fn user_name_falls_back_to_email() {
        let mut user = activity("1", ActivityKind::Like, "ana", None).user;
        assert_eq!(user_name(&user), "ana@example.com");
        user.name = "Ana".into();
        assert_eq!(user_name(&user), "Ana");
    }

    #[test]
    fn like_text_names_the_target() {
        let photo = activity("1", ActivityKind::Like, "ana", Some("p1"));
        let album = activity("2", ActivityKind::Like, "ana", None);
        assert_eq!(like_text(&photo), "Liked this photo");
        assert_eq!(like_text(&album), "Liked the album");
    }

    #[test]
    fn stats_summary_counts_both_kinds() {
        assert_eq!(
            stats_summary(ActivityStatistics::default()),
            "No likes or comments yet"
        );
        let stats = ActivityStatistics {
            likes: 1,
            comments: 3,
        };
        assert_eq!(stats_summary(stats), "1 like · 3 comments");
    }

    #[test]
    fn album_needs_sharing_and_activity_enabled() {
        let mut album: LibraryAlbum = serde_json::from_value(serde_json::json!({
            "id": "a1",
            "albumName": "Trip",
            "assetCount": 1,
            "albumThumbnailAssetId": null,
            "createdAt": "2024-01-01T00:00:00.000Z",
            "updatedAt": "2024-01-01T00:00:00.000Z",
            "shared": true,
            "isActivityEnabled": true
        }))
        .unwrap();
        assert!(has_activity(&album));
        album.is_activity_enabled = false;
        assert!(!has_activity(&album));
        album.is_activity_enabled = true;
        album.shared = false;
        assert!(!has_activity(&album));
    }
}
