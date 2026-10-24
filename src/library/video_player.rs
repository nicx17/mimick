mod controls;
mod pipeline;
mod tracks;

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gst_play::{PlayMediaInfo, PlayMessage, PlayState};
use gstreamer as gst;
use gstreamer_play as gst_play;
use gtk::prelude::*;
use parking_lot::Mutex;

use controls::{Controls, NORMAL_SPEED, SPEEDS, format_time, speed_label, volume_icon};
use pipeline::frame_duration;

const SKIP: gst::ClockTime = gst::ClockTime::from_seconds(10);
const DEFAULT_FRAME: gst::ClockTime = gst::ClockTime::from_nseconds(33_333_333);
const HIDE_DELAY: Duration = Duration::from_secs(3);
const SEEK_SETTLE: Duration = Duration::from_millis(400);

pub(super) struct VideoSource {
    pub(super) uri: String,
    pub(super) api_key: Option<String>,
}

pub(super) struct VideoHooks {
    pub(super) start: Box<dyn Fn()>,
    pub(super) fullscreen: Box<dyn Fn()>,
    pub(super) error: Box<dyn Fn(String)>,
}

pub(super) struct VideoPlayer {
    play: gst_play::Play,
    api_key: Arc<Mutex<Option<String>>>,
    _bus_watch: gst::bus::BusWatchGuard,
    surface: gtk::Picture,
    badge: gtk::Button,
    controls: Controls,
    hooks: VideoHooks,
    state: Cell<PlayState>,
    available: Cell<bool>,
    loaded: Cell<bool>,
    restarting: Cell<bool>,
    last_seek: Cell<Option<Instant>>,
    frame: Cell<gst::ClockTime>,
    track_counts: Cell<(u32, u32)>,
    pointer: Cell<(f64, f64)>,
    hide_at: Cell<Instant>,
    hide_armed: Cell<bool>,
}

impl VideoPlayer {
    pub(super) fn new(
        badge: &gtk::Button,
        volume: f64,
        muted: bool,
        hooks: VideoHooks,
    ) -> Result<Rc<Self>, String> {
        let api_key = Arc::new(Mutex::new(None));
        let (play, paintable) = pipeline::build_play(volume, muted, api_key.clone())?;
        let bus = play.message_bus();
        let player = Rc::new_cyclic(|weak: &Weak<Self>| Self {
            _bus_watch: watch_bus(&bus, weak.clone()),
            play,
            api_key,
            surface: video_surface(&paintable),
            badge: badge.clone(),
            controls: Controls::new(volume, muted),
            hooks,
            state: Cell::new(PlayState::Stopped),
            available: Cell::new(false),
            loaded: Cell::new(false),
            restarting: Cell::new(false),
            last_seek: Cell::new(None),
            frame: Cell::new(DEFAULT_FRAME),
            track_counts: Cell::new((0, 0)),
            pointer: Cell::new((0.0, 0.0)),
            hide_at: Cell::new(Instant::now()),
            hide_armed: Cell::new(false),
        });
        player.connect_buttons();
        player.connect_volume();
        player.connect_pointer();
        Ok(player)
    }

    pub(super) fn surface(&self) -> &gtk::Picture {
        &self.surface
    }

    pub(super) fn controls(&self) -> &gtk::Revealer {
        &self.controls.revealer
    }

    pub(super) fn volume(&self) -> f64 {
        self.controls.volume.value()
    }

    pub(super) fn muted(&self) -> bool {
        self.controls.mute.is_active()
    }

    pub(super) fn bind_compact(&self, split: &libadwaita::OverlaySplitView) {
        for button in [&self.controls.back, &self.controls.forward] {
            split
                .bind_property("collapsed", button, "visible")
                .invert_boolean()
                .sync_create()
                .build();
        }
        for margin in ["margin-start", "margin-end", "margin-bottom"] {
            split
                .bind_property("collapsed", &self.controls.revealer, margin)
                .transform_to(|_, collapsed: bool| Some(if collapsed { 6 } else { 12 }))
                .sync_create()
                .build();
        }
    }

