//! Slideshow: the lightbox steps through photos on a timer, fullscreen, with auto-hiding controls.

use gettextrs::gettext;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use libadwaita::prelude::*;

use super::Lightbox;
use crate::config::ConfigData;
use crate::library::timeline;

const HIDE_DELAY: Duration = Duration::from_secs(3);
const FADE_MS: u32 = 600;
const MIN_SECONDS: u32 = 2;
const MAX_SECONDS: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::library) enum Transition {
    Crossfade,
    Slide,
    None,
}

impl Transition {
    const ALL: [Transition; 3] = [Transition::Crossfade, Transition::Slide, Transition::None];

    fn from_key(key: &str) -> Self {
        match key {
            "slide" => Transition::Slide,
            "none" => Transition::None,
            _ => Transition::Crossfade,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Transition::Crossfade => "crossfade",
            Transition::Slide => "slide",
            Transition::None => "none",
        }
    }

    fn label(self) -> String {
        match self {
            Transition::Crossfade => gettext("Crossfade"),
            Transition::Slide => gettext("Slide"),
            Transition::None => gettext("None"),
        }
    }
}

/// How a slideshow runs; memories pass their own instead of the saved settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::library) struct SlideshowOptions {
    pub interval: Duration,
    pub shuffle: bool,
    pub repeat: bool,
    pub transition: Transition,
}

impl SlideshowOptions {
    fn from_config(data: &ConfigData) -> Self {
        Self {
            interval: Duration::from_secs(clamp_seconds(data.slideshow_interval_secs).into()),
            shuffle: data.slideshow_shuffle,
            repeat: data.slideshow_repeat,
            transition: Transition::from_key(&data.slideshow_transition),
        }
    }

    /// One photo every `seconds`, in order, crossfading, stopping at the end.
    pub(in crate::library) fn story(seconds: u32) -> Self {
        Self {
            interval: Duration::from_secs(clamp_seconds(seconds).into()),
            shuffle: false,
            repeat: false,
            transition: Transition::Crossfade,
        }
    }
}

