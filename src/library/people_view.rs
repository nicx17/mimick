use crate::i18n::fill;
use gettextrs::gettext;
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{Person, PersonUpdate};

use super::LibraryWindowUi;
use super::context_menu::show_alert_dialog;
use super::dialogs::{TextPrompt, popover_menu, prompt_text};
use super::explore_view::spawn_person_thumbnail;
use super::state::LibrarySource;

const AVATAR_SIZE: i32 = 140;

pub(super) struct PeopleView {
    pub root: gtk::Box,
    search: gtk::SearchEntry,
    show_hidden: gtk::ToggleButton,
    grid: gtk::FlowBox,
    empty: libadwaita::StatusPage,
    pub people: RefCell<Vec<Person>>,
    /// People the server found by name who weren't among those loaded, for the current search.
    found: RefCell<Vec<Person>>,
}

pub(super) fn build_people_view() -> PeopleView {
    let (toolbar, search, show_hidden) = build_toolbar();
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(30)
        .column_spacing(4)
        .row_spacing(12)
        .valign(gtk::Align::Start)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(8)
        .margin_end(8)
        .build();
    let empty = libadwaita::StatusPage::builder()
        .icon_name("avatar-default-symbolic")
        .vexpand(true)
        .visible(false)
        .build();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    root.append(&toolbar);
    root.append(&scrolled(&grid, &empty));
    PeopleView {
        root,
        search,
        show_hidden,
        grid,
        empty,
        people: RefCell::default(),
        found: RefCell::default(),
    }
}

fn build_toolbar() -> (gtk::Box, gtk::SearchEntry, gtk::ToggleButton) {
    let search = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search people"))
        .hexpand(true)
        .build();
    let show_hidden = gtk::ToggleButton::builder()
        .icon_name("view-reveal-symbolic")
        .tooltip_text(gettext("Show hidden people"))
        .build();
    let toolbar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    toolbar.append(&search);
    toolbar.append(&show_hidden);
    (toolbar, search, show_hidden)
}

fn scrolled(grid: &gtk::FlowBox, empty: &libadwaita::StatusPage) -> gtk::ScrolledWindow {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    content.append(grid);
    content.append(empty);
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build()
}

pub(super) fn connect_people_view(ui: Rc<LibraryWindowUi>) {
    let ui_search = ui.clone();
    ui.people.search.connect_search_changed(move |_| {
        ui_search.people.found.borrow_mut().clear();
        render(&ui_search);
        search_server(&ui_search);
    });
    let ui_hidden = ui.clone();
    ui.people.show_hidden.connect_toggled(move |button| {
        button.set_icon_name(if button.is_active() {
            "view-conceal-symbolic"
        } else {
            "view-reveal-symbolic"
        });
        button.set_tooltip_text(Some(&if button.is_active() {
            gettext("Hide hidden people")
        } else {
            gettext("Show hidden people")
        }));
        render(&ui_hidden);
    });
}

pub(super) fn show(ui: &Rc<LibraryWindowUi>) {
    ui.content_stack.set_visible_child_name("people");
    reload(ui.clone());
}

pub(super) fn reload(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.fetch_people(true).await {
            Ok(people) => {
                *ui.people.people.borrow_mut() = people;
                render(&ui);
            }
            Err(err) => show_alert_dialog(
                &ui,
                &gettext("Could Not Load People"),
                &fill(
                    gettext("{err}\n\nYour API key needs the '{permission}' permission."),
                    &[("err", &err.to_string()), ("permission", "person.read")],
                ),
            ),
        }
    });
}

fn render(ui: &Rc<LibraryWindowUi>) {
    let view = &ui.people;
    while let Some(child) = view.grid.first_child() {
        view.grid.remove(&child);
    }
    let query = view.search.text().to_lowercase();
    let mut people = view.people.borrow().clone();
    people.extend(view.found.borrow().iter().cloned());
    let shown = visible_people(&people, &query, view.show_hidden.is_active());
    for person in &shown {
        view.grid.append(&person_tile(ui, person));
    }
    let (title, description) = if people.is_empty() {
        (
            &gettext("No People"),
            &gettext("People appear here once Immich has recognized faces in your photos."),
        )
    } else {
        (
            &gettext("No Matches"),
            &gettext("No one matches your search."),
        )
    };
    view.empty.set_title(title);
    view.empty.set_description(Some(description));
    view.empty.set_visible(shown.is_empty());
    view.grid.set_visible(!shown.is_empty());
}