    pub(super) fn prepare(self: &Rc<Self>, available: bool) {
        self.play.stop();
        self.available.set(available);
        self.loaded.set(false);
        self.restarting.set(false);
        self.frame.set(DEFAULT_FRAME);
        self.track_counts.set((0, 0));
        self.controls.tracks.set_visible(false);
        self.controls.revealer.set_visible(available);
        self.show_duration(None);
        self.state.set(PlayState::Stopped);
        self.apply_state();
    }

    pub(super) fn load(&self, source: VideoSource) {
        *self.api_key.lock() = source.api_key;
        self.loaded.set(true);
        self.play.set_uri(Some(&source.uri));
        self.play.set_subtitle_track_enabled(false);
        self.play.play();
    }

    pub(super) fn toggle_play(&self) {
        if !self.available.get() {
            return;
        }
        if !self.loaded.get() {
            (self.hooks.start)();
        } else if self.is_playing() {
            self.play.pause();
        } else {
            self.play.play();
        }
    }

    pub(super) fn pause(&self) {
        if self.is_playing() {
            self.play.pause();
        }
    }

    pub(super) fn stop(&self) {
        self.restarting.set(false);
        self.play.stop();
    }

    pub(super) fn skip(&self, forward: bool) {
        let Some(position) = self.active_position() else {
            return;
        };
        let target = if forward {
            position + SKIP
        } else {
            position.saturating_sub(SKIP)
        };
        self.play
            .seek(self.play.duration().map_or(target, |d| target.min(d)));
    }

    pub(super) fn step_frame(&self, forward: bool) {
        let Some(position) = self.active_position() else {
            return;
        };
        self.pause();
        let frame = self.frame.get();
        self.play.seek(if forward {
            position + frame
        } else {
            position.saturating_sub(frame)
        });
    }

    pub(super) fn toggle_mute(&self) {
        self.controls
            .mute
            .set_active(!self.controls.mute.is_active());
    }

    pub(super) fn set_fullscreen(&self, fullscreen: bool) {
        let (icon, tooltip) = if fullscreen {
            ("view-restore-symbolic", "Exit fullscreen (F11)")
        } else {
            ("view-fullscreen-symbolic", "Fullscreen (F11)")
        };
        self.controls.fullscreen.set_icon_name(icon);
        self.controls.fullscreen.set_tooltip_text(Some(tooltip));
    }

    fn is_playing(&self) -> bool {
        matches!(self.state.get(), PlayState::Playing | PlayState::Buffering)
    }

    fn active_position(&self) -> Option<gst::ClockTime> {
        if self.state.get() == PlayState::Stopped {
            return None;
        }
        self.play.position()
    }

    fn handle_message(self: &Rc<Self>, message: &gst::Message) {
        let Ok(message) = PlayMessage::parse(message) else {
            return;
        };
        match message {
            PlayMessage::StateChanged(m) => self.on_state_changed(m.state()),
            PlayMessage::PositionUpdated(m) => self.show_position(m.position()),
            PlayMessage::SeekDone(m) => self.show_position(m.position()),
            PlayMessage::DurationChanged(m) => self.show_duration(m.duration()),
            PlayMessage::MediaInfoUpdated(m) => self.show_media_info(m.media_info()),
            PlayMessage::EndOfStream(_) => self.on_end_of_stream(),
            PlayMessage::Error(m) => {
                log::warn!("Video playback failed: {}", m.error());
                (self.hooks.error)(m.error().message().to_string());
            }
            _ => {}
        }
    }

    fn on_state_changed(self: &Rc<Self>, state: PlayState) {
        if self.restarting.get() {
            if state == PlayState::Stopped {
                return;
            }
            if state == PlayState::Playing {
                self.restarting.set(false);
                self.apply_speed();
            }
        }
        self.state.set(state);
        self.apply_state();
    }

    fn on_end_of_stream(&self) {
        if self.controls.repeat.is_active() {
            self.restarting.set(true);
            self.play.play();
        }
    }