fn clamp_seconds(seconds: u32) -> u32 {
    seconds.clamp(MIN_SECONDS, MAX_SECONDS)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Run {
    Stopped,
    Playing,
    Paused,
}

/// Slideshow widgets and timer state; the lightbox owns one for its lifetime.
pub(super) struct Slideshow {
    bar: gtk::Revealer,
    progress: gtk::ProgressBar,
    fade: gtk::Picture,
    play: gtk::Button,
    prev: gtk::Button,
    next: gtk::Button,
    settings: gtk::MenuButton,
    exit: gtk::Button,
    hover: gtk::EventControllerMotion,
    run: Cell<Run>,
    started: Cell<Instant>,
    banked: Cell<Duration>,
    options: Cell<SlideshowOptions>,
    persist: Cell<bool>,
    entered_fullscreen: Cell<bool>,
    tick: RefCell<Option<gtk::TickCallbackId>>,
    fade_animation: RefCell<Option<libadwaita::TimedAnimation>>,
    planned: Cell<Option<u32>>,
    hide_at: Cell<Instant>,
    hide_armed: Cell<bool>,
    rows: std::cell::OnceCell<SettingsRows>,
}

/// The settings popover's rows, set from the running options each time it opens.
struct SettingsRows {
    interval: libadwaita::SpinRow,
    transition: libadwaita::ComboRow,
    shuffle: libadwaita::SwitchRow,
    repeat: libadwaita::SwitchRow,
}

impl Slideshow {
    pub(super) fn new() -> Self {
        let slideshow = Self {
            bar: gtk::Revealer::builder()
                .transition_type(gtk::RevealerTransitionType::Crossfade)
                .halign(gtk::Align::Center)
                .valign(gtk::Align::End)
                .margin_bottom(24)
                .visible(false)
                .build(),
            progress: gtk::ProgressBar::builder()
                .valign(gtk::Align::Start)
                .hexpand(true)
                .can_target(false)
                .visible(false)
                .css_classes(["mimick-slideshow-progress"])
                .build(),
            fade: gtk::Picture::builder()
                .content_fit(gtk::ContentFit::Contain)
                .can_target(false)
                .visible(false)
                .build(),
            play: bar_button("media-playback-pause-symbolic", &gettext("Pause (Space)")),
            prev: bar_button("go-previous-symbolic", &gettext("Previous (Left)")),
            next: bar_button("go-next-symbolic", &gettext("Next (Right)")),
            settings: gtk::MenuButton::builder()
                .icon_name("emblem-system-symbolic")
                .tooltip_text(gettext("Slideshow Settings"))
                .css_classes(["flat", "circular"])
                .build(),
            exit: bar_button("window-close-symbolic", &gettext("End Slideshow (Esc)")),
            hover: gtk::EventControllerMotion::new(),
            run: Cell::new(Run::Stopped),
            started: Cell::new(Instant::now()),
            banked: Cell::new(Duration::ZERO),
            options: Cell::new(SlideshowOptions::story(5)),
            persist: Cell::new(true),
            entered_fullscreen: Cell::new(false),
            tick: RefCell::default(),
            fade_animation: RefCell::default(),
            planned: Cell::new(None),
            hide_at: Cell::new(Instant::now()),
            hide_armed: Cell::new(false),
            rows: std::cell::OnceCell::new(),
        };
        slideshow.assemble();
        slideshow
    }

    fn assemble(&self) {
        let panel = gtk::Box::builder()
            .spacing(4)
            .css_classes(["osd", "mimick-slideshow-controls"])
            .build();
        let separator = gtk::Separator::new(gtk::Orientation::Vertical);
        let parts: [&gtk::Widget; 6] = [
            self.prev.upcast_ref(),
            self.play.upcast_ref(),
            self.next.upcast_ref(),
            separator.upcast_ref(),
            self.settings.upcast_ref(),
            self.exit.upcast_ref(),
        ];
        for part in parts {
            panel.append(part);
        }
        panel.add_controller(self.hover.clone());
        self.bar.set_child(Some(&panel));
    }

    /// Bottom to top: the fading previous photo, the progress line, the controls.
    pub(super) fn overlays(&self) -> [gtk::Widget; 3] {
        [
            self.fade.clone().upcast(),
            self.progress.clone().upcast(),
            self.bar.clone().upcast(),
        ]
    }

    pub(super) fn is_running(&self) -> bool {
        self.run.get() != Run::Stopped
    }
}

fn bar_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .css_classes(["flat", "circular"])
        .build()
}

impl Lightbox {
    pub(super) fn connect_slideshow(self: &Rc<Self>) {
        let s = &self.slideshow;
        self.on_click(&s.play, Self::toggle_slideshow_pause);
        self.on_click(&s.prev, Self::goto_prev);
        self.on_click(&s.next, Self::goto_next);
        self.on_click(&s.exit, |lb| lb.stop_slideshow());
        s.settings.set_popover(Some(&self.slideshow_settings()));
        let motion = gtk::EventControllerMotion::new();
        let lb = Rc::downgrade(self);
        motion.connect_motion(move |_, _, _| {
            if let Some(lb) = lb.upgrade() {
                lb.bump_slideshow_bar();
            }
        });
        self.picture.overlay.add_controller(motion);
        // Touch has no pointer motion, so a tap on the photo brings the controls back.
        let tap = gtk::GestureClick::new();
        let lb = Rc::downgrade(self);
        tap.connect_released(move |_, _, _, _| {
            if let Some(lb) = lb.upgrade() {
                lb.bump_slideshow_bar();
            }
        });
        self.picture.overlay.add_controller(tap);
    }

