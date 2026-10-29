//! Selection toolbar construction for the library window.

use gtk::prelude::*;

use super::{BulkParts, hbox};

struct BulkActions {
    delete: gtk::Button,
    download: gtk::Button,
    clear: gtk::Button,
    share_link: gtk::Button,
    location: gtk::Button,
    date: gtk::Button,
    add_to_album: gtk::Button,
    remove_from_album: gtk::Button,
    archive: gtk::Button,
    trash: super::super::trash_view::TrashControls,
}

fn build_bulk_actions() -> BulkActions {
    let (delete, download, clear, share_link) = super::bulk_buttons();
    let archive = gtk::Button::builder()
        .icon_name("mimick-archive-symbolic")
        .tooltip_text("Archive selected")
        .build();
    let location = gtk::Button::builder()
        .icon_name("mark-location-symbolic")
        .tooltip_text("Set location for selected")
        .build();
    let date = gtk::Button::builder()
        .icon_name("x-office-calendar-symbolic")
        .tooltip_text("Change date for selected")
        .build();
    let add_to_album = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add to album")
        .build();
    let remove_from_album = gtk::Button::builder()
        .icon_name("list-remove-symbolic")
        .tooltip_text("Remove from album")
        .visible(false)
        .build();
    let trash = super::super::trash_view::build_trash_controls(vec![
        delete.clone(),
        download.clone(),
        share_link.clone(),
        archive.clone(),
        location.clone(),
        date.clone(),
        add_to_album.clone(),
    ]);
    BulkActions {
        delete,
        download,
        clear,
        share_link,
        location,
        date,
        add_to_album,
        remove_from_album,
        archive,
        trash,
    }
}

pub(super) fn build_bulk_bar() -> BulkParts {
    let count_label = gtk::Label::builder()
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let actions = build_bulk_actions();
    let (bar, secondary, overflow_box) = assemble_bulk_bar(&count_label, &actions);
    BulkParts {
        bar,
        count_label,
        delete: actions.delete,
        download: actions.download,
        clear: actions.clear,
        share_link: actions.share_link,
        location: actions.location,
        date: actions.date,
        add_to_album: actions.add_to_album,
        remove_from_album: actions.remove_from_album,
        archive: actions.archive,
        secondary,
        overflow_box,
        trash: actions.trash,
    }
}

fn assemble_bulk_bar(
    count_label: &gtk::Label,
    actions: &BulkActions,
) -> (gtk::Revealer, gtk::Box, gtk::Box) {
    let secondary_buttons = [
        actions.share_link.clone(),
        actions.archive.clone(),
        actions.location.clone(),
        actions.date.clone(),
        actions.remove_from_album.clone(),
    ];
    let secondary = hbox(&[]);
    for button in &secondary_buttons {
        secondary.append(button);
    }
    let overflow_box = hbox(&[super::super::bulk_overflow::build(&secondary_buttons).upcast_ref()]);
    overflow_box.set_visible(false);
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(16)
        .margin_start(12)
        .margin_end(12)
        .css_classes(vec!["toolbar".to_string()])
        .build();
    inner.append(count_label);
    inner.append(&actions.clear);
    inner.append(&secondary);
    inner.append(&overflow_box);
    inner.append(&actions.add_to_album);
    inner.append(&actions.download);
    inner.append(&actions.delete);
    inner.append(&actions.trash.restore_selected);
    inner.append(&actions.trash.delete_selected);
    let bar = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideUp)
        .reveal_child(false)
        .child(&inner)
        .build();
    (bar, secondary, overflow_box)
}
