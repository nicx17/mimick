//! Implements the GTK4/Libadwaita settings window and status dashboard.
//!
//! Builds the tabbed preferences interface covering server configuration,
//! watch-folder management, behaviour toggles, and a live health dashboard.
//! Changes are validated and persisted to the JSON config file on save.

use crate::config::{FolderRules, WatchPathEntry};
use adw::prelude::*;
use gtk::prelude::*;
use libadwaita as adw;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use crate::app_context::AppContext;

mod actions_ui;
mod apply;
mod behavior;
mod connectivity;
mod form;
mod library;
mod queue_inspector;
mod status;
mod status_poll;
mod watch_folders;
mod wiring;

pub use queue_inspector::show_queue_inspector;

/// Holds GTK widgets for a single watch-folder row in the settings list.
struct FolderRowData {
    /// Absolute local file path.
    path: String,
    /// Picker label: an album name, `DEFAULT_ALBUM_LABEL`, or `LIBRARY_ALBUM_LABEL`.
    album_name: Rc<RefCell<String>>,
    /// Whether this folder explicitly uploads directly to the library.
    uploads_to_library: Rc<Cell<bool>>,
    /// Custom path filtering rules.
    rules: Rc<RefCell<FolderRules>>,
    /// Libadwaita row widget representing this watched folder.
    action_row: adw::ExpanderRow,
    /// Base status subtitle for the row.
    base_subtitle: String,
}

const DEFAULT_ALBUM_LABEL: &str = "Default (Folder Name)";
const LIBRARY_ALBUM_LABEL: &str = "Library (No Album)";

/// Display a standard Libadwaita modal alert message dialog to the user.
fn show_alert(parent: &impl gtk::prelude::IsA<gtk::Widget>, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("ok", "OK");
    dialog.present(Some(parent));
}

/// Saved watch-path entry for one Settings folder row.
/// Library targets get no album; unnamed folders fall back to a folder-name album.
fn watch_path_entry(
    folder: String,
    album_name: &str,
    uploads_to_library: bool,
    mut rules: FolderRules,
    albums: &HashMap<String, String>,
) -> WatchPathEntry {
    let has_rules = rules != FolderRules::default();
    let is_default = album_name.is_empty() || album_name == DEFAULT_ALBUM_LABEL;
    let resolved_album_name = if uploads_to_library {
        None
    } else if is_default {
        Path::new(&folder)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .map(ToString::to_string)
    } else {
        Some(album_name.to_string())
    };

    if uploads_to_library {
        rules.restrict_to_library_uploads();
    }

    // Nothing to store beyond the path, so keep the legacy plain-string form.
    if is_default && !has_rules && resolved_album_name.is_none() {
        return WatchPathEntry::Simple(folder);
    }

    let album_id = resolved_album_name
        .as_ref()
        .and_then(|n| albums.get(n).cloned());
    // A non-UTF-8 or root folder still needs an album name: storing `None` here
    // would make the entry read back as a library-only target.
    let stored_album_name = if uploads_to_library {
        None
    } else {
        resolved_album_name.or_else(|| {
            Some(Path::new(&folder).file_name().map_or_else(
                || "Mimick".to_string(),
                |n| n.to_string_lossy().into_owned(),
            ))
        })
    };
    WatchPathEntry::WithConfig {
        path: folder,
        album_id,
        album_name: stored_album_name,
        rules,
    }
}

/// Alert text shown when the API key had to be stored in the fallback file.
fn api_key_file_notice(path: &Path) -> String {
    format!(
        "Settings were saved, but the system keyring is unavailable, so the API key \
         was stored in {}.\n\nThe file is readable only by your user account but is \
         not encrypted.\n\nSee: https://github.com/nicx17/mimick/wiki/Keyring-Setup",
        path.display()
    )
}

