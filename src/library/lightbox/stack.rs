//! A stack's photos as a strip under the lightbox photo, with the stack's actions.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use super::Lightbox;
use crate::api_client::Stack;
use crate::library::actions::is_mine;
use crate::library::asset_object::AssetObject;
use crate::library::context_menu::{Menu, connect_context_menu, menu_key, show_error_toast};

const THUMB_SIZE: i32 = 44;
const THUMB_SPACING: i32 = 4;

pub(super) struct StackStrip {
    /// A row of its own under the photo, so it never covers it.
    pub(super) root: gtk::Box,
    scroller: gtk::ScrolledWindow,
    thumbs: gtk::Box,
    actions: gtk::Button,
    loaded: RefCell<Option<Stack>>,
}

impl StackStrip {
    pub(super) fn new() -> Self {
        let thumbs = gtk::Box::builder().spacing(THUMB_SPACING).build();
        // Capped to the thumbnails' size, since each picture asks for its whole image's size;
        // the width cap is set per stack in `fill_stack`.
        // `External`, not `Never`: with `Never`, GTK ignores the height cap.
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::External)
            .propagate_natural_width(true)
            .propagate_natural_height(true)
            .min_content_height(THUMB_SIZE)
            .max_content_height(THUMB_SIZE)
            .child(&thumbs)
            .build();
        let actions = gtk::Button::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text(gettext("Stack Actions"))
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        let root = gtk::Box::builder()
            .spacing(6)
            .halign(gtk::Align::Center)
            .visible(false)
            .css_classes(["mimick-stack-strip"])
            .build();
        root.append(&gtk::Image::from_icon_name("mimick-stack-symbolic"));
        root.append(&scroller);
        root.append(&actions);
        Self {
            root,
            scroller,
            thumbs,
            actions,
            loaded: RefCell::default(),
        }
    }

    fn hide(&self) {
        self.root.set_visible(false);
        self.loaded.take();
        while let Some(child) = self.thumbs.first_child() {
            self.thumbs.remove(&child);
        }
    }
}

impl Lightbox {
    pub(super) fn connect_stack(self: &Rc<Self>) {
        let lb = Rc::downgrade(self);
        self.stack.actions.connect_clicked(move |button| {
            if let Some(lb) = lb.upgrade() {
                let shown = lb.current_item().map(|item| item.id()).unwrap_or_default();
                let (x, y) = centre(button);
                lb.show_stack_menu(button, &shown, x, y);
            }
        });
    }

    /// Under a stacked photo, except while editing or in fullscreen.
    pub(super) fn sync_stack_visible(&self) {
        let shown = self.stack.loaded.borrow().is_some()
            && !self.edit_mode.is_active()
            && !self.ui.window.is_fullscreen();
        self.stack.root.set_visible(shown);
    }

    /// Drops the loaded stack, so the next render fetches it again.
    pub(super) fn forget_stack(&self) {
        self.stack.hide();
    }

    /// Shows the strip for a stacked grid item, loading the stack once per stack.
    pub(super) fn sync_stack(self: &Rc<Self>) {
        let stack_id = self
            .grid_item()
            .map(|item| item.stack_id())
            .unwrap_or_default();
        if stack_id.is_empty() {
            self.stack.hide();
            return;
        }
        let loaded = self
            .stack
            .loaded
            .borrow()
            .as_ref()
            .is_some_and(|stack| stack.id == stack_id);
        if loaded {
            self.mark_shown_member();
        } else {
            self.stack.hide();
            self.load_stack(stack_id);
        }
    }