    fn apply_state(self: &Rc<Self>) {
        let active = self.state.get() != PlayState::Stopped;
        let playing = self.is_playing();
        let c = &self.controls;
        self.surface.set_visible(active);
        self.badge.set_visible(self.available.get() && !active);
        let (icon, tooltip) = if playing {
            ("media-playback-pause-symbolic", "Pause (Space)")
        } else {
            ("media-playback-start-symbolic", "Play (Space)")
        };
        c.play.set_icon_name(icon);
        c.play.set_tooltip_text(Some(tooltip));
        c.stop.set_sensitive(active);
        c.back.set_sensitive(active);
        c.forward.set_sensitive(active);
        c.seek.set_sensitive(active);
        c.speed.set_sensitive(active);
        if !active {
            c.speeds[NORMAL_SPEED].set_active(true);
            self.set_position(gst::ClockTime::ZERO);
        }
        if playing {
            self.bump_controls();
        } else {
            self.reveal_controls();
        }
    }

    fn apply_speed(&self) {
        let index = self.speed_index();
        self.controls.speed.set_label(&speed_label(SPEEDS[index]));
        if self.state.get() != PlayState::Stopped && !self.restarting.get() {
            self.play.set_rate(SPEEDS[index]);
        }
    }

    fn speed_index(&self) -> usize {
        self.controls
            .speeds
            .iter()
            .position(gtk::CheckButton::is_active)
            .unwrap_or(NORMAL_SPEED)
    }

    fn show_position(&self, position: Option<gst::ClockTime>) {
        let settling = self
            .last_seek
            .get()
            .is_some_and(|t| t.elapsed() < SEEK_SETTLE);
        if self.state.get() == PlayState::Stopped || settling {
            return;
        }
        self.set_position(position.unwrap_or(gst::ClockTime::ZERO));
    }

    fn set_position(&self, position: gst::ClockTime) {
        self.controls.seek.set_value(position.seconds_f64());
        self.controls.elapsed.set_label(&format_time(position));
    }

    fn show_duration(&self, duration: Option<gst::ClockTime>) {
        let duration = duration.unwrap_or(gst::ClockTime::ZERO);
        self.controls
            .seek
            .set_range(0.0, duration.seconds_f64().max(1.0));
        self.controls.total.set_label(&format_time(duration));
    }

    fn seek_to(&self, seconds: f64) {
        self.last_seek.set(Some(Instant::now()));
        let target = gst::ClockTime::from_seconds_f64(seconds.max(0.0));
        self.controls.elapsed.set_label(&format_time(target));
        self.play.seek(target);
    }

    fn show_media_info(self: &Rc<Self>, info: &PlayMediaInfo) {
        if let Some(frame) = info.video_streams().first().and_then(|video| {
            let rate = video.framerate();
            frame_duration(rate.numer(), rate.denom())
        }) {
            self.frame.set(frame);
        }
        let counts = (
            info.number_of_audio_streams(),
            info.number_of_subtitle_streams(),
        );
        if self.track_counts.replace(counts) == counts {
            return;
        }
        self.controls
            .tracks
            .set_visible(counts.0 > 1 || counts.1 > 0);
        self.controls
            .tracks
            .set_popover(Some(&self.track_popover(info)));
    }

    fn connect_buttons(self: &Rc<Self>) {
        let c = &self.controls;
        c.play.connect_clicked(self.on_click(|p| p.toggle_play()));
        c.stop.connect_clicked(self.on_click(|p| p.stop()));
        c.back.connect_clicked(self.on_click(|p| p.skip(false)));
        c.forward.connect_clicked(self.on_click(|p| p.skip(true)));
        c.fullscreen
            .connect_clicked(self.on_click(|p| (p.hooks.fullscreen)()));
        for check in &c.speeds {
            check.connect_toggled(self.on_picked(|p| p.apply_speed()));
        }
        let weak = Rc::downgrade(self);
        c.seek.connect_change_value(move |_, _, value| {
            if let Some(p) = weak.upgrade() {
                p.seek_to(value);
            }
            glib::Propagation::Proceed
        });
    }

