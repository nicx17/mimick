//! Watch-folder row construction, folder-rules dialog, and album picker dialog.
//!
//! Each configured watch path gets a row with inline controls for the
//! target album and a rules button. The rules dialog exposes sync method,
//! extension and size filters, deletion policy, and hidden-file settings.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use glib::clone;
use gtk::prelude::*;
use gtk::{Box, Button, Entry, ListBox, Orientation, ScrolledWindow};
use libadwaita as adw;

use crate::config::{FolderRules, FolderSyncMethod, StartupCatchupMode};
use crate::watch_path_display::{display_watch_path, watch_path_subtitle};

use super::{DEFAULT_ALBUM_LABEL, FolderRowData, LIBRARY_ALBUM_LABEL, WatchPathEntry};

/// A folder row's upload target, remembering its rules from before it switched to Library.
#[derive(Clone)]
pub(super) struct UploadTargetState {
    rules: Rc<RefCell<FolderRules>>,
    library: Rc<Cell<bool>>,
    rules_before_library: Rc<RefCell<Option<FolderRules>>>,
}

impl UploadTargetState {
    fn select_library(&self) {
        if !self.library.replace(true) {
            *self.rules_before_library.borrow_mut() = Some(self.rules.borrow().clone());
        }
        self.rules.borrow_mut().restrict_to_library_uploads();
    }

    fn select_album(&self) {
        if !self.library.replace(false) {
            return;
        }
        if let Some(previous) = self.rules_before_library.borrow_mut().take() {
            let mut rules = self.rules.borrow_mut();
            rules.sync_method = previous.sync_method;
            rules.delete_folder_to_album = previous.delete_folder_to_album;
            rules.delete_album_to_folder = previous.delete_album_to_folder;
        }
    }
}

/// Append a new watched folder row to the settings folders ListBox.
pub(super) fn add_folder_row(
    list: &ListBox,
    entry: &WatchPathEntry,
    fallback_catchup_mode: StartupCatchupMode,
    albums_ref: Rc<RefCell<Vec<(String, String)>>>,
    tracked_rows: &Rc<RefCell<Vec<FolderRowData>>>,
    on_settings_changed: Rc<dyn Fn()>,
) {
    let path = entry.path().to_string();
    let base_subtitle = watch_path_subtitle(&path).unwrap_or_default().to_string();
    let row = FolderRowData {
        action_row: folder_expander_row(&path, &base_subtitle),
        album_name: Rc::new(RefCell::new(initial_target_label(entry))),
        uploads_to_library: Rc::new(Cell::new(entry.uploads_to_library())),
        rules: Rc::new(RefCell::new(initial_rules(entry))),
        path,
        base_subtitle,
    };
    add_target_row(&row, albums_ref, on_settings_changed.clone());
    add_rules_row(
        &row.action_row,
        RulesTarget {
            folder_path: row.path.clone(),
            fallback_catchup_mode,
            rules: row.rules.clone(),
            uploads_to_library: row.uploads_to_library.clone(),
            on_changed: on_settings_changed.clone(),
        },
    );
    add_remove_row(
        list,
        &row.action_row,
        tracked_rows,
        &row.path,
        on_settings_changed,
    );
    list.append(&row.action_row);
    tracked_rows.borrow_mut().push(row);
}

fn folder_expander_row(path: &str, base_subtitle: &str) -> adw::ExpanderRow {
    adw::ExpanderRow::builder()
        .title(display_watch_path(path))
        .subtitle(initial_subtitle(base_subtitle))
        .subtitle_lines(2)
        .title_lines(1)
        .build()
}

/// "Upload Target" sub-row whose button opens the album picker.
fn add_target_row(
    row: &FolderRowData,
    albums_ref: Rc<RefCell<Vec<(String, String)>>>,
    on_changed: Rc<dyn Fn()>,
) {
    let picker = PickerTarget {
        label: row.album_name.clone(),
        button: picker_button(&row.album_name.borrow()),
        upload_target: UploadTargetState {
            rules: row.rules.clone(),
            library: row.uploads_to_library.clone(),
            rules_before_library: Rc::new(RefCell::new(None)),
        },
        on_changed,
    };
    row.action_row
        .add_row(&suffix_row("Upload Target", &picker.button));
    connect_picker_button(&row.action_row, albums_ref, picker);
}

/// Folder subtitle plus a status line that the status poller updates later.
fn initial_subtitle(base_subtitle: &str) -> String {
    if base_subtitle.is_empty() {
        "Status: Idle".to_string()
    } else {
        format!("{base_subtitle}\nStatus: Idle")
    }
}

