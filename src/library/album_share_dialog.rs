//! "Share album" dialog: see who an album is shared with, change roles, add or remove people.
//!
//! Every change goes straight to Immich, then the member list is refetched so the
//! dialog always shows the server's state.

use crate::i18n::fill;
use gettextrs::gettext;
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{AlbumRole, LibraryAlbum, UserSummary};

use super::LibraryWindowUi;
use super::context_menu::show_alert_dialog;

/// Members group, add-people group, and the album being edited. Cheap to clone.
#[derive(Clone)]
struct ShareDialog {
    ui: Rc<LibraryWindowUi>,
    album_id: String,
    members: libadwaita::PreferencesGroup,
    people: libadwaita::PreferencesGroup,
    search: gtk::SearchEntry,
    /// Rows currently shown in each group, so they can be removed on refresh.
    member_rows: Rc<RefCell<Vec<gtk::Widget>>>,
    people_rows: Rc<RefCell<Vec<gtk::Widget>>>,
    all_users: Rc<RefCell<Vec<UserSummary>>>,
    album: Rc<RefCell<Option<LibraryAlbum>>>,
}

/// Open the sharing dialog for `album` (one of the user's own albums).
pub(super) fn present(ui: Rc<LibraryWindowUi>, album: LibraryAlbum) {
    let members = libadwaita::PreferencesGroup::builder()
        .title(gettext("Shared with"))
        .build();
    let search = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Find people by name or email"))
        .build();
    let people = libadwaita::PreferencesGroup::builder()
        .title(gettext("Add people"))
        .header_suffix(&search)
        .build();
    let page = libadwaita::PreferencesPage::new();
    page.add(&members);
    page.add(&people);
    let toolbar = libadwaita::ToolbarView::builder().content(&page).build();
    toolbar.add_top_bar(&libadwaita::HeaderBar::new());
    let dialog = libadwaita::Dialog::builder()
        .title(fill(
            gettext("Share “{album_name}”"),
            &[("album_name", &album.album_name.to_string())],
        ))
        .content_width(480)
        .content_height(560)
        .child(&toolbar)
        .build();

    let share = ShareDialog {
        ui: ui.clone(),
        album_id: album.id.clone(),
        members,
        people,
        search: search.clone(),
        member_rows: Rc::default(),
        people_rows: Rc::default(),
        all_users: Rc::default(),
        album: Rc::default(),
    };
    let share_for_search = share.clone();
    search.connect_search_changed(move |_| share_for_search.render_people());
    // The albums page shows shared state, so refresh it once the dialog closes.
    let ui_for_close = ui.clone();
    dialog.connect_closed(move |_| super::refresh_albums_view(ui_for_close.clone()));
    share.load();
    dialog.present(Some(&ui.window));
}

