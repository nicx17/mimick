//! One card per photo in a duplicate group: the photo, its Keep or Trash label, and its facts.

use crate::i18n::{fill, format_day};
use gettextrs::gettext;
use std::collections::HashSet;
use std::rc::Rc;

use gtk::prelude::*;

use super::sync_buttons;
use crate::api_client::{DuplicateGroup, LibraryAsset};
use crate::library::LibraryWindowUi;

/// Photo height in the two-column grid on phone-width windows.
const NARROW_THUMB: i32 = 170;
/// Narrowest a photo gets side by side; more photos than fit scroll sideways.
const WIDE_CARD_MIN: i32 = 260;

/// Phones: two columns that scroll down. Wider: side by side, as large as the window allows.
pub(super) fn arrange(cards: &[gtk::ToggleButton], narrow: bool) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::builder().vexpand(true).build();
    if narrow {
        let grid = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(2)
            .max_children_per_line(2)
            .column_spacing(12)
            .row_spacing(12)
            .valign(gtk::Align::Start)
            .build();
        for card in cards {
            grid.append(card);
        }
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_child(Some(&grid));
    } else {
        let row = gtk::Box::builder().spacing(16).homogeneous(true).build();
        for card in cards {
            row.append(card);
        }
        scroller.set_vscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_child(Some(&row));
    }
    scroller
}

/// Immich's suggestion, else the largest file, so there's always one photo to keep.
pub(super) fn initial_keep(group: &DuplicateGroup) -> HashSet<String> {
    let ids: HashSet<&str> = group.assets.iter().map(|a| a.id.as_str()).collect();
    let suggested: HashSet<String> = group
        .suggested_keep_asset_ids
        .iter()
        .filter(|id| ids.contains(id.as_str()))
        .cloned()
        .collect();
    if !suggested.is_empty() {
        return suggested;
    }
    group
        .assets
        .iter()
        .max_by_key(|asset| file_size(asset))
        .map(|asset| HashSet::from([asset.id.clone()]))
        .unwrap_or_default()
}

pub(super) fn file_size(asset: &LibraryAsset) -> u64 {
    asset
        .exif_info
        .as_ref()
        .and_then(|exif| exif.file_size_in_byte)
        .unwrap_or(0)
}

pub(super) fn card(
    ui: &Rc<LibraryWindowUi>,
    asset: &LibraryAsset,
    narrow: bool,
    largest: u64,
) -> gtk::ToggleButton {
    let (frame, verdict) = photo_frame(ui, asset, narrow);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    content.append(&frame);
    for (text, class) in card_facts(asset, largest) {
        content.append(
            &gtk::Label::builder()
                .label(&text)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .tooltip_text(&text)
                .css_classes([class])
                .build(),
        );
    }
    let keep = ui.duplicates.keep.borrow().contains(&asset.id);
    let button = gtk::ToggleButton::builder()
        .child(&content)
        .active(keep)
        .css_classes(["card", "mimick-duplicate-card"])
        .build();
    if !narrow {
        button.set_width_request(WIDE_CARD_MIN);
    }
    show_verdict(&button, &verdict);
    let (ui, id) = (ui.clone(), asset.id.clone());
    button.connect_toggled(move |button| {
        let mut keep = ui.duplicates.keep.borrow_mut();
        if button.is_active() {
            keep.insert(id.clone());
        } else {
            keep.remove(&id);
        }
        drop(keep);
        show_verdict(button, &verdict);
        sync_buttons(&ui);
    });
    button
}

/// The photo with its Keep or Trash label. Wide layouts show the whole photo at preview
/// quality, filling the height; phones crop a thumbnail to a fixed height.
fn photo_frame(
    ui: &Rc<LibraryWindowUi>,
    asset: &LibraryAsset,
    narrow: bool,
) -> (gtk::Overlay, gtk::Label) {
    let picture = gtk::Picture::builder().vexpand(!narrow).build();
    if narrow {
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_height_request(NARROW_THUMB);
        crate::library::thumbnail_loader::load_into_picture(
            ui.ctx.clone(),
            asset.id.clone(),
            picture.clone(),
            true,
        );
    } else {
        picture.set_content_fit(gtk::ContentFit::Contain);
        load_preview(ui, &asset.id, &picture);
    }
    let frame = gtk::Overlay::builder()
        .child(&picture)
        .vexpand(!narrow)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["mimick-duplicate-thumb"])
        .build();
    let verdict = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .margin_top(8)
        .margin_start(8)
        .css_classes(["osd", "mimick-duplicate-verdict"])
        .build();
    frame.add_overlay(&verdict);
    (frame, verdict)
}

