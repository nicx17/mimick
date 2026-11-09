//! A filter row that picks several people, tags, or albums from a checklist with a filter.

use crate::i18n::fill;
use gettextrs::gettext;
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

/// Draws a choice's picture, such as a person's face, when its row is shown.
pub type ChoiceIcon = Rc<dyn Fn() -> gtk::Widget>;

/// One thing that can be picked.
#[derive(Clone)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub icon: Option<ChoiceIcon>,
}

/// The row in the filters panel, what it can pick from, and what is picked.
pub struct MultiChoice {
    pub row: libadwaita::ActionRow,
    title: String,
    /// The subtitle when nothing is picked, such as "Anyone".
    any: String,
    options: RefCell<Vec<Choice>>,
    chosen: RefCell<Vec<(String, String)>>,
}

impl MultiChoice {
    pub(super) fn new(title: &str, any: &str) -> Rc<Self> {
        let row = libadwaita::ActionRow::builder()
            .title(title)
            .subtitle(any)
            .subtitle_lines(2)
            .activatable(true)
            .build();
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        let choice = Rc::new(Self {
            row,
            title: title.to_string(),
            any: any.to_string(),
            options: RefCell::default(),
            chosen: RefCell::default(),
        });
        let weak = Rc::downgrade(&choice);
        choice.row.connect_activated(move |_| {
            if let Some(choice) = weak.upgrade() {
                choice.open();
            }
        });
        choice
    }

    /// The picked ids, or `None` when nothing is picked.
    pub fn ids(&self) -> Option<Vec<String>> {
        let ids: Vec<String> = self
            .chosen
            .borrow()
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        (!ids.is_empty()).then_some(ids)
    }

    /// What can be picked; picks that are gone are dropped.
    pub fn set_options(&self, options: Vec<Choice>) {
        self.chosen
            .borrow_mut()
            .retain(|(id, _)| options.iter().any(|option| option.id == *id));
        self.options.replace(options);
        self.show_chosen();
    }

    pub fn clear(&self) {
        self.chosen.borrow_mut().clear();
        self.show_chosen();
    }

    /// Pick `id` alone, if it's an option.
    pub fn choose_only(&self, id: &str) {
        let found = self.options.borrow().iter().find(|o| o.id == id).cloned();
        if let Some(option) = found {
            self.chosen.replace(vec![(option.id, option.label)]);
            self.show_chosen();
        }
    }

    fn set_chosen(&self, option: &Choice, chosen: bool) {
        let mut list = self.chosen.borrow_mut();
        list.retain(|(id, _)| *id != option.id);
        if chosen {
            list.push((option.id.clone(), option.label.clone()));
        }
        drop(list);
        self.show_chosen();
    }

    fn show_chosen(&self) {
        let labels: Vec<String> = self
            .chosen
            .borrow()
            .iter()
            .map(|(_, l)| l.clone())
            .collect();
        let subtitle = if labels.is_empty() {
            self.any.clone()
        } else {
            labels.join(", ")
        };
        self.row.set_subtitle(&subtitle);
    }

    /// The checklist; picks apply as they're ticked.
    fn open(self: &Rc<Self>) {
        let filter = gtk::SearchEntry::builder()
            .placeholder_text(fill(gettext("Filter {what}"), &[("what", &self.title)]))
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .valign(gtk::Align::Start)
            .build();
        let options = self.options.borrow().clone();
        for option in &options {
            list.append(&self.option_row(option));
        }
        if options.is_empty() {
            list.append(&nothing_row());
        }
        connect_filter(&filter, &list, &options);
        let clear = gtk::Button::builder().label(gettext("Clear")).build();
        let (weak, list_ref) = (Rc::downgrade(self), list.clone());
        clear.connect_clicked(move |_| {
            if let Some(choice) = weak.upgrade() {
                choice.clear();
                uncheck_all(&list_ref);
            }
        });
        let dialog = checklist_dialog(&self.title, &filter, &list, &clear);
        dialog.present(Some(&self.row));
        filter.grab_focus();
    }

    fn option_row(self: &Rc<Self>, option: &Choice) -> libadwaita::ActionRow {
        let picked = self.chosen.borrow().iter().any(|(id, _)| *id == option.id);
        let check = gtk::CheckButton::builder()
            .active(picked)
            .valign(gtk::Align::Center)
            .build();
        let row = libadwaita::ActionRow::builder()
            .title(&option.label)
            .use_markup(false)
            .activatable_widget(&check)
            .build();
        row.add_prefix(&check);
        if let Some(icon) = &option.icon {
            row.add_prefix(&icon());
        }
        let (weak, option) = (Rc::downgrade(self), option.clone());
        check.connect_toggled(move |check| {
            if let Some(choice) = weak.upgrade() {
                choice.set_chosen(&option, check.is_active());
            }
        });
        row
    }
}

/// Rows whose label doesn't contain the typed text hide.
fn connect_filter(filter: &gtk::SearchEntry, list: &gtk::ListBox, options: &[Choice]) {
    let labels: Rc<Vec<String>> = Rc::new(options.iter().map(|o| o.label.to_lowercase()).collect());
    let query = Rc::new(RefCell::new(String::new()));
    let (labels_ref, query_ref) = (labels.clone(), query.clone());
    list.set_filter_func(move |row| {
        let query = query_ref.borrow();
        let label = usize::try_from(row.index())
            .ok()
            .and_then(|index| labels_ref.get(index));
        query.is_empty() || label.is_some_and(|label| label.contains(query.as_str()))
    });
    let list = list.clone();
    filter.connect_search_changed(move |entry| {
        query.replace(entry.text().trim().to_lowercase());
        list.invalidate_filter();
    });
}

fn uncheck_all(list: &gtk::ListBox) {
    let mut child = list.first_child();
    while let Some(row) = child {
        if let Some(check) = row
            .downcast_ref::<libadwaita::ActionRow>()
            .and_then(libadwaita::ActionRow::activatable_widget)
            .and_downcast::<gtk::CheckButton>()
        {
            check.set_active(false);
        }
        child = row.next_sibling();
    }
}

fn nothing_row() -> gtk::ListBoxRow {
    let label = gtk::Label::builder()
        .label(gettext("Nothing to choose from"))
        .margin_top(12)
        .margin_bottom(12)
        .css_classes(["dim-label"])
        .build();
    gtk::ListBoxRow::builder()
        .child(&label)
        .activatable(false)
        .build()
}

/// The filter above the list, with `clear` at the header's start.
fn checklist_dialog(
    title: &str,
    filter: &gtk::SearchEntry,
    list: &gtk::ListBox,
    clear: &gtk::Button,
) -> libadwaita::Dialog {
    let header = libadwaita::HeaderBar::new();
    header.pack_start(clear);
    let search = gtk::Box::builder()
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(6)
        .build();
    filter.set_hexpand(true);
    search.append(filter);
    let clamp = libadwaita::Clamp::builder()
        .maximum_size(560)
        .margin_top(6)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .child(list)
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
        .title(title)
        .content_width(400)
        .content_height(520)
        .child(&toolbar)
        .build()
}
