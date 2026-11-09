//! A day picked from a calendar popover, or left open ("Any Day"), as a preferences row.

use gettextrs::gettext;
use std::cell::RefCell;
use std::rc::Rc;

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::i18n::format_day;

/// How a picked day is kept: `YYYY-MM-DD`.
pub(in crate::library) const DAY_FORMAT: &str = "%Y-%m-%d";

/// Local midnight at the start of a `YYYY-MM-DD` day.
pub(in crate::library) fn day_start(day: Option<&str>) -> Option<DateTime<Utc>> {
    let day = NaiveDate::parse_from_str(day?, DAY_FORMAT).ok()?;
    let local = Local
        .from_local_datetime(&day.and_time(NaiveTime::MIN))
        .earliest()?;
    Some(local.with_timezone(&Utc))
}

/// A day picked from a calendar popover, or open ("Any").
pub(in crate::library) struct DayRow {
    pub(in crate::library) row: libadwaita::ActionRow,
    pub(in crate::library) day: Rc<RefCell<Option<String>>>,
}

impl DayRow {
    pub(in crate::library) fn new(title: &str, day: Option<&str>) -> Self {
        let row = libadwaita::ActionRow::builder().title(title).build();
        let day = Rc::new(RefCell::new(day.map(str::to_owned)));
        let calendar = gtk::Calendar::new();
        let clear = gtk::Button::builder()
            .label(gettext("Any Day"))
            .css_classes(["flat"])
            .build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        content.append(&calendar);
        content.append(&clear);
        let pick = gtk::MenuButton::builder()
            .popover(&gtk::Popover::builder().child(&content).build())
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        row.add_suffix(&pick);
        row.set_activatable_widget(Some(&pick));
        show_day(&pick, &calendar, day.borrow().as_deref());
        connect_day(&pick, &calendar, &clear, &day);
        Self { row, day }
    }
}

fn show_day(pick: &gtk::MenuButton, calendar: &gtk::Calendar, day: Option<&str>) {
    let parsed = day.and_then(|day| NaiveDate::parse_from_str(day, DAY_FORMAT).ok());
    match parsed {
        Some(date) => {
            pick.set_label(&format_day(date, &gettext("%-d %b %Y")));
            let (month, day) = (date.month() as i32, date.day() as i32);
            if let Ok(glib_day) = glib::DateTime::from_local(date.year(), month, day, 0, 0, 0.0) {
                calendar.set_date(&glib_day);
            }
        }
        None => pick.set_label(&gettext("Any Day")),
    }
}

fn connect_day(
    pick: &gtk::MenuButton,
    calendar: &gtk::Calendar,
    clear: &gtk::Button,
    day: &Rc<RefCell<Option<String>>>,
) {
    let (pick_ref, day_ref) = (pick.clone(), day.clone());
    calendar.connect_day_selected(move |calendar| {
        let picked = calendar.date();
        let text = format!(
            "{:04}-{:02}-{:02}",
            picked.year(),
            picked.month(),
            picked.day_of_month()
        );
        show_day(&pick_ref, calendar, Some(&text));
        day_ref.replace(Some(text));
        pick_ref.popdown();
    });
    let (pick, calendar, day) = (pick.clone(), calendar.clone(), day.clone());
    clear.connect_clicked(move |_| {
        day.replace(None);
        show_day(&pick, &calendar, None);
        pick.popdown();
    });
}
