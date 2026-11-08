//! Go to Path: the breadcrumbs swap for an entry that suggests folders as you type, as file
//! managers do on Ctrl+L. A path can be typed or pasted; Copy Path copies the open folder's.

use crate::i18n::fill;
use gettextrs::gettext;
use std::collections::BTreeSet;
use std::rc::Rc;

use gtk::prelude::*;

use super::super::LibraryWindowUi;
use super::super::tree_page::TreePage;
use super::{copy_path, open_path};

/// More would push the list past a phone's screen.
const MAX_SUGGESTIONS: usize = 8;

pub(in crate::library) struct PathBar {
    entry: gtk::Entry,
    toggle: gtk::ToggleButton,
    copy: gtk::Button,
    popover: gtk::Popover,
    suggestions: gtk::ListBox,
}

/// Add the entry beside the breadcrumbs, and the Go to Path and Copy Path buttons after them.
pub(super) fn build(page: &TreePage) -> PathBar {
    let entry = gtk::Entry::builder()
        .hexpand(true)
        .placeholder_text(gettext("Folder path"))
        .build();
    let suggestions = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .propagate_natural_width(true)
        .max_content_height(320)
        .child(&suggestions)
        .build();
    // Not autohide: that would take the keyboard away from the entry while typing.
    let popover = gtk::Popover::builder()
        .child(&scroller)
        .autohide(false)
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        .build();
    popover.set_parent(&entry);
    let toggle = gtk::ToggleButton::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(gettext("Enter a path (Ctrl+L)"))
        .build();
    let copy = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text(gettext("Copy path"))
        .build();
    page.toolbar
        .insert_child_after(&entry, Some(&page.crumb_scroller));
    page.toolbar.append(&toggle);
    page.toolbar.append(&copy);
    toggle
        .bind_property("active", &page.crumb_scroller, "visible")
        .invert_boolean()
        .sync_create()
        .build();
    toggle
        .bind_property("active", &entry, "visible")
        .sync_create()
        .build();
    PathBar {
        entry,
        toggle,
        copy,
        popover,
        suggestions,
    }
}

/// Wire the entry, its suggestions, both buttons, and Ctrl+L.
pub(super) fn connect(ui: &Rc<LibraryWindowUi>) {
    let bar = &ui.folders.path_bar;
    let popover = bar.popover.clone();
    bar.entry.connect_destroy(move |_| popover.unparent());
    let ui_toggle = ui.clone();
    bar.toggle
        .connect_toggled(move |toggle| on_toggled(&ui_toggle, toggle.is_active()));
    let ui_changed = ui.clone();
    bar.entry
        .connect_changed(move |_| refresh_suggestions(&ui_changed));
    let ui_go = ui.clone();
    bar.entry
        .connect_activate(move |entry| go_to(&ui_go, &entry.text()));
    bar.entry.add_controller(entry_keys(ui));
    let ui_pick = ui.clone();
    bar.suggestions.connect_row_activated(move |_, row| {
        if let Some(path) = row.child().and_downcast::<gtk::Label>() {
            go_to(&ui_pick, &path.label());
        }
    });
    let ui_copy = ui.clone();
    bar.copy.connect_clicked(move |_| {
        if let Some(path) = ui_copy.folders.path.borrow().clone() {
            copy_path(&ui_copy, &path);
        }
    });
    super::super::controls::add_shortcut(ui, "<Control>l", open_entry);
}

/// Copy Path only has something to copy inside a folder.
pub(super) fn sync(ui: &LibraryWindowUi) {
    let bar = &ui.folders.path_bar;
    bar.copy.set_sensitive(ui.folders.path.borrow().is_some());
}

/// Ctrl+L on the Folders page opens the entry.
fn open_entry(ui: &Rc<LibraryWindowUi>) -> bool {
    let shown = ui.folders.page.root.is_mapped();
    if shown {
        ui.folders.path_bar.toggle.set_active(true);
    }
    shown
}

fn on_toggled(ui: &Rc<LibraryWindowUi>, active: bool) {
    let bar = &ui.folders.path_bar;
    if !active {
        bar.popover.popdown();
        return;
    }
    let path = ui.folders.path.borrow().clone().unwrap_or_default();
    bar.entry.set_text(&path);
    bar.entry.grab_focus();
    bar.entry.set_position(-1);
    bar.popover.popdown();
}

