//! Handles autostart integration for both native installations and sandboxed Flatpak builds.
//!
//! Under Flatpak, uses the XDG Background portal to request autostart
//! permission. On bare-metal installs and AppImages, writes or removes a `.desktop`
//! file in `~/.config/autostart/`. The portal request is asynchronous
//! and may be denied by the user.

use crate::i18n::fill;
use ashpd::WindowIdentifier;
use ashpd::desktop::background::Background;
use gettextrs::gettext;
use gtk::prelude::*;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

const AUTOSTART_DESKTOP_ID: &str = "dev.nicx.mimick.desktop";
const APP_ID: &str = "dev.nicx.mimick";
const NATIVE_COMMAND: &str = "mimick";

/// Configure or request autostart registration depending on container and integration style.
pub async fn apply(window: &impl IsA<gtk::Window>, enable: bool) -> Result<bool, String> {
    if is_flatpak_sandbox() {
        request_background_portal(window, enable).await
    } else if enable {
        install_desktop_entry().map(|_| true)
    } else {
        remove_desktop_entry().map(|_| false)
    }
}

/// Check if running inside Flatpak sandbox environment.
fn is_flatpak_sandbox() -> bool {
    Path::new("/.flatpak-info").exists()
}

/// Request autostart state change through the XDG Background portal.
async fn request_background_portal(
    window: &impl IsA<gtk::Window>,
    enable: bool,
) -> Result<bool, String> {
    let identifier = match window.as_ref().native() {
        Some(native) => WindowIdentifier::from_native(&native).await,
        None => None,
    };

    let response = Background::request()
        .identifier(identifier)
        .reason(
            gettext("Reason for requesting background access: Mimick must run in the background to automatically sync media to Immich.")
                .as_str(),
        )
        .auto_start(enable)
        .dbus_activatable(false)
        .send()
        .await
        .map_err(|err| fill(gettext("Failed to contact the background portal: {err}"), &[("err", &err.to_string())]))?
        .response()
        .map_err(|err| fill(gettext("The desktop rejected the autostart request: {err}"), &[("err", &err.to_string())]))?;

    if enable {
        Ok(response.auto_start() && response.run_in_background())
    } else {
        Ok(response.auto_start())
    }
}

/// Install autostart desktop shortcut entry in standard non-flatpak configurations.
fn install_desktop_entry() -> Result<(), String> {
    let entry_path = default_autostart_entry_path()?;
    if let Some(parent) = entry_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            fill(
                gettext("Failed to create autostart directory: {err}"),
                &[("err", &err.to_string())],
            )
        })?;
    }

    let escaped_exec = escape_desktop_exec_arg(&launcher_command(std::env::var_os("APPIMAGE")));

    let desktop_entry = format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Mimick\nComment=Unofficial Immich desktop client and auto-sync agent\nExec={escaped_exec}\nIcon=dev.nicx.mimick\nTerminal=false\nCategories=Utility;\nX-GNOME-Autostart-enabled=true\nStartupNotify=false\n"
    );

    fs::write(&entry_path, desktop_entry).map_err(|err| {
        fill(
            gettext("Failed to write autostart entry: {err}"),
            &[("err", &err.to_string())],
        )
    })?;

    Ok(())
}

/// The command the autostart entry runs, given the value of `$APPIMAGE`.
///
/// From an AppImage it's the `.AppImage` file the runtime exports, since the binary itself runs
/// from a temporary mount that changes on every launch. Otherwise it's `mimick` on `PATH`, like
/// the app's own desktop file, rather than `current_exe()`, which Semgrep flags as untrusted.
fn launcher_command(appimage: Option<OsString>) -> String {
    appimage.filter(|path| !path.is_empty()).map_or_else(
        || NATIVE_COMMAND.to_string(),
        |path| path.to_string_lossy().into_owned(),
    )
}

/// Remove autostart desktop shortcut entry if one exists.
fn remove_desktop_entry() -> Result<(), String> {
    for entry_path in autostart_entry_paths()? {
        if entry_path.exists() {
            fs::remove_file(&entry_path).map_err(|err| {
                fill(
                    gettext("Failed to remove autostart entry: {err}"),
                    &[("err", &err.to_string())],
                )
            })?;
        }
    }
    Ok(())
}

/// Retrieve default system autostart configuration shortcut folder path.
fn default_autostart_entry_path() -> Result<PathBuf, String> {
    let config_dir =
        dirs::config_dir().ok_or_else(|| gettext("Could not locate the user config directory."))?;
    Ok(config_dir.join("autostart").join(AUTOSTART_DESKTOP_ID))
}

/// Retrieve possible system autostart shortcut path list (sandboxed and unsandboxed).
fn autostart_entry_paths() -> Result<Vec<PathBuf>, String> {
    let config_dir =
        dirs::config_dir().ok_or_else(|| gettext("Could not locate the user config directory."))?;
    let mut paths = vec![config_dir.join("autostart").join(AUTOSTART_DESKTOP_ID)];

    if let Some(host_config_dir) = flatpak_host_config_dir_from(&config_dir) {
        paths.push(host_config_dir.join("autostart").join(AUTOSTART_DESKTOP_ID));
    }

    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Map a sandboxed `~/.var/app/<app-id>/config` path back to the host `~/.config` path.
fn flatpak_host_config_dir_from(config_dir: &Path) -> Option<PathBuf> {
    let app_dir = config_dir.parent()?;
    let app_parent = app_dir.parent()?;
    let var_dir = app_parent.parent()?;
    let host_home = var_dir.parent()?;

    if config_dir.file_name()? != "config" {
        return None;
    }
    if app_dir.file_name()? != APP_ID {
        return None;
    }
    if app_parent.file_name()? != "app" {
        return None;
    }
    if var_dir.file_name()? != ".var" {
        return None;
    }

    Some(host_home.join(".config"))
}

/// Escape an executable path for the `Exec=` field of a desktop entry.
fn escape_desktop_exec_arg(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            ' ' => escaped.push_str("\\ "),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '"' => escaped.push_str("\\\""),
            '\'' => escaped.push_str("\\'"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::{escape_desktop_exec_arg, flatpak_host_config_dir_from, launcher_command};
    use std::ffi::OsString;
    use std::path::Path;

    #[test]
    fn launcher_command_uses_the_appimage_file() {
        assert_eq!(
            launcher_command(Some(OsString::from("/home/user/Apps/Mimick.AppImage"))),
            "/home/user/Apps/Mimick.AppImage"
        );
    }

    #[test]
    fn launcher_command_falls_back_to_mimick_on_path() {
        assert_eq!(launcher_command(None), "mimick");
        assert_eq!(launcher_command(Some(OsString::new())), "mimick");
    }

    #[test]
    fn test_escape_desktop_exec_arg() {
        assert_eq!(
            escape_desktop_exec_arg("/tmp/My App/mimick"),
            "/tmp/My\\ App/mimick"
        );
    }

    #[test]
    fn test_flatpak_host_config_dir_from_sandbox_path() {
        assert_eq!(
            flatpak_host_config_dir_from(Path::new("/home/user/.var/app/dev.nicx.mimick/config"))
                .unwrap(),
            Path::new("/home/user/.config")
        );
    }
}
