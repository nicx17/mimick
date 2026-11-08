//! Zooming and panning the lightbox photo: buttons, pinch, drag, clicks, and Ctrl+scroll.

use std::rc::Rc;

use gtk::prelude::*;

use super::super::{Lightbox, apply_lightbox_zoom, fit_size, max_zoom, zoom_label};

impl Lightbox {
    /// Zoom buttons, pinch, drag-to-pan, double/middle click, and Ctrl+scroll.
    pub(in crate::library::lightbox) fn connect_zoom_controls(self: &Rc<Self>) {
        self.on_click(&self.actions.zoom_in, |lb| lb.zoom_by(1.2));
        self.on_click(&self.actions.zoom_out, |lb| lb.zoom_by(1.0 / 1.2));
        self.on_click(&self.actions.zoom_reset, Self::zoom_reset);
        self.connect_pan_and_pinch();
        self.connect_zoom_clicks();

        // Ctrl+wheel zoom on the picture area, captured before the scrolled window
        // can use it for panning. Listening on both axes so trackpad two-finger
        // scrolls (which sometimes emit horizontal deltas) still trigger zoom.
        let zoom_scroll =
            gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        zoom_scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        let lb = Rc::downgrade(self);
        zoom_scroll.connect_scroll(move |ctrl, dx, dy| match lb.upgrade() {
            Some(lb) => lb.handle_zoom_scroll(ctrl, dx, dy),
            None => glib::Propagation::Proceed,
        });
        self.picture.scrolled.add_controller(zoom_scroll);

        // Resizing the viewer changes the fitted size, and so the real-pixel percentage.
        let scrolled = &self.picture.scrolled;
        for adjustment in [scrolled.hadjustment(), scrolled.vadjustment()] {
            let lb = Rc::downgrade(self);
            adjustment.connect_page_size_notify(move |_| {
                if let Some(lb) = lb.upgrade() {
                    lb.show_zoom_label();
                }
            });
        }
    }

    /// Double-click or double-tap zooms 2x toward the point, or back out; middle-click resets.
    fn connect_zoom_clicks(self: &Rc<Self>) {
        let double_click = gtk::GestureClick::new();
        double_click.set_button(gtk::gdk::BUTTON_PRIMARY);
        let lb = Rc::downgrade(self);
        double_click.connect_pressed(move |_, n_press, x, y| {
            if let Some(lb) = lb.upgrade()
                && n_press == 2
                && lb.ocr_box_under(x, y).is_none()
            {
                lb.toggle_double_click_zoom(x, y);
            }
        });
        self.picture.scrolled.add_controller(double_click);

        let middle_click = gtk::GestureClick::new();
        middle_click.set_button(gtk::gdk::BUTTON_MIDDLE);
        let lb = Rc::downgrade(self);
        middle_click.connect_pressed(move |_, _, _, _| {
            if let Some(lb) = lb.upgrade() {
                lb.zoom_reset();
            }
        });
        self.picture.scrolled.add_controller(middle_click);
    }

    fn toggle_double_click_zoom(self: &Rc<Self>, x: f64, y: f64) {
        if self.zoom_level.get() > 1.0 {
            self.zoom_reset();
        } else {
            self.cursor_pos.set(Some((x, y)));
            self.set_zoom(2.0);
        }
    }

    /// Pinch-to-zoom and click-and-drag panning, grouped so they don't fight over touches.
    fn connect_pan_and_pinch(self: &Rc<Self>) {
        // Trackpad pinch-to-zoom. On the picture scroller so it shares a stable
        // coordinate frame with drag (see drag comment below).
        let pinch = gtk::GestureZoom::new();
        let lb = Rc::downgrade(self);
        pinch.connect_begin(move |pinch, _| {
            // Two fingers only zoom (and pan, which is grouped with it): claiming them cancels
            // the sideways photo swipe, the swipe up, and the long-press on those touches.
            pinch.set_state(gtk::EventSequenceState::Claimed);
            if let Some(lb) = lb.upgrade() {
                lb.pinch_start.set(lb.zoom_level.get());
            }
        });
        let lb = Rc::downgrade(self);
        pinch.connect_scale_changed(move |_, scale| {
            if let Some(lb) = lb.upgrade() {
                lb.set_zoom(lb.pinch_start.get() * scale);
            }
        });
        self.picture.scrolled.add_controller(pinch.clone());

        // Click-and-drag panning when zoomed in. Attached to the scroller, not the
        // picture stack: the stack moves under the cursor when we update the scroll
        // adjustments, so an offset measured relative to it oscillates frame-to-frame
        // and jitters the image.
        let drag = gtk::GestureDrag::new();
        drag.set_button(gtk::gdk::BUTTON_PRIMARY);
        let lb = Rc::downgrade(self);
        drag.connect_drag_begin(move |_, _, _| {
            let Some(lb) = lb.upgrade() else {
                return;
            };
            let hadj = lb.picture.scrolled.hadjustment();
            let vadj = lb.picture.scrolled.vadjustment();
            lb.drag_start.set((hadj.value(), vadj.value()));
        });
        let lb = Rc::downgrade(self);
        drag.connect_drag_update(move |_, off_x, off_y| {
            let Some(lb) = lb.upgrade() else {
                return;
            };
            let (sx0, sy0) = lb.drag_start.get();
            lb.picture.scrolled.hadjustment().set_value(sx0 - off_x);
            lb.picture.scrolled.vadjustment().set_value(sy0 - off_y);
        });
        self.picture.scrolled.add_controller(drag.clone());
        drag.group_with(&pinch);
    }

