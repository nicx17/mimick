use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use crate::api_client::ThumbnailSize;

const MAX_LOADS: usize = 8;
const FLING_VIEWPORTS_PER_SEC: f64 = 3.0;
pub(super) const SETTLE_DELAY: Duration = Duration::from_millis(150);

pub(super) type CancelFlag = Rc<Cell<bool>>;

pub(super) struct LoadRequest {
    pub(super) priority: f32,
    pub(super) asset_id: String,
    pub(super) asset_index: u32,
    pub(super) bucket: ThumbnailSize,
    pub(super) local_path: String,
    pub(super) is_local: bool,
}

pub(super) struct LoadScheduler {
    in_flight: HashMap<String, CancelFlag>,
    limit: usize,
}

impl Default for LoadScheduler {
    fn default() -> Self {
        Self::with_limit(MAX_LOADS)
    }
}

impl LoadScheduler {
    fn with_limit(limit: usize) -> Self {
        Self {
            in_flight: HashMap::new(),
            limit,
        }
    }

    pub(super) fn plan(&mut self, mut wanted: Vec<LoadRequest>) -> Vec<(LoadRequest, CancelFlag)> {
        let keep: HashSet<&str> = wanted.iter().map(|r| r.asset_id.as_str()).collect();
        for (id, cancel) in &self.in_flight {
            cancel.set(!keep.contains(id.as_str()));
        }
        let active = self
            .in_flight
            .values()
            .filter(|cancel| !cancel.get())
            .count();
        wanted.retain(|r| !self.in_flight.contains_key(&r.asset_id));
        wanted.sort_by(|a, b| a.priority.total_cmp(&b.priority));
        wanted.truncate(self.limit.saturating_sub(active));
        wanted
            .into_iter()
            .map(|request| {
                let cancel = CancelFlag::default();
                self.in_flight
                    .insert(request.asset_id.clone(), cancel.clone());
                (request, cancel)
            })
            .collect()
    }

    pub(super) fn finish(&mut self, asset_id: &str) {
        self.in_flight.remove(asset_id);
    }

    pub(super) fn in_flight(&self) -> usize {
        self.in_flight.len()
    }
}

pub(super) fn is_fling(distance: f64, elapsed_us: i64, viewport_h: f64) -> bool {
    let seconds = elapsed_us.max(1) as f64 / 1_000_000.0;
    distance.abs() / seconds > viewport_h * FLING_VIEWPORTS_PER_SEC
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str, priority: f32) -> LoadRequest {
        LoadRequest {
            priority,
            asset_id: id.to_string(),
            asset_index: 0,
            bucket: ThumbnailSize::Thumbnail,
            local_path: String::new(),
            is_local: false,
        }
    }

    fn ids(started: &[(LoadRequest, CancelFlag)]) -> Vec<&str> {
        started.iter().map(|(r, _)| r.asset_id.as_str()).collect()
    }

    #[test]
    fn the_nearest_tiles_load_first_up_to_the_limit() {
        let mut scheduler = LoadScheduler::with_limit(2);
        let started = scheduler.plan(vec![
            request("far", 30.0),
            request("near", 10.0),
            request("mid", 20.0),
        ]);
        assert_eq!(ids(&started), ["near", "mid"]);
    }

    #[test]
    fn loads_that_scroll_away_are_cancelled() {
        let mut scheduler = LoadScheduler::with_limit(2);
        let first = scheduler.plan(vec![request("a", 0.0)]);
        scheduler.plan(vec![request("b", 0.0)]);
        assert!(first[0].1.get());
    }

    #[test]
    fn a_load_wanted_again_resumes_instead_of_restarting() {
        let mut scheduler = LoadScheduler::with_limit(2);
        let first = scheduler.plan(vec![request("a", 0.0)]);
        scheduler.plan(Vec::new());
        assert!(scheduler.plan(vec![request("a", 0.0)]).is_empty());
        assert!(!first[0].1.get());
    }

    #[test]
    fn cancelled_loads_give_up_their_slot() {
        let mut scheduler = LoadScheduler::with_limit(1);
        scheduler.plan(vec![request("a", 0.0)]);
        assert_eq!(ids(&scheduler.plan(vec![request("b", 0.0)])), ["b"]);
    }

    #[test]
    fn finished_loads_free_their_slot() {
        let mut scheduler = LoadScheduler::with_limit(1);
        let both = || vec![request("a", 0.0), request("b", 1.0)];
        scheduler.plan(both());
        assert!(scheduler.plan(both()).is_empty());
        scheduler.finish("a");
        assert_eq!(ids(&scheduler.plan(vec![request("b", 1.0)])), ["b"]);
    }

    #[test]
    fn only_fast_scrolling_counts_as_a_fling() {
        assert!(is_fling(800.0, 16_000, 800.0));
        assert!(!is_fling(100.0, 150_000, 800.0));
        assert!(!is_fling(100.0, 2_000_000, 800.0));
        assert!(!is_fling(0.0, 0, 800.0));
    }
}
