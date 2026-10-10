//! The settings form: widgets read on save, their values, and how they map onto config.
//!
//! Signal handlers keep only a weak form (`SettingsFormWeak`) so the window's
//! widgets aren't kept alive by closures attached to themselves.

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::config::{Config, ConfigData, StartupCatchupMode, WatchPathEntry};

use super::behavior::BehaviorWidgets;
use super::connectivity::ConnectivityWidgets;
use super::library::LibraryWidgets;
use super::watch_folders::{catchup_from_index, catchup_index};

/// Declares the strong form plus a twin of `glib::WeakRef`s with `downgrade` / `upgrade`.
macro_rules! settings_form {
    ($($field:ident: $ty:ty),* $(,)?) => {
        /// Every widget whose value is saved to config.
        #[derive(Clone)]
        pub(super) struct SettingsForm {
            $(pub $field: $ty,)*
        }

        /// Weak references to the form, for handlers attached to its own widgets.
        pub(super) struct SettingsFormWeak {
            $($field: glib::WeakRef<$ty>,)*
        }

        impl SettingsForm {
            pub(super) fn downgrade(&self) -> SettingsFormWeak {
                SettingsFormWeak {
                    $($field: self.$field.downgrade(),)*
                }
            }
        }

        impl SettingsFormWeak {
            /// `None` once the settings window has been destroyed.
            pub(super) fn upgrade(&self) -> Option<SettingsForm> {
                Some(SettingsForm {
                    $($field: self.$field.upgrade()?,)*
                })
            }
        }
    };
}

settings_form! {
    window: adw::ApplicationWindow,
    internal_switch: gtk::Switch,
    external_switch: gtk::Switch,
    internal_entry: gtk::Entry,
    external_entry: gtk::Entry,
    api_key_entry: gtk::PasswordEntry,
    startup_row: adw::SwitchRow,
    background_sync_row: adw::SwitchRow,
    metered_row: adw::SwitchRow,
    battery_row: adw::SwitchRow,
    notifications_row: adw::SwitchRow,
    library_view_row: adw::SwitchRow,
    catchup_row: adw::ComboRow,
    concurrency_row: adw::SpinRow,
    xmp_sidecar_row: adw::SwitchRow,
    quiet_hours_row: adw::SwitchRow,
    quiet_start_row: adw::SpinRow,
    quiet_end_row: adw::SpinRow,
    preview_full_row: adw::SwitchRow,
    raw_full_decode_row: adw::SwitchRow,
    raw_cache_row: adw::SwitchRow,
    disk_cache_row: adw::SpinRow,
    border_width_row: adw::SpinRow,
    border_color_btn: gtk::ColorDialogButton,
}

/// Server addresses and API key, as entered or as currently saved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ConnectionValues {
    pub internal_url_enabled: bool,
    pub external_url_enabled: bool,
    pub internal_url: String,
    pub external_url: String,
    pub api_key: String,
}

impl ConnectionValues {
    pub(super) fn from_config(config: &Config) -> Self {
        Self {
            internal_url_enabled: config.data.internal_url_enabled,
            external_url_enabled: config.data.external_url_enabled,
            internal_url: config.data.internal_url.clone(),
            external_url: config.data.external_url.clone(),
            api_key: config.get_api_key().unwrap_or_default(),
        }
    }

    /// Enabled URLs only; a disabled address is passed to the API client as empty.
    pub(super) fn runtime_urls(&self) -> (String, String) {
        let pick = |enabled: bool, url: &str| {
            if enabled {
                url.to_string()
            } else {
                String::new()
            }
        };
        (
            pick(self.internal_url_enabled, &self.internal_url),
            pick(self.external_url_enabled, &self.external_url),
        )
    }
}

/// Reject non-HTTP(S) URL schemes before persisting; returns the alert heading and message.
pub(super) fn validate_connection(conn: &ConnectionValues) -> Result<(), (&'static str, String)> {
    let check = |enabled: bool, url: &str, heading: &'static str| {
        if enabled && !url.trim().is_empty() {
            crate::sanitize::validate_http_url(url).map_err(|err| (heading, err))?;
        }
        Ok(())
    };
    check(
        conn.internal_url_enabled,
        &conn.internal_url,
        "Invalid Internal URL",
    )?;
    check(
        conn.external_url_enabled,
        &conn.external_url,
        "Invalid External URL",
    )
}

/// Every non-connection setting read from the form.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SettingsValues {
    pub run_on_startup: bool,
    pub background_sync_enabled: bool,
    pub pause_on_metered_network: bool,
    pub pause_on_battery_power: bool,
    pub notifications_enabled: bool,
    pub library_view_enabled: bool,
    pub library_preview_full_resolution: bool,
    pub raw_decode_cache_enabled: bool,
    pub raw_full_decode: bool,
    pub cache_disk_cap_mb: u32,
    pub upload_concurrency: u8,
    pub quiet_hours_start: Option<u8>,
    pub quiet_hours_end: Option<u8>,
    pub upload_xmp_sidecars: bool,
    pub grid_border_width: f32,
    pub grid_border_color: String,
    pub catchup_mode: StartupCatchupMode,
}