    /// Starts from the current photo; `None` uses the saved settings, which the popover then edits.
    pub(in crate::library) fn start_slideshow(self: &Rc<Self>, options: Option<SlideshowOptions>) {
        let s = &self.slideshow;
        if s.is_running() {
            return;
        }
        s.persist.set(options.is_none());
        let options = options
            .unwrap_or_else(|| SlideshowOptions::from_config(&self.ui.ctx.config.read().data));
        s.options.set(options);
        self.picture.loader.set_reveal_child(false);
        self.stop_live();
        if let Some(player) = self.video.borrow().as_ref() {
            player.pause();
        }
        if !self.ui.window.is_fullscreen() {
            s.entered_fullscreen.set(true);
            self.ui.window.fullscreen();
        }
        s.run.set(Run::Playing);
        s.bar.set_visible(true);
        s.progress.set_visible(true);
        self.sync_slideshow_play();
        self.restart_slide_timer();
        self.install_slide_tick();
        self.bump_slideshow_bar();
        log::debug!("Slideshow started {options:?}");
    }

    pub(super) fn stop_slideshow(&self) {
        let s = &self.slideshow;
        if s.run.replace(Run::Stopped) == Run::Stopped {
            return;
        }
        if let Some(tick) = s.tick.take() {
            tick.remove();
        }
        s.progress.set_visible(false);
        s.bar.set_reveal_child(false);
        s.bar.set_visible(false);
        s.planned.set(None);
        if s.entered_fullscreen.replace(false) && self.ui.window.is_fullscreen() {
            self.ui.window.unfullscreen();
        }
        log::debug!("Slideshow stopped");
    }

    fn toggle_slideshow_pause(self: &Rc<Self>) {
        let s = &self.slideshow;
        match s.run.get() {
            Run::Playing => {
                s.banked.set(s.banked.get() + s.started.get().elapsed());
                s.run.set(Run::Paused);
            }
            Run::Paused => {
                s.started.set(Instant::now());
                s.run.set(Run::Playing);
            }
            Run::Stopped => return,
        }
        self.sync_slideshow_play();
        self.bump_slideshow_bar();
    }

    fn sync_slideshow_play(&self) {
        let paused = self.slideshow.run.get() == Run::Paused;
        let (icon, tooltip) = if paused {
            ("media-playback-start-symbolic", gettext("Resume (Space)"))
        } else {
            ("media-playback-pause-symbolic", gettext("Pause (Space)"))
        };
        self.slideshow.play.set_icon_name(icon);
        self.slideshow.play.set_tooltip_text(Some(&tooltip));
    }

    /// Space pauses, S and Escape end it; arrow keys fall through to normal navigation.
    pub(super) fn handle_slideshow_key(self: &Rc<Self>, key: gtk::gdk::Key) -> bool {
        use gtk::gdk::Key;
        if !self.slideshow.is_running() {
            return false;
        }
        match key {
            Key::space => self.toggle_slideshow_pause(),
            Key::Escape | Key::s | Key::S => self.stop_slideshow(),
            _ => return false,
        }
        true
    }

    /// Each photo gets the full interval, however it was reached.
    pub(super) fn on_slide_shown(self: &Rc<Self>) {
        if self.slideshow.is_running() {
            self.restart_slide_timer();
            if !self.slideshow.options.get().shuffle {
                self.prefetch_ahead();
            }
        }
    }

    /// While this photo shows, loads the next month or page when the step would need it,
    /// then preloads the photo after this one, so stepping never waits on the network.
    fn prefetch_ahead(self: &Rc<Self>) {
        let pos = self.pos.get();
        let lb = Rc::downgrade(self);
        let started = timeline::load_neighbor_then(&self.ui, pos, true, move |inserted| {
            let Some(lb) = lb.upgrade() else {
                return;
            };
            if let Some((at, added)) = inserted
                && at <= lb.pos.get()
            {
                lb.pos.set(lb.pos.get() + added);
            }
            lb.refill_neighbours();
        });
        if started || pos + 2 < self.ui.grid.model.n_items() {
            return;
        }
        let request = self.ui.ctx.library_state.lock().load_next_page_if_needed();
        if let Some(request) = request {
            self.refill_when_page_loads();
            crate::library::load_source_page(self.ui.clone(), request, true);
        }
    }