/// Ask the server too: the People page loads only the first 500 people.
fn search_server(ui: &Rc<LibraryWindowUi>) {
    let query = ui.people.search.text().trim().to_string();
    if query.chars().count() < 2 {
        return;
    }
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let found = ui.ctx.api_client.search_people(&query).await;
        if ui.people.search.text().trim() != query {
            return;
        }
        match found {
            Ok(found) => {
                let loaded = ui.people.people.borrow();
                let new: Vec<Person> = found
                    .into_iter()
                    .filter(|person| loaded.iter().all(|known| known.id != person.id))
                    .collect();
                drop(loaded);
                if !new.is_empty() {
                    ui.people.found.replace(new);
                    render(&ui);
                }
            }
            Err(err) => log::debug!("People search failed: {err}"),
        }
    });
}

fn visible_people<'a>(people: &'a [Person], query: &str, show_hidden: bool) -> Vec<&'a Person> {
    let mut shown: Vec<&Person> = people
        .iter()
        .filter(|person| show_hidden || !person.is_hidden)
        .filter(|person| query.is_empty() || person.name.to_lowercase().contains(query))
        .collect();
    shown.sort_by_key(|person| !person.is_favorite);
    shown
}

fn tile_avatar(ui: &LibraryWindowUi, person: &Person) -> gtk::Overlay {
    let overlay = gtk::Overlay::builder()
        .child(&avatar(ui, person, AVATAR_SIZE))
        .halign(gtk::Align::Center)
        .build();
    if person.is_favorite {
        let badge = gtk::Image::builder()
            .icon_name("mimick-heart-filled-symbolic")
            .halign(gtk::Align::End)
            .valign(gtk::Align::End)
            .tooltip_text(gettext("Favorite"))
            .css_classes(["mimick-person-favorite"])
            .build();
        overlay.add_overlay(&badge);
    }
    overlay
}

pub(super) fn display_name(person: &Person) -> std::borrow::Cow<'_, str> {
    if person.name.is_empty() {
        gettext("Unnamed").into()
    } else {
        person.name.as_str().into()
    }
}

pub(super) fn avatar(ui: &LibraryWindowUi, person: &Person, size: i32) -> libadwaita::Avatar {
    let avatar = libadwaita::Avatar::new(size, Some(&person.name), true);
    let avatar_for_image = avatar.clone();
    spawn_person_thumbnail(ui.ctx.clone(), person.id.clone(), move |texture| {
        avatar_for_image.set_custom_image(Some(texture))
    });
    avatar
}

fn person_tile(ui: &Rc<LibraryWindowUi>, person: &Person) -> gtk::Overlay {
    let label = gtk::Label::builder()
        .label(display_name(person))
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(14)
        .build();
    if person.name.is_empty() {
        label.add_css_class("dim-label");
    }
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .halign(gtk::Align::Center)
        .build();
    inner.append(&tile_avatar(ui, person));
    inner.append(&label);
    let open = gtk::Button::builder()
        .child(&inner)
        .tooltip_text(fill(
            gettext("Show photos of {name}"),
            &[("name", &display_name(person))],
        ))
        .css_classes(["flat"])
        .build();
    if person.is_hidden {
        open.set_opacity(0.5);
    }
    let (ui_open, id, name) = (
        ui.clone(),
        person.id.clone(),
        display_name(person).to_string(),
    );
    open.connect_clicked(move |_| {
        super::navigation::navigate_source(
            &ui_open,
            LibrarySource::Person {
                id: id.clone(),
                name: name.clone(),
            },
        );
    });
    let menu = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(gettext("More"))
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .css_classes(["circular", "osd"])
        .build();
    popover_menu(&menu, person_menu_items(ui, person));
    let tile = gtk::Overlay::builder().child(&open).build();
    tile.add_overlay(&menu);
    tile
}

pub(super) fn person_menu_items(
    ui: &Rc<LibraryWindowUi>,
    person: &Person,
) -> Vec<(String, Box<dyn Fn()>)> {
    let rename_label = &if person.name.is_empty() {
        gettext("Add Name…")
    } else {
        gettext("Rename…")
    };
    let hide_label = &if person.is_hidden {
        gettext("Unhide")
    } else {
        gettext("Hide")
    };
    let favorite_label = &if person.is_favorite {
        gettext("Remove from Favorites")
    } else {
        gettext("Add to Favorites")
    };
    let (ui_favorite, p_favorite) = (ui.clone(), person.clone());
    let (ui_rename, p_rename) = (ui.clone(), person.clone());
    let (ui_birthday, p_birthday) = (ui.clone(), person.clone());
    let (ui_hide, p_hide) = (ui.clone(), person.clone());
    let (ui_merge, p_merge) = (ui.clone(), person.clone());
    vec![
        (
            rename_label.to_string(),
            Box::new(move || rename(&ui_rename, &p_rename)),
        ),
        (
            favorite_label.to_string(),
            Box::new(move || set_favorite(&ui_favorite, &p_favorite, !p_favorite.is_favorite)),
        ),
        (
            gettext("Set Birthday…"),
            Box::new(move || edit_birthday(&ui_birthday, &p_birthday)),
        ),
        (
            hide_label.to_string(),
            Box::new(move || {
                let update = PersonUpdate {
                    is_hidden: Some(!p_hide.is_hidden),
                    ..PersonUpdate::default()
                };
                save(&ui_hide, &p_hide.id, update);
            }),
        ),
        (
            gettext("Merge People…"),
            Box::new(move || super::person_merge_dialog::present(&ui_merge, &p_merge)),
        ),
    ]
}

