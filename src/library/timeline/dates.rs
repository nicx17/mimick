use std::ops::Range;

use chrono::{Datelike, NaiveDate};

/// The photographer's local date; Immich writes local time with a `Z` suffix.
pub fn local_day(local_date_time: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(local_date_time.get(..10)?, "%Y-%m-%d").ok()
}

pub fn day_heading(day: NaiveDate, today: NaiveDate) -> String {
    let format = if day.year() == today.year() {
        "%a, %b %-d"
    } else {
        "%a, %b %-d, %Y"
    };
    day.format(format).to_string()
}

pub fn month_heading(month_key: &str) -> Option<String> {
    Some(local_day(month_key)?.format("%B %Y").to_string())
}

pub fn month_short(month_key: &str) -> Option<String> {
    Some(local_day(month_key)?.format("%b %Y").to_string())
}

pub fn month_year(month_key: &str) -> Option<i32> {
    Some(local_day(month_key)?.year())
}

pub fn day_groups(days: &[Option<NaiveDate>]) -> Vec<Range<usize>> {
    let mut groups: Vec<Range<usize>> = Vec::new();
    for (i, day) in days.iter().enumerate() {
        match groups.last_mut() {
            Some(group) if days[group.start] == *day => group.end = i + 1,
            _ => groups.push(i..i + 1),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn local_day_ignores_the_zone() {
        assert_eq!(local_day("2026-03-31T23:30:00.000Z"), Some(d("2026-03-31")));
        assert_eq!(local_day("nope"), None);
    }

    #[test]
    fn day_headings_add_the_year_only_for_other_years() {
        assert_eq!(day_heading(d("2026-10-26"), d("2026-10-26")), "Mon, Oct 26");
        assert_eq!(
            day_heading(d("2025-03-15"), d("2026-10-26")),
            "Sat, Mar 15, 2025"
        );
    }

    #[test]
    fn month_labels() {
        assert_eq!(month_heading("2026-10-01").as_deref(), Some("October 2026"));
        assert_eq!(month_short("2026-03-01").as_deref(), Some("Mar 2026"));
        assert_eq!(month_year("2019-07-01"), Some(2019));
    }

    #[test]
    fn day_groups_split_runs() {
        let days = [
            Some(d("2026-10-26")),
            Some(d("2026-10-26")),
            Some(d("2026-10-25")),
            None,
        ];
        assert_eq!(day_groups(&days), [0..2, 2..3, 3..4]);
    }
}