/// Write the form's values and watch paths into config data (connection fields included).
pub(super) fn apply_to_config(
    data: &mut ConfigData,
    conn: &ConnectionValues,
    values: &SettingsValues,
    watch_paths: Vec<WatchPathEntry>,
) {
    data.internal_url_enabled = conn.internal_url_enabled;
    data.external_url_enabled = conn.external_url_enabled;
    data.internal_url = conn.internal_url.clone();
    data.external_url = conn.external_url.clone();
    data.watch_paths = watch_paths;
    data.run_on_startup = values.run_on_startup;
    data.background_sync_enabled = values.background_sync_enabled;
    data.pause_on_metered_network = values.pause_on_metered_network;
    data.pause_on_battery_power = values.pause_on_battery_power;
    data.notifications_enabled = values.notifications_enabled;
    data.library_view_enabled = values.library_view_enabled;
    data.library_preview_full_resolution = values.library_preview_full_resolution;
    data.raw_decode_cache_enabled = values.raw_decode_cache_enabled;
    data.raw_full_decode = values.raw_full_decode;
    data.cache_disk_cap_mb = values.cache_disk_cap_mb;
    data.startup_catchup_mode = values.catchup_mode.clone();
    data.upload_concurrency = values.upload_concurrency;
    data.quiet_hours_start = values.quiet_hours_start;
    data.quiet_hours_end = values.quiet_hours_end;
    data.upload_xmp_sidecars = values.upload_xmp_sidecars;
    data.grid_border_width = values.grid_border_width;
    data.grid_border_color = values.grid_border_color.clone();
}

impl SettingsForm {
    pub(super) fn new(
        window: &adw::ApplicationWindow,
        conn: &ConnectivityWidgets,
        behavior: &BehaviorWidgets,
        library: &LibraryWidgets,
    ) -> Self {
        Self {
            window: window.clone(),
            internal_switch: conn.internal_switch.clone(),
            external_switch: conn.external_switch.clone(),
            internal_entry: conn.internal_entry.clone(),
            external_entry: conn.external_entry.clone(),
            api_key_entry: conn.api_key_entry.clone(),
            startup_row: behavior.startup_row.clone(),
            background_sync_row: behavior.background_sync_row.clone(),
            metered_row: behavior.metered_row.clone(),
            battery_row: behavior.battery_row.clone(),
            notifications_row: behavior.notifications_row.clone(),
            library_view_row: behavior.library_view_row.clone(),
            catchup_row: behavior.catchup_row.clone(),
            concurrency_row: behavior.concurrency_row.clone(),
            xmp_sidecar_row: behavior.xmp_sidecar_row.clone(),
            quiet_hours_row: behavior.quiet_hours_row.clone(),
            quiet_start_row: behavior.quiet_start_row.clone(),
            quiet_end_row: behavior.quiet_end_row.clone(),
            preview_full_row: library.preview_full_row.clone(),
            raw_full_decode_row: library.raw_full_decode_row.clone(),
            raw_cache_row: library.raw_cache_row.clone(),
            disk_cache_row: library.disk_cache_row.clone(),
            border_width_row: library.border_width_row.clone(),
            border_color_btn: library.border_color_btn.clone(),
        }
    }

    pub(super) fn connection_values(&self) -> ConnectionValues {
        ConnectionValues {
            internal_url_enabled: self.internal_switch.is_active(),
            external_url_enabled: self.external_switch.is_active(),
            internal_url: self.internal_entry.text().to_string(),
            external_url: self.external_entry.text().to_string(),
            api_key: self.api_key_entry.text().to_string(),
        }
    }

    pub(super) fn values(&self) -> SettingsValues {
        let quiet_hours_enabled = self.quiet_hours_row.is_active();
        SettingsValues {
            run_on_startup: self.startup_row.is_active(),
            background_sync_enabled: self.background_sync_row.is_active(),
            pause_on_metered_network: self.metered_row.is_active(),
            pause_on_battery_power: self.battery_row.is_active(),
            notifications_enabled: self.notifications_row.is_active(),
            library_view_enabled: self.library_view_row.is_active(),
            library_preview_full_resolution: self.preview_full_row.is_active(),
            raw_decode_cache_enabled: self.raw_cache_row.is_active(),
            raw_full_decode: self.raw_full_decode_row.is_active(),
            cache_disk_cap_mb: self.disk_cache_row.value() as u32,
            upload_concurrency: self.concurrency_row.value() as u8,
            quiet_hours_start: quiet_hours_enabled.then(|| self.quiet_start_row.value() as u8),
            quiet_hours_end: quiet_hours_enabled.then(|| self.quiet_end_row.value() as u8),
            upload_xmp_sidecars: self.xmp_sidecar_row.is_active(),
            grid_border_width: self.border_width_row.value() as f32,
            grid_border_color: self.border_color_btn.rgba().to_string(),
            catchup_mode: catchup_from_index(self.catchup_row.selected()),
        }
    }

