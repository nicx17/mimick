//! Duplicates page: review one group of look-alike photos at a time, keep some, trash the rest.

use crate::i18n::{fill, format_day, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use gtk::prelude::*;

use super::LibraryWindowUi;
use super::context_menu::show_error_toast;
use crate::api_client::{DuplicateGroup, LibraryAsset};

/// Photo height in the two-column grid on phone-width windows.
const NARROW_THUMB: i32 = 170;
/// Narrowest a photo gets side by side; more photos than fit scroll sideways.
const WIDE_CARD_MIN: i32 = 260;

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
    let dismiss = gtk::Button::builder()
        .label(gettext("Not Duplicates"))
        .tooltip_text(gettext("Keep every photo and stop showing this group"))
        .build();
    let resolve = gtk::Button::builder()
        .css_classes(["destructive-action"])
        .build();
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

/// Phones: two columns that scroll down. Wider: side by side, as large as the window allows.
fn arrange(cards: &[gtk::ToggleButton], narrow: bool) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::builder().vexpand(true).build();
    if narrow {
        let grid = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(2)
            .max_children_per_line(2)
            .column_spacing(12)
            .row_spacing(12)
            .valign(gtk::Align::Start)
            .build();
        for card in cards {
            grid.append(card);
        }
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_child(Some(&grid));
    } else {
        let row = gtk::Box::builder().spacing(16).homogeneous(true).build();
        for card in cards {
            row.append(card);
        }
        scroller.set_vscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_child(Some(&row));
    }
    scroller
}

/// Immich's suggestion, else the largest file, so there's always one photo to keep.
fn initial_keep(group: &DuplicateGroup) -> HashSet<String> {
    let ids: HashSet<&str> = group.assets.iter().map(|a| a.id.as_str()).collect();
    let suggested: HashSet<String> = group
        .suggested_keep_asset_ids
        .iter()
        .filter(|id| ids.contains(id.as_str()))
        .cloned()
        .collect();
    if !suggested.is_empty() {
        return suggested;
    }
    group
        .assets
        .iter()
        .max_by_key(|asset| file_size(asset))
        .map(|asset| HashSet::from([asset.id.clone()]))
        .unwrap_or_default()
}

fn file_size(asset: &LibraryAsset) -> u64 {
    asset
        .exif_info
        .as_ref()
        .and_then(|exif| exif.file_size_in_byte)
        .unwrap_or(0)
}

fn card(
    ui: &Rc<LibraryWindowUi>,
    asset: &LibraryAsset,
    narrow: bool,
    largest: u64,
) -> gtk::ToggleButton {
    let (frame, verdict) = photo_frame(ui, asset, narrow);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    content.append(&frame);
    for (text, class) in card_facts(asset, largest) {
        content.append(
            &gtk::Label::builder()
                .label(&text)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .tooltip_text(&text)
                .css_classes([class])
                .build(),
        );
    }
    let keep = ui.duplicates.keep.borrow().contains(&asset.id);
    let button = gtk::ToggleButton::builder()
        .child(&content)
        .active(keep)
        .css_classes(["card", "mimick-duplicate-card"])
        .build();
    if !narrow {
        button.set_width_request(WIDE_CARD_MIN);
    }
    show_verdict(&button, &verdict);
    let (ui, id) = (ui.clone(), asset.id.clone());
    button.connect_toggled(move |button| {
        let mut keep = ui.duplicates.keep.borrow_mut();
        if button.is_active() {
            keep.insert(id.clone());
        } else {
            keep.remove(&id);
        }
        drop(keep);
        show_verdict(button, &verdict);
        sync_buttons(&ui);
    });
    button
}

/// The photo with its Keep or Trash label. Wide layouts show the whole photo at preview
/// quality, filling the height; phones crop a thumbnail to a fixed height.
fn photo_frame(
    ui: &Rc<LibraryWindowUi>,
    asset: &LibraryAsset,
    narrow: bool,
) -> (gtk::Overlay, gtk::Label) {
    let picture = gtk::Picture::builder().vexpand(!narrow).build();
    if narrow {
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_height_request(NARROW_THUMB);
        super::thumbnail_loader::load_into_picture(
            ui.ctx.clone(),
            asset.id.clone(),
            picture.clone(),
            true,
        );
    } else {
        picture.set_content_fit(gtk::ContentFit::Contain);
        load_preview(ui, &asset.id, &picture);
    }
    let frame = gtk::Overlay::builder()
        .child(&picture)
        .vexpand(!narrow)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["mimick-duplicate-thumb"])
        .build();
    let verdict = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .margin_top(8)
        .margin_start(8)
        .css_classes(["osd", "mimick-duplicate-verdict"])
        .build();
    frame.add_overlay(&verdict);
    (frame, verdict)
}

