//! Loads Mimick's gettext translations for the user's locale.

use std::path::PathBuf;

use gettextrs::{bind_textdomain_codeset, bindtextdomain, textdomain};

const DOMAIN: &str = "mimick";

/// Points gettext at Mimick's catalogs; must run before any `gettext` call.
/// The locale itself is applied by `gtk_init`, which runs `setlocale(LC_ALL, "")`.
pub fn init() {
    let Some(dir) = locale_dir() else {
        log::debug!("No translations installed; using English");
        return;
    };
    if let Err(err) = bind(dir) {
        log::warn!("Could not load translations: {}", err);
    }
}

fn bind(dir: PathBuf) -> std::io::Result<()> {
    bindtextdomain(DOMAIN, dir)?;
    bind_textdomain_codeset(DOMAIN, "UTF-8")?;
    textdomain(DOMAIN)?;
    Ok(())
}

/// Marks a `const` string for extraction; pass it through `gettext` where it is shown.
pub const fn gettext_noop(msgid: &str) -> &str {
    msgid
}

/// Replaces each `{key}` in a translated template with its value.
pub fn fill(template: String, args: &[(&str, &str)]) -> String {
    args.iter().fold(template, |text, (key, value)| {
        text.replace(&format!("{{{key}}}"), value)
    })
}

/// Count argument for `ngettext`, which takes a `u32`.
pub fn plural_n(n: impl TryInto<u32>) -> u32 {
    n.try_into().unwrap_or(u32::MAX)
}

/// Formats a calendar day with a strftime pattern, using the locale's month and weekday names.
pub fn format_day(day: chrono::NaiveDate, format: &str) -> String {
    use chrono::Datelike;
    glib::DateTime::from_utc(day.year(), day.month() as i32, day.day() as i32, 0, 0, 0.0)
        .and_then(|dt| dt.format(format))
        .map_or_else(|_| day.format(format).to_string(), String::from)
}

/// Formats a moment in the local time zone like `format_day`.
pub fn format_local(moment: chrono::DateTime<chrono::FixedOffset>, format: &str) -> String {
    glib::DateTime::from_unix_local(moment.timestamp())
        .and_then(|dt| dt.format(format))
        .map_or_else(
            |_| {
                moment
                    .with_timezone(&chrono::Local)
                    .format(format)
                    .to_string()
            },
            String::from,
        )
}

/// `share/locale` next to `bin/` (Flatpak, distro packages, `cargo run` into `target/`),
/// falling back to the mounted AppImage, where the binary runs through sharun's loader.
fn locale_dir() -> Option<PathBuf> {
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join("share/locale")));
    let in_appimage = std::env::var_os("APPDIR").map(|dir| PathBuf::from(dir).join("share/locale"));
    [beside_exe, in_appimage]
        .into_iter()
        .flatten()
        .find(|dir| dir.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_replaces_every_named_placeholder() {
        let text = fill(
            "{n} of {total} in {n}".to_string(),
            &[("n", "3"), ("total", "9")],
        );
        assert_eq!(text, "3 of 9 in 3");
    }

    #[test]
    fn fill_leaves_unknown_placeholders() {
        assert_eq!(fill("Hi {name}".to_string(), &[]), "Hi {name}");
    }

    #[test]
    fn plural_n_saturates_huge_counts() {
        assert_eq!(plural_n(7_usize), 7);
        assert_eq!(plural_n(u64::MAX), u32::MAX);
    }
}
