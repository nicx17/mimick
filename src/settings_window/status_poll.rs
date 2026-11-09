//! Status page refresh: polls in-memory app state and updates the dashboard rows.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::config::Config;
use crate::state_manager::{AppState, FolderSyncStatus};

use super::status::{StatusWidgets, show_pill};
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
    route: Option<String>,
    last_successful_sync_at: Option<f64>,
    last_error: Option<String>,
    last_error_guidance: Option<String>,
    /// No server is saved yet: first run, before Save and Connect.
    needs_setup: bool,
}

impl StatusSnapshot {
    fn from_state(s: &AppState, needs_setup: bool) -> Self {
        Self {
            needs_setup,
            status: s.status.clone(),
            progress: s.progress,
            processed: s.processed_count,
            total: s.total_queued,
            failed: s.failed_count,
            current_file: s.current_file.clone().unwrap_or_else(|| "...".to_string()),
            paused: s.paused,
            pause_reason: s.pause_reason.clone(),
            route: s.active_server_route.clone(),
            last_successful_sync_at: s.last_successful_sync_at,
            last_error: s.last_error.clone(),
            last_error_guidance: s.last_error_guidance.clone(),
        }
    }
}

/// Refresh the status page twice a second. No disk I/O; the timer stops once the
/// window is closed and the status widgets are taken out of it.
pub(super) fn start_status_poller(
    widgets: StatusWidgets,
    tracked_rows: Rc<RefCell<Vec<FolderRowData>>>,
    shared_state: Arc<parking_lot::Mutex<AppState>>,
    config: Arc<parking_lot::RwLock<Config>>,
) {
    // `None` so the first tick draws the list even when there are no folders yet.
    let shown_paths: RefCell<Option<Vec<String>>> = RefCell::default();
    glib::timeout_add_local(Duration::from_millis(500), move || {
        if widgets.title.root().is_none() {
            return glib::ControlFlow::Break;
        }
        let paths: Vec<String> = tracked_rows
            .borrow()
            .iter()
            .map(|r| r.path.clone())
            .collect();
        let needs_setup = !config.read().data.has_server();
        let (snapshot, folder_lines) = {
            let state = shared_state.lock();
            let lines: Vec<String> = paths
                .iter()
                .map(|path| folder_status_line(state.folder_statuses.get(path)))
                .collect();
            (StatusSnapshot::from_state(&state, needs_setup), lines)
        };
        widgets.show(&snapshot);
        if shown_paths.borrow().as_ref() != Some(&paths) {
            rebuild_folder_rows(&widgets.folders_list, &paths);
            *shown_paths.borrow_mut() = Some(paths);
        }
        update_folder_rows(&widgets.folders_list, &folder_lines);
        glib::ControlFlow::Continue
    });
}

impl StatusWidgets {
    fn show(&self, s: &StatusSnapshot) {
        if let Some(headline) = status_headline(s) {
            self.icon.set_icon_name(Some(headline.icon));
            self.title.set_label(&headline.title);
            self.subtitle.set_label(&headline.subtitle);
            self.subtitle.set_visible(!headline.subtitle.is_empty());
            self.progress_bar.set_visible(headline.progress.is_some());
            self.progress_bar
                .set_fraction(headline.progress.unwrap_or(0.0));
        }
        // Before setup the sync actions have nothing to act on; Set Up leads to Settings.
        for button in [&self.sync_now_btn, &self.pause_btn, &self.queue_btn] {
            show_pill(button, !s.needs_setup);
        }
        show_pill(&self.setup_btn, s.needs_setup);
        self.pause_btn.set_label(&if s.paused {
            gettext("Resume")
        } else {
            gettext("Pause")
        });
        self.failed_row.set_visible(s.failed > 0);
        self.failed_row.set_title(&failed_title(s.failed));
        self.error_row.set_visible(s.last_error.is_some());
        self.error_row
            .set_title(s.last_error.as_deref().unwrap_or_default());
        self.error_row
            .set_subtitle(s.last_error_guidance.as_deref().unwrap_or_default());
        self.attention_group
            .set_visible(!s.needs_setup && (s.failed > 0 || s.last_error.is_some()));
        self.route_row.set_subtitle(&if s.needs_setup {
            gettext("No server set up yet")
        } else {
            route_subtitle(s.route.as_deref())
        });
        self.last_sync_row
            .set_subtitle(&format_sync_age(s.last_successful_sync_at));
    }
}