    fn refill_neighbours(&self) {
        self.fill_neighbours();
        self.load_neighbours();
    }

    /// Once the next page lands, fills and preloads the photo after this one.
    fn refill_when_page_loads(self: &Rc<Self>) {
        let model = self.ui.grid.model.clone();
        let before = model.n_items();
        let handler = Rc::new(RefCell::new(None::<glib::SignalHandlerId>));
        let (lb, handler_ref) = (Rc::downgrade(self), handler.clone());
        let id = model.connect_items_changed(move |model, _, _, _| {
            if model.n_items() <= before {
                return;
            }
            if let Some(id) = handler_ref.borrow_mut().take() {
                model.disconnect(id);
            }
            if let Some(lb) = lb.upgrade() {
                lb.refill_neighbours();
            }
        });
        handler.replace(Some(id));
    }

    fn restart_slide_timer(&self) {
        let s = &self.slideshow;
        s.started.set(Instant::now());
        s.banked.set(Duration::ZERO);
        s.progress.set_fraction(0.0);
        if s.options.get().shuffle {
            self.plan_shuffle();
        }
    }

    /// Picks the next shuffled photo now and warms its preview, so the switch is instant.
    fn plan_shuffle(&self) {
        let n = self.ui.grid.model.n_items();
        if n < 2 {
            return;
        }
        let target = random_other(self.pos.get(), n);
        self.slideshow.planned.set(Some(target));
        let Some(item) = self
            .ui
            .grid
            .model
            .item(target)
            .and_downcast::<crate::library::asset_object::AssetObject>()
        else {
            return;
        };
        let info = super::AssetInfo::from_item(&item);
        if info.asset_id.starts_with(crate::library::LOCAL_ID_PREFIX) {
            return;
        }
        let cache = self.ui.ctx.thumbnail_cache.clone();
        let edited = info.edited && !info.is_video();
        glib::MainContext::default().spawn_local(async move {
            let _ = cache.load_preview(&info.asset_id, edited).await;
        });
    }

    fn install_slide_tick(self: &Rc<Self>) {
        let lb = Rc::downgrade(self);
        let id = self.slideshow.progress.add_tick_callback(move |_, _| {
            if let Some(lb) = lb.upgrade() {
                lb.slide_tick();
            }
            glib::ControlFlow::Continue
        });
        if let Some(previous) = self.slideshow.tick.replace(Some(id)) {
            previous.remove();
        }
    }

    fn slide_tick(self: &Rc<Self>) {
        let s = &self.slideshow;
        if s.run.get() != Run::Playing {
            return;
        }
        let shown = s.banked.get() + s.started.get().elapsed();
        let fraction = shown.as_secs_f64() / s.options.get().interval.as_secs_f64().max(0.1);
        s.progress.set_fraction(fraction.min(1.0));
        if fraction >= 1.0 {
            // Restarted before advancing, so a slow step can't fire twice.
            self.restart_slide_timer();
            self.advance_slideshow();
        }
    }

    fn advance_slideshow(self: &Rc<Self>) {
        let s = &self.slideshow;
        let options = s.options.get();
        let n = self.ui.grid.model.n_items();
        let pos = self.pos.get();
        if options.shuffle && n > 1 {
            let target = s
                .planned
                .take()
                .filter(|&target| target < n && target != pos)
                .unwrap_or_else(|| random_other(pos, n));
            self.jump_slideshow(target, options.transition);
            return;
        }
        let more_loading = timeline::has_unloaded_neighbor(&self.ui, pos, true)
            || self.ui.ctx.library_state.lock().has_more;
        if pos + 1 < n || more_loading {
            self.step_slideshow(options.transition);
        } else if options.repeat && n > 1 {
            self.jump_slideshow(0, options.transition);
        } else {
            self.stop_slideshow();
            self.ui
                .toasts
                .add_toast(libadwaita::Toast::new(&gettext("End of slideshow")));
        }
    }

