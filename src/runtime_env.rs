//! Provides best-effort checks of system conditions to determine when uploads should be deferred.
//!
//! Reads `/sys/class/power_supply` to detect battery state, and follows GIO's network
//! monitor (the network portal inside the Flatpak) for metered connections. All checks
//! are non-fatal: if a source is unavailable, the condition is assumed to be clear.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use gtk::gio::prelude::*;

static METERED: AtomicBool = AtomicBool::new(false);

/// Follow the system's metered-network state. Call once on the GTK main thread.
pub fn watch_metered_network() {
    let monitor = gtk::gio::NetworkMonitor::default();
    METERED.store(monitor.is_network_metered(), Ordering::Relaxed);
    monitor.connect_network_metered_notify(|monitor| {
        let metered = monitor.is_network_metered();
        log::debug!("Metered network: {}", metered);
        METERED.store(metered, Ordering::Relaxed);
    });
}

/// Returns true if the current network connection is metered.
pub fn is_metered_connection() -> bool {
    METERED.load(Ordering::Relaxed)
}

/// Returns true if the system appears to be running on battery power.
pub fn is_on_battery_power() -> bool {
    let power_supply_root = Path::new("/sys/class/power_supply");
    let entries = match fs::read_dir(power_supply_root) {
        Ok(entries) => entries,
        Err(_) => return false,
    };

    let mut statuses = Vec::new();

    for entry in entries.flatten() {
        let supply_path = entry.path();
        let supply_type = fs::read_to_string(supply_path.join("type"))
            .ok()
            .map(|value| value.trim().to_string());
        let online = fs::read_to_string(supply_path.join("online"))
            .ok()
            .map(|value| value.trim() == "1");
        let status = fs::read_to_string(supply_path.join("status"))
            .ok()
            .map(|value| value.trim().to_ascii_lowercase());
        statuses.push((supply_type, online, status));
    }

    is_on_battery_power_from_statuses(&statuses)
}

/// Determine if running on battery based on parsed power supply types and online statuses.
fn is_on_battery_power_from_statuses(
    statuses: &[(Option<String>, Option<bool>, Option<String>)],
) -> bool {
    let mut found_battery = false;
    let mut mains_online = false;

    for (supply_type, online, status) in statuses {
        match supply_type.as_deref() {
            Some("Mains") | Some("USB") | Some("USB_C") if online.unwrap_or(false) => {
                mains_online = true;
            }
            Some("Battery") => {
                found_battery = true;
                let status = status.as_deref().unwrap_or_default();
                if status == "charging" || status == "full" {
                    return false;
                }
            }
            _ => {}
        }
    }

    found_battery && !mains_online
}

#[cfg(test)]
mod tests {
    use super::{is_metered_connection, is_on_battery_power, is_on_battery_power_from_statuses};

    #[test]
    fn runtime_checks_are_safe() {
        let _ = is_metered_connection();
        let _ = is_on_battery_power();
    }

    #[test]
    fn test_battery_power_detection_from_statuses() {
        assert!(is_on_battery_power_from_statuses(&[
            (Some("Battery".into()), None, Some("discharging".into())),
            (Some("Mains".into()), Some(false), None),
        ]));

        assert!(!is_on_battery_power_from_statuses(&[
            (Some("Battery".into()), None, Some("charging".into())),
            (Some("Mains".into()), Some(true), None),
        ]));

        assert!(!is_on_battery_power_from_statuses(&[(
            Some("Mains".into()),
            Some(true),
            None,
        )]));
    }
}
