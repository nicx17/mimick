//! "Add to Album" picker: put the selected photos in one of your albums, several ticked ones,
//! or a new one.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{BulkOutcome, LibraryAlbum};
use crate::app_context::AppContext;

use super::LibraryWindowUi;
use super::album_assets::{open_album, refresh_albums, showing_album};
use super::context_menu::{show_alert_on, show_error_toast};
use super::dialogs::{TextPrompt, prompt_text_on, toast_with_action};
use super::header_format::count_label;

mod several;
#[cfg(test)]
mod tests;
mod widgets;

use several::{OnSeveral, Several};
pub(super) use widgets::{album_cover, asset_cover};
use widgets::{cover_placeholder, no_matches_row, picker_dialog};

type OnChosen = Rc<dyn Fn(String, String)>;

/// What picking does: one album, and, where offered, every ticked album at once.
struct Choices {
    chosen: OnChosen,
    several: Option<OnSeveral>,
}

pub(super) fn present(ui: &Rc<LibraryWindowUi>, asset_ids: Vec<String>, on_added: Rc<dyn Fn()>) {
    if asset_ids.is_empty() {
        return;
    }
    let count = asset_ids.len();
    let (ui_add, ids_add, on_added_one) = (ui.clone(), asset_ids.clone(), on_added.clone());
    let ui_several = ui.clone();
    let choices = Choices {
        chosen: Rc::new(move |id, name| {
            add_and_report(&ui_add, id, name, ids_add.clone(), on_added_one.clone());
        }),
        several: Some(Rc::new(move |albums| {
            several::add_and_report(&ui_several, albums, asset_ids.clone(), on_added.clone());
        })),
    };
    let title = gettext("Add to Album");
    open_picker(
        ui.window.upcast_ref(),
        ui.ctx.clone(),
        (title, count),
        choices,
    );
}

/// The picker with one album to choose, as for uploads.
pub(super) fn choose(
    parent: &gtk::Widget,
    ctx: Arc<AppContext>,
    title: String,
    count: usize,
    on_chosen: OnChosen,
) {
    let choices = Choices {
        chosen: on_chosen,
        several: None,
    };
    open_picker(parent, ctx, (title, count), choices);
}

fn open_picker(
    parent: &gtk::Widget,
    ctx: Arc<AppContext>,
    (title, count): (String, usize),
    choices: Choices,
) {
    let parent = parent.clone();
    glib::MainContext::default().spawn_local(async move {
        let me = super::ensure_current_user_id(&ctx)
            .await
            .unwrap_or_default();
        let albums = known_albums(&ctx).await;
        log::debug!(
            "Album picker \"{title}\" opened with {} albums",
            albums.len()
        );
        let picker = Picker::new(ctx, parent, choices, albums, me, (&title, count));
        picker.render();
        let keep_alive = RefCell::new(Some(picker.clone()));
        picker.dialog.connect_closed(move |_| {
            keep_alive.borrow_mut().take();
        });
        picker.dialog.present(Some(&picker.parent));
        picker.filter.grab_focus();
    });
}

async fn known_albums(ctx: &AppContext) -> Vec<LibraryAlbum> {
    let cached = ctx.library_state.lock().albums.clone();
    if !cached.is_empty() {
        return cached;
    }
    ctx.api_client
        .fetch_library_albums()
        .await
        .inspect_err(|err| log::warn!("Album picker could not load albums: {err}"))
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
        (0, 0) => {
            return fill(gettext("Couldn't add to “{album}”"), &[("album", album)]);
        }
        (0, _) => fill(gettext("Already in “{album}”"), &[("album", album)]),
        (done, 0) => fill(
            gettext("Added {items} to “{album}”"),
            &[("items", &count_label(done as u64)), ("album", album)],
        ),
        (done, duplicate) => fill(
            gettext("Added {items} to “{album}” · {duplicate} already there"),
            &[
                ("items", &count_label(done as u64)),
                ("album", album),
                ("duplicate", &duplicate.to_string()),
            ],
        ),
    };
    if outcome.denied > 0 {
        message.push_str(" · ");
        message.push_str(&fill(
            ngettext(
                "{n} not allowed",
                "{n} not allowed",
                plural_n(outcome.denied),
            ),
            &[("n", &outcome.denied.to_string())],
        ));
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
            Err(err) => show_error_toast(
                &ui,
                &gettext("Couldn't add to the album"),
                &fill(
                    gettext("Immich rejected the request: {err}"),
                    &[("err", &err.to_string())],
                ),
            ),
        }
    });
}

fn report_added(ui: &Rc<LibraryWindowUi>, outcome: BulkOutcome, album_id: String, name: String) {
    let message = added_message(outcome, &name);
    let toast = if outcome.done > 0 || outcome.duplicate > 0 {
        let ui = ui.clone();
        toast_with_action(&message, &gettext("Open"), move || {
            open_album(&ui, &album_id, &name)
        })
    } else {
        libadwaita::Toast::new(&message)
    };
    ui.toasts.add_toast(toast);
}

