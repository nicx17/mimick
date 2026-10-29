use std::rc::Rc;

use gtk::prelude::*;

use super::{AssetInfo, Lightbox};
use crate::library::context_menu::show_error_toast;
use crate::library::download::{open_with_chooser, spawn_video_handoff};
use crate::library::video_player::{VideoHooks, VideoPlayer, VideoSource};

impl Lightbox {
    fn video_player(self: &Rc<Self>) -> Result<Rc<VideoPlayer>, String> {
        if let Some(player) = self.video.borrow().as_ref() {
            return Ok(player.clone());
        }
        let (volume, muted) = {
            let config = self.ui.ctx.config.read();
            (config.data.video_volume, config.data.video_muted)
        };
        let player = VideoPlayer::new(&self.video_badge_button, volume, muted, self.video_hooks())?;
        self.picture.overlay.add_overlay(player.surface());
        self.picture.overlay.add_overlay(player.controls());
        let swipe = gtk::GestureSwipe::new();
        swipe.set_touch_only(false);
        let lb = Rc::downgrade(self);
        swipe.connect_swipe(move |_, vx, _| {
            if let Some(lb) = lb.upgrade() {
                lb.handle_swipe(vx);
            }
        });
        player.surface().add_controller(swipe);
        player.bind_compact(&self.ui.split);
        player.set_fullscreen(self.ui.window.is_fullscreen());
        *self.video.borrow_mut() = Some(player.clone());
        Ok(player)
    }

    fn video_hooks(self: &Rc<Self>) -> VideoHooks {
        let start = Rc::downgrade(self);
        let fullscreen = Rc::downgrade(self);
        let error = Rc::downgrade(self);
        VideoHooks {
            start: Box::new(move || {
                if let Some(lb) = start.upgrade() {
                    lb.load_video();
                }
            }),
            fullscreen: Box::new(move || {
                if let Some(lb) = fullscreen.upgrade() {
                    lb.toggle_fullscreen();
                }
            }),
            error: Box::new(move |message| {
                if let Some(lb) = error.upgrade() {
                    show_error_toast(&lb.ui, "Playback failed", &message);
                }
            }),
        }
    }

    pub(super) fn prepare_video(self: &Rc<Self>, is_video: bool) {
        let player = if is_video {
            self.video_player()
                .inspect_err(|err| log::debug!("Video playback unavailable: {}", err))
                .ok()
        } else {
            self.video.borrow().clone()
        };
        if let Some(player) = player {
            player.prepare(is_video);
        }
    }

    pub(super) fn toggle_video(self: &Rc<Self>) {
        match self.video_player() {
            Ok(player) => player.toggle_play(),
            Err(err) => show_error_toast(&self.ui, "Playback isn't available", &err),
        }
    }

    fn load_video(self: &Rc<Self>) {
        let Some(item) = self.current_item() else {
            return;
        };
        let info = AssetInfo::from_item(&item);
        let generation = self.load_gen.get();
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let source = lb.video_source(&info).await;
            if lb.load_gen.get() != generation {
                return;
            }
            let Some(player) = lb.video.borrow().clone() else {
                return;
            };
            match source {
                Ok(source) => player.load(source),
                Err(err) => show_error_toast(&lb.ui, "Playback failed", &err),
            }
        });
    }

    async fn video_source(&self, info: &AssetInfo) -> Result<VideoSource, String> {
        if !info.local_path.is_empty() && std::path::Path::new(&info.local_path).exists() {
            return Ok(VideoSource {
                uri: gtk::gio::File::for_path(&info.local_path).uri().to_string(),
                api_key: None,
            });
        }
        if info.is_local() {
            return Err("The video file is no longer on disk.".into());
        }
        let (uri, api_key) = self
            .ui
            .ctx
            .api_client
            .video_playback_source(&info.asset_id)
            .await
            .ok_or_else(|| "Not connected to the Immich server.".to_string())?;
        Ok(VideoSource {
            uri,
            api_key: Some(api_key),
        })
    }

    pub(super) fn handle_video_key(self: &Rc<Self>, key: gtk::gdk::Key) -> bool {
        use gtk::gdk::Key;
        let is_video = self
            .current_item()
            .is_some_and(|item| AssetInfo::from_item(&item).is_video());
        if !is_video {
            return false;
        }
        match key {
            Key::space => self.toggle_video(),
            _ => {
                let Some(player) = self.video.borrow().clone() else {
                    return false;
                };
                match key {
                    Key::j | Key::J => player.skip(false),
                    Key::l | Key::L => player.skip(true),
                    Key::comma => player.step_frame(false),
                    Key::period => player.step_frame(true),
                    Key::m | Key::M => player.toggle_mute(),
                    _ => return false,
                }
            }
        }
        true
    }

    pub(super) fn close_video(&self) {
        let Some(player) = self.video.take() else {
            return;
        };
        player.stop();
        let mut config = self.ui.ctx.config.write();
        if config.data.video_volume != player.volume() || config.data.video_muted != player.muted()
        {
            config.data.video_volume = player.volume();
            config.data.video_muted = player.muted();
            let _ = config.save();
        }
    }

    pub(super) fn open_current_with(&self) {
        let Some(item) = self.current_item() else {
            return;
        };
        let info = AssetInfo::from_item(&item);
        if let Some(player) = self.video.borrow().as_ref() {
            player.pause();
        }
        if !info.local_path.is_empty() {
            open_with_chooser(&self.ui.window, std::path::Path::new(&info.local_path));
        } else {
            spawn_video_handoff(self.ui.clone(), info.asset_id, info.filename);
        }
    }
}
