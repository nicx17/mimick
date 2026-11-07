//! Redact: drag over the photo to hide what's under it. A box once drawn is picked: drag it to
//! move it, its corners and edges to resize it, or pick another way to hide it. Faces and
//! recognized text can be hidden all at once.

use std::rc::Rc;

use gtk::prelude::*;

use super::super::Lightbox;
use super::super::faces::face_region;
use super::Tool;
use crate::library::photo_canvas::Region;
use crate::library::photo_editor::EditSession;
use crate::library::photo_editor::redaction::RedactStyle;

/// Boxes smaller than this many preview pixels are taken as slips.
const MIN_SIDE: f64 = 8.0;
/// Room left around recognized text, as a share of its height, so no stroke shows.
const TEXT_MARGIN: f64 = 0.2;
/// How the ways of hiding are remembered in the config, in the page's chip order.
const STYLE_KEYS: [&str; 4] = ["pixelate", "blur", "frosted", "bar"];

impl Lightbox {
    pub(super) fn connect_redact(self: &Rc<Self>) {
        let layer = &self.picture.redact;
        layer.set_can_draw(true);
        layer.set_outline_all(true);
        let lb = Rc::downgrade(self);
        layer.attach_input(
            &self.picture.scrolled,
            gtk::PropagationPhase::Capture,
            move |x, y| {
                let lb = lb.upgrade().filter(|lb| lb.editing_with(Tool::Redact))?;
                let point = lb.picture.scrolled.compute_point(
                    &lb.picture.redact.area,
                    &gtk::graphene::Point::new(x as f32, y as f32),
                )?;
                Some((f64::from(point.x()), f64::from(point.y())))
            },
        );
        let lb = Rc::downgrade(self);
        layer.connect_selection_changed(move |region| {
            if let Some(lb) = lb.upgrade() {
                lb.redaction_changed(region);
            }
        });
        let lb = Rc::downgrade(self);
        layer.connect_marker_tapped(move |index| {
            if let Some(lb) = lb.upgrade() {
                lb.pick_redaction(Some(index));
            }
        });
        self.connect_redact_page();
    }

    fn connect_redact_page(self: &Rc<Self>) {
        self.restore_redact_style();
        let page = &self.edit_mode.widgets.redact;
        self.on_click(&page.hide_faces, Self::hide_faces);
        self.on_click(&page.hide_text, Self::hide_text);
        self.on_click(&page.remove, Self::remove_picked_redaction);
        self.on_click(&page.clear, |lb| {
            lb.with_session(EditSession::clear_redactions);
            lb.pick_redaction(None);
            lb.show_edit();
        });
        for chip in &page.styles {
            let lb = Rc::downgrade(self);
            chip.connect_toggled(move |chip| {
                if let Some(lb) = lb.upgrade().filter(|_| chip.is_active()) {
                    lb.restyle_picked();
                }
            });
        }
        let lb = Rc::downgrade(self);
        page.colour.connect_rgba_notify(move |_| {
            if let Some(lb) = lb.upgrade() {
                lb.restyle_picked();
            }
        });
    }

    /// A new box hides a new area, picked; a moved or resized one follows; none unpicks.
    fn redaction_changed(self: &Rc<Self>, region: Option<Region>) {
        let picked = self.edit_mode.redact_picked.get();
        match region {
            Some(r) if self.picture.redact.drew() => {
                if r.w < MIN_SIDE || r.h < MIN_SIDE {
                    self.pick_redaction(None);
                    return;
                }
                let style = self.redact_style();
                let added = self
                    .edit_mode
                    .session
                    .borrow_mut()
                    .as_mut()
                    .map(|s| s.add_redaction(r, style));
                self.edit_mode.redact_picked.set(added);
            }
            Some(r) => {
                if let Some(index) = picked {
                    self.with_session(|s| s.move_redaction(index, r));
                }
            }
            None => self.edit_mode.redact_picked.set(None),
        }
        self.show_edit();
    }

    /// Picks a hidden area to change, and shows how it's hidden.
    fn pick_redaction(&self, index: Option<usize>) {
        self.edit_mode.redact_picked.set(index);
        let style = index.and_then(|i| {
            self.edit_mode
                .session
                .borrow()
                .as_ref()
                .and_then(|s| s.redaction_style(i))
        });
        if let Some(style) = style {
            self.show_redact_style(style);
        }
        self.show_redactions();
    }

    /// The way of hiding the page shows: the picked chip, with the bar's colour.
    fn redact_style(&self) -> RedactStyle {
        let page = &self.edit_mode.widgets.redact;
        let picked = page.styles.iter().position(gtk::ToggleButton::is_active);
        match picked {
            Some(1) => RedactStyle::Blur,
            Some(2) => RedactStyle::Frosted,
            Some(3) => RedactStyle::Bar(rgb(&page.colour.rgba())),
            _ => RedactStyle::Pixelate,
        }
    }

    fn show_redact_style(&self, style: RedactStyle) {
        let page = &self.edit_mode.widgets.redact;
        let was = self.edit_mode.syncing.replace(true);
        let index = match style {
            RedactStyle::Pixelate => 0,
            RedactStyle::Blur => 1,
            RedactStyle::Frosted => 2,
            RedactStyle::Bar([r, g, b]) => {
                let rgba = gtk::gdk::RGBA::new(
                    f32::from(r) / 255.0,
                    f32::from(g) / 255.0,
                    f32::from(b) / 255.0,
                    1.0,
                );
                page.colour.set_rgba(&rgba);
                3
            }
        };
        page.styles[index].set_active(true);
        self.edit_mode.syncing.set(was);
    }

