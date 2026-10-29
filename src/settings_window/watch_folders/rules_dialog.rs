use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use glib::clone;
use gtk::prelude::*;
use gtk::{Box, Button, Orientation, ScrolledWindow};
use libadwaita as adw;

use crate::config::{FolderRules, FolderSyncMethod, StartupCatchupMode};
use crate::watch_path_display::display_watch_path;

use super::{parent_window, suffix_row};

/// What the rules dialog edits for one folder row.
#[derive(Clone)]
pub(super) struct RulesTarget {
    pub(super) folder_path: String,
    pub(super) fallback_catchup_mode: StartupCatchupMode,
    pub(super) rules: Rc<RefCell<FolderRules>>,
    pub(super) uploads_to_library: Rc<Cell<bool>>,
    pub(super) on_changed: Rc<dyn Fn()>,
}

/// "Folder Rules" sub-row whose button opens the rules dialog.
pub(super) fn add_rules_row(expander_row: &adw::ExpanderRow, target: RulesTarget) {
    let rules_btn = Button::builder()
        .label("Rules")
        .tooltip_text("Edit folder rules")
        .valign(gtk::Align::Center)
        .build();
    expander_row.add_row(&suffix_row("Folder Rules", &rules_btn));
    rules_btn.connect_clicked(clone!(
        #[weak]
        expander_row,
        move |_| {
            let Some(window) = parent_window(&expander_row) else {
                return;
            };
            let target = target.clone();
            glib::idle_add_local_once(move || show_folder_rules_dialog(&window, target));
        }
    ));
}

fn show_folder_rules_dialog(parent: &impl gtk::prelude::IsA<gtk::Window>, target: RulesTarget) {
    let (dialog, content) = rules_dialog_shell(parent, &target.folder_path);
    let current = target.rules.borrow().clone();
    let form = RulesForm::new(
        &current,
        target.fallback_catchup_mode.clone(),
        target.uploads_to_library.get(),
    );
    form.append_to(&content);

    let (cancel_btn, save_btn) = append_save_cancel(&content);
    cancel_btn.connect_clicked(clone!(
        #[weak]
        dialog,
        move |_| dialog.close()
    ));
    save_btn.connect_clicked(clone!(
        #[weak]
        dialog,
        move |_| {
            let updated = form.rules(&target.rules.borrow());
            *target.rules.borrow_mut() = updated;
            (target.on_changed)();
            dialog.close();
        }
    ));
    dialog.present();
}

fn rules_dialog_shell(
    parent: &impl gtk::prelude::IsA<gtk::Window>,
    folder_path: &str,
) -> (adw::Window, Box) {
    let dialog = adw::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("Folder Rules")
        .default_width(420)
        .default_height(680)
        .width_request(360)
        .build();
    let content = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&content)
        .build();
    dialog.set_content(Some(&scroll));
    let title = gtk::Label::builder()
        .label(display_watch_path(folder_path))
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .max_width_chars(28)
        .css_classes(["title-4"])
        .build();
    content.append(&title);
    (dialog, content)
}

fn append_save_cancel(content: &Box) -> (Button, Button) {
    let actions = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .build();
    let cancel_btn = Button::builder().label("Cancel").build();
    let save_btn = Button::builder()
        .label("Save")
        .css_classes(vec!["suggested-action".to_string()])
        .build();
    actions.append(&cancel_btn);
    actions.append(&save_btn);
    content.append(&actions);
    (cancel_btn, save_btn)
}

/// Editable rows of the folder rules dialog.
struct RulesForm {
    sync_method: adw::ComboRow,
    startup_scan: adw::ComboRow,
    delete_folder_to_album: adw::SwitchRow,
    delete_album_to_folder: adw::SwitchRow,
    ignore_hidden: adw::SwitchRow,
    include_xmp: adw::SwitchRow,
    max_size_entry: adw::EntryRow,
    extensions_entry: adw::EntryRow,
}

fn switch_row(title: &str, subtitle: &str, subtitle_lines: i32, active: bool) -> adw::SwitchRow {
    adw::SwitchRow::builder()
        .title(title)
        .subtitle(subtitle)
        .title_lines(2)
        .subtitle_lines(subtitle_lines)
        .active(active)
        .build()
}

fn combo_row(title: &str, subtitle: &str, items: &[&str], selected: u32) -> adw::ComboRow {
    let row = adw::ComboRow::builder()
        .title(title)
        .subtitle(subtitle)
        .title_lines(1)
        .subtitle_lines(2)
        .model(&gtk::StringList::new(items))
        .build();
    row.set_selected(selected);
    row
}

fn entry_row(title: &str, text: &str) -> adw::EntryRow {
    adw::EntryRow::builder().title(title).text(text).build()
}

/// What each sync method does, shown under the Sync Method row.
pub(super) fn sync_method_description(index: u32) -> &'static str {
    match index {
        0 => "Uploads new photos from this folder and downloads new album photos into it.",
        2 => "Downloads new album photos into this folder. Nothing is uploaded.",
        _ => "Uploads new photos from this folder. Nothing is downloaded.",
    }
}

impl RulesForm {
    fn new(current: &FolderRules, fallback: StartupCatchupMode, uploads_to_library: bool) -> Self {
        let (delete_folder_to_album, delete_album_to_folder) = deletion_rows(current);
        let (ignore_hidden, include_xmp) = filter_rows(current);
        let (max_size_entry, extensions_entry) = limit_entries(current);
        let form = Self {
            sync_method: combo_row(
                "Sync Method",
                sync_method_description(sync_method_index(&current.sync_method)),
                &["Full Sync", "Upload Only", "Download Only"],
                sync_method_index(&current.sync_method),
            ),
            startup_scan: combo_row(
                "When Mimick Starts, Check",
                "Which files in this folder are checked at startup.",
                &crate::settings_window::behavior::CATCHUP_LABELS,
                catchup_index(&current.startup_catchup_mode.clone().unwrap_or(fallback)),
            ),
            delete_folder_to_album,
            delete_album_to_folder,
            ignore_hidden,
            include_xmp,
            max_size_entry,
            extensions_entry,
        };
        if uploads_to_library {
            form.restrict_to_library();
        } else {
            form.sync_method.connect_selected_notify(|row| {
                row.set_subtitle(sync_method_description(row.selected()));
            });
        }
        form
    }

