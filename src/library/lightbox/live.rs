//! Live and motion photos in the lightbox: the Live button and its looping clip.

use gettextrs::gettext;
use std::rc::Rc;

use gtk::prelude::*;

use super::{AssetInfo, Lightbox};
use crate::library::context_menu::show_error_toast;
use crate::library::video_player::{MotionPlayer, VideoSource};

/// "Live" pill in the photo's top-left corner; shown only on live photos.
pub(super) fn build_live_button() -> gtk::ToggleButton {
    let content = gtk::Box::builder().spacing(6).build();
    content.append(&gtk::Image::from_icon_name("mimick-live-symbolic"));
    content.append(&gtk::Label::new(Some(&gettext("Live"))));
    gtk::ToggleButton::builder()
        .child(&content)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .margin_top(12)
        .margin_start(12)
        .tooltip_text(gettext("Play motion (Space)"))
        .css_classes(["osd", "pill", "mimick-live-button"])
        .visible(false)
        .build()
}

/// Holds the clip's surface under the Live button, whenever the player gets created.
pub(super) fn build_live_layer() -> gtk::Box {
    gtk::Box::builder()
        .hexpand(true)
        .vexpand(true)
        .can_target(false)
        .build()
}

impl Lightbox {
    pub(super) fn connect_live(self: &Rc<Self>) {
        let lb = Rc::downgrade(self);
        self.live_button.connect_toggled(move |button| {
            let Some(lb) = lb.upgrade() else {
                return;
            };
            let playing = lb.motion.borrow().as_ref().is_some_and(|m| m.is_active());
            match (button.is_active(), playing) {
                (true, false) => lb.start_live(),
                (false, true) => lb.stop_live(),
                _ => {}
            }
        });
    }

    /// Shows the Live button for live photos; any clip from the previous photo stops.
    pub(super) fn sync_live(&self, info: &AssetInfo) {
        self.stop_live();
        let live = !info.live_video_id.is_empty() && !info.is_video();
        self.live_button.set_visible(live);
    }

    /// Space on a live photo; false when the photo isn't one.
    pub(super) fn handle_live_key(self: &Rc<Self>, key: gtk::gdk::Key) -> bool {
        let shown = self.live_button.is_visible();
        if shown && key == gtk::gdk::Key::space {
            self.live_button.set_active(!self.live_button.is_active());
        }
        shown && key == gtk::gdk::Key::space
    }

    pub(super) fn stop_live(&self) {
        if let Some(motion) = self.motion.borrow().as_ref() {
            motion.stop();
        }
        self.live_button.set_active(false);
    }

    fn start_live(self: &Rc<Self>) {
        let Some(video_id) = self
            .current_item()
            .map(|item| item.live_video_id())
            .filter(|id| !id.is_empty())
        else {
            self.live_button.set_active(false);
            return;
        };
        let motion = match self.motion_player() {
            Ok(motion) => motion,
            Err(err) => {
                self.live_button.set_active(false);
                show_error_toast(&self.ui, &gettext("Playback isn't available"), &err);
                return;
            }
        };
        if (self.zoom_level.get() - 1.0).abs() > 0.001 {
            self.zoom_reset();
        }
        let generation = self.load_gen.get();
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let source = lb.ui.ctx.api_client.video_playback_source(&video_id).await;
            if lb.load_gen.get() != generation || !lb.live_button.is_active() {
                return;
            }
            match source {
                Some((uri, api_key)) => motion.start(VideoSource {
                    uri,
                    api_key: Some(api_key),
                }),
                None => {
                    lb.live_button.set_active(false);
                    show_error_toast(
                        &lb.ui,
                        &gettext("Playback failed"),
                        &gettext("Not connected to the Immich server."),
                    );
                }
            }
        });
    }

    /// Created on first use, so photos that are never played cost no GStreamer pipeline.
    fn motion_player(self: &Rc<Self>) -> Result<Rc<MotionPlayer>, String> {
        if let Some(motion) = self.motion.borrow().as_ref() {
            return Ok(motion.clone());
        }
        let (volume, muted) = {
            let config = self.ui.ctx.config.read();
            (config.data.video_volume, config.data.video_muted)
        };
        let lb = Rc::downgrade(self);
        let motion = MotionPlayer::new(volume, muted, move |playing| {
            if let Some(lb) = lb.upgrade()
                && !playing
            {
                lb.live_button.set_active(false);
            }
        })?;
        self.live_layer.append(motion.surface());
        *self.motion.borrow_mut() = Some(motion.clone());
        Ok(motion)
    }

    pub(super) fn close_live(&self) {
        if let Some(motion) = self.motion.take() {
            motion.stop();
            self.live_layer.remove(motion.surface());
        }
    }
}
