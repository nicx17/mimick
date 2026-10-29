//! "Add to Album" picker: put the selected photos in one of your albums, or a new one.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{BulkOutcome, LibraryAlbum};

use super::LibraryWindowUi;
use super::album_assets::{open_album, refresh_albums, showing_album};
use super::albums_view::spawn_thumbnail;
use super::context_menu::show_alert_dialog;
use super::dialogs::{TextPrompt, prompt_text, toast_with_action};
use super::header_format::count_label;

const COVER_SIZE: i32 = 40;

pub(super) fn present(ui: &Rc<LibraryWindowUi>, asset_ids: Vec<String>, on_added: Rc<dyn Fn()>) {
    if asset_ids.is_empty() {
        return;
    }
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let me = super::ensure_current_user_id(&ui.ctx)
            .await
            .unwrap_or_default();
        let albums = known_albums(&ui).await;
        let picker = Picker::new(ui, asset_ids, on_added, albums, me);
        picker.render();
        let keep_alive = RefCell::new(Some(picker.clone()));
        picker.dialog.connect_closed(move |_| {
            keep_alive.borrow_mut().take();
        });
        picker.dialog.present(Some(&picker.ui.window));
        picker.filter.grab_focus();
    });
}

async fn known_albums(ui: &LibraryWindowUi) -> Vec<LibraryAlbum> {
    let cached = ui.ctx.library_state.lock().albums.clone();
    if !cached.is_empty() {
        return cached;
    }
    ui.ctx
        .api_client
        .fetch_library_albums()
        .await
        .unwrap_or_default()
}

/// Albums the user may add to, filtered by name and most recently changed first.
pub(super) fn pickable_albums<'a>(
    albums: &'a [LibraryAlbum],
    me: &str,
    query: &str,
) -> Vec<&'a LibraryAlbum> {
    let query = query.trim().to_lowercase();
    let mut picked: Vec<&LibraryAlbum> = albums
        .iter()
        .filter(|album| album.can_edit_assets(me))
        .filter(|album| query.is_empty() || album.album_name.to_lowercase().contains(&query))
        .collect();
    picked.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    picked
}

pub(super) fn added_message(outcome: BulkOutcome, album: &str) -> String {
    let mut message = match (outcome.done, outcome.duplicate) {
        (0, 0) => return format!("Couldn't add to “{album}”"),
        (0, _) => format!("Already in “{album}”"),
        (done, 0) => format!("Added {} to “{album}”", count_label(done as u64)),
        (done, duplicate) => format!(
            "Added {} to “{album}” · {duplicate} already there",
            count_label(done as u64)
        ),
    };
    if outcome.denied > 0 {
        message.push_str(&format!(" · {} not allowed", outcome.denied));
    }
    message
}

/// Add `asset_ids` to the album, then say how it went with an "Open" shortcut.
pub(super) fn add_and_report(
    ui: &Rc<LibraryWindowUi>,
    album_id: String,
    album_name: String,
    asset_ids: Vec<String>,
    on_added: Rc<dyn Fn()>,
) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.add_to_album(&album_id, &asset_ids).await {
            Ok(results) => {
                report_added(
                    &ui,
                    BulkOutcome::from_results(&results),
                    album_id.clone(),
                    album_name,
                );
                on_added();
                refresh_albums(&ui);
                if showing_album(&ui, &album_id) {
                    super::navigation::reload_current(&ui);
                }
            }
            Err(err) => show_alert_dialog(
                &ui,
                "Could Not Add to Album",
                &format!("Immich rejected the request: {err}"),
            ),
        }
    });
}

fn report_added(ui: &Rc<LibraryWindowUi>, outcome: BulkOutcome, album_id: String, name: String) {
    let message = added_message(outcome, &name);
    let toast = if outcome.done > 0 || outcome.duplicate > 0 {
        let ui = ui.clone();
        toast_with_action(&message, "Open", move || open_album(&ui, &album_id, &name))
    } else {
        libadwaita::Toast::new(&message)
    };
    ui.toasts.add_toast(toast);
}

fn create_and_add(
    ui: &Rc<LibraryWindowUi>,
    name: String,
    asset_ids: Vec<String>,
    on_added: Rc<dyn Fn()>,
) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.create_album(&name).await {
            Ok(Some(id)) => add_and_report(&ui, id, name, asset_ids, on_added),
            Ok(None) => show_alert_dialog(
                &ui,
                "Could Not Create Album",
                "Immich didn't return the new album.",
            ),
            Err(err) => show_alert_dialog(
                &ui,
                "Could Not Create Album",
                &format!("Immich rejected the request: {err}"),
            ),
        }
    });
}

