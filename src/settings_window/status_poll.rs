//! Status page refresh: polls in-memory app state and updates the dashboard rows.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::state_manager::{AppState, FolderSyncStatus};

use super::status::StatusWidgets;
use super::{FolderRowData, format_sync_age};

/// Values the status page shows, copied out so the state lock isn't held while touching GTK.
#[derive(Debug, Clone, Default)]
struct StatusSnapshot {
    status: String,
    progress: u8,
    processed: usize,
    total: usize,
    failed: usize,
    current_file: String,
    paused: bool,
    pause_reason: Option<String>,
    pending: usize,
    route: Option<String>,
    watched_folder_count: usize,
    last_successful_sync_at: Option<f64>,
    last_error: Option<String>,
    last_error_guidance: Option<String>,
}

impl StatusSnapshot {
    fn from_state(s: &AppState) -> Self {
        Self {
            status: s.status.clone(),
            progress: s.progress,
            processed: s.processed_count,
            total: s.total_queued,
            failed: s.failed_count,
            current_file: s.current_file.clone().unwrap_or_else(|| "...".to_string()),
            paused: s.paused,
            pause_reason: s.pause_reason.clone(),
            pending: s.queue_size,
            route: s.active_server_route.clone(),
            watched_folder_count: s.watched_folder_count,
            last_successful_sync_at: s.last_successful_sync_at,
            last_error: s.last_error.clone(),
            last_error_guidance: s.last_error_guidance.clone(),
        }
    }
}

/// Refresh the status page twice a second. No disk I/O; the timer stops once the
/// status widgets are destroyed with the window.
pub(super) fn start_status_poller(
    widgets: &StatusWidgets,
    pause_btn: &gtk::Button,
    tracked_rows: Rc<RefCell<Vec<FolderRowData>>>,
    shared_state: Arc<parking_lot::Mutex<AppState>>,
) {
    let weak_pause = pause_btn.downgrade();
    let rows = StatusRows::from_widgets(widgets);
    glib::timeout_add_local(Duration::from_millis(500), move || {
        let (Some(rows), Some(pause_btn)) = (rows.upgrade(), weak_pause.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        let (snapshot, folder_subtitles) = {
            let state = shared_state.lock();
            (
                StatusSnapshot::from_state(&state),
                folder_subtitles(&tracked_rows.borrow(), &state),
            )
        };
        pause_btn.set_label(if snapshot.paused { "Resume" } else { "Pause" });
        rows.show(&snapshot);
        for (row, subtitle) in folder_subtitles {
            row.set_subtitle(&subtitle);
        }
        glib::ControlFlow::Continue
    });
}

/// Weak handles to the status rows, upgraded on each tick.
struct StatusRows {
    status_row: glib::WeakRef<libadwaita::ActionRow>,
    progress_bar: glib::WeakRef<gtk::ProgressBar>,
    route_row: glib::WeakRef<libadwaita::ActionRow>,
    folders_row: glib::WeakRef<libadwaita::ActionRow>,
    queue_health_row: glib::WeakRef<libadwaita::ActionRow>,
    last_sync_row: glib::WeakRef<libadwaita::ActionRow>,
    error_row: glib::WeakRef<libadwaita::ActionRow>,
}

impl StatusRows {
    fn from_widgets(widgets: &StatusWidgets) -> Self {
        Self {
            status_row: widgets.status_row.downgrade(),
            progress_bar: widgets.progress_bar.downgrade(),
            route_row: widgets.route_row.downgrade(),
            folders_row: widgets.folders_row.downgrade(),
            queue_health_row: widgets.queue_health_row.downgrade(),
            last_sync_row: widgets.last_sync_row.downgrade(),
            error_row: widgets.error_row.downgrade(),
        }
    }

    fn upgrade(&self) -> Option<StatusWidgets> {
        Some(StatusWidgets {
            status_row: self.status_row.upgrade()?,
            progress_bar: self.progress_bar.upgrade()?,
            route_row: self.route_row.upgrade()?,
            folders_row: self.folders_row.upgrade()?,
            queue_health_row: self.queue_health_row.upgrade()?,
            last_sync_row: self.last_sync_row.upgrade()?,
            error_row: self.error_row.upgrade()?,
        })
    }
}

impl StatusWidgets {
    fn show(&self, s: &StatusSnapshot) {
        self.route_row
            .set_subtitle(route_subtitle(s.route.as_deref()));
        self.folders_row
            .set_subtitle(&format!("{} configured", s.watched_folder_count));
        self.queue_health_row.set_subtitle(&format!(
            "{} pending, {} waiting to retry",
            s.pending, s.failed
        ));
        self.last_sync_row
            .set_subtitle(&format_sync_age(s.last_successful_sync_at));
        self.error_row
            .set_title(s.last_error.as_deref().unwrap_or("No recent errors"));
        self.error_row.set_subtitle(
            s.last_error_guidance
                .as_deref()
                .unwrap_or("Uploads are healthy."),
        );
        // Unknown statuses leave the headline row as it was.
        if let Some((title, subtitle, fraction)) = status_headline(s) {
            self.status_row.set_title(&title);
            self.status_row.set_subtitle(&subtitle);
            self.progress_bar.set_fraction(fraction);
        }
    }
}

fn route_subtitle(route: Option<&str>) -> &'static str {
    match route {
        Some("LAN") => "Connected through LAN",
        Some("WAN") => "Connected through WAN",
        Some(_) => "Connected through configured server",
        None => "Waiting for a successful connection check",
    }
}

