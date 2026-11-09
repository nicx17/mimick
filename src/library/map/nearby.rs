//! Set Location's "Taken Nearby": where your other photos from the same hours were taken,
//! such as phone photos beside a camera's photos that have no GPS.

use gettextrs::gettext;
use std::rc::Rc;

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use gtk::prelude::*;

use super::coords::GeoPoint;
use crate::api_client::{ImmichApiClient, LibraryAsset, MetadataSearchFilters};
use crate::i18n::format_day;
use crate::library::LibraryWindowUi;

/// How far either side of the photos' own times to look.
const SLACK_HOURS: i64 = 12;
const MAX_PLACES: usize = 4;
const SEARCH_SIZE: u32 = 200;

struct Nearby {
    point: GeoPoint,
    place: String,
    /// When that photo was taken, on its own clock.
    when: String,
    /// Seconds between it and the closest of the photos being placed.
    gap: i64,
}

/// Above the search: up to four places from photos taken within hours of `around`.
/// Shows nothing when there are none; `pick` moves the pin to the one tapped.
pub(super) fn offer(
    ui: &Rc<LibraryWindowUi>,
    content: &gtk::Box,
    around: (String, String),
    exclude: &[String],
    pick: impl Fn(GeoPoint) + 'static,
) {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .visible(false)
        .build();
    content.prepend(&section);
    let (api, exclude, pick) = (ui.ctx.api_client.clone(), exclude.to_vec(), Rc::new(pick));
    glib::MainContext::default().spawn_local(async move {
        match nearby_places(&api, &around, &exclude).await {
            Ok(places) => fill_section(&section, places, pick),
            Err(err) => log::debug!("No places taken nearby: {err}"),
        }
    });
}

async fn nearby_places(
    api: &ImmichApiClient,
    around: &(String, String),
    exclude: &[String],
) -> Result<Vec<Nearby>, String> {
    let (start, end) = (parse(&around.0)?, parse(&around.1)?);
    let slack = Duration::hours(SLACK_HOURS);
    let filters = MetadataSearchFilters {
        taken_after: Some(iso(start - slack)),
        taken_before: Some(iso(end + slack)),
        with_exif: Some(true),
        ..MetadataSearchFilters::default()
    };
    let (assets, _) = api
        .search_metadata_with_filters(&filters, 1, SEARCH_SIZE)
        .await?;
    Ok(places_from(&assets, (start, end), exclude))
}

/// The closest in time first, one per place name.
fn places_from(
    assets: &[LibraryAsset],
    span: (DateTime<Utc>, DateTime<Utc>),
    exclude: &[String],
) -> Vec<Nearby> {
    let mut found: Vec<Nearby> = assets
        .iter()
        .filter(|asset| !exclude.contains(&asset.id))
        .filter_map(|asset| nearby(asset, span))
        .collect();
    found.sort_by_key(|nearby| nearby.gap);
    let mut seen: Vec<String> = Vec::new();
    found.retain(|nearby| {
        let fresh = !seen.contains(&nearby.place);
        if fresh {
            seen.push(nearby.place.clone());
        }
        fresh
    });
    found.truncate(MAX_PLACES);
    found
}

fn nearby(asset: &LibraryAsset, span: (DateTime<Utc>, DateTime<Utc>)) -> Option<Nearby> {
    let exif = asset.exif_info.as_ref()?;
    let point = GeoPoint::new(exif.latitude?, exif.longitude?)?;
    let taken = parse(&asset.created_at).ok()?;
    let place = exif.city.clone().or_else(|| exif.country.clone());
    let local = asset
        .local_date_time
        .as_deref()
        .unwrap_or(&asset.created_at);
    Some(Nearby {
        point,
        place: place.unwrap_or_else(|| point.display()),
        when: clock_time(local),
        gap: gap_seconds(taken, span),
    })
}

fn gap_seconds(taken: DateTime<Utc>, (start, end): (DateTime<Utc>, DateTime<Utc>)) -> i64 {
    if taken < start {
        (start - taken).num_seconds()
    } else {
        (taken - end).num_seconds().max(0)
    }
}

/// "3 Jun, 14:20" from Immich's local time, which it writes as if it were UTC.
fn clock_time(local: &str) -> String {
    let Some(at) = local.get(..19) else {
        return String::new();
    };
    match NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M:%S") {
        Ok(at) => format!(
            "{}, {}",
            format_day(at.date(), &gettext("%-d %b")),
            at.format("%H:%M")
        ),
        Err(_) => String::new(),
    }
}

fn fill_section(section: &gtk::Box, places: Vec<Nearby>, pick: Rc<dyn Fn(GeoPoint)>) {
    if places.is_empty() {
        return;
    }
    let heading = gtk::Label::builder()
        .label(gettext("Taken Nearby"))
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    let buttons = libadwaita::WrapBox::builder()
        .child_spacing(6)
        .line_spacing(6)
        .build();
    for nearby in places {
        buttons.append(&place_button(nearby, pick.clone()));
    }
    section.append(&heading);
    section.append(&buttons);
    section.set_visible(true);
}

fn place_button(nearby: Nearby, pick: Rc<dyn Fn(GeoPoint)>) -> gtk::Button {
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let place = gtk::Label::builder()
        .label(&nearby.place)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(18)
        .build();
    let when = gtk::Label::builder()
        .label(&nearby.when)
        .xalign(0.0)
        .css_classes(["caption", "dim-label"])
        .build();
    text.append(&place);
    text.append(&when);
    let button = gtk::Button::builder().child(&text).build();
    let point = nearby.point;
    button.connect_clicked(move |_| pick(point));
    button
}

fn parse(time: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(time)
        .map(|at| at.with_timezone(&Utc))
        .map_err(|err| err.to_string())
}

fn iso(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(time: &str) -> DateTime<Utc> {
        parse(time).unwrap()
    }

    #[test]
    fn photos_inside_the_span_count_as_no_gap() {
        let span = (at("2024-06-03T10:00:00Z"), at("2024-06-03T12:00:00Z"));
        assert_eq!(gap_seconds(at("2024-06-03T11:00:00Z"), span), 0);
        assert_eq!(gap_seconds(at("2024-06-03T09:00:00Z"), span), 3600);
        assert_eq!(gap_seconds(at("2024-06-03T13:30:00Z"), span), 5400);
    }

    #[test]
    fn clock_time_keeps_the_photos_own_clock() {
        assert_eq!(clock_time("2024-06-03T14:20:05.000Z"), "3 Jun, 14:20");
        assert_eq!(clock_time("nonsense"), "");
    }
}
