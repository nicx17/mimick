//! Each utility's filters in the header: Large Files' kind and size, Missing Location's
//! days, and which Unusual Dates to list. A change shows the same view refiltered.

use gettextrs::gettext;
use std::rc::Rc;

use chrono::NaiveDate;
use gtk::prelude::*;
use libadwaita::prelude::*;

use super::filters::MIN_SIZES_MB;
use crate::i18n::{fill, format_day};
use crate::library::LibraryWindowUi;
use crate::library::day_row::{DAY_FORMAT, DayRow};
use crate::library::navigation;
use crate::library::state::{
    DayRange, EARLY_YEAR, LargeFilesFilter, LibrarySource, Location, MediaKind, OddDates,
};

const KINDS: [MediaKind; 3] = [MediaKind::All, MediaKind::Photos, MediaKind::Videos];
const MB: u64 = 1_000_000;

fn refilter(ui: &Rc<LibraryWindowUi>, source: LibrarySource) {
    navigation::refine(ui, Location::Source(source));
}

/// Large Files: which kind of file, and the smallest size worth listing.
pub(super) fn large_files(ui: &Rc<LibraryWindowUi>, filter: LargeFilesFilter) -> Vec<gtk::Widget> {
    let kind_labels = [gettext("All Files"), gettext("Photos"), gettext("Videos")];
    let kind_at = KINDS.iter().position(|kind| *kind == filter.kind);
    let kinds = dropdown(&kind_labels, kind_at, &gettext("Kind of file"));
    let size_labels: Vec<String> = MIN_SIZES_MB.iter().map(|mb| size_label(*mb)).collect();
    let size_at = MIN_SIZES_MB
        .iter()
        .position(|mb| mb * MB == filter.min_bytes);
    let sizes = dropdown(&size_labels, size_at, &gettext("Smallest size"));
    let ui_kind = ui.clone();
    kinds.connect_selected_notify(move |kinds| {
        let kind = KINDS.get(kinds.selected() as usize).copied();
        let filter = LargeFilesFilter {
            kind: kind.unwrap_or_default(),
            ..filter
        };
        refilter(&ui_kind, LibrarySource::LargeFiles { filter });
    });
    let ui = ui.clone();
    sizes.connect_selected_notify(move |sizes| {
        let mb = MIN_SIZES_MB.get(sizes.selected() as usize).copied();
        let filter = LargeFilesFilter {
            min_bytes: mb.unwrap_or(0) * MB,
            ..filter
        };
        refilter(&ui, LibrarySource::LargeFiles { filter });
    });
    vec![kinds.upcast(), sizes.upcast()]
}

fn dropdown(labels: &[String], selected: Option<usize>, name: &str) -> gtk::DropDown {
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let dropdown = gtk::DropDown::builder()
        .model(&gtk::StringList::new(&labels))
        .selected(selected.unwrap_or(0) as u32)
        .tooltip_text(name)
        .valign(gtk::Align::Center)
        .build();
    dropdown.update_property(&[gtk::accessible::Property::Label(name)]);
    dropdown
}

fn size_label(mb: u64) -> String {
    match mb {
        0 => gettext("Any Size"),
        mb if mb >= 1000 => fill(gettext("Over {n} GB"), &[("n", &(mb / 1000).to_string())]),
        mb => fill(gettext("Over {n} MB"), &[("n", &mb.to_string())]),
    }
}

/// Missing Location: limit the list to photos taken between two days.
pub(super) fn dates_button(ui: &Rc<LibraryWindowUi>, range: &DayRange) -> gtk::Widget {
    let button = gtk::Button::builder()
        .child(
            &libadwaita::ButtonContent::builder()
                .icon_name("x-office-calendar-symbolic")
                .label(gettext("Dates"))
                .build(),
        )
        .tooltip_text(gettext("Show only photos taken between two days"))
        .valign(gtk::Align::Center)
        .build();
    let (ui, range) = (ui.clone(), range.clone());
    button.connect_clicked(move |_| present_dates(&ui, &range));
    button.upcast()
}

