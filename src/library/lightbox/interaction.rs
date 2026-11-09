use gettextrs::gettext;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::Tag;
use crate::library::asset_object::AssetObject;

mod details_loading;
mod layout;
mod menus;
mod navigation;
mod neighbours;
mod rendering;
mod zoom;

use super::fullscreen;
use super::strip::Slot;
use super::{LOCAL_ID_PREFIX, LibraryWindowUi, Lightbox, build_header, build_picture_area};
use crate::library::actions::{TrashTarget, confirm_trash};
use crate::library::archive::{archive_icon, archive_tooltip, set_archived};
use crate::library::asset_edit::show_edit_dialog;
use crate::library::context_menu::{connect_context_menu, menu_key};
use crate::library::download::{open_local_with_default_app, start_download};
use crate::library::favorites::{FavoriteHooks, heart_icon, heart_tooltip, toggle_favorite};
use crate::library::trash_view;
use layout::{build_layers, lay_out};
pub(super) use menus::is_swipe_up;

impl Lightbox {
    /// Build the page's widget tree; signal handlers are connected by the `connect_*` methods.
    pub(super) fn new(ui: Rc<LibraryWindowUi>, position: u32, initial_filename: &str) -> Rc<Self> {
        let header = build_header(&ui, initial_filename);
        let picture = build_picture_area();
        let layers = build_layers(&picture);
        let (actions, details, viewer) = lay_out(&ui, &header, &picture, &layers);

        Rc::new(Self {
            ui,
            header,
            picture,
            unavailable: layers.unavailable,
            video_badge_button: layers.video_badge_button,
            live_button: layers.live_button,
            live_layer: layers.live_layer,
            panorama: layers.panorama,
            motion: RefCell::new(None),
            member: RefCell::new(None),
            stack: layers.stack,
            faces: super::faces::PhotoFaces::default(),
            face_mode: layers.face_mode,
            edit_mode: layers.edit_mode,
            slideshow: layers.slideshow,
            actions,
            details,
            viewer,
            pos: Cell::new(position),
            load_gen: Cell::new(0),
            zoom_level: Cell::new(1.0),
            cursor_pos: Cell::new(None),
            drag_start: Cell::new((0.0, 0.0)),
            pinch_start: Cell::new(1.0),
            unavailable_path: RefCell::new(None),
            video: RefCell::new(None),
            fullscreen_handler: RefCell::new(None),
            collapse_handler: RefCell::new(None),
            drag_path: RefCell::new(None),
            full_source: RefCell::new(None),
            reload_on_close: Cell::new(false),
            chrome: fullscreen::Chrome::new(),
        })
    }

    /// Wire the external-open button, the video play badge, and the drag-out source.
    pub(super) fn connect_overlays(self: &Rc<Self>) {
        self.on_click(&self.unavailable.open, |lb| {
            if let Some(path) = lb.unavailable_path.borrow().as_deref() {
                open_local_with_default_app(path);
            }
        });
        self.on_click(&self.video_badge_button, Self::toggle_video);

        let drag_source = gtk::DragSource::new();
        drag_source.set_actions(gtk::gdk::DragAction::COPY);
        let lb = Rc::downgrade(self);
        drag_source.connect_prepare(move |source, _x, _y| {
            // A finger drag pans, swipes, or opens the sheet; files are dragged out with a
            // mouse or pen. A drag while zoomed in pans, and while tagging faces or cropping
            // it draws on the photo instead.
            if source
                .device()
                .is_some_and(|device| device.source() == gtk::gdk::InputSource::Touchscreen)
            {
                return None;
            }
            let lb = lb.upgrade().filter(|lb| {
                (lb.zoom_level.get() - 1.0).abs() < 0.001
                    && !lb.face_mode.is_active()
                    && !lb.edit_mode.is_active()
            })?;
            let path = lb.drag_path.borrow().clone()?;
            if !path.exists() {
                return None;
            }
            let file = gtk::gio::File::for_path(&path);
            Some(gtk::gdk::ContentProvider::for_value(&file.to_value()))
        });
        self.picture.overlay.add_controller(drag_source);
    }

    /// Prev/next buttons, hidden on narrow layouts.
    pub(super) fn connect_navigation(self: &Rc<Self>) {
        // On narrow widths, hide the prev/next header buttons -- Left/Right
        // keyboard shortcuts still work, and the saved space lets the title fit.
        // The details toggle stays visible so users can still reach the EXIF pane.
        let sync_nav_visibility = {
            let prev_btn = self.header.prev_btn.clone();
            let next_btn = self.header.next_btn.clone();
            let split = self.ui.split.clone();
            move || {
                let show = !split.is_collapsed();
                prev_btn.set_visible(show);
                next_btn.set_visible(show);
            }
        };
        sync_nav_visibility();
        let handler = self
            .ui
            .split
            .connect_notify_local(Some("collapsed"), move |_, _| sync_nav_visibility());
        *self.collapse_handler.borrow_mut() = Some(handler);

        self.on_click(&self.header.prev_btn, Self::goto_prev);
        self.on_click(&self.header.next_btn, Self::goto_next);

        let lb = Rc::downgrade(self);
        self.header.page.connect_hidden(move |_| {
            if let Some(lb) = lb.upgrade() {
                lb.close();
            }
        });
    }