    fn restrict_to_library(&self) {
        self.sync_method
            .set_subtitle("Library uploads can only upload from this folder.");
        self.sync_method.set_sensitive(false);
        self.delete_folder_to_album
            .set_subtitle("Not available for library uploads.");
        self.delete_folder_to_album.set_active(false);
        self.delete_folder_to_album.set_sensitive(false);
    }

    fn append_to(&self, content: &Box) {
        let section = |title: &str, rows: &[&gtk::Widget]| {
            let group = adw::PreferencesGroup::builder().title(title).build();
            for row in rows {
                group.add(*row);
            }
            content.append(&group);
        };
        section(
            "Sync",
            &[
                self.sync_method.upcast_ref(),
                self.startup_scan.upcast_ref(),
            ],
        );
        section(
            "Deletions",
            &[
                self.delete_folder_to_album.upcast_ref(),
                self.delete_album_to_folder.upcast_ref(),
            ],
        );
        section(
            "Files",
            &[
                self.ignore_hidden.upcast_ref(),
                self.include_xmp.upcast_ref(),
                self.max_size_entry.upcast_ref(),
                self.extensions_entry.upcast_ref(),
            ],
        );
    }

    /// Rules from the form. Album-to-folder mirroring keeps its stored value while
    /// its switch is forced off.
    fn rules(&self, previous: &FolderRules) -> FolderRules {
        FolderRules {
            ignore_hidden: self.ignore_hidden.is_active(),
            max_file_size_mb: parse_max_size(&self.max_size_entry.text()),
            allowed_extensions: parse_extensions(&self.extensions_entry.text()),
            sync_method: sync_method_from_index(self.sync_method.selected()),
            startup_catchup_mode: Some(catchup_from_index(self.startup_scan.selected())),
            delete_folder_to_album: self.delete_folder_to_album.is_active(),
            delete_album_to_folder: previous.delete_album_to_folder,
            include_xmp_sidecar: Some(self.include_xmp.is_active()),
        }
    }
}

fn deletion_rows(current: &FolderRules) -> (adw::SwitchRow, adw::SwitchRow) {
    let folder_to_album = switch_row(
        "Delete from Immich Too",
        "When you delete a photo from this folder, move its copy in Immich to the trash. It can be restored from the Immich trash.",
        3,
        current.delete_folder_to_album,
    );
    // Forced off and insensitive until the Flatpak Trash portal fix lands upstream.
    let album_to_folder = switch_row(
        "Delete from This Folder Too",
        "Not available yet: waiting on a fix to the Flatpak trash portal.",
        4,
        false,
    );
    album_to_folder.set_sensitive(false);
    (folder_to_album, album_to_folder)
}

fn filter_rows(current: &FolderRules) -> (adw::SwitchRow, adw::SwitchRow) {
    let ignore_hidden = switch_row(
        "Skip Hidden Files",
        "Ignore files and folders starting with a dot, such as .thumbnails.",
        3,
        current.ignore_hidden,
    );
    let include_xmp = switch_row(
        "Upload XMP Sidecars",
        "Send .xmp edit files along with their photos.",
        2,
        current.include_xmp_sidecar.unwrap_or(true),
    );
    ignore_hidden.set_title_lines(1);
    include_xmp.set_title_lines(1);
    (ignore_hidden, include_xmp)
}

fn limit_entries(current: &FolderRules) -> (adw::EntryRow, adw::EntryRow) {
    let max_size = current
        .max_file_size_mb
        .map(|value| value.to_string())
        .unwrap_or_default();
    (
        entry_row("Largest File in MB (empty for no limit)", &max_size),
        entry_row(
            "Only These Types, e.g. jpg, png, mp4 (empty for all)",
            &current.allowed_extensions.join(", "),
        ),
    )
}

pub(super) fn sync_method_index(method: &FolderSyncMethod) -> u32 {
    match method {
        FolderSyncMethod::Full => 0,
        FolderSyncMethod::UploadOnly => 1,
        FolderSyncMethod::DownloadOnly => 2,
    }
}

pub(super) fn sync_method_from_index(index: u32) -> FolderSyncMethod {
    match index {
        1 => FolderSyncMethod::UploadOnly,
        2 => FolderSyncMethod::DownloadOnly,
        _ => FolderSyncMethod::Full,
    }
}

pub fn catchup_index(mode: &StartupCatchupMode) -> u32 {
    match mode {
        StartupCatchupMode::Full => 0,
        StartupCatchupMode::RecentOnly => 1,
        StartupCatchupMode::NewFilesOnly => 2,
    }
}

pub fn catchup_from_index(index: u32) -> StartupCatchupMode {
    match index {
        1 => StartupCatchupMode::RecentOnly,
        2 => StartupCatchupMode::NewFilesOnly,
        _ => StartupCatchupMode::Full,
    }
}

/// Blank or non-numeric input means "no limit".
pub(super) fn parse_max_size(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok()
}

/// Comma-separated extensions, lowercased, with any leading dot removed.
pub(super) fn parse_extensions(text: &str) -> Vec<String> {
    text.split(',')
        .map(|part| part.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|part| !part.is_empty())
        .collect()
}
