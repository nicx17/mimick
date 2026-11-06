//! Edit mode in the lightbox: crop, rotate, and flip right on the photo.

use gettextrs::gettext;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

use super::Lightbox;
use super::details::edited_original_cache_path;
use crate::api_client::ImmichApiClient;
use crate::library::actions::TrashTarget;
use crate::library::asset_object::AssetObject;
use crate::library::context_menu::show_error_toast;
use crate::library::photo_editor::{ASPECTS, EditSession, aspect_label, shaped_crop};

/// A tool button: icon, caption, and what it does to the session.
type Tool = (&'static str, String, EditAction);
/// A change a tool button makes to the photo being edited.
type EditAction = fn(&mut EditSession);
/// A tool's button and its change.
type ToolButton = (gtk::Button, EditAction);

/// How often to ask whether Immich has rendered a saved edit, and how many times.
const RENDER_POLL: std::time::Duration = std::time::Duration::from_millis(1500);
const RENDER_POLLS: u32 = 40;

/// Cancel and Save in the header, the tools in a panel under the photo, and the photo being edited.
pub(super) struct EditMode {
    /// Stands in for the lightbox header while editing.
    pub(super) header: libadwaita::HeaderBar,
    /// Stands in for the action bar under the photo while editing.
    pub(super) panel: gtk::Box,
    cancel: gtk::Button,
    save: gtk::Button,
    revert: gtk::Button,
    shapes: Vec<gtk::ToggleButton>,
    tool_buttons: Vec<ToolButton>,
    reset: gtk::Button,
    session: RefCell<Option<EditSession>>,
    item: RefCell<Option<AssetObject>>,
    active: Cell<bool>,
    render_gen: Cell<u64>,
}

impl EditMode {
    pub(super) fn new() -> Self {
        let cancel = gtk::Button::with_label(&gettext("Cancel"));
        let save = gtk::Button::builder()
            .label(gettext("Save"))
            .sensitive(false)
            .css_classes(["suggested-action"])
            .build();
        let (shape_row, shapes) = shape_chips();
        let tool_buttons = tool_buttons();
        let reset = gtk::Button::builder()
            .label(gettext("Reset"))
            .css_classes(["flat"])
            .build();
        let revert = gtk::Button::builder()
            .label(gettext("Revert to Original…"))
            .css_classes(["flat", "mimick-edit-revert"])
            .build();
        Self {
            header: edit_header(&cancel, &save),
            panel: edit_panel(&shape_row, &tool_buttons, &reset, &revert),
            cancel,
            save,
            revert,
            shapes,
            tool_buttons,
            reset,
            session: RefCell::default(),
            item: RefCell::default(),
            active: Cell::new(false),
            render_gen: Cell::new(0),
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.active.get()
    }
}

/// Without the window buttons, so Cancel, the title, and Save fit a phone.
fn edit_header(cancel: &gtk::Button, save: &gtk::Button) -> libadwaita::HeaderBar {
    let header = libadwaita::HeaderBar::builder()
        .show_back_button(false)
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .title_widget(&libadwaita::WindowTitle::new(&gettext("Edit Photo"), ""))
        .visible(false)
        .build();
    header.pack_start(cancel);
    header.pack_end(save);
    header
}

/// Crop shapes, the turn and flip tools, then Reset and Revert.
fn edit_panel(
    shapes: &gtk::ScrolledWindow,
    tools: &[ToolButton],
    reset: &gtk::Button,
    revert: &gtk::Button,
) -> gtk::Box {
    // One row of equal buttons; on narrow windows their names wrap instead of the row.
    let tool_row = gtk::Box::builder()
        .spacing(6)
        .homogeneous(true)
        .halign(gtk::Align::Center)
        .build();
    for (button, _) in tools {
        tool_row.append(button);
    }
    let footer = gtk::Box::builder().spacing(6).build();
    footer.append(reset);
    footer.append(&gtk::Box::builder().hexpand(true).build());
    footer.append(revert);
    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .visible(false)
        .css_classes(["mimick-edit-panel"])
        .build();
    panel.append(shapes);
    panel.append(&tool_row);
    panel.append(&footer);
    panel
}

fn tool_buttons() -> Vec<ToolButton> {
    let tools: [Tool; 4] = [
        ("mimick-rotate-left-symbolic", gettext("Rotate Left"), |s| {
            s.rotate(false)
        }),
        (
            "mimick-rotate-right-symbolic",
            gettext("Rotate Right"),
            |s| s.rotate(true),
        ),
        ("mimick-flip-horizontal-symbolic", gettext("Mirror"), |s| {
            s.flip(true)
        }),
        ("mimick-flip-vertical-symbolic", gettext("Flip"), |s| {
            s.flip(false)
        }),
    ];
    tools
        .into_iter()
        .map(|(icon, label, action)| (tool_button(icon, &label), action))
        .collect()
}

/// An icon over its name, so each tool says what it does.
fn tool_button(icon: &str, label: &str) -> gtk::Button {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(
        &gtk::Label::builder()
            .label(label)
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["caption"])
            .build(),
    );
    gtk::Button::builder()
        .child(&content)
        .css_classes(["flat", "mimick-edit-tool"])
        .build()
}

/// The crop shapes as chips, always in view; the row scrolls sideways on narrow windows.
fn shape_chips() -> (gtk::ScrolledWindow, Vec<gtk::ToggleButton>) {
    let row = gtk::Box::builder()
        .spacing(6)
        .halign(gtk::Align::Center)
        .build();
    let mut shapes: Vec<gtk::ToggleButton> = Vec::new();
    for (index, (_, ratio)) in ASPECTS.iter().enumerate() {
        let glyph = shape_glyph(*ratio);
        let content = gtk::Box::builder().spacing(6).build();
        content.append(&glyph);
        content.append(&gtk::Label::new(Some(&aspect_label(index))));
        let chip = gtk::ToggleButton::builder()
            .child(&content)
            .active(index == 0)
            .css_classes(["mimick-edit-chip"])
            .build();
        // The picture is drawn in the chip's text color, which changes when it's picked.
        chip.connect_toggled(move |_| glyph.queue_draw());
        if let Some(first) = shapes.first() {
            chip.set_group(Some(first));
        }
        row.append(&chip);
        shapes.push(chip);
    }
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .child(&row)
        .build();
    (scroller, shapes)
}

/// Room for a crop shape's picture in its chip, in pixels.
const GLYPH_ROOM: (i32, i32) = (20, 14);
/// The free shape is drawn as a plain, dashed rectangle of this width over height.
const FREE_GLYPH_RATIO: f64 = 1.4;

/// A small picture of the shape: the photo icon for Original, else an outline of the shape.
fn shape_glyph(ratio: Option<f64>) -> gtk::Widget {
    if ratio == Some(0.0) {
        return gtk::Image::from_icon_name("image-x-generic-symbolic").upcast();
    }
    let area = gtk::DrawingArea::builder()
        .content_width(GLYPH_ROOM.0)
        .content_height(GLYPH_ROOM.1)
        .valign(gtk::Align::Center)
        .build();
    area.set_draw_func(move |area, cr, width, height| {
        draw_shape(
            cr,
            &area.color(),
            ratio,
            (f64::from(width), f64::from(height)),
        );
    });
    area.upcast()
}

/// Outlines the shape in the chip's text color, dashed when the shape is free.
fn draw_shape(
    cr: &gtk::cairo::Context,
    color: &gtk::gdk::RGBA,
    ratio: Option<f64>,
    room: (f64, f64),
) {
    const LINE: f64 = 1.5;
    let (w, h) = glyph_box(
        ratio.unwrap_or(FREE_GLYPH_RATIO),
        (room.0 - LINE, room.1 - LINE),
    );
    cr.set_source_rgba(
        f64::from(color.red()),
        f64::from(color.green()),
        f64::from(color.blue()),
        f64::from(color.alpha()),
    );
    cr.set_line_width(LINE);
    if ratio.is_none() {
        cr.set_dash(&[3.0, 2.0], 0.0);
    }
    cr.rectangle((room.0 - w) / 2.0, (room.1 - h) / 2.0, w, h);
    let _ = cr.stroke();
}

/// The largest box of `ratio` (width over height) that fits in `room`.
fn glyph_box(ratio: f64, (w, h): (f64, f64)) -> (f64, f64) {
    if w / h > ratio {
        (h * ratio, h)
    } else {
        (w, w / ratio)
    }
}

impl Lightbox {
    pub(super) fn connect_edit_mode(self: &Rc<Self>) {
        let mode = &self.edit_mode;
        self.on_click(&mode.cancel, Self::exit_edit_mode);
        self.on_click(&mode.save, Self::save_edit);
        self.on_click(&mode.revert, Self::confirm_revert_edit);
        self.on_click(&mode.reset, |lb| lb.change_edit(EditSession::reset));
        for (button, action) in &mode.tool_buttons {
            let (lb, action) = (Rc::downgrade(self), *action);
            button.connect_clicked(move |_| {
                if let Some(lb) = lb.upgrade() {
                    lb.change_edit(action);
                }
            });
        }
        for check in &mode.shapes {
            let lb = Rc::downgrade(self);
            check.connect_toggled(move |check| {
                if let Some(lb) = lb.upgrade().filter(|_| check.is_active()) {
                    lb.apply_shape();
                    lb.crop_from_layer();
                }
            });
        }
        self.connect_crop_input();
    }

    /// Drags on the photo move and resize the crop box only while editing.
    fn connect_crop_input(self: &Rc<Self>) {
        self.picture.crop.set_crop_look(true);
        let lb = Rc::downgrade(self);
        self.picture.crop.attach_input(
            &self.picture.scrolled,
            gtk::PropagationPhase::Capture,
            move |x, y| {
                let lb = lb.upgrade().filter(|lb| lb.edit_mode.is_active())?;
                let point = lb.picture.scrolled.compute_point(
                    &lb.picture.crop.area,
                    &gtk::graphene::Point::new(x as f32, y as f32),
                )?;
                Some((f64::from(point.x()), f64::from(point.y())))
            },
        );
        let lb = Rc::downgrade(self);
        self.picture.crop.connect_selection_changed(move |_| {
            if let Some(lb) = lb.upgrade() {
                lb.crop_from_layer();
            }
        });
    }

    /// Starts editing the photo on screen, in place.
    pub(in crate::library) fn enter_edit_mode(self: &Rc<Self>) {
        let Some(item) = self.current_item() else {
            return;
        };
        let Some(id) = TrashTarget::from_item(&item).map(|t| t.remote_id().to_string()) else {
            return;
        };
        self.leave_modes_for_edit();
        let mode = &self.edit_mode;
        mode.active.set(true);
        mode.item.replace(Some(item.clone()));
        mode.revert.set_visible(item.is_edited());
        mode.save.set_sensitive(false);
        mode.shapes[0].set_active(true);
        // Any load still running for this photo would replace the picture being edited.
        self.load_gen.set(self.load_gen.get().wrapping_add(1));
        self.set_edit_chrome(true);
        let (lb, edited) = (self.clone(), item.is_edited());
        glib::MainContext::default().spawn_local(async move {
            let session = EditSession::load(&lb.ui.ctx, id, edited).await;
            if !lb.edit_mode.is_active() {
                return;
            }
            match session {
                Ok(session) => {
                    lb.edit_mode.session.replace(Some(session));
                    lb.show_edit();
                }
                Err(err) => {
                    lb.exit_edit_mode();
                    show_error_toast(&lb.ui, &gettext("Couldn't load the photo"), &err);
                }
            }
        });
    }

    fn leave_modes_for_edit(self: &Rc<Self>) {
        // Fullscreen hides the header, where Cancel and Save are.
        if self.ui.window.is_fullscreen() {
            self.ui.window.unfullscreen();
        }
        self.stop_slideshow();
        self.stop_live();
        self.exit_face_mode();
        self.actions.ocr_toggle.set_active(false);
        if (self.zoom_level.get() - 1.0).abs() > 0.001 {
            self.zoom_reset();
        }
    }

    /// The edit header and panel replace the lightbox header, action bar, and the photo's own
    /// overlays; hiding the header also keeps Prev/Next from dropping the unsaved edit.
    fn set_edit_chrome(&self, editing: bool) {
        let mode = &self.edit_mode;
        mode.header.set_visible(editing);
        mode.panel.set_visible(editing);
        self.header.bar.set_visible(!editing);
        self.actions
            .bar
            .set_visible(!editing && !self.ui.window.is_fullscreen());
        self.picture.crop.area.set_visible(editing);
        self.sync_stack_visible();
        self.picture
            .strip
            .set_swipe_enabled(!editing && (self.zoom_level.get() - 1.0).abs() < 0.001);
        self.show_overlays(!editing);
    }

    /// Draws the turned preview in place of the photo, with the crop box over it.
    fn show_edit(self: &Rc<Self>) {
        let generation = self.edit_mode.render_gen.get().wrapping_add(1);
        self.edit_mode.render_gen.set(generation);
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let drawing = lb
                .edit_mode
                .session
                .borrow()
                .as_ref()
                .map(EditSession::oriented_texture);
            let Some(drawing) = drawing else {
                return;
            };
            let texture = drawing.await;
            if lb.edit_mode.render_gen.get() != generation || !lb.edit_mode.is_active() {
                return;
            }
            let Some(texture) = texture else {
                return;
            };
            lb.active_picture().set_paintable(Some(&texture));
            let size = (f64::from(texture.width()), f64::from(texture.height()));
            lb.picture.crop.set_image_size(size);
            let crop = lb
                .edit_mode
                .session
                .borrow()
                .as_ref()
                .map(EditSession::screen_crop);
            lb.picture.crop.set_selection(crop);
            lb.apply_shape();
            lb.sync_edit_save();
        });
    }

    fn change_edit(self: &Rc<Self>, change: EditAction) {
        if let Some(session) = self.edit_mode.session.borrow_mut().as_mut() {
            change(session);
        }
        self.show_edit();
    }

    /// Locks the crop box to the chosen shape, turned to match the box's orientation.
    fn apply_shape(&self) {
        let mode = &self.edit_mode;
        let preset = mode
            .shapes
            .iter()
            .position(gtk::ToggleButton::is_active)
            .and_then(|index| ASPECTS.get(index))
            .and_then(|(_, ratio)| *ratio);
        let (Some(preset), Some(selection)) = (preset, self.picture.crop.selection()) else {
            self.picture.crop.set_aspect(None);
            return;
        };
        let (ratio, shaped) = shaped_crop(preset, selection, self.picture.crop.image_size());
        self.picture.crop.set_aspect(Some(ratio));
        self.picture.crop.set_selection(Some(shaped));
    }

    fn crop_from_layer(&self) {
        let Some(selection) = self.picture.crop.selection() else {
            return;
        };
        if let Some(session) = self.edit_mode.session.borrow_mut().as_mut() {
            session.set_screen_crop(selection);
        }
        self.sync_edit_save();
    }

    fn sync_edit_save(&self) {
        let changed = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .is_some_and(EditSession::has_changes);
        self.edit_mode.save.set_sensitive(changed);
    }

    /// Leaves edit mode without saving and puts the photo back.
    pub(super) fn exit_edit_mode(self: &Rc<Self>) {
        let mode = &self.edit_mode;
        if !mode.active.replace(false) {
            return;
        }
        mode.session.take();
        mode.item.take();
        self.picture.crop.set_selection(None);
        self.set_edit_chrome(false);
        self.render();
    }

    /// While editing, Escape cancels and Enter saves; other keys (arrows, Delete) do nothing,
    /// so a stray key can't move on and drop the edit.
    pub(super) fn handle_edit_mode_key(self: &Rc<Self>, key: gtk::gdk::Key) -> bool {
        use gtk::gdk::Key;
        if !self.edit_mode.is_active() {
            return false;
        }
        match key {
            Key::Escape => self.exit_edit_mode(),
            Key::Return | Key::KP_Enter if self.edit_mode.save.is_sensitive() => self.save_edit(),
            _ => {}
        }
        true
    }

    fn save_edit(self: &Rc<Self>) {
        let Some((id, edits, thumbhash)) = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .map(|s| (s.asset_id.clone(), s.edits(), s.thumbhash.clone()))
        else {
            return;
        };
        self.edit_mode.save.set_sensitive(false);
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let api = &lb.ui.ctx.api_client;
            let result = if edits.is_empty() {
                api.revert_edits(&id).await
            } else {
                api.save_edits(&id, &edits).await
            };
            lb.finish_edit(result, id, !edits.is_empty(), thumbhash);
        });
    }

    fn confirm_revert_edit(self: &Rc<Self>) {
        let Some((id, thumbhash)) = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .filter(|s| s.was_edited())
            .map(|s| (s.asset_id.clone(), s.thumbhash.clone()))
        else {
            return;
        };
        let lb = self.clone();
        crate::library::dialogs::confirm(
            &self.ui,
            &gettext("Revert to the Original?"),
            &gettext("Every crop, turn, and flip made in Immich is removed."),
            &gettext("Revert"),
            move || {
                let (lb, id, thumbhash) = (lb.clone(), id.clone(), thumbhash.clone());
                glib::MainContext::default().spawn_local(async move {
                    let result = lb.ui.ctx.api_client.revert_edits(&id).await;
                    lb.finish_edit(result, id, false, thumbhash);
                });
            },
        );
    }

    /// Immich renders the edit in the background; the photo refreshes once that's done.
    fn finish_edit(
        self: &Rc<Self>,
        result: Result<(), String>,
        id: String,
        edited: bool,
        thumbhash: Option<String>,
    ) {
        if let Err(err) = result {
            self.sync_edit_save();
            show_error_toast(&self.ui, &gettext("Immich didn't save the edit"), &err);
            return;
        }
        if let Some(item) = self.edit_mode.item.borrow().as_ref() {
            item.set_is_edited(edited);
        }
        self.exit_edit_mode();
        self.ui.toasts.add_toast(libadwaita::Toast::new(&gettext(
            "Saved. Immich is updating the photo.",
        )));
        let (lb, api) = (Rc::downgrade(self), self.ui.ctx.api_client.clone());
        glib::MainContext::default().spawn_local(async move {
            wait_for_render(&api, &id, thumbhash.as_deref()).await;
            if let Some(lb) = lb.upgrade() {
                lb.show_rendered_edit(&id);
            }
        });
    }

    /// Drops every copy of the old render, then redraws the grid and, if still shown, the photo.
    fn show_rendered_edit(self: &Rc<Self>, id: &str) {
        self.ui.ctx.thumbnail_cache.forget(id);
        if let Some(dir) = crate::profile::cache_dir() {
            let _ = std::fs::remove_file(edited_original_cache_path(&dir.join("preview"), id));
        }
        self.ui.grid.canvas.queue_draw();
        let still_here = self.current_item().is_some_and(|item| item.id() == id);
        if still_here && !self.edit_mode.is_active() {
            self.render();
        }
    }
}

/// Immich's render job changes the thumbhash as its last step; gives up after a minute.
async fn wait_for_render(api: &ImmichApiClient, id: &str, before: Option<&str>) {
    for _ in 0..RENDER_POLLS {
        glib::timeout_future(RENDER_POLL).await;
        match api.fetch_asset_by_id(id).await {
            Ok(asset) if asset.thumbhash.as_deref() == before => {}
            Ok(_) => return,
            Err(err) => {
                log::debug!("Stopped waiting for the edit of {id}: {err}");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_pictures_fill_their_room_in_their_own_shape() {
        assert_eq!(glyph_box(1.0, (20.0, 14.0)), (14.0, 14.0));
        assert_eq!(glyph_box(2.0, (20.0, 14.0)), (20.0, 10.0));
        let (w, h) = glyph_box(4.0 / 3.0, (20.0, 14.0));
        assert!((w - 56.0 / 3.0).abs() < 1e-9 && (h - 14.0).abs() < 1e-9);
    }
}
