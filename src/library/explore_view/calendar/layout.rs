//! Where each day of the calendar goes, how dark its square is, and where month names fit.

use chrono::{Datelike, Duration, NaiveDate};

/// A day's square on a phone, where the year scrolls sideways.
pub(super) const MIN_CELL: f32 = 10.0;
/// The largest square, so a very wide window doesn't get giant ones.
pub(super) const MAX_CELL: f32 = 20.0;
pub(super) const GAP: f32 = 3.0;

/// Week column and weekday row (Monday first) of the `index`th day after `first`.
pub(super) fn cell_of(first: NaiveDate, index: usize) -> (usize, usize) {
    let offset = first.weekday().num_days_from_monday() as usize + index;
    (offset / 7, offset % 7)
}

/// The day at a column and row, if the grid has one there.
pub(super) fn index_at(first: NaiveDate, days: usize, (col, row): (usize, usize)) -> Option<usize> {
    if row >= 7 {
        return None;
    }
    let lead = first.weekday().num_days_from_monday() as usize;
    let index = (col * 7 + row).checked_sub(lead)?;
    (index < days).then_some(index)
}

/// How many week columns `days` days starting on `first` take.
pub(super) fn columns(first: NaiveDate, days: usize) -> usize {
    days.checked_sub(1)
        .map_or(0, |last| cell_of(first, last).0 + 1)
}

/// 0 for no photos, then 1 to 4 by how close the day is to the busiest one.
pub(super) fn level(count: u32, max: u32) -> usize {
    if count == 0 || max == 0 {
        return 0;
    }
    (f64::from(count) / f64::from(max) * 4.0)
        .ceil()
        .clamp(1.0, 4.0) as usize
}

/// The distance from one column to the next when `columns` share `width` pixels.
pub(super) fn step_for(width: f32, columns: usize) -> f32 {
    (width / columns.max(1) as f32).clamp(MIN_CELL + GAP, MAX_CELL + GAP)
}

/// The column where each month begins, with its first day. A month starting fewer than
/// `min_gap` columns after the previous label replaces it, so a short first month gives way.
pub(super) fn month_starts(
    first: NaiveDate,
    days: usize,
    min_gap: usize,
) -> Vec<(usize, NaiveDate)> {
    let mut starts: Vec<(usize, NaiveDate)> = Vec::new();
    for index in 0..days {
        let day = first + Duration::days(index as i64);
        if index != 0 && day.day() != 1 {
            continue;
        }
        let col = cell_of(first, index).0;
        match starts.last_mut() {
            Some(last) if col - last.0 < min_gap => *last = (col, day),
            _ => starts.push((col, day)),
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn days_fill_week_columns_from_monday() {
        // 5 Nov 2026 is a Thursday, so it sits in the first column's fourth row.
        let first = day(2026, 11, 5);
        assert_eq!(cell_of(first, 0), (0, 3));
        assert_eq!(cell_of(first, 4), (1, 0));
        assert_eq!(index_at(first, 10, (1, 0)), Some(4));
        assert_eq!(index_at(first, 10, (0, 0)), None);
        assert_eq!(index_at(first, 3, (1, 0)), None);
        assert_eq!(index_at(first, 10, (0, 7)), None);
        assert_eq!(columns(first, 10), 2);
        assert_eq!(columns(first, 0), 0);
    }

    #[test]
    fn busier_days_get_higher_levels() {
        assert_eq!(level(0, 10), 0);
        assert_eq!(level(1, 10), 1);
        assert_eq!(level(10, 10), 4);
        assert_eq!(level(3, 0), 0);
    }

    #[test]
    fn squares_grow_with_the_window_up_to_a_limit() {
        assert_eq!(step_for(300.0, 53), MIN_CELL + GAP);
        assert_eq!(step_for(53.0 * 15.0, 53), 15.0);
        assert_eq!(step_for(5000.0, 53), MAX_CELL + GAP);
    }

    #[test]
    fn month_names_sit_where_months_begin() {
        // From Wednesday 28 Oct, 1 Nov (a Sunday) is in the same column: November's name wins.
        let starts = month_starts(day(2026, 10, 28), 70, 3);
        let months: Vec<u32> = starts.iter().map(|(_, d)| d.month()).collect();
        assert_eq!(months, [11, 12, 1]);
        assert_eq!(starts[0].0, 0);
        assert_eq!(starts[1].0, 5);
        // With room to spare, the first month keeps its label.
        let roomy = month_starts(day(2026, 10, 1), 70, 3);
        assert_eq!(roomy[0], (0, day(2026, 10, 1)));
    }
}
