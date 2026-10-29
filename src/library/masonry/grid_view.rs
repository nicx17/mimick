//! Photos grid construction. Builds a `MasonryCanvas` inside a
//! `ScrolledWindow`, returns it alongside the shared model + selection.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;

use super::canvas::{GridQuality, MasonryCanvas};
use crate::app_context::{AppContext, GridStyle};
use crate::library::asset_model::LibraryAssetModel;

pub type AssetContextMenuHandler = Rc<RefCell<Option<Box<dyn Fn(u32, f64, f64)>>>>;

pub struct GridViewParts {
    pub model: LibraryAssetModel,
    pub scrolled: gtk::ScrolledWindow,
    pub content: gtk::Box,
    pub canvas: MasonryCanvas,
    pub selection: gtk::MultiSelection,
    pub context_menu_handler: AssetContextMenuHandler,
}

fn scrolled_content(canvas: &MasonryCanvas) -> (gtk::ScrolledWindow, gtk::Box) {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    canvas.set_vexpand(true);
    content.append(canvas);
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .build();
    (scrolled, content)
}

fn apply_grid_style(canvas: &MasonryCanvas, style: &GridStyle) {
    canvas.set_quality(GridQuality::parse(&style.quality));
    if let Ok(color) = style.border_color.parse::<gdk4::RGBA>() {
        canvas.set_border_color(color);
    }
    canvas.set_border_gap(style.border_width);
}

/// Apply grid settings changed in Settings while this window is open.
fn follow_grid_style(ctx: &AppContext, canvas: &MasonryCanvas) {
    let mut changes = ctx.grid_style.subscribe();
    let canvas = canvas.downgrade();
    glib::MainContext::default().spawn_local(async move {
        while changes.changed().await.is_ok() {
            let style = changes.borrow_and_update().clone();
            let Some(canvas) = canvas.upgrade() else {
                break;
            };
            apply_grid_style(&canvas, &style);
        }
    });
}

pub fn build_grid_view(
    ctx: Arc<AppContext>,
    select_toggle: gtk::ToggleButton,
    narrow: Rc<Cell<bool>>,
) -> GridViewParts {
    let model = LibraryAssetModel::new();
    let selection = gtk::MultiSelection::new(Some(model.clone()));
    let context_menu_handler: AssetContextMenuHandler = Rc::new(RefCell::new(None));

    let canvas = MasonryCanvas::new(
        ctx.thumbnail_cache.clone(),
        model.clone(),
        selection.clone(),
    );
    canvas.set_narrow(narrow.get());
    canvas.set_select_mode(select_toggle.is_active());
    canvas.set_context_menu_handler(context_menu_handler.clone());
    apply_grid_style(&canvas, &ctx.grid_style.borrow());
    follow_grid_style(&ctx, &canvas);
    canvas.install_drag_source(ctx.clone());

    {
        let canvas = canvas.clone();
        select_toggle.connect_toggled(move |toggle| {
            canvas.set_select_mode(toggle.is_active());
        });
    }
    {
        let canvas = canvas.clone();
        let toggle = select_toggle.clone();
        canvas.set_select_mode_changer(move |on| {
            if toggle.is_active() != on {
                toggle.set_active(on);
            }
        });
    }

    let (scrolled, content) = scrolled_content(&canvas);

    GridViewParts {
        model,
        scrolled,
        content,
        canvas,
        selection,
        context_menu_handler,
    }
}
