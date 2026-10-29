use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::Tag;
use crate::library::asset_object::AssetObject;

mod navigation;
mod rendering;
use super::fullscreen;
use super::{
    LOCAL_ID_PREFIX, LibraryWindowUi, Lightbox, apply_lightbox_zoom, build_action_bar,
    build_details_pane, build_header, build_picture_area, build_split_view, build_unavailable_card,
    build_video_badge, build_viewer,
};
use crate::library::actions::{TrashTarget, confirm_trash};
use crate::library::archive::{archive_icon, archive_tooltip, set_archived};
use crate::library::asset_edit::show_edit_dialog;
use crate::library::context_menu::{AssetMenuHooks, show_asset_context_menu};
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

        let initial_full = ui.ctx.config.read().data.library_preview_full_resolution;
        let actions = build_action_bar(initial_full);
        // The full bar doesn't fit a 360 px window; Ctrl+0 still resets the zoom.
        ui.split
            .bind_property("collapsed", &actions.zoom_reset, "visible")
            .invert_boolean()
            .sync_create()
            .build();
        let details = build_details_pane();
        let viewer = build_viewer(&picture, &actions);
        let body = build_split_view(&ui, &viewer, &details, &header.details_btn);
        header.toolbar.set_content(Some(&body));

        Rc::new(Self {
            ui,
            header,
            picture,
            unavailable,
            video_badge_button,
            actions,
            details,
            viewer,
            pos: Cell::new(position),
            load_gen: Cell::new(0),
            active_a: Cell::new(true),
            zoom_level: Cell::new(1.0),
            nav_dir: Cell::new(0),
            cursor_pos: Cell::new(None),
            drag_start: Cell::new((0.0, 0.0)),
            pinch_start: Cell::new(1.0),
            unavailable_path: RefCell::new(None),
            video: RefCell::new(None),
            fullscreen_handler: RefCell::new(None),
            drag_path: RefCell::new(None),
            reload_on_close: Cell::new(false),
            chrome: fullscreen::Chrome::new(),
        })
    }

    /// Wire the external-open button, the video play badge, and the drag-out source.
    pub(super) fn connect_overlays(self: &Rc<Self>) {
        let lb = self.clone();
        self.unavailable.open.connect_clicked(move |_| {
            if let Some(path) = lb.unavailable_path.borrow().as_deref() {
                open_local_with_default_app(path);
            }
        });

        let lb = self.clone();
        self.video_badge_button
            .connect_clicked(move |_| lb.toggle_video());

        let drag_source = gtk::DragSource::new();
        drag_source.set_actions(gtk::gdk::DragAction::COPY);
        let lb = self.clone();
        drag_source.connect_prepare(move |_source, _x, _y| {
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
        self.ui
            .split
            .connect_notify_local(Some("collapsed"), move |_, _| sync_nav_visibility());

        let lb = self.clone();
        self.header
            .prev_btn
            .connect_clicked(move |_| lb.goto_prev());
        let lb = self.clone();
        self.header
            .next_btn
            .connect_clicked(move |_| lb.goto_next());

        let lb = self.clone();
        self.header.page.connect_hidden(move |_| {
            lb.close_fullscreen();
            lb.close_video();
            if lb.reload_on_close.replace(false) {
                crate::library::refresh_library_after_mutation(lb.ui.clone(), true);
            }
        });
    }

    /// Zoom buttons, pinch, drag-to-pan, double/middle click, and Ctrl+scroll.
    pub(super) fn connect_zoom_controls(self: &Rc<Self>) {
        let lb = self.clone();
        self.actions
            .zoom_in
            .connect_clicked(move |_| lb.zoom_by(1.2));
        let lb = self.clone();
        self.actions
            .zoom_out
            .connect_clicked(move |_| lb.zoom_by(1.0 / 1.2));
        let lb = self.clone();
        self.actions
            .zoom_reset
            .connect_clicked(move |_| lb.zoom_reset());

        self.connect_pan_and_pinch();

        // Double-click on the picture: zoom in 2x toward the click position.
        let double_click = gtk::GestureClick::new();
        double_click.set_button(gtk::gdk::BUTTON_PRIMARY);
        let lb = self.clone();
        double_click.connect_pressed(move |_, n_press, x, y| {
            if n_press == 2 && lb.ocr_box_under(x, y).is_none() {
                lb.cursor_pos.set(Some((x, y)));
                lb.set_zoom(lb.zoom_level.get() * 2.0);
            }
        });
        self.picture.scrolled.add_controller(double_click);

        // Middle-click: reset zoom to 100%.
        let middle_click = gtk::GestureClick::new();
        middle_click.set_button(gtk::gdk::BUTTON_MIDDLE);
        let lb = self.clone();
        middle_click.connect_pressed(move |_, _, _, _| lb.zoom_reset());
        self.picture.scrolled.add_controller(middle_click);

        // Ctrl+wheel zoom on the picture area, captured before the scrolled window
        // can use it for panning. Listening on both axes so trackpad two-finger
        // scrolls (which sometimes emit horizontal deltas) still trigger zoom.
        let zoom_scroll =
            gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        zoom_scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        let lb = self.clone();
        zoom_scroll.connect_scroll(move |ctrl, dx, dy| lb.handle_zoom_scroll(ctrl, dx, dy));
        self.picture.scrolled.add_controller(zoom_scroll);
    }

    /// Pinch-to-zoom and click-and-drag panning, grouped so they don't fight over touches.
    fn connect_pan_and_pinch(self: &Rc<Self>) {
        // Trackpad pinch-to-zoom. On the picture scroller so it shares a stable
        // coordinate frame with drag (see drag comment below).
        let pinch = gtk::GestureZoom::new();
        let lb = self.clone();
        pinch.connect_begin(move |_, _| lb.pinch_start.set(lb.zoom_level.get()));
        let lb = self.clone();
        pinch.connect_scale_changed(move |_, scale| lb.set_zoom(lb.pinch_start.get() * scale));
        self.picture.scrolled.add_controller(pinch.clone());

        // Click-and-drag panning when zoomed in. Attached to the scroller, not the
        // picture stack: the stack moves under the cursor when we update the scroll
        // adjustments, so an offset measured relative to it oscillates frame-to-frame
        // and jitters the image.
        let drag = gtk::GestureDrag::new();
        drag.set_button(gtk::gdk::BUTTON_PRIMARY);
        let lb = self.clone();
        drag.connect_drag_begin(move |_, _, _| {
            let hadj = lb.picture.scrolled.hadjustment();
            let vadj = lb.picture.scrolled.vadjustment();
            lb.drag_start.set((hadj.value(), vadj.value()));
        });
        let lb = self.clone();
        drag.connect_drag_update(move |_, off_x, off_y| {
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
        let lb = self.clone();
        motion.connect_motion(move |_, x, y| lb.cursor_pos.set(Some((x, y))));
        let lb = self.clone();
        motion.connect_leave(move |_| lb.cursor_pos.set(None));
        self.picture.scrolled.add_controller(motion);

        // Right-click: open the standard asset context menu.
        let right_click = gtk::GestureClick::new();
        right_click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let lb = self.clone();
        right_click.connect_pressed(move |_, _, x, y| {
            show_asset_context_menu(
                lb.ui.clone(),
                &lb.picture.scrolled,
                lb.pos.get(),
                x,
                y,
                lb.menu_hooks(),
            );
        });
        self.picture.scrolled.add_controller(right_click);

        // Horizontal swipe for prev/next navigation; ignored when zoomed in.
        let swipe = gtk::GestureSwipe::new();
        swipe.set_touch_only(false);
        let lb = self.clone();
        swipe.connect_swipe(move |_, vx, _vy| lb.handle_swipe(vx));
        self.picture.stack.add_controller(swipe);
    }

    /// Ctrl +/-/0 zoom, Left/Right navigate, I toggles details, Escape closes.
    pub(super) fn connect_keys(self: &Rc<Self>) {
        let key_controller = gtk::EventControllerKey::new();
        let lb = self.clone();
        key_controller.connect_key_pressed(move |_, key, _, mods| lb.handle_key(key, mods));
        self.header.page.add_controller(key_controller);
    }

    /// Download button and the preview/original resolution toggle.
    pub(super) fn connect_actions(self: &Rc<Self>) {
        let lb = self.clone();
        self.actions.download.connect_clicked(move |_| {
            let Some(item) = lb
                .ui
                .grid
                .model
                .item(lb.pos.get())
                .and_downcast::<AssetObject>()
            else {
                return;
            };
            let asset_id = item.property::<String>("id");
            if !asset_id.starts_with(LOCAL_ID_PREFIX) {
                start_download(lb.ui.clone(), asset_id, item.property("filename"));
            }
        });

        let lb = self.clone();
        self.actions
            .open_with
            .connect_clicked(move |_| lb.open_current_with());

        let lb = self.clone();
        self.actions.resolution_toggle.connect_toggled(move |btn| {
            btn.set_label(if btn.is_active() { "Raw" } else { "Prev" });
            lb.render();
        });

        let lb = self.clone();
        self.actions
            .trash
            .connect_clicked(move |_| lb.confirm_trash_current());
        let lb = self.clone();
        self.actions
            .favorite
            .connect_clicked(move |_| lb.toggle_current_favorite());
        let lb = self.clone();
        self.actions
            .archive
            .connect_clicked(move |_| lb.toggle_current_archive());
        let lb = self.clone();
        self.details
            .edit_btn
            .connect_clicked(move |_| lb.edit_current());
        let lb = self.clone();
        self.details
            .location_btn
            .connect_clicked(move |_| lb.set_location_current());
        let lb = self.clone();
        self.details
            .album_btn
            .connect_clicked(move |_| lb.add_current_to_album());
        let lb = self.clone();
        self.details
            .tags
            .bind(self.ui.clone(), Rc::new(move |tag| lb.search_tag(tag)));
        self.details.rating.bind(self.ui.clone());
    }

    /// Close the lightbox and show every photo carrying `tag`.
    fn search_tag(&self, tag: Tag) {
        self.ui.nav.pop();
        crate::library::search_by_tag(self.ui.clone(), tag);
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

    pub(super) fn current_item(&self) -> Option<AssetObject> {
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
        AssetMenuHooks {
            on_trashed: Rc::new(move || lb_trashed.after_trash()),
            on_edited: Rc::new(move |date_changed| lb_edited.after_edit(date_changed)),
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
            .set_tooltip_text(Some(heart_tooltip(favorite)));
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
            .set_tooltip_text(Some(archive_tooltip(archived)));
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

    fn active_picture(&self) -> gtk::Picture {
        if self.active_a.get() {
            self.picture.picture_a.clone()
        } else {
            self.picture.picture_b.clone()
        }
    }

    pub(super) fn zoom_by(&self, factor: f64) {
        self.set_zoom(self.zoom_level.get() * factor);
    }

    pub(super) fn zoom_reset(&self) {
        self.set_zoom(1.0);
    }

    /// Zoom to `z` (clamped to 1–10×), keeping the focal point under the cursor.
    fn set_zoom(&self, z: f64) {
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
    }
}