    /// Fill the form from saved config.
    pub(super) fn populate(&self, config: &Config) {
        let data = &config.data;
        self.internal_switch.set_active(data.internal_url_enabled);
        self.external_switch.set_active(data.external_url_enabled);
        self.internal_entry.set_text(&data.internal_url);
        self.external_entry.set_text(&data.external_url);
        self.internal_entry.set_sensitive(data.internal_url_enabled);
        self.external_entry.set_sensitive(data.external_url_enabled);
        self.startup_row.set_active(data.run_on_startup);
        self.metered_row.set_active(data.pause_on_metered_network);
        self.battery_row.set_active(data.pause_on_battery_power);
        self.background_sync_row
            .set_active(data.background_sync_enabled);
        self.notifications_row
            .set_active(data.notifications_enabled);
        self.library_view_row.set_active(data.library_view_enabled);
        self.preview_full_row
            .set_active(data.library_preview_full_resolution);
        self.raw_cache_row.set_active(data.raw_decode_cache_enabled);
        self.raw_full_decode_row.set_active(data.raw_full_decode);
        self.raw_cache_row.set_sensitive(data.raw_full_decode);
        // 0 means "no cap"; keep the spin row's default rather than showing 0.
        if data.cache_disk_cap_mb > 0 {
            self.disk_cache_row.set_value(data.cache_disk_cap_mb as f64);
        }
        self.concurrency_row
            .set_value(data.upload_concurrency as f64);
        self.xmp_sidecar_row.set_active(data.upload_xmp_sidecars);
        self.border_width_row
            .set_value(data.grid_border_width as f64);
        if let Ok(color) = data.grid_border_color.parse::<gdk4::RGBA>() {
            self.border_color_btn.set_rgba(&color);
        }
        self.populate_quiet_hours(data);
        self.catchup_row
            .set_selected(catchup_index(&data.startup_catchup_mode));
        if let Some(key) = config.get_api_key() {
            self.api_key_entry.set_text(&key);
        }
    }

    /// Quiet hours default to 22:00–07:00 when first enabled.
    fn populate_quiet_hours(&self, data: &ConfigData) {
        let enabled = data.quiet_hours_start.is_some();
        self.quiet_hours_row.set_active(enabled);
        self.quiet_start_row
            .set_value(data.quiet_hours_start.unwrap_or(22) as f64);
        self.quiet_end_row
            .set_value(data.quiet_hours_end.unwrap_or(7) as f64);
        self.quiet_start_row.set_sensitive(enabled);
        self.quiet_end_row.set_sensitive(enabled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> SettingsValues {
        SettingsValues {
            run_on_startup: true,
            background_sync_enabled: false,
            pause_on_metered_network: true,
            pause_on_battery_power: false,
            notifications_enabled: true,
            library_view_enabled: true,
            library_preview_full_resolution: false,
            raw_decode_cache_enabled: true,
            raw_full_decode: false,
            cache_disk_cap_mb: 512,
            upload_concurrency: 4,
            quiet_hours_start: Some(22),
            quiet_hours_end: Some(7),
            upload_xmp_sidecars: true,
            grid_border_width: 2.5,
            grid_border_color: "rgb(1,2,3)".into(),
            catchup_mode: StartupCatchupMode::RecentOnly,
        }
    }

    fn conn() -> ConnectionValues {
        ConnectionValues {
            internal_url_enabled: true,
            external_url_enabled: false,
            internal_url: "http://192.168.1.50:2283".into(),
            external_url: "https://photos.example.com".into(),
            api_key: "secret".into(),
        }
    }

    #[test]
    fn apply_to_config_copies_every_field() {
        let mut data = ConfigData::default();
        let watch = vec![WatchPathEntry::Simple("/photos".into())];
        apply_to_config(&mut data, &conn(), &values(), watch);
        assert!(data.internal_url_enabled);
        assert!(!data.external_url_enabled);
        assert_eq!(data.external_url, "https://photos.example.com");
        assert_eq!(data.watch_paths.len(), 1);
        assert!(data.run_on_startup);
        assert!(!data.background_sync_enabled);
        assert_eq!(data.cache_disk_cap_mb, 512);
        assert_eq!(data.upload_concurrency, 4);
        assert_eq!(data.quiet_hours_start, Some(22));
        assert_eq!(data.startup_catchup_mode, StartupCatchupMode::RecentOnly);
        assert_eq!(data.grid_border_color, "rgb(1,2,3)");
    }

    #[test]
    fn runtime_urls_blank_out_disabled_addresses() {
        assert_eq!(
            conn().runtime_urls(),
            ("http://192.168.1.50:2283".to_string(), String::new())
        );
    }

    #[test]
    fn validate_connection_rejects_bad_schemes_only_when_enabled() {
        assert_eq!(validate_connection(&conn()), Ok(()));
        let bad_internal = ConnectionValues {
            internal_url: "ftp://nas".into(),
            ..conn()
        };
        assert_eq!(
            validate_connection(&bad_internal).unwrap_err().0,
            "Invalid Internal URL"
        );
        let disabled_bad = ConnectionValues {
            external_url: "ftp://nas".into(),
            ..conn()
        };
        assert_eq!(validate_connection(&disabled_bad), Ok(()));
    }
}