/// Format a Unix timestamp representing the last sync time into a relative display string.
fn format_sync_age(timestamp: Option<f64>) -> String {
    let Some(timestamp) = timestamp else {
        return "No successful sync yet".to_string();
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let elapsed = (now - timestamp).max(0.0);

    if elapsed < 60.0 {
        "Less than a minute ago".to_string()
    } else if elapsed < 3600.0 {
        format!("{} minute(s) ago", (elapsed / 60.0).floor() as u64)
    } else if elapsed < 86_400.0 {
        format!("{} hour(s) ago", (elapsed / 3600.0).floor() as u64)
    } else {
        format!("{} day(s) ago", (elapsed / 86_400.0).floor() as u64)
    }
}

/// Construct and present the settings window as a top-level window.
pub fn build_settings_window(app: &adw::Application, ctx: Arc<AppContext>) {
    build_settings_window_with_parent(app, ctx, None);
}

/// Construct and present the settings window, optionally transient for a parent window.
pub fn build_settings_window_with_parent(
    app: &adw::Application,
    ctx: Arc<AppContext>,
    parent: Option<&adw::ApplicationWindow>,
) {
    let (window, status_page, settings_page) = wiring::build_window_shell(app, parent);
    let config = ctx.config.read().clone();
    if config.get_api_key().unwrap_or_default().is_empty() {
        wiring::add_welcome_group(&settings_page);
    }

    let status_widgets = status::build_status_group(&status_page);
    let conn = connectivity::build_connectivity_group(&settings_page, &window);
    wiring::connect_test_button(&window, &conn, ctx.api_client.clone());
    let behavior = behavior::build_behavior_group(&settings_page);
    wiring::connect_quiet_hours_sensitivity(&behavior);
    let library = library::build_library_group(&settings_page);
    wiring::connect_library_rows(&ctx, &library, &behavior.library_view_row);
    wiring::connect_download_folder(&ctx, &library, &window);

    let form = form::SettingsForm::new(&window, &conn, &behavior, &library);
    let state = apply::ApplyState::new(
        &ctx,
        config.data.run_on_startup,
        config.data.background_sync_enabled,
    );
    let apply_settings = apply::make_apply_settings(&form, state.clone());
    let auto_apply: Rc<dyn Fn()> = {
        let apply_settings = apply_settings.clone();
        Rc::new(move || apply_settings(false, false))
    };
    wiring::fetch_albums_for_picker(&window, &state);
    wiring::add_folders_group(&settings_page, &window, &config, &state, auto_apply.clone());

    let actions = actions_ui::build_actions_group(&status_page, &settings_page);
    wiring::connect_action_buttons(app, &window, &ctx, &actions);
    conn.save_btn
        .connect_clicked(move |_| apply_settings(true, true));

    // Populate before connecting auto-apply so loading saved values doesn't trigger a save.
    form.populate(&config);
    wiring::connect_url_switches(&window, &conn);
    wiring::connect_auto_apply(&form, auto_apply);
    wiring::connect_close_request(&window, app, &ctx);
    status_poll::start_status_poller(
        &status_widgets,
        &actions.pause_btn,
        state.tracked_rows.clone(),
        ctx.state.clone(),
    );
    window.present();
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_ALBUM_LABEL, LIBRARY_ALBUM_LABEL, format_sync_age, watch_path_entry};
    use crate::config::{FolderRules, FolderSyncMethod, WatchPathEntry};
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn albums() -> HashMap<String, String> {
        HashMap::from([
            ("Camera".to_string(), "camera-id".to_string()),
            ("Trips".to_string(), "trips-id".to_string()),
        ])
    }

    #[test]
    fn test_watch_path_entry_library_target_has_no_album_and_restricted_rules() {
        let rules = FolderRules {
            sync_method: FolderSyncMethod::Full,
            delete_folder_to_album: true,
            ..FolderRules::default()
        };
        let entry = watch_path_entry(
            "/home/user/Camera".into(),
            LIBRARY_ALBUM_LABEL,
            true,
            rules,
            &albums(),
        );

        assert!(entry.uploads_to_library());
        assert_eq!(entry.rules().sync_method, FolderSyncMethod::UploadOnly);
        assert!(!entry.rules().delete_folder_to_album);
    }

    #[test]
    fn test_watch_path_entry_default_uses_folder_name_album() {
        let entry = watch_path_entry(
            "/home/user/Camera".into(),
            DEFAULT_ALBUM_LABEL,
            false,
            FolderRules::default(),
            &albums(),
        );

        let WatchPathEntry::WithConfig { album_id, .. } = &entry else {
            panic!("expected WithConfig, got {entry:?}");
        };
        assert_eq!(entry.album_name(), Some("Camera"));
        assert_eq!(album_id.as_deref(), Some("camera-id"));
    }

    #[test]
    fn test_watch_path_entry_explicit_album_resolves_id() {
        let entry = watch_path_entry(
            "/home/user/Camera".into(),
            "Trips",
            false,
            FolderRules::default(),
            &albums(),
        );

        let WatchPathEntry::WithConfig { album_id, .. } = &entry else {
            panic!("expected WithConfig, got {entry:?}");
        };
        assert_eq!(entry.album_name(), Some("Trips"));
        assert_eq!(album_id.as_deref(), Some("trips-id"));
    }

    #[test]
    fn test_watch_path_entry_unnamed_folder_never_becomes_library_target() {
        let simple = watch_path_entry(
            "/".into(),
            DEFAULT_ALBUM_LABEL,
            false,
            FolderRules::default(),
            &albums(),
        );
        assert!(matches!(simple, WatchPathEntry::Simple(ref path) if path == "/"));

        let with_rules = watch_path_entry(
            "/".into(),
            DEFAULT_ALBUM_LABEL,
            false,
            FolderRules {
                ignore_hidden: true,
                ..FolderRules::default()
            },
            &albums(),
        );
        assert!(!with_rules.uploads_to_library());
        assert_eq!(with_rules.album_name(), Some("Mimick"));
    }

    #[test]
    fn test_format_sync_age_for_missing_timestamp() {
        assert_eq!(format_sync_age(None), "No successful sync yet");
    }

    #[test]
    fn test_format_sync_age_for_recent_timestamp() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        assert_eq!(format_sync_age(Some(now - 30.0)), "Less than a minute ago");
    }
}
