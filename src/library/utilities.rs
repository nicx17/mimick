//! Large Files, Missing Location, and Unusual Dates, kept to their one task as Immich's
//! Utilities are: their filters and main action up front, none of the everyday actions.

mod controls;
pub(super) mod filters;

use gettextrs::{gettext, ngettext};
use std::rc::Rc;

use gtk::prelude::*;

use super::LibraryWindowUi;
use super::asset_object::AssetObject;
use super::state::{LibrarySource, Location};
use crate::i18n::{fill, plural_n};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Utility {
    LargeFiles,
    MissingLocation,
    UnusualDates,
}

fn utility(source: &LibrarySource) -> Option<Utility> {
    match source {
        LibrarySource::LargeFiles { .. } => Some(Utility::LargeFiles),
        LibrarySource::MissingLocation { .. } => Some(Utility::MissingLocation),
        LibrarySource::UnusualDates { .. } => Some(Utility::UnusualDates),
        _ => None,
    }
}

fn current(ui: &LibraryWindowUi) -> Option<Utility> {
    match super::navigation::current(ui) {
        Location::Source(source) => utility(&source),
        Location::Page(_) => None,
    }
}

/// Utilities skip the slideshow: their photos are there to be fixed, not watched.
pub(super) fn is_utility(source: &LibrarySource) -> bool {
    utility(source).is_some()
}

/// The sidebar row a utility belongs to.
pub(super) fn sidebar_key(source: &LibrarySource) -> &'static str {
    match utility(source) {
        Some(Utility::LargeFiles) => "large-files",
        Some(Utility::MissingLocation) => "missing-location",
        _ => "unusual-dates",
    }
}

/// The main action's icon and label, for the header and the selection bar.
fn task(utility: Utility) -> Option<(&'static str, String)> {
    match utility {
        Utility::LargeFiles => None,
        Utility::MissingLocation => Some(("mark-location-symbolic", gettext("Add Location"))),
        Utility::UnusualDates => Some(("x-office-calendar-symbolic", gettext("Change Date"))),
    }
}

/// The selection bar button the main action stands for.
fn task_target(ui: &LibraryWindowUi) -> Option<gtk::Button> {
    match current(ui)? {
        Utility::MissingLocation => Some(ui.bulk.location.clone()),
        Utility::UnusualDates => Some(ui.bulk.date.clone()),
        Utility::LargeFiles => None,
    }
}

/// Show each utility's own controls, and hide the everyday selection actions there.
pub(super) fn sync(ui: &LibraryWindowUi, location: &Location) {
    let shown = match location {
        Location::Source(source) => utility(source),
        Location::Page(_) => None,
    };
    ui.grid
        .canvas
        .set_show_file_sizes(shown == Some(Utility::LargeFiles));
    ui.bulk.general.set_visible(shown.is_none());
    let main = shown.and_then(task);
    ui.bulk.task.set_visible(main.is_some());
    if let Some((icon, label)) = main {
        ui.bulk.task_content.set_icon_name(icon);
        ui.bulk.task_content.set_label(&label);
    }
}

/// The selection bar's labelled main action clicks the utility's own bulk button.
pub(super) fn connect(ui: &Rc<LibraryWindowUi>) {
    let weak = Rc::downgrade(ui);
    ui.bulk.task.connect_clicked(move |_| {
        if let Some(target) = weak.upgrade().as_deref().and_then(task_target) {
            target.emit_clicked();
        }
    });
}

/// The header's filters, then the main action, for a utility; nothing for other views.
pub(super) fn header_actions(ui: &Rc<LibraryWindowUi>, source: &LibrarySource) -> Vec<gtk::Widget> {
    let mut widgets = match source {
        LibrarySource::LargeFiles { filter } => controls::large_files(ui, *filter),
        LibrarySource::MissingLocation { range } => {
            let detail = controls::range_text(range);
            super::source_header::set_details(&ui.header, detail.into_iter().collect());
            vec![controls::dates_button(ui, range)]
        }
        LibrarySource::UnusualDates { which } => {
            let detail = controls::odd_dates_text(*which);
            super::source_header::set_details(&ui.header, vec![detail]);
            vec![controls::odd_dates_toggle(ui, *which)]
        }
        _ => return Vec::new(),
    };
    widgets.extend(utility(source).and_then(|utility| main_button(ui, utility)));
    widgets
}

/// The main action: act on the selection, or start selecting with a hint.
fn main_button(ui: &Rc<LibraryWindowUi>, utility: Utility) -> Option<gtk::Widget> {
    let (icon, label) = task(utility)?;
    let hint = match utility {
        Utility::UnusualDates => gettext("Select photos, then Change Date"),
        _ => gettext("Select photos, then Add Location"),
    };
    let button = gtk::Button::builder()
        .child(
            &libadwaita::ButtonContent::builder()
                .icon_name(icon)
                .label(label)
                .build(),
        )
        .valign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .build();
    let ui = ui.clone();
    button.connect_clicked(move |_| {
        if ui.grid.selection.selection().size() > 0 {
            ui.bulk.task.emit_clicked();
            return;
        }
        ui.select_toggle.set_active(true);
        ui.toasts.add_toast(libadwaita::Toast::new(&hint));
    });
    Some(button.upcast())
}

/// After `then`, fixed photos leave Missing Location or Unusual Dates; Large Files keeps them.
pub(super) fn after_fixing(
    ui: &Rc<LibraryWindowUi>,
    ids: Vec<String>,
    then: Rc<dyn Fn()>,
) -> Rc<dyn Fn()> {
    let ui = ui.clone();
    Rc::new(move || {
        then();
        if matches!(
            current(&ui),
            Some(Utility::MissingLocation | Utility::UnusualDates)
        ) {
            super::actions::remove_assets_from_grid(&ui, &ids);
        }
    })
}

/// Large Files' header: the listed files' combined size beside their count.
pub(super) fn show_total_size(ui: &LibraryWindowUi) {
    let total: u64 = {
        let state = ui.ctx.library_state.lock();
        if utility(&state.source) != Some(Utility::LargeFiles) {
            return;
        }
        let sizes = state.assets.iter().filter_map(|asset| {
            let exif = asset.exif_info.as_ref()?;
            exif.file_size_in_byte
        });
        sizes.sum()
    };
    let text = fill(
        gettext("{size} in all"),
        &[("size", &glib::format_size(total))],
    );
    super::source_header::set_details(&ui.header, vec![text]);
}

/// "3 selected", and with `sizes` (Large Files) how much space they take.
pub(super) fn selection_text(selection: &gtk::MultiSelection, n: u32, sizes: bool) -> String {
    let count = fill(
        ngettext("{n} selected", "{n} selected", plural_n(n)),
        &[("n", &n.to_string())],
    );
    if !sizes {
        return count;
    }
    let picked = selection.selection();
    let bytes: u64 = (0..picked.size())
        .filter_map(|i| selection.item(picked.nth(i as u32)))
        .filter_map(|item| item.downcast::<AssetObject>().ok())
        .map(|item| item.file_size())
        .sum();
    format!("{count} · {}", glib::format_size(bytes))
}
