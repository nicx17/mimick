//! The slideshow's settings popover: seconds per photo, transition, shuffle, and repeat.

use gettextrs::gettext;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use libadwaita::prelude::*;

use super::super::Lightbox;
use super::options::{MAX_SECONDS, MIN_SECONDS};
use super::{SettingsRows, SlideshowOptions, Transition};

impl Lightbox {
    pub(super) fn slideshow_settings(self: &Rc<Self>) -> gtk::Popover {
        let rows = SettingsRows {
            interval: self.interval_row(),
            transition: self.transition_row(),
            shuffle: self.option_switch(&gettext("Shuffle"), |o| &mut o.shuffle),
            repeat: self.option_switch(&gettext("Repeat"), |o| &mut o.repeat),
        };
        let group = libadwaita::PreferencesGroup::new();
        group.add(&rows.interval);
        group.add(&rows.transition);
        group.add(&rows.shuffle);
        group.add(&rows.repeat);
        let _ = self.slideshow.rows.set(rows);
        let content = gtk::Box::builder()
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .width_request(280)
            .build();
        content.append(&group);
        let popover = gtk::Popover::builder().child(&content).build();
        let lb = Rc::downgrade(self);
        // Rows show the running slideshow's options, which memories set without saving.
        popover.connect_show(move |_| {
            if let Some(lb) = lb.upgrade() {
                lb.sync_settings_rows();
            }
        });
        popover
    }

    fn sync_settings_rows(&self) {
        let Some(rows) = self.slideshow.rows.get() else {
            return;
        };
        let options = self.slideshow.options.get();
        rows.interval.set_value(options.interval.as_secs() as f64);
        let index = Transition::ALL
            .iter()
            .position(|t| *t == options.transition)
            .unwrap_or(0);
        rows.transition.set_selected(index as u32);
        rows.shuffle.set_active(options.shuffle);
        rows.repeat.set_active(options.repeat);
    }

    fn interval_row(self: &Rc<Self>) -> libadwaita::SpinRow {
        let row =
            libadwaita::SpinRow::with_range(f64::from(MIN_SECONDS), f64::from(MAX_SECONDS), 1.0);
        row.set_title(&gettext("Seconds per Photo"));
        let lb = Rc::downgrade(self);
        row.connect_value_notify(move |row| {
            if let Some(lb) = lb.upgrade() {
                let seconds = row.value().round() as u32;
                lb.update_options(|o| o.interval = Duration::from_secs(seconds.into()));
            }
        });
        row
    }

    fn transition_row(self: &Rc<Self>) -> libadwaita::ComboRow {
        let labels: Vec<String> = Transition::ALL.iter().map(|t| t.label()).collect();
        let model = gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
        let row = libadwaita::ComboRow::builder()
            .title(gettext("Transition"))
            .model(&model)
            .build();
        let lb = Rc::downgrade(self);
        row.connect_selected_notify(move |row| {
            if let (Some(lb), Some(&transition)) =
                (lb.upgrade(), Transition::ALL.get(row.selected() as usize))
            {
                lb.update_options(|o| o.transition = transition);
            }
        });
        row
    }

    fn option_switch(
        self: &Rc<Self>,
        title: &str,
        field: fn(&mut SlideshowOptions) -> &mut bool,
    ) -> libadwaita::SwitchRow {
        let row = libadwaita::SwitchRow::builder().title(title).build();
        let lb = Rc::downgrade(self);
        row.connect_active_notify(move |row| {
            if let Some(lb) = lb.upgrade() {
                let active = row.is_active();
                lb.update_options(|o| *field(o) = active);
            }
        });
        row
    }

    /// Applies a settings change to the running slideshow, and saves it unless a memory set it.
    fn update_options(&self, change: impl FnOnce(&mut SlideshowOptions)) {
        let s = &self.slideshow;
        let mut options = s.options.get();
        change(&mut options);
        if options == s.options.get() {
            return;
        }
        s.options.set(options);
        if options.shuffle && s.planned.get().is_none() {
            self.plan_shuffle();
        }
        if !s.persist.get() {
            return;
        }
        let mut config = self.ui.ctx.config.write();
        let data = &mut config.data;
        data.slideshow_interval_secs = options.interval.as_secs() as u32;
        data.slideshow_shuffle = options.shuffle;
        data.slideshow_repeat = options.repeat;
        data.slideshow_transition = options.transition.key().to_string();
        if !config.save() {
            log::warn!("Could not save slideshow settings");
        }
    }
}
