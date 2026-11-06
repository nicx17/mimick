//! Duplicates page: review one group of look-alike photos at a time, keep some, trash the rest.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use gtk::prelude::*;

use super::LibraryWindowUi;
use super::context_menu::show_error_toast;

mod cards;

use crate::api_client::DuplicateGroup;
use cards::{arrange, card, file_size, initial_keep};

pub(super) struct DuplicatesView {
    pub root: gtk::Stack,
    position: gtk::Label,
    prev: gtk::Button,
    next: gtk::Button,
    /// Holds the shown group's cards, laid out for the window's width.
    stage: gtk::Box,
    dismiss: gtk::Button,
    resolve: gtk::Button,
    empty: libadwaita::StatusPage,
    groups: RefCell<Vec<DuplicateGroup>>,
    index: Cell<usize>,
    keep: RefCell<HashSet<String>>,
    /// Group the keep choices belong to, so laying it out again doesn't undo them.
    keep_group: RefCell<String>,
    busy: Cell<bool>,
}

pub(super) fn build_duplicates_view() -> DuplicatesView {
    let position = gtk::Label::builder()
        .hexpand(true)
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    let prev = nav_button("go-previous-symbolic", &gettext("Previous group"));
    let next = nav_button("go-next-symbolic", &gettext("Next group"));
    let stage = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .build();
    let (dismiss, resolve) = choice_buttons();
    let empty = libadwaita::StatusPage::builder()
        .icon_name("edit-copy-symbolic")
        .vexpand(true)
        .build();
    let review = review_layout(&ReviewParts {
        position: &position,
        prev: &prev,
        next: &next,
        stage: &stage,
        dismiss: &dismiss,
        resolve: &resolve,
    });
    let root = gtk::Stack::builder().vexpand(true).build();
    root.add_named(&super::build_loading_view(), Some("loading"));
    root.add_named(&empty, Some("empty"));
    root.add_named(&review, Some("review"));
    DuplicatesView {
        root,
        position,
        prev,
        next,
        stage,
        dismiss,
        resolve,
        empty,
        groups: RefCell::default(),
        index: Cell::new(0),
        keep: RefCell::default(),
        keep_group: RefCell::default(),
        busy: Cell::new(false),
    }
}

/// Not Duplicates keeps every photo; the other trashes the ones marked, and `sync_buttons` names it.
fn choice_buttons() -> (gtk::Button, gtk::Button) {
    let dismiss = gtk::Button::builder()
        .label(gettext("Not Duplicates"))
        .tooltip_text(gettext("Keep every photo and stop showing this group"))
        .build();
    let resolve = gtk::Button::builder()
        .css_classes(["destructive-action"])
        .build();
    (dismiss, resolve)
}

fn show_empty(view: &DuplicatesView) {
    view.empty.set_title(&gettext("No Duplicates"));
    view.empty.set_description(Some(&gettext(
        "Immich hasn't found any duplicate photos. It looks for them when its duplicate detection job runs.",
    )));
    view.root.set_visible_child_name("empty");
}

fn nav_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .css_classes(["flat", "circular"])
        .build()
}

struct ReviewParts<'a> {
    position: &'a gtk::Label,
    prev: &'a gtk::Button,
    next: &'a gtk::Button,
    stage: &'a gtk::Box,
    dismiss: &'a gtk::Button,
    resolve: &'a gtk::Button,
}

/// Group counter, arrows, and a hint on top; the photos fill the space; the choices at the bottom.
fn review_layout(parts: &ReviewParts<'_>) -> gtk::Box {
    let hint = gtk::Label::builder()
        .label(gettext("Tap a photo to keep it or trash it."))
        .xalign(1.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["dim-label"])
        .build();
    let top = gtk::Box::builder()
        .spacing(6)
        .margin_top(12)
        .margin_start(18)
        .margin_end(18)
        .build();
    top.append(parts.position);
    top.append(parts.prev);
    top.append(parts.next);
    top.append(&hint);
    parts.stage.set_margin_top(12);
    parts.stage.set_margin_bottom(12);
    parts.stage.set_margin_start(18);
    parts.stage.set_margin_end(18);
    // Wraps when large text makes the pair wider than a phone.
    let buttons = libadwaita::WrapBox::builder()
        .child_spacing(8)
        .line_spacing(8)
        .align(1.0)
        .halign(gtk::Align::End)
        .margin_top(8)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(18)
        .build();
    buttons.append(parts.dismiss);
    buttons.append(parts.resolve);
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    root.append(&top);
    root.append(parts.stage);
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    root.append(&buttons);
    root
}

