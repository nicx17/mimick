//! "Change Date and Time" for a selection: set one moment for all, or shift each one.

use std::rc::Rc;

use chrono::Local;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::DateChange;

use super::LibraryWindowUi;
use super::asset_edit::{DATE_FORMAT, DateField};
use super::context_menu::show_alert_dialog;
use super::header_format::{count_label, count_noun};
use super::time_zones::{self, offset_seconds, offset_text, zone_labels, zone_row, zoned_rfc3339};

const MINUTES_PER_DAY: i64 = 24 * 60;

pub(super) fn shift_minutes(days: i64, hours: i64, minutes: i64) -> i64 {
    days * MINUTES_PER_DAY + hours * 60 + minutes
}

pub(super) fn shift_summary(minutes: i64) -> String {
    if minutes == 0 {
        return "No change".to_string();
    }
    let total = minutes.unsigned_abs();
    let parts: Vec<String> = [
        (total / MINUTES_PER_DAY as u64, "day", "days"),
        (total % MINUTES_PER_DAY as u64 / 60, "hour", "hours"),
        (total % 60, "minute", "minutes"),
    ]
    .into_iter()
    .filter(|(n, _, _)| *n > 0)
    .map(|(n, one, many)| count_noun(n, one, many))
    .collect();
    let direction = if minutes > 0 { "Later" } else { "Earlier" };
    format!("{direction} by {}", parts.join(", "))
}

pub(super) fn present(
    ui: &Rc<LibraryWindowUi>,
    ids: Vec<String>,
    title: String,
    on_done: Rc<dyn Fn()>,
) {
    let form = Rc::new(DateForm::new());
    let (dialog, save) = dialog_shell(&form, &title);
    form.connect_changes(&save);
    let (ui_save, dialog_save, form_save) = (ui.clone(), dialog.clone(), form.clone());
    save.connect_clicked(move |save| {
        let Some(change) = form_save.change() else {
            return;
        };
        save.set_sensitive(false);
        apply(
            ui_save.clone(),
            ids.clone(),
            change,
            (dialog_save.clone(), save.clone()),
            on_done.clone(),
        );
    });
    dialog.present(Some(&ui.window));
}

fn apply(
    ui: Rc<LibraryWindowUi>,
    ids: Vec<String>,
    change: DateChange,
    (dialog, save): (libadwaita::Dialog, gtk::Button),
    on_done: Rc<dyn Fn()>,
) {
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.set_assets_date(&ids, &change).await {
            Ok(()) => {
                dialog.close();
                let message = format!("Changed the date of {}", count_label(ids.len() as u64));
                ui.toasts.add_toast(libadwaita::Toast::new(&message));
                on_done();
                super::refresh_library_after_mutation(ui.clone(), true);
            }
            Err(err) => {
                save.set_sensitive(true);
                show_alert_dialog(
                    &ui,
                    "Could Not Change Date",
                    &format!("Immich rejected the change: {err}"),
                );
            }
        }
    });
}

/// Both modes' widgets; `change()` reads whichever mode is showing.
struct DateForm {
    modes: libadwaita::ToggleGroup,
    hint: gtk::Label,
    stack: gtk::Stack,
    zones: Vec<String>,
    date: DateField,
    set_zone: libadwaita::ComboRow,
    preview: gtk::Label,
    days: libadwaita::SpinRow,
    hours: libadwaita::SpinRow,
    minutes: libadwaita::SpinRow,
    summary: gtk::Label,
    shift_zone: libadwaita::ComboRow,
}

impl DateForm {
    fn new() -> Self {
        let zones = time_zones::all();
        let now = Local::now().format(DATE_FORMAT).to_string();
        let labels = zone_labels(&zones, &now);
        let local = crate::api_client::local_timezone_name().unwrap_or_else(|| "UTC".into());
        let local_index = zones.iter().position(|zone| *zone == local).unwrap_or(0);
        let set_zone = zone_row(labels.iter().map(String::as_str), local_index);
        let keep = std::iter::once(KEEP_ZONE).chain(labels.iter().map(String::as_str));
        let shift_zone = zone_row(keep, 0);
        Self {
            modes: mode_toggles(),
            hint: dim_label(SET_HINT),
            stack: gtk::Stack::builder()
                .transition_type(gtk::StackTransitionType::Crossfade)
                .transition_duration(150)
                .vhomogeneous(false)
                .interpolate_size(true)
                .build(),
            zones,
            date: DateField::new(&now),
            set_zone,
            preview: dim_label(""),
            days: shift_row("Days", 36500.0),
            hours: shift_row("Hours", 23.0),
            minutes: shift_row("Minutes", 59.0),
            summary: dim_label(""),
            shift_zone,
        }
    }

    fn set_page(&self) -> gtk::Box {
        self.date.row.set_title("Date and time");
        let group = libadwaita::PreferencesGroup::new();
        group.add(&self.date.row);
        group.add(&self.set_zone);
        page(&[group.upcast_ref(), self.preview.upcast_ref()])
    }

    fn shift_page(&self) -> gtk::Box {
        let amount = libadwaita::PreferencesGroup::new();
        amount.add(&self.days);
        amount.add(&self.hours);
        amount.add(&self.minutes);
        let zone = libadwaita::PreferencesGroup::new();
        zone.add(&self.shift_zone);
        page(&[
            amount.upcast_ref(),
            self.summary.upcast_ref(),
            zone.upcast_ref(),
        ])
    }

    fn showing_set(&self) -> bool {
        self.modes.active_name().as_deref() != Some("shift")
    }

    fn set_zone_name(&self) -> Option<&str> {
        self.zones
            .get(self.set_zone.selected() as usize)
            .map(String::as_str)
    }

