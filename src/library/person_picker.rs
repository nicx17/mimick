//! "Who is this?": pick a named person, or create someone new, for a face.

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

type Chosen = Rc<RefCell<Option<PersonChoice>>>;

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

/// Whether `name` is worth creating: typed, and not anyone's name already.
fn is_new_name(people: &[Person], name: &str) -> bool {
    !name.is_empty() && !people.iter().any(|p| p.name.eq_ignore_ascii_case(name))
}

/// The first page: search, everyone named, and a way to create someone new.
struct PickPage {
    root: gtk::Box,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    create: gtk::Button,
}

/// The second page, which names the person to create.
struct NamePage {
    root: gtk::Box,
    back: gtk::Button,
    name: gtk::Entry,
    tag: gtk::Button,
}

fn build(
    ui: &Rc<LibraryWindowUi>,
    people: Vec<Person>,
    on_pick: Rc<dyn Fn(PersonChoice)>,
) -> gtk::Popover {
    let popover = gtk::Popover::builder().autohide(true).build();
    let people = Rc::new(people);
    let pick = pick_page(ui, &people);
    let naming = name_page();
    let pages = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::SlideLeftRight)
        .vhomogeneous(false)
        .interpolate_size(true)
        .width_request(260)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    pages.add_named(&pick.root, Some("pick"));
    pages.add_named(&naming.root, Some("name"));
    let chosen: Chosen = Rc::default();
    connect_picks(&popover, &pick, &people, &chosen);
    connect_search(&pick, &people);
    connect_pages(&pages, &pick, &naming);
    connect_naming(&popover, &naming, &chosen);
    popover.connect_closed(move |popover| {
        let popover = popover.clone();
        glib::idle_add_local_once(move || popover.unparent());
        if let Some(choice) = chosen.take() {
            on_pick(choice);
        }
    });
    popover.set_child(Some(&pages));
    let search = pick.search.clone();
    popover.connect_map(move |_| {
        search.grab_focus();
    });
    popover
}

/// Title, search, and Create stay put; the list scrolls, so the popover fits small screens.
fn pick_page(ui: &LibraryWindowUi, people: &[Person]) -> PickPage {
    let search = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search or type a new name"))
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .valign(gtk::Align::Start)
        .build();
    for person in people {
        list.append(&person_row(ui, person));
    }
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(300)
        .propagate_natural_height(true)
        .child(&list)
        .build();
    let create = gtk::Button::with_label(&gettext("Create Person…"));
    let root = page_box();
    root.append(&heading(&gettext("Who Is This?")));
    root.append(&search);
    root.append(&scroller);
    root.append(&create);
    PickPage {
        root,
        search,
        list,
        create,
    }
}

/// A name field and Tag Face, under a back button to the list.
fn name_page() -> NamePage {
    let back = gtk::Button::builder()
        .icon_name("go-previous-symbolic")
        .tooltip_text(gettext("Back"))
        .css_classes(["flat"])
        .build();
    let top = gtk::Box::builder().spacing(6).build();
    top.append(&back);
    top.append(&heading(&gettext("Create Person")));
    let name = gtk::Entry::builder()
        .placeholder_text(gettext("Name"))
        .build();
    let tag = gtk::Button::builder()
        .label(gettext("Tag Face"))
        .sensitive(false)
        .css_classes(["suggested-action"])
        .build();
    let root = page_box();
    root.append(&top);
    root.append(&name);
    root.append(&tag);
    NamePage {
        root,
        back,
        name,
        tag,
    }
}

fn page_box() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build()
}

fn heading(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(["heading"])
        .build()
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

/// A row, or Enter on a name nobody has, records the choice and closes; closing hands it over.
fn connect_picks(
    popover: &gtk::Popover,
    pick: &PickPage,
    people: &Rc<Vec<Person>>,
    chosen: &Chosen,
) {
    let (popover_row, people_row, chosen_row) = (popover.clone(), people.clone(), chosen.clone());
    pick.list.connect_row_activated(move |_, row| {
        if let Some(person) = people_row.get(row.index() as usize) {
            chosen_row.replace(Some(PersonChoice::Existing(person.clone())));
            popover_row.popdown();
        }
    });
    let (popover, people, chosen) = (popover.clone(), people.clone(), chosen.clone());
    pick.search.connect_activate(move |entry| {
        let name = entry.text().trim().to_string();
        if is_new_name(&people, &name) {
            chosen.replace(Some(PersonChoice::New(name)));
            popover.popdown();
        }
    });
}

/// Filters the list by name as the search changes.
fn connect_search(pick: &PickPage, people: &[Person]) {
    let query = Rc::new(RefCell::new(String::new()));
    let filter_query = query.clone();
    let names: Vec<String> = people.iter().map(|p| p.name.to_lowercase()).collect();
    pick.list.set_filter_func(move |row| {
        let query = filter_query.borrow();
        names
            .get(row.index() as usize)
            .is_some_and(|name| name.contains(query.as_str()))
    });
    let list = pick.list.clone();
    pick.search.connect_search_changed(move |entry| {
        *query.borrow_mut() = entry.text().trim().to_lowercase();
        list.invalidate_filter();
    });
}

/// Create Person… opens the name page with whatever was typed; Back returns to the list.
fn connect_pages(pages: &gtk::Stack, pick: &PickPage, naming: &NamePage) {
    let (to_name, search, name) = (pages.clone(), pick.search.clone(), naming.name.clone());
    pick.create.connect_clicked(move |_| {
        name.set_text(search.text().trim());
        to_name.set_visible_child_name("name");
        name.grab_focus();
    });
    let (to_pick, search) = (pages.clone(), pick.search.clone());
    naming.back.connect_clicked(move |_| {
        to_pick.set_visible_child_name("pick");
        search.grab_focus();
    });
}

/// Tag Face, or Enter in the name field, creates the person once a name is typed.
fn connect_naming(popover: &gtk::Popover, naming: &NamePage, chosen: &Chosen) {
    let tag = naming.tag.clone();
    naming.name.connect_changed(move |entry| {
        tag.set_sensitive(!entry.text().trim().is_empty());
    });
    let (popover, name, chosen) = (popover.clone(), naming.name.clone(), chosen.clone());
    let create = Rc::new(move || {
        let typed = name.text().trim().to_string();
        if !typed.is_empty() {
            chosen.replace(Some(PersonChoice::New(typed)));
            popover.popdown();
        }
    });
    let on_tag = create.clone();
    naming.tag.connect_clicked(move |_| on_tag());
    naming.name.connect_activate(move |_| create());
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

    #[test]
    fn enter_creates_only_names_nobody_has() {
        let people = [person("Ana")];
        assert!(is_new_name(&people, "Bea"));
        assert!(!is_new_name(&people, "ana"));
        assert!(!is_new_name(&people, ""));
    }
}
