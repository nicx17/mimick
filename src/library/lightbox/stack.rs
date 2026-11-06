//! A stack's photos as a strip along the bottom of the lightbox, with the stack's actions.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use super::Lightbox;
use crate::api_client::Stack;
use crate::library::actions::is_mine;
use crate::library::asset_object::AssetObject;
use crate::library::context_menu::{Menu, show_error_toast};

const THUMB_SIZE: i32 = 44;

pub(super) struct StackStrip {
    pub(super) root: gtk::Revealer,
    thumbs: gtk::Box,
    actions: gtk::Button,
    loaded: RefCell<Option<Stack>>,
}

impl StackStrip {
    pub(super) fn new() -> Self {
        let thumbs = gtk::Box::builder().spacing(4).build();
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::External)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(true)
            .child(&thumbs)
            .build();
        let actions = gtk::Button::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text(gettext("Stack Actions"))
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        let panel = gtk::Box::builder()
            .spacing(6)
            .css_classes(["osd", "mimick-stack-strip"])
            .build();
        panel.append(&gtk::Image::from_icon_name("mimick-stack-symbolic"));
        panel.append(&scroller);
        panel.append(&actions);
        let root = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .child(&panel)
            .build();
        Self {
            root,
            thumbs,
            actions,
            loaded: RefCell::default(),
        }
    }

    fn hide(&self) {
        self.root.set_reveal_child(false);
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
                lb.show_stack_menu(button);
            }
        });
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
        *self.stack.loaded.borrow_mut() = Some(stack);
        self.mark_shown_member();
        self.stack.root.set_reveal_child(true);
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
        button
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

    fn show_stack_menu(self: &Rc<Self>, button: &gtk::Button) {
        let Some(stack) = self.stack.loaded.borrow().clone() else {
            return;
        };
        let shown = self
            .current_item()
            .map(|item| item.id())
            .unwrap_or_default();
        let menu = Menu::new(
            button,
            f64::from(button.width()) / 2.0,
            f64::from(button.height()) / 2.0,
        );
        if shown != stack.primary_asset_id {
            let (lb, stack_id, id) = (self.clone(), stack.id.clone(), shown.clone());
            menu.add(&gettext("Set as Stack Cover"), false, move || {
                lb.change_stack(StackChange::Cover(stack_id.clone(), id.clone()))
            });
            let (lb, stack_id, id) = (self.clone(), stack.id.clone(), shown);
            menu.add(&gettext("Remove from Stack"), false, move || {
                lb.change_stack(StackChange::Remove(stack_id.clone(), id.clone()))
            });
        }
        let (lb, stack_id) = (self.clone(), stack.id);
        menu.add(&gettext("Unstack"), true, move || {
            lb.change_stack(StackChange::Unstack(stack_id.clone()))
        });
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
    menu.add(&gettext("Unstack"), false, move || {
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
    });
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
