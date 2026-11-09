//! Immich's notifications inbox: a bell with the unread count, a list to read and dismiss
//! them in, and new ones passed on as desktop notifications.

use crate::i18n::fill;
use gettextrs::gettext;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::Notification;

use super::LibraryWindowUi;
use super::dialogs::{clamped_list, scrolling_dialog};
use super::shared_link_dialog::format_expiry;
use super::state::LibrarySource;

/// How often the bell asks Immich for new notifications.
const POLL_SECONDS: u32 = 120;
/// More than this shows as "9+", which fits the badge.
const BADGE_MAX: usize = 9;

/// An icon for each of Immich's levels; anything else reads as information.
const LEVEL_ICONS: [(&str, &str); 3] = [
    ("success", "emblem-ok-symbolic"),
    ("warning", "dialog-warning-symbolic"),
    ("error", "dialog-error-symbolic"),
];

/// The header's bell and the unread ids it has seen, so only new ones reach the desktop.
pub(super) struct Bell {
    pub(super) button: gtk::Button,
    badge: gtk::Label,
    seen: RefCell<Option<HashSet<String>>>,
}

impl Bell {
    /// Hidden until something is unread, so it costs no room on a phone's header otherwise.
    pub(super) fn new() -> Self {
        let badge = gtk::Label::builder()
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .css_classes(["mimick-bell-badge"])
            .can_target(false)
            .build();
        let icon = gtk::Overlay::builder()
            .child(&gtk::Image::from_icon_name(
                "preferences-system-notifications-symbolic",
            ))
            .build();
        icon.add_overlay(&badge);
        let button = gtk::Button::builder()
            .child(&icon)
            .tooltip_text(gettext("Notifications"))
            .css_classes(["mimick-pressable"])
            .visible(false)
            .build();
        Self {
            button,
            badge,
            seen: RefCell::default(),
        }
    }

    fn show_unread(&self, unread: usize) {
        self.button.set_visible(unread > 0);
        let text = if unread > BADGE_MAX {
            format!("{BADGE_MAX}+")
        } else {
            unread.to_string()
        };
        self.badge.set_label(&text);
        self.button.set_tooltip_text(Some(&fill(
            gettext("Notifications ({count} unread)"),
            &[("count", &text)],
        )));
    }

    fn new_arrivals<'a>(&self, unread: &'a [Notification]) -> Vec<&'a Notification> {
        let ids: HashSet<String> = unread.iter().map(|n| n.id.clone()).collect();
        let previous = self.seen.replace(Some(ids));
        arrivals(previous.as_ref(), unread)
    }
}

/// The ones not seen before; on the first check, none, so old ones don't pop up.
fn arrivals<'a>(
    seen: Option<&HashSet<String>>,
    unread: &'a [Notification],
) -> Vec<&'a Notification> {
    match seen {
        Some(seen) => unread.iter().filter(|n| !seen.contains(&n.id)).collect(),
        None => Vec::new(),
    }
}

/// The `win.notifications` action and the bell's click; returns the check timer.
pub(super) fn connect(ui: &Rc<LibraryWindowUi>) -> glib::SourceId {
    let action = gtk::gio::SimpleAction::new("notifications", None);
    let weak = Rc::downgrade(ui);
    action.connect_activate(move |_, _| {
        if let Some(ui) = weak.upgrade() {
            present(&ui);
        }
    });
    ui.window.add_action(&action);
    let weak = Rc::downgrade(ui);
    ui.bell.button.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            present(&ui);
        }
    });
    check(ui);
    let weak = Rc::downgrade(ui);
    glib::timeout_add_seconds_local(POLL_SECONDS, move || match weak.upgrade() {
        Some(ui) => {
            check(&ui);
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    })
}

/// Count what's unread and pass new arrivals to the desktop; a key without
/// `notification.read` just leaves the bell hidden.
fn check(ui: &Rc<LibraryWindowUi>) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let unread = match ui.ctx.api_client.fetch_notifications(true).await {
            Ok(unread) => unread,
            Err(err) => return log::debug!("Notifications unavailable: {err}"),
        };
        for notification in ui.bell.new_arrivals(&unread) {
            let body = notification.description.clone().unwrap_or_default();
            crate::notifications::send_server_notification(
                &notification.title,
                &body,
                &notification.id,
            );
        }
        ui.bell.show_unread(unread.len());
    });
}

/// The inbox dialog's list, filled again after each change.
struct Inbox {
    ui: Rc<LibraryWindowUi>,
    dialog: libadwaita::Dialog,
    list: gtk::ListBox,
    empty: libadwaita::StatusPage,
    mark_all: gtk::Button,
    items: RefCell<Vec<Notification>>,
}

fn present(ui: &Rc<LibraryWindowUi>) {
    let inbox = Rc::new(Inbox::new(ui));
    inbox.connect();
    let keep_alive = RefCell::new(Some(inbox.clone()));
    inbox.dialog.connect_closed(move |_| {
        let inbox = keep_alive.borrow_mut().take();
        if let Some(inbox) = inbox {
            check(&inbox.ui);
        }
    });
    inbox.dialog.present(Some(&ui.window));
    inbox.load();
}

