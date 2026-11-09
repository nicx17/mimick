//! Photos, albums, and other collections as a timeline whose full height is known up front:
//! month (or page) bookkeeping, date labels, layout, and the Photos scrubber.

pub mod dates;
pub mod index;
pub mod layout;
mod load_all;
pub mod marks;
pub mod nav;
mod scope;
pub mod scrubber;

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use super::LibraryWindowUi;
use super::asset_object::AssetObject;
use super::state::{LibrarySortMode, LibrarySource};
use crate::api_client::LibraryAsset;
use index::{Month, TimelineIndex};
use layout::section_at;
pub(super) use load_all::load_all;
use scope::Plan;

/// Called once a month finishes loading: `(insert position, photos added)`, or `None` on failure.
type Waiter = Box<dyn FnOnce(Option<(u32, u32)>)>;

pub(super) struct Timeline {
    index: Rc<RefCell<TimelineIndex>>,
    generation: u64,
    plan: Plan,
    waiters: RefCell<Vec<(usize, Waiter)>>,
}

pub(super) fn eligible(source: &LibrarySource, sort: &LibrarySortMode) -> bool {
    scope::plan(source, sort).is_some()
}

pub(super) fn is_active(ui: &LibraryWindowUi) -> bool {
    ui.interaction.timeline.borrow().is_some()
}

/// Only Photos (and a partner's) shows a stack as its cover; elsewhere every photo shows.
pub(super) fn groups_stacks(ui: &LibraryWindowUi) -> bool {
    let timeline = ui.interaction.timeline.borrow();
    timeline.as_ref().is_some_and(|t| t.plan.is_photos())
}

/// Scroll Photos to the next or previous day, month, or year; `false` when it isn't shown.
pub(super) fn jump(ui: &LibraryWindowUi, step: nav::Step, forward: bool) -> bool {
    let canvas = &ui.grid.canvas;
    if !is_active(ui) || !canvas.is_mapped() {
        return false;
    }
    let (top, sections) = (canvas.scroll_top(), canvas.timeline_sections());
    let target = match step {
        nav::Step::Day => nav::day_target(
            &row_days(ui),
            &canvas.heading_tops(),
            &sections,
            top,
            forward,
        )
        .or_else(|| nav::month_target(&sections, top, forward)),
        nav::Step::Month => nav::month_target(&sections, top, forward),
        nav::Step::Year => nav::year_target(&sections, &month_years(ui), top, forward),
    };
    if let Some(y) = target {
        canvas.scroll_to(y);
    }
    true
}

fn row_days(ui: &LibraryWindowUi) -> Vec<(f32, Option<chrono::NaiveDate>)> {
    let model = &ui.grid.model;
    let day_of = |position: u32| {
        let item = model.item(position).and_downcast::<AssetObject>()?;
        dates::local_day(&item.local_date_time())
    };
    ui.grid
        .canvas
        .row_starts()
        .into_iter()
        .map(|(y, first)| (y, first.and_then(day_of)))
        .collect()
}

fn month_years(ui: &LibraryWindowUi) -> Vec<Option<i32>> {
    let timeline = ui.interaction.timeline.borrow();
    let Some(timeline) = timeline.as_ref() else {
        return Vec::new();
    };
    let index = timeline.index.borrow();
    index
        .months()
        .iter()
        .map(|month| dates::month_year(&month.key))
        .collect()
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

/// Count every month (or page) and show the source as a timeline; page by page if that fails.
pub(super) fn open(
    ui: &Rc<LibraryWindowUi>,
    request: (u64, LibrarySource, u32),
    keep: Option<(String, f32)>,
) {
    let sort = ui.ctx.library_state.lock().sort_mode.clone();
    let Some(plan) = scope::plan(&request.1, &sort) else {
        return super::load_pages(ui.clone(), request, false);
    };
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match scope::sections(&ui.ctx.api_client, &plan).await {
            Ok(months) => show(&ui, request.0, plan, months, keep),
            Err(err) => {
                log::warn!("Timeline unavailable, loading page by page: {err}");
                super::load_pages(ui, request, false);
            }
        }
    });
}

fn show(
    ui: &Rc<LibraryWindowUi>,
    generation: u64,
    plan: Plan,
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
    let total: u32 = months.iter().map(|month| month.count).sum();
    let (dated, photos) = (plan.dated(), plan.is_photos());
    let index = Rc::new(RefCell::new(TimelineIndex::new(months)));
    ui.interaction.timeline.replace(Some(Rc::new(Timeline {
        index: index.clone(),
        generation,
        plan,
        waiters: RefCell::default(),
    })));
    ui.grid.model.reset_with_objects(Vec::new());
    ui.grid.canvas.set_timeline(Some(index.clone()), dated);
    show_scrollbar(ui, photos.then_some(index));
    super::sync_content_state(ui);
    if !dated {
        super::source_header::set_loaded_count(ui, total as usize);
    }
    if let Some((asset_id, month)) = ui.interaction.timeline_reveal.take() {
        reveal(ui, asset_id, &month);
    } else if let Some((month, offset)) = anchor {
        scroll_when_laid_out(ui, month, offset);
    }
}

