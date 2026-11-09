//! Mimick's About dialog: What's New, links, credits, the license, and troubleshooting
//! details to paste into an issue.

use std::path::Path;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::prelude::*;
use libadwaita as adw;

/// The metainfo, compiled into the app's resources by `build.rs`. It supplies the name,
/// icon, website, issue tracker, license, source links, and each release's notes.
const METAINFO: &str = "/dev/nicx/mimick/dev.nicx.mimick.metainfo.xml";
const DISCUSSIONS: &str = "https://github.com/nicx17/mimick/discussions";
const WIKI: &str = "https://github.com/nicx17/mimick/wiki";
const BUG_REPORT: &str = "https://github.com/nicx17/mimick/issues/new?template=bug_report.yml";

const ATTRIBUTION: [&str; 7] = [
    "App icon by Round Icons https://unsplash.com/illustrations/a-white-and-orange-flower-on-a-white-background-IkQ_WrJzZOM",
    "OpenStreetMap contributors https://www.openstreetmap.org/copyright",
    "OpenTopoMap (CC-BY-SA) https://opentopomap.org",
    "Humanitarian OpenStreetMap Team, hosted by OpenStreetMap France https://www.hotosm.org",
    "VersaTiles https://versatiles.org",
    "VersaTiles satellite imagery sources https://versatiles.org/sources/",
    "Photon by komoot, Apache License 2.0 https://github.com/komoot/photon",
];

const BUILT_WITH: [&str; 9] = [
    "GTK https://www.gtk.org",
    "libadwaita https://gnome.pages.gitlab.gnome.org/libadwaita/",
    "libshumate https://gitlab.gnome.org/GNOME/libshumate",
    "GStreamer https://gstreamer.freedesktop.org",
    "libheif https://github.com/strukturag/libheif",
    "LibRaw https://www.libraw.org",
    "libjpeg-turbo https://www.libjpeg-turbo.org",
    "libwebp https://chromium.googlesource.com/webm/libwebp",
    "OpenJPEG https://www.openjpeg.org",
];

pub fn present(parent: &impl IsA<gtk::Widget>) {
    let version = env!("CARGO_PKG_VERSION");
    // Given the running version, the dialog shows that release's notes as What's New.
    let about = adw::AboutDialog::from_appdata(METAINFO, Some(version));
    about.set_version(version);
    // The metainfo's help link is the wiki; questions belong in Discussions.
    about.set_support_url(DISCUSSIONS);
    // Report an Issue opens the bug form, which asks for this dialog's debug info.
    about.set_issue_url(BUG_REPORT);
    about.add_link(&gettext("Documentation"), WIKI);
    about.set_developers(&["Nick Cardoso <nick@nickcardoso.com>"]);
    about.add_acknowledgement_section(Some(&gettext("Attribution")), &ATTRIBUTION);
    about.add_acknowledgement_section(
        Some(&gettext("Special Thanks")),
        &["Immich https://immich.app"],
    );
    about.add_acknowledgement_section(Some(&gettext("Built With")), &BUILT_WITH);
    about.set_debug_info(&debug_info(parent.as_ref()));
    about.set_debug_info_filename("mimick-debug-info.txt");
    about.present(Some(parent));
}

/// Where Mimick runs and how it draws, for bug reports. Nothing personal: no server,
/// account, or paths.
fn debug_info(widget: &gtk::Widget) -> String {
    let renderer = widget
        .native()
        .and_then(|native| native.renderer())
        .map_or_else(|| "unknown".to_string(), |r| r.type_().name().to_string());
    let mut lines = vec![format!("Mimick {}", env!("CARGO_PKG_VERSION"))];
    lines.extend(environment_lines());
    // libadwaita's getters assert they run on GTK's thread, so only the dialog adds them.
    lines.push(format!(
        "libadwaita: {}.{}.{}",
        adw::major_version(),
        adw::minor_version(),
        adw::micro_version()
    ));
    lines.push(format!("Renderer: {renderer}"));
    lines.join("\n")
}

/// Package, GTK version, and desktop, shared with the diagnostics bundle's summary. Safe off
/// the main thread, where the bundle is written.
pub(crate) fn environment_lines() -> Vec<String> {
    vec![
        format!("Package: {}", packaging()),
        format!("Profile: {}", crate::profile::name().unwrap_or("default")),
        format!(
            "GTK: {}.{}.{}",
            gtk::major_version(),
            gtk::minor_version(),
            gtk::micro_version()
        ),
        format!("OS: {}", os_name()),
        format!("Desktop: {}", env_or_unknown("XDG_CURRENT_DESKTOP")),
        format!("Session: {}", env_or_unknown("XDG_SESSION_TYPE")),
        format!("Language: {}", env_or_unknown("LANG")),
    ]
}

fn packaging() -> &'static str {
    if Path::new("/.flatpak-info").exists() {
        "Flatpak"
    } else if std::env::var_os("APPIMAGE").is_some() {
        "AppImage"
    } else {
        "native"
    }
}

/// The host's name for itself; inside Flatpak, /etc describes the runtime instead.
fn os_name() -> String {
    ["/run/host/os-release", "/etc/os-release"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| pretty_name(&text))
        .unwrap_or_else(|| "unknown".to_string())
}

fn pretty_name(os_release: &str) -> Option<String> {
    os_release
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim_matches('"').to_string())
}

fn env_or_unknown(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::pretty_name;

    #[test]
    fn pretty_name_drops_the_quotes() {
        let text = "NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nID=arch\n";
        assert_eq!(pretty_name(text).as_deref(), Some("Arch Linux"));
        assert_eq!(pretty_name("ID=arch\n"), None);
    }
}