fn failed_title(failed: usize) -> String {
    fill(
        ngettext("{n} upload failed", "{n} uploads failed", plural_n(failed)),
        &[("n", &failed.to_string())],
    )
}

/// One row per watched folder, or a hint to add one.
fn rebuild_folder_rows(list: &gtk::ListBox, paths: &[String]) {
    list.remove_all();
    if paths.is_empty() {
        list.append(
            &libadwaita::ActionRow::builder()
                .title(gettext("No folders yet"))
                .subtitle(gettext("Add one under Settings → Watch Folders."))
                .build(),
        );
        return;
    }
    for path in paths {
        let row = libadwaita::ActionRow::builder()
            .title(crate::watch_path_display::display_watch_path(path))
            .title_lines(1)
            .use_markup(false)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
        list.append(&row);
    }
}

fn update_folder_rows(list: &gtk::ListBox, lines: &[String]) {
    let mut child = list.first_child();
    for line in lines {
        let Some(widget) = child else {
            return;
        };
        if let Some(row) = widget.downcast_ref::<libadwaita::ActionRow>() {
            row.set_subtitle(line);
        }
        child = widget.next_sibling();
    }
}

fn route_subtitle(route: Option<&str>) -> String {
    match route {
        Some("LAN") => gettext("Connected through LAN"),
        Some("WAN") => gettext("Connected through WAN"),
        Some(_) => gettext("Connected through configured server"),
        None => gettext("Waiting for a successful connection check"),
    }
}

/// What the status headline shows.
struct Headline {
    icon: &'static str,
    title: String,
    subtitle: String,
    /// Shown as a progress bar while uploading.
    progress: Option<f64>,
}

/// Headline for the current sync state; `None` for states that keep the last one.
fn status_headline(s: &StatusSnapshot) -> Option<Headline> {
    if s.needs_setup {
        return Some(Headline {
            icon: "network-server-symbolic",
            title: gettext("Not Set Up Yet"),
            subtitle: gettext("Connect Mimick to your Immich server to start."),
            progress: None,
        });
    }
    if s.status == "paused" || s.paused {
        return Some(Headline {
            icon: "media-playback-pause-symbolic",
            title: gettext("Paused"),
            subtitle: s.pause_reason.clone().unwrap_or_default(),
            progress: None,
        });
    }
    match s.status.as_str() {
        // Idle with no route and an error: nothing is being uploaded because Immich is unreachable.
        "idle" if s.route.is_none() && s.last_error.is_some() => Some(Headline {
            icon: "network-offline-symbolic",
            title: gettext("Can't Reach Immich"),
            subtitle: gettext("Check the server address and API key in Settings."),
            progress: None,
        }),
        "idle" if s.failed > 0 => Some(Headline {
            icon: "network-offline-symbolic",
            title: gettext("Waiting for Immich"),
            subtitle: gettext("Failed uploads retry when the server is reachable."),
            progress: None,
        }),
        "idle" => Some(Headline {
            icon: "mimick-check-circle-symbolic",
            title: gettext("Up to Date"),
            subtitle: match s.last_successful_sync_at {
                Some(_) => fill(
                    gettext("Last upload {when}"),
                    &[(
                        "when",
                        &lowercase_first(&format_sync_age(s.last_successful_sync_at)),
                    )],
                ),
                None => String::new(),
            },
            progress: None,
        }),
        "uploading" => Some(Headline {
            icon: "mimick-upload-symbolic",
            title: fill(
                gettext("Uploading {done} of {total}"),
                &[
                    ("done", &s.processed.to_string()),
                    ("total", &s.total.to_string()),
                ],
            ),
            subtitle: std::path::Path::new(&s.current_file)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            progress: Some(f64::from(s.progress) / 100.0),
        }),
        _ => None,
    }
}