/// The cached thumbnail right away, then the sharper preview once it arrives.
fn load_preview(ui: &LibraryWindowUi, asset_id: &str, picture: &gtk::Picture) {
    let cache = ui.ctx.thumbnail_cache.clone();
    if let Some(texture) = cache.get_cached(asset_id, crate::api_client::ThumbnailSize::Thumbnail) {
        picture.set_paintable(Some(&texture));
    }
    let (id, picture) = (asset_id.to_string(), picture.clone());
    glib::MainContext::default().spawn_local(async move {
        if let Ok(texture) = cache.load_preview(&id, false).await {
            picture.set_paintable(Some(&texture));
        }
    });
}

fn show_verdict(button: &gtk::ToggleButton, verdict: &gtk::Label) {
    if button.is_active() {
        verdict.set_label(&gettext("Keep"));
        verdict.remove_css_class("mimick-duplicate-trash");
    } else {
        verdict.set_label(&gettext("Trash"));
        verdict.add_css_class("mimick-duplicate-trash");
    }
}

/// File name; resolution and size, marking the group's largest file; then the capture day.
fn card_facts(asset: &LibraryAsset, largest: u64) -> Vec<(String, &'static str)> {
    let mut facts = vec![(asset.filename.clone(), "heading")];
    let exif = asset.exif_info.as_ref();
    let size = file_size(asset);
    let dims = exif
        .and_then(|e| e.exif_image_width.zip(e.exif_image_height))
        .or(asset.width.zip(asset.height));
    let mut detail = Vec::new();
    if let Some((w, h)) = dims {
        let megapixels = f64::from(w) * f64::from(h) / 1_000_000.0;
        detail.push(format!("{w} × {h} ({megapixels:.1} MP)"));
    }
    if size > 0 {
        let size_text = glib::format_size(size).to_string();
        detail.push(if size == largest {
            fill(gettext("{size}, largest"), &[("size", &size_text)])
        } else {
            size_text
        });
    }
    if !detail.is_empty() {
        facts.push((detail.join(" · "), "caption"));
    }
    let taken = asset
        .local_date_time
        .as_deref()
        .unwrap_or(&asset.created_at)
        .get(..10)
        .and_then(|day| chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok());
    if let Some(day) = taken {
        facts.push((format_day(day, &gettext("%-d %b %Y")), "dim-label"));
    }
    facts
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

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: &str, size: u64) -> LibraryAsset {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "originalFileName": format!("{id}.jpg"),
            "originalMimeType": "image/jpeg",
            "fileCreatedAt": "2026-03-15T12:00:00.000Z",
            "type": "IMAGE",
            "exifInfo": { "fileSizeInByte": size }
        }))
        .unwrap()
    }

    #[test]
    fn suggestion_wins_else_the_largest_file_is_kept() {
        let mut group = DuplicateGroup {
            duplicate_id: "d".into(),
            assets: vec![asset("small", 10), asset("big", 99)],
            suggested_keep_asset_ids: vec!["small".into()],
        };
        assert_eq!(initial_keep(&group), HashSet::from(["small".to_string()]));
        group.suggested_keep_asset_ids = vec!["gone".into()];
        assert_eq!(initial_keep(&group), HashSet::from(["big".to_string()]));
    }

    #[test]
    fn card_facts_show_size_and_day() {
        let facts = card_facts(&asset("a", 2_000_000), 9_000_000);
        assert_eq!(facts[0].0, "a.jpg");
        assert!(facts[1].0.contains("MB"));
        assert!(!facts[1].0.contains("largest"));
        assert_eq!(facts.len(), 3);
        let biggest = card_facts(&asset("b", 9_000_000), 9_000_000);
        assert!(biggest[1].0.ends_with(", largest"));
    }
}
