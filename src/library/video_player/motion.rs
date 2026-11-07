//! Live photo playback: the motion clip plays over the still on a loop, with no controls.

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use gst_play::{PlayMessage, PlayState};
use gstreamer as gst;
use gstreamer_play as gst_play;
use gtk::prelude::*;
use parking_lot::Mutex;

use super::VideoSource;

pub(in crate::library) struct MotionPlayer {
    play: gst_play::Play,
    api_key: Arc<Mutex<Option<String>>>,
    _bus_watch: gst::bus::BusWatchGuard,
    surface: gtk::Picture,
    active: Cell<bool>,
    on_change: Box<dyn Fn(bool)>,
}

impl MotionPlayer {
    /// `on_change` hears when playback starts and stops, including after an error.
    pub(in crate::library) fn new(
        volume: f64,
        muted: bool,
        on_change: impl Fn(bool) + 'static,
    ) -> Result<Rc<Self>, String> {
        let api_key = Arc::new(Mutex::new(None));
        let (play, paintable) = super::pipeline::build_play(volume, muted, api_key.clone())?;
        let bus = play.message_bus();
        Ok(Rc::new_cyclic(|weak: &Weak<Self>| Self {
            _bus_watch: watch_bus(&bus, weak.clone()),
            play,
            api_key,
            surface: motion_surface(&paintable),
            active: Cell::new(false),
            on_change: Box::new(on_change),
        }))
    }

    pub(in crate::library) fn surface(&self) -> &gtk::Picture {
        &self.surface
    }

    pub(in crate::library) fn is_active(&self) -> bool {
        self.active.get()
    }

    pub(in crate::library) fn start(&self, source: VideoSource) {
        *self.api_key.lock() = source.api_key;
        self.play.set_uri(Some(&source.uri));
        self.set_active(true);
        self.play.play();
    }

    pub(in crate::library) fn stop(&self) {
        self.play.stop();
        self.surface.set_visible(false);
        self.set_active(false);
    }

    fn set_active(&self, active: bool) {
        if self.active.replace(active) != active {
            (self.on_change)(active);
        }
    }

    fn handle_message(&self, message: &gst::Message) {
        match PlayMessage::parse(message) {
            // Shown only once frames flow, so the still never flashes black.
            Ok(PlayMessage::StateChanged(m)) if m.state() == PlayState::Playing => {
                self.surface.set_visible(self.active.get());
            }
            Ok(PlayMessage::EndOfStream(_)) if self.active.get() => self.play.play(),
            Ok(PlayMessage::Error(m)) => {
                log::warn!("Live photo playback failed: {}", m.error());
                self.stop();
            }
            _ => {}
        }
    }
}

impl Drop for MotionPlayer {
    fn drop(&mut self) {
        self.play.stop();
    }
}

fn watch_bus(bus: &gst::Bus, player: Weak<MotionPlayer>) -> gst::bus::BusWatchGuard {
    bus.add_watch_local(move |_, message| {
        if let Some(player) = player.upgrade() {
            player.handle_message(message);
        }
        glib::ControlFlow::Continue
    })
    .expect("a new GstPlay bus has no watch")
}

/// Input passes through to the picture scroller, so pan, pinch and swipe keep working.
fn motion_surface(paintable: &gtk::gdk::Paintable) -> gtk::Picture {
    gtk::Picture::builder()
        .paintable(paintable)
        .content_fit(gtk::ContentFit::Contain)
        .hexpand(true)
        .vexpand(true)
        .can_target(false)
        .visible(false)
        .build()
}