impl ShareDialog {
    /// Fetch the album members and the user directory, then render both groups.
    fn load(&self) {
        let share = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let api = &share.ui.ctx.api_client;
            match api.fetch_album_detail(&share.album_id).await {
                Ok(album) => *share.album.borrow_mut() = Some(album),
                Err(err) => show_alert_dialog(&share.ui, &gettext("Could Not Load Album"), &err),
            }
            match api.list_users().await {
                Ok(users) => *share.all_users.borrow_mut() = users,
                // Without `user.read` the add list stays empty; the members can still be managed.
                Err(err) => log::warn!("Could not list users for sharing: {}", err),
            }
            share.render_members();
            share.render_people();
        });
    }

    fn render_members(&self) {
        clear_rows(&self.members, &self.member_rows);
        let Some(album) = self.album.borrow().clone() else {
            return;
        };
        let current_user = self
            .ui
            .ctx
            .current_user_id
            .lock()
            .clone()
            .unwrap_or_default();
        for member in &album.album_users {
            let row = libadwaita::ActionRow::builder()
                .title(display_name(&member.user.name, &member.user.email))
                .subtitle(&member.user.email)
                .build();
            match AlbumRole::parse(&member.role) {
                Some(role) if member.user.id != current_user => {
                    row.add_suffix(&self.role_dropdown(&member.user.id, role));
                    row.add_suffix(&self.remove_button(&member.user.id));
                }
                _ => row.add_suffix(&role_label(&member.role)),
            }
            self.members.add(&row);
            self.member_rows.borrow_mut().push(row.upcast());
        }
    }

    fn render_people(&self) {
        clear_rows(&self.people, &self.people_rows);
        let album = self.album.borrow().clone();
        let member_ids: Vec<String> = album
            .map(|a| a.album_users.iter().map(|m| m.user.id.clone()).collect())
            .unwrap_or_default();
        let query = self.search.text().to_string();
        for user in addable_users(&self.all_users.borrow(), &member_ids, &query) {
            let row = libadwaita::ActionRow::builder()
                .title(display_name(&user.name, &user.email))
                .subtitle(&user.email)
                .build();
            row.add_suffix(&self.add_button(&user.id));
            self.people.add(&row);
            self.people_rows.borrow_mut().push(row.upcast());
        }
    }

    /// Viewer / Editor dropdown that saves on change.
    fn role_dropdown(&self, user_id: &str, role: AlbumRole) -> gtk::DropDown {
        let dropdown = gtk::DropDown::from_strings(&[&gettext("Viewer"), &gettext("Editor")]);
        dropdown.set_valign(gtk::Align::Center);
        dropdown.set_selected(u32::from(role == AlbumRole::Editor));
        let (share, user_id) = (self.clone(), user_id.to_string());
        dropdown.connect_selected_notify(move |dropdown| {
            let role = if dropdown.selected() == 1 {
                AlbumRole::Editor
            } else {
                AlbumRole::Viewer
            };
            let user_id = user_id.clone();
            share.apply(
                gettext("Could Not Change Role"),
                move |api, album_id| async move {
                    api.set_album_user_role(&album_id, &user_id, role).await
                },
            );
        });
        dropdown
    }

    fn remove_button(&self, user_id: &str) -> gtk::Button {
        let button = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text(gettext("Stop sharing with this person"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let (share, user_id) = (self.clone(), user_id.to_string());
        button.connect_clicked(move |_| {
            let user_id = user_id.clone();
            share.apply(gettext("Could Not Remove Person"), move |api, album_id| async move {
                api.remove_album_user(&album_id, &user_id).await
            });
        });
        button
    }

    /// Adds the person as a Viewer; their role can be raised afterwards.
    fn add_button(&self, user_id: &str) -> gtk::Button {
        let button = gtk::Button::builder()
            .label(gettext("Add"))
            .valign(gtk::Align::Center)
            .build();
        let (share, user_id) = (self.clone(), user_id.to_string());
        button.connect_clicked(move |_| {
            let members = vec![(user_id.clone(), AlbumRole::Viewer)];
            share.apply(
                gettext("Could Not Share Album"),
                move |api, album_id| async move { api.add_album_users(&album_id, &members).await },
            );
        });
        button
    }

    /// Run one sharing change, then reload; on failure show `error_heading` with the reason.
    fn apply<F, Fut>(&self, error_heading: String, change: F)
    where
        F: FnOnce(std::sync::Arc<crate::api_client::ImmichApiClient>, String) -> Fut + 'static,
        Fut: std::future::Future<Output = Result<(), String>>,
    {
        let share = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let api = share.ui.ctx.api_client.clone();
            if let Err(err) = change(api, share.album_id.clone()).await {
                show_alert_dialog(&share.ui, &error_heading, &err);
            }
            share.load();
        });
    }
}

fn clear_rows(group: &libadwaita::PreferencesGroup, rows: &RefCell<Vec<gtk::Widget>>) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }
}

fn role_label(role: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(capitalize(role))
        .css_classes(["dim-label"])
        .build()
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

pub(super) fn display_name(name: &str, email: &str) -> String {
    if name.trim().is_empty() {
        email.to_string()
    } else {
        name.to_string()
    }
}

/// Users not yet in the album whose name or email contains `query` (case-insensitive).
pub(super) fn addable_users<'a>(
    users: &'a [UserSummary],
    member_ids: &[String],
    query: &str,
) -> Vec<&'a UserSummary> {
    let query = query.trim().to_lowercase();
    users
        .iter()
        .filter(|user| !member_ids.contains(&user.id))
        .filter(|user| {
            query.is_empty()
                || user.name.to_lowercase().contains(&query)
                || user.email.to_lowercase().contains(&query)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str, name: &str, email: &str) -> UserSummary {
        UserSummary {
            id: id.into(),
            name: name.into(),
            email: email.into(),
        }
    }

    #[test]
    fn addable_users_skip_members_and_match_name_or_email() {
        let users = [
            user("me", "Me", "me@x.dev"),
            user("u1", "Ana", "ana@x.dev"),
            user("u2", "Bob", "bob@work.dev"),
        ];
        let members = vec!["me".to_string()];
        let all: Vec<&str> = addable_users(&users, &members, "")
            .iter()
            .map(|u| u.id.as_str())
            .collect();
        assert_eq!(all, ["u1", "u2"]);
        let by_email: Vec<&str> = addable_users(&users, &members, "WORK")
            .iter()
            .map(|u| u.id.as_str())
            .collect();
        assert_eq!(by_email, ["u2"]);
    }

    #[test]
    fn display_name_and_role_labels() {
        assert_eq!(display_name("", "ana@x.dev"), "ana@x.dev");
        assert_eq!(display_name("Ana", "ana@x.dev"), "Ana");
        assert_eq!(capitalize("owner"), "Owner");
        assert_eq!(capitalize(""), "");
    }
}
