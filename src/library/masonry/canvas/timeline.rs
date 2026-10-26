use std::cell::RefCell;
use std::rc::Rc;

use chrono::NaiveDate;
use gtk::glib;
use gtk::graphene::{Point, Rect};
use gtk::pango;
use gtk::prelude::*;
use gtk::subclass::prelude::{ObjectSubclassExt, ObjectSubclassIsExt};

use super::{LayoutConfig, LibraryAssetModel, MasonryCanvas, imp, placeholder_color};
use crate::config::DateHeadings;
use crate::library::asset_object::AssetObject;
use crate::library::timeline::dates::{day_groups, day_heading, local_day, month_heading};
use crate::library::timeline::index::{MonthState, TimelineIndex};
use crate::library::timeline::layout::{
    ESTIMATED_ASPECT, Group, HEADING_HEIGHT, LaidSection, PackedSection, Placed, TimelineLayout,
    anchored_scroll, compose, estimated_height, pack_section,
};

const HEADING_INSET: f32 = 12.0;
const PLACEHOLDER_GAP: f32 = 2.0;

#[derive(Clone, Copy, PartialEq)]
struct SectionKey {
    width: f32,
    min_row: f32,
    max_row: f32,
    gap: f32,
    headings: DateHeadings,
}

pub(super) struct CachedSection {
    key: SectionKey,
    packed: PackedSection,
}

impl MasonryCanvas {
    /// Lay Photos out by month from `index`; `None` returns to the plain grid.
    pub fn set_timeline(&self, index: Option<Rc<RefCell<TimelineIndex>>>) {
        let imp = self.imp();
        *imp.timeline.borrow_mut() = index;
        imp.section_cache.borrow_mut().clear();
        imp.sections.borrow_mut().clear();
        imp.headings.borrow_mut().clear();
        imp.invalidate_layout();
        self.queue_draw();
    }

    pub fn set_date_headings(&self, mode: DateHeadings) {
        let imp = self.imp();
        if imp.date_headings.replace(mode) != mode && imp.timeline.borrow().is_some() {
            imp.schedule_relayout();
        }
    }

    pub fn set_month_request_handler(&self, f: impl Fn(Vec<usize>) + 'static) {
        *self.imp().month_request_handler.borrow_mut() = Some(Rc::new(f));
    }

    pub fn invalidate_month(&self, month: usize) {
        let imp = self.imp();
        imp.section_cache.borrow_mut().remove(&month);
        imp.schedule_relayout();
    }

    pub fn timeline_sections(&self) -> Vec<LaidSection> {
        self.imp().sections.borrow().clone()
    }

    pub fn scroll_top(&self) -> f32 {
        self.imp().viewport().0
    }

    /// Scroll so canvas position `y` sits at the top of the view.
    pub fn scroll_to(&self, y: f32) {
        let imp = self.imp();
        if let Some(adj) = imp.find_vadjustment() {
            adj.set_value(adj.value() + f64::from(y - imp.viewport().0));
        }
    }
}

impl imp::MasonryCanvas {
    pub(super) fn timeline_layout(
        &self,
        model: &LibraryAssetModel,
        index: &TimelineIndex,
        width: f32,
    ) -> TimelineLayout {
        let cfg = self.cfg();
        let mode = self.date_headings.get();
        let key = SectionKey {
            width,
            min_row: cfg.min_row_height,
            max_row: cfg.max_row_height,
            gap: cfg.gap,
            headings: mode,
        };
        let offsets = self.pack_loaded_months(model, index, key, cfg);
        let cache = self.section_cache.borrow();
        let placed: Vec<Placed<'_>> = index
            .months()
            .iter()
            .enumerate()
            .map(|(month, entry)| match cache.get(&month) {
                Some(cached) if matches!(index.state(month), MonthState::Loaded(_)) => {
                    Placed::Packed {
                        section: &cached.packed,
                        model_offset: offsets[month],
                    }
                }
                _ => Placed::Estimated {
                    height: estimated_height(entry.count, width, cfg, mode),
                },
            })
            .collect();
        compose(&placed)
    }

    fn pack_loaded_months(
        &self,
        model: &LibraryAssetModel,
        index: &TimelineIndex,
        key: SectionKey,
        cfg: LayoutConfig,
    ) -> Vec<u32> {
        let today = chrono::Local::now().date_naive();
        let mut offsets = Vec::with_capacity(index.months().len());
        let mut offset = 0;
        let mut cache = self.section_cache.borrow_mut();
        for (month, entry) in index.months().iter().enumerate() {
            offsets.push(offset);
            let MonthState::Loaded(len) = index.state(month) else {
                continue;
            };
            if cache.get(&month).is_none_or(|cached| cached.key != key) {
                let packed = pack_month(model, offset, len, &entry.key, key, cfg, today);
                cache.insert(month, CachedSection { key, packed });
            }
            offset += len;
        }
        offsets
    }

    pub(super) fn paint_timeline_extras(
        &self,
        snapshot: &gtk::Snapshot,
        top: f32,
        bottom: f32,
        width: f32,
    ) {
        self.paint_estimated_sections(snapshot, top, bottom, width);
        self.paint_headings(snapshot, top, bottom);
    }

