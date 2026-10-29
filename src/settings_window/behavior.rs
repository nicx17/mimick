use adw::prelude::*;
use libadwaita as adw;

pub struct BehaviorWidgets {
    pub startup_row: adw::SwitchRow,
    pub background_sync_row: adw::SwitchRow,
    pub metered_row: adw::SwitchRow,
    pub battery_row: adw::SwitchRow,
    pub notifications_row: adw::SwitchRow,
    pub library_view_row: adw::SwitchRow,
    pub catchup_row: adw::ComboRow,
    pub concurrency_row: adw::SpinRow,
    pub xmp_sidecar_row: adw::SwitchRow,
    pub quiet_hours_row: adw::SwitchRow,
    pub quiet_start_row: adw::SpinRow,
    pub quiet_end_row: adw::SpinRow,
}

/// "Uploads" (how and when files go up) then "App" (how Mimick runs), in that order.
pub fn build_behavior_group(settings_page: &adw::PreferencesPage) -> BehaviorWidgets {
    let uploads = build_uploads_group(settings_page);
    let app = build_app_group(settings_page);
    BehaviorWidgets {
        startup_row: app.startup,
        background_sync_row: app.background_sync,
        metered_row: uploads.metered,
        battery_row: uploads.battery,
        notifications_row: app.notifications,
        library_view_row: app.library_view,
        catchup_row: uploads.catchup,
        concurrency_row: uploads.concurrency,
        xmp_sidecar_row: uploads.xmp_sidecar,
        quiet_hours_row: uploads.quiet_hours,
        quiet_start_row: uploads.quiet_start,
        quiet_end_row: uploads.quiet_end,
    }
}

struct UploadWidgets {
    concurrency: adw::SpinRow,
    xmp_sidecar: adw::SwitchRow,
    catchup: adw::ComboRow,
    metered: adw::SwitchRow,
    battery: adw::SwitchRow,
    quiet_hours: adw::SwitchRow,
    quiet_start: adw::SpinRow,
    quiet_end: adw::SpinRow,
}

fn build_uploads_group(settings_page: &adw::PreferencesPage) -> UploadWidgets {
    let uploads = adw::PreferencesGroup::builder()
        .title("Uploads")
        .description("How and when your watched folders are backed up.")
        .build();
    settings_page.add(&uploads);
    let concurrency_row = add_spin(
        &uploads,
        "Parallel Uploads",
        "How many files upload at once. Higher is faster but uses more bandwidth.",
        gtk::Adjustment::new(3.0, 1.0, 10.0, 1.0, 1.0, 0.0),
    );
    let xmp_sidecar_row = add_switch(
        &uploads,
        "Upload XMP Sidecars",
        "Send .xmp edit files along with their photos.",
    );
    let catchup_row = add_catchup_row(&uploads);
    let metered_row = add_switch(
        &uploads,
        "Pause on Metered Networks",
        "Wait for an unmetered connection, such as home Wi-Fi.",
    );
    let battery_row = add_switch(
        &uploads,
        "Pause on Battery",
        "Wait until the computer is plugged in.",
    );
    let (quiet_hours_row, quiet_start_row, quiet_end_row) = add_quiet_hours_rows(&uploads);
    UploadWidgets {
        concurrency: concurrency_row,
        xmp_sidecar: xmp_sidecar_row,
        catchup: catchup_row,
        metered: metered_row,
        battery: battery_row,
        quiet_hours: quiet_hours_row,
        quiet_start: quiet_start_row,
        quiet_end: quiet_end_row,
    }
}

struct AppWidgets {
    startup: adw::SwitchRow,
    background_sync: adw::SwitchRow,
    notifications: adw::SwitchRow,
    library_view: adw::SwitchRow,
}

fn build_app_group(settings_page: &adw::PreferencesPage) -> AppWidgets {
    let app = adw::PreferencesGroup::builder().title("App").build();
    settings_page.add(&app);
    let startup_row = add_switch(&app, "Run on Startup", "Start Mimick when you log in.");
    let background_sync_row = add_switch(
        &app,
        "Background Sync",
        "Keep watching folders after you close this window.",
    );
    let notifications_row = add_switch(
        &app,
        "Notifications",
        "Tell me when uploads finish or something goes wrong.",
    );
    let library_view_row = add_switch(
        &app,
        "Library View",
        "Browse your Immich photos in Mimick. The tray menu updates after a restart.",
    );
    AppWidgets {
        startup: startup_row,
        background_sync: background_sync_row,
        notifications: notifications_row,
        library_view: library_view_row,
    }
}

fn add_quiet_hours_rows(
    group: &adw::PreferencesGroup,
) -> (adw::SwitchRow, adw::SpinRow, adw::SpinRow) {
    let toggle = add_switch(
        group,
        "Quiet Hours",
        "Don't upload during these hours each day.",
    );
    let start = clock_row(group, "From", 22.0);
    let end = clock_row(group, "Until", 7.0);
    (toggle, start, end)
}

/// An hour picker that shows and accepts "22:00".
fn clock_row(group: &adw::PreferencesGroup, title: &str, hour: f64) -> adw::SpinRow {
    let row = add_spin(
        group,
        title,
        "",
        gtk::Adjustment::new(hour, 0.0, 23.0, 1.0, 1.0, 0.0),
    );
    row.set_wrap(true);
    row.connect_output(|row| {
        row.set_text(&hour_label(row.value()));
        true
    });
    row.connect_input(|row| parse_hour(&row.text()).map(Ok));
    row
}

pub(super) fn hour_label(hour: f64) -> String {
    format!("{:02}:00", hour.round() as u32)
}

pub(super) fn parse_hour(text: &str) -> Option<f64> {
    let hour: u32 = text.trim().split(':').next()?.trim().parse().ok()?;
    (hour < 24).then_some(f64::from(hour))
}

fn add_switch(group: &adw::PreferencesGroup, title: &str, subtitle: &str) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    group.add(&row);
    row
}

pub(super) const CATCHUP_LABELS: [&str; 3] =
    ["All Files", "Changed in the Last 7 Days", "New Files Only"];

fn add_catchup_row(group: &adw::PreferencesGroup) -> adw::ComboRow {
    let row = adw::ComboRow::builder()
        .title("When Mimick Starts, Check")
        .subtitle("Folders can choose their own in Folder Rules.")
        .model(&gtk::StringList::new(&CATCHUP_LABELS))
        .build();
    group.add(&row);
    row
}

fn add_spin(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    adjustment: gtk::Adjustment,
) -> adw::SpinRow {
    let mut builder = adw::SpinRow::builder().title(title).adjustment(&adjustment);
    if !subtitle.is_empty() {
        builder = builder.subtitle(subtitle);
    }
    let row = builder.build();
    group.add(&row);
    row
}

#[cfg(test)]
mod tests {
    use super::{hour_label, parse_hour};

    #[test]
    fn quiet_hours_read_as_clock_times() {
        assert_eq!(hour_label(22.0), "22:00");
        assert_eq!(hour_label(7.0), "07:00");
        assert_eq!(parse_hour("22:00"), Some(22.0));
        assert_eq!(parse_hour(" 7 "), Some(7.0));
        assert_eq!(parse_hour("07:30"), Some(7.0));
        assert_eq!(parse_hour("25"), None);
        assert_eq!(parse_hour("soon"), None);
    }
}
