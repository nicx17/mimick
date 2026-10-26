use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::graphene::{Point, Rect, Size};
use gtk::gsk::RoundedRect;
use gtk::pango;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use libadwaita::prelude::AnimationExt;

use super::dates::month_short;
use super::index::TimelineIndex;
use super::layout::{LaidSection, section_at};
use super::marks::{Mark, canvas_to_track, handle_top, marks, track_to_canvas};
use crate::library::masonry::canvas::MasonryCanvas;

const WIDTH: i32 = 64;
const TRACK_PAD: f32 = 12.0;
const MIN_GAP: f32 = 14.0;
const RIGHT_INSET: f32 = 10.0;
const DOT_SIZE: f32 = 4.0;
const MARKER_W: f32 = 24.0;
const PILL_PAD_X: f32 = 10.0;
const PILL_PAD_Y: f32 = 5.0;
const PILL_GAP: f32 = 8.0;
const HANDLE_W: f32 = 28.0;
const HANDLE_H: f32 = 44.0;
const HANDLE_HOLD: std::time::Duration = std::time::Duration::from_millis(1500);
const HANDLE_FADE_MS: u32 = 300;

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::rc::Rc;

    use gtk::glib;
    use gtk::subclass::prelude::*;

    use crate::library::masonry::canvas::MasonryCanvas;
    use crate::library::timeline::index::TimelineIndex;

    #[derive(Default)]
    pub struct TimelineScrubber {
        pub canvas: OnceCell<MasonryCanvas>,
        pub adjustment: OnceCell<gtk::Adjustment>,
        pub compact: Cell<bool>,
        pub hide_timer: RefCell<Option<glib::SourceId>>,
        pub fade: RefCell<Option<libadwaita::TimedAnimation>>,
        pub index: RefCell<Option<Rc<RefCell<TimelineIndex>>>>,
        pub pointer_y: Cell<Option<f32>>,
        pub dragging: Cell<bool>,
        pub drag_start: Cell<f32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TimelineScrubber {
        const NAME: &str = "MimickTimelineScrubber";
        type Type = super::TimelineScrubber;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for TimelineScrubber {}

    impl WidgetImpl for TimelineScrubber {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal if self.compact.get() => {
                    let width = super::HANDLE_W as i32;
                    (width, width, -1, -1)
                }
                gtk::Orientation::Horizontal => (super::WIDTH, super::WIDTH, -1, -1),
                _ => (0, 0, -1, -1),
            }
        }

        fn contains(&self, x: f64, y: f64) -> bool {
            self.obj().accepts_pointer(x as f32, y as f32)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct TimelineScrubber(ObjectSubclass<imp::TimelineScrubber>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl TimelineScrubber {
    pub fn new(canvas: &MasonryCanvas, scrolled: &gtk::ScrolledWindow) -> Self {
        let scrubber: Self = glib::Object::builder()
            .property("halign", gtk::Align::End)
            .property("visible", false)
            .build();
        let _ = scrubber.imp().canvas.set(canvas.clone());
        let adj = scrolled.vadjustment();
        let _ = scrubber.imp().adjustment.set(adj.clone());
        let weak = scrubber.downgrade();
        adj.connect_value_changed(move |_| {
            if let Some(scrubber) = weak.upgrade() {
                if scrubber.imp().compact.get() {
                    scrubber.reveal_handle();
                }
                scrubber.queue_draw();
            }
        });
        let weak = scrubber.downgrade();
        adj.connect_changed(move |_| {
            if let Some(scrubber) = weak.upgrade() {
                scrubber.queue_draw();
            }
        });
        scrubber.connect_hover();
        scrubber.connect_drag();
        scrubber
    }

    /// Show the scrubber for `index`; `None` hides it.
    pub fn set_index(&self, index: Option<Rc<RefCell<TimelineIndex>>>) {
        self.set_visible(index.is_some());
        *self.imp().index.borrow_mut() = index;
        self.sync_gutter();
        self.queue_draw();
    }

    /// Phone width: a drag handle that appears while scrolling instead of the full strip.
    pub fn set_compact(&self, compact: bool) {
        let imp = self.imp();
        if imp.compact.replace(compact) == compact {
            return;
        }
        if let Some(fade) = imp.fade.take() {
            fade.pause();
        }
        self.set_opacity(if compact { 0.0 } else { 1.0 });
        self.sync_gutter();
        self.queue_resize();
    }

    /// Keep photos out from under the year labels.
    fn sync_gutter(&self) {
        if let Some(canvas) = self.imp().canvas.get() {
            let gutter = self.is_visible() && !self.imp().compact.get();
            canvas.set_margin_end(if gutter { WIDTH } else { 0 });
        }
    }

    fn accepts_pointer(&self, x: f32, y: f32) -> bool {
        if !self.imp().compact.get() {
            return x >= 0.0 && x <= self.width() as f32 && y >= 0.0 && y <= self.height() as f32;
        }
        let top = self.handle_rect().y();
        self.opacity() > 0.01 && y >= top && y <= top + HANDLE_H
    }

    fn handle_rect(&self) -> Rect {
        let (top, track_h) = self.track();
        let y = self.imp().adjustment.get().map_or(0.0, |adj| {
            handle_top(adj.value(), adj.upper(), adj.page_size(), track_h, HANDLE_H)
        });
        Rect::new(self.width() as f32 - HANDLE_W, top + y, HANDLE_W, HANDLE_H)
    }

    fn reveal_handle(&self) {
        let imp = self.imp();
        if let Some(fade) = imp.fade.take() {
            fade.pause();
        }
        self.set_opacity(1.0);
        if let Some(timer) = imp.hide_timer.take() {
            timer.remove();
        }
        let weak = self.downgrade();
        let timer = glib::timeout_add_local_once(HANDLE_HOLD, move || {
            if let Some(scrubber) = weak.upgrade() {
                scrubber.imp().hide_timer.take();
                scrubber.fade_handle();
            }
        });
        imp.hide_timer.replace(Some(timer));
    }

    fn fade_handle(&self) {
        let imp = self.imp();
        if imp.dragging.get() {
            self.reveal_handle();
            return;
        }
        let target = libadwaita::PropertyAnimationTarget::new(self, "opacity");
        let fade =
            libadwaita::TimedAnimation::new(self, self.opacity(), 0.0, HANDLE_FADE_MS, target);
        fade.play();
        imp.fade.replace(Some(fade));
    }

    fn connect_hover(&self) {
        let motion = gtk::EventControllerMotion::new();
        let weak = self.downgrade();
        motion.connect_motion(move |_, _, y| {
            if let Some(scrubber) = weak.upgrade() {
                scrubber.imp().pointer_y.set(Some(y as f32));
                scrubber.queue_draw();
            }
        });
        let weak = self.downgrade();
        motion.connect_leave(move |_| {
            if let Some(scrubber) = weak.upgrade()
                && !scrubber.imp().dragging.get()
            {
                scrubber.imp().pointer_y.set(None);
                scrubber.queue_draw();
            }
        });
        self.add_controller(motion);
    }

    fn connect_drag(&self) {
        let drag = gtk::GestureDrag::new();
        let weak = self.downgrade();
        drag.connect_drag_begin(move |_, _, y| {
            if let Some(scrubber) = weak.upgrade() {
                let imp = scrubber.imp();
                imp.dragging.set(true);
                if imp.compact.get() {
                    imp.drag_start.set(scrubber.handle_rect().y());
                    scrubber.reveal_handle();
                } else {
                    imp.drag_start.set(y as f32);
                    scrubber.scrub_to(y as f32);
                }
            }
        });
        let weak = self.downgrade();
        drag.connect_drag_update(move |_, _, dy| {
            if let Some(scrubber) = weak.upgrade() {
                let y = scrubber.imp().drag_start.get() + dy as f32;
                if scrubber.imp().compact.get() {
                    scrubber.drag_handle_to(y);
                } else {
                    scrubber.scrub_to(y);
                }
            }
        });
        let weak = self.downgrade();
        drag.connect_drag_end(move |_, _, _| {
            if let Some(scrubber) = weak.upgrade() {
                scrubber.imp().dragging.set(false);
                scrubber.queue_draw();
            }
        });
        self.add_controller(drag);
    }

    fn track(&self) -> (f32, f32) {
        (TRACK_PAD, (self.height() as f32 - 2.0 * TRACK_PAD).max(1.0))
    }

    fn sections(&self) -> (Vec<LaidSection>, f32) {
        let Some(canvas) = self.imp().canvas.get() else {
            return (Vec::new(), 0.0);
        };
        let sections = canvas.timeline_sections();
        let height = sections.last().map_or(0.0, |s| s.y + s.h);
        (sections, height)
    }

    fn scrub_to(&self, y: f32) {
        self.imp().pointer_y.set(Some(y));
        let (top, track_h) = self.track();
        let (_, canvas_h) = self.sections();
        if let Some(canvas) = self.imp().canvas.get() {
            canvas.scroll_to(track_to_canvas(
                (y - top).clamp(0.0, track_h),
                track_h,
                canvas_h,
            ));
        }
        self.queue_draw();
    }

    fn drag_handle_to(&self, handle_y: f32) {
        let (top, track_h) = self.track();
        let travel = (track_h - HANDLE_H).max(1.0);
        if let Some(adj) = self.imp().adjustment.get() {
            let fraction = f64::from(((handle_y - top) / travel).clamp(0.0, 1.0));
            adj.set_value(fraction * (adj.upper() - adj.page_size()));
        }
    }

    fn draw_handle(
        &self,
        snapshot: &gtk::Snapshot,
        index: &TimelineIndex,
        sections: &[LaidSection],
    ) {
        let handle = self.handle_rect();
        rounded(
            snapshot,
            &handle,
            HANDLE_W / 2.0,
            &gdk::RGBA::new(0.0, 0.0, 0.0, 0.6),
        );
        let grip_w = HANDLE_W * 0.4;
        for offset in [-3.0, 3.0] {
            let grip = Rect::new(
                handle.x() + (HANDLE_W - grip_w) / 2.0,
                handle.y() + HANDLE_H / 2.0 + offset - 1.0,
                grip_w,
                2.0,
            );
            snapshot.append_color(&gdk::RGBA::new(1.0, 1.0, 1.0, 0.85), &grip);
        }
        if !self.imp().dragging.get() {
            return;
        }
        let Some(canvas) = self.imp().canvas.get() else {
            return;
        };
        let month = section_at(sections, canvas.scroll_top().max(0.0));
        if let Some(label) = month.and_then(|month| month_short(&index.months().get(month)?.key)) {
            self.draw_pill(snapshot, &label, handle.y() + HANDLE_H / 2.0);
        }
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let imp = self.imp();
        let Some(index) = imp.index.borrow().clone() else {
            return;
        };
        let Some(canvas) = imp.canvas.get() else {
            return;
        };
        let (sections, canvas_h) = self.sections();
        if canvas_h <= 0.0 {
            return;
        }
        if imp.compact.get() {
            self.draw_handle(snapshot, &index.borrow(), &sections);
            return;
        }
        self.draw_marks(snapshot, &index.borrow(), &sections, canvas_h);

        let (top, track_h) = self.track();
        let width = self.width() as f32;
        let accent = libadwaita::StyleManager::default().accent_color().to_rgba();
        let marker_y = top + canvas_to_track(canvas.scroll_top().max(0.0), track_h, canvas_h);
        snapshot.append_color(
            &accent,
            &Rect::new(width - MARKER_W, marker_y - 1.0, MARKER_W, 2.0),
        );
        self.draw_hover(snapshot, &index.borrow(), &sections, canvas_h, &accent);
    }

    fn draw_marks(
        &self,
        snapshot: &gtk::Snapshot,
        index: &TimelineIndex,
        sections: &[LaidSection],
        canvas_h: f32,
    ) {
        let (top, track_h) = self.track();
        let width = self.width() as f32;
        let fg = self.color();
        let tint = |alpha: f32| gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), alpha);
        for mark in marks(index.months(), sections, canvas_h, track_h, MIN_GAP) {
            let y = top + mark.y();
            match mark {
                Mark::Year { year, .. } => {
                    self.draw_text(
                        snapshot,
                        &year.to_string(),
                        width - RIGHT_INSET,
                        y,
                        &tint(0.7),
                    );
                }
                Mark::Month { .. } => {
                    let dot = Rect::new(
                        width - RIGHT_INSET - DOT_SIZE,
                        y - DOT_SIZE / 2.0,
                        DOT_SIZE,
                        DOT_SIZE,
                    );
                    rounded(snapshot, &dot, DOT_SIZE / 2.0, &tint(0.45));
                }
            }
        }
    }

    fn draw_hover(
        &self,
        snapshot: &gtk::Snapshot,
        index: &TimelineIndex,
        sections: &[LaidSection],
        canvas_h: f32,
        accent: &gdk::RGBA,
    ) {
        let Some(pointer_y) = self.imp().pointer_y.get() else {
            return;
        };
        let (top, track_h) = self.track();
        let y = pointer_y.clamp(top, top + track_h);
        snapshot.append_color(accent, &Rect::new(0.0, y - 1.0, self.width() as f32, 2.0));
        let canvas_y = track_to_canvas(y - top, track_h, canvas_h);
        let month = section_at(sections, canvas_y).or(sections.len().checked_sub(1));
        if let Some(label) = month.and_then(|month| month_short(&index.months().get(month)?.key)) {
            self.draw_pill(snapshot, &label, y);
        }
    }

    fn draw_text(
        &self,
        snapshot: &gtk::Snapshot,
        text: &str,
        right: f32,
        y: f32,
        color: &gdk::RGBA,
    ) {
        let layout = self.create_pango_layout(Some(text));
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrFloat::new_scale(0.8));
        attrs.insert(pango::AttrFontFeatures::new("tnum"));
        layout.set_attributes(Some(&attrs));
        let (w, h) = layout.pixel_size();
        snapshot.save();
        snapshot.translate(&Point::new(right - w as f32, y - h as f32 / 2.0));
        snapshot.append_layout(&layout, color);
        snapshot.restore();
    }

    /// Month label in an OSD pill to the left of the track, over the photos.
    fn draw_pill(&self, snapshot: &gtk::Snapshot, text: &str, y: f32) {
        let layout = self.create_pango_layout(Some(text));
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
        layout.set_attributes(Some(&attrs));
        let (w, h) = layout.pixel_size();
        let pill_w = w as f32 + 2.0 * PILL_PAD_X;
        let pill_h = h as f32 + 2.0 * PILL_PAD_Y;
        let x = -PILL_GAP - pill_w;
        let pill = Rect::new(x, y - pill_h / 2.0, pill_w, pill_h);
        rounded(
            snapshot,
            &pill,
            pill_h / 2.0,
            &gdk::RGBA::new(0.0, 0.0, 0.0, 0.75),
        );
        snapshot.save();
        snapshot.translate(&Point::new(x + PILL_PAD_X, y - h as f32 / 2.0));
        snapshot.append_layout(&layout, &gdk::RGBA::WHITE);
        snapshot.restore();
    }
}

fn rounded(snapshot: &gtk::Snapshot, rect: &Rect, radius: f32, color: &gdk::RGBA) {
    let corner = Size::new(radius, radius);
    snapshot.push_rounded_clip(&RoundedRect::new(*rect, corner, corner, corner, corner));
    snapshot.append_color(color, rect);
    snapshot.pop();
}