impl Inbox {
    fn new(ui: &Rc<LibraryWindowUi>) -> Self {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .valign(gtk::Align::Start)
            .build();
        let empty = libadwaita::StatusPage::builder()
            .icon_name("preferences-system-notifications-symbolic")
            .title(gettext("No Notifications"))
            .description(gettext(
                "Album invites and messages from Immich show up here.",
            ))
            .vexpand(true)
            .visible(false)
            .build();
        let mark_all = gtk::Button::builder()
            .label(gettext("Mark All Read"))
            .sensitive(false)
            .build();
        let dialog = inbox_dialog(&list, &empty, &mark_all);
        Self {
            ui: ui.clone(),
            dialog,
            list,
            empty,
            mark_all,
            items: RefCell::default(),
        }
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.mark_all.connect_clicked(move |_| {
            if let Some(inbox) = weak.upgrade() {
                let ids = inbox.unread_ids();
                inbox.mark_read(ids);
            }
        });
        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            if let Some(inbox) = weak.upgrade() {
                inbox.open(row.index());
            }
        });
    }

    fn unread_ids(&self) -> Vec<String> {
        let items = self.items.borrow();
        items
            .iter()
            .filter(|n| n.is_unread())
            .map(|n| n.id.clone())
            .collect()
    }

    fn load(self: &Rc<Self>) {
        let inbox = self.clone();
        glib::MainContext::default().spawn_local(async move {
            match inbox.ui.ctx.api_client.fetch_notifications(false).await {
                Ok(items) => inbox.show(items),
                Err(err) => {
                    inbox
                        .empty
                        .set_title(&gettext("Couldn't Load Notifications"));
                    inbox.empty.set_description(Some(&fill(
                        gettext("{err}\n\nYour API key needs the “{permission}” permission."),
                        &[("err", &err), ("permission", "notification.read")],
                    )));
                    inbox.empty.set_visible(true);
                }
            }
        });
    }

    fn show(self: &Rc<Self>, items: Vec<Notification>) {
        self.list.remove_all();
        for item in &items {
            self.list.append(&self.row(item));
        }
        self.empty.set_visible(items.is_empty());
        self.list.set_visible(!items.is_empty());
        self.mark_all
            .set_sensitive(items.iter().any(Notification::is_unread));
        self.items.replace(items);
    }

    /// Unread ones are bold with a dot; album invites open the album when tapped.
    fn row(self: &Rc<Self>, item: &Notification) -> libadwaita::ActionRow {
        let row = libadwaita::ActionRow::builder()
            .title(&item.title)
            .subtitle(row_subtitle(item))
            .subtitle_lines(3)
            .use_markup(false)
            .activatable(true)
            .build();
        if item.is_unread() {
            row.add_css_class("mimick-unread");
        }
        row.add_prefix(&gtk::Image::from_icon_name(level_icon(&item.level)));
        let dismiss = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text(gettext("Dismiss"))
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        let (weak, id) = (Rc::downgrade(self), item.id.clone());
        dismiss.connect_clicked(move |_| {
            if let Some(inbox) = weak.upgrade() {
                inbox.dismiss(id.clone());
            }
        });
        row.add_suffix(&dismiss);
        row
    }

    /// Mark the notification read, and open its album if it has one.
    fn open(self: &Rc<Self>, index: i32) {
        let item = usize::try_from(index)
            .ok()
            .and_then(|index| self.items.borrow().get(index).cloned());
        let Some(item) = item else {
            return;
        };
        if item.is_unread() {
            self.mark_read(vec![item.id.clone()]);
        }
        if let Some(album_id) = item.album_id() {
            self.dialog.close();
            super::navigation::navigate_source(
                &self.ui,
                LibrarySource::Album {
                    id: album_id.to_string(),
                    name: String::new(),
                },
            );
        }
    }

    fn mark_read(self: &Rc<Self>, ids: Vec<String>) {
        if ids.is_empty() {
            return;
        }
        let inbox = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let api = &inbox.ui.ctx.api_client;
            if let Err(err) = api.mark_notifications_read(&ids).await {
                log::warn!("Couldn't mark notifications read: {err}");
            }
            inbox.load();
        });
    }

    fn dismiss(self: &Rc<Self>, id: String) {
        let inbox = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let api = &inbox.ui.ctx.api_client;
            if let Err(err) = api.delete_notifications(&[id]).await {
                log::warn!("Couldn't dismiss a notification: {err}");
            }
            inbox.load();
        });
    }
}

fn row_subtitle(item: &Notification) -> String {
    let when = format_expiry(&item.created_at);
    match item.description.as_deref().filter(|d| !d.trim().is_empty()) {
        Some(description) => format!("{description}\n{when}"),
        None => when,
    }
}

fn level_icon(level: &str) -> &'static str {
    LEVEL_ICONS
        .iter()
        .find(|(name, _)| *name == level)
        .map_or("dialog-information-symbolic", |(_, icon)| *icon)
}

/// "Mark All Read" in the header; the list scrolls, or the empty page fills the dialog.
fn inbox_dialog(
    list: &gtk::ListBox,
    empty: &libadwaita::StatusPage,
    mark_all: &gtk::Button,
) -> libadwaita::Dialog {
    let header = libadwaita::HeaderBar::new();
    header.pack_start(mark_all);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    content.append(&clamped_list(list));
    content.append(empty);
    scrolling_dialog(&gettext("Notifications"), &header, &content, (440, 560))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(id: &str) -> Notification {
        serde_json::from_value(serde_json::json!({
            "id": id, "title": "t", "level": "info", "type": "SystemMessage",
            "createdAt": "2026-11-01T10:00:00.000Z"
        }))
        .unwrap()
    }

    #[test]
    fn only_notifications_after_the_first_check_are_new() {
        assert!(arrivals(None, &[notification("a")]).is_empty());
        let seen: HashSet<String> = ["a".to_string()].into();
        let next = [notification("a"), notification("b")];
        let arrived: Vec<&str> = arrivals(Some(&seen), &next)
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(arrived, ["b"]);
    }

    #[test]
    fn unknown_levels_read_as_information() {
        assert_eq!(level_icon("error"), "dialog-error-symbolic");
        assert_eq!(level_icon("other"), "dialog-information-symbolic");
    }
}
