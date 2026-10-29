#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use glib::clone;
use gtk::prelude::*;
use gtk::{Box, Button, Orientation};
use libadwaita as adw;

#[cfg(test)]
use crate::config::{FolderRules, FolderSyncMethod, StartupCatchupMode};

use crate::settings_window::{DEFAULT_ALBUM_LABEL, LIBRARY_ALBUM_LABEL};

#[cfg(test)]
use super::rules_dialog::{
    parse_extensions, parse_max_size, sync_method_description, sync_method_from_index,
    sync_method_index,
};
use super::{UploadTargetState, parent_window};
#[cfg(test)]
use super::{WatchPathEntry, initial_rules, initial_subtitle, initial_target_label};
#[cfg(test)]
use super::{catchup_from_index, catchup_index};

/// The folder row's target button and the state the album picker updates.
#[derive(Clone)]
pub(super) struct PickerTarget {
    pub(super) label: Rc<RefCell<String>>,
    pub(super) button: Button,
    pub(super) upload_target: UploadTargetState,
    pub(super) on_changed: Rc<dyn Fn()>,
}

pub(super) fn connect_picker_button(
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

    #[test]
    fn every_sync_method_explains_itself() {
        assert!(sync_method_description(0).contains("downloads"));
        assert!(sync_method_description(1).starts_with("Uploads new photos"));
        assert!(sync_method_description(2).starts_with("Downloads"));
    }

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
        assert_eq!(initial_subtitle(""), "");
        assert_eq!(initial_subtitle("~/Pictures"), "~/Pictures");

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
