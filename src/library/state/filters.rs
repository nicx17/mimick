//! Filters the Utilities keep in their source, so a change reloads like any other view.

/// Large Files: which kind of file, and the smallest size to list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LargeFilesFilter {
    pub kind: MediaKind,
    pub min_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaKind {
    #[default]
    All,
    Photos,
    Videos,
}

impl MediaKind {
    /// Immich's asset `type`, or `None` for both.
    pub fn asset_type(self) -> Option<&'static str> {
        match self {
            MediaKind::All => None,
            MediaKind::Photos => Some("IMAGE"),
            MediaKind::Videos => Some("VIDEO"),
        }
    }
}

/// Days a list is limited to, as `YYYY-MM-DD` in local time; `None` leaves that end open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DayRange {
    pub from: Option<String>,
    pub until: Option<String>,
}

/// Unusual Dates: photos dated after today, or before digital cameras were common.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OddDates {
    #[default]
    Future,
    Early,
}

/// Earlier than this is too early for most libraries: a camera clock reset to 1970 or 1980.
pub const EARLY_YEAR: i32 = 1990;