    fn paint_estimated_sections(
        &self,
        snapshot: &gtk::Snapshot,
        top: f32,
        bottom: f32,
        width: f32,
    ) {
        let cfg = self.cfg();
        let gap = cfg.gap.max(PLACEHOLDER_GAP);
        let row_h = (cfg.min_row_height + cfg.max_row_height) / 2.0;
        let per_row = ((width - 2.0 * gap) / (ESTIMATED_ASPECT * row_h))
            .floor()
            .max(1.0);
        let tile_w = (width - gap * (per_row + 1.0)) / per_row;
        let color = placeholder_color();
        let sections = self.sections.borrow();
        for section in sections
            .iter()
            .filter(|s| !s.loaded && s.y + s.h > top && s.y < bottom)
        {
            let end = section.y + section.h;
            let mut y = section.y + gap;
            while y < end.min(bottom) {
                let h = row_h.min(end - y);
                if y + h > top {
                    for i in 0..per_row as usize {
                        let x = gap + i as f32 * (tile_w + gap);
                        snapshot.append_color(&color, &Rect::new(x, y, tile_w, h));
                    }
                }
                y += row_h + gap;
            }
        }
    }

    fn paint_headings(&self, snapshot: &gtk::Snapshot, top: f32, bottom: f32) {
        let widget = self.obj();
        let color = widget.color();
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
        for heading in self
            .headings
            .borrow()
            .iter()
            .filter(|h| h.y + HEADING_HEIGHT > top && h.y < bottom)
        {
            let layout = widget.create_pango_layout(Some(&heading.text));
            layout.set_attributes(Some(&attrs));
            let (_, text_h) = layout.pixel_size();
            snapshot.save();
            snapshot.translate(&Point::new(
                HEADING_INSET,
                heading.y + (HEADING_HEIGHT - text_h as f32) / 2.0,
            ));
            snapshot.append_layout(&layout, &color);
            snapshot.restore();
        }
    }

    pub(super) fn request_visible_months(&self, band_top: f32, band_bottom: f32) {
        if self.flinging.get() || self.month_request_pending.get() {
            return;
        }
        let Some(index) = self.timeline.borrow().clone() else {
            return;
        };
        let Some(handler) = self.month_request_handler.borrow().clone() else {
            return;
        };
        let months: Vec<usize> = {
            let index = index.borrow();
            self.sections
                .borrow()
                .iter()
                .enumerate()
                .filter(|(month, s)| {
                    s.y + s.h >= band_top
                        && s.y <= band_bottom
                        && index.state(*month) == MonthState::Unloaded
                })
                .map(|(month, _)| month)
                .collect()
        };
        if months.is_empty() {
            return;
        }
        self.month_request_pending.set(true);
        let weak = self.obj().downgrade();
        glib::idle_add_local_once(move || {
            if let Some(canvas) = weak.upgrade() {
                canvas.imp().month_request_pending.set(false);
            }
            handler(months);
        });
    }

    /// Relayout once, just before the next frame, after the model or index changed.
    pub(super) fn schedule_relayout(&self) {
        if self.relayout_pending.replace(true) {
            return;
        }
        let weak = self.obj().downgrade();
        glib::idle_add_local_full(glib::Priority::HIGH_IDLE, move || {
            if let Some(canvas) = weak.upgrade() {
                let imp = canvas.imp();
                imp.relayout_pending.set(false);
                imp.relayout_anchored();
            }
            glib::ControlFlow::Break
        });
    }

    pub(super) fn relayout_for_position(&self, position: u32) {
        let timeline = self.timeline.borrow().clone();
        match timeline {
            Some(index) => {
                if let Some(month) = index.borrow().month_at(position) {
                    self.section_cache.borrow_mut().remove(&month);
                }
                self.schedule_relayout();
            }
            None => self.invalidate_layout(),
        }
    }

    fn relayout_anchored(&self) {
        let width = self.cached_width.get();
        let old_sections = self.sections.borrow().clone();
        let old_height = self.layout_h.get();
        let (top, _) = self.viewport();
        self.invalidate_layout();
        if width <= 0.0 {
            return;
        }
        let new_height = self.layout_for_width(width);
        let new_top = anchored_scroll(&old_sections, &self.sections.borrow(), top);
        let delta = f64::from(new_top - top);
        let Some(adj) = self.find_vadjustment() else {
            return;
        };
        if delta.abs() < 0.5 {
            return;
        }
        self.stop_kinetic_scrolling();
        adj.set_upper(adj.upper() + f64::from(new_height - old_height));
        let value = adj.value() + delta;
        self.last_scroll.set((value, glib::monotonic_time()));
        adj.set_value(value);
    }

    /// Kinetic scrolling sets absolute positions, which would undo the anchor shift.
    fn stop_kinetic_scrolling(&self) {
        let scrolled = self
            .obj()
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>();
        if let Some(scrolled) = scrolled
            && scrolled.is_kinetic_scrolling()
        {
            scrolled.set_kinetic_scrolling(false);
            scrolled.set_kinetic_scrolling(true);
        }
    }
}

fn pack_month(
    model: &LibraryAssetModel,
    offset: u32,
    len: u32,
    month_key: &str,
    key: SectionKey,
    cfg: LayoutConfig,
    today: NaiveDate,
) -> PackedSection {
    let width = key.width;
    let objects: Vec<AssetObject> = (offset..offset + len)
        .filter_map(|position| model.item(position).and_downcast::<AssetObject>())
        .collect();
    if objects.is_empty() {
        return pack_section(&[], width, cfg);
    }
    let dims: Vec<(u32, u32)> = objects.iter().map(|o| (o.width(), o.height())).collect();
    let groups: Vec<Group<'_>> = match key.headings {
        DateHeadings::Day => {
            let days: Vec<Option<NaiveDate>> = objects
                .iter()
                .map(|o| local_day(&o.local_date_time()))
                .collect();
            day_groups(&days)
                .into_iter()
                .map(|range| Group {
                    heading: days[range.start].map(|day| day_heading(day, today)),
                    dims: &dims[range],
                })
                .collect()
        }
        DateHeadings::Month => vec![Group {
            heading: month_heading(month_key),
            dims: &dims,
        }],
        DateHeadings::Off => vec![Group {
            heading: None,
            dims: &dims,
        }],
    };
    pack_section(&groups, width, cfg)
}
