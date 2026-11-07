//! Memories above the Photos timeline: a row of cards, each opening as a slideshow.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use chrono::Datelike;
use gtk::prelude::*;

use super::LibraryWindowUi;
use super::context_menu::{Menu, show_error_toast};
use super::state::{LibrarySource, Location};
use crate::api_client::{LibraryAsset, Memory};

const CARD_WIDTH: i32 = 116;
const CARD_HEIGHT: i32 = 156;
/// Memories change once a day; this only catches a new day while the window stays open.
const REFRESH_AFTER: Duration = Duration::from_secs(30 * 60);

pub(super) struct MemoriesRow {
    pub root: gtk::ScrolledWindow,
    cards: gtk::Box,
    memories: RefCell<Vec<Memory>>,
    fetched: RefCell<Option<Instant>>,
}

pub(super) fn build() -> MemoriesRow {
    let cards = gtk::Box::builder().spacing(8).build();
    let root = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&cards)
        .visible(false)
        .build();
    MemoriesRow {
        root,
        cards,
        memories: RefCell::default(),
        fetched: RefCell::default(),
    }
}

/// Shows the row on Photos, fetching when the cached memories are stale; hides it elsewhere.
pub(super) fn sync(ui: &Rc<LibraryWindowUi>, source: &LibrarySource) {
    let row = &ui.memories;
    let wanted = matches!(source, LibrarySource::AllAssets) && ui.features.get().memories;
    if !wanted {
        row.root.set_visible(false);
        return;
    }
    row.root.set_visible(!row.memories.borrow().is_empty());
    let fresh = row
        .fetched
        .borrow()
        .is_some_and(|at| at.elapsed() < REFRESH_AFTER);
    if !fresh {
        fetch(ui);
    }
}

fn fetch(ui: &Rc<LibraryWindowUi>) {
    ui.memories.fetched.replace(Some(Instant::now()));
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let now = chrono::Local::now().fixed_offset();
        match ui.ctx.api_client.fetch_memories(&now).await {
            Ok(memories) => show(&ui, memories),
            Err(err) => {
                log::debug!("Memories unavailable: {err}");
                ui.memories.fetched.replace(None);
            }
        }
    });
}

fn show(ui: &Rc<LibraryWindowUi>, memories: Vec<Memory>) {
    let row = &ui.memories;
    let unchanged = *row.memories.borrow() == memories;
    if !unchanged {
        while let Some(child) = row.cards.first_child() {
            row.cards.remove(&child);
        }
        let year = chrono::Local::now().year();
        for memory in &memories {
            row.cards.append(&card(ui, memory, year));
        }
        *row.memories.borrow_mut() = memories;
    }
    let on_photos = matches!(
        super::navigation::current(ui),
        Location::Source(LibrarySource::AllAssets)
    );
    row.root
        .set_visible(on_photos && !row.memories.borrow().is_empty());
}

fn card(ui: &Rc<LibraryWindowUi>, memory: &Memory, this_year: i32) -> gtk::Button {
    let title = memory_title(memory, this_year);
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .width_request(CARD_WIDTH)
        .height_request(CARD_HEIGHT)
        .build();
    if let Some(cover) = memory.assets.first() {
        super::thumbnail_loader::load_into_picture(
            ui.ctx.clone(),
            cover.id.clone(),
            picture.clone(),
            true,
        );
    }
    let caption = gtk::Label::builder()
        .label(&title)
        .xalign(0.0)
        .valign(gtk::Align::End)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .css_classes(["mimick-memory-caption"])
        .build();
    let overlay = gtk::Overlay::builder()
        .child(&picture)
        .overflow(gtk::Overflow::Hidden)
        .build();
    overlay.add_overlay(&caption);
    let count = memory.assets.len();
    let button = gtk::Button::builder()
        .child(&overlay)
        .tooltip_text(format!(
            "{title} · {}",
            fill(
                ngettext("{n} photo", "{n} photos", plural_n(count)),
                &[("n", &count.to_string())],
            )
        ))
        .css_classes(["flat", "mimick-memory-card"])
        .build();
    if memory.seen_at.is_some() {
        button.add_css_class("mimick-memory-seen");
    }
    let (ui, id) = (ui.clone(), memory.id.clone());
    button.connect_clicked(move |button| {
        button.add_css_class("mimick-memory-seen");
        open(&ui, &id);
    });
    button
}

/// "1 year ago", "Ana's birthday", or the year for anything else.
pub(super) fn memory_title(memory: &Memory, this_year: i32) -> String {
    if let Some(name) = memory.data.person_name.as_deref().filter(|n| !n.is_empty()) {
        return fill(gettext("{name}'s birthday"), &[("name", name)]);
    }
    let year = memory
        .data
        .year
        .or_else(|| memory.memory_at.get(..4).and_then(|year| year.parse().ok()));
    match year.map(|year| this_year - year) {
        Some(years) if years > 0 => fill(
            ngettext("{n} year ago", "{n} years ago", years.unsigned_abs()),
            &[("n", &years.to_string())],
        ),
        Some(_) => gettext("This year"),
        None => gettext("Memory"),
    }
}

