//! Folder rows. A folder with subfolders expands in place to show them, and Open steps into
//! it; one without shows its photos. Every row's menu can copy the folder's path.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::Cell;
use std::rc::Rc;

use libadwaita::prelude::*;

use super::super::LibraryWindowUi;
use super::super::context_menu::{Menu, menu_key};
use super::super::tree_page::leaf_name;
use super::{copy_path, open_path, open_photos, subfolders};

/// The row for `path`: expandable when it has subfolders.
pub(super) fn folder_row(ui: &Rc<LibraryWindowUi>, path: &str) -> gtk::Widget {
    let children = subfolders(&ui.folders.folders.borrow(), Some(path)).len();
    let row: gtk::Widget = if children == 0 {
        leaf_row(ui, path).upcast()
    } else {
        expander_row(ui, path, children).upcast()
    };
    connect_menu(ui, &row, path);
    row
}

/// "Photos in {folder}": the folder's own photos, above its subfolders.
pub(super) fn photos_row(ui: &Rc<LibraryWindowUi>, path: &str) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .title(fill(
            gettext("Photos in {folder}"),
            &[("folder", leaf_name(path))],
        ))
        .use_markup(false)
        .title_lines(1)
        .activatable(true)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name("image-x-generic-symbolic"));
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let (ui, path) = (ui.clone(), path.to_string());
    row.connect_activated(move |_| open_photos(&ui, &path));
    row
}

fn leaf_row(ui: &Rc<LibraryWindowUi>, path: &str) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .title(leaf_name(path))
        .use_markup(false)
        .title_lines(1)
        .activatable(true)
        .tooltip_text(gettext("Show photos"))
        .build();
    row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
    let (ui, path) = (ui.clone(), path.to_string());
    row.connect_activated(move |_| open_photos(&ui, &path));
    row
}

fn expander_row(ui: &Rc<LibraryWindowUi>, path: &str, children: usize) -> libadwaita::ExpanderRow {
    let row = libadwaita::ExpanderRow::builder()
        .title(leaf_name(path))
        .use_markup(false)
        .subtitle(fill(
            ngettext("{n} folder", "{n} folders", plural_n(children)),
            &[("n", &children.to_string())],
        ))
        .build();
    row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
    let open = gtk::Button::builder()
        .icon_name("go-next-symbolic")
        .tooltip_text(fill(gettext("Open {name}"), &[("name", leaf_name(path))]))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let (ui_open, path_open) = (ui.clone(), path.to_string());
    open.connect_clicked(move |_| open_path(&ui_open, Some(path_open.clone())));
    row.add_suffix(&open);
    // Filled on first expand, so a large library costs nothing until it's browsed.
    let filled = Cell::new(false);
    let (ui, path) = (ui.clone(), path.to_string());
    row.connect_expanded_notify(move |row| {
        if row.is_expanded() && !filled.replace(true) {
            fill_children(&ui, row, &path);
        }
    });
    row
}

fn fill_children(ui: &Rc<LibraryWindowUi>, row: &libadwaita::ExpanderRow, path: &str) {
    if ui.folders.asset_folders.borrow().contains(path) {
        row.add_row(&photos_row(ui, path));
    }
    let children: Vec<String> = subfolders(&ui.folders.folders.borrow(), Some(path))
        .into_iter()
        .map(str::to_string)
        .collect();
    for child in &children {
        row.add_row(&folder_row(ui, child));
    }
}

/// Copy Path, and Show Photos when the folder has its own, on right-click, long-press, or
/// Shift+F10. The click is claimed so a folder inside an expanded one opens only its menu.
fn connect_menu(ui: &Rc<LibraryWindowUi>, row: &gtk::Widget, path: &str) {
    let open = {
        let (ui, row, path) = (ui.clone(), row.downgrade(), path.to_string());
        Rc::new(move |x: f64, y: f64| {
            if let Some(row) = row.upgrade() {
                show_menu(&ui, &row, &path, (x, y));
            }
        })
    };
    let click = gtk::GestureClick::builder()
        .button(gtk::gdk::BUTTON_SECONDARY)
        .build();
    let on_click = open.clone();
    click.connect_pressed(move |gesture, _, x, y| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        on_click(x, y);
    });
    row.add_controller(click);
    let press = gtk::GestureLongPress::builder().touch_only(true).build();
    let on_press = open.clone();
    press.connect_pressed(move |gesture, x, y| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        on_press(x, y);
    });
    row.add_controller(press);
    let weak = row.downgrade();
    row.add_controller(menu_key(move || {
        if let Some(row) = weak.upgrade() {
            open(f64::from(row.width()) / 2.0, f64::from(row.height()) / 2.0);
        }
    }));
}

fn show_menu(ui: &Rc<LibraryWindowUi>, row: &gtk::Widget, path: &str, (x, y): (f64, f64)) {
    let menu = Menu::new(row, x, y);
    menu.heading(leaf_name(path));
    let (ui_copy, path_copy) = (ui.clone(), path.to_string());
    menu.add_with_icon(
        "edit-copy-symbolic",
        &gettext("Copy Path"),
        false,
        move || {
            copy_path(&ui_copy, &path_copy);
        },
    );
    if ui.folders.asset_folders.borrow().contains(path) {
        let (ui_photos, path_photos) = (ui.clone(), path.to_string());
        menu.add_with_icon(
            "image-x-generic-symbolic",
            &gettext("Show Photos"),
            false,
            move || open_photos(&ui_photos, &path_photos),
        );
    }
    menu.popup();
}