/// The cached thumbnail right away, then the sharper preview once it arrives.
fn load_preview(ui: &LibraryWindowUi, asset_id: &str, picture: &gtk::Picture) {
    let cache = ui.ctx.thumbnail_cache.clone();
    if let Some(texture) = cache.get_cached(asset_id, crate::api_client::ThumbnailSize::Thumbnail) {
        picture.set_paintable(Some(&texture));
    }
    let (id, picture) = (asset_id.to_string(), picture.clone());
    glib::MainContext::default().spawn_local(async move {
        if let Ok(texture) = cache.load_preview(&id, false).await {
            picture.set_paintable(Some(&texture));
        }
    });
}

fn show_verdict(button: &gtk::ToggleButton, verdict: &gtk::Label) {
    if button.is_active() {
        verdict.set_label(&gettext("Keep"));
        verdict.remove_css_class("mimick-duplicate-trash");
    } else {
        verdict.set_label(&gettext("Trash"));
        verdict.add_css_class("mimick-duplicate-trash");
    }
}

/// File name; resolution and size, marking the group's largest file; then the capture day.
fn card_facts(asset: &LibraryAsset, largest: u64) -> Vec<(String, &'static str)> {
    let mut facts = vec![(asset.filename.clone(), "heading")];
    let exif = asset.exif_info.as_ref();
    let size = file_size(asset);
    let dims = exif
        .and_then(|e| e.exif_image_width.zip(e.exif_image_height))
        .or(asset.width.zip(asset.height));
    let mut detail = Vec::new();
    if let Some((w, h)) = dims {
        let megapixels = f64::from(w) * f64::from(h) / 1_000_000.0;
        detail.push(format!("{w} × {h} ({megapixels:.1} MP)"));
    }
    if size > 0 {
        let size_text = glib::format_size(size).to_string();
        detail.push(if size == largest {
            fill(gettext("{size}, largest"), &[("size", &size_text)])
        } else {
            size_text
        });
    }
    if !detail.is_empty() {
        facts.push((detail.join(" · "), "caption"));
    }
    let taken = asset
        .local_date_time
        .as_deref()
        .unwrap_or(&asset.created_at)
        .get(..10)
        .and_then(|day| chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok());
    if let Some(day) = taken {
        facts.push((format_day(day, &gettext("%-d %b %Y")), "dim-label"));
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: &str, size: u64) -> LibraryAsset {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "originalFileName": format!("{id}.jpg"),
            "originalMimeType": "image/jpeg",
            "fileCreatedAt": "2026-03-15T12:00:00.000Z",
            "type": "IMAGE",
            "exifInfo": { "fileSizeInByte": size }
        }))
        .unwrap()
    }

    #[test]
    fn suggestion_wins_else_the_largest_file_is_kept() {
        let mut group = DuplicateGroup {
            duplicate_id: "d".into(),
            assets: vec![asset("small", 10), asset("big", 99)],
            suggested_keep_asset_ids: vec!["small".into()],
        };
        assert_eq!(initial_keep(&group), HashSet::from(["small".to_string()]));
        group.suggested_keep_asset_ids = vec!["gone".into()];
        assert_eq!(initial_keep(&group), HashSet::from(["big".to_string()]));
    }

    #[test]
    fn card_facts_show_size_and_day() {
        let facts = card_facts(&asset("a", 2_000_000), 9_000_000);
        assert_eq!(facts[0].0, "a.jpg");
        assert!(facts[1].0.contains("MB"));
        assert!(!facts[1].0.contains("largest"));
        assert_eq!(facts.len(), 3);
        let biggest = card_facts(&asset("b", 9_000_000), 9_000_000);
        assert!(biggest[1].0.ends_with(", largest"));
    }
}
