//! The Straighten, Adjust, and Filters controls; every change redraws the photo.

use std::rc::Rc;

use gettextrs::gettext;
use gtk::prelude::*;

use super::super::Lightbox;
use super::panel::Dial;
use crate::i18n::fill;
use crate::library::photo_editor::EditSession;
use crate::library::photo_editor::look::{FILTERS, Filter, Look, SLIDERS};

/// Marks the chip of a slider moved from zero, so changes are easy to find again.
const CHANGED: &str = "mimick-edit-changed";

impl Lightbox {
    pub(super) fn connect_look_pages(self: &Rc<Self>) {
        let w = &self.edit_mode.widgets;
        self.on_dial(&w.crop.straighten, 0.0, |lb, value| {
            lb.with_session(|s| s.set_straighten(value));
        });
        self.on_dial(&w.adjust.dial, 0.0, |lb, value| {
            let slider = lb.edit_mode.slider.get();
            lb.with_session(|s| s.look.adjust.set(slider, value));
            lb.mark_changed_sliders();
        });
        self.on_dial(&w.filters.strength, 100.0, |lb, value| {
            lb.with_session(|s| s.look.strength = (value / 100.0) as f32);
        });
        self.connect_slider_chips();
        self.connect_filter_tiles();
    }

    /// `change` gets the dial's value whenever the user moves it, and the photo redraws;
    /// its reset button puts it back to `rest`.
    fn on_dial(self: &Rc<Self>, dial: &Dial, rest: f64, change: impl Fn(&Self, f64) + 'static) {
        let lb = Rc::downgrade(self);
        dial.adjustment.connect_value_changed(move |adjustment| {
            let Some(lb) = lb.upgrade() else {
                return;
            };
            lb.sync_dial_labels();
            if !lb.edit_mode.syncing.get() {
                change(&lb, adjustment.value());
                lb.show_edit();
            }
        });
        let adjustment = dial.adjustment.clone();
        dial.reset
            .connect_clicked(move |_| adjustment.set_value(rest));
    }

    pub(super) fn with_session(&self, change: impl FnOnce(&mut EditSession)) {
        if let Some(session) = self.edit_mode.session.borrow_mut().as_mut() {
            change(session);
        }
    }

    fn connect_slider_chips(self: &Rc<Self>) {
        for (chip, slider) in self.edit_mode.widgets.adjust.chips.iter().zip(SLIDERS) {
            let lb = Rc::downgrade(self);
            chip.connect_toggled(move |chip| {
                if let Some(lb) = lb.upgrade().filter(|_| chip.is_active()) {
                    lb.edit_mode.slider.set(slider);
                    lb.sync_adjust_dial();
                }
            });
        }
    }

    fn connect_filter_tiles(self: &Rc<Self>) {
        let tiles = &self.edit_mode.widgets.filters.tiles;
        for ((tile, _), (filter, _, _)) in tiles.iter().zip(FILTERS) {
            let lb = Rc::downgrade(self);
            tile.connect_toggled(move |tile| {
                let Some(lb) = lb.upgrade() else {
                    return;
                };
                if tile.is_active() && !lb.edit_mode.syncing.get() {
                    lb.with_session(|s| s.look.filter = filter);
                    lb.show_strength(filter);
                    lb.show_edit();
                }
            });
        }
    }

    fn show_strength(&self, filter: Filter) {
        let strength = &self.edit_mode.widgets.filters.strength;
        strength.root.set_visible(filter != Filter::None);
    }

    /// Sets every control to the session's values, without that counting as edits.
    pub(super) fn sync_edit_controls(&self) {
        let Some((straighten, look)) = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .map(|s| (s.straighten(), s.look))
        else {
            return;
        };
        let w = &self.edit_mode.widgets;
        let was = self.edit_mode.syncing.replace(true);
        w.crop.straighten.adjustment.set_value(straighten);
        if let Some(index) = FILTERS.iter().position(|(f, _, _)| *f == look.filter) {
            w.filters.tiles[index].0.set_active(true);
        }
        w.filters
            .strength
            .adjustment
            .set_value(f64::from(look.strength) * 100.0);
        self.show_strength(look.filter);
        self.edit_mode.syncing.set(was);
        self.sync_adjust_dial();
        self.mark_changed_sliders();
    }

    /// Points the Adjust dial at the picked slider, at that slider's value.
    fn sync_adjust_dial(&self) {
        let slider = self.edit_mode.slider.get();
        let value = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .map_or(0.0, |s| s.look.adjust.get(slider));
        let dial = &self.edit_mode.widgets.adjust.dial;
        let was = self.edit_mode.syncing.replace(true);
        dial.adjustment
            .configure(value, slider.min(), 100.0, 1.0, 10.0, 0.0);
        dial.control
            .update_property(&[gtk::accessible::Property::Label(&slider.name())]);
        self.edit_mode.syncing.set(was);
        self.sync_dial_labels();
    }

    fn mark_changed_sliders(&self) {
        let look = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .map_or_else(Look::default, |s| s.look);
        for (chip, slider) in self.edit_mode.widgets.adjust.chips.iter().zip(SLIDERS) {
            if look.adjust.get(slider) == 0.0 {
                chip.remove_css_class(CHANGED);
            } else {
                chip.add_css_class(CHANGED);
            }
        }
    }

    fn sync_dial_labels(&self) {
        let w = &self.edit_mode.widgets;
        let degrees = w.crop.straighten.adjustment.value();
        w.crop.straighten.show_value(&fill(
            gettext("{degrees}°"),
            &[("degrees", &format!("{degrees:.1}"))],
        ));
        w.adjust
            .dial
            .show_value(&signed(w.adjust.dial.adjustment.value()));
        let percent = w.filters.strength.adjustment.value().round() as i64;
        w.filters.strength.show_value(&fill(
            gettext("{percent}%"),
            &[("percent", &percent.to_string())],
        ));
    }

    /// One small picture per filter, drawn with the sliders as they are.
    pub(super) fn refresh_filter_previews(self: &Rc<Self>) {
        let drawing = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .map(EditSession::filter_previews);
        let Some(drawing) = drawing else {
            return;
        };
        let (lb, generation) = (self.clone(), self.edit_mode.render_gen.get());
        glib::MainContext::default().spawn_local(async move {
            let textures = drawing.await;
            let mode = &lb.edit_mode;
            if !mode.is_active() || mode.render_gen.get() != generation {
                return;
            }
            for ((_, picture), texture) in mode.widgets.filters.tiles.iter().zip(textures) {
                picture.set_paintable(Some(&texture));
            }
        });
    }
}

/// A slider value with its sign, as "+25", "-10", or "0".
fn signed(value: f64) -> String {
    let value = value.round() as i64;
    if value > 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_values_show_their_sign() {
        assert_eq!(signed(24.6), "+25");
        assert_eq!(signed(-10.2), "-10");
        assert_eq!(signed(-0.3), "0");
    }
}
