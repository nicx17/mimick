use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::Person;

use super::LibraryWindowUi;
use super::context_menu::show_alert_dialog;
use super::dialogs::confirm;
use super::people_view::{avatar, display_name};

type Selection = Rc<RefCell<HashSet<String>>>;

pub(super) fn present(ui: &Rc<LibraryWindowUi>, target: &Person) {
    if !ui.people.people.borrow().is_empty() {
        present_list(ui, target);
        return;
    }
    let (ui, target) = (ui.clone(), target.clone());
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.fetch_people(true).await {
            Ok(people) => {
                *ui.people.people.borrow_mut() = people;
                present_list(&ui, &target);
            }
            Err(err) => show_alert_dialog(&ui, &gettext("Could Not Load People"), &err),
        }
    });
}

fn present_list(ui: &Rc<LibraryWindowUi>, target: &Person) {
    let others: Vec<Person> = ui
        .people
        .people
        .borrow()
        .iter()
        .filter(|person| person.id != target.id)
        .cloned()
        .collect();
    let selected: Selection = Rc::default();
    let merge = gtk::Button::builder()
        .label(gettext("Merge"))
        .sensitive(false)
        .css_classes(["destructive-action"])
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .valign(gtk::Align::Start)
        .build();
    for person in &others {
        list.append(&merge_row(ui, person, &selected, &merge));
    }
    let search = filtering_search(&list, &others);
    let dialog = build_dialog(target, &merge, &search, &list);
    connect_merge(ui, &dialog, &merge, target, selected);
    dialog.present(Some(&ui.window));
}

fn filtering_search(list: &gtk::ListBox, people: &[Person]) -> gtk::SearchEntry {
    let search = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search people"))
        .build();
    let query = Rc::new(RefCell::new(String::new()));
    let names: Vec<String> = people.iter().map(|p| p.name.to_lowercase()).collect();
    let query_for_filter = query.clone();
    list.set_filter_func(move |row| {
        let query = query_for_filter.borrow();
        query.is_empty()
            || names
                .get(row.index() as usize)
                .is_some_and(|name| name.contains(query.as_str()))
    });
    let list = list.clone();
    search.connect_search_changed(move |entry| {
        *query.borrow_mut() = entry.text().to_lowercase();
        list.invalidate_filter();
    });
    search
}

fn build_dialog(
    target: &Person,
    merge: &gtk::Button,
    search: &gtk::SearchEntry,
    list: &gtk::ListBox,
) -> libadwaita::Dialog {
    let name = display_name(target);
    let note = gtk::Label::builder()
        .label(fill(gettext("Pick the people who are really {name}. Their faces move to {name} and they're removed. This can't be undone."), &[("name", name.as_ref())]))
        .wrap(true)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(6)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    body.append(&note);
    body.append(search);
    body.append(list);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .vexpand(true)
        .child(&body)
        .build();
    let header = libadwaita::HeaderBar::new();
    header.pack_end(merge);
    let toolbar = libadwaita::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroller));
    libadwaita::Dialog::builder()
        .title(fill(
            gettext("Merge Into {name}"),
            &[("name", name.as_ref())],
        ))
        .content_width(420)
        .content_height(560)
        .child(&toolbar)
        .build()
}

fn connect_merge(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::Dialog,
    merge: &gtk::Button,
    target: &Person,
    selected: Selection,
) {
    let (ui, dialog, target) = (ui.clone(), dialog.clone(), target.clone());
    merge.connect_clicked(move |_| {
        let ids: Vec<String> = selected.borrow().iter().cloned().collect();
        let n = ids.len();
        let heading = fill(
            ngettext(
                "Merge {count} person into {name}?",
                "Merge {count} people into {name}?",
                plural_n(n),
            ),
            &[("count", &n.to_string()), ("name", &display_name(&target))],
        );
        let (ui_merge, dialog, target_id) = (ui.clone(), dialog.clone(), target.id.clone());
        confirm(
            &ui,
            &heading,
            &gettext("This can't be undone."),
            &gettext("Merge"),
            move || {
                dialog.close();
                merge_people(ui_merge.clone(), target_id.clone(), ids.clone());
            },
        );
    });
}

fn merge_row(
    ui: &LibraryWindowUi,
    person: &Person,
    selected: &Selection,
    merge: &gtk::Button,
) -> libadwaita::ActionRow {
    let check = gtk::CheckButton::builder()
        .valign(gtk::Align::Center)
        .build();
    let row = libadwaita::ActionRow::builder()
        .title(display_name(person))
        .use_markup(false)
        .activatable_widget(&check)
        .build();
    row.add_prefix(&avatar(ui, person, 40));
    row.add_suffix(&check);
    let (selected, merge, id) = (selected.clone(), merge.clone(), person.id.clone());
    check.connect_toggled(move |check| {
        let mut selected = selected.borrow_mut();
        if check.is_active() {
            selected.insert(id.clone());
        } else {
            selected.remove(&id);
        }
        merge.set_sensitive(!selected.is_empty());
    });
    row
}

fn merge_people(ui: Rc<LibraryWindowUi>, target_id: String, ids: Vec<String>) {
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.merge_people(&target_id, &ids).await {
            Ok(()) => {
                super::source_header::refresh(&ui);
                super::people_view::reload(ui);
            }
            Err(err) => show_alert_dialog(&ui, &gettext("Could Not Merge People"), &err),
        }
    });
}