fn initial_target_label(entry: &WatchPathEntry) -> String {
    if entry.uploads_to_library() {
        LIBRARY_ALBUM_LABEL.to_string()
    } else {
        entry
            .album_name()
            .unwrap_or(DEFAULT_ALBUM_LABEL)
            .to_string()
    }
}

fn initial_rules(entry: &WatchPathEntry) -> FolderRules {
    let mut rules = entry.rules();
    if entry.uploads_to_library() {
        rules.restrict_to_library_uploads();
    }
    rules
}

fn picker_button(label: &str) -> Button {
    let button = Button::builder()
        .label(label)
        .valign(gtk::Align::Center)
        .tooltip_text("Select a library or album upload target")
        .build();
    if let Some(label) = button.child().and_downcast::<gtk::Label>() {
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(16);
    }
    button
}

fn suffix_row(title: &str, suffix: &impl IsA<gtk::Widget>) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .title_lines(1)
        .build();
    row.add_suffix(suffix);
    row
}

fn parent_window(row: &adw::ExpanderRow) -> Option<gtk::Window> {
    row.root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
}

fn connect_picker_button(
    expander_row: &adw::ExpanderRow,
    albums_ref: Rc<RefCell<Vec<(String, String)>>>,
    picker: PickerTarget,
) {
    let button = picker.button.clone();
    button.connect_clicked(clone!(
        #[weak]
        expander_row,
        move |_| {
            let Some(window) = parent_window(&expander_row) else {
                return;
            };
            let albums_ref = albums_ref.clone();
            let picker = picker.clone();
            // Deferred to idle so the click finishes before the modal opens.
            glib::idle_add_local_once(move || {
                show_album_picker_dialog(&window, albums_ref, picker);
            });
        }
    ));
}

/// What the rules dialog edits for one folder row.
#[derive(Clone)]
struct RulesTarget {
    folder_path: String,
    fallback_catchup_mode: StartupCatchupMode,
    rules: Rc<RefCell<FolderRules>>,
    uploads_to_library: Rc<Cell<bool>>,
    on_changed: Rc<dyn Fn()>,
}