    pub(super) fn on_click(
        self: &Rc<Self>,
        button: &impl IsA<gtk::Button>,
        action: impl Fn(&Rc<Self>) + 'static,
    ) {
        let lb = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(lb) = lb.upgrade() {
                action(&lb);
            }
        });
    }

    /// Cursor tracking for focal zoom, the right-click menu, and swipe navigation.
    pub(super) fn connect_pointer_gestures(self: &Rc<Self>) {
        let motion = gtk::EventControllerMotion::new();
        let lb = Rc::downgrade(self);
        motion.connect_motion(move |_, x, y| {
            if let Some(lb) = lb.upgrade() {
                lb.cursor_pos.set(Some((x, y)));
            }
        });
        let lb = Rc::downgrade(self);
        motion.connect_leave(move |_| {
            if let Some(lb) = lb.upgrade() {
                lb.cursor_pos.set(None);
            }
        });
        self.picture.scrolled.add_controller(motion);

        let lb = Rc::downgrade(self);
        connect_context_menu(&self.picture.scrolled, move |x, y| {
            if let Some(lb) = lb.upgrade() {
                lb.show_menu_at(x, y);
            }
        });
        let lb = Rc::downgrade(self);
        self.header.page.add_controller(menu_key(move || {
            if let Some(lb) = lb.upgrade() {
                let scrolled = &lb.picture.scrolled;
                lb.show_menu_at(
                    f64::from(scrolled.width()) / 2.0,
                    f64::from(scrolled.height()) / 2.0,
                );
            }
        }));
        self.connect_swipe_tracker();
        self.connect_swipe_up();
        self.on_click(&self.header.menu_btn, Self::show_menu_sheet);
    }

    /// Ctrl +/-/0 zoom, Left/Right navigate, I toggles details, Escape closes.
    pub(super) fn connect_keys(self: &Rc<Self>) {
        let key_controller = gtk::EventControllerKey::new();
        let lb = Rc::downgrade(self);
        key_controller.connect_key_pressed(move |_, key, _, mods| match lb.upgrade() {
            Some(lb) => lb.handle_key(key, mods),
            None => glib::Propagation::Proceed,
        });
        self.header.page.add_controller(key_controller);
    }

    /// Download button and the preview/original resolution toggle.
    pub(super) fn connect_actions(self: &Rc<Self>) {
        self.on_click(&self.actions.download, |lb| {
            let Some(item) = lb.current_item() else {
                return;
            };
            let asset_id = item.property::<String>("id");
            if !asset_id.starts_with(LOCAL_ID_PREFIX) {
                start_download(lb.ui.clone(), asset_id, item.property("filename"));
            }
        });

        self.on_click(&self.actions.open_with, |lb| lb.open_current_with());

        let lb = Rc::downgrade(self);
        self.actions.resolution_toggle.connect_toggled(move |btn| {
            btn.set_label(&if btn.is_active() {
                gettext("Raw")
            } else {
                gettext("Prev")
            });
            if let Some(lb) = lb.upgrade() {
                lb.render();
            }
        });

        self.on_click(&self.actions.edit, Self::enter_edit_mode);
        self.on_click(&self.actions.trash, Self::confirm_trash_current);
        self.on_click(&self.actions.favorite, Self::toggle_current_favorite);
        self.on_click(&self.actions.archive, Self::toggle_current_archive);
        self.on_click(&self.details.edit_btn, Self::edit_current);
        self.on_click(&self.details.album_btn, |lb| lb.add_current_to_album());
        self.on_click(&self.details.people.edit, Self::tag_people);
        let lb = Rc::downgrade(self);
        self.details.tags.bind(
            self.ui.clone(),
            Rc::new(move |tag| {
                if let Some(lb) = lb.upgrade() {
                    lb.search_tag(tag);
                }
            }),
        );
        self.details.rating.bind(self.ui.clone());
    }

    /// Close the lightbox and show every photo carrying `tag`.
    fn search_tag(&self, tag: Tag) {
        self.ui.nav.pop();
        crate::library::search_by_tag(self.ui.clone(), tag);
    }

    /// Face mode for the photo on screen.
    fn tag_people(self: &Rc<Self>) {
        self.enter_face_mode();
    }

    fn add_current_to_album(&self) {
        let Some(target) = self
            .current_item()
            .as_ref()
            .and_then(TrashTarget::from_item)
        else {
            return;
        };
        let ids = vec![target.remote_id().to_string()];
        crate::library::album_picker::present(&self.ui, ids, Rc::new(|| {}));
    }

    /// The photo on screen: a stack member picked in the strip, else the grid item.
    pub(super) fn current_item(&self) -> Option<AssetObject> {
        self.member.borrow().clone().or_else(|| self.grid_item())
    }

    /// The grid item at the lightbox position, which is a stack's cover when stacked.
    pub(super) fn grid_item(&self) -> Option<AssetObject> {
        self.ui
            .grid
            .model
            .item(self.pos.get())
            .and_downcast::<AssetObject>()
    }

    /// Delete key / trash button: move to trash, or delete permanently in the Trash view.
    fn confirm_trash_current(self: &Rc<Self>) {
        let Some(target) = self
            .current_item()
            .as_ref()
            .and_then(TrashTarget::from_item)
        else {
            return;
        };
        let lb = self.clone();
        if trash_view::is_trash_active(&self.ui) {
            let ids = vec![target.remote_id().to_string()];
            trash_view::confirm_delete_permanently(self.ui.clone(), ids, move || lb.after_trash());
        } else {
            confirm_trash(self.ui.clone(), vec![target], 0, move || lb.after_trash());
        }
    }

    /// The trashed item is already gone from the grid model, so the same index now
    /// holds the next photo; step back at the end and close once nothing is left.
    fn after_trash(self: &Rc<Self>) {
        // A trashed stack member isn't a grid item: show the cover again and reload the strip.
        if self.member.take().is_some() {
            self.forget_stack();
        }
        let n = self.ui.grid.model.n_items();
        if n == 0 {
            self.ui.nav.pop();
            return;
        }
        if self.pos.get() >= n {
            self.pos.set(n - 1);
        }
        self.render();
    }

    /// Stands in for an empty description on the user's own photos and opens Edit Info.
    fn add_description_row(self: &Rc<Self>) -> libadwaita::ActionRow {
        let row = libadwaita::ActionRow::builder()
            .title(gettext("Add a description"))
            .activatable(true)
            .css_classes(["dim-label"])
            .build();
        row.add_suffix(&gtk::Image::from_icon_name("document-edit-symbolic"));
        let weak = Rc::downgrade(self);
        row.connect_activated(move |_| {
            if let Some(lb) = weak.upgrade() {
                lb.edit_current();
            }
        });
        row
    }

    fn edit_current(self: &Rc<Self>) {
        let Some(item) = self.current_item() else {
            return;
        };
        let Some(target) = TrashTarget::from_item(&item) else {
            return;
        };
        let lb = self.clone();
        show_edit_dialog(
            self.ui.clone(),
            target.remote_id().to_string(),
            item.property("filename"),
            Rc::new(move |date_changed| lb.after_edit(date_changed)),
        );
    }

    /// Heart button / F key. Un-favoriting inside the Favorites view drops the photo
    /// from the grid, so the lightbox then moves on as it does after a trash.
    fn toggle_current_favorite(self: &Rc<Self>) {
        let Some(item) = self.current_item() else {
            return;
        };
        let (lb_change, lb_removed) = (self.clone(), self.clone());
        let hooks = FavoriteHooks {
            on_change: Rc::new(move |favorite| lb_change.show_heart(favorite)),
            on_removed: Rc::new(move || lb_removed.after_trash()),
        };
        toggle_favorite(&self.ui, &item, hooks);
    }

    fn show_heart(&self, favorite: bool) {
        self.actions.favorite.set_icon_name(heart_icon(favorite));
        self.actions
            .favorite
            .set_tooltip_text(Some(&heart_tooltip(favorite)));
    }

    fn toggle_current_archive(self: &Rc<Self>) {
        let Some(item) = self.current_item() else {
            return;
        };
        let archived = !item.is_archived();
        let lb = self.clone();
        set_archived(&self.ui, vec![item], archived, move |removed| {
            if removed {
                lb.after_trash();
            } else {
                lb.show_archive(archived);
            }
        });
    }

    fn show_archive(&self, archived: bool) {
        self.actions.archive.set_icon_name(archive_icon(archived));
        self.actions
            .archive
            .set_tooltip_text(Some(&archive_tooltip(archived)));
    }

    fn after_edit(self: &Rc<Self>, date_changed: bool) {
        if date_changed {
            self.reload_on_close.set(true);
        }
        self.render();
    }

    pub(super) fn handle_swipe(self: &Rc<Self>, vx: f64) {
        // Ignore swipes when zoomed in — those should pan instead.
        if (self.zoom_level.get() - 1.0).abs() > 0.01 {
            return;
        }
        // vx < 0 means finger moved left → go to next asset.
        // vx > 0 means finger moved right → go to previous asset.
        const MIN_VELOCITY: f64 = 50.0;
        if vx < -MIN_VELOCITY {
            self.goto_next();
        } else if vx > MIN_VELOCITY {
            self.goto_prev();
        }
    }

    pub(in crate::library::lightbox) fn active_picture(&self) -> gtk::Picture {
        self.picture.strip.picture(Slot::Current)
    }
}
