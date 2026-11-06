use crate::i18n::format_day;
use crate::i18n::{fill, plural_n};
use chrono::{DateTime, Datelike, NaiveDate};
use gettextrs::{gettext, ngettext};

use crate::api_client::LibraryAlbum;

pub(super) fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Fills `{n}` in an `ngettext` result with the count, grouped by thousands.
pub(super) fn count_text(n: u64, template: String) -> String {
    fill(template, &[("n", &group_thousands(n))])
}

pub(super) fn count_label(n: u64) -> String {
    count_text(n, ngettext("{n} item", "{n} items", plural_n(n)))
}

fn parse_day(value: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.date_naive())
        .ok()
        .or_else(|| NaiveDate::parse_from_str(value.get(..10)?, "%Y-%m-%d").ok())
}

fn age_on(birth: NaiveDate, today: NaiveDate) -> i32 {
    let mut age = today.year() - birth.year();
    if (today.month(), today.day()) < (birth.month(), birth.day()) {
        age -= 1;
    }
    age
}

pub(super) fn person_details(birth_date: Option<&str>, today: NaiveDate) -> Vec<String> {
    let Some(birth) = birth_date.and_then(parse_day) else {
        return Vec::new();
    };
    vec![
        fill(
            gettext("Age {age}"),
            &[("age", &age_on(birth, today).to_string())],
        ),
        fill(
            gettext("Born {date}"),
            // Translators: strftime date, e.g. "9 Oct 2026".
            &[("date", &format_day(birth, &gettext("%-d %b %Y")))],
        ),
    ]
}

fn format_date_range(start: Option<&str>, end: Option<&str>) -> Option<String> {
    let start = start.and_then(parse_day);
    let end = end.and_then(parse_day);
    let (start, end) = match (start, end) {
        (Some(start), Some(end)) => (start.min(end), start.max(end)),
        (Some(day), None) | (None, Some(day)) => (day, day),
        (None, None) => return None,
    };
    let full = |day: NaiveDate| format_day(day, &gettext("%-d %b %Y"));
    Some(if start == end {
        full(start)
    } else if start.year() == end.year() && start.month() == end.month() {
        format!("{}–{}", start.format("%-d"), full(end))
    } else if start.year() == end.year() {
        // Translators: strftime day and month that start a date range, e.g. "30 Dec".
        let start = format_day(start, &gettext("%-d %b"));
        format!("{start} – {}", full(end))
    } else {
        format!("{} – {}", full(start), full(end))
    })
}

pub(super) fn owns_album(album: &LibraryAlbum, me: &str) -> bool {
    let owner = album.owner_id();
    owner.is_empty() || owner == me
}

pub(super) fn album_details(album: &LibraryAlbum, me: &str) -> Vec<String> {
    let mut details = Vec::new();
    if let Some(range) = format_date_range(album.start_date.as_deref(), album.end_date.as_deref()) {
        details.push(range);
    }
    if owns_album(album, me) {
        match album.album_users.len().saturating_sub(1) {
            0 => {}
            n => details.push(fill(
                ngettext(
                    "Shared with {n} person",
                    "Shared with {n} people",
                    plural_n(n),
                ),
                &[("n", &n.to_string())],
            )),
        }
        return details;
    }
    let owner = album
        .album_users
        .iter()
        .find(|member| member.user.id == album.owner_id())
        .map(|member| member.user.name.as_str())
        .unwrap_or_default();
    if !owner.is_empty() {
        details.push(fill(gettext("Shared by {owner}"), &[("owner", owner)]));
    }
    details
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn age_counts_whole_years() {
        assert_eq!(age_on(day(1990, 5, 4), day(2026, 5, 3)), 35);
        assert_eq!(age_on(day(1990, 5, 4), day(2026, 5, 4)), 36);
    }

    #[test]
    fn person_details_show_age_and_birthday() {
        assert_eq!(
            person_details(Some("1990-05-04"), day(2026, 10, 12)),
            ["Age 36", "Born 4 May 1990"]
        );
        assert!(person_details(None, day(2026, 10, 12)).is_empty());
    }

    #[test]
    fn date_ranges_collapse_shared_parts() {
        let range = |a: &str, b: &str| format_date_range(Some(a), Some(b)).unwrap();
        assert_eq!(
            range("2024-05-04T10:00:00.000Z", "2024-05-04T18:00:00.000Z"),
            "4 May 2024"
        );
        assert_eq!(
            range("2024-05-04T10:00:00.000Z", "2024-05-09T10:00:00.000Z"),
            "4–9 May 2024"
        );
        assert_eq!(
            range("2024-05-04T10:00:00.000Z", "2024-06-09T10:00:00.000Z"),
            "4 May – 9 Jun 2024"
        );
        assert_eq!(
            range("2023-12-30T10:00:00.000Z", "2024-01-02T10:00:00.000Z"),
            "30 Dec 2023 – 2 Jan 2024"
        );
        assert_eq!(format_date_range(None, None), None);
    }

    #[test]
    fn counts_group_thousands_and_pluralize() {
        assert_eq!(count_label(1), "1 item");
        assert_eq!(count_label(1234567), "1,234,567 items");
        assert_eq!(
            count_text(0, ngettext("{n} photo", "{n} photos", 0)),
            "0 photos"
        );
    }

    #[test]
    fn capitalize_first_letter_only() {
        assert_eq!(capitalize("beach"), "Beach");
        assert_eq!(capitalize(""), "");
    }

    fn album(owner: &str, members: &[&str]) -> LibraryAlbum {
        let mut users = vec![serde_json::json!({
            "user": { "id": owner, "name": "Ana" },
            "role": "owner"
        })];
        for member in members {
            users.push(serde_json::json!({
                "user": { "id": member, "name": "Ben" },
                "role": "viewer"
            }));
        }
        serde_json::from_value(serde_json::json!({
            "id": "a1",
            "albumName": "Trips",
            "assetCount": 3,
            "albumThumbnailAssetId": null,
            "createdAt": "2024-01-01T00:00:00.000Z",
            "updatedAt": "2024-01-01T00:00:00.000Z",
            "startDate": "2024-05-04T10:00:00.000Z",
            "endDate": "2024-05-09T10:00:00.000Z",
            "albumUsers": users
        }))
        .unwrap()
    }

    #[test]
    fn album_details_describe_sharing_from_my_side() {
        assert_eq!(
            album_details(&album("me", &["u2", "u3"]), "me"),
            ["4–9 May 2024", "Shared with 2 people"]
        );
        assert_eq!(album_details(&album("me", &[]), "me"), ["4–9 May 2024"]);
        assert_eq!(
            album_details(&album("u1", &["me"]), "me"),
            ["4–9 May 2024", "Shared by Ana"]
        );
        assert!(owns_album(&album("me", &[]), "me"));
        assert!(!owns_album(&album("u1", &["me"]), "me"));
    }
}
