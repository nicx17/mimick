use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use chrono::NaiveDate;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::{ObjectSubclassExt, ObjectSubclassIsExt};

use super::{LayoutConfig, LibraryAssetModel, MasonryCanvas, imp};
use crate::config::DateHeadings;
use crate::library::asset_object::AssetObject;
use crate::library::timeline::dates::{day_groups, day_heading, local_day, month_heading};
use crate::library::timeline::index::{MonthState, TimelineIndex};
use crate::library::timeline::layout::{
    Group, HEADING_HEIGHT, LaidSection, PackedSection, Placed, TimelineLayout, anchored_scroll,
    compose, estimated_height, pack_run, pack_section,
};

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
    months: usize,
    packed: PackedSection,
}

impl MasonryCanvas {
    /// Lay photos out by month from `index`; `None` returns to the plain grid.
    /// Without `dated`, its sections are pages, not months, and get no date headings.
    pub fn set_timeline(&self, index: Option<Rc<RefCell<TimelineIndex>>>, dated: bool) {
        let imp = self.imp();
        *imp.timeline.borrow_mut() = index;
        imp.timeline_dated.set(dated);
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
        forget_month(&mut imp.section_cache.borrow_mut(), month);
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

    /// Each laid row's top and the model position of its first photo, top to bottom.
    pub fn row_starts(&self) -> Vec<(f32, Option<u32>)> {
        self.imp()
            .rows
            .borrow()
            .iter()
            .map(|row| (row.y, row.items.first().map(|item| item.asset_index)))
            .collect()
    }

    pub fn heading_tops(&self) -> Vec<f32> {
        self.imp()
            .headings
            .borrow()
            .iter()
            .map(|heading| heading.y)
            .collect()
    }

    /// The photos under the date heading at canvas height `y`, as `(first, count)`.
    pub(super) fn heading_range_at(&self, y: f32) -> Option<(u32, u32)> {
        self.imp()
            .headings
            .borrow()
            .iter()
            .find(|heading| (heading.y..heading.y + HEADING_HEIGHT).contains(&y))
            .map(|heading| (heading.first, heading.count))
    }

    /// The photos under the same date heading as the photo at `pos`.
    pub(super) fn heading_range_of(&self, pos: u32) -> Option<(u32, u32)> {
        self.imp()
            .headings
            .borrow()
            .iter()
            .find(|heading| (heading.first..heading.first + heading.count).contains(&pos))
            .map(|heading| (heading.first, heading.count))
    }

    /// Selects every photo in `(first, count)`, or clears them when they all are already.
    pub(super) fn toggle_group(&self, (first, count): (u32, u32)) {
        let Some(sel) = self.imp().selection.get() else {
            return;
        };
        if all_selected(sel, first, count) {
            sel.unselect_range(first, count);
        } else {
            sel.select_range(first, count, false);
        }
    }
}

pub(super) fn all_selected(sel: &gtk::MultiSelection, first: u32, count: u32) -> bool {
    count > 0 && sel.selection_in_range(first, count).size() == u64::from(count)
}

impl imp::MasonryCanvas {
    pub(super) fn timeline_layout(
        &self,
        model: &LibraryAssetModel,
        index: &TimelineIndex,
        width: f32,
    ) -> TimelineLayout {
        let cfg = self.cfg();
        let mode = self.headings_mode();
        let key = SectionKey {
            width,
            min_row: cfg.min_row_height,
            max_row: cfg.max_row_height,
            gap: cfg.gap,
            headings: mode,
        };
        let offsets = self.pack_loaded_months(model, index, key, cfg);
        let cache = self.section_cache.borrow();
        let months = index.months();
        let mut placed: Vec<Placed<'_>> = Vec::with_capacity(months.len());
        let mut month = 0;
        while month < months.len() {
            match cache.get(&month) {
                Some(cached) if matches!(index.state(month), MonthState::Loaded(_)) => {
                    placed.push(Placed::Packed {
                        section: &cached.packed,
                        model_offset: offsets[month],
                    });
                    month += cached.months;
                }
                _ => {
                    placed.push(Placed::Estimated {
                        height: estimated_height(months[month].count, width, cfg, mode),
                    });
                    month += 1;
                }
            }
        }
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
        let months = index.months();
        let lens = loaded_lens(index);
        let offsets = model_offsets(&lens);
        let mut cache = self.section_cache.borrow_mut();
        let mut month = 0;
        while month < months.len() {
            let run = run_lens(&lens[month..], key.headings);
            if run.is_empty() {
                month += 1;
                continue;
            }
            let fresh = cache
                .get(&month)
                .is_some_and(|cached| cached.key == key && cached.months == run.len());
            if !fresh {
                let end = month + run.len();
                cache.retain(|first, cached| first + cached.months <= month || *first >= end);
                let packed = pack_month(
                    model,
                    offsets[month],
                    &run,
                    &months[month].key,
                    key,
                    cfg,
                    today,
                );
                let months = run.len();
                cache.insert(
                    month,
                    CachedSection {
                        key,
                        months,
                        packed,
                    },
                );
            }
            month += run.len();
        }
        offsets
    }

