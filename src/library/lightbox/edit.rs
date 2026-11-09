//! Edit mode in the lightbox, right on the photo: crop, turn, and level it, adjust its light and
//! colour or pick a filter, and hide parts of it. Immich keeps crops, turns, and flips on the
//! photo itself; anything else is saved as a new photo stacked with the original.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gettextrs::gettext;
use gtk::prelude::*;

mod looks;
mod pages;
mod panel;
mod redact;
mod saving;
mod wheel;

use super::Lightbox;
use crate::i18n::gettext_noop;
use crate::library::actions::TrashTarget;
use crate::library::asset_object::AssetObject;
use crate::library::context_menu::show_error_toast;
use crate::library::photo_editor::look::Slider;
use crate::library::photo_editor::{ASPECTS, EditSession, shaped_crop};

/// A change a turn or flip makes to the photo being edited.
type EditAction = fn(&mut EditSession);

/// The editor's tools, one page each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tool {
    Crop,
    Adjust,
    Filters,
    Redact,
}

/// Each tool's page name, title, and icon, in switcher order.
const TOOLS: [(Tool, &str, &str, &str); 4] = [
    (
        Tool::Crop,
        "crop",
        gettext_noop("Crop"),
        "mimick-crop-symbolic",
    ),
    (
        Tool::Adjust,
        "adjust",
        gettext_noop("Adjust"),
        "mimick-adjust-symbolic",
    ),
    (
        Tool::Filters,
        "filters",
        gettext_noop("Filters"),
        "mimick-filters-symbolic",
    ),
    (
        Tool::Redact,
        "redact",
        gettext_noop("Redact"),
        "view-conceal-symbolic",
    ),
];

/// The editor's header and panel, and the photo being edited.
pub(super) struct EditMode {
    /// Stands in for the lightbox header while editing.
    pub(super) header: libadwaita::HeaderBar,
    /// Stands in for the action bar under the photo while editing.
    pub(super) panel: gtk::Box,
    widgets: panel::EditWidgets,
    session: RefCell<Option<EditSession>>,
    item: RefCell<Option<AssetObject>>,
    active: Cell<bool>,
    /// Bumped per session, so a render started for an earlier one is dropped.
    render_gen: Cell<u64>,
    /// A render is running; `stale` asks for one more once it's done.
    rendering: Cell<bool>,
    stale: Cell<bool>,
    /// Controls are being set to match the session, so their changes aren't edits.
    syncing: Cell<bool>,
    /// A copy is being made and uploaded.
    saving: Cell<bool>,
    /// The hidden area picked on the Redact page, to move, resize, or restyle.
    redact_picked: Cell<Option<usize>>,
    /// The slider the Adjust page's dial moves.
    slider: Cell<Slider>,
}

impl EditMode {
    pub(super) fn new() -> Self {
        let widgets = panel::EditWidgets::new();
        Self {
            header: widgets.header.clone(),
            panel: widgets.panel.clone(),
            widgets,
            session: RefCell::default(),
            item: RefCell::default(),
            active: Cell::new(false),
            render_gen: Cell::new(0),
            rendering: Cell::new(false),
            stale: Cell::new(false),
            syncing: Cell::new(false),
            saving: Cell::new(false),
            redact_picked: Cell::new(None),
            slider: Cell::new(Slider::Exposure),
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.active.get()
    }
}

impl Lightbox {
    pub(super) fn connect_edit_mode(self: &Rc<Self>) {
        let w = &self.edit_mode.widgets;
        self.on_click(&w.cancel, Self::exit_edit_mode);
        self.on_click(&w.save, Self::save_edit);
        self.on_click(&w.revert, Self::confirm_revert_edit);
        self.on_click(&w.reset, |lb| lb.change_edit(EditSession::reset));
        self.connect_copy_choices();
        self.connect_crop_page();
        self.connect_look_pages();
        self.connect_redact();
        self.connect_tool_switch();
        self.connect_crop_input();
    }

    /// Remembers whether copies keep the original's details, for the next edit too.
    /// The ⋯ menu's choices for copies, remembered for the next edit too.
    fn connect_copy_choices(self: &Rc<Self>) {
        let (w, config) = (&self.edit_mode.widgets, self.ui.ctx.config.read());
        w.keep_details
            .set_active(config.data.edit_copy_keeps_details);
        w.stack_copy.set_active(config.data.edit_copy_stacks);
        drop(config);
        self.remember_choice(&w.keep_details, |data, on| {
            data.edit_copy_keeps_details = on
        });
        self.remember_choice(&w.stack_copy, |data, on| data.edit_copy_stacks = on);
    }

