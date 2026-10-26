pub struct Month {
    pub key: String,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonthState {
    Unloaded,
    Loading,
    Loaded(u32),
}

impl MonthState {
    fn len(self) -> u32 {
        match self {
            MonthState::Loaded(len) => len,
            MonthState::Unloaded | MonthState::Loading => 0,
        }
    }
}

/// Months newest first, and which of them are in the grid model.
pub struct TimelineIndex {
    months: Vec<Month>,
    states: Vec<MonthState>,
}

impl TimelineIndex {
    pub fn new(months: Vec<Month>) -> Self {
        let states = vec![MonthState::Unloaded; months.len()];
        Self { months, states }
    }

    pub fn months(&self) -> &[Month] {
        &self.months
    }

    pub fn state(&self, month: usize) -> MonthState {
        self.states
            .get(month)
            .copied()
            .unwrap_or(MonthState::Unloaded)
    }

    pub fn offset(&self, month: usize) -> u32 {
        let end = month.min(self.states.len());
        self.states[..end].iter().map(|state| state.len()).sum()
    }

    pub fn begin_load(&mut self, month: usize) -> bool {
        match self.states.get_mut(month) {
            Some(state @ MonthState::Unloaded) => {
                *state = MonthState::Loading;
                true
            }
            _ => false,
        }
    }

    pub fn finish_load(&mut self, month: usize, len: u32) -> u32 {
        if let Some(state) = self.states.get_mut(month) {
            *state = MonthState::Loaded(len);
        }
        self.offset(month)
    }

    pub fn fail_load(&mut self, month: usize) {
        if let Some(state @ MonthState::Loading) = self.states.get_mut(month) {
            *state = MonthState::Unloaded;
        }
    }

    pub fn month_at(&self, position: u32) -> Option<usize> {
        let mut start = 0;
        for (month, state) in self.states.iter().enumerate() {
            start += state.len();
            if position < start {
                return Some(month);
            }
        }
        None
    }

    /// The month to load before stepping past `position`, when it sits at its month's edge.
    pub fn unloaded_neighbor(&self, position: u32, forward: bool) -> Option<usize> {
        let month = self.month_at(position)?;
        let start = self.offset(month);
        let at_edge = if forward {
            position + 1 == start + self.states[month].len()
        } else {
            position == start
        };
        if !at_edge {
            return None;
        }
        let mut neighbor = month;
        loop {
            neighbor = if forward {
                neighbor + 1
            } else {
                neighbor.checked_sub(1)?
            };
            match self.states.get(neighbor)? {
                MonthState::Loaded(0) => continue,
                MonthState::Loaded(_) => return None,
                MonthState::Unloaded | MonthState::Loading => return Some(neighbor),
            }
        }
    }

    pub fn remove_at(&mut self, position: u32) {
        let Some(month) = self.month_at(position) else {
            return;
        };
        if let MonthState::Loaded(len) = self.states[month] {
            self.states[month] = MonthState::Loaded(len.saturating_sub(1));
        }
        let count = &mut self.months[month].count;
        *count = count.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(counts: &[u32]) -> TimelineIndex {
        TimelineIndex::new(
            counts
                .iter()
                .enumerate()
                .map(|(i, &count)| Month {
                    key: format!("m{i}"),
                    count,
                })
                .collect(),
        )
    }

    #[test]
    fn offsets_count_only_loaded_months() {
        let mut idx = index(&[3, 5, 2]);
        assert!(idx.begin_load(2));
        assert_eq!(idx.finish_load(2, 2), 0);
        assert!(idx.begin_load(0));
        assert_eq!(idx.finish_load(0, 3), 0);
        assert_eq!(idx.offset(2), 3);
        assert_eq!(idx.month_at(3), Some(2));
        assert_eq!(idx.month_at(5), None);
    }

    #[test]
    fn loading_months_are_not_requested_twice() {
        let mut idx = index(&[3]);
        assert!(idx.begin_load(0));
        assert!(!idx.begin_load(0));
        idx.fail_load(0);
        assert!(idx.begin_load(0));
    }

    #[test]
    fn finish_load_with_zero_settles() {
        let mut idx = index(&[4, 1]);
        idx.begin_load(0);
        idx.finish_load(0, 0);
        assert_eq!(idx.state(0), MonthState::Loaded(0));
        assert!(!idx.begin_load(0));
    }

    #[test]
    fn unloaded_neighbor_at_month_edges() {
        let mut idx = index(&[2, 3, 2]);
        idx.begin_load(1);
        idx.finish_load(1, 3);
        assert_eq!(idx.unloaded_neighbor(2, true), Some(2));
        assert_eq!(idx.unloaded_neighbor(0, false), Some(0));
        assert_eq!(idx.unloaded_neighbor(1, true), None);
    }

    #[test]
    fn remove_at_shrinks_the_owning_month() {
        let mut idx = index(&[2, 3]);
        idx.begin_load(0);
        idx.finish_load(0, 2);
        idx.begin_load(1);
        idx.finish_load(1, 3);
        idx.remove_at(1);
        assert_eq!(idx.state(0), MonthState::Loaded(1));
        assert_eq!(idx.months()[0].count, 1);
        assert_eq!(idx.offset(1), 1);
    }
}