    fn step_slideshow(self: &Rc<Self>, transition: Transition) {
        let loaded_next = self.pos.get() + 1 < self.ui.grid.model.n_items();
        if transition == Transition::Slide || !loaded_next {
            self.goto_next();
            return;
        }
        if transition == Transition::Crossfade {
            self.begin_fade();
        }
        self.step_instantly(1);
    }

    fn jump_slideshow(self: &Rc<Self>, target: u32, transition: Transition) {
        if transition != Transition::None {
            self.begin_fade();
        }
        self.picture.strip.stop();
        self.leave_current();
        self.picture.strip.set_progress(0.0);
        self.pos.set(target);
        self.render();
        self.show_overlays(true);
    }

    /// Lays the outgoing photo over the new one and fades it out.
    fn begin_fade(&self) {
        let s = &self.slideshow;
        let Some(paintable) = self.active_picture().paintable() else {
            return;
        };
        s.fade.set_paintable(Some(&paintable));
        s.fade.set_opacity(1.0);
        s.fade.set_visible(true);
        let fade = s.fade.clone();
        let target = libadwaita::CallbackAnimationTarget::new(move |value| fade.set_opacity(value));
        let animation = libadwaita::TimedAnimation::new(&s.fade, 1.0, 0.0, FADE_MS, target);
        animation.set_easing(libadwaita::Easing::EaseInOutCubic);
        let fade = s.fade.clone();
        animation.connect_done(move |_| {
            fade.set_visible(false);
            fade.set_paintable(None::<&gtk::gdk::Paintable>);
        });
        animation.play();
        // Kept here: a playing animation isn't held alive by its widget.
        if let Some(previous) = s.fade_animation.replace(Some(animation)) {
            previous.skip();
        }
    }

    fn bump_slideshow_bar(self: &Rc<Self>) {
        let s = &self.slideshow;
        if !s.is_running() {
            return;
        }
        s.bar.set_reveal_child(true);
        s.hide_at.set(Instant::now() + HIDE_DELAY);
        if !s.hide_armed.replace(true) {
            self.arm_slideshow_hide(HIDE_DELAY);
        }
    }