    fn remember_choice(
        self: &Rc<Self>,
        check: &gtk::CheckButton,
        store: fn(&mut crate::config::ConfigData, bool),
    ) {
        let lb = Rc::downgrade(self);
        check.connect_toggled(move |check| {
            let Some(lb) = lb.upgrade() else {
                return;
            };
            let mut config = lb.ui.ctx.config.write();
            store(&mut config.data, check.is_active());
            if !config.save() {
                log::error!("Failed to save config after changing how copies are saved");
            }
            drop(config);
            lb.sync_edit_save();
        });
    }

    fn connect_crop_page(self: &Rc<Self>) {
        let page = &self.edit_mode.widgets.crop;
        for (button, action) in &page.turns {
            let (lb, action) = (Rc::downgrade(self), *action);
            button.connect_clicked(move |_| {
                if let Some(lb) = lb.upgrade() {
                    lb.change_edit(action);
                }
            });
        }
        for check in &page.shapes {
            let lb = Rc::downgrade(self);
            check.connect_toggled(move |check| {
                if let Some(lb) = lb.upgrade().filter(|_| check.is_active()) {
                    lb.apply_shape();
                    lb.crop_from_layer();
                }
            });
        }
    }

    /// Each tool shows its own boxes; phones get the switcher's compact, stacked labels.
    fn connect_tool_switch(self: &Rc<Self>) {
        let lb = Rc::downgrade(self);
        self.edit_mode
            .widgets
            .tools
            .connect_visible_child_name_notify(move |_| {
                if let Some(lb) = lb.upgrade().filter(|lb| lb.edit_mode.is_active()) {
                    lb.show_tool_layers();
                    lb.show_edit();
                }
            });
        self.ui
            .split
            .bind_property("collapsed", &self.edit_mode.widgets.switcher, "policy")
            .transform_to(|_, collapsed: bool| {
                Some(if collapsed {
                    libadwaita::ViewSwitcherPolicy::Narrow
                } else {
                    libadwaita::ViewSwitcherPolicy::Wide
                })
            })
            .sync_create()
            .build();
    }

