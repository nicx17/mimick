//! "About" dialog for the connected Immich server: component versions, source and
//! build links, and the server's upgrade history (mirrors the Immich web About panel).

use crate::i18n::fill;
use crate::i18n::format_local;
use gettextrs::gettext;
use std::sync::Arc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{ServerAbout, VersionHistoryEntry};
use crate::app_context::AppContext;

/// One name/value cell; `url` turns the value into a link.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AboutRow {
    label: String,
    value: String,
    url: Option<String>,
}

/// Cells in display order (two per grid row), skipping anything the server didn't report.
fn about_rows(about: &ServerAbout) -> Vec<AboutRow> {
    let row = |label: &str, value: Option<&String>, url: Option<&String>| {
        value.filter(|v| !v.is_empty()).map(|v| AboutRow {
            label: label.to_string(),
            value: v.clone(),
            url: url.cloned(),
        })
    };
    [
        row("Immich", Some(&about.version), about.version_url.as_ref()),
        row("ExifTool", about.exiftool.as_ref(), None),
        row("Node.js", about.nodejs.as_ref(), None),
        row("Libvips", about.libvips.as_ref(), None),
        row("ImageMagick", about.imagemagick.as_ref(), None),
        row("FFmpeg", about.ffmpeg.as_ref(), None),
        row(
            &gettext("Repository"),
            about.repository.as_ref(),
            about.repository_url.as_ref(),
        ),
        row(
            &gettext("Source"),
            source_label(about).as_ref(),
            about.source_url.as_ref(),
        ),
        row(
            &gettext("Build"),
            about.build.as_ref(),
            about.build_url.as_ref(),
        ),
        row(
            &gettext("Build Image"),
            about.build_image.as_ref(),
            about.build_image_url.as_ref(),
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// `ref@commit` like the web UI (commit shortened to 9 characters).
fn source_label(about: &ServerAbout) -> Option<String> {
    let reference = about.source_ref.as_deref()?;
    Some(match about.source_commit.as_deref() {
        Some(commit) => format!("{}@{}", reference, &commit[..commit.len().min(9)]),
        None => reference.to_string(),
    })
}

/// "Installed 3.3.1 on 9 Oct 2026", in the local timezone.
fn history_line(entry: &VersionHistoryEntry) -> String {
    let date = chrono::DateTime::parse_from_rfc3339(&entry.created_at)
        // Translators: strftime date, e.g. "9 Oct 2026".
        .map(|dt| format_local(dt, &gettext("%-d %b %Y")))
        .unwrap_or_else(|_| entry.created_at.clone());
    fill(
        gettext("Installed {version} on {date}"),
        &[("version", &entry.version), ("date", &date)],
    )
}

/// Fetch server details and present the About dialog over `parent`.
pub fn present(ctx: Arc<AppContext>, parent: &libadwaita::ApplicationWindow) {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_top(18)
        .margin_bottom(24)
        .margin_start(18)
        .margin_end(18)
        .build();
    let spinner = gtk::Spinner::builder().spinning(true).build();
    content.append(&spinner);
    let dialog = build_dialog(&content);
    dialog.present(Some(parent));

    glib::MainContext::default().spawn_local(async move {
        let api = &ctx.api_client;
        let (about, history) =
            futures_util::join!(api.fetch_server_about(), api.fetch_version_history());
        content.remove(&spinner);
        match about {
            Ok(about) => {
                content.append(&about_grid(&about_rows(&about)));
                // History is optional: older servers or keys without access just omit it.
                if let Ok(history) = history
                    && !history.is_empty()
                {
                    content.append(&history_section(&history));
                }
            }
            Err(err) => content.append(&error_page(&err)),
        }
    });
}

fn build_dialog(content: &gtk::Box) -> libadwaita::Dialog {
    let title = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    // The server's icon and name, so it isn't mistaken for the main menu's About Mimick.
    title.append(
        &gtk::Image::builder()
            .icon_name("network-server-symbolic")
            .pixel_size(28)
            .build(),
    );
    title.append(
        &gtk::Label::builder()
            .label(gettext("Immich Server"))
            .css_classes(["title-2"])
            .build(),
    );
    let header = libadwaita::HeaderBar::builder()
        .title_widget(&title)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(content)
        .build();
    let toolbar = libadwaita::ToolbarView::builder().content(&scroll).build();
    toolbar.add_top_bar(&header);
    libadwaita::Dialog::builder()
        .title(gettext("Immich Server"))
        .content_width(640)
        // Fixed height: the dialog is sized before the details load, so its natural height
        // would only fit the spinner. Smaller windows still shrink it to fit.
        .content_height(600)
        .child(&toolbar)
        .build()
}

/// Name/value cells, filled left to right: two columns, or one on narrow windows.
fn about_grid(rows: &[AboutRow]) -> gtk::FlowBox {
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(1)
        .max_children_per_line(2)
        .homogeneous(true)
        .column_spacing(32)
        .row_spacing(18)
        .build();
    for row in rows {
        grid.insert(&about_cell(row), -1);
    }
    grid
}

fn about_cell(row: &AboutRow) -> gtk::Box {
    let cell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .build();
    cell.append(
        &gtk::Label::builder()
            .label(&row.label)
            .xalign(0.0)
            .css_classes(["heading", "accent"])
            .build(),
    );
    let value = gtk::Label::builder()
        .xalign(0.0)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .css_classes(["dim-label"])
        .build();
    match &row.url {
        // GTK opens `<a>` links in the default browser.
        Some(url) => value.set_markup(&format!(
            "<a href=\"{}\">{}</a>",
            glib::markup_escape_text(url),
            glib::markup_escape_text(&row.value)
        )),
        None => value.set_label(&row.value),
    }
    cell.append(&value);
    cell
}

fn history_section(history: &[VersionHistoryEntry]) -> gtk::Box {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    section.append(
        &gtk::Label::builder()
            .label(gettext("Version History"))
            .xalign(0.0)
            .css_classes(["heading", "accent"])
            .build(),
    );
    for entry in history {
        section.append(
            &gtk::Label::builder()
                .label(history_line(entry))
                .xalign(0.0)
                .wrap(true)
                .build(),
        );
    }
    section
}

fn error_page(err: &str) -> libadwaita::StatusPage {
    libadwaita::StatusPage::builder()
        .icon_name("dialog-warning-symbolic")
        .title(gettext("Server details unavailable"))
        .description(fill(gettext("Your API key needs the 'server.about' and 'server.versionCheck' permissions.\n\n{err}"), &[("err", err)]))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn about() -> ServerAbout {
        ServerAbout {
            version: "v3.3.1".into(),
            version_url: Some("https://github.com/immich-app/immich/releases/tag/v3.3.1".into()),
            exiftool: Some("13.59".into()),
            nodejs: Some("v24.21.0".into()),
            repository: Some("immich-app/immich".into()),
            repository_url: Some("https://github.com/immich-app/immich".into()),
            source_ref: Some("v3.3.1".into()),
            source_commit: Some("db6e11ecc1234567".into()),
            ..ServerAbout::default()
        }
    }

    #[test]
    fn about_rows_skip_missing_fields_and_keep_order() {
        let rows = about_rows(&about());
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Immich", "ExifTool", "Node.js", "Repository", "Source"]
        );
        let immich = &about_rows(&about())[0];
        assert!(immich.url.as_deref().unwrap().ends_with("v3.3.1"));
    }

    #[test]
    fn source_label_shortens_the_commit() {
        assert_eq!(source_label(&about()).as_deref(), Some("v3.3.1@db6e11ecc"));
        let no_commit = ServerAbout {
            source_commit: None,
            ..about()
        };
        assert_eq!(source_label(&no_commit).as_deref(), Some("v3.3.1"));
        assert_eq!(source_label(&ServerAbout::default()), None);
    }

    #[test]
    fn history_line_formats_the_install_date() {
        let entry = VersionHistoryEntry {
            version: "3.3.1".into(),
            created_at: "2026-10-09T12:00:00.000Z".into(),
        };
        let line = history_line(&entry);
        assert!(line.starts_with("Installed 3.3.1 on "));
        assert!(line.ends_with("Oct 2026"));
        let bad = VersionHistoryEntry {
            version: "1.0.0".into(),
            created_at: "unknown".into(),
        };
        assert_eq!(history_line(&bad), "Installed 1.0.0 on unknown");
    }
}