    fn load_stack(self: &Rc<Self>, stack_id: String) {
        let generation = self.load_gen.get();
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = lb.ui.ctx.api_client.fetch_stack(&stack_id).await;
            if lb.load_gen.get() != generation {
                return;
            }
            match result {
                Ok(stack) if stack.assets.len() > 1 => lb.fill_stack(stack),
                Ok(_) => {}
                Err(err) => log::debug!("Could not load stack {stack_id}: {err}"),
            }
        });
    }

    fn fill_stack(self: &Rc<Self>, mut stack: Stack) {
        // The cover first, then the rest in Immich's order.
        let primary = stack.primary_asset_id.clone();
        stack.assets.sort_by_key(|asset| asset.id != primary);
        for asset in &stack.assets {
            self.stack.thumbs.append(&self.stack_thumb(&asset.id));
        }
        let count = i32::try_from(stack.assets.len()).unwrap_or(0);
        let width = count.saturating_mul(THUMB_SIZE + THUMB_SPACING) - THUMB_SPACING;
        self.stack.scroller.set_max_content_width(width);
        *self.stack.loaded.borrow_mut() = Some(stack);
        self.mark_shown_member();
        self.sync_stack_visible();
    }

    fn stack_thumb(self: &Rc<Self>, asset_id: &str) -> gtk::Button {
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .width_request(THUMB_SIZE)
            .height_request(THUMB_SIZE)
            .build();
        crate::library::thumbnail_loader::load_into_picture(
            self.ui.ctx.clone(),
            asset_id.to_string(),
            picture.clone(),
            false,
        );
        let button = gtk::Button::builder()
            .child(&picture)
            .overflow(gtk::Overflow::Hidden)
            .css_classes(["flat", "mimick-stack-thumb"])
            .build();
        button.set_widget_name(asset_id);
        let (lb, id) = (Rc::downgrade(self), asset_id.to_string());
        button.connect_clicked(move |_| {
            if let Some(lb) = lb.upgrade() {
                lb.show_stack_member(&id);
            }
        });
        self.connect_thumb_menu(&button, asset_id);
        button
    }

    /// Right-click, long-press, or the Menu key on a thumbnail acts on that photo directly.
    fn connect_thumb_menu(self: &Rc<Self>, button: &gtk::Button, asset_id: &str) {
        let (lb, id) = (Rc::downgrade(self), asset_id.to_string());
        let open = Rc::new(move |thumb: &gtk::Button, x: f64, y: f64| {
            if let Some(lb) = lb.upgrade() {
                lb.show_stack_menu(thumb, &id, x, y);
            }
        });
        let (on_press, thumb) = (open.clone(), button.downgrade());
        connect_context_menu(button, move |x, y| {
            if let Some(thumb) = thumb.upgrade() {
                on_press(&thumb, x, y);
            }
        });
        let thumb = button.downgrade();
        button.add_controller(menu_key(move || {
            if let Some(thumb) = thumb.upgrade() {
                let (x, y) = centre(&thumb);
                open(&thumb, x, y);
            }
        }));
    }

    /// Shows a member in place of the cover; picking the cover goes back to the grid item.
    fn show_stack_member(self: &Rc<Self>, asset_id: &str) {
        let shown = self.current_item().map(|item| item.id());
        if shown.as_deref() == Some(asset_id) {
            return;
        }
        let asset = self.stack.loaded.borrow().as_ref().and_then(|stack| {
            stack
                .assets
                .iter()
                .find(|asset| asset.id == asset_id)
                .cloned()
        });
        let Some(asset) = asset else {
            return;
        };
        let cover = self.grid_item().is_some_and(|item| item.id() == asset_id);
        let member = (!cover).then(|| {
            let object = crate::library::asset_model::object_for(&self.ui.ctx, &asset);
            if let Some(stack) = self.stack.loaded.borrow().as_ref() {
                object.set_stack_id(stack.id.as_str());
            }
            object
        });
        self.stop_live();
        *self.member.borrow_mut() = member;
        self.render();
    }

    fn mark_shown_member(&self) {
        let shown = self
            .current_item()
            .map(|item| item.id())
            .unwrap_or_default();
        let mut child = self.stack.thumbs.first_child();
        while let Some(thumb) = child {
            child = thumb.next_sibling();
            if thumb.widget_name() == shown {
                thumb.add_css_class("mimick-stack-current");
            } else {
                thumb.remove_css_class("mimick-stack-current");
            }
        }
        let owned = self
            .current_item()
            .is_some_and(|item| is_mine(&self.ui, &item));
        self.stack.actions.set_visible(owned);
    }

    /// Your stack's actions for `asset_id`: make it the cover or take it out, or unstack them all.
    fn show_stack_menu(self: &Rc<Self>, parent: &gtk::Button, asset_id: &str, x: f64, y: f64) {
        let Some(stack) = self.stack.loaded.borrow().clone() else {
            return;
        };
        if !self
            .current_item()
            .is_some_and(|item| is_mine(&self.ui, &item))
        {
            return;
        }
        let menu = Menu::new(parent, x, y);
        if asset_id != stack.primary_asset_id {
            let (lb, stack_id, id) = (self.clone(), stack.id.clone(), asset_id.to_string());
            menu.add_with_icon(
                "starred-symbolic",
                &gettext("Set as Stack Cover"),
                false,
                move || lb.change_stack(StackChange::Cover(stack_id.clone(), id.clone())),
            );
            let (lb, stack_id, id) = (self.clone(), stack.id.clone(), asset_id.to_string());
            menu.add_with_icon(
                "list-remove-symbolic",
                &gettext("Remove from Stack"),
                false,
                move || lb.change_stack(StackChange::Remove(stack_id.clone(), id.clone())),
            );
        }
        let (lb, stack_id) = (self.clone(), stack.id);
        menu.add_with_icon(
            "mimick-stack-symbolic",
            &gettext("Unstack"),
            true,
            move || lb.change_stack(StackChange::Unstack(stack_id.clone())),
        );
        menu.popup();
    }

    fn change_stack(self: &Rc<Self>, change: StackChange) {
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let api = &lb.ui.ctx.api_client;
            let result = match &change {
                StackChange::Cover(stack, asset) => api.set_stack_cover(stack, asset).await,
                StackChange::Remove(stack, asset) => api.remove_from_stack(stack, asset).await,
                StackChange::Unstack(stack) => api.delete_stack(stack).await,
            };
            match result {
                Ok(()) => lb.after_stack_change(&change),
                Err(err) => show_error_toast(&lb.ui, &gettext("Couldn't change the stack"), &err),
            }
        });
    }

    /// The grid regroups when the lightbox closes, so the photo on screen doesn't move now.
    fn after_stack_change(self: &Rc<Self>, change: &StackChange) {
        self.reload_on_close.set(true);
        let message = match change {
            StackChange::Cover(..) => {
                if let Some(stack) = self.stack.loaded.borrow_mut().as_mut()
                    && let StackChange::Cover(_, asset) = change
                {
                    stack.primary_asset_id = asset.clone();
                }
                gettext("Stack cover changed")
            }
            StackChange::Remove(stack_id, _) => {
                self.stack.hide();
                self.load_stack(stack_id.clone());
                gettext("Removed from the stack")
            }
            StackChange::Unstack(_) => {
                self.stack.hide();
                gettext("Unstacked")
            }
        };
        self.mark_shown_member();
        self.ui.toasts.add_toast(libadwaita::Toast::new(&message));
    }
}

