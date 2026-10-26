use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::library::LibraryWindowUi;
use crate::library::context_menu::show_alert_dialog;

const STAR_COUNT: u8 = 5;

#[derive(Clone)]
pub(super) struct RatingSection {
    pub root: libadwaita::PreferencesGroup,
    stars: Rc<[gtk::Button]>,
    clear_btn: gtk::Button,
    asset_id: Rc<RefCell<String>>,
    rating: Rc<Cell<Option<u8>>>,
    editable: Rc<Cell<bool>>,
    ui: Rc<RefCell<Option<Rc<LibraryWindowUi>>>>,
}

impl RatingSection {
    pub(super) fn new() -> Self {
        let clear_btn = gtk::Button::builder()
            .label("Clear")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let root = libadwaita::PreferencesGroup::builder()
            .title("Rating")
            .header_suffix(&clear_btn)
            .visible(false)
            .build();
        let row = gtk::Box::builder().halign(gtk::Align::Start).build();
        let stars: Rc<[gtk::Button]> = (1..=STAR_COUNT).map(star_button).collect();
        for star in stars.iter() {
            row.append(star);
        }
        root.add(&row);
        let section = Self {
            root,
            stars,
            clear_btn,
            asset_id: Rc::default(),
            rating: Rc::default(),
            editable: Rc::default(),
            ui: Rc::default(),
        };
        section.connect();
        section
    }

    pub(super) fn bind(&self, ui: Rc<LibraryWindowUi>) {
        *self.ui.borrow_mut() = Some(ui);
    }

    pub(super) fn clear(&self) {
        self.root.set_visible(false);
    }

    pub(super) fn show(&self, asset_id: &str, rating: Option<u8>, editable: bool) {
        *self.asset_id.borrow_mut() = asset_id.to_string();
        self.rating.set(rating);
        self.editable.set(editable);
        for star in self.stars.iter() {
            star.set_can_target(editable);
            star.set_focusable(editable);
        }
        self.refresh();
        self.root.set_visible(editable || rating.is_some());
    }

    fn connect(&self) {
        for (star, value) in self.stars.iter().zip(1..) {
            let section = self.clone();
            star.connect_clicked(move |star| section.choose(star, value));
            let hover = gtk::EventControllerMotion::new();
            let section = self.clone();
            hover.connect_enter(move |_, _, _| section.paint(Some(value)));
            let section = self.clone();
            hover.connect_leave(move |_| section.paint(section.rating.get()));
            star.add_controller(hover);
        }
        let section = self.clone();
        self.clear_btn.connect_clicked(move |_| section.save(None));
    }

    fn choose(&self, star: &gtk::Button, value: u8) {
        if self.rating.get() == Some(value) {
            return;
        }
        pop(star);
        self.save(Some(value));
    }

    fn save(&self, rating: Option<u8>) {
        let Some(ui) = self.ui.borrow().clone() else {
            return;
        };
        let previous = self.rating.replace(rating);
        self.refresh();
        let section = self.clone();
        let asset_id = self.asset_id.borrow().clone();
        glib::MainContext::default().spawn_local(async move {
            let Err(err) = ui.ctx.api_client.set_asset_rating(&asset_id, rating).await else {
                return;
            };
            if *section.asset_id.borrow() == asset_id {
                section.rating.set(previous);
                section.refresh();
            }
            show_alert_dialog(&ui, "Could Not Save Rating", &err);
        });
    }

    fn refresh(&self) {
        self.paint(self.rating.get());
        self.clear_btn
            .set_visible(self.editable.get() && self.rating.get().is_some());
    }

    fn paint(&self, rating: Option<u8>) {
        let filled = rating.unwrap_or(0);
        for (star, value) in self.stars.iter().zip(1..) {
            star.set_icon_name(if value <= filled {
                "starred-symbolic"
            } else {
                "non-starred-symbolic"
            });
        }
    }
}

pub(super) fn stars_from_exif(rating: Option<i8>) -> Option<u8> {
    rating
        .and_then(|r| u8::try_from(r).ok())
        .filter(|r| (1..=STAR_COUNT).contains(r))
}

fn star_button(value: u8) -> gtk::Button {
    let tooltip = match value {
        1 => "1 star".to_string(),
        n => format!("{n} stars"),
    };
    gtk::Button::builder()
        .icon_name("non-starred-symbolic")
        .tooltip_text(tooltip)
        .css_classes(["flat", "accent"])
        .build()
}

fn pop(star: &gtk::Button) {
    star.add_css_class("mimick-pop");
    let star = star.downgrade();
    glib::timeout_add_local_once(Duration::from_millis(300), move || {
        if let Some(star) = star.upgrade() {
            star.remove_css_class("mimick-pop");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_to_five_stars_count_as_rated() {
        assert_eq!(stars_from_exif(Some(4)), Some(4));
        assert_eq!(stars_from_exif(Some(5)), Some(5));
        assert_eq!(stars_from_exif(None), None);
        assert_eq!(stars_from_exif(Some(0)), None);
        assert_eq!(stars_from_exif(Some(-1)), None);
        assert_eq!(stars_from_exif(Some(6)), None);
    }
}