/// Escape closes the entry, Tab completes as far as the suggestions agree, Down moves into them.
fn entry_keys(ui: &Rc<LibraryWindowUi>) -> gtk::EventControllerKey {
    let keys = gtk::EventControllerKey::new();
    let ui = ui.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        let bar = &ui.folders.path_bar;
        let handled = match key {
            gtk::gdk::Key::Escape => {
                bar.toggle.set_active(false);
                true
            }
            gtk::gdk::Key::Tab => complete(&ui),
            gtk::gdk::Key::Down => bar
                .suggestions
                .row_at_index(0)
                .is_some_and(|row| row.grab_focus()),
            _ => false,
        };
        if handled {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    keys
}

fn complete(ui: &Rc<LibraryWindowUi>) -> bool {
    let bar = &ui.folders.path_bar;
    let typed = bar.entry.text();
    let folders = ui.folders.folders.borrow();
    let matches = suggestions(&folders, &typed);
    let prefix = common_prefix(&matches);
    if prefix.chars().count() <= typed.chars().count() {
        return false;
    }
    bar.entry.set_text(&prefix);
    bar.entry.set_position(-1);
    true
}

fn refresh_suggestions(ui: &Rc<LibraryWindowUi>) {
    let bar = &ui.folders.path_bar;
    bar.suggestions.remove_all();
    let typed = bar.entry.text();
    let folders = ui.folders.folders.borrow();
    let matches = suggestions(&folders, &typed);
    for path in &matches {
        let label = gtk::Label::builder()
            .label(*path)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::Start)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        bar.suggestions.append(&label);
    }
    let exact = matches.len() == 1 && matches[0] == typed.as_str();
    if typed.is_empty() || matches.is_empty() || exact {
        bar.popover.popdown();
    } else {
        bar.popover.popup();
    }
}

fn go_to(ui: &Rc<LibraryWindowUi>, typed: &str) {
    let resolved = resolve(&ui.folders.folders.borrow(), typed);
    match resolved {
        Some(path) => {
            ui.folders.path_bar.toggle.set_active(false);
            open_path(ui, Some(path));
        }
        None => ui.toasts.add_toast(libadwaita::Toast::new(&fill(
            gettext("No folder at {path}"),
            &[("path", typed.trim())],
        ))),
    }
}

/// Folders whose path starts with `typed`, ignoring case, up to `MAX_SUGGESTIONS`.
fn suggestions<'a>(folders: &'a BTreeSet<String>, typed: &str) -> Vec<&'a str> {
    let typed = typed.trim().to_lowercase();
    folders
        .iter()
        .filter(|folder| folder.to_lowercase().starts_with(&typed))
        .take(MAX_SUGGESTIONS)
        .map(String::as_str)
        .collect()
}

/// The folder `typed` names, allowing a trailing slash, or the only one it could mean.
fn resolve(folders: &BTreeSet<String>, typed: &str) -> Option<String> {
    let typed = typed.trim();
    let bare = typed
        .strip_suffix('/')
        .filter(|rest| !rest.is_empty())
        .unwrap_or(typed);
    if folders.contains(bare) {
        return Some(bare.to_string());
    }
    match suggestions(folders, bare).as_slice() {
        [only] => Some((*only).to_string()),
        _ => None,
    }
}

/// The longest start every path shares, by character.
fn common_prefix(paths: &[&str]) -> String {
    let Some((first, rest)) = paths.split_first() else {
        return String::new();
    };
    let mut prefix: Vec<char> = first.chars().collect();
    for path in rest {
        let shared = prefix
            .iter()
            .zip(path.chars())
            .take_while(|(a, b)| **a == *b)
            .count();
        prefix.truncate(shared);
    }
    prefix.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn suggestions_match_the_start_of_the_path_in_any_case() {
        let set = folders(&["/photos/2023", "/photos/2024", "/scans"]);
        assert_eq!(
            suggestions(&set, "/Photos/"),
            ["/photos/2023", "/photos/2024"]
        );
        assert_eq!(suggestions(&set, "2024"), Vec::<&str>::new());
    }

    #[test]
    fn resolve_takes_an_exact_path_or_the_only_match() {
        let set = folders(&["/photos", "/photos/2023", "/photos/2024", "/scans"]);
        assert_eq!(resolve(&set, "/photos/").as_deref(), Some("/photos"));
        assert_eq!(resolve(&set, "/sc").as_deref(), Some("/scans"));
        assert_eq!(resolve(&set, "/photos/20"), None);
        assert_eq!(resolve(&set, "/nowhere"), None);
    }

    #[test]
    fn common_prefix_stops_where_paths_differ() {
        assert_eq!(
            common_prefix(&["/photos/2023", "/photos/2024"]),
            "/photos/202"
        );
        assert_eq!(common_prefix(&["/a"]), "/a");
        assert_eq!(common_prefix(&[]), "");
    }
}
