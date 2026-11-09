//! The photos in an individual shared link: take some out, or add photos to a link.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::SharedLink;

use super::LibraryWindowUi;
use super::album_picker::asset_cover;
use super::albums_view::spawn_thumbnail;
use super::context_menu::show_error_toast;
use super::dialogs::toast_with_action;
use super::header_format::count_label;
use super::shared_link_dialog::{copy_to_clipboard, format_expiry};

const TILE_SIZE: i32 = 96;

/// The dialog's grid, what's selected in it, and the bar that removes the selection.
struct PhotosDialog {
    ui: Rc<LibraryWindowUi>,
    link_id: String,
    on_changed: Rc<dyn Fn()>,
    dialog: libadwaita::Dialog,
    title: libadwaita::WindowTitle,
    grid: gtk::FlowBox,
    remove: gtk::Button,
    hint: gtk::Label,
    toasts: libadwaita::ToastOverlay,
    selected: RefCell<Vec<String>>,
}

/// Show `link`'s photos; `on_changed` runs after some are taken out.
pub(super) fn present(ui: &Rc<LibraryWindowUi>, link: &SharedLink, on_changed: Rc<dyn Fn()>) {
    let view = Rc::new(PhotosDialog::new(ui, link.id.clone(), on_changed));
    let weak = Rc::downgrade(&view);
    view.remove.connect_clicked(move |_| {
        if let Some(view) = weak.upgrade() {
            view.remove_selected();
        }
    });
    let keep_alive = RefCell::new(Some(view.clone()));
    view.dialog.connect_closed(move |_| {
        keep_alive.borrow_mut().take();
    });
    view.dialog.present(Some(&ui.window));
    view.load();
}

impl PhotosDialog {
    fn new(ui: &Rc<LibraryWindowUi>, link_id: String, on_changed: Rc<dyn Fn()>) -> Self {
        let title = libadwaita::WindowTitle::new(&gettext("Photos in Link"), "");
        let grid = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(3)
            .max_children_per_line(8)
            .column_spacing(6)
            .row_spacing(6)
            .valign(gtk::Align::Start)
            .margin_top(6)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        let remove = gtk::Button::builder()
            .css_classes(["destructive-action", "pill"])
            .halign(gtk::Align::Center)
            .visible(false)
            .build();
        let hint = gtk::Label::builder()
            .label(gettext(
                "Tap photos to take them out. To add some, select them in your library and choose Add to Share Link.",
            ))
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["dim-label", "caption"])
            .build();
        let toasts = libadwaita::ToastOverlay::new();
        let dialog = dialog_shell(&title, &grid, (&remove, &hint), &toasts);
        Self {
            ui: ui.clone(),
            link_id,
            on_changed,
            dialog,
            title,
            grid,
            remove,
            hint,
            toasts,
            selected: RefCell::default(),
        }
    }

    fn load(self: &Rc<Self>) {
        let view = self.clone();
        glib::MainContext::default().spawn_local(async move {
            match view
                .ui
                .ctx
                .api_client
                .fetch_shared_link(&view.link_id)
                .await
            {
                Ok(link) => view.show(&link),
                Err(err) => view.toasts.add_toast(libadwaita::Toast::new(&fill(
                    gettext("Couldn't load the photos: {err}"),
                    &[("err", &err)],
                ))),
            }
        });
    }

    fn show(self: &Rc<Self>, link: &SharedLink) {
        self.grid.remove_all();
        self.selected.borrow_mut().clear();
        self.sync_selection();
        self.title
            .set_subtitle(&count_label(link.assets.len() as u64));
        for asset in &link.assets {
            self.grid.append(&self.tile(&asset.id));
        }
    }

    /// A photo that toggles in and out of the selection; a tick marks it.
    fn tile(self: &Rc<Self>, asset_id: &str) -> gtk::ToggleButton {
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .build();
        spawn_thumbnail(self.ui.ctx.clone(), asset_id.to_string(), picture.clone());
        let tick = gtk::Image::builder()
            .icon_name("object-select-symbolic")
            .halign(gtk::Align::End)
            .valign(gtk::Align::End)
            .margin_end(4)
            .margin_bottom(4)
            .css_classes(["mimick-link-photo-tick"])
            .visible(false)
            .build();
        let frame = gtk::Overlay::builder()
            .child(&picture)
            .width_request(TILE_SIZE)
            .height_request(TILE_SIZE)
            .overflow(gtk::Overflow::Hidden)
            .build();
        frame.add_overlay(&tick);
        let tile = gtk::ToggleButton::builder()
            .child(&frame)
            .tooltip_text(gettext("Select"))
            .css_classes(["flat", "mimick-link-photo"])
            .build();
        let (weak, id) = (Rc::downgrade(self), asset_id.to_string());
        tile.connect_toggled(move |tile| {
            tick.set_visible(tile.is_active());
            if let Some(view) = weak.upgrade() {
                view.toggle(&id, tile.is_active());
            }
        });
        tile
    }

    fn toggle(&self, id: &str, selected: bool) {
        let mut list = self.selected.borrow_mut();
        list.retain(|picked| picked != id);
        if selected {
            list.push(id.to_string());
        }
        drop(list);
        self.sync_selection();
    }

    /// The hint shows until something is selected, then the Remove button does.
    fn sync_selection(&self) {
        let count = self.selected.borrow().len();
        self.remove.set_visible(count > 0);
        self.hint.set_visible(count == 0);
        self.remove.set_label(&fill(
            ngettext("Remove {n} Photo", "Remove {n} Photos", plural_n(count)),
            &[("n", &count.to_string())],
        ));
    }

    fn remove_selected(self: &Rc<Self>) {
        let ids = self.selected.borrow().clone();
        self.remove.set_sensitive(false);
        let view = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let api = &view.ui.ctx.api_client;
            let result = api.remove_shared_link_assets(&view.link_id, &ids).await;
            view.remove.set_sensitive(true);
            match result {
                Ok(_) => {
                    (view.on_changed)();
                    view.load();
                }
                Err(err) => view.toasts.add_toast(libadwaita::Toast::new(&fill(
                    gettext("Couldn't take them out: {err}"),
                    &[("err", &err)],
                ))),
            }
        });
    }
}

