use adw::prelude::*;
use libadwaita as adw;

pub struct LibraryWidgets {
    /// Library-only sections, hidden while the library view is off.
    pub library_groups: Vec<adw::PreferencesGroup>,
    pub preview_full_row: adw::SwitchRow,
    pub grid_quality_row: adw::ComboRow,
    pub date_headings_row: adw::ComboRow,
    pub raw_full_decode_row: adw::SwitchRow,
    pub raw_cache_row: adw::SwitchRow,
    pub disk_cache_row: adw::SpinRow,
    pub clear_cache_btn: gtk::Button,
    pub download_folder_row: adw::ActionRow,
    pub download_change_btn: gtk::Button,
    pub download_clear_btn: gtk::Button,
    pub border_width_row: adw::SpinRow,
    pub border_color_btn: gtk::ColorDialogButton,
}

fn group(page: &adw::PreferencesPage, title: &str) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title(title).build();
    page.add(&group);
    group
}

pub fn build_library_group(settings_page: &adw::PreferencesPage) -> LibraryWidgets {
    let viewer = group(settings_page, "Library");
    let preview_full_row = adw::SwitchRow::builder()
        .title("Open Originals in Lightbox")
        .subtitle("Full resolution instead of the 1440 px preview. Slower to open.")
        .build();
    viewer.add(&preview_full_row);
    let grid_quality_row = adw::ComboRow::builder()
        .title("Thumbnail Quality")
        .subtitle("Sharper thumbnails use more memory.")
        .model(&gtk::StringList::new(&[
            "Automatic",
            "Thumbnail",
            "Preview",
            "Full Size",
        ]))
        .build();
    viewer.add(&grid_quality_row);
    let date_headings_row = adw::ComboRow::builder()
        .title("Date Headings")
        .subtitle("Dates between photos in Photos.")
        .model(&gtk::StringList::new(&["By Day", "By Month", "Off"]))
        .build();
    viewer.add(&date_headings_row);

    let grid = group(settings_page, "Grid Appearance");
    let border_width_row = adw::SpinRow::builder()
        .title("Border Width")
        .subtitle("Gap between photos, in pixels. 0 for none.")
        .adjustment(&gtk::Adjustment::new(0.0, 0.0, 50.0, 0.1, 1.0, 0.0))
        .digits(1)
        .build();
    grid.add(&border_width_row);
    let border_color_row = adw::ActionRow::builder().title("Border Color").build();
    let border_color_btn = gtk::ColorDialogButton::builder()
        .dialog(&gtk::ColorDialog::builder().build())
        .valign(gtk::Align::Center)
        .build();
    border_color_row.add_suffix(&border_color_btn);
    border_color_row.set_activatable_widget(Some(&border_color_btn));
    grid.add(&border_color_row);

    let raw = group(settings_page, "RAW Files");
    let raw_full_decode_row = adw::SwitchRow::builder()
        .title("Full RAW Decoding")
        .subtitle("Decode the sensor data instead of the embedded preview. Slower.")
        .build();
    raw.add(&raw_full_decode_row);
    let raw_cache_row = adw::SwitchRow::builder()
        .title("Keep Decoded RAW Files")
        .subtitle("Reopen instantly, using disk space.")
        .build();
    raw.add(&raw_cache_row);

    let storage = group(settings_page, "Storage");
    let disk_cache_row = adw::SpinRow::builder()
        .title("Cache Limit (MB)")
        .subtitle("Thumbnails, previews, and videos. Trimmed when Mimick starts.")
        .adjustment(&gtk::Adjustment::new(
            2000.0, 200.0, 10000.0, 100.0, 500.0, 0.0,
        ))
        .build();
    storage.add(&disk_cache_row);
    let clear_cache_btn = gtk::Button::builder()
        .label("Clear")
        .valign(gtk::Align::Center)
        .build();
    let clear_row = adw::ActionRow::builder()
        .title("Clear Cache")
        .subtitle("Free the space now. Thumbnails reload as you browse.")
        .build();
    clear_row.add_suffix(&clear_cache_btn);
    storage.add(&clear_row);
    let download_folder_row = adw::ActionRow::builder()
        .title("Download Folder")
        .subtitle("Ask each time")
        .subtitle_lines(1)
        .build();
    let download_change_btn = gtk::Button::builder()
        .label("Change…")
        .valign(gtk::Align::Center)
        .build();
    let download_clear_btn = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .tooltip_text("Ask each time")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .visible(false)
        .build();
    download_folder_row.add_suffix(&download_clear_btn);
    download_folder_row.add_suffix(&download_change_btn);
    storage.add(&download_folder_row);

    LibraryWidgets {
        library_groups: vec![viewer, grid, raw, storage],
        preview_full_row,
        grid_quality_row,
        date_headings_row,
        raw_full_decode_row,
        raw_cache_row,
        disk_cache_row,
        clear_cache_btn,
        download_folder_row,
        download_change_btn,
        download_clear_btn,
        border_width_row,
        border_color_btn,
    }
}

/// "Using 1.2 GB of 2.0 GB" for the cache limit row.
pub(super) fn cache_usage_label(used_bytes: u64, cap_mb: u32) -> String {
    format!(
        "Using {} of {}",
        size_text(used_bytes as f64 / (1024.0 * 1024.0)),
        size_text(f64::from(cap_mb))
    )
}

fn size_text(megabytes: f64) -> String {
    if megabytes >= 1000.0 {
        format!("{:.1} GB", megabytes / 1024.0)
    } else {
        format!("{} MB", megabytes.round() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::cache_usage_label;

    #[test]
    fn cache_usage_reads_naturally() {
        assert_eq!(cache_usage_label(0, 2000), "Using 0 MB of 2.0 GB");
        assert_eq!(
            cache_usage_label(1_258_291_200, 2000),
            "Using 1.2 GB of 2.0 GB"
        );
        assert_eq!(cache_usage_label(52_428_800, 500), "Using 50 MB of 500 MB");
    }
}
