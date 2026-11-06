//! "Who is this?": pick a named person, or type a new name, for a face.

use crate::i18n::fill;
use gettextrs::gettext;
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use super::LibraryWindowUi;
use super::people_view::avatar;
use crate::api_client::Person;

/// The picked person, or a name for someone new.
pub(super) enum PersonChoice {
    Existing(Person),
    New(String),
}

/// Opens the picker in a popover on `parent`, pointing at `at` (in `parent`'s coordinates).
/// `on_pick` runs once, after the popover closes, and not at all when it's dismissed.
pub(super) fn present(
    ui: &Rc<LibraryWindowUi>,
    parent: &impl IsA<gtk::Widget>,
    at: gtk::gdk::Rectangle,
    on_pick: impl Fn(PersonChoice) + 'static,
) {
    let (ui, parent) = (ui.clone(), parent.clone().upcast::<gtk::Widget>());
    let on_pick: Rc<dyn Fn(PersonChoice)> = Rc::new(on_pick);
    glib::MainContext::default().spawn_local(async move {
        if ui.people.people.borrow().is_empty()
            && let Ok(people) = ui.ctx.api_client.fetch_people(true).await
        {
            *ui.people.people.borrow_mut() = people;
        }
        let named = named_people(&ui.people.people.borrow());
        let popover = build(&ui, named, on_pick);
        popover.set_parent(&parent);
        popover.set_pointing_to(Some(&at));
        popover.popup();
    });
}

/// Named people, A to Z; unnamed clusters would bury them.
fn named_people(people: &[Person]) -> Vec<Person> {
    let mut named: Vec<Person> = people
        .iter()
        .filter(|person| !person.name.trim().is_empty())
        .cloned()
        .collect();
    named.sort_by_key(|person| person.name.to_lowercase());
    named
}

fn build(
    ui: &Rc<LibraryWindowUi>,
    people: Vec<Person>,
    on_pick: Rc<dyn Fn(PersonChoice)>,
) -> gtk::Popover {
    let popover = gtk::Popover::builder().autohide(true).build();
    let search = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search or type a new name"))
        .build();
    let create = gtk::Button::builder()
        .halign(gtk::Align::Start)
        .visible(false)
        .css_classes(["flat"])
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .valign(gtk::Align::Start)
        .build();
    let people = Rc::new(people);
    for person in people.iter() {
        list.append(&person_row(ui, person));
    }
    let chosen = Rc::new(RefCell::new(None::<PersonChoice>));
    let typed = Rc::new(RefCell::new(String::new()));
    connect_choices(&popover, &list, &create, &people, &chosen, &typed);
    connect_search(&search, &list, &create, &people, &typed);
    popover.connect_closed(move |popover| {
        let popover = popover.clone();
        glib::idle_add_local_once(move || popover.unparent());
        if let Some(choice) = chosen.take() {
            on_pick(choice);
        }
    });
    popover.set_child(Some(&layout(&search, &create, &list)));
    let search_focus = search.clone();
    popover.connect_map(move |_| {
        search_focus.grab_focus();
    });
    popover
}

/// Title, search, and "Add" stay put; the list scrolls, so the popover fits small screens.
fn layout(search: &gtk::SearchEntry, create: &gtk::Button, list: &gtk::ListBox) -> gtk::Widget {
    let title = gtk::Label::builder()
        .label(gettext("Who Is This?"))
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(300)
        .propagate_natural_height(true)
        .child(list)
        .build();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .width_request(260)
        .build();
    body.append(&title);
    body.append(search);
    body.append(create);
    body.append(&scroller);
    body.upcast()
}

fn person_row(ui: &LibraryWindowUi, person: &Person) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .title(&person.name)
        .use_markup(false)
        .activatable(true)
        .build();
    row.add_prefix(&avatar(ui, person, 36));
    row
}

/// A row or the "Add" button records the choice and closes; closing hands it over.
fn connect_choices(
    popover: &gtk::Popover,
    list: &gtk::ListBox,
    create: &gtk::Button,
    people: &Rc<Vec<Person>>,
    chosen: &Rc<RefCell<Option<PersonChoice>>>,
    typed: &Rc<RefCell<String>>,
) {
    let (popover_row, people_row, chosen_row) = (popover.clone(), people.clone(), chosen.clone());
    list.connect_row_activated(move |_, row| {
        if let Some(person) = people_row.get(row.index() as usize) {
            chosen_row.replace(Some(PersonChoice::Existing(person.clone())));
            popover_row.popdown();
        }
    });
    let (popover_new, chosen_new, typed) = (popover.clone(), chosen.clone(), typed.clone());
    create.connect_clicked(move |_| {
        let name = typed.borrow().clone();
        if !name.is_empty() {
            chosen_new.replace(Some(PersonChoice::New(name)));
            popover_new.popdown();
        }
    });
}

/// Filters the list by name; a name nobody has yet offers to add that person.
fn connect_search(
    search: &gtk::SearchEntry,
    list: &gtk::ListBox,
    create: &gtk::Button,
    people: &Rc<Vec<Person>>,
    typed: &Rc<RefCell<String>>,
) {
    let query = Rc::new(RefCell::new(String::new()));
    let (filter_query, names) = (
        query.clone(),
        people
            .iter()
            .map(|p| p.name.to_lowercase())
            .collect::<Vec<_>>(),
    );
    list.set_filter_func(move |row| {
        let query = filter_query.borrow();
        names
            .get(row.index() as usize)
            .is_some_and(|name| name.contains(query.as_str()))
    });
    let create_on_enter = create.clone();
    let (list, create, people, typed) =
        (list.clone(), create.clone(), people.clone(), typed.clone());
    search.connect_search_changed(move |entry| {
        let text = entry.text().trim().to_string();
        *query.borrow_mut() = text.to_lowercase();
        list.invalidate_filter();
        let exists = people.iter().any(|p| p.name.eq_ignore_ascii_case(&text));
        create.set_visible(!text.is_empty() && !exists);
        create.set_label(&fill(gettext("Add “{name}”"), &[("name", &text)]));
        *typed.borrow_mut() = text;
    });
    search.connect_activate(move |_| {
        if create_on_enter.is_visible() {
            create_on_enter.emit_clicked();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: &str) -> Person {
        serde_json::from_value(serde_json::json!({ "id": name, "name": name })).unwrap()
    }

    #[test]
    fn only_named_people_are_offered_in_order() {
        let named = named_people(&[person("bea"), person(""), person("Ana"), person("  ")]);
        let names: Vec<&str> = named.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Ana", "bea"]);
    }
}
