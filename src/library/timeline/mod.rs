//! Photos as a month timeline: month bookkeeping, date labels, layout, and the scrubber.

pub mod dates;
pub mod index;
pub mod layout;

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use super::LibraryWindowUi;
use super::state::{LibrarySortMode, LibrarySource};
use crate::api_client::LibraryAsset;
use index::{Month, TimelineIndex};
use layout::section_at;

/// Called once a month finishes loading: `(insert position, photos added)`, or `None` on failure.
type Waiter = Box<dyn FnOnce(Option<(u32, u32)>)>;

pub(super) struct Timeline {
    index: Rc<RefCell<TimelineIndex>>,
    generation: u64,
    waiters: RefCell<Vec<(usize, Waiter)>>,
}

pub(super) fn eligible(source: &LibrarySource, sort: &LibrarySortMode) -> bool {
    matches!(source, LibrarySource::AllAssets) && matches!(sort, LibrarySortMode::NewestFirst)
}

pub(super) fn is_active(ui: &LibraryWindowUi) -> bool {
    ui.timeline.borrow().is_some()
}

pub(super) fn connect(ui: &Rc<LibraryWindowUi>) {
    let weak = Rc::downgrade(ui);
    ui.grid.canvas.set_month_request_handler(move |months| {
        if let Some(ui) = weak.upgrade() {
            load_months(&ui, months);
        }
    });
    ui.grid
        .canvas
        .set_date_headings(*ui.ctx.date_headings.borrow());
    let mut changes = ui.ctx.date_headings.subscribe();
    let canvas = ui.grid.canvas.downgrade();
    glib::MainContext::default().spawn_local(async move {
        while changes.changed().await.is_ok() {
            let mode = *changes.borrow_and_update();
            let Some(canvas) = canvas.upgrade() else {
                break;
            };
            canvas.set_date_headings(mode);
        }
    });
}

/// Fetch the month counts and show Photos as a timeline; page by page if that fails.
pub(super) fn open(
    ui: &Rc<LibraryWindowUi>,
    request: (u64, LibrarySource, u32),
    keep: Option<(String, f32)>,
) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.fetch_time_buckets().await {
            Ok(buckets) => {
                let months = buckets
                    .into_iter()
                    .map(|bucket| Month {
                        key: bucket.month,
                        count: bucket.count,
                    })
                    .collect();
                show(&ui, request.0, months, keep);
            }
            Err(err) => {
                log::warn!("Timeline unavailable, loading Photos page by page: {err}");
                super::load_pages(ui, request, false);
            }
        }
    });
}

fn show(
    ui: &Rc<LibraryWindowUi>,
    generation: u64,
    months: Vec<Month>,
    keep: Option<(String, f32)>,
) {
    let empty = months.iter().all(|month| month.count == 0);
    if !ui
        .ctx
        .library_state
        .lock()
        .enter_timeline(generation, empty)
    {
        return;
    }
    let anchor = keep.and_then(|(key, offset)| {
        Some((months.iter().position(|month| month.key == key)?, offset))
    });
    let index = Rc::new(RefCell::new(TimelineIndex::new(months)));
    ui.timeline.replace(Some(Rc::new(Timeline {
        index: index.clone(),
        generation,
        waiters: RefCell::default(),
    })));
    ui.grid.model.reset_with_objects(Vec::new());
    ui.grid.canvas.set_timeline(Some(index));
    super::sync_content_state(ui);
    if let Some((month, offset)) = anchor {
        scroll_when_laid_out(ui, month, offset);
    }
}

pub(super) fn close(ui: &LibraryWindowUi) {
    if ui.timeline.take().is_some() {
        ui.grid.canvas.set_timeline(None);
    }
}

/// Refetch the counts and months, keeping the same point of the same month at the top.
pub(super) fn reload(ui: &Rc<LibraryWindowUi>) {
    let keep = anchor(ui);
    let request = {
        let mut state = ui.ctx.library_state.lock();
        let source = state.source.clone();
        state.switch_source(source)
    };
    super::source_header::show(ui, &request.1);
    open(ui, request, keep);
}

fn anchor(ui: &LibraryWindowUi) -> Option<(String, f32)> {
    let timeline = ui.timeline.borrow().clone()?;
    let sections = ui.grid.canvas.timeline_sections();
    let top = ui.grid.canvas.scroll_top();
    let month = section_at(&sections, top)?;
    let key = timeline.index.borrow().months().get(month)?.key.clone();
    Some((key, top - sections[month].y))
}

