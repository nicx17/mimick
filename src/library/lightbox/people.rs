use std::rc::Rc;

use chrono::{DateTime, Datelike, NaiveDate};
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::Person;
use crate::library::LibraryWindowUi;
use crate::library::navigation;
use crate::library::people_view::{avatar, display_name};
use crate::library::state::LibrarySource;

const AVATAR_SIZE: i32 = 56;

pub(super) struct PeopleSection {
    pub root: libadwaita::PreferencesGroup,
    faces: gtk::FlowBox,
}

impl PeopleSection {
    pub(super) fn new() -> Self {
        let root = libadwaita::PreferencesGroup::builder()
            .title("People")
            .visible(false)
            .build();
        let faces = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .column_spacing(6)
            .row_spacing(6)
            .max_children_per_line(6)
            .homogeneous(true)
            .build();
        root.add(&faces);
        Self { root, faces }
    }

    pub(super) fn clear(&self) {
        self.root.set_visible(false);
        self.faces.remove_all();
    }

    pub(super) fn show(&self, ui: &Rc<LibraryWindowUi>, people: &[Person], taken: Option<&str>) {
        self.faces.remove_all();
        let visible: Vec<&Person> = people.iter().filter(|p| !p.is_hidden).collect();
        for person in &visible {
            let age = person
                .birth_date
                .as_deref()
                .zip(taken)
                .and_then(|(birth, taken)| age_text(birth, taken));
            let slot = gtk::FlowBoxChild::builder()
                .child(&person_button(ui, person, age.as_deref()))
                .focusable(false)
                .css_classes(["mimick-person-slot"])
                .build();
            self.faces.insert(&slot, -1);
        }
        self.root.set_visible(!visible.is_empty());
    }
}

fn person_button(ui: &Rc<LibraryWindowUi>, person: &Person, age: Option<&str>) -> gtk::Button {
    let name = display_name(person).to_string();
    let label = gtk::Label::builder()
        .label(&name)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(10)
        .css_classes(["caption"])
        .build();
    if person.name.is_empty() {
        label.add_css_class("dim-label");
    }
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    content.append(&avatar(ui, person, AVATAR_SIZE));
    content.append(&label);
    if let Some(age) = age {
        content.append(
            &gtk::Label::builder()
                .label(age)
                .wrap(true)
                .justify(gtk::Justification::Center)
                .max_width_chars(10)
                .css_classes(["caption", "dim-label"])
                .build(),
        );
    }
    let button = gtk::Button::builder()
        .child(&content)
        .tooltip_text(format!("Show photos of {name}"))
        .css_classes(["flat"])
        .build();
    let (ui, id) = (ui.clone(), person.id.clone());
    button.connect_clicked(move |_| {
        ui.nav.pop();
        let source = LibrarySource::Person {
            id: id.clone(),
            name: name.clone(),
        };
        navigation::navigate_source(&ui, source);
    });
    button
}

fn age_text(birth_date: &str, taken: &str) -> Option<String> {
    let birth = NaiveDate::parse_from_str(birth_date, "%Y-%m-%d").ok()?;
    let taken = DateTime::parse_from_rfc3339(taken).ok()?.naive_utc().date();
    let months = full_months(birth, taken)?;
    Some(match months {
        0..=11 => format!("Age {}", month_count(months)),
        13..=23 => format!("Age 1 year, {}", month_count(months - 12)),
        _ => format!("Age {}", months / 12),
    })
}

fn full_months(from: NaiveDate, to: NaiveDate) -> Option<u32> {
    let months = (to.year() - from.year()) * 12 + to.month() as i32
        - from.month() as i32
        - i32::from(to.day() < from.day());
    u32::try_from(months).ok()
}

fn month_count(months: u32) -> String {
    match months {
        1 => "1 month".to_string(),
        n => format!("{n} months"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn age(birth: &str, taken: &str) -> Option<String> {
        age_text(birth, &format!("{taken}T12:00:00.000Z"))
    }

    #[test]
    fn babies_show_months() {
        assert_eq!(
            age("2020-03-15", "2020-04-14").as_deref(),
            Some("Age 0 months")
        );
        assert_eq!(
            age("2020-03-15", "2020-04-15").as_deref(),
            Some("Age 1 month")
        );
        assert_eq!(
            age("2020-03-15", "2020-06-20").as_deref(),
            Some("Age 3 months")
        );
    }

    #[test]
    fn second_year_shows_year_and_months() {
        assert_eq!(age("2020-03-15", "2021-03-15").as_deref(), Some("Age 1"));
        assert_eq!(
            age("2020-03-15", "2021-05-15").as_deref(),
            Some("Age 1 year, 2 months")
        );
    }

    #[test]
    fn adults_show_years_counted_from_the_birthday() {
        assert_eq!(age("1990-06-01", "2024-05-31").as_deref(), Some("Age 33"));
        assert_eq!(age("1990-06-01", "2024-06-01").as_deref(), Some("Age 34"));
    }

    #[test]
    fn no_age_before_birth_or_for_bad_dates() {
        assert_eq!(age("2024-06-01", "2024-05-31"), None);
        assert_eq!(age("not a date", "2024-05-31"), None);
        assert_eq!(age_text("1990-06-01", "garbage"), None);
    }
}