/// The middle of `widget`, where a menu opened from the keyboard points.
fn centre(widget: &impl IsA<gtk::Widget>) -> (f64, f64) {
    (
        f64::from(widget.width()) / 2.0,
        f64::from(widget.height()) / 2.0,
    )
}

enum StackChange {
    Cover(String, String),
    Remove(String, String),
    Unstack(String),
}

/// "Unstack" for a stacked tile in the grid; the grid then shows every photo again.
pub(in crate::library) fn append_unstack_button(
    menu: &Menu,
    ui: &Rc<crate::library::LibraryWindowUi>,
    item: &AssetObject,
) {
    let stack_id = item.stack_id();
    if stack_id.is_empty() || item.stack_count() < 2 || !is_mine(ui, item) {
        return;
    }
    let ui = ui.clone();
    menu.add_with_icon(
        "mimick-stack-symbolic",
        &gettext("Unstack"),
        false,
        move || {
            let (ui, stack_id) = (ui.clone(), stack_id.clone());
            glib::MainContext::default().spawn_local(async move {
                match ui.ctx.api_client.delete_stack(&stack_id).await {
                    Ok(()) => {
                        ui.toasts
                            .add_toast(libadwaita::Toast::new(&gettext("Unstacked")));
                        crate::library::refresh_library_after_mutation(ui.clone(), true);
                    }
                    Err(err) => show_error_toast(&ui, &gettext("Couldn't change the stack"), &err),
                }
            });
        },
    );
}

/// Stacks the selected photos under the first one, then regroups the grid.
pub(in crate::library) fn stack_selected(
    ui: &Rc<crate::library::LibraryWindowUi>,
    ids: Vec<String>,
) {
    if ids.len() < 2 {
        return;
    }
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.create_stack(&ids).await {
            Ok(_) => {
                ui.grid.selection.unselect_all();
                ui.select_toggle.set_active(false);
                let message = fill(
                    ngettext(
                        "Stacked {n} photo",
                        "Stacked {n} photos",
                        plural_n(ids.len()),
                    ),
                    &[("n", &ids.len().to_string())],
                );
                ui.toasts.add_toast(libadwaita::Toast::new(&message));
                crate::library::refresh_library_after_mutation(ui.clone(), true);
            }
            Err(err) => show_error_toast(&ui, &gettext("Couldn't stack the photos"), &err),
        }
    });
}