/// "Folder Rules" sub-row whose button opens the rules dialog.
fn add_rules_row(expander_row: &adw::ExpanderRow, target: RulesTarget) {
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

/// "Remove Folder" sub-row that drops the row and its tracked settings.
fn add_remove_row(
    list: &ListBox,
    expander_row: &adw::ExpanderRow,
    tracked_rows: &Rc<RefCell<Vec<FolderRowData>>>,
    path: &str,
    on_changed: Rc<dyn Fn()>,
) {
    let remove_btn = Button::builder()
        .icon_name("user-trash-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(vec!["destructive-action".to_string()])
        .build();
    expander_row.add_row(&suffix_row("Remove Folder", &remove_btn));
    let list = list.clone();
    let tracked_rows = tracked_rows.clone();
    let path = path.to_string();
    remove_btn.connect_clicked(clone!(
        #[weak]
        expander_row,
        move |_| {
            let (list, tracked_rows, path, on_changed) = (
                list.clone(),
                tracked_rows.clone(),
                path.clone(),
                on_changed.clone(),
            );
            let expander_row = expander_row.clone();
            glib::idle_add_local_once(move || {
                // Move focus off the row before removing it so GTK doesn't warn.
                if let Some(focus_target) = list.first_child() {
                    focus_target.grab_focus();
                }
                list.remove(&expander_row);
                tracked_rows.borrow_mut().retain(|r| r.path != path);
                on_changed();
            });
        }
    ));
}

/// Present a modal dialog for editing a folder's sync and filter rules.
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
        .default_width(380)
        .default_height(640)
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
        .label(format!("Rules for {}", display_watch_path(folder_path)))
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .max_width_chars(28)
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
    max_size_entry: Entry,
    extensions_entry: Entry,
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

fn small_entry(placeholder: &str, text: &str) -> Entry {
    Entry::builder()
        .placeholder_text(placeholder)
        .width_request(0)
        .max_width_chars(16)
        .text(text)
        .build()
}

impl RulesForm {
    fn new(current: &FolderRules, fallback: StartupCatchupMode, uploads_to_library: bool) -> Self {
        let (delete_folder_to_album, delete_album_to_folder) = deletion_rows(current);
        let (ignore_hidden, include_xmp) = filter_rows(current);
        let (max_size_entry, extensions_entry) = limit_entries(current);
        let form = Self {
            sync_method: combo_row(
                "Sync Method",
                "Controls which direction this folder syncs.",
                &[
                    "Full Sync",
                    "Only Upload from Folder",
                    "Only Download to Folder",
                ],
                sync_method_index(&current.sync_method),
            ),
            startup_scan: combo_row(
                "Startup Scan",
                "Controls how this folder is scanned when Mimick starts.",
                &["Full Scan", "Recent Only (7d)", "New Files Only"],
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
        let list_box = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(vec![String::from("boxed-list")])
            .build();
        list_box.append(&self.sync_method);
        list_box.append(&self.startup_scan);
        list_box.append(&self.delete_folder_to_album);
        list_box.append(&self.delete_album_to_folder);
        list_box.append(&self.ignore_hidden);
        list_box.append(&self.include_xmp);
        content.append(&list_box);
        content.append(&self.max_size_entry);
        content.append(&self.extensions_entry);
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
        "Mirror Folder Deletions to Album",
        "When a synced folder file is gone, move the matching Immich asset to trash.",
        3,
        current.delete_folder_to_album,
    );
    // Forced off and insensitive until the Flatpak Trash portal fix lands upstream.
    let album_to_folder = switch_row(
        "Mirror Album Deletions to Folder",
        "Currently unavailable — waiting on an upstream Flatpak Trash portal fix. The setting stays off until then.",
        4,
        false,
    );
    album_to_folder.set_sensitive(false);
    (folder_to_album, album_to_folder)
}

fn filter_rows(current: &FolderRules) -> (adw::SwitchRow, adw::SwitchRow) {
    let ignore_hidden = switch_row(
        "Ignore Hidden Files / Folders",
        "Skip paths that contain hidden components such as .cache or .thumbnails.",
        3,
        current.ignore_hidden,
    );
    let include_xmp = switch_row(
        "Include XMP Sidecars",
        "Attach companion .xmp files alongside media during upload.",
        2,
        current.include_xmp_sidecar.unwrap_or(true),
    );
    ignore_hidden.set_title_lines(1);
    include_xmp.set_title_lines(1);
    (ignore_hidden, include_xmp)
}

fn limit_entries(current: &FolderRules) -> (Entry, Entry) {
    let max_size = current
        .max_file_size_mb
        .map(|value| value.to_string())
        .unwrap_or_default();
    (
        small_entry("Max size in MB (blank = no limit)", &max_size),
        small_entry(
            "Extensions: jpg,png,mp4",
            &current.allowed_extensions.join(", "),
        ),
    )
}

fn sync_method_index(method: &FolderSyncMethod) -> u32 {
    match method {
        FolderSyncMethod::Full => 0,
        FolderSyncMethod::UploadOnly => 1,
        FolderSyncMethod::DownloadOnly => 2,
    }
}

fn sync_method_from_index(index: u32) -> FolderSyncMethod {
    match index {
        1 => FolderSyncMethod::UploadOnly,
        2 => FolderSyncMethod::DownloadOnly,
        _ => FolderSyncMethod::Full,
    }
}

pub(super) fn catchup_index(mode: &StartupCatchupMode) -> u32 {
    match mode {
        StartupCatchupMode::Full => 0,
        StartupCatchupMode::RecentOnly => 1,
        StartupCatchupMode::NewFilesOnly => 2,
    }
}

pub(super) fn catchup_from_index(index: u32) -> StartupCatchupMode {
    match index {
        1 => StartupCatchupMode::RecentOnly,
        2 => StartupCatchupMode::NewFilesOnly,
        _ => StartupCatchupMode::Full,
    }
}

/// Blank or non-numeric input means "no limit".
fn parse_max_size(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok()
}

/// Comma-separated extensions, lowercased, with any leading dot removed.
fn parse_extensions(text: &str) -> Vec<String> {
    text.split(',')
        .map(|part| part.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|part| !part.is_empty())
        .collect()
}

/// The folder row's target button and the state the album picker updates.
#[derive(Clone)]
pub(super) struct PickerTarget {
    label: Rc<RefCell<String>>,
    button: Button,
    upload_target: UploadTargetState,
    on_changed: Rc<dyn Fn()>,
}

impl PickerTarget {
    fn choose(&self, choice: &PickerChoice) {
        let label = choice.stored_label();
        *self.label.borrow_mut() = label.clone();
        if matches!(choice, PickerChoice::Library) {
            self.upload_target.select_library();
        } else {
            self.upload_target.select_album();
        }
        self.button.set_label(&label);
        (self.on_changed)();
    }
}

/// One row of the album picker.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PickerChoice {
    Library,
    DefaultFolderName,
    Create(String),
    Album(String),
}

impl PickerChoice {
    fn title(&self) -> String {
        match self {
            Self::Create(name) => format!("Create new: \"{name}\""),
            other => other.stored_label(),
        }
    }

    fn subtitle(&self) -> Option<&'static str> {
        match self {
            Self::Library => Some(
                "Uploads directly to the library; album sync and deletion mirroring are disabled",
            ),
            Self::DefaultFolderName => Some("Creates album dynamically per-folder"),
            _ => None,
        }
    }

    /// What the folder row stores and shows on its button.
    fn stored_label(&self) -> String {
        match self {
            Self::Library => LIBRARY_ALBUM_LABEL.to_string(),
            Self::DefaultFolderName => DEFAULT_ALBUM_LABEL.to_string(),
            Self::Create(name) | Self::Album(name) => name.clone(),
        }
    }
}

/// Picker rows for `query`: Library, the folder-name default, "Create new" for typed
/// text, then matching albums (case-insensitive).
fn picker_choices(query: &str, albums: &[(String, String)]) -> Vec<PickerChoice> {
    let typed = query.trim();
    let q = typed.to_lowercase();
    let matches = |label: &str| q.is_empty() || label.to_lowercase().contains(&q);
    let mut choices = Vec::new();
    if matches(LIBRARY_ALBUM_LABEL) {
        choices.push(PickerChoice::Library);
    }
    if matches(DEFAULT_ALBUM_LABEL) {
        choices.push(PickerChoice::DefaultFolderName);
    }
    if !q.is_empty() {
        choices.push(PickerChoice::Create(typed.to_string()));
    }
    choices.extend(
        albums
            .iter()
            .map(|(name, _)| name)
            // The default label is listed above; skip it if an album shares the name.
            .filter(|name| name.as_str() != DEFAULT_ALBUM_LABEL && matches(name))
            .map(|name| PickerChoice::Album(name.clone())),
    );
    choices
}

/// Construct and present a modal search and select window for a folder's upload target.
pub(super) fn show_album_picker_dialog(
    parent: &impl gtk::prelude::IsA<gtk::Window>,
    albums_ref: Rc<RefCell<Vec<(String, String)>>>,
    target: PickerTarget,
) {
    let (dialog, search_entry, list_box) = picker_dialog_shell(parent);
    let dialog_for_rows = dialog.clone();
    let fill = Rc::new(move |query: &str| {
        let choices = picker_choices(query, &albums_ref.borrow());
        fill_picker_rows(&list_box, &dialog_for_rows, &choices, &target);
    });
    fill("");
    search_entry.connect_search_changed(move |entry| fill(&entry.text()));
    dialog.present();
}

fn fill_picker_rows(
    list_box: &gtk::ListBox,
    dialog: &adw::Window,
    choices: &[PickerChoice],
    target: &PickerTarget,
) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }
    for choice in choices {
        let row = adw::ActionRow::builder()
            .title(choice.title())
            .activatable(true)
            .build();
        if let Some(subtitle) = choice.subtitle() {
            row.set_subtitle(subtitle);
        }
        let (choice, target, dialog) = (choice.clone(), target.clone(), dialog.clone());
        row.connect_activated(move |_| {
            target.choose(&choice);
            dialog.close();
        });
        list_box.append(&row);
    }
}

