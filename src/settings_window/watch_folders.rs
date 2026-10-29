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
use gtk::{Button, ListBox};
use libadwaita as adw;

use crate::config::{FolderRules, StartupCatchupMode};
use crate::watch_path_display::{display_watch_path, watch_path_subtitle};

use super::{DEFAULT_ALBUM_LABEL, FolderRowData, LIBRARY_ALBUM_LABEL, WatchPathEntry};

mod album_picker;
mod rules_dialog;
use album_picker::PickerTarget;
use rules_dialog::RulesTarget;
pub use rules_dialog::{catchup_from_index, catchup_index};

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
    };
    add_target_row(&row, albums_ref, on_settings_changed.clone());
    rules_dialog::add_rules_row(
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
    album_picker::connect_picker_button(&row.action_row, albums_ref, picker);
}

/// Folder subtitle; each folder's sync state is shown on the Status page.
pub(super) fn initial_subtitle(base_subtitle: &str) -> String {
    base_subtitle.to_string()
}

pub(super) fn initial_target_label(entry: &WatchPathEntry) -> String {
    if entry.uploads_to_library() {
        LIBRARY_ALBUM_LABEL.to_string()
    } else {
        entry
            .album_name()
            .unwrap_or(DEFAULT_ALBUM_LABEL)
            .to_string()
    }
}

pub(super) fn initial_rules(entry: &WatchPathEntry) -> FolderRules {
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