    fn shift_total(&self) -> i64 {
        let value = |row: &libadwaita::SpinRow| row.value() as i64;
        shift_minutes(value(&self.days), value(&self.hours), value(&self.minutes))
    }

    fn change(&self) -> Option<DateChange> {
        if self.showing_set() {
            let zone = self.set_zone_name()?;
            let date_time = zoned_rfc3339(&self.date.text.borrow(), zone)?;
            return Some(DateChange::Set {
                date_time,
                time_zone: zone.to_string(),
            });
        }
        let minutes = self.shift_total();
        let time_zone = (self.shift_zone.selected() as usize)
            .checked_sub(1)
            .and_then(|index| self.zones.get(index).cloned());
        (minutes != 0 || time_zone.is_some()).then_some(DateChange::Shift { minutes, time_zone })
    }

    fn refresh(&self, save: &gtk::Button) {
        let showing_set = self.showing_set();
        self.stack
            .set_visible_child_name(if showing_set { "set" } else { "shift" });
        self.hint
            .set_label(if showing_set { SET_HINT } else { SHIFT_HINT });
        self.preview.set_label(&self.preview_text());
        self.summary.set_label(&shift_summary(self.shift_total()));
        save.set_sensitive(self.change().is_some());
    }

    fn preview_text(&self) -> String {
        let date = self.date.text.borrow();
        match self
            .set_zone_name()
            .and_then(|zone| offset_seconds(zone, &date))
        {
            Some(offset) => format!("Saved as {date} UTC{}", offset_text(offset)),
            None => String::new(),
        }
    }

    fn connect_changes(self: &Rc<Self>, save: &gtk::Button) {
        let refresh = {
            let (form, save) = (Rc::downgrade(self), save.clone());
            move || {
                if let Some(form) = form.upgrade() {
                    form.refresh(&save);
                }
            }
        };
        let r = refresh.clone();
        self.modes.connect_active_name_notify(move |_| r());
        let r = refresh.clone();
        self.date.row.connect_subtitle_notify(move |_| r());
        for combo in [&self.set_zone, &self.shift_zone] {
            let r = refresh.clone();
            combo.connect_selected_notify(move |_| r());
        }
        for spin in [&self.days, &self.hours, &self.minutes] {
            let r = refresh.clone();
            spin.connect_value_notify(move |_| r());
        }
        refresh();
    }
}

const SET_HINT: &str = "Give every selected photo the same date and time.";
const SHIFT_HINT: &str = "Move each photo earlier or later, keeping the time between them.";
const KEEP_ZONE: &str = "Keep current zone";

fn mode_toggles() -> libadwaita::ToggleGroup {
    let group = libadwaita::ToggleGroup::builder()
        .homogeneous(true)
        .halign(gtk::Align::Center)
        .build();
    for (name, label) in [("set", "Set"), ("shift", "Shift")] {
        group.add(
            libadwaita::Toggle::builder()
                .name(name)
                .label(label)
                .build(),
        );
    }
    group.set_active_name(Some("set"));
    group
}

fn shift_row(title: &str, limit: f64) -> libadwaita::SpinRow {
    libadwaita::SpinRow::builder()
        .title(title)
        .adjustment(&gtk::Adjustment::new(0.0, -limit, limit, 1.0, 10.0, 0.0))
        .numeric(true)
        .build()
}

fn dim_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label", "caption"])
        .build()
}

fn page(children: &[&gtk::Widget]) -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    for child in children {
        page.append(*child);
    }
    page
}

/// Cancel and Save in the header, the mode switch and both pages below.
fn dialog_shell(form: &DateForm, subtitle: &str) -> (libadwaita::Dialog, gtk::Button) {
    let cancel = gtk::Button::builder().label("Cancel").build();
    let save = gtk::Button::builder()
        .label("Save")
        .css_classes(["suggested-action"])
        .build();
    let header = libadwaita::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .title_widget(&libadwaita::WindowTitle::new(
            "Change Date and Time",
            subtitle,
        ))
        .build();
    header.pack_start(&cancel);
    header.pack_end(&save);
    form.stack.add_named(&form.set_page(), Some("set"));
    form.stack.add_named(&form.shift_page(), Some("shift"));
    let content = page(&[
        form.modes.upcast_ref(),
        form.hint.upcast_ref(),
        form.stack.upcast_ref(),
    ]);
    content.set_margin_top(12);
    content.set_margin_bottom(18);
    content.set_margin_start(12);
    content.set_margin_end(12);
    let toolbar = libadwaita::ToolbarView::builder().content(&content).build();
    toolbar.add_top_bar(&header);
    let dialog = libadwaita::Dialog::builder()
        .title("Change Date and Time")
        .content_width(420)
        .child(&toolbar)
        .default_widget(&save)
        .build();
    let dialog_cancel = dialog.clone();
    cancel.connect_clicked(move |_| {
        dialog_cancel.close();
    });
    (dialog, save)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_minutes_combines_units() {
        assert_eq!(shift_minutes(1, 2, 3), 1563);
        assert_eq!(shift_minutes(-1, 0, 0), -1440);
        assert_eq!(shift_minutes(0, -1, 30), -30);
    }

    #[test]
    fn shift_summary_reads_naturally() {
        assert_eq!(shift_summary(1563), "Later by 1 day, 2 hours, 3 minutes");
        assert_eq!(shift_summary(-90), "Earlier by 1 hour, 30 minutes");
        assert_eq!(shift_summary(60), "Later by 1 hour");
        assert_eq!(shift_summary(0), "No change");
    }
}
