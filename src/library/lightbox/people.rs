use crate::i18n::fill;
use gettextrs::{gettext, ngettext};
use std::cell::RefCell;
use std::rc::Rc;

use chrono::{DateTime, Datelike, NaiveDate};
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::Person;
use crate::library::LibraryWindowUi;
use crate::library::context_menu::{connect_context_menu, menu_key};
use crate::library::navigation;
use crate::library::people_view::{avatar, display_name};
use crate::library::state::LibrarySource;

const AVATAR_SIZE: i32 = 56;
/// Rings a person whose face is pointed at on the photo.
const MARKED: &str = "mimick-person-marked";

/// Gets the person's id while pointed at or focused, `None` once left.
type PointHook = Box<dyn Fn(Option<&str>)>;
/// Gets the person and their tile, with a point on it to open a menu at.
type MenuHook = Box<dyn Fn(&Person, &gtk::Button, f64, f64)>;

/// What the lightbox does when a person is pointed at, focused, or asked for their menu.
pub(super) struct PeopleHooks {
    pub(super) point: PointHook,
    /// Right-click, long-press, or the Menu key on a person.
    pub(super) menu: MenuHook,
}

pub(super) struct PeopleSection {
    pub root: libadwaita::PreferencesGroup,
    /// Opens the face editor; shown on the user's own photos.
    pub edit: gtk::Button,
    faces: gtk::FlowBox,
    untagged: gtk::Label,
    /// Each shown person's id and tile, to ring the one pointed at on the photo.
    tiles: RefCell<Vec<(String, gtk::Button)>>,
    hooks: RefCell<Option<Rc<PeopleHooks>>>,
}

impl PeopleSection {
    pub(super) fn new() -> Self {
        let root = libadwaita::PreferencesGroup::builder()
            .title(gettext("People"))
            .visible(false)
            .build();
        let faces = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .column_spacing(6)
            .row_spacing(6)
            .max_children_per_line(6)
            .homogeneous(true)
            .build();
        let edit = gtk::Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text(gettext("Tag People…"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        root.set_header_suffix(Some(&edit));
        let untagged = gtk::Label::builder()
            .label(gettext("No one is tagged yet."))
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        root.add(&faces);
        root.add(&untagged);
        Self {
            root,
            edit,
            faces,
            untagged,
            tiles: RefCell::default(),
            hooks: RefCell::default(),
        }
    }

    pub(super) fn bind(&self, hooks: PeopleHooks) {
        self.hooks.replace(Some(Rc::new(hooks)));
    }

    pub(super) fn clear(&self) {
        self.root.set_visible(false);
        self.faces.remove_all();
        self.tiles.borrow_mut().clear();
    }

    /// Rings `person`'s tile, and only theirs; `None` rings no one.
    pub(super) fn mark(&self, person: Option<&str>) {
        for (id, tile) in self.tiles.borrow().iter() {
            if person == Some(id.as_str()) {
                tile.add_css_class(MARKED);
            } else {
                tile.remove_css_class(MARKED);
            }
        }
    }

    /// `editable` keeps the section, with its edit button, even when no one is tagged.
    pub(super) fn show(
        &self,
        ui: &Rc<LibraryWindowUi>,
        people: &[Person],
        taken: Option<&str>,
        editable: bool,
    ) {
        self.clear();
        let visible: Vec<&Person> = people.iter().filter(|p| !p.is_hidden).collect();
        for person in &visible {
            let age = person
                .birth_date
                .as_deref()
                .zip(taken)
                .and_then(|(birth, taken)| age_text(birth, taken));
            let tile = person_button(ui, person, age.as_deref());
            self.connect_tile(&tile, person, editable);
            self.tiles
                .borrow_mut()
                .push((person.id.clone(), tile.clone()));
            let slot = gtk::FlowBoxChild::builder()
                .child(&tile)
                .focusable(false)
                .css_classes(["mimick-person-slot"])
                .build();
            self.faces.insert(&slot, -1);
        }
        self.faces.set_visible(!visible.is_empty());
        self.untagged.set_visible(visible.is_empty());
        self.edit.set_visible(editable);
        self.root.set_visible(editable || !visible.is_empty());
    }

    /// Pointing at or focusing a tile reports its person; on your photos it has a menu too.
    fn connect_tile(&self, tile: &gtk::Button, person: &Person, editable: bool) {
        let Some(hooks) = self.hooks.borrow().clone() else {
            return;
        };
        connect_pointing(tile, &hooks, &person.id);
        if editable {
            connect_tile_menu(tile, hooks, person);
        }
    }
}

/// Hover and keyboard focus both say which person is meant; leaving either clears it.
fn connect_pointing(tile: &gtk::Button, hooks: &Rc<PeopleHooks>, id: &str) {
    let motion = gtk::EventControllerMotion::new();
    let focus = gtk::EventControllerFocus::new();
    let (on_hover, on_focus) = (hooks.clone(), hooks.clone());
    let (hover_id, focus_id) = (id.to_string(), id.to_string());
    motion.connect_enter(move |_, _, _| (on_hover.point)(Some(&hover_id)));
    focus.connect_enter(move |_| (on_focus.point)(Some(&focus_id)));
    let (off_hover, off_focus) = (hooks.clone(), hooks.clone());
    motion.connect_leave(move |_| (off_hover.point)(None));
    focus.connect_leave(move |_| (off_focus.point)(None));
    tile.add_controller(motion);
    tile.add_controller(focus);
}

/// Right-click, long-press, and Shift+F10 or the Menu key open the person's menu.
fn connect_tile_menu(tile: &gtk::Button, hooks: Rc<PeopleHooks>, person: &Person) {
    let person = person.clone();
    let open = Rc::new(move |tile: &gtk::Button, x: f64, y: f64| {
        (hooks.menu)(&person, tile, x, y);
    });
    let (on_press, pressed) = (open.clone(), tile.downgrade());
    connect_context_menu(tile, move |x, y| {
        if let Some(tile) = pressed.upgrade() {
            on_press(&tile, x, y);
        }
    });
    let keyed = tile.downgrade();
    tile.add_controller(menu_key(move || {
        if let Some(tile) = keyed.upgrade() {
            let (w, h) = (tile.width(), tile.height());
            open(&tile, f64::from(w) / 2.0, f64::from(h) / 2.0);
        }
    }));
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
        .tooltip_text(fill(
            gettext("Show photos of {name}"),
            &[("name", &name.to_string())],
        ))
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
        0..=11 => fill(gettext("Age {months}"), &[("months", &month_count(months))]),
        13..=23 => fill(
            gettext("Age 1 year, {months}"),
            &[("months", &month_count(months - 12))],
        ),
        _ => fill(gettext("Age {age}"), &[("age", &(months / 12).to_string())]),
    })
}

fn full_months(from: NaiveDate, to: NaiveDate) -> Option<u32> {
    let months = (to.year() - from.year()) * 12 + to.month() as i32
        - from.month() as i32
        - i32::from(to.day() < from.day());
    u32::try_from(months).ok()
}

fn month_count(months: u32) -> String {
    fill(
        ngettext("{n} month", "{n} months", months),
        &[("n", &months.to_string())],
    )
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