fn scroll_when_laid_out(ui: &LibraryWindowUi, month: usize, offset: f32) {
    let handler = Rc::new(RefCell::new(None::<glib::SignalHandlerId>));
    let (canvas, handler_ref) = (ui.grid.canvas.clone(), handler.clone());
    let id = ui.grid.scrolled.vadjustment().connect_changed(move |adj| {
        let Some(section) = canvas.timeline_sections().get(month).copied() else {
            return;
        };
        canvas.scroll_to(section.y + offset);
        if let Some(id) = handler_ref.borrow_mut().take() {
            adj.disconnect(id);
        }
    });
    handler.replace(Some(id));
}

pub(super) fn load_months(ui: &Rc<LibraryWindowUi>, months: Vec<usize>) {
    let Some(timeline) = ui.timeline.borrow().clone() else {
        return;
    };
    for month in months {
        load_month(ui, &timeline, month);
    }
}

fn load_month(ui: &Rc<LibraryWindowUi>, timeline: &Rc<Timeline>, month: usize) {
    let key = {
        let mut index = timeline.index.borrow_mut();
        if !index.begin_load(month) {
            return;
        }
        index.months()[month].key.clone()
    };
    let (ui, timeline) = (ui.clone(), timeline.clone());
    glib::MainContext::default().spawn_local(async move {
        let result = ui.ctx.api_client.fetch_month_assets(&key).await;
        if !is_current(&ui, &timeline) {
            return;
        }
        let inserted = match result {
            Ok(assets) => insert_month(&ui, &timeline, month, assets),
            Err(err) => {
                log::warn!("Could not load photos for {key}: {err}");
                timeline.index.borrow_mut().fail_load(month);
                None
            }
        };
        let done: Vec<(usize, Waiter)> = {
            let mut waiters = timeline.waiters.borrow_mut();
            let (done, rest) = std::mem::take(&mut *waiters)
                .into_iter()
                .partition(|(waiting, _)| *waiting == month);
            *waiters = rest;
            done
        };
        for (_, waiter) in done {
            waiter(inserted);
        }
    });
}

fn is_current(ui: &LibraryWindowUi, timeline: &Rc<Timeline>) -> bool {
    ui.timeline
        .borrow()
        .as_ref()
        .is_some_and(|current| Rc::ptr_eq(current, timeline))
}

fn insert_month(
    ui: &LibraryWindowUi,
    timeline: &Timeline,
    month: usize,
    assets: Vec<LibraryAsset>,
) -> Option<(u32, u32)> {
    if ui.ctx.library_state.lock().generation != timeline.generation {
        return None;
    }
    let added = assets.len() as u32;
    let position = timeline.index.borrow_mut().finish_load(month, added);
    ui.grid.canvas.invalidate_month(month);
    ui.grid.model.insert(&ui.ctx, position, &assets);
    ui.ctx
        .library_state
        .lock()
        .insert_assets_at(timeline.generation, position as usize, assets);
    Some((position, added))
}

/// Load the month past `position` when it isn't loaded yet; `then` runs once it lands.
pub(super) fn load_neighbor_then(
    ui: &Rc<LibraryWindowUi>,
    position: u32,
    forward: bool,
    then: impl FnOnce(Option<(u32, u32)>) + 'static,
) -> bool {
    let Some(timeline) = ui.timeline.borrow().clone() else {
        return false;
    };
    let Some(month) = timeline.index.borrow().unloaded_neighbor(position, forward) else {
        return false;
    };
    timeline.waiters.borrow_mut().push((month, Box::new(then)));
    load_month(ui, &timeline, month);
    true
}

pub(super) fn has_unloaded_neighbor(ui: &LibraryWindowUi, position: u32, forward: bool) -> bool {
    ui.timeline.borrow().as_ref().is_some_and(|timeline| {
        timeline
            .index
            .borrow()
            .unloaded_neighbor(position, forward)
            .is_some()
    })
}

/// Drop deleted or archived photos in place; false when Photos isn't a timeline.
pub(super) fn remove(ui: &Rc<LibraryWindowUi>, ids: &[String]) -> bool {
    let Some(timeline) = ui.timeline.borrow().clone() else {
        return false;
    };
    let positions = ui.grid.model.remove_ids(ids);
    {
        let mut index = timeline.index.borrow_mut();
        for &position in positions.iter().rev() {
            if let Some(month) = index.month_at(position) {
                ui.grid.canvas.invalidate_month(month);
            }
            index.remove_at(position);
        }
    }
    ui.ctx
        .library_state
        .lock()
        .assets
        .retain(|asset| !ids.contains(&asset.id));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_newest_photos_use_the_timeline() {
        assert!(eligible(
            &LibrarySource::AllAssets,
            &LibrarySortMode::NewestFirst
        ));
        assert!(!eligible(
            &LibrarySource::AllAssets,
            &LibrarySortMode::Filename
        ));
        assert!(!eligible(
            &LibrarySource::Favorites,
            &LibrarySortMode::NewestFirst
        ));
    }
}
