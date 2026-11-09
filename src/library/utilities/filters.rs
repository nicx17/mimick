//! What each utility's filter asks Immich for.

use chrono::{DateTime, Duration, Utc};

use crate::api_client::{MetadataSearchFilters, TakenRange};
use crate::library::day_row::day_start;
use crate::library::state::{DayRange, EARLY_YEAR, LibrarySource, OddDates};

/// Minimum sizes Large Files offers, in megabytes; 0 lists every size.
pub(super) const MIN_SIZES_MB: [u64; 6] = [0, 10, 50, 100, 500, 1000];

/// The search behind Unusual Dates; `None` for any other source.
pub(in crate::library) fn search_filters(source: &LibrarySource) -> Option<MetadataSearchFilters> {
    let LibrarySource::UnusualDates { which } = source else {
        return None;
    };
    Some(odd_date_filters(*which, Utc::now()))
}

/// A day of slack, so a camera a few hours ahead doesn't count as the future.
fn odd_date_filters(which: OddDates, now: DateTime<Utc>) -> MetadataSearchFilters {
    match which {
        OddDates::Future => MetadataSearchFilters {
            taken_after: Some(iso(now + Duration::days(1))),
            ..MetadataSearchFilters::default()
        },
        OddDates::Early => MetadataSearchFilters {
            taken_before: Some(format!("{EARLY_YEAR}-01-01T00:00:00.000Z")),
            ..MetadataSearchFilters::default()
        },
    }
}

/// Missing Location's days as Immich's bounds: from the first day's start to the last day's end.
pub(in crate::library) fn taken_range(range: &DayRange) -> TakenRange {
    let start = |day: &Option<String>| day_start(day.as_deref());
    TakenRange {
        after: start(&range.from).map(iso),
        before: start(&range.until).map(|day| iso(day + Duration::days(1))),
    }
}

fn iso(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn future_dates_allow_a_day_of_clock_drift() {
        let now = DateTime::parse_from_rfc3339("2026-11-09T10:00:00Z").unwrap();
        let future = odd_date_filters(OddDates::Future, now.with_timezone(&Utc));
        assert_eq!(
            future.taken_after.as_deref(),
            Some("2026-11-10T10:00:00.000Z")
        );
        let early = odd_date_filters(OddDates::Early, now.with_timezone(&Utc));
        assert_eq!(
            early.taken_before.as_deref(),
            Some("1990-01-01T00:00:00.000Z")
        );
    }

    #[test]
    fn an_open_range_sends_no_dates() {
        assert_eq!(taken_range(&DayRange::default()), TakenRange::default());
        let until = DayRange {
            from: None,
            until: Some("2024-06-30".into()),
        };
        assert!(taken_range(&until).before.is_some());
        assert!(taken_range(&until).after.is_none());
    }

    #[test]
    fn other_sources_have_no_utility_search() {
        assert!(search_filters(&LibrarySource::Favorites).is_none());
        let unusual = LibrarySource::UnusualDates {
            which: OddDates::Early,
        };
        assert!(search_filters(&unusual).is_some());
    }
}
