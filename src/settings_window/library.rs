use crate::i18n::fill;
use adw::prelude::*;
use gettextrs::gettext;
use libadwaita as adw;

pub struct LibraryWidgets {
    /// Library-only sections, hidden while the library view is off.
    pub library_groups: Vec<adw::PreferencesGroup>,
    pub preview_full_row: adw::SwitchRow,
    pub grid_quality_row: adw::ComboRow,
    pub grid_fit_row: adw::SwitchRow,
    pub date_headings_row: adw::ComboRow,
    pub map_full_grid_row: adw::SwitchRow,
    pub raw_full_decode_row: adw::SwitchRow,
    pub raw_cache_row: adw::SwitchRow,
    pub disk_cache_row: adw::SpinRow,
    pub memory_cache_row: adw::SpinRow,
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
    let (viewer, preview_full_row, grid_quality_row, grid_fit_row, date_headings_row) =
        build_viewer_group(settings_page);
    let map_full_grid_row = map_full_grid_row(&viewer);
    let (grid, border_width_row, border_color_btn) = build_appearance_group(settings_page);
    let (raw, raw_full_decode_row, raw_cache_row) = build_raw_group(settings_page);
    let (
        storage,
        disk_cache_row,
        memory_cache_row,
        clear_cache_btn,
        download_folder_row,
        download_change_btn,
        download_clear_btn,
    ) = build_storage_group(settings_page);

    LibraryWidgets {
        library_groups: vec![viewer, grid, raw, storage],
        preview_full_row,
        grid_quality_row,
        grid_fit_row,
        date_headings_row,
        map_full_grid_row,
        raw_full_decode_row,
        raw_cache_row,
        disk_cache_row,
        memory_cache_row,
        clear_cache_btn,
        download_folder_row,
        download_change_btn,
        download_clear_btn,
        border_width_row,
        border_color_btn,
    }
}

fn map_full_grid_row(viewer: &adw::PreferencesGroup) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(gettext("Open Map Places in a Full Grid"))
        .subtitle(gettext("Otherwise their photos open beside the map."))
        .build();
    viewer.add(&row);
    row
}

fn build_viewer_group(
    settings_page: &adw::PreferencesPage,
) -> (
    adw::PreferencesGroup,
    adw::SwitchRow,
    adw::ComboRow,
    adw::SwitchRow,
    adw::ComboRow,
) {
    let viewer = group(settings_page, &gettext("Library"));
    let preview_full_row = adw::SwitchRow::builder()
        .title(gettext("Open Originals in Lightbox"))
        .subtitle(gettext("Slower to open."))
        .build();
    viewer.add(&preview_full_row);
    let grid_quality_row = adw::ComboRow::builder()
        .title(gettext("Thumbnail Quality"))
        .subtitle(gettext("Sharper thumbnails use more memory."))
        .model(&gtk::StringList::new(&[
            &gettext("Automatic"),
            &gettext("Thumbnail"),
            &gettext("Preview"),
            &gettext("Full Size"),
        ]))
        .build();
    viewer.add(&grid_quality_row);
    let grid_fit_row = adw::SwitchRow::builder()
        .title(gettext("Fit Thumbnails to Tiles"))
        .subtitle(gettext(
            "Preview and Full Size thumbnails use far less memory.",
        ))
        .build();
    viewer.add(&grid_fit_row);
    let date_headings_row = adw::ComboRow::builder()
        .title(gettext("Date Headings"))
        .subtitle(gettext(
            "Dates between photos in Photos, albums, and other timelines.",
        ))
        .model(&gtk::StringList::new(&[
            &gettext("By Day"),
            &gettext("By Month"),
            &gettext("Off"),
        ]))
        .build();
    viewer.add(&date_headings_row);
    (
        viewer,
        preview_full_row,
        grid_quality_row,
        grid_fit_row,
        date_headings_row,
    )
}

fn build_appearance_group(
    settings_page: &adw::PreferencesPage,
) -> (adw::PreferencesGroup, adw::SpinRow, gtk::ColorDialogButton) {
    let grid = group(settings_page, &gettext("Grid Appearance"));
    let border_width_row = adw::SpinRow::builder()
        .title(gettext("Border Width"))
        .subtitle(gettext("Gap between photos, in pixels. 0 for none."))
        .adjustment(&gtk::Adjustment::new(0.0, 0.0, 50.0, 0.1, 1.0, 0.0))
        .digits(1)
        .build();
    grid.add(&border_width_row);
    let border_color_row = adw::ActionRow::builder()
        .title(gettext("Border Color"))
        .build();
    let border_color_btn = gtk::ColorDialogButton::builder()
        .dialog(&gtk::ColorDialog::builder().build())
        .valign(gtk::Align::Center)
        .build();
    border_color_row.add_suffix(&border_color_btn);
    border_color_row.set_activatable_widget(Some(&border_color_btn));
    grid.add(&border_color_row);
    (grid, border_width_row, border_color_btn)
}

