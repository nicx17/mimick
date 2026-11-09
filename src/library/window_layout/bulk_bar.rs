//! Selection toolbar construction for the library window.

use gettextrs::gettext;
use gtk::prelude::*;

use super::hbox;
use crate::api_client::AssetJob;
use crate::library::trash_view::TrashControls;

/// Bottom selection bar; `delete`, `download`, and `clear` are wired by `connect_bulk_actions`.
pub(in crate::library) struct BulkParts {
    pub(in crate::library) bar: gtk::Revealer,
    pub(in crate::library) count_label: gtk::Label,
    pub(in crate::library) delete: gtk::Button,
    pub(in crate::library) download: gtk::Button,
    pub(in crate::library) clear: gtk::Button,
    pub(in crate::library) share_link: gtk::Button,
    pub(in crate::library) location: gtk::Button,
    pub(in crate::library) date: gtk::Button,
    pub(in crate::library) add_to_album: gtk::Button,
    pub(in crate::library) remove_from_album: gtk::Button,
    pub(in crate::library) archive: gtk::Button,
    pub(in crate::library) stack: gtk::Button,
    pub(in crate::library) favorite: gtk::Button,
    pub(in crate::library) description: gtk::Button,
    pub(in crate::library) jobs: Vec<(AssetJob, gtk::Button)>,
    /// The everyday actions; the Utilities hide them to keep to their one task.
    pub(in crate::library) general: gtk::Box,
    /// A utility's labelled main action (Add Location, Change Date); `utilities` labels it.
    pub(in crate::library) task: gtk::Button,
    pub(in crate::library) task_content: libadwaita::ButtonContent,
    pub(in crate::library::window_layout) secondary: gtk::Box,
    pub(in crate::library) trash: TrashControls,
}

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
    stack: gtk::Button,
    favorite: gtk::Button,
    /// Only in the More menu: Edit Description, then each Immich job.
    description: gtk::Button,
    jobs: Vec<(AssetJob, gtk::Button)>,
    trash: TrashControls,
}

fn bar_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build()
}

fn build_bulk_actions() -> BulkActions {
    let (delete, download, clear, share_link) = bulk_buttons();
    let archive = bar_button("mimick-archive-symbolic", &gettext("Archive selected"));
    let location = bar_button(
        "mark-location-symbolic",
        &gettext("Set location for selected"),
    );
    let date = bar_button(
        "x-office-calendar-symbolic",
        &gettext("Change date for selected"),
    );
    let add_to_album = bar_button("list-add-symbolic", &gettext("Add to album"));
    let stack = bar_button("mimick-stack-symbolic", &gettext("Stack selected"));
    stack.set_visible(false);
    let remove_from_album = bar_button("list-remove-symbolic", &gettext("Remove from album"));
    remove_from_album.set_visible(false);
    let favorite = bar_button(
        "mimick-heart-outline-symbolic",
        &gettext("Favorite selected"),
    );
    let description = bar_button(
        "format-justify-left-symbolic",
        &gettext("Edit Description…"),
    );
    let jobs = super::super::asset_jobs::bulk_buttons();
    let mut live_only = vec![
        delete.clone(),
        download.clone(),
        share_link.clone(),
        archive.clone(),
        location.clone(),
        date.clone(),
        add_to_album.clone(),
        favorite.clone(),
        description.clone(),
    ];
    live_only.extend(jobs.iter().map(|(_, button)| button.clone()));
    let trash = super::super::trash_view::build_trash_controls(live_only);
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
        stack,
        favorite,
        description,
        jobs,
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
    let task_content = libadwaita::ButtonContent::builder()
        .can_shrink(true)
        .build();
    let task = gtk::Button::builder()
        .child(&task_content)
        .css_classes(["suggested-action"])
        .visible(false)
        .build();
    let (bar, general, secondary) = assemble_bulk_bar(&count_label, &actions, &task);
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
        stack: actions.stack,
        favorite: actions.favorite,
        description: actions.description,
        jobs: actions.jobs,
        general,
        task,
        task_content,
        secondary,
        trash: actions.trash,
    }
}

fn assemble_bulk_bar(
    count_label: &gtk::Label,
    actions: &BulkActions,
    task: &gtk::Button,
) -> (gtk::Revealer, gtk::Box, gtk::Box) {
    let (general, secondary) = general_actions(actions);
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
    inner.append(&general);
    inner.append(task);
    inner.append(&actions.download);
    inner.append(&actions.delete);
    inner.append(&actions.trash.restore_selected);
    inner.append(&actions.trash.delete_selected);
    let bar = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideUp)
        .reveal_child(false)
        .child(&inner)
        .build();
    (bar, general, secondary)
}

/// The everyday actions in one box, and the row of them that phone widths fold into More.
fn general_actions(actions: &BulkActions) -> (gtk::Box, gtk::Box) {
    let secondary_buttons = [
        actions.favorite.clone(),
        actions.share_link.clone(),
        actions.archive.clone(),
        actions.location.clone(),
        actions.date.clone(),
        actions.stack.clone(),
        actions.remove_from_album.clone(),
    ];
    let secondary = hbox(&[]);
    for button in &secondary_buttons {
        secondary.append(button);
    }
    let mut extras = vec![actions.description.clone()];
    extras.extend(actions.jobs.iter().map(|(_, button)| button.clone()));
    let overflow = super::super::bulk_overflow::build(&secondary_buttons, &secondary, &extras);
    let general = hbox(&[
        secondary.upcast_ref(),
        hbox(&[overflow.upcast_ref()]).upcast_ref(),
        actions.add_to_album.upcast_ref(),
    ]);
    (general, secondary)
}

/// Delete, download, clear, and share-link buttons for the selection bar.
fn bulk_buttons() -> (gtk::Button, gtk::Button, gtk::Button, gtk::Button) {
    let delete = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Delete selected"))
        .css_classes(vec!["destructive-action".to_string()])
        .build();
    let download = gtk::Button::builder()
        .icon_name("mimick-download-symbolic")
        .tooltip_text(gettext("Download selected"))
        .build();
    let clear = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .tooltip_text(gettext("Clear selection"))
        .css_classes(vec!["flat".to_string()])
        .build();
    let share_link = gtk::Button::builder()
        .icon_name("mimick-share-symbolic")
        .tooltip_text(gettext("Share selected"))
        .build();
    (delete, download, clear, share_link)
}