pub(super) fn connect_duplicates_view(ui: Rc<LibraryWindowUi>) {
    let view = &ui.duplicates;
    let ui_prev = ui.clone();
    view.prev.connect_clicked(move |_| step(&ui_prev, false));
    let ui_next = ui.clone();
    view.next.connect_clicked(move |_| step(&ui_next, true));
    let ui_dismiss = ui.clone();
    view.dismiss
        .connect_clicked(move |_| dismiss_current(&ui_dismiss));
    let ui_resolve = ui.clone();
    view.resolve
        .connect_clicked(move |_| confirm_resolve(&ui_resolve));
    // Phone and desktop layouts differ, so the group is laid out again across the 600 px line.
    let ui_layout = ui.clone();
    ui.split.connect_collapsed_notify(move |_| {
        let page = ui_layout.content_stack.visible_child_name();
        let review = ui_layout.duplicates.root.visible_child_name();
        if page.as_deref() == Some("duplicates") && review.as_deref() == Some("review") {
            render(&ui_layout);
        }
    });
}

pub(super) fn show(ui: &Rc<LibraryWindowUi>) {
    ui.content_stack.set_visible_child_name("duplicates");
    reload(ui);
}

fn reload(ui: &Rc<LibraryWindowUi>) {
    let view = &ui.duplicates;
    if view.groups.borrow().is_empty() {
        view.root.set_visible_child_name("loading");
    }
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.fetch_duplicates().await {
            Ok(groups) => {
                let view = &ui.duplicates;
                let index = view.index.get().min(groups.len().saturating_sub(1));
                *view.groups.borrow_mut() = groups;
                view.index.set(index);
                render(&ui);
            }
            Err(err) => {
                ui.duplicates
                    .empty
                    .set_title(&gettext("Could Not Load Duplicates"));
                ui.duplicates.empty.set_description(Some(&fill(
                    gettext("{err}\n\nYour API key needs the '{permission}' permission."),
                    &[("err", &err), ("permission", "duplicate.read")],
                )));
                ui.duplicates.root.set_visible_child_name("empty");
            }
        }
    });
}

fn step(ui: &Rc<LibraryWindowUi>, forward: bool) {
    let view = &ui.duplicates;
    let n = view.groups.borrow().len();
    let index = view.index.get();
    let target = if forward {
        (index + 1).min(n.saturating_sub(1))
    } else {
        index.saturating_sub(1)
    };
    if target != index {
        view.index.set(target);
        render(ui);
    }
}

fn render(ui: &Rc<LibraryWindowUi>) {
    let view = &ui.duplicates;
    while let Some(child) = view.stage.first_child() {
        view.stage.remove(&child);
    }
    let groups = view.groups.borrow();
    let Some(group) = groups.get(view.index.get()) else {
        show_empty(view);
        return;
    };
    if *view.keep_group.borrow() != group.duplicate_id {
        *view.keep.borrow_mut() = initial_keep(group);
        *view.keep_group.borrow_mut() = group.duplicate_id.clone();
    }
    let largest = group.assets.iter().map(file_size).max().unwrap_or(0);
    let narrow = ui.split.is_collapsed();
    let cards: Vec<gtk::ToggleButton> = group
        .assets
        .iter()
        .map(|asset| card(ui, asset, narrow, largest))
        .collect();
    view.stage.append(&arrange(&cards, narrow));
    view.position.set_label(&fill(
        gettext("Group {n} of {total}"),
        &[
            ("n", &(view.index.get() + 1).to_string()),
            ("total", &groups.len().to_string()),
        ],
    ));
    view.prev.set_sensitive(view.index.get() > 0);
    view.next.set_sensitive(view.index.get() + 1 < groups.len());
    drop(groups);
    sync_buttons(ui);
    view.root.set_visible_child_name("review");
}

