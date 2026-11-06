use gettextrs::gettext;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::Tag;
use crate::library::asset_object::AssetObject;

mod details_loading;
mod navigation;
mod rendering;
use super::fullscreen;
use super::strip::Slot;
use super::{
    LOCAL_ID_PREFIX, LibraryWindowUi, Lightbox, apply_lightbox_zoom, build_action_bar,
    build_details_pane, build_header, build_picture_area, build_split_view, build_unavailable_card,
    build_video_badge, build_viewer,
};
use crate::library::actions::{TrashTarget, confirm_trash};
use crate::library::archive::{archive_icon, archive_tooltip, set_archived};
use crate::library::asset_edit::show_edit_dialog;
use crate::library::context_menu::{
    AssetMenuHooks, MenuTarget, connect_context_menu, menu_key, show_item_context_menu,
};
use crate::library::download::{open_local_with_default_app, start_download};
use crate::library::favorites::{FavoriteHooks, heart_icon, heart_tooltip, toggle_favorite};
use crate::library::trash_view;

impl Lightbox {
    /// Build the page's widget tree; signal handlers are connected by the `connect_*` methods.
    pub(super) fn new(ui: Rc<LibraryWindowUi>, position: u32, initial_filename: &str) -> Rc<Self> {
        let header = build_header(&ui, initial_filename);
        let picture = build_picture_area();
        let unavailable = build_unavailable_card();
        picture.overlay.add_overlay(&unavailable.overlay);
        let video_badge_button = build_video_badge();
        picture.overlay.add_overlay(&video_badge_button);
        let live_layer = super::live::build_live_layer();
        picture.overlay.add_overlay(&live_layer);
        let live_button = super::live::build_live_button();
        picture.overlay.add_overlay(&live_button);
        let stack = super::stack::StackStrip::new();
        picture.overlay.add_overlay(&stack.root);
        let face_mode = super::faces::FaceMode::new();
        picture.overlay.add_overlay(&face_mode.bar);
        let edit_mode = super::edit::EditMode::new();
        // Not overlays, so the photo shrinks to fit between them instead of being covered.
        header.toolbar.add_top_bar(&edit_mode.header);
        let slideshow = super::slideshow::Slideshow::new();
        for overlay in slideshow.overlays() {
            picture.overlay.add_overlay(&overlay);
        }

        let initial_full = ui.ctx.config.read().data.library_preview_full_resolution;
        let actions = build_action_bar(initial_full);
        // The full bar doesn't fit a 360 px window; Ctrl+0 still resets the zoom,
        // and Edit Photo stays in the ⋯ menu.
        for narrow_hidden in [
            actions.zoom_reset.upcast_ref::<gtk::Widget>(),
            actions.edit_slot.upcast_ref(),
        ] {
            ui.split
                .bind_property("collapsed", narrow_hidden, "visible")
                .invert_boolean()
                .sync_create()
                .build();
        }
        let details = build_details_pane();
        let viewer = build_viewer(&picture, &actions);
        viewer.append(&edit_mode.panel);
        let body = build_split_view(&ui, &viewer, &details, &header.details_btn);
        header.toolbar.set_content(Some(&body));

        Rc::new(Self {
            ui,
            header,
            picture,
            unavailable,
            video_badge_button,
            live_button,
            live_layer,
            motion: RefCell::new(None),
            member: RefCell::new(None),
            stack,
            face_mode,
            edit_mode,
            slideshow,
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
        drag_source.connect_prepare(move |_source, _x, _y| {
            // A drag while tagging faces or cropping draws on the photo instead.
            let lb = lb
                .upgrade()
                .filter(|lb| !lb.face_mode.is_active() && !lb.edit_mode.is_active())?;
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

    /// Zoom buttons, pinch, drag-to-pan, double/middle click, and Ctrl+scroll.
    pub(super) fn connect_zoom_controls(self: &Rc<Self>) {
        self.on_click(&self.actions.zoom_in, |lb| lb.zoom_by(1.2));
        self.on_click(&self.actions.zoom_out, |lb| lb.zoom_by(1.0 / 1.2));
        self.on_click(&self.actions.zoom_reset, Self::zoom_reset);

        self.connect_pan_and_pinch();

        // Double-click or double-tap: zoom 2x toward the point, or back out when zoomed in.
        let double_click = gtk::GestureClick::new();
        double_click.set_button(gtk::gdk::BUTTON_PRIMARY);
        let lb = Rc::downgrade(self);
        double_click.connect_pressed(move |_, n_press, x, y| {
            if let Some(lb) = lb.upgrade()
                && n_press == 2
                && lb.ocr_box_under(x, y).is_none()
            {
                if lb.zoom_level.get() > 1.0 {
                    lb.zoom_reset();
                } else {
                    lb.cursor_pos.set(Some((x, y)));
                    lb.set_zoom(2.0);
                }
            }
        });
        self.picture.scrolled.add_controller(double_click);

        // Middle-click: reset zoom to 100%.
        let middle_click = gtk::GestureClick::new();
        middle_click.set_button(gtk::gdk::BUTTON_MIDDLE);
        let lb = Rc::downgrade(self);
        middle_click.connect_pressed(move |_, _, _, _| {
            if let Some(lb) = lb.upgrade() {
                lb.zoom_reset();
            }
        });
        self.picture.scrolled.add_controller(middle_click);

        // Ctrl+wheel zoom on the picture area, captured before the scrolled window
        // can use it for panning. Listening on both axes so trackpad two-finger
        // scrolls (which sometimes emit horizontal deltas) still trigger zoom.
        let zoom_scroll =
            gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        zoom_scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        let lb = Rc::downgrade(self);
        zoom_scroll.connect_scroll(move |ctrl, dx, dy| match lb.upgrade() {
            Some(lb) => lb.handle_zoom_scroll(ctrl, dx, dy),
            None => glib::Propagation::Proceed,
        });
        self.picture.scrolled.add_controller(zoom_scroll);
    }

    /// Pinch-to-zoom and click-and-drag panning, grouped so they don't fight over touches.
    fn connect_pan_and_pinch(self: &Rc<Self>) {
        // Trackpad pinch-to-zoom. On the picture scroller so it shares a stable
        // coordinate frame with drag (see drag comment below).
        let pinch = gtk::GestureZoom::new();
        let lb = Rc::downgrade(self);
        pinch.connect_begin(move |_, _| {
            if let Some(lb) = lb.upgrade() {
                lb.pinch_start.set(lb.zoom_level.get());
            }
        });
        let lb = Rc::downgrade(self);
        pinch.connect_scale_changed(move |_, scale| {
            if let Some(lb) = lb.upgrade() {
                lb.set_zoom(lb.pinch_start.get() * scale);
            }
        });
        self.picture.scrolled.add_controller(pinch.clone());

        // Click-and-drag panning when zoomed in. Attached to the scroller, not the
        // picture stack: the stack moves under the cursor when we update the scroll
        // adjustments, so an offset measured relative to it oscillates frame-to-frame
        // and jitters the image.
        let drag = gtk::GestureDrag::new();
        drag.set_button(gtk::gdk::BUTTON_PRIMARY);
        let lb = Rc::downgrade(self);
        drag.connect_drag_begin(move |_, _, _| {
            let Some(lb) = lb.upgrade() else {
                return;
            };
            let hadj = lb.picture.scrolled.hadjustment();
            let vadj = lb.picture.scrolled.vadjustment();
            lb.drag_start.set((hadj.value(), vadj.value()));
        });
        let lb = Rc::downgrade(self);
        drag.connect_drag_update(move |_, off_x, off_y| {
            let Some(lb) = lb.upgrade() else {
                return;
            };
            let (sx0, sy0) = lb.drag_start.get();
            lb.picture.scrolled.hadjustment().set_value(sx0 - off_x);
            lb.picture.scrolled.vadjustment().set_value(sy0 - off_y);
        });
        self.picture.scrolled.add_controller(drag.clone());
        drag.group_with(&pinch);
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
        self.on_click(&self.header.menu_btn, |lb| {
            let button = &lb.header.menu_btn;
            lb.show_menu_on(
                button,
                f64::from(button.width()) / 2.0,
                f64::from(button.height()),
            );
        });
    }

    fn show_menu_at(self: &Rc<Self>, x: f64, y: f64) {
        self.show_menu_on(&self.picture.scrolled, x, y);
    }

    fn show_menu_on(self: &Rc<Self>, parent: &impl IsA<gtk::Widget>, x: f64, y: f64) {
        let Some(item) = self.current_item() else {
            return;
        };
        let target = MenuTarget {
            item,
            position: self.pos.get(),
        };
        show_item_context_menu(self.ui.clone(), parent, target, x, y, self.menu_hooks());
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

    /// Hooks for the right-click menu so trash/edit there update this lightbox.
    fn menu_hooks(self: &Rc<Self>) -> AssetMenuHooks {
        let lb_trashed = self.clone();
        let lb_edited = self.clone();
        let lb_fullscreen = Rc::downgrade(self);
        AssetMenuHooks {
            on_trashed: Rc::new(move || lb_trashed.after_trash()),
            on_edited: Rc::new(move |date_changed| lb_edited.after_edit(date_changed)),
            toggle_fullscreen: Some(Rc::new(move || {
                if let Some(lb) = lb_fullscreen.upgrade() {
                    lb.toggle_fullscreen();
                }
            })),
        }
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

    pub(super) fn zoom_by(self: &Rc<Self>, factor: f64) {
        self.set_zoom(self.zoom_level.get() * factor);
    }

    pub(super) fn zoom_reset(self: &Rc<Self>) {
        self.set_zoom(1.0);
    }

    /// Zoom to `z` (clamped to 1–10×), keeping the focal point under the cursor.
    fn set_zoom(self: &Rc<Self>, z: f64) {
        // Zooming would swap in a sharper decode of the unedited photo under the crop box.
        if self.edit_mode.is_active() {
            return;
        }
        let z_new = z.clamp(1.0, 10.0);
        let z_old = self.zoom_level.get();
        let zoom_label = format!("{}%", (z_new * 100.0).round() as i32);
        if (z_new - z_old).abs() < 0.0001 {
            self.actions.zoom_reset.set_label(&zoom_label);
            return;
        }

        // Pick the focal point: cursor if inside the viewer, else centre.
        let scrolled = &self.picture.scrolled;
        let viewer_w = scrolled.width().max(1) as f64;
        let viewer_h = scrolled.height().max(1) as f64;
        let (fx, fy) = self
            .cursor_pos
            .get()
            .filter(|&(x, y)| x >= 0.0 && y >= 0.0 && x <= viewer_w && y <= viewer_h)
            .unwrap_or((viewer_w / 2.0, viewer_h / 2.0));

        let hadj = scrolled.hadjustment();
        let vadj = scrolled.vadjustment();
        let ratio = z_new / z_old.max(0.0001);
        let target_scroll_x = (hadj.value() + fx) * ratio - fx;
        let target_scroll_y = (vadj.value() + fy) * ratio - fy;

        self.zoom_level.set(z_new);
        if (z_new - 1.0).abs() > 0.001 {
            self.stop_live();
        }
        // Swiping stays off while face mode owns drags on the photo.
        let swipe = (z_new - 1.0).abs() < 0.001 && !self.face_mode.is_active();
        self.picture.strip.set_swipe_enabled(swipe);
        let content = apply_lightbox_zoom(&self.active_picture(), scrolled, z_new);
        self.actions.zoom_reset.set_label(&zoom_label);

        // Pre-set adjustment ranges to match the new content size so the
        // scroll position can be applied in the same frame. Without this
        // the value would be clamped to the stale (old-zoom) range and
        // corrected only after layout, causing a one-frame flicker.
        let (cw, ch) = content.unwrap_or((viewer_w, viewer_h));
        hadj.set_upper(cw.max(viewer_w));
        hadj.set_page_size(viewer_w);
        vadj.set_upper(ch.max(viewer_h));
        vadj.set_page_size(viewer_h);
        hadj.set_value(target_scroll_x);
        vadj.set_value(target_scroll_y);
        if let Some((content_width, _)) = content {
            self.sharpen_for_zoom(content_width);
        }
    }
}