struct Picker {
    ui: Rc<LibraryWindowUi>,
    asset_ids: Vec<String>,
    on_added: Rc<dyn Fn()>,
    albums: Vec<LibraryAlbum>,
    me: String,
    dialog: libadwaita::Dialog,
    filter: gtk::SearchEntry,
    list: gtk::ListBox,
    first_pick: RefCell<Option<Box<dyn Fn()>>>,
}

impl Picker {
    fn new(
        ui: Rc<LibraryWindowUi>,
        asset_ids: Vec<String>,
        on_added: Rc<dyn Fn()>,
        albums: Vec<LibraryAlbum>,
        me: String,
    ) -> Rc<Self> {
        let filter = gtk::SearchEntry::builder()
            .placeholder_text("Filter albums")
            .hexpand(true)
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .valign(gtk::Align::Start)
            .build();
        let dialog = picker_dialog(&filter, &list, asset_ids.len());
        let picker = Rc::new(Self {
            ui,
            asset_ids,
            on_added,
            albums,
            me,
            dialog,
            filter,
            list,
            first_pick: RefCell::default(),
        });
        picker.connect_filter();
        picker
    }

    fn connect_filter(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.filter.connect_search_changed(move |_| {
            if let Some(picker) = weak.upgrade() {
                picker.render();
            }
        });
        let weak = Rc::downgrade(self);
        self.filter.connect_activate(move |_| {
            if let Some(picker) = weak.upgrade()
                && let Some(pick) = picker.first_pick.borrow().as_ref()
            {
                pick();
            }
        });
    }

    fn render(self: &Rc<Self>) {
        self.list.remove_all();
        let query = self.filter.text().trim().to_string();
        let matches = pickable_albums(&self.albums, &self.me, &query);
        let named_new = !query.is_empty() && matches.is_empty();
        self.list
            .append(&self.new_album_row(named_new.then_some(query.as_str())));
        if named_new {
            self.list.append(&no_matches_row());
        }
        for album in &matches {
            self.list.append(&self.album_row(album));
        }
        let first = matches
            .first()
            .map(|album| (album.id.clone(), album.album_name.clone()));
        let weak = Rc::downgrade(self);
        *self.first_pick.borrow_mut() = Some(match first {
            Some((id, name)) => Box::new(move || pick_album(&weak, id.clone(), name.clone())),
            None => Box::new(move || pick_new(&weak, named_new.then(|| query.clone()))),
        });
    }

    fn new_album_row(self: &Rc<Self>, name: Option<&str>) -> libadwaita::ActionRow {
        let title = match name {
            Some(name) => format!("New Album “{name}”"),
            None => "New Album…".to_string(),
        };
        let row = libadwaita::ActionRow::builder()
            .title(title)
            .use_markup(false)
            .activatable(true)
            .build();
        row.add_prefix(&cover_placeholder("list-add-symbolic"));
        let (weak, name) = (Rc::downgrade(self), name.map(str::to_string));
        row.connect_activated(move |_| pick_new(&weak, name.clone()));
        row
    }

    fn album_row(self: &Rc<Self>, album: &LibraryAlbum) -> libadwaita::ActionRow {
        let mut subtitle = count_label(album.asset_count.into());
        if album.is_shared() {
            subtitle.push_str(" · Shared");
        }
        let row = libadwaita::ActionRow::builder()
            .title(&album.album_name)
            .subtitle(subtitle)
            .use_markup(false)
            .title_lines(1)
            .activatable(true)
            .build();
        row.add_prefix(&album_cover(&self.ui, album));
        let (weak, id, name) = (
            Rc::downgrade(self),
            album.id.clone(),
            album.album_name.clone(),
        );
        row.connect_activated(move |_| pick_album(&weak, id.clone(), name.clone()));
        row
    }
}

fn pick_album(picker: &Weak<Picker>, album_id: String, name: String) {
    let Some(picker) = picker.upgrade() else {
        return;
    };
    picker.dialog.close();
    add_and_report(
        &picker.ui,
        album_id,
        name,
        picker.asset_ids.clone(),
        picker.on_added.clone(),
    );
}

/// Create an album named `name`, or ask for a name first when there isn't one.
fn pick_new(picker: &Weak<Picker>, name: Option<String>) {
    let Some(picker) = picker.upgrade() else {
        return;
    };
    picker.dialog.close();
    let (ui, ids, on_added) = (
        picker.ui.clone(),
        picker.asset_ids.clone(),
        picker.on_added.clone(),
    );
    if let Some(name) = name {
        create_and_add(&ui, name, ids, on_added);
        return;
    }
    let ui_create = ui.clone();
    prompt_text(
        &ui,
        TextPrompt {
            heading: "New Album",
            body: "The selected items will be added to it.",
            initial: "",
            placeholder: "Album name",
            accept: "Create",
        },
        move |name| create_and_add(&ui_create, name, ids.clone(), on_added.clone()),
    );
}