/// Scroll to the month starting `YYYY-MM`, load it, then scroll to `asset_id` and ring it.
fn reveal(ui: &Rc<LibraryWindowUi>, asset_id: String, month_prefix: &str) {
    let Some(timeline) = ui.interaction.timeline.borrow().clone() else {
        return;
    };
    let found = timeline
        .index
        .borrow()
        .months()
        .iter()
        .position(|month| month.key.get(..7) == Some(month_prefix));
    let Some(month) = found else {
        reveal_failed(ui);
        return;
    };
    scroll_when_laid_out(ui, month, 0.0);
    if matches!(
        timeline.index.borrow().state(month),
        index::MonthState::Loaded(_)
    ) {
        focus_asset(ui, &asset_id);
        return;
    }
    let weak = Rc::downgrade(ui);
    let waiter: Waiter = Box::new(move |_| {
        if let Some(ui) = weak.upgrade() {
            focus_asset(&ui, &asset_id);
        }
    });
    timeline.waiters.borrow_mut().push((month, waiter));
    load_month(ui, &timeline, month);
}

fn focus_asset(ui: &LibraryWindowUi, asset_id: &str) {
    let model = &ui.grid.model;
    let position = (0..model.n_items()).find(|&position| {
        model
            .item(position)
            .and_downcast::<AssetObject>()
            .is_some_and(|item| item.id() == asset_id)
    });
    match position {
        Some(position) => ui.grid.canvas.reveal_tile(position),
        None => reveal_failed(ui),
    }
}

fn reveal_failed(ui: &LibraryWindowUi) {
    ui.toasts
        .add_toast(libadwaita::Toast::new(&gettextrs::gettext(
            "This photo isn't in your Photos timeline",
        )));
}

/// Photos scrolls by its month scrubber; everything else by the plain scrollbar.
fn show_scrollbar(ui: &LibraryWindowUi, scrubber: Option<Rc<RefCell<TimelineIndex>>>) {
    let policy = if scrubber.is_some() {
        gtk::PolicyType::External
    } else {
        gtk::PolicyType::Automatic
    };
    ui.scrubber.set_index(scrubber);
    ui.grid.scrolled.set_vscrollbar_policy(policy);
}

pub(super) fn close(ui: &LibraryWindowUi) {
    ui.interaction.timeline_reveal.take();
    if ui.interaction.timeline.take().is_some() {
        ui.grid.canvas.set_timeline(None, true);
        show_scrollbar(ui, None);
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
    let timeline = ui.interaction.timeline.borrow().clone()?;
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
    let Some(timeline) = ui.interaction.timeline.borrow().clone() else {
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
        let result = scope::fetch(&ui, &timeline.plan, month, &key).await;
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
    ui.interaction
        .timeline
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
    let Some(timeline) = ui.interaction.timeline.borrow().clone() else {
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
    ui.interaction
        .timeline
        .borrow()
        .as_ref()
        .is_some_and(|timeline| {
            timeline
                .index
                .borrow()
                .unloaded_neighbor(position, forward)
                .is_some()
        })
}

/// Drop deleted or archived photos in place; false when Photos isn't a timeline.
/// Drops the cached layout of the month holding `position`, after its photo was swapped.
pub(super) fn forget_layout_at(ui: &LibraryWindowUi, position: u32) {
    let Some(timeline) = ui.interaction.timeline.borrow().clone() else {
        return;
    };
    if let Some(month) = timeline.index.borrow().month_at(position) {
        ui.grid.canvas.invalidate_month(month);
    }
}

pub(super) fn remove(ui: &Rc<LibraryWindowUi>, ids: &[String]) -> bool {
    let Some(timeline) = ui.interaction.timeline.borrow().clone() else {
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
    // Later pages now start further on, so count again; an open lightbox keeps its place.
    if !timeline.plan.dated() && ui.interaction.lightbox.borrow().is_none() {
        reload(ui);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_sorted_collections_use_the_timeline() {
        assert!(eligible(
            &LibrarySource::AllAssets,
            &LibrarySortMode::NewestFirst
        ));
        assert!(!eligible(
            &LibrarySource::AllAssets,
            &LibrarySortMode::Filename
        ));
        assert!(eligible(
            &LibrarySource::Favorites,
            &LibrarySortMode::OldestFirst
        ));
    }

    #[test]
    fn a_partners_photos_use_the_timeline() {
        let partner = LibrarySource::Partner {
            id: "u2".into(),
            name: "Ana".into(),
        };
        assert!(eligible(&partner, &LibrarySortMode::NewestFirst));
    }
}