fn save(ui: &Rc<LibraryWindowUi>, person_id: &str, update: PersonUpdate) {
    let (ui, person_id) = (ui.clone(), person_id.to_string());
    glib::MainContext::default().spawn_local(async move {
        if let Err(err) = ui.ctx.api_client.update_person(&person_id, &update).await {
            show_alert_dialog(&ui, &gettext("Could Not Update Person"), &err);
            return;
        }
        for list in [&ui.people.people, &ui.people.found] {
            if let Some(person) = list
                .borrow_mut()
                .iter_mut()
                .find(|person| person.id == person_id)
            {
                apply_update(person, &update);
            }
        }
        render(&ui);
        super::source_header::refresh(&ui);
    });
}

pub(super) fn set_favorite(ui: &Rc<LibraryWindowUi>, person: &Person, favorite: bool) {
    let update = PersonUpdate {
        is_favorite: Some(favorite),
        ..PersonUpdate::default()
    };
    save(ui, &person.id, update);
}

fn apply_update(person: &mut Person, update: &PersonUpdate) {
    if let Some(name) = &update.name {
        person.name = name.clone();
    }
    if let Some(hidden) = update.is_hidden {
        person.is_hidden = hidden;
    }
    if let Some(favorite) = update.is_favorite {
        person.is_favorite = favorite;
    }
    if let Some(birth_date) = &update.birth_date {
        person.birth_date = birth_date.clone();
    }
}

fn rename(ui: &Rc<LibraryWindowUi>, person: &Person) {
    let ui_save = ui.clone();
    let person_id = person.id.clone();
    prompt_text(
        ui,
        TextPrompt {
            heading: &if person.name.is_empty() {
                gettext("Add Name")
            } else {
                gettext("Rename Person")
            },
            body: &gettext("This name is used in search and on the People page."),
            initial: &person.name,
            placeholder: &gettext("Name"),
            accept: &gettext("Save"),
        },
        move |name| {
            let update = PersonUpdate {
                name: Some(name),
                ..PersonUpdate::default()
            };
            save(&ui_save, &person_id, update);
        },
    );
}

fn edit_birthday(ui: &Rc<LibraryWindowUi>, person: &Person) {
    let calendar = gtk::Calendar::builder().halign(gtk::Align::Center).build();
    if let Some((year, month, day)) = person.birth_date.as_deref().and_then(parse_date) {
        calendar.set_year(year);
        calendar.set_month(month - 1);
        calendar.set_day(day);
    }
    let dialog = libadwaita::AlertDialog::builder()
        .heading(gettext("Set Birthday"))
        .body(fill(
            gettext("Immich shows {name}'s age on photos."),
            &[("name", &display_name(person))],
        ))
        .extra_child(&calendar)
        .build();
    dialog.add_response("cancel", &gettext("Cancel"));
    if person.birth_date.is_some() {
        dialog.add_response("clear", &gettext("Remove"));
        dialog.set_response_appearance("clear", libadwaita::ResponseAppearance::Destructive);
    }
    dialog.add_response("save", &gettext("Save"));
    dialog.set_response_appearance("save", libadwaita::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");
    let (ui_save, person_id) = (ui.clone(), person.id.clone());
    dialog.connect_response(None, move |_, response| {
        let birth_date = match response {
            "save" => Some(format!(
                "{:04}-{:02}-{:02}",
                calendar.year(),
                calendar.month() + 1,
                calendar.day()
            )),
            "clear" => None,
            _ => return,
        };
        let update = PersonUpdate {
            birth_date: Some(birth_date),
            ..PersonUpdate::default()
        };
        save(&ui_save, &person_id, update);
    });
    dialog.present(Some(&ui.window));
}

fn parse_date(date: &str) -> Option<(i32, i32, i32)> {
    let mut parts = date.get(..10)?.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    Some((year, month, day))
}

#[cfg(test)]
mod tests;