fn create_then(ctx: Arc<AppContext>, parent: gtk::Widget, name: String, on_chosen: OnChosen) {
    glib::MainContext::default().spawn_local(async move {
        match ctx.api_client.create_album(&name).await {
            Ok(Some(id)) => on_chosen(id, name),
            Ok(None) => show_alert_on(
                &parent,
                &gettext("Could Not Create Album"),
                &gettext("Immich didn't return the new album."),
            ),
            Err(err) => show_alert_on(
                &parent,
                &gettext("Could Not Create Album"),
                &fill(
                    gettext("Immich rejected the request: {err}"),
                    &[("err", &err.to_string())],
                ),
            ),
        }
    });
}

struct Picker {
    ctx: Arc<AppContext>,
    parent: gtk::Widget,
    on_chosen: OnChosen,
    /// Ticked albums and their Add button, when the picker can add to several.
    several: Option<Several>,
    albums: Vec<LibraryAlbum>,
    me: String,
    dialog: libadwaita::Dialog,
    filter: gtk::SearchEntry,
    list: gtk::ListBox,
    first_pick: RefCell<Option<Box<dyn Fn()>>>,
}

impl Picker {
    fn new(
        ctx: Arc<AppContext>,
        parent: gtk::Widget,
        choices: Choices,
        albums: Vec<LibraryAlbum>,
        me: String,
        (title, count): (&str, usize),
    ) -> Rc<Self> {
        let filter = gtk::SearchEntry::builder()
            .placeholder_text(gettext("Filter albums"))
            .hexpand(true)
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .valign(gtk::Align::Start)
            .hexpand(true)
            .build();
        let several = choices.several.map(Several::new);
        let add = several.as_ref().map(|several| &several.add);
        let dialog = picker_dialog(&filter, &list, (title, count), add);
        let picker = Rc::new(Self {
            ctx,
            parent,
            on_chosen: choices.chosen,
            several,
            albums,
            me,
            dialog,
            filter,
            list,
            first_pick: RefCell::default(),
        });
        picker.connect_filter();
        picker.connect_add();
        picker
    }

    fn connect_add(self: &Rc<Self>) {
        let Some(several) = &self.several else {
            return;
        };
        let weak = Rc::downgrade(self);
        several.add.connect_clicked(move |_| {
            if let Some(picker) = weak.upgrade() {
                picker.add_ticked();
            }
        });
    }

    /// Close and add to every ticked album; false when none is ticked.
    fn add_ticked(&self) -> bool {
        let Some(several) = self.several.as_ref().filter(|several| several.any()) else {
            return false;
        };
        self.dialog.close();
        several.commit();
        true
    }

    fn ticking(&self) -> bool {
        self.several.as_ref().is_some_and(Several::any)
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
                && !picker.add_ticked()
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
            Some(name) => fill(gettext("New Album “{name}”"), &[("name", name)]),
            None => gettext("New Album…"),
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
        row.add_prefix(&album_cover(&self.ctx, album));
        let tick = self.several.as_ref().map(|several| several.tick(album));
        if let Some(tick) = &tick {
            row.add_suffix(tick);
        }
        let (weak, id, name) = (
            Rc::downgrade(self),
            album.id.clone(),
            album.album_name.clone(),
        );
        // Once an album is ticked, tapping rows ticks them too instead of adding right away.
        row.connect_activated(move |_| match (&tick, weak.upgrade()) {
            (Some(tick), Some(picker)) if picker.ticking() => tick.set_active(!tick.is_active()),
            _ => pick_album(&weak, id.clone(), name.clone()),
        });
        row
    }
}

fn pick_album(picker: &Weak<Picker>, album_id: String, name: String) {
    let Some(picker) = picker.upgrade() else {
        return;
    };
    picker.dialog.close();
    (picker.on_chosen)(album_id, name);
}

/// Create an album named `name`, or ask for a name first when there isn't one.
fn pick_new(picker: &Weak<Picker>, name: Option<String>) {
    let Some(picker) = picker.upgrade() else {
        return;
    };
    picker.dialog.close();
    let (ctx, parent, on_chosen) = (
        picker.ctx.clone(),
        picker.parent.clone(),
        picker.on_chosen.clone(),
    );
    if let Some(name) = name {
        create_then(ctx, parent, name, on_chosen);
        return;
    }
    prompt_text_on(
        &picker.parent,
        TextPrompt {
            heading: &gettext("New Album"),
            body: &gettext("The selected items will be added to it."),
            initial: "",
            placeholder: &gettext("Album name"),
            accept: &gettext("Create"),
        },
        move |name| create_then(ctx.clone(), parent.clone(), name, on_chosen.clone()),
    );
}