    fn connect_volume(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.controls.volume.connect_value_changed(move |scale| {
            if let Some(p) = weak.upgrade() {
                p.play.set_volume(scale.value());
                p.update_volume_icon();
            }
        });
        let weak = Rc::downgrade(self);
        self.controls.mute.connect_toggled(move |mute| {
            if let Some(p) = weak.upgrade() {
                p.play.set_mute(mute.is_active());
                p.update_volume_icon();
            }
        });
    }

    fn connect_pointer(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.controls.hover.connect_motion(move |_, _, _| {
            if let Some(p) = weak.upgrade() {
                p.bump_controls();
            }
        });
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(self);
        motion.connect_motion(move |_, x, y| {
            if let Some(p) = weak.upgrade()
                && p.pointer.replace((x, y)) != (x, y)
            {
                p.bump_controls();
            }
        });
        self.surface.add_controller(motion);
        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(self);
        click.connect_pressed(move |_, n_press, _, _| {
            let Some(p) = weak.upgrade() else {
                return;
            };
            match n_press {
                1 => p.bump_controls(),
                2 => (p.hooks.fullscreen)(),
                _ => {}
            }
        });
        self.surface.add_controller(click);
    }

    fn on_click<F: Fn(&Rc<Self>) + 'static>(
        self: &Rc<Self>,
        f: F,
    ) -> impl Fn(&gtk::Button) + use<F> {
        let weak = Rc::downgrade(self);
        move |_| {
            if let Some(p) = weak.upgrade() {
                f(&p);
            }
        }
    }

    fn on_picked<F: Fn(&Rc<Self>) + 'static>(
        self: &Rc<Self>,
        f: F,
    ) -> impl Fn(&gtk::CheckButton) + use<F> {
        let weak = Rc::downgrade(self);
        move |check| {
            if check.is_active()
                && let Some(p) = weak.upgrade()
            {
                f(&p);
            }
        }
    }

    fn update_volume_icon(&self) {
        self.controls
            .volume_menu
            .set_icon_name(volume_icon(self.volume(), self.muted()));
    }

    fn reveal_controls(&self) {
        self.controls.revealer.set_reveal_child(true);
        self.surface.set_cursor(None::<&gtk::gdk::Cursor>);
    }

    fn bump_controls(self: &Rc<Self>) {
        self.reveal_controls();
        self.hide_at.set(Instant::now() + HIDE_DELAY);
        if !self.hide_armed.replace(true) {
            self.arm_hide(HIDE_DELAY);
        }
    }

    fn arm_hide(self: &Rc<Self>, delay: Duration) {
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(delay, move || {
            let Some(p) = weak.upgrade() else {
                return;
            };
            let remaining = p.hide_at.get().saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                p.arm_hide(remaining);
                return;
            }
            p.hide_armed.set(false);
            if p.is_playing() && !p.controls_in_use() {
                p.controls.revealer.set_reveal_child(false);
                p.surface.set_cursor_from_name(Some("none"));
            }
        });
    }

    fn controls_in_use(&self) -> bool {
        let c = &self.controls;
        c.hover.contains_pointer()
            || c.volume_menu.is_active()
            || c.tracks.is_active()
            || c.speed.is_active()
    }
}

fn watch_bus(bus: &gst::Bus, player: Weak<VideoPlayer>) -> gst::bus::BusWatchGuard {
    bus.add_watch_local(move |_, message| {
        if let Some(player) = player.upgrade() {
            player.handle_message(message);
        }
        glib::ControlFlow::Continue
    })
    .expect("a new GstPlay bus has no watch")
}

fn video_surface(paintable: &gtk::gdk::Paintable) -> gtk::Picture {
    gtk::Picture::builder()
        .paintable(paintable)
        .content_fit(gtk::ContentFit::Contain)
        .hexpand(true)
        .vexpand(true)
        .visible(false)
        .css_classes(["mimick-video-surface"])
        .build()
}