/// Status row title, subtitle, and progress fraction for the current sync state.
fn status_headline(s: &StatusSnapshot) -> Option<(String, String, f64)> {
    let progress = f64::from(s.progress) / 100.0;
    if s.status == "paused" || s.paused {
        let reason = s
            .pause_reason
            .as_deref()
            .unwrap_or("Sync has been temporarily paused.");
        return Some(("Paused".into(), reason.into(), progress));
    }
    match s.status.as_str() {
        // Failed items while idle are waiting for the network to come back.
        "idle" if s.failed > 0 => Some((
            "Offline / Waiting".into(),
            format!("{} item(s) pending network", s.failed),
            1.0,
        )),
        "idle" => Some((
            "Idle".into(),
            format!(
                "Successfully processed {} file(s)",
                s.processed.saturating_sub(s.failed)
            ),
            if s.processed > 0 { 1.0 } else { 0.0 },
        )),
        "uploading" => {
            let filename = std::path::Path::new(&s.current_file)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "...".to_string());
            Some((
                format!("Uploading ({}/{})", s.processed, s.total),
                filename,
                progress,
            ))
        }
        _ => None,
    }
}

fn folder_subtitles(
    rows: &[FolderRowData],
    state: &AppState,
) -> Vec<(libadwaita::ExpanderRow, String)> {
    rows.iter()
        .map(|row| {
            let line = folder_status_line(state.folder_statuses.get(&row.path));
            let subtitle = if row.base_subtitle.is_empty() {
                line
            } else {
                format!("{}\n{}", row.base_subtitle, line)
            };
            (row.action_row.clone(), subtitle)
        })
        .collect()
}

/// One folder's status line: its error, or pending count and last sync time.
fn folder_status_line(status: Option<&FolderSyncStatus>) -> String {
    let Some(status) = status else {
        return "Status: Idle".to_string();
    };
    if let Some(err) = &status.last_error {
        return format!("Error: {}", err);
    }
    let mut line = format!("Pending: {}", status.pending_count);
    if let Some(t) = status.last_sync_at {
        line.push_str(&format!(" - Last Sync: {}", format_sync_age(Some(t))));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_subtitle_names_the_connection() {
        assert_eq!(route_subtitle(Some("LAN")), "Connected through LAN");
        assert_eq!(
            route_subtitle(Some("custom")),
            "Connected through configured server"
        );
        assert_eq!(
            route_subtitle(None),
            "Waiting for a successful connection check"
        );
    }

    #[test]
    fn headline_for_paused_idle_and_uploading() {
        let paused = StatusSnapshot {
            paused: true,
            progress: 40,
            ..StatusSnapshot::default()
        };
        let (title, subtitle, fraction) = status_headline(&paused).unwrap();
        assert_eq!(title, "Paused");
        assert_eq!(subtitle, "Sync has been temporarily paused.");
        assert!((fraction - 0.4).abs() < f64::EPSILON);

        let waiting = StatusSnapshot {
            status: "idle".into(),
            failed: 2,
            ..StatusSnapshot::default()
        };
        assert_eq!(status_headline(&waiting).unwrap().0, "Offline / Waiting");

        let idle = StatusSnapshot {
            status: "idle".into(),
            processed: 5,
            failed: 0,
            ..StatusSnapshot::default()
        };
        let (_, subtitle, fraction) = status_headline(&idle).unwrap();
        assert_eq!(subtitle, "Successfully processed 5 file(s)");
        assert_eq!(fraction, 1.0);

        let uploading = StatusSnapshot {
            status: "uploading".into(),
            processed: 1,
            total: 3,
            current_file: "/photos/a.jpg".into(),
            ..StatusSnapshot::default()
        };
        let (title, subtitle, _) = status_headline(&uploading).unwrap();
        assert_eq!(title, "Uploading (1/3)");
        assert_eq!(subtitle, "a.jpg");

        let unknown = StatusSnapshot {
            status: "starting".into(),
            ..StatusSnapshot::default()
        };
        assert!(status_headline(&unknown).is_none());
    }

    #[test]
    fn folder_status_line_prefers_errors() {
        assert_eq!(folder_status_line(None), "Status: Idle");
        let failing = FolderSyncStatus {
            last_error: Some("Permission lost".into()),
            pending_count: 3,
            ..FolderSyncStatus::default()
        };
        assert_eq!(folder_status_line(Some(&failing)), "Error: Permission lost");
        let pending = FolderSyncStatus {
            pending_count: 3,
            ..FolderSyncStatus::default()
        };
        assert_eq!(folder_status_line(Some(&pending)), "Pending: 3");
    }
}
