//! Explore's Calendar: a card with a square for each day of a year, darker the more photos
//! it has. Pick a day by tapping, clicking, or with the arrow keys, then Show Photos opens it.

use crate::i18n::{fill, format_day, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use chrono::{Duration, NaiveDate, Utc};
use gtk::prelude::*;

use crate::api_client::MetadataSearchFilters;
use crate::library::LibraryWindowUi;
use crate::library::state::LibrarySource;

mod grid;
mod layout;

use grid::HeatmapGrid;

/// 52 weeks, as Immich's own calendar shows.
const SPAN_DAYS: i64 = 52 * 7 - 1;
/// The key under the grid has one square per level.
const LEGEND_LEVELS: usize = 5;

/// "Nov 2025 – Nov 2026"
fn period_label(from: NaiveDate, to: NaiveDate) -> String {
    let month = gettext("%b %Y");
    format!("{} – {}", format_day(from, &month), format_day(to, &month))
}

/// "3 Nov 2026 · 12 photos"
fn day_label(day: NaiveDate, count: u32) -> String {
    format!(
        "{} · {}",
        format_day(day, &gettext("%-d %b %Y")),
        photos_label(count)
    )
}

fn photos_label(count: u32) -> String {
    fill(
        ngettext("{n} photo", "{n} photos", plural_n(count)),
        &[("n", &count.to_string())],
    )
}

pub struct Calendar {
    pub root: gtk::Box,
    grid: HeatmapGrid,
    scroller: gtk::ScrolledWindow,
    total: gtk::Label,
    period: gtk::Label,
    prev: gtk::Button,
    next: gtk::Button,
    taken: gtk::ToggleButton,
    picked: gtk::Label,
    show: gtk::Button,
    /// The last day shown; the year before it is on screen.
    end: Cell<NaiveDate>,
    ui: RefCell<Weak<LibraryWindowUi>>,
}

impl Calendar {
    pub fn new() -> Rc<Self> {
        let grid = HeatmapGrid::new();
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&grid)
            .build();
        let calendar = Rc::new(Self {
            root: gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(8)
                .visible(false)
                .build(),
            grid,
            scroller,
            total: label(&["heading"]),
            period: label(&["dim-label", "caption"]),
            prev: arrow("go-previous-symbolic", &gettext("Earlier year")),
            next: arrow("go-next-symbolic", &gettext("Later year")),
            taken: gtk::ToggleButton::builder()
                .label(gettext("Taken"))
                .tooltip_text(gettext("By when the photos were taken"))
                .active(true)
                .build(),
            picked: label(&["dim-label"]),
            show: gtk::Button::builder()
                .label(gettext("Show Photos"))
                .css_classes(["pill", "suggested-action"])
                .valign(gtk::Align::Center)
                .visible(false)
                .build(),
            end: Cell::new(Utc::now().date_naive()),
            ui: RefCell::default(),
        });
        calendar.lay_out();
        calendar.connect();
        calendar.show_picked(None);
        calendar
    }

    /// The title, then a card: summary and controls, the grid, and the picked day with the key.
    fn lay_out(&self) {
        let title = gtk::Label::builder()
            .label(gettext("Calendar"))
            .xalign(0.0)
            .css_classes(["title-2"])
            .build();
        self.root.append(&title);
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .css_classes(["card", "mimick-calendar-card"])
            .build();
        card.append(&self.header());
        card.append(&self.scroller);
        card.append(&self.footer());
        self.root.append(&card);
    }

    /// The year's total and dates, with the year arrows and Taken or Uploaded beside them;
    /// on a phone the controls wrap below.
    fn header(&self) -> libadwaita::WrapBox {
        let summary = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .build();
        summary.append(&self.total);
        summary.append(&self.period);
        let uploaded = gtk::ToggleButton::builder()
            .label(gettext("Uploaded"))
            .tooltip_text(gettext("By when the photos reached Immich"))
            .group(&self.taken)
            .build();
        let controls = gtk::Box::builder()
            .spacing(6)
            .valign(gtk::Align::Center)
            .build();
        controls.append(&linked(&[self.prev.upcast_ref(), self.next.upcast_ref()]));
        controls.append(&linked(&[self.taken.upcast_ref(), uploaded.upcast_ref()]));
        spread(&[summary.upcast_ref(), controls.upcast_ref()])
    }

    /// The picked day and Show Photos, and the key from fewer photos to more.
    fn footer(&self) -> libadwaita::WrapBox {
        let picked = gtk::Box::builder()
            .spacing(12)
            .valign(gtk::Align::Center)
            .build();
        picked.append(&self.picked);
        picked.append(&self.show);
        spread(&[picked.upcast_ref(), legend().upcast_ref()])
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.taken.connect_toggled(move |_| {
            if let Some(calendar) = weak.upgrade() {
                calendar.reload();
            }
        });
        for (button, years) in [(&self.prev, -1), (&self.next, 1)] {
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(calendar) = weak.upgrade() {
                    calendar.step_year(years);
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.show.connect_clicked(move |_| {
            if let Some(calendar) = weak.upgrade() {
                calendar.open_selected();
            }
        });
        let weak = Rc::downgrade(self);
        self.grid.connect_selected(move |day| {
            if let Some(calendar) = weak.upgrade() {
                calendar.show_picked(day);
            }
        });
        let weak = Rc::downgrade(self);
        self.grid.connect_activated(move || {
            if let Some(calendar) = weak.upgrade() {
                calendar.open_selected();
            }
        });
    }

    fn show_picked(&self, day: Option<(NaiveDate, u32)>) {
        match day {
            Some((day, count)) => {
                self.picked.set_label(&day_label(day, count));
                self.show.set_visible(count > 0);
            }
            None => {
                self.picked
                    .set_label(&gettext("Pick a day to see its photos."));
                self.show.set_visible(false);
            }
        }
    }

    fn step_year(self: &Rc<Self>, years: i64) {
        let today = Utc::now().date_naive();
        let end = (self.end.get() + Duration::days(years * (SPAN_DAYS + 1))).min(today);
        self.end.set(end);
        self.reload();
    }

    fn reload(self: &Rc<Self>) {
        // Taken out first: `load` stores the window again, which needs the cell free.
        let ui = self.ui.borrow().upgrade();
        if let Some(ui) = ui {
            load(self, &ui);
        }
    }

    /// The day's photos, by when they were taken or uploaded, as a search.
    fn open_selected(&self) {
        let ui = self.ui.borrow().upgrade();
        let (Some(ui), Some((day, count))) = (ui, self.grid.selected_day()) else {
            return;
        };
        if count == 0 {
            return;
        }
        let (start, end) = (
            format!("{day}T00:00:00.000Z"),
            format!("{day}T23:59:59.999Z"),
        );
        let mut filters = MetadataSearchFilters::default();
        if self.taken.is_active() {
            (filters.taken_after, filters.taken_before) = (Some(start), Some(end));
        } else {
            (filters.created_after, filters.created_before) = (Some(start), Some(end));
        }
        crate::library::navigation::navigate_source(
            &ui,
            LibrarySource::AdvancedSearch {
                filters: Box::new(filters),
            },
        );
    }

    fn show_days(&self, list: Vec<(NaiveDate, u32)>) {
        let total: u32 = list.iter().map(|(_, count)| *count).sum();
        self.total.set_label(&photos_label(total));
        self.grid.set_days(list);
        self.show_picked(None);
        self.root.set_visible(true);
        // The newest days are on the right, so a narrow window starts there.
        let adjustment = self.scroller.hadjustment();
        glib::idle_add_local_once(move || adjustment.set_value(adjustment.upper()));
    }
}

/// Fetch the year ending at the calendar's end day; the section hides if Immich can't say.
pub(in crate::library) fn load(calendar: &Rc<Calendar>, ui: &Rc<LibraryWindowUi>) {
    calendar.ui.replace(Rc::downgrade(ui));
    let end = calendar.end.get();
    let start = end - Duration::days(SPAN_DAYS);
    calendar.period.set_label(&period_label(start, end));
    calendar.next.set_sensitive(end < Utc::now().date_naive());
    let (calendar, ui, taken) = (calendar.clone(), ui.clone(), calendar.taken.is_active());
    glib::MainContext::default().spawn_local(async move {
        let api = &ui.ctx.api_client;
        match api.fetch_calendar_heatmap(start, end, taken).await {
            Ok(list) => calendar.show_days(list),
            Err(err) => log::debug!("Photo calendar unavailable: {err}"),
        }
    });
}

fn label(classes: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .css_classes(classes)
        .build()
}

fn arrow(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build()
}

fn linked(children: &[&gtk::Widget]) -> gtk::Box {
    let group = gtk::Box::builder().css_classes(["linked"]).build();
    for child in children {
        group.append(*child);
    }
    group
}

/// Two groups at either end of a wide row, one under the other when there isn't room.
fn spread(children: &[&gtk::Widget]) -> libadwaita::WrapBox {
    let row = libadwaita::WrapBox::builder()
        .child_spacing(12)
        .line_spacing(8)
        .justify(libadwaita::JustifyMode::Spread)
        .build();
    for child in children {
        row.append(*child);
    }
    row
}

/// "Less", a square for each level, "More".
fn legend() -> gtk::Box {
    let key = gtk::Box::builder()
        .spacing(3)
        .valign(gtk::Align::Center)
        .build();
    key.append(&label(&["dim-label", "caption"]));
    for level in 0..LEGEND_LEVELS {
        let square = gtk::Box::builder()
            .valign(gtk::Align::Center)
            .css_classes(["mimick-heat-cell", &format!("mimick-heat-{level}")])
            .build();
        key.append(&square);
    }
    key.append(&label(&["dim-label", "caption"]));
    if let (Some(less), Some(more)) = (
        key.first_child().and_downcast::<gtk::Label>(),
        key.last_child().and_downcast::<gtk::Label>(),
    ) {
        less.set_label(&gettext("Less"));
        more.set_label(&gettext("More"));
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn labels_read_naturally() {
        assert_eq!(day_label(day(2026, 11, 3), 1), "3 Nov 2026 · 1 photo");
        assert_eq!(
            period_label(day(2025, 11, 9), day(2026, 11, 8)),
            "Nov 2025 – Nov 2026"
        );
        assert_eq!(photos_label(1234), "1234 photos");
    }
}