fn build_raw_group(
    settings_page: &adw::PreferencesPage,
) -> (adw::PreferencesGroup, adw::SwitchRow, adw::SwitchRow) {
    let raw = group(settings_page, &gettext("RAW Files"));
    let raw_full_decode_row = adw::SwitchRow::builder()
        .title(gettext("Full RAW Decoding"))
        .subtitle(gettext(
            "Uses the sensor data, not the embedded preview, with every colour the camera recorded. Slower, and uses more memory.",
        ))
        .build();
    raw.add(&raw_full_decode_row);
    let raw_cache_row = adw::SwitchRow::builder()
        .title(gettext("Keep Decoded RAW Files"))
        .subtitle(gettext("Reopen instantly, using disk space."))
        .build();
    raw.add(&raw_cache_row);
    (raw, raw_full_decode_row, raw_cache_row)
}

fn build_storage_group(
    settings_page: &adw::PreferencesPage,
) -> (
    adw::PreferencesGroup,
    adw::SpinRow,
    adw::SpinRow,
    gtk::Button,
    adw::ActionRow,
    gtk::Button,
    gtk::Button,
) {
    let storage = group(settings_page, &gettext("Storage"));
    let disk_cache_row = adw::SpinRow::builder()
        .title(gettext("Cache Limit (MB)"))
        .subtitle(gettext("Trimmed when Mimick starts."))
        .adjustment(&gtk::Adjustment::new(
            2000.0, 200.0, 10000.0, 100.0, 500.0, 0.0,
        ))
        .build();
    storage.add(&disk_cache_row);
    let memory_cache_row = adw::SpinRow::builder()
        .title(gettext("Memory Limit (MB)"))
        .subtitle(gettext("At most half your RAM."))
        .adjustment(&gtk::Adjustment::new(
            2048.0, 256.0, 16384.0, 256.0, 1024.0, 0.0,
        ))
        .build();
    storage.add(&memory_cache_row);
    let clear_cache_btn = gtk::Button::builder()
        .label(gettext("Clear"))
        .valign(gtk::Align::Center)
        .build();
    let clear_row = adw::ActionRow::builder()
        .title(gettext("Clear Cache"))
        .subtitle(gettext(
            "Free the space now. Thumbnails reload as you browse.",
        ))
        .build();
    clear_row.add_suffix(&clear_cache_btn);
    storage.add(&clear_row);
    let download_folder_row = adw::ActionRow::builder()
        .title(gettext("Download Folder"))
        .subtitle(gettext("Ask each time"))
        .subtitle_lines(1)
        .build();
    let download_change_btn = gtk::Button::builder()
        .label(gettext("Change…"))
        .valign(gtk::Align::Center)
        .build();
    let download_clear_btn = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .tooltip_text(gettext("Ask each time"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .visible(false)
        .build();
    download_folder_row.add_suffix(&download_clear_btn);
    download_folder_row.add_suffix(&download_change_btn);
    storage.add(&download_folder_row);
    (
        storage,
        disk_cache_row,
        memory_cache_row,
        clear_cache_btn,
        download_folder_row,
        download_change_btn,
        download_clear_btn,
    )
}

/// "Using 1.2 GB of 2.0 GB" for the cache limit row.
pub(super) fn cache_usage_label(used_bytes: u64, cap_mb: u32) -> String {
    fill(
        gettext("Using {used} of {limit}"),
        &[
            ("used", &size_text(used_bytes as f64 / (1024.0 * 1024.0))),
            ("limit", &size_text(f64::from(cap_mb))),
        ],
    )
}

fn size_text(megabytes: f64) -> String {
    if megabytes >= 1000.0 {
        fill(
            gettext("{size} GB"),
            &[("size", &format!("{:.1}", megabytes / 1024.0))],
        )
    } else {
        fill(
            gettext("{size} MB"),
            &[("size", &(megabytes.round() as u64).to_string())],
        )
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
