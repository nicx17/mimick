//! The widgets over and around the lightbox photo, and how the viewer is put together.

use gtk::prelude::*;

use super::super::{
    ActionBar, DetailsPane, LibraryWindowUi, LightboxHeader, PictureArea, UnavailableCard,
    build_action_bar, build_details_pane, build_split_view, build_unavailable_card,
    build_video_badge, build_viewer,
};

/// Everything over and around the photo, added in stacking order.
pub(super) struct Layers {
    pub(super) unavailable: UnavailableCard,
    pub(super) video_badge_button: gtk::Button,
    pub(super) live_layer: gtk::Box,
    pub(super) live_button: gtk::ToggleButton,
    pub(super) panorama: std::rc::Rc<super::super::panorama::Panorama>,
    pub(super) stack: super::super::stack::StackStrip,
    pub(super) face_mode: super::super::faces::FaceMode,
    pub(super) edit_mode: super::super::edit::EditMode,
    pub(super) slideshow: super::super::slideshow::Slideshow,
}

pub(super) fn build_layers(picture: &PictureArea) -> Layers {
    let unavailable = build_unavailable_card();
    picture.overlay.add_overlay(&unavailable.overlay);
    let video_badge_button = build_video_badge();
    picture.overlay.add_overlay(&video_badge_button);
    let live_layer = super::super::live::build_live_layer();
    picture.overlay.add_overlay(&live_layer);
    let panorama = super::super::panorama::Panorama::new();
    picture.overlay.add_overlay(&panorama.layer);
    let live_button = super::super::live::build_live_button();
    picture.overlay.add_overlay(&pills(&[
        live_button.upcast_ref(),
        panorama.button.upcast_ref(),
    ]));
    let face_mode = super::super::faces::FaceMode::new();
    picture.overlay.add_overlay(&face_mode.bar);
    let slideshow = super::super::slideshow::Slideshow::new();
    for overlay in slideshow.overlays() {
        picture.overlay.add_overlay(&overlay);
    }
    Layers {
        unavailable,
        video_badge_button,
        live_layer,
        live_button,
        panorama,
        stack: super::super::stack::StackStrip::new(),
        face_mode,
        edit_mode: super::super::edit::EditMode::new(),
        slideshow,
    }
}

/// Live and 360° in the photo's top-left corner.
fn pills(buttons: &[&gtk::Widget]) -> gtk::Box {
    let row = gtk::Box::builder()
        .spacing(6)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .margin_top(12)
        .margin_start(12)
        .build();
    for button in buttons {
        row.append(*button);
    }
    row
}

/// The action bar, the details pane, and the viewer holding the photo. The stack strip and
/// the edit header and panel aren't overlays, so the photo shrinks to fit instead of being covered.
pub(super) fn lay_out(
    ui: &LibraryWindowUi,
    header: &LightboxHeader,
    picture: &PictureArea,
    layers: &Layers,
) -> (ActionBar, DetailsPane, gtk::Box) {
    header.toolbar.add_top_bar(&layers.edit_mode.header);
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
    let viewer = build_viewer(picture, &actions);
    viewer.insert_child_after(&layers.stack.root, Some(&picture.overlay));
    viewer.append(&layers.edit_mode.panel);
    let body = build_split_view(ui, &viewer, &details, &header.details_btn);
    header.toolbar.set_content(Some(&body));
    (actions, details, viewer)
}