    /// A new way of hiding applies to the picked area; new boxes use it from now on, in
    /// this edit and the next.
    fn restyle_picked(self: &Rc<Self>) {
        self.sync_redact_page();
        if self.edit_mode.syncing.get() {
            return;
        }
        self.remember_redact_style();
        if let Some(index) = self.edit_mode.redact_picked.get() {
            let style = self.redact_style();
            self.with_session(|s| s.set_redaction_style(index, style));
            self.show_edit();
        }
    }

    /// The way of hiding and bar colour last picked, from the config.
    fn restore_redact_style(&self) {
        let page = &self.edit_mode.widgets.redact;
        let config = self.ui.ctx.config.read();
        let index = STYLE_KEYS
            .iter()
            .position(|key| *key == config.data.edit_redact_style)
            .unwrap_or(0);
        if let Ok(colour) = gtk::gdk::RGBA::parse(&config.data.edit_bar_colour) {
            page.colour.set_rgba(&colour);
        }
        page.styles[index].set_active(true);
    }

    fn remember_redact_style(&self) {
        let page = &self.edit_mode.widgets.redact;
        let index = page
            .styles
            .iter()
            .position(gtk::ToggleButton::is_active)
            .unwrap_or(0);
        let [r, g, b] = rgb(&page.colour.rgba());
        let mut config = self.ui.ctx.config.write();
        config.data.edit_redact_style = STYLE_KEYS[index].to_string();
        config.data.edit_bar_colour = format!("#{r:02x}{g:02x}{b:02x}");
        if !config.save() {
            log::error!("Failed to save config after changing how areas are hidden");
        }
    }

    pub(super) fn remove_picked_redaction(self: &Rc<Self>) {
        if let Some(index) = self.edit_mode.redact_picked.take() {
            self.with_session(|s| s.remove_redaction(index));
            self.show_edit();
        }
    }

    /// Outlines the hidden areas on the result, with the picked one ready to move or resize.
    pub(super) fn show_redactions(&self) {
        let shown = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .map(EditSession::shown_redactions)
            .unwrap_or_default();
        let picked = self
            .edit_mode
            .redact_picked
            .get()
            .filter(|i| *i < shown.len());
        self.edit_mode.redact_picked.set(picked);
        self.picture.redact.set_selection(picked.map(|i| shown[i]));
        self.picture.redact.set_markers(shown);
        self.sync_redact_page();
    }

    pub(super) fn sync_redact_page(&self) {
        let page = &self.edit_mode.widgets.redact;
        let any = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .is_some_and(EditSession::has_redactions);
        page.hint.set_visible(!any);
        page.clear.set_sensitive(any);
        page.remove
            .set_visible(self.edit_mode.redact_picked.get().is_some());
        page.colour.set_visible(page.styles[3].is_active());
        page.hide_faces.set_visible(self.can_hide(self.has_faces()));
        page.hide_text
            .set_visible(self.can_hide(self.picture.ocr.has_boxes()));
    }

    /// Tagged faces and recognized text line up with the unedited preview only while Immich
    /// has no edits stored.
    fn can_hide(&self, found: bool) -> bool {
        found
            && self
                .edit_mode
                .session
                .borrow()
                .as_ref()
                .is_some_and(|s| !s.was_edited())
    }

    /// Faces are loaded for the photo being edited.
    fn has_faces(&self) -> bool {
        let session = self.edit_mode.session.borrow();
        session
            .as_ref()
            .is_some_and(|s| *self.faces.asset_id.borrow() == s.asset_id)
            && !self.faces.list.borrow().is_empty()
    }

    fn hide_faces(self: &Rc<Self>) {
        let Some((w, h)) = self.preview_size() else {
            return;
        };
        let areas: Vec<Region> = self
            .faces
            .list
            .borrow()
            .iter()
            .map(|face| face_region(face, (w, h)))
            .collect();
        self.hide_found(&areas);
    }

    fn hide_text(self: &Rc<Self>) {
        let Some((w, h)) = self.preview_size() else {
            return;
        };
        let areas: Vec<Region> = self
            .picture
            .ocr
            .text_bounds()
            .into_iter()
            .map(|(x, y, bw, bh)| {
                let margin = bh * h * TEXT_MARGIN;
                Region {
                    x: x * w - margin,
                    y: y * h - margin,
                    w: bw * w + 2.0 * margin,
                    h: bh * h + 2.0 * margin,
                }
            })
            .collect();
        self.hide_found(&areas);
    }

    fn hide_found(self: &Rc<Self>, areas: &[Region]) {
        let style = self.redact_style();
        self.with_session(|s| s.add_original_redactions(areas, style));
        self.pick_redaction(None);
        self.show_edit();
    }

    fn preview_size(&self) -> Option<(f64, f64)> {
        self.edit_mode
            .session
            .borrow()
            .as_ref()
            .map(EditSession::preview_size)
    }
}

/// A colour as three bytes.
fn rgb(colour: &gtk::gdk::RGBA) -> [u8; 3] {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [
        byte(colour.red()),
        byte(colour.green()),
        byte(colour.blue()),
    ]
}