/// The grid scrolls between the header and a bottom bar with the hint or Remove.
fn dialog_shell(
    title: &libadwaita::WindowTitle,
    grid: &gtk::FlowBox,
    (remove, hint): (&gtk::Button, &gtk::Label),
    toasts: &libadwaita::ToastOverlay,
) -> libadwaita::Dialog {
    let header = libadwaita::HeaderBar::builder().title_widget(title).build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(grid)
        .build();
    let bottom = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(6)
        .margin_bottom(12)
        .margin_start(18)
        .margin_end(18)
        .build();
    bottom.append(hint);
    bottom.append(remove);
    let toolbar = libadwaita::ToolbarView::builder()
        .content(&scroller)
        .build();
    toolbar.add_top_bar(&header);
    toolbar.add_bottom_bar(&bottom);
    toasts.set_child(Some(&toolbar));
    libadwaita::Dialog::builder()
        .title(gettext("Photos in Link"))
        .content_width(520)
        .content_height(560)
        .child(toasts)
        .build()
}

/// Pick one of your individual links and put `asset_ids` in it.
pub(super) fn add_to_link(ui: &Rc<LibraryWindowUi>, asset_ids: Vec<String>) {
    if asset_ids.is_empty() {
        return;
    }
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let links = match ui.ctx.api_client.fetch_shared_links().await {
            Ok(links) => links,
            Err(err) => return show_error_toast(&ui, &gettext("Couldn't load your links"), &err),
        };
        let individual: Vec<SharedLink> = links
            .into_iter()
            .filter(|link| link.album.is_none())
            .collect();
        link_picker(&ui, individual, asset_ids);
    });
}

