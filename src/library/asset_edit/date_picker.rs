//! Calendar and time controls used by the asset information editor.

use std::cell::RefCell;
use std::rc::Rc;

use chrono::{Local, NaiveDateTime};
use gtk::prelude::*;
use libadwaita::prelude::*;

use super::DATE_FORMAT;

/// Calendar plus hour/minute/second spinners for picking a local date and time.
struct DatePicker {
    calendar: gtk::Calendar,
    hour: gtk::SpinButton,
    minute: gtk::SpinButton,
    second: gtk::SpinButton,
}

impl DatePicker {
    fn new(parts: DateParts) -> Self {
        let calendar = gtk::Calendar::new();
        if let Ok(day) =
            glib::DateTime::from_local(parts.0, parts.1 as i32, parts.2 as i32, 0, 0, 0.0)
        {
            calendar.set_date(&day);
        }
        let spin = |max: f64, value: u32| {
            let button = gtk::SpinButton::with_range(0.0, max, 1.0);
            button.set_value(f64::from(value));
            button.set_wrap(true);
            button
        };
        Self {
            calendar,
            hour: spin(23.0, parts.3),
            minute: spin(59.0, parts.4),
            second: spin(59.0, parts.5),
        }
    }

    /// "HH : MM : SS" spinner row.
    fn time_row(&self) -> gtk::Box {
        let time = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .halign(gtk::Align::Center)
            .build();
        time.append(&self.hour);
        time.append(&gtk::Label::new(Some(":")));
        time.append(&self.minute);
        time.append(&gtk::Label::new(Some(":")));
        time.append(&self.second);
        time
    }

    fn picked_text(&self) -> Option<String> {
        let day = self.calendar.date();
        compose_date_text(
            day.year(),
            day.month() as u32,
            day.day_of_month() as u32,
            self.hour.value() as u32,
            self.minute.value() as u32,
            self.second.value() as u32,
        )
    }
}

/// Popover with the date picker; "Set Date" writes the result back to the row.
pub(super) fn date_popover(
    row: &libadwaita::ActionRow,
    text: &Rc<RefCell<String>>,
) -> gtk::Popover {
    let picker = DatePicker::new(date_parts(&text.borrow()).unwrap_or_else(now_parts));
    let apply = gtk::Button::builder()
        .label("Set Date")
        .css_classes(["suggested-action"])
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    content.append(&picker.calendar);
    content.append(&picker.time_row());
    content.append(&apply);
    let popover = gtk::Popover::builder().child(&content).build();
    let (row, text, popover_ref) = (row.clone(), text.clone(), popover.clone());
    apply.connect_clicked(move |_| {
        if let Some(picked) = picker.picked_text() {
            row.set_subtitle(&picked);
            *text.borrow_mut() = picked;
        }
        popover_ref.popdown();
    });
    popover
}

pub(super) type DateParts = (i32, u32, u32, u32, u32, u32);

/// `(year, month, day, hour, minute, second)` from a `DATE_FORMAT` string.
pub(super) fn date_parts(text: &str) -> Option<DateParts> {
    use chrono::{Datelike, Timelike};
    let dt = NaiveDateTime::parse_from_str(text.trim(), DATE_FORMAT).ok()?;
    Some((
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
    ))
}

/// Picker start for assets without a capture date: now, local time.
pub(super) fn now_parts() -> DateParts {
    let now = Local::now().naive_local().format(DATE_FORMAT).to_string();
    date_parts(&now).unwrap_or((2000, 1, 1, 12, 0, 0))
}

/// `DATE_FORMAT` text for picked values, or `None` for an impossible date.
pub(super) fn compose_date_text(
    year: i32,
    month: u32,
    day: u32,
    h: u32,
    m: u32,
    s: u32,
) -> Option<String> {
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    Some(date.and_hms_opt(h, m, s)?.format(DATE_FORMAT).to_string())
}

pub(super) fn display_date(text: &str) -> String {
    if text.trim().is_empty() {
        "Not set".to_string()
    } else {
        text.to_string()
    }
}
