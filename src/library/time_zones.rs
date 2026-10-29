//! IANA time zones for the date dialogs, read from the system's tzdata.

use chrono::{Datelike, FixedOffset, NaiveDateTime, TimeZone, Timelike};
use gtk::prelude::*;
use libadwaita::prelude::*;

use super::asset_edit::DATE_FORMAT;

const ZONE_TABLE: &str = "/usr/share/zoneinfo/zone1970.tab";

/// Canonical zone names, sorted, always including UTC and this computer's zone.
pub(super) fn all() -> Vec<String> {
    let mut zones: Vec<String> = std::fs::read_to_string(ZONE_TABLE)
        .map(|table| zones_from_table(&table))
        .unwrap_or_default();
    zones.extend(crate::api_client::local_timezone_name());
    zones.push("UTC".to_string());
    zones.sort();
    zones.dedup();
    zones
}

fn zones_from_table(table: &str) -> Vec<String> {
    table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split('\t').nth(2))
        .map(str::to_string)
        .collect()
}

/// `zone`'s UTC offset at the local time `date_text`, so daylight saving is respected.
pub(super) fn offset_seconds(zone: &str, date_text: &str) -> Option<i32> {
    let naive = NaiveDateTime::parse_from_str(date_text.trim(), DATE_FORMAT).ok()?;
    let tz = glib::TimeZone::from_identifier(Some(zone))?;
    let local = glib::DateTime::new(
        &tz,
        naive.year(),
        naive.month() as i32,
        naive.day() as i32,
        naive.hour() as i32,
        naive.minute() as i32,
        f64::from(naive.second()),
    )
    .ok()?;
    i32::try_from(local.utc_offset().as_seconds()).ok()
}

pub(super) fn zoned_rfc3339(date_text: &str, zone: &str) -> Option<String> {
    let naive = NaiveDateTime::parse_from_str(date_text.trim(), DATE_FORMAT).ok()?;
    let offset = FixedOffset::east_opt(offset_seconds(zone, date_text)?)?;
    Some(offset.from_local_datetime(&naive).single()?.to_rfc3339())
}

/// `iso` (a UTC instant from Immich) as wall-clock text in `zone`.
pub(super) fn wall_time_in(iso: &str, zone: &str) -> Option<String> {
    let instant = chrono::DateTime::parse_from_rfc3339(iso).ok()?;
    let tz = glib::TimeZone::from_identifier(Some(zone))?;
    let local = glib::DateTime::from_unix_utc(instant.timestamp())
        .ok()?
        .to_timezone(&tz)
        .ok()?;
    Some(local.format("%Y-%m-%d %H:%M:%S").ok()?.to_string())
}

/// One label per zone, with the offset each zone has at the local time `at`.
pub(super) fn zone_labels(zones: &[String], at: &str) -> Vec<String> {
    zones
        .iter()
        .map(|zone| zone_label(zone, offset_seconds(zone, at).unwrap_or(0)))
        .collect()
}

/// Searchable "Time zone" row over `labels`, starting at `selected`.
pub(super) fn zone_row<'a>(
    labels: impl Iterator<Item = &'a str>,
    selected: usize,
) -> libadwaita::ComboRow {
    let model = gtk::StringList::new(&labels.collect::<Vec<_>>());
    let row = libadwaita::ComboRow::builder()
        .title("Time zone")
        .model(&model)
        .enable_search(true)
        .search_match_mode(gtk::StringFilterMatchMode::Substring)
        .expression(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            None::<gtk::Expression>,
            "string",
        ))
        .build();
    row.set_selected(selected as u32);
    row
}

pub(super) fn zone_label(zone: &str, offset_seconds: i32) -> String {
    format!(
        "{} · UTC{}",
        zone.replace('_', " "),
        offset_text(offset_seconds)
    )
}

/// `+05:30` style offset, with a real minus sign.
pub(super) fn offset_text(offset_seconds: i32) -> String {
    let sign = if offset_seconds < 0 { '−' } else { '+' };
    let abs = offset_seconds.unsigned_abs();
    format!("{sign}{:02}:{:02}", abs / 3600, abs % 3600 / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoned_dates_carry_the_zone_offset() {
        assert_eq!(
            zoned_rfc3339("2024-01-15 19:55:15", "Asia/Kolkata").as_deref(),
            Some("2024-01-15T19:55:15+05:30")
        );
        assert_eq!(
            zoned_rfc3339("2024-01-15 19:55:15", "UTC").as_deref(),
            Some("2024-01-15T19:55:15+00:00")
        );
        assert_eq!(zoned_rfc3339("not a date", "UTC"), None);
    }

    #[test]
    fn offsets_follow_daylight_saving() {
        assert_eq!(
            offset_seconds("Europe/Lisbon", "2024-01-15 12:00:00"),
            Some(0)
        );
        assert_eq!(
            offset_seconds("Europe/Lisbon", "2024-07-15 12:00:00"),
            Some(3600)
        );
    }

    #[test]
    fn zone_labels_are_readable() {
        assert_eq!(
            zone_label("America/New_York", -18000),
            "America/New York · UTC−05:00"
        );
        assert_eq!(
            zone_label("Asia/Kolkata", 19800),
            "Asia/Kolkata · UTC+05:30"
        );
        assert_eq!(zone_label("UTC", 0), "UTC · UTC+00:00");
    }

    #[test]
    fn utc_instants_show_in_the_photo_zone() {
        assert_eq!(
            wall_time_in("2024-01-15T10:25:15.000Z", "Asia/Kolkata").as_deref(),
            Some("2024-01-15 15:55:15")
        );
        assert_eq!(
            wall_time_in("2024-07-15T12:00:00+00:00", "Europe/Lisbon").as_deref(),
            Some("2024-07-15 13:00:00")
        );
        assert_eq!(wall_time_in("garbage", "UTC"), None);
    }

    #[test]
    fn zone_list_includes_utc() {
        assert!(all().iter().any(|zone| zone == "UTC"));
    }
}