    /// Zoom by `factor`; in the 360° view, zoom the view instead of the flat photo.
    pub(in crate::library::lightbox) fn zoom_by(self: &Rc<Self>, factor: f64) {
        if self.panorama.is_active() {
            return self.panorama.zoom(factor);
        }
        self.set_zoom(self.zoom_level.get() * factor);
    }

    pub(in crate::library::lightbox) fn zoom_reset(self: &Rc<Self>) {
        if self.panorama.is_active() {
            return self.panorama.reset();
        }
        self.set_zoom(1.0);
    }

    /// Share of the photo's real pixels a fitted photo shows on screen; 1.0 when unknown.
    fn fit_scale(&self) -> f64 {
        let picture = self.active_picture();
        let Some(paintable) = picture.paintable() else {
            return 1.0;
        };
        let (width, height) = (paintable.intrinsic_width(), paintable.intrinsic_height());
        // The asset's size can be a thumbnail's when Immich has none; the decode never is.
        let asset_edge = self
            .current_item()
            .map_or(0, |item| item.width().max(item.height()));
        let real_edge = f64::from(width.max(height)).max(f64::from(asset_edge));
        if width <= 0 || height <= 0 || real_edge <= 0.0 {
            return 1.0;
        }
        let (fit_w, fit_h) = fit_size(width, height, &self.picture.scrolled);
        let scale = self.ui.window.surface().map_or(1.0, |s| s.scale());
        fit_w.max(fit_h) * scale / real_edge
    }

    pub(in crate::library::lightbox) fn show_zoom_label(&self) {
        let label = zoom_label(self.zoom_level.get(), self.fit_scale());
        self.actions.zoom_reset.set_label(&label);
    }

    /// Zoom to `z` (from fit up to `max_zoom`), keeping the focal point under the cursor.
    fn set_zoom(self: &Rc<Self>, z: f64) {
        // Zooming would swap in a sharper decode of the unedited photo under the crop box.
        if self.edit_mode.is_active() {
            return;
        }
        let z_new = z.clamp(1.0, max_zoom(self.fit_scale()));
        let z_old = self.zoom_level.get();
        if (z_new - z_old).abs() < 0.0001 {
            self.show_zoom_label();
            return;
        }
        let focus = self.focal_point();
        let scrolled = &self.picture.scrolled;
        let ratio = z_new / z_old.max(0.0001);
        let target = (
            (scrolled.hadjustment().value() + focus.0) * ratio - focus.0,
            (scrolled.vadjustment().value() + focus.1) * ratio - focus.1,
        );

        self.zoom_level.set(z_new);
        let fitted = (z_new - 1.0).abs() < 0.001;
        if !fitted {
            self.stop_live();
        }
        // Swiping stays off while face mode owns drags on the photo.
        let swipe = fitted && !self.face_mode.is_active();
        self.picture.strip.set_swipe_enabled(swipe);
        let content = apply_lightbox_zoom(&self.active_picture(), scrolled, z_new);
        self.show_zoom_label();
        self.scroll_zoomed(content, target);
        if let Some((content_width, _)) = content {
            self.sharpen_for_zoom(content_width);
        }
    }

    /// The point zoom keeps still: the cursor if it's inside the viewer, else the centre.
    fn focal_point(&self) -> (f64, f64) {
        let scrolled = &self.picture.scrolled;
        let viewer_w = f64::from(scrolled.width().max(1));
        let viewer_h = f64::from(scrolled.height().max(1));
        self.cursor_pos
            .get()
            .filter(|&(x, y)| x >= 0.0 && y >= 0.0 && x <= viewer_w && y <= viewer_h)
            .unwrap_or((viewer_w / 2.0, viewer_h / 2.0))
    }

    /// Scroll to `target` within the zoomed `content` size.
    fn scroll_zoomed(&self, content: Option<(f64, f64)>, target: (f64, f64)) {
        let scrolled = &self.picture.scrolled;
        let viewer_w = f64::from(scrolled.width().max(1));
        let viewer_h = f64::from(scrolled.height().max(1));
        let (hadj, vadj) = (scrolled.hadjustment(), scrolled.vadjustment());
        // Pre-set adjustment ranges to match the new content size so the
        // scroll position can be applied in the same frame. Without this
        // the value would be clamped to the stale (old-zoom) range and
        // corrected only after layout, causing a one-frame flicker.
        let (cw, ch) = content.unwrap_or((viewer_w, viewer_h));
        hadj.set_upper(cw.max(viewer_w));
        hadj.set_page_size(viewer_w);
        vadj.set_upper(ch.max(viewer_h));
        vadj.set_page_size(viewer_h);
        hadj.set_value(target.0);
        vadj.set_value(target.1);
    }
}