fn present_dates(ui: &Rc<LibraryWindowUi>, range: &DayRange) {
    let from = DayRow::new(&gettext("From"), range.from.as_deref());
    let until = DayRow::new(&gettext("Until"), range.until.as_deref());
    let group = libadwaita::PreferencesGroup::builder()
        .description(gettext(
            "Show the photos without a place taken between these days.",
        ))
        .build();
    group.add(&from.row);
    group.add(&until.row);
    let page = libadwaita::PreferencesPage::new();
    page.add(&group);
    let show = gtk::Button::builder()
        .label(gettext("Show"))
        .css_classes(["suggested-action"])
        .build();
    let header = libadwaita::HeaderBar::new();
    header.pack_end(&show);
    let toolbar = libadwaita::ToolbarView::builder().content(&page).build();
    toolbar.add_top_bar(&header);
    let dialog = libadwaita::Dialog::builder()
        .title(gettext("Dates"))
        .content_width(360)
        .follows_content_size(true)
        .child(&toolbar)
        .build();
    let (ui_show, dialog_ref) = (ui.clone(), dialog.clone());
    show.connect_clicked(move |_| {
        let range = DayRange {
            from: from.day.borrow().clone(),
            until: until.day.borrow().clone(),
        };
        dialog_ref.close();
        refilter(&ui_show, LibrarySource::MissingLocation { range });
    });
    dialog.present(Some(&ui.window));
}

/// "From 3 Jun 2024 to 30 Jun 2024" for the header; `None` with no days picked.
pub(super) fn range_text(range: &DayRange) -> Option<String> {
    let day = |day: &Option<String>| {
        let date = NaiveDate::parse_from_str(day.as_deref()?, DAY_FORMAT).ok()?;
        Some(format_day(date, &gettext("%-d %b %Y")))
    };
    match (day(&range.from), day(&range.until)) {
        (Some(from), Some(until)) => Some(fill(
            gettext("From {from} to {until}"),
            &[("from", &from), ("until", &until)],
        )),
        (Some(from), None) => Some(fill(gettext("From {from}"), &[("from", &from)])),
        (None, Some(until)) => Some(fill(gettext("Until {until}"), &[("until", &until)])),
        (None, None) => None,
    }
}

/// Unusual Dates: photos dated in the future, or too early to be right.
pub(super) fn odd_dates_toggle(ui: &Rc<LibraryWindowUi>, which: OddDates) -> gtk::Widget {
    let group = libadwaita::ToggleGroup::builder()
        .valign(gtk::Align::Center)
        .build();
    let early = fill(
        gettext("Before {year}"),
        &[("year", &EARLY_YEAR.to_string())],
    );
    for (name, label) in [("future", gettext("Future")), ("early", early)] {
        group.add(
            libadwaita::Toggle::builder()
                .name(name)
                .label(&label)
                .build(),
        );
    }
    let active = match which {
        OddDates::Future => "future",
        OddDates::Early => "early",
    };
    group.set_active_name(Some(active));
    let ui = ui.clone();
    group.connect_active_name_notify(move |group| {
        let which = match group.active_name().as_deref() {
            Some("early") => OddDates::Early,
            _ => OddDates::Future,
        };
        refilter(&ui, LibrarySource::UnusualDates { which });
    });
    group.upcast()
}

/// What the header says Unusual Dates is listing.
pub(super) fn odd_dates_text(which: OddDates) -> String {
    match which {
        OddDates::Future => gettext("Dated after today"),
        OddDates::Early => fill(
            gettext("Dated before {year}"),
            &[("year", &EARLY_YEAR.to_string())],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_in_whole_units() {
        assert_eq!(size_label(0), "Any Size");
        assert_eq!(size_label(50), "Over 50 MB");
        assert_eq!(size_label(1000), "Over 1 GB");
    }

    #[test]
    fn a_range_names_only_the_days_picked() {
        assert_eq!(range_text(&DayRange::default()), None);
        let until = DayRange {
            from: None,
            until: Some("2024-06-30".into()),
        };
        assert!(range_text(&until).is_some_and(|text| text.starts_with("Until")));
    }
}