    fn arm_slideshow_hide(self: &Rc<Self>, delay: Duration) {
        let lb = Rc::downgrade(self);
        glib::timeout_add_local_once(delay, move || {
            let Some(lb) = lb.upgrade() else {
                return;
            };
            let s = &lb.slideshow;
            let remaining = s.hide_at.get().saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                lb.arm_slideshow_hide(remaining);
                return;
            }
            s.hide_armed.set(false);
            let in_use = s.hover.contains_pointer() || s.settings.is_active();
            if s.run.get() == Run::Playing && !in_use {
                s.bar.set_reveal_child(false);
            }
        });
    }

    fn slideshow_settings(self: &Rc<Self>) -> gtk::Popover {
        let rows = SettingsRows {
            interval: self.interval_row(),
            transition: self.transition_row(),
            shuffle: self.option_switch(&gettext("Shuffle"), |o| &mut o.shuffle),
            repeat: self.option_switch(&gettext("Repeat"), |o| &mut o.repeat),
        };
        let group = libadwaita::PreferencesGroup::new();
        group.add(&rows.interval);
        group.add(&rows.transition);
        group.add(&rows.shuffle);
        group.add(&rows.repeat);
        let _ = self.slideshow.rows.set(rows);
        let content = gtk::Box::builder()
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .width_request(280)
            .build();
        content.append(&group);
        let popover = gtk::Popover::builder().child(&content).build();
        let lb = Rc::downgrade(self);
        // Rows show the running slideshow's options, which memories set without saving.
        popover.connect_show(move |_| {
            if let Some(lb) = lb.upgrade() {
                lb.sync_settings_rows();
            }
        });
        popover
    }

    fn sync_settings_rows(&self) {
        let Some(rows) = self.slideshow.rows.get() else {
            return;
        };
        let options = self.slideshow.options.get();
        rows.interval.set_value(options.interval.as_secs() as f64);
        let index = Transition::ALL
            .iter()
            .position(|t| *t == options.transition)
            .unwrap_or(0);
        rows.transition.set_selected(index as u32);
        rows.shuffle.set_active(options.shuffle);
        rows.repeat.set_active(options.repeat);
    }

    fn interval_row(self: &Rc<Self>) -> libadwaita::SpinRow {
        let row =
            libadwaita::SpinRow::with_range(f64::from(MIN_SECONDS), f64::from(MAX_SECONDS), 1.0);
        row.set_title(&gettext("Seconds per Photo"));
        let lb = Rc::downgrade(self);
        row.connect_value_notify(move |row| {
            if let Some(lb) = lb.upgrade() {
                let seconds = row.value().round() as u32;
                lb.update_options(|o| o.interval = Duration::from_secs(seconds.into()));
            }
        });
        row
    }

    fn transition_row(self: &Rc<Self>) -> libadwaita::ComboRow {
        let labels: Vec<String> = Transition::ALL.iter().map(|t| t.label()).collect();
        let model = gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
        let row = libadwaita::ComboRow::builder()
            .title(gettext("Transition"))
            .model(&model)
            .build();
        let lb = Rc::downgrade(self);
        row.connect_selected_notify(move |row| {
            if let (Some(lb), Some(&transition)) =
                (lb.upgrade(), Transition::ALL.get(row.selected() as usize))
            {
                lb.update_options(|o| o.transition = transition);
            }
        });
        row
    }

    fn option_switch(
        self: &Rc<Self>,
        title: &str,
        field: fn(&mut SlideshowOptions) -> &mut bool,
    ) -> libadwaita::SwitchRow {
        let row = libadwaita::SwitchRow::builder().title(title).build();
        let lb = Rc::downgrade(self);
        row.connect_active_notify(move |row| {
            if let Some(lb) = lb.upgrade() {
                let active = row.is_active();
                lb.update_options(|o| *field(o) = active);
            }
        });
        row
    }

    /// Applies a settings change to the running slideshow, and saves it unless a memory set it.
    fn update_options(&self, change: impl FnOnce(&mut SlideshowOptions)) {
        let s = &self.slideshow;
        let mut options = s.options.get();
        change(&mut options);
        if options == s.options.get() {
            return;
        }
        s.options.set(options);
        if options.shuffle && s.planned.get().is_none() {
            self.plan_shuffle();
        }
        if !s.persist.get() {
            return;
        }
        let mut config = self.ui.ctx.config.write();
        let data = &mut config.data;
        data.slideshow_interval_secs = options.interval.as_secs() as u32;
        data.slideshow_shuffle = options.shuffle;
        data.slideshow_repeat = options.repeat;
        data.slideshow_transition = options.transition.key().to_string();
        if !config.save() {
            log::warn!("Could not save slideshow settings");
        }
    }
}

/// A random position in `0..n` other than `current`; `n` must be at least 2.
fn random_other(current: u32, n: u32) -> u32 {
    let pick = glib::random_int_range(0, (n - 1) as i32) as u32;
    if pick >= current { pick + 1 } else { pick }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions_round_trip_and_default_to_crossfade() {
        for transition in Transition::ALL {
            assert_eq!(Transition::from_key(transition.key()), transition);
        }
        assert_eq!(Transition::from_key(""), Transition::Crossfade);
    }

    #[test]
    fn saved_interval_is_clamped() {
        let data = ConfigData {
            slideshow_interval_secs: 0,
            ..ConfigData::default()
        };
        assert_eq!(
            SlideshowOptions::from_config(&data).interval,
            Duration::from_secs(2)
        );
        assert_eq!(
            SlideshowOptions::story(500).interval,
            Duration::from_secs(60)
        );
    }

    #[test]
    fn random_other_never_repeats_the_current_photo() {
        for _ in 0..200 {
            let pick = random_other(1, 3);
            assert!(pick == 0 || pick == 2);
        }
        assert_eq!(random_other(0, 2), 1);
    }
}