/// Lowercases only the first letter, so a phrase can sit mid-sentence without breaking German nouns.
fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_lowercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// One folder's status line: its error, or pending count and last sync time.
fn folder_status_line(status: Option<&FolderSyncStatus>) -> String {
    let Some(status) = status else {
        return gettext("Up to date");
    };
    if let Some(err) = &status.last_error {
        return fill(gettext("Problem: {err}"), &[("err", err)]);
    }
    let mut line = match status.pending_count {
        0 => gettext("Up to date"),
        n => fill(
            ngettext(
                "{n} waiting to upload",
                "{n} waiting to upload",
                plural_n(n),
            ),
            &[("n", &n.to_string())],
        ),
    };
    if let Some(t) = status.last_sync_at {
        line.push_str(" · ");
        line.push_str(&fill(
            gettext("last upload {when}"),
            &[("when", &lowercase_first(&format_sync_age(Some(t))))],
        ));
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
            pause_reason: Some("Paused by user".into()),
            ..StatusSnapshot::default()
        };
        let headline = status_headline(&paused).unwrap();
        assert_eq!(headline.title, "Paused");
        assert_eq!(headline.subtitle, "Paused by user");
        assert!(headline.progress.is_none());

        let waiting = StatusSnapshot {
            status: "idle".into(),
            failed: 2,
            ..StatusSnapshot::default()
        };
        assert_eq!(
            status_headline(&waiting).unwrap().title,
            "Waiting for Immich"
        );

        let idle = StatusSnapshot {
            status: "idle".into(),
            ..StatusSnapshot::default()
        };
        let headline = status_headline(&idle).unwrap();
        assert_eq!(headline.title, "Up to Date");
        assert_eq!(headline.subtitle, "");

        let uploading = StatusSnapshot {
            status: "uploading".into(),
            processed: 1,
            total: 3,
            progress: 40,
            current_file: "/photos/a.jpg".into(),
            ..StatusSnapshot::default()
        };
        let headline = status_headline(&uploading).unwrap();
        assert_eq!(headline.title, "Uploading 1 of 3");
        assert_eq!(headline.subtitle, "a.jpg");
        assert_eq!(headline.progress, Some(0.4));

        let unknown = StatusSnapshot {
            status: "starting".into(),
            ..StatusSnapshot::default()
        };
        assert!(status_headline(&unknown).is_none());
    }

    #[test]
    fn headline_says_when_immich_is_unreachable() {
        let offline = StatusSnapshot {
            status: "idle".into(),
            last_error: Some("Could not reach the Immich server".into()),
            ..StatusSnapshot::default()
        };
        assert_eq!(
            status_headline(&offline).unwrap().title,
            "Can't Reach Immich"
        );
        let connected = StatusSnapshot {
            route: Some("LAN".into()),
            ..offline
        };
        assert_eq!(status_headline(&connected).unwrap().title, "Up to Date");
    }

    #[test]
    fn headline_asks_for_setup_before_anything_else() {
        let first_run = StatusSnapshot {
            status: "idle".into(),
            last_error: Some("Could not reach the Immich server".into()),
            needs_setup: true,
            ..StatusSnapshot::default()
        };
        let headline = status_headline(&first_run).unwrap();
        assert_eq!(headline.title, "Not Set Up Yet");
        assert_eq!(headline.icon, "network-server-symbolic");
    }

    #[test]
    fn folder_status_line_prefers_errors() {
        assert_eq!(folder_status_line(None), "Up to date");
        let failing = FolderSyncStatus {
            last_error: Some("Permission lost".into()),
            pending_count: 3,
            ..FolderSyncStatus::default()
        };
        assert_eq!(
            folder_status_line(Some(&failing)),
            "Problem: Permission lost"
        );
        let pending = FolderSyncStatus {
            pending_count: 3,
            ..FolderSyncStatus::default()
        };
        assert_eq!(folder_status_line(Some(&pending)), "3 waiting to upload");
    }
}
