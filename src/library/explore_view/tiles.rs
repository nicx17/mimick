use std::rc::Rc;
use std::sync::Arc;

use gdk4::Texture;
use glib::Bytes;
use gtk::prelude::*;

use crate::api_client::Person;
use crate::app_context::AppContext;

use super::ExploreClick;

fn parse_iso_date(iso: &str) -> Option<(i64, u32, u32, u32, u32, u32)> {
    let parsed = iso
        .replace('T', " ")
        .replace('Z', "")
        .chars()
        .take(19)
        .collect::<String>();

    let parts: Vec<&str> = parsed.split(&['-', ' ', ':'][..]).collect();
    if parts.len() < 6 {
        return None;
    }
    match (
        parts[0].parse::<i64>(),
        parts[1].parse::<u32>(),
        parts[2].parse::<u32>(),
        parts[3].parse::<u32>(),
        parts[4].parse::<u32>(),
        parts[5].parse::<u32>(),
    ) {
        (Ok(y), Ok(mo), Ok(d), Ok(h), Ok(mi), Ok(s)) => Some((y, mo, d, h, mi, s)),
        _ => None,
    }
}

fn compute_epoch_seconds(year: i64, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> i64 {
    let days_in_month = [0u32, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut total_days: i64 = 0;
    for y in 1970..year {
        total_days += if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
    }
    let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    for m in 1..month {
        total_days += days_in_month[m as usize] as i64;
        if m == 2 && is_leap {
            total_days += 1;
        }
    }
    total_days += (day - 1) as i64;
    total_days * 86400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64
}

/// Format an ISO 8601 timestamp into a human-readable relative label.
pub(crate) fn format_relative_date(iso: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let Some((year, month, day, hour, min, sec)) = parse_iso_date(iso) else {
        return iso.chars().take(10).collect();
    };

    let ts = compute_epoch_seconds(year, month, day, hour, min, sec);
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let diff = now_secs - ts;

    if diff < 60 {
        "Just now".to_string()
    } else if diff < 3600 {
        let m = diff / 60;
        format!("{m} min ago")
    } else if diff < 86400 {
        let h = diff / 3600;
        format!("{h}h ago")
    } else if diff < 86400 * 7 {
        let d = diff / 86400;
        format!("{d}d ago")
    } else {
        format!("{year:04}-{month:02}-{day:02}")
    }
}

/// Construct an individual circular avatar widget representing a recognized person face.
pub(crate) fn person_tile(
    ctx: Arc<AppContext>,
    person: &Person,
    on_click: Rc<dyn Fn(String, String)>,
) -> gtk::Button {
    let avatar = gtk::Picture::builder()
        .width_request(96)
        .height_request(96)
        .can_shrink(true)
        .content_fit(gtk::ContentFit::Cover)
        .css_classes(vec!["mimick-person-avatar".to_string()])
        .build();
    let label = gtk::Label::builder()
        .label(if person.name.is_empty() {
            "Unnamed"
        } else {
            &person.name
        })
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(12)
        .build();
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .halign(gtk::Align::Center)
        .build();
    inner.append(&avatar);
    inner.append(&label);

    let button = gtk::Button::builder()
        .child(&inner)
        .css_classes(vec!["flat".to_string()])
        .build();

    let id = person.id.clone();
    let name = if person.name.is_empty() {
        "Unnamed".to_string()
    } else {
        person.name.clone()
    };
    button.connect_clicked(move |_| on_click(id.clone(), name.clone()));

    spawn_person_thumbnail(ctx, person.id.clone(), move |texture| {
        avatar.set_paintable(Some(texture))
    });
    button
}

/// Construct a rectangular tile representation for a specific explore category node.
pub(crate) fn explore_tile(
    ctx: Arc<AppContext>,
    kind: &'static str,
    value: &str,
    asset_id: &str,
    on_click: ExploreClick,
) -> gtk::Button {
    // Fixed-height thumbnail container: the Overlay sizes itself from the
    // spacer child (100px) so portrait thumbnails cannot inflate the row
    // height.  The Picture overlay fills that space with ContentFit::Cover.
    let thumb = gtk::Overlay::builder()
        .overflow(gtk::Overflow::Hidden)
        .css_classes(vec!["mimick-explore-tile".to_string()])
        .build();
    let spacer = gtk::Box::builder()
        .css_classes(vec!["mimick-explore-spacer".to_string()])
        .build();
    let picture = gtk::Picture::builder()
        .can_shrink(true)
        .content_fit(gtk::ContentFit::Cover)
        .build();
    thumb.set_child(Some(&spacer));
    thumb.add_overlay(&picture);

    let label = gtk::Label::builder()
        .label(value)
        .xalign(0.0)
        .width_chars(1)
        .max_width_chars(1)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(vec!["caption-heading".to_string()])
        .build();
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    inner.append(&thumb);
    inner.append(&label);

    let button = gtk::Button::builder()
        .child(&inner)
        .css_classes(vec!["flat".to_string()])
        .build();

    let value_owned = value.to_string();
    let asset_id_owned = asset_id.to_string();
    button.connect_clicked(move |_| on_click(kind, value_owned.clone(), asset_id_owned.clone()));

    super::super::thumbnail_loader::load_into_picture(ctx, asset_id.to_string(), picture, false);
    button
}

/// Helper to asynchronously request and render a round avatar person face thumbnail.
pub(crate) fn spawn_person_thumbnail(
    ctx: Arc<AppContext>,
    person_id: String,
    on_loaded: impl FnOnce(&Texture) + 'static,
) {
    glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
        glib::MainContext::default().spawn_local(async move {
            let bytes = match ctx.api_client.fetch_person_thumbnail(&person_id).await {
                Ok(b) => b,
                Err(_) => return,
            };
            let texture = tokio::task::spawn_blocking(move || -> Option<Texture> {
                Texture::from_bytes(&Bytes::from(&bytes[..])).ok()
            })
            .await
            .ok()
            .flatten();
            if let Some(texture) = texture {
                on_loaded(&texture);
            }
        });
    });
}