    /// Drags on the photo move and resize the crop box only on the Crop page.
    fn connect_crop_input(self: &Rc<Self>) {
        self.picture.crop.set_crop_look(true);
        let lb = Rc::downgrade(self);
        self.picture.crop.attach_input(
            &self.picture.scrolled,
            gtk::PropagationPhase::Capture,
            move |x, y| {
                let lb = lb.upgrade().filter(|lb| lb.editing_with(Tool::Crop))?;
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

    fn editing_with(&self, tool: Tool) -> bool {
        self.edit_mode.is_active() && self.edit_mode.widgets.tool() == tool
    }

    /// Starts editing the photo on screen, in place, on the Crop page.
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
        mode.widgets.revert.set_visible(item.is_edited());
        mode.widgets.save.set_sensitive(false);
        mode.widgets.crop.shapes[0].set_active(true);
        mode.widgets.show_tool(Tool::Crop);
        mode.redact_picked.set(None);
        mode.render_gen.set(mode.render_gen.get().wrapping_add(1));
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
                    lb.sync_edit_controls();
                    lb.show_tool_layers();
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
        // On a phone the details pane covers the photo being edited.
        if self.ui.split.is_collapsed() {
            self.header.details_btn.set_active(false);
        }
        self.stop_slideshow();
        self.stop_live();
        self.leave_panorama();
        self.exit_face_mode();
        self.actions.ocr_toggle.set_active(false);
        if (self.zoom_level.get() - 1.0).abs() > 0.001 {
            self.zoom_reset();
        }
    }

    /// The edit header and panel replace the lightbox header, action bar, and the photo's own
    /// overlays; hiding the header also keeps Prev/Next from dropping the unsaved edit.
    fn set_edit_chrome(self: &Rc<Self>, editing: bool) {
        let mode = &self.edit_mode;
        mode.header.set_visible(editing);
        mode.panel.set_visible(editing);
        self.header.bar.set_visible(!editing);
        self.actions
            .bar
            .set_visible(!editing && !self.ui.window.is_fullscreen());
        if editing {
            self.show_tool_layers();
        } else {
            self.picture.crop.area.set_visible(false);
            self.picture.redact.area.set_visible(false);
        }
        self.sync_stack_visible();
        self.picture
            .strip
            .set_swipe_enabled(!editing && (self.zoom_level.get() - 1.0).abs() < 0.001);
        self.show_overlays(!editing);
    }

    /// The crop box on the Crop page and the hidden areas on the Redact page.
    fn show_tool_layers(self: &Rc<Self>) {
        let tool = self.edit_mode.widgets.tool();
        self.picture.crop.area.set_visible(tool == Tool::Crop);
        self.picture.redact.area.set_visible(tool == Tool::Redact);
        match tool {
            Tool::Filters => self.refresh_filter_previews(),
            Tool::Redact => self.sync_redact_page(),
            Tool::Crop | Tool::Adjust => {}
        }
    }

    /// Redraws the photo as edited: the whole frame on the Crop page, else the result.
    /// Changes made while a render runs fold into one more render after it.
    fn show_edit(self: &Rc<Self>) {
        let mode = &self.edit_mode;
        if mode.rendering.replace(true) {
            mode.stale.set(true);
            return;
        }
        mode.stale.set(false);
        let cropped = mode.widgets.tool() != Tool::Crop;
        let drawing = mode
            .session
            .borrow()
            .as_ref()
            .map(|session| session.developed_texture(cropped));
        let Some(drawing) = drawing else {
            mode.rendering.set(false);
            return;
        };
        let (lb, generation) = (self.clone(), mode.render_gen.get());
        glib::MainContext::default().spawn_local(async move {
            let texture = drawing.await;
            let mode = &lb.edit_mode;
            mode.rendering.set(false);
            let current = mode.is_active() && mode.render_gen.get() == generation;
            if let Some(texture) = texture.filter(|_| current) {
                lb.show_developed(&texture, cropped);
            }
            if mode.stale.get() && mode.is_active() {
                lb.show_edit();
            }
        });
    }

    /// Puts a render on screen, with the crop box or the hidden areas over it.
    fn show_developed(self: &Rc<Self>, texture: &gtk::gdk::Texture, cropped: bool) {
        self.active_picture().set_paintable(Some(texture));
        let size = (f64::from(texture.width()), f64::from(texture.height()));
        if cropped {
            self.picture.redact.set_image_size(size);
            self.show_redactions();
        } else {
            self.picture.crop.set_image_size(size);
            let crop = self
                .edit_mode
                .session
                .borrow()
                .as_ref()
                .map(EditSession::screen_crop);
            self.picture.crop.set_selection(crop);
            self.apply_shape();
        }
        self.sync_edit_save();
    }

    fn change_edit(self: &Rc<Self>, change: EditAction) {
        if let Some(session) = self.edit_mode.session.borrow_mut().as_mut() {
            change(session);
        }
        self.sync_edit_controls();
        self.show_edit();
    }

    /// Locks the crop box to the chosen shape, turned to match the box's orientation.
    fn apply_shape(&self) {
        let shapes = &self.edit_mode.widgets.crop.shapes;
        let preset = shapes
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

    /// Save is on when there's something to save, and says when it makes a copy.
    fn sync_edit_save(&self) {
        let (changed, copy) = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .map_or((false, false), |s| (s.has_changes(), s.needs_copy()));
        let (w, saving) = (&self.edit_mode.widgets, self.edit_mode.saving.get());
        w.save.set_sensitive(changed && !saving);
        // Save keeps one short label so the header fits a phone; the subtitle says the rest.
        let subtitle = match (saving, copy, w.stack_copy.is_active()) {
            (true, _, _) => gettext("Saving a copy…"),
            (false, false, _) => String::new(),
            (false, true, true) => gettext("Saves a copy stacked with the original"),
            (false, true, false) => gettext("Saves as a new photo"),
        };
        w.title.set_subtitle(&subtitle);
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
        self.picture.redact.set_selection(None);
        self.picture.redact.set_markers(Vec::new());
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
            Key::Delete | Key::BackSpace if self.editing_with(Tool::Redact) => {
                self.remove_picked_redaction()
            }
            Key::Return | Key::KP_Enter if self.edit_mode.widgets.save.is_sensitive() => {
                self.save_edit()
            }
            _ => {}
        }
        true
    }
}
