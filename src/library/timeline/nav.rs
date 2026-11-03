//! Keyboard jumps through the Photos timeline: the next or previous day, month, or year.

use chrono::NaiveDate;

use super::layout::LaidSection;

/// Slack so a view resting on a boundary counts as being at it.
const EDGE: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Day,
    Month,
    Year,
}

pub fn month_target(sections: &[LaidSection], top: f32, forward: bool) -> Option<f32> {
    let tops = section_tops(sections);
    let keys: Vec<usize> = (0..sections.len()).collect();
    run_target(&tops, &keys, top, forward).map(|i| tops[i])
}

/// `years` holds each section's year, in the same order.
pub fn year_target(
    sections: &[LaidSection],
    years: &[Option<i32>],
    top: f32,
    forward: bool,
) -> Option<f32> {
    if years.len() != sections.len() {
        return None;
    }
    let tops = section_tops(sections);
    run_target(&tops, years, top, forward).map(|i| tops[i])
}

/// The next or previous day among laid `rows`, with its heading in view; `None` past them.
pub fn day_target(
    rows: &[(f32, Option<NaiveDate>)],
    headings: &[f32],
    sections: &[LaidSection],
    top: f32,
    forward: bool,
) -> Option<f32> {
    let tops: Vec<f32> = rows.iter().map(|row| row.0).collect();
    let days: Vec<Option<NaiveDate>> = rows.iter().map(|row| row.1).collect();
    let index = run_target(&tops, &days, top, forward)?;
    let above = index.checked_sub(1).map(|i| tops[i]);
    let target = headings
        .iter()
        .copied()
        .filter(|&heading| heading <= tops[index] && above.is_none_or(|row| heading > row))
        .fold(tops[index], f32::min);
    Some(stop_at_unloaded(sections, top, target))
}

fn section_tops(sections: &[LaidSection]) -> Vec<f32> {
    sections.iter().map(|section| section.y).collect()
}

/// Where the next (`forward`) or previous run of equal `keys` starts, seen from `top`.
fn run_target<K: PartialEq>(tops: &[f32], keys: &[K], top: f32, forward: bool) -> Option<usize> {
    let current = tops.iter().rposition(|&y| y <= top + EDGE).unwrap_or(0);
    let key = keys.get(current)?;
    if forward {
        return (current + 1..keys.len()).find(|&i| keys[i] != *key);
    }
    let start = run_start(keys, current);
    if top > tops[start] + EDGE {
        Some(start)
    } else {
        start
            .checked_sub(1)
            .map(|previous| run_start(keys, previous))
    }
}

fn run_start<K: PartialEq>(keys: &[K], index: usize) -> usize {
    let key = &keys[index];
    keys[..index]
        .iter()
        .rposition(|other| other != key)
        .map_or(0, |i| i + 1)
}

/// A day jump stops at the top of a month that hasn't loaded rather than skipping it.
fn stop_at_unloaded(sections: &[LaidSection], top: f32, target: f32) -> f32 {
    let unloaded = sections.iter().filter(|section| !section.loaded);
    if target > top {
        unloaded
            .map(|section| section.y)
            .filter(|&y| y > top + EDGE && y < target - EDGE)
            .fold(target, f32::min)
    } else {
        unloaded
            .filter(|s| s.y + s.h <= top + EDGE && s.y + s.h > target + EDGE)
            .map(|section| section.y)
            .fold(target, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(y: f32, h: f32, loaded: bool) -> LaidSection {
        LaidSection { y, h, loaded }
    }

    fn day(d: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(2024, 5, d)
    }

    #[test]
    fn months_step_to_neighbours_and_back_to_their_start() {
        let sections = [
            section(0.0, 100.0, true),
            section(100.0, 100.0, true),
            section(200.0, 100.0, false),
        ];
        assert_eq!(month_target(&sections, 0.0, true), Some(100.0));
        assert_eq!(month_target(&sections, 150.0, true), Some(200.0));
        assert_eq!(month_target(&sections, 250.0, true), None);
        assert_eq!(month_target(&sections, 150.0, false), Some(100.0));
        assert_eq!(month_target(&sections, 100.0, false), Some(0.0));
        assert_eq!(month_target(&sections, 0.0, false), None);
    }

    #[test]
    fn years_skip_to_the_first_month_of_another_year() {
        let sections: Vec<LaidSection> = (0..5)
            .map(|i| section(i as f32 * 100.0, 100.0, true))
            .collect();
        let years = [Some(2024), Some(2024), Some(2023), Some(2023), Some(2022)];
        assert_eq!(year_target(&sections, &years, 50.0, true), Some(200.0));
        assert_eq!(year_target(&sections, &years, 350.0, false), Some(200.0));
        assert_eq!(year_target(&sections, &years, 200.0, false), Some(0.0));
        assert_eq!(year_target(&sections, &years[..4], 0.0, true), None);
    }

    #[test]
    fn days_land_on_their_heading() {
        let rows = [
            (20.0, day(9)),
            (120.0, day(9)),
            (260.0, day(8)),
            (360.0, day(7)),
        ];
        let headings = [0.0, 230.0, 330.0];
        let sections = [section(0.0, 500.0, true)];
        assert_eq!(
            day_target(&rows, &headings, &sections, 0.0, true),
            Some(230.0)
        );
        assert_eq!(
            day_target(&rows, &headings, &sections, 260.0, true),
            Some(330.0)
        );
        assert_eq!(
            day_target(&rows, &headings, &sections, 300.0, false),
            Some(230.0)
        );
        assert_eq!(
            day_target(&rows, &headings, &sections, 260.0, false),
            Some(0.0)
        );
        assert_eq!(day_target(&rows, &[], &sections, 400.0, true), None);
        assert_eq!(day_target(&[], &[], &sections, 0.0, true), None);
    }

    #[test]
    fn days_stop_at_a_month_that_has_not_loaded() {
        let rows = [(0.0, day(9)), (600.0, day(2))];
        let sections = [
            section(0.0, 200.0, true),
            section(200.0, 300.0, false),
            section(500.0, 200.0, true),
        ];
        assert_eq!(day_target(&rows, &[], &sections, 0.0, true), Some(200.0));
        assert_eq!(day_target(&rows, &[], &sections, 600.0, false), Some(200.0));
    }
}