fn picker_dialog(
    filter: &gtk::SearchEntry,
    list: &gtk::ListBox,
    count: usize,
) -> libadwaita::Dialog {
    let header = libadwaita::HeaderBar::builder()
        .title_widget(&libadwaita::WindowTitle::new(
            "Add to Album",
            &count_label(count as u64),
        ))
        .build();
    let search = gtk::Box::builder()
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(6)
        .build();
    search.append(filter);
    let content = gtk::Box::builder()
        .margin_top(6)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(list);
    let clamp = libadwaita::Clamp::builder()
        .maximum_size(600)
        .child(&content)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    let toolbar = libadwaita::ToolbarView::builder()
        .content(&scroller)
        .build();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&search);
    libadwaita::Dialog::builder()
        .title("Add to Album")
        .content_width(400)
        .content_height(560)
        .child(&toolbar)
        .build()
}

fn no_matches_row() -> gtk::ListBoxRow {
    let label = gtk::Label::builder()
        .label("No matching albums")
        .margin_top(12)
        .margin_bottom(12)
        .css_classes(["dim-label"])
        .build();
    gtk::ListBoxRow::builder()
        .child(&label)
        .activatable(false)
        .selectable(false)
        .build()
}

fn cover_frame() -> gtk::Box {
    gtk::Box::builder()
        .width_request(COVER_SIZE)
        .height_request(COVER_SIZE)
        .valign(gtk::Align::Center)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["mimick-picker-cover"])
        .build()
}

fn cover_placeholder(icon: &str) -> gtk::Box {
    let frame = cover_frame();
    frame.append(
        &gtk::Image::builder()
            .icon_name(icon)
            .hexpand(true)
            .halign(gtk::Align::Center)
            .build(),
    );
    frame
}

pub(super) fn album_cover(ui: &LibraryWindowUi, album: &LibraryAlbum) -> gtk::Box {
    let Some(thumb_id) = album.thumbnail_asset_id.clone() else {
        return cover_placeholder("folder-pictures-symbolic");
    };
    let frame = cover_frame();
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .hexpand(true)
        .build();
    frame.append(&picture);
    spawn_thumbnail(ui.ctx.clone(), thumb_id, picture);
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album(id: &str, name: &str, role: &str, updated: &str) -> LibraryAlbum {
        let owner = if role == "owner" { "me" } else { "boss" };
        let mut users = vec![serde_json::json!({ "role": "owner", "user": { "id": owner } })];
        if role != "owner" {
            users.push(serde_json::json!({ "role": role, "user": { "id": "me" } }));
        }
        serde_json::from_value(serde_json::json!({
            "id": id,
            "albumName": name,
            "assetCount": 1,
            "albumThumbnailAssetId": null,
            "createdAt": "2024-01-01T00:00:00.000Z",
            "updatedAt": updated,
            "albumUsers": users
        }))
        .unwrap()
    }

    fn fixture() -> Vec<LibraryAlbum> {
        vec![
            album("a", "Beach Trip", "owner", "2024-01-01T00:00:00.000Z"),
            album("b", "Family", "editor", "2024-03-01T00:00:00.000Z"),
            album("c", "Wedding", "viewer", "2024-04-01T00:00:00.000Z"),
            album("d", "Garden", "owner", "2024-02-01T00:00:00.000Z"),
        ]
    }

    #[test]
    fn picker_lists_owned_and_editor_albums_newest_first() {
        let albums = fixture();
        let ids: Vec<&str> = pickable_albums(&albums, "me", "")
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(ids, ["b", "d", "a"]);
    }

    #[test]
    fn picker_filter_ignores_case() {
        let albums = fixture();
        let names: Vec<&str> = pickable_albums(&albums, "me", "TRIP")
            .iter()
            .map(|a| a.album_name.as_str())
            .collect();
        assert_eq!(names, ["Beach Trip"]);
    }

    #[test]
    fn added_message_variants() {
        let o = |done, duplicate, denied| BulkOutcome {
            done,
            duplicate,
            denied,
        };
        assert_eq!(added_message(o(1, 0, 0), "Trip"), "Added 1 item to “Trip”");
        assert_eq!(
            added_message(o(3, 2, 0), "Trip"),
            "Added 3 items to “Trip” · 2 already there"
        );
        assert_eq!(added_message(o(0, 2, 0), "Trip"), "Already in “Trip”");
        assert_eq!(
            added_message(o(2, 0, 1), "Trip"),
            "Added 2 items to “Trip” · 1 not allowed"
        );
        assert_eq!(added_message(o(0, 0, 3), "Trip"), "Couldn't add to “Trip”");
    }
}
