//! Select All on Photos: load every month still missing from the grid, a few at a time.

use std::rc::Rc;

use super::{LibraryWindowUi, Timeline, Waiter, index, load_month};

/// Months fetched at once, so a big library doesn't open a connection per month.
const PARALLEL_MONTHS: usize = 4;

/// Load every month not in the grid yet, a few at a time. False if one failed or Photos closed.
pub(in crate::library) async fn load_all(ui: &Rc<LibraryWindowUi>) -> bool {
    let Some(timeline) = ui.interaction.timeline.borrow().clone() else {
        return true;
    };
    let missing = timeline.index.borrow().not_loaded();
    // Held weakly while waiting: a replaced timeline must drop its waiters to end the wait.
    let timeline = Rc::downgrade(&timeline);
    for months in missing.chunks(PARALLEL_MONTHS) {
        let Some(current) = timeline.upgrade() else {
            return false;
        };
        let landings: Vec<_> = months
            .iter()
            .filter_map(|&month| month_landing(ui, &current, month))
            .collect();
        drop(current);
        let landed = futures_util::future::join_all(landings).await;
        // An error means the sender was dropped: the timeline went before the month came.
        if !landed.into_iter().all(|landed| landed.unwrap_or(false)) {
            return false;
        }
    }
    true
}

/// Start loading `month`, or join the load under way; `None` when it's already in the grid.
fn month_landing(
    ui: &Rc<LibraryWindowUi>,
    timeline: &Rc<Timeline>,
    month: usize,
) -> Option<tokio::sync::oneshot::Receiver<bool>> {
    // Scrolling may have loaded it while earlier months were fetched; its waiters already ran.
    if matches!(
        timeline.index.borrow().state(month),
        index::MonthState::Loaded(_)
    ) {
        return None;
    }
    let (done, landed) = tokio::sync::oneshot::channel();
    let waiter: Waiter = Box::new(move |inserted| {
        done.send(inserted.is_some()).ok();
    });
    timeline.waiters.borrow_mut().push((month, waiter));
    load_month(ui, timeline, month);
    Some(landed)
}