/// The trash button names how many photos go; it needs one kept and one trashed.
fn sync_buttons(ui: &LibraryWindowUi) {
    let view = &ui.duplicates;
    let (keep, trash) = current_split(ui);
    let n = trash.len();
    let template = if ui.features.get().trash {
        ngettext("Trash {n} Photo", "Trash {n} Photos", plural_n(n))
    } else {
        ngettext("Delete {n} Photo", "Delete {n} Photos", plural_n(n))
    };
    view.resolve
        .set_label(&fill(template, &[("n", &n.to_string())]));
    view.resolve
        .set_sensitive(!keep.is_empty() && n > 0 && !view.busy.get());
    view.dismiss.set_sensitive(!view.busy.get());
}

/// Ids to keep and to trash in the shown group.
fn current_split(ui: &LibraryWindowUi) -> (Vec<String>, Vec<String>) {
    let view = &ui.duplicates;
    let groups = view.groups.borrow();
    let Some(group) = groups.get(view.index.get()) else {
        return (Vec::new(), Vec::new());
    };
    let keep = view.keep.borrow();
    group
        .assets
        .iter()
        .map(|asset| asset.id.clone())
        .partition(|id| keep.contains(id))
}

fn current_id(ui: &LibraryWindowUi) -> Option<String> {
    let view = &ui.duplicates;
    let groups = view.groups.borrow();
    groups
        .get(view.index.get())
        .map(|group| group.duplicate_id.clone())
}

/// Permanent deletion (trash turned off on the server) asks first, as everywhere else.
fn confirm_resolve(ui: &Rc<LibraryWindowUi>) {
    if ui.features.get().trash {
        resolve_current(ui);
        return;
    }
    let (_, trash) = current_split(ui);
    let heading = fill(
        ngettext(
            "Delete {n} photo permanently?",
            "Delete {n} photos permanently?",
            plural_n(trash.len()),
        ),
        &[("n", &trash.len().to_string())],
    );
    let ui_delete = ui.clone();
    super::dialogs::confirm(
        ui,
        &heading,
        &gettext("Trash is turned off on this server, so this can't be undone."),
        &gettext("Delete"),
        move || resolve_current(&ui_delete),
    );
}

fn resolve_current(ui: &Rc<LibraryWindowUi>) {
    let Some(id) = current_id(ui) else {
        return;
    };
    let (keep, trash) = current_split(ui);
    run_change(ui, id.clone(), async move |ui: Rc<LibraryWindowUi>| {
        ui.ctx
            .api_client
            .resolve_duplicate(&id, &keep, &trash)
            .await
            .map(|()| {
                fill(
                    ngettext(
                        "Kept {kept}, trashed {n}",
                        "Kept {kept}, trashed {n}",
                        plural_n(trash.len()),
                    ),
                    &[
                        ("kept", &keep.len().to_string()),
                        ("n", &trash.len().to_string()),
                    ],
                )
            })
    });
}

fn dismiss_current(ui: &Rc<LibraryWindowUi>) {
    let Some(id) = current_id(ui) else {
        return;
    };
    run_change(ui, id.clone(), async move |ui: Rc<LibraryWindowUi>| {
        ui.ctx
            .api_client
            .dismiss_duplicate(&id)
            .await
            .map(|()| gettext("Marked as not duplicates"))
    });
}

/// Runs a change to group `id`; on success the group leaves the list and the next one shows.
fn run_change<F, Fut>(ui: &Rc<LibraryWindowUi>, id: String, change: F)
where
    F: FnOnce(Rc<LibraryWindowUi>) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<String, String>> + 'static,
{
    let view = &ui.duplicates;
    if view.busy.replace(true) {
        return;
    }
    sync_buttons(ui);
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let result = change(ui.clone()).await;
        ui.duplicates.busy.set(false);
        match result {
            Ok(message) => {
                let view = &ui.duplicates;
                view.groups.borrow_mut().retain(|g| g.duplicate_id != id);
                let last = view.groups.borrow().len().saturating_sub(1);
                view.index.set(view.index.get().min(last));
                render(&ui);
                ui.toasts.add_toast(libadwaita::Toast::new(&message));
            }
            Err(err) => {
                sync_buttons(&ui);
                show_error_toast(&ui, &gettext("Couldn't update the duplicates"), &err);
            }
        }
    });
}