fn picker_dialog_shell(
    parent: &impl gtk::prelude::IsA<gtk::Window>,
) -> (adw::Window, gtk::SearchEntry, gtk::ListBox) {
    let dialog = adw::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("Select Upload Target")
        .default_width(400)
        .default_height(500)
        .width_request(360)
        .build();
    let vbox = Box::builder().orientation(Orientation::Vertical).build();
    dialog.set_content(Some(&vbox));
    vbox.append(&adw::HeaderBar::new());
    let search_entry = gtk::SearchEntry::builder()
        .halign(gtk::Align::Center)
        .width_request(300)
        .margin_top(8)
        .margin_bottom(8)
        .build();
    vbox.append(&search_entry);
    let list_box = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(12)
        .css_classes(["boxed-list"])
        .build();
    let scrolled_window = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&list_box)
        .build();
    vbox.append(&scrolled_window);
    (dialog, search_entry, list_box)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn albums(names: &[&str]) -> Vec<(String, String)> {
        names
            .iter()
            .map(|name| (name.to_string(), format!("id-{name}")))
            .collect()
    }

    #[test]
    fn picker_lists_library_default_then_albums_without_query() {
        let choices = picker_choices("", &albums(&["Trips", DEFAULT_ALBUM_LABEL]));
        assert_eq!(
            choices,
            vec![
                PickerChoice::Library,
                PickerChoice::DefaultFolderName,
                PickerChoice::Album("Trips".into()),
            ]
        );
    }

    #[test]
    fn picker_query_offers_create_and_filters_albums() {
        let choices = picker_choices("  tri ", &albums(&["Trips", "Family"]));
        assert_eq!(
            choices,
            vec![
                PickerChoice::Create("tri".into()),
                PickerChoice::Album("Trips".into()),
            ]
        );
    }

    #[test]
    fn picker_choice_labels() {
        assert_eq!(PickerChoice::Library.stored_label(), LIBRARY_ALBUM_LABEL);
        assert_eq!(
            PickerChoice::Create("New".into()).title(),
            "Create new: \"New\""
        );
        assert_eq!(PickerChoice::Create("New".into()).stored_label(), "New");
        assert!(PickerChoice::Album("Trips".into()).subtitle().is_none());
    }

    #[test]
    fn parse_extensions_normalises_input() {
        assert_eq!(
            parse_extensions(" .JPG, png ,, .Mp4"),
            vec!["jpg".to_string(), "png".to_string(), "mp4".to_string()]
        );
        assert!(parse_extensions("").is_empty());
    }

    #[test]
    fn parse_max_size_treats_blank_or_invalid_as_no_limit() {
        assert_eq!(parse_max_size(" 25 "), Some(25));
        assert_eq!(parse_max_size(""), None);
        assert_eq!(parse_max_size("ten"), None);
    }

    #[test]
    fn combo_indices_round_trip() {
        for method in [
            FolderSyncMethod::Full,
            FolderSyncMethod::UploadOnly,
            FolderSyncMethod::DownloadOnly,
        ] {
            assert_eq!(sync_method_from_index(sync_method_index(&method)), method);
        }
        for mode in [
            StartupCatchupMode::Full,
            StartupCatchupMode::RecentOnly,
            StartupCatchupMode::NewFilesOnly,
        ] {
            assert_eq!(catchup_from_index(catchup_index(&mode)), mode);
        }
    }

    #[test]
    fn initial_row_values_follow_the_entry() {
        assert_eq!(initial_subtitle(""), "Status: Idle");
        assert_eq!(initial_subtitle("~/Pictures"), "~/Pictures\nStatus: Idle");

        let library = WatchPathEntry::WithConfig {
            path: "/home/user/Camera".into(),
            album_id: None,
            album_name: None,
            rules: FolderRules {
                sync_method: FolderSyncMethod::Full,
                ..FolderRules::default()
            },
        };
        assert_eq!(initial_target_label(&library), LIBRARY_ALBUM_LABEL);
        assert_eq!(
            initial_rules(&library).sync_method,
            FolderSyncMethod::UploadOnly
        );

        let simple = WatchPathEntry::Simple("/home/user/Camera".into());
        assert_eq!(initial_target_label(&simple), DEFAULT_ALBUM_LABEL);
    }

    fn target_with(rules: FolderRules) -> UploadTargetState {
        UploadTargetState {
            rules: Rc::new(RefCell::new(rules)),
            library: Rc::new(Cell::new(false)),
            rules_before_library: Rc::new(RefCell::new(None)),
        }
    }

    #[test]
    fn test_switching_back_from_library_restores_sync_rules() {
        let target = target_with(FolderRules {
            sync_method: FolderSyncMethod::Full,
            delete_folder_to_album: true,
            ..FolderRules::default()
        });

        target.select_library();
        assert_eq!(
            target.rules.borrow().sync_method,
            FolderSyncMethod::UploadOnly
        );
        assert!(!target.rules.borrow().delete_folder_to_album);

        target.select_album();
        assert!(!target.library.get());
        assert_eq!(target.rules.borrow().sync_method, FolderSyncMethod::Full);
        assert!(target.rules.borrow().delete_folder_to_album);
    }

    #[test]
    fn test_selecting_library_twice_keeps_original_rules() {
        let target = target_with(FolderRules {
            sync_method: FolderSyncMethod::Full,
            ..FolderRules::default()
        });

        target.select_library();
        target.select_library();
        target.select_album();

        assert_eq!(target.rules.borrow().sync_method, FolderSyncMethod::Full);
    }
}