    /// The date headings to lay out: none on a timeline of pages.
    pub(super) fn headings_mode(&self) -> DateHeadings {
        if self.timeline_dated.get() {
            self.date_headings.get()
        } else {
            DateHeadings::Off
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
                    forget_month(&mut self.section_cache.borrow_mut(), month);
                }
                self.schedule_relayout();
            }
            None => self.invalidate_layout(),
        }
    }

    /// Lay out again after a gap change without losing the reader's place: the timeline
    /// stays on the same month, other grids keep their relative scroll position.
    pub(in crate::library::masonry) fn relayout_keeping_place(&self) {
        if self.timeline.borrow().is_some() {
            self.section_cache.borrow_mut().clear();
            self.schedule_relayout();
            return;
        }
        let fraction = self.find_vadjustment().and_then(|adj| {
            let range = adj.upper() - adj.page_size();
            (range > 0.0).then(|| adj.value() / range)
        });
        self.invalidate_layout();
        let Some(fraction) = fraction else {
            return;
        };
        let frames = std::cell::Cell::new(0);
        self.obj().add_tick_callback(move |canvas, _| {
            frames.set(frames.get() + 1);
            if frames.get() < 2 {
                return glib::ControlFlow::Continue;
            }
            if let Some(adj) = canvas.imp().find_vadjustment() {
                adj.set_value(fraction * (adj.upper() - adj.page_size()));
            }
            glib::ControlFlow::Break
        });
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

/// Each month's asset count once loaded; `None` while it isn't.
fn loaded_lens(index: &TimelineIndex) -> Vec<Option<u32>> {
    (0..index.months().len())
        .map(|month| match index.state(month) {
            MonthState::Loaded(len) => Some(len),
            _ => None,
        })
        .collect()
}

/// Where each month's first asset sits in the model; unloaded months take no room.
fn model_offsets(lens: &[Option<u32>]) -> Vec<u32> {
    lens.iter()
        .scan(0, |offset, len| {
            let start = *offset;
            *offset += len.unwrap_or(0);
            Some(start)
        })
        .collect()
}

fn run_lens(lens: &[Option<u32>], headings: DateHeadings) -> Vec<u32> {
    let loaded = lens.iter().map_while(|len| *len);
    match headings {
        DateHeadings::Off => loaded.collect(),
        DateHeadings::Day | DateHeadings::Month => loaded.take(1).collect(),
    }
}

fn forget_month(cache: &mut HashMap<usize, CachedSection>, month: usize) {
    cache.retain(|first, cached| !(*first <= month && month < first + cached.months));
}

fn pack_month(
    model: &LibraryAssetModel,
    offset: u32,
    lens: &[u32],
    month_key: &str,
    key: SectionKey,
    cfg: LayoutConfig,
    today: NaiveDate,
) -> PackedSection {
    let width = key.width;
    let len: u32 = lens.iter().sum();
    let objects: Vec<AssetObject> = (offset..offset + len)
        .filter_map(|position| model.item(position).and_downcast::<AssetObject>())
        .collect();
    let dims: Vec<(u32, u32)> = objects.iter().map(|o| (o.width(), o.height())).collect();
    let groups: Vec<Group<'_>> = match key.headings {
        DateHeadings::Off => return pack_run(&dims, lens, width, cfg),
        _ if objects.is_empty() => Vec::new(),
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
    };
    pack_section(&groups, width, cfg)
}

#[cfg(test)]
mod tests {
    use super::model_offsets;

    #[test]
    fn model_offsets_skip_unloaded_months() {
        assert_eq!(
            model_offsets(&[Some(3), None, Some(2), Some(4)]),
            [0, 3, 3, 5]
        );
        assert!(model_offsets(&[]).is_empty());
    }
}