/// Opens the memory's photos and starts them as a slideshow at the user's memory pace.
fn open(ui: &Rc<LibraryWindowUi>, id: &str) {
    let Some(memory) = ui
        .memories
        .memories
        .borrow()
        .iter()
        .find(|m| m.id == id)
        .cloned()
    else {
        return;
    };
    let title = memory_title(&memory, chrono::Local::now().year());
    super::navigation::navigate_source(
        ui,
        LibrarySource::Memory {
            id: memory.id.clone(),
            title,
        },
    );
    play_when_loaded(ui);
    if memory.seen_at.is_none() {
        mark_seen(ui, memory.id);
    }
}

/// Starts the slideshow once the memory's photos reach the grid, unless the user moved on.
fn play_when_loaded(ui: &Rc<LibraryWindowUi>) {
    let generation = ui.ctx.library_state.lock().generation;
    let handler = Rc::new(RefCell::new(None::<glib::SignalHandlerId>));
    let (weak, handler_ref) = (Rc::downgrade(ui), handler.clone());
    let id = ui.grid.model.connect_items_changed(move |model, _, _, _| {
        if model.n_items() == 0 {
            return;
        }
        if let Some(id) = handler_ref.borrow_mut().take() {
            model.disconnect(id);
        }
        let Some(ui) = weak.upgrade() else {
            return;
        };
        // Deferred: the loader emits this while holding `library_state`, which isn't
        // re-entrant, and opening the lightbox reads the model this signal is changing.
        glib::idle_add_local_once(move || {
            let current = ui.ctx.library_state.lock().generation == generation;
            if current {
                let seconds = ui.features.get().memory_seconds;
                super::lightbox::start_slideshow_at(
                    &ui,
                    0,
                    Some(super::lightbox::SlideshowOptions::story(seconds)),
                );
            }
        });
    });
    handler.replace(Some(id));
}

fn mark_seen(ui: &Rc<LibraryWindowUi>, id: String) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let now = chrono::Utc::now().to_rfc3339();
        match ui.ctx.api_client.mark_memory_seen(&id, &now).await {
            Ok(()) => {
                if let Some(memory) = ui
                    .memories
                    .memories
                    .borrow_mut()
                    .iter_mut()
                    .find(|m| m.id == id)
                {
                    memory.seen_at = Some(now);
                }
            }
            Err(err) => log::debug!("Could not mark memory {id} as seen: {err}"),
        }
    });
}

/// The memory's photos from the row's cache, else from the server (after back/forward).
pub(super) async fn assets(ui: &LibraryWindowUi, id: &str) -> Result<Vec<LibraryAsset>, String> {
    let cached = ui
        .memories
        .memories
        .borrow()
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.assets.clone());
    match cached {
        Some(assets) => Ok(assets),
        None => Ok(ui.ctx.api_client.fetch_memory(id).await?.assets),
    }
}

/// "Remove from Memory" while a memory is open.
pub(super) fn append_remove_button(
    menu: &Menu,
    ui: &Rc<LibraryWindowUi>,
    asset_id: &str,
    on_removed: Rc<dyn Fn()>,
) {
    let Location::Source(LibrarySource::Memory { id, .. }) = super::navigation::current(ui) else {
        return;
    };
    let (ui, asset_id) = (ui.clone(), asset_id.to_string());
    menu.add_with_icon(
        "list-remove-symbolic",
        &gettext("Remove from Memory"),
        false,
        move || remove(&ui, id.clone(), asset_id.clone(), on_removed.clone()),
    );
}

fn remove(ui: &Rc<LibraryWindowUi>, memory_id: String, asset_id: String, on_removed: Rc<dyn Fn()>) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let ids = vec![asset_id.clone()];
        match ui
            .ctx
            .api_client
            .remove_memory_assets(&memory_id, &ids)
            .await
        {
            Ok(()) => {
                if let Some(memory) = ui
                    .memories
                    .memories
                    .borrow_mut()
                    .iter_mut()
                    .find(|m| m.id == memory_id)
                {
                    memory.assets.retain(|asset| asset.id != asset_id);
                }
                super::actions::remove_assets_from_grid(&ui, &ids);
                on_removed();
                ui.toasts
                    .add_toast(libadwaita::Toast::new(&gettext("Removed from this memory")));
            }
            Err(err) => show_error_toast(&ui, &gettext("Couldn't change the memory"), &err),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::MemoryData;

    fn memory(year: Option<i32>, person: Option<&str>) -> Memory {
        Memory {
            id: "m".into(),
            kind: "on_this_day".into(),
            data: MemoryData {
                year,
                person_name: person.map(str::to_string),
            },
            assets: Vec::new(),
            seen_at: None,
            memory_at: "2023-11-06T00:00:00.000Z".into(),
        }
    }

    #[test]
    fn titles_count_years_back_from_now() {
        assert_eq!(memory_title(&memory(Some(2025), None), 2026), "1 year ago");
        assert_eq!(memory_title(&memory(Some(2020), None), 2026), "6 years ago");
        assert_eq!(memory_title(&memory(None, None), 2026), "3 years ago");
        assert_eq!(memory_title(&memory(Some(2026), None), 2026), "This year");
    }

    #[test]
    fn birthdays_name_the_person() {
        assert_eq!(
            memory_title(&memory(Some(2020), Some("Ana")), 2026),
            "Ana's birthday"
        );
    }
}