fn link_picker(ui: &Rc<LibraryWindowUi>, links: Vec<SharedLink>, asset_ids: Vec<String>) {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .valign(gtk::Align::Start)
        .build();
    let dialog = picker_shell(&list, asset_ids.len(), links.is_empty());
    for link in &links {
        list.append(&link_row(ui, link));
    }
    let (ui_add, dialog_ref) = (ui.clone(), dialog.clone());
    list.connect_row_activated(move |_, row| {
        let link = usize::try_from(row.index())
            .ok()
            .and_then(|index| links.get(index));
        if let Some(link) = link {
            dialog_ref.close();
            add_and_report(&ui_add, link.clone(), asset_ids.clone());
        }
    });
    dialog.present(Some(&ui.window));
}

fn link_row(ui: &Rc<LibraryWindowUi>, link: &SharedLink) -> libadwaita::ActionRow {
    let title = link
        .description
        .clone()
        .filter(|description| !description.trim().is_empty())
        .unwrap_or_else(|| gettext("Individual share"));
    let row = libadwaita::ActionRow::builder()
        .title(title)
        .subtitle(fill(
            gettext("Made {date}"),
            &[("date", &format_expiry(&link.created_at))],
        ))
        .use_markup(false)
        .activatable(true)
        .build();
    if let Some(asset) = link.assets.first() {
        row.add_prefix(&asset_cover(&ui.ctx, asset.id.clone()));
    }
    row
}

fn picker_shell(list: &gtk::ListBox, count: usize, empty: bool) -> libadwaita::Dialog {
    let title =
        libadwaita::WindowTitle::new(&gettext("Add to Share Link"), &count_label(count as u64));
    let header = libadwaita::HeaderBar::builder()
        .title_widget(&title)
        .build();
    let content: gtk::Widget = if empty {
        libadwaita::StatusPage::builder()
            .icon_name("mimick-share-symbolic")
            .title(gettext("No Links to Add To"))
            .description(gettext(
                "Albums' links always show the whole album. Make a link for some photos with Create Share Link first.",
            ))
            .build()
            .upcast()
    } else {
        libadwaita::Clamp::builder()
            .maximum_size(560)
            .margin_top(6)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .child(list)
            .build()
            .upcast()
    };
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    let toolbar = libadwaita::ToolbarView::builder()
        .content(&scroller)
        .build();
    toolbar.add_top_bar(&header);
    libadwaita::Dialog::builder()
        .title(gettext("Add to Share Link"))
        .content_width(400)
        .content_height(480)
        .child(&toolbar)
        .build()
}

/// Add, then say how many went in, with the link one tap away.
fn add_and_report(ui: &Rc<LibraryWindowUi>, link: SharedLink, asset_ids: Vec<String>) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let api = &ui.ctx.api_client;
        match api.add_shared_link_assets(&link.id, &asset_ids).await {
            Ok(added) => {
                let message = added_message(added, asset_ids.len());
                let ui_copy = ui.clone();
                let toast = toast_with_action(&message, &gettext("Copy Link"), move || {
                    let (ui, link) = (ui_copy.clone(), link.clone());
                    glib::MainContext::default().spawn_local(async move {
                        if let Err(err) = copy_to_clipboard(&ui, &link).await {
                            show_error_toast(&ui, &gettext("Couldn't copy the link"), &err);
                        }
                    });
                });
                ui.toasts.add_toast(toast);
            }
            Err(err) => show_error_toast(&ui, &gettext("Couldn't add to the link"), &err),
        }
    });
}

fn added_message(added: usize, asked: usize) -> String {
    if added == 0 {
        return gettext("Already in the link");
    }
    let mut message = fill(
        gettext("Added {items} to the link"),
        &[("items", &count_label(added as u64))],
    );
    if added < asked {
        message.push_str(" · ");
        message.push_str(&fill(
            ngettext(
                "{n} wasn't added",
                "{n} weren't added",
                plural_n(asked - added),
            ),
            &[("n", &(asked - added).to_string())],
        ));
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_message_counts_what_went_in() {
        assert_eq!(added_message(0, 2), "Already in the link");
        assert_eq!(added_message(2, 2), "Added 2 items to the link");
        assert_eq!(
            added_message(1, 3),
            "Added 1 item to the link · 2 weren't added"
        );
    }
}
