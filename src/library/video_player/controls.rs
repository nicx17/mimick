use gstreamer as gst;
use gtk::prelude::*;

pub(super) const SPEEDS: [f64; 8] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
pub(super) const NORMAL_SPEED: usize = 3;

pub(super) struct Controls {
    pub(super) revealer: gtk::Revealer,
    pub(super) hover: gtk::EventControllerMotion,
    pub(super) play: gtk::Button,
    pub(super) stop: gtk::Button,
    pub(super) back: gtk::Button,
    pub(super) forward: gtk::Button,
    pub(super) seek: gtk::Scale,
    pub(super) elapsed: gtk::Label,
    pub(super) total: gtk::Label,
    pub(super) repeat: gtk::ToggleButton,
    pub(super) tracks: gtk::MenuButton,
    pub(super) speed: gtk::MenuButton,
    pub(super) speeds: Vec<gtk::CheckButton>,
    pub(super) volume_menu: gtk::MenuButton,
    pub(super) volume: gtk::Scale,
    pub(super) mute: gtk::ToggleButton,
    pub(super) fullscreen: gtk::Button,
}

impl Controls {
    pub(super) fn new(volume: f64, muted: bool) -> Self {
        let (speed, speeds) = speed_menu();
        let (volume_menu, volume_scale, mute) = volume_menu(volume, muted);
        let tracks = menu_button("Audio and subtitles", &gtk::Popover::new());
        tracks.set_icon_name("media-view-subtitles-symbolic");
        tracks.set_visible(false);
        let controls = Self {
            revealer: controls_revealer(),
            hover: gtk::EventControllerMotion::new(),
            play: control_button("media-playback-start-symbolic", "Play (Space)"),
            stop: control_button("media-playback-stop-symbolic", "Stop"),
            back: control_button("media-seek-backward-symbolic", "Back 10 seconds (J)"),
            forward: control_button("media-seek-forward-symbolic", "Forward 10 seconds (L)"),
            seek: seek_scale(),
            elapsed: time_label(),
            total: time_label(),
            repeat: repeat_toggle(),
            tracks,
            speed,
            speeds,
            volume_menu,
            volume: volume_scale,
            mute,
            fullscreen: control_button("view-fullscreen-symbolic", "Fullscreen (F11)"),
        };
        controls.assemble();
        controls
    }

    fn assemble(&self) {
        let seek_row = row(8);
        seek_row.append(&self.elapsed);
        seek_row.append(&self.seek);
        seek_row.append(&self.total);
        let button_row = row(2);
        let spacer = gtk::Box::builder().hexpand(true).build();
        let buttons: [&gtk::Widget; 10] = [
            self.back.upcast_ref(),
            self.play.upcast_ref(),
            self.forward.upcast_ref(),
            self.stop.upcast_ref(),
            spacer.upcast_ref(),
            self.repeat.upcast_ref(),
            self.tracks.upcast_ref(),
            self.speed.upcast_ref(),
            self.volume_menu.upcast_ref(),
            self.fullscreen.upcast_ref(),
        ];
        for button in buttons {
            button_row.append(button);
        }
        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .css_classes(["osd", "mimick-video-controls"])
            .build();
        panel.append(&seek_row);
        panel.append(&button_row);
        panel.add_controller(self.hover.clone());
        self.revealer.set_child(Some(&panel));
    }
}

pub(super) fn popover_box() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build()
}

pub(super) fn track_section(
    content: &gtk::Box,
    title: &str,
    labels: &[String],
    active: usize,
) -> Vec<gtk::CheckButton> {
    content.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["heading"])
            .build(),
    );
    let checks = radio_checks(labels, active);
    for check in &checks {
        content.append(check);
    }
    checks
}

pub(super) fn format_time(time: gst::ClockTime) -> String {
    let secs = time.seconds();
    let (hours, minutes, seconds) = (secs / 3600, secs / 60 % 60, secs % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub(super) fn speed_label(rate: f64) -> String {
    format!("{rate}×")
}

pub(super) fn track_label(n: usize, language: Option<&str>) -> String {
    match language {
        Some(lang) if !lang.is_empty() => format!("Track {} ({})", n + 1, lang),
        _ => format!("Track {}", n + 1),
    }
}

pub(super) fn volume_icon(volume: f64, muted: bool) -> &'static str {
    if muted || volume <= 0.0 {
        "audio-volume-muted-symbolic"
    } else if volume < 0.34 {
        "audio-volume-low-symbolic"
    } else if volume < 0.67 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}

fn row(spacing: i32) -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(spacing)
        .build()
}

fn control_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .focus_on_click(false)
        .css_classes(["flat"])
        .build()
}

fn menu_button(tooltip: &str, popover: &gtk::Popover) -> gtk::MenuButton {
    gtk::MenuButton::builder()
        .tooltip_text(tooltip)
        .popover(popover)
        .direction(gtk::ArrowType::Up)
        .focus_on_click(false)
        .css_classes(["flat"])
        .build()
}

fn repeat_toggle() -> gtk::ToggleButton {
    gtk::ToggleButton::builder()
        .icon_name("media-playlist-repeat-symbolic")
        .tooltip_text("Loop")
        .focus_on_click(false)
        .css_classes(["flat"])
        .build()
}

fn seek_scale() -> gtk::Scale {
    let seek = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .draw_value(false)
        .hexpand(true)
        .focus_on_click(false)
        .build();
    seek.set_range(0.0, 1.0);
    seek
}

fn time_label() -> gtk::Label {
    gtk::Label::builder()
        .label(format_time(gst::ClockTime::ZERO))
        .css_classes(["numeric", "caption"])
        .build()
}

fn controls_revealer() -> gtk::Revealer {
    gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(180)
        .reveal_child(true)
        .valign(gtk::Align::End)
        .halign(gtk::Align::Fill)
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(12)
        .visible(false)
        .build()
}

fn radio_checks(labels: &[String], active: usize) -> Vec<gtk::CheckButton> {
    let checks: Vec<gtk::CheckButton> = labels
        .iter()
        .enumerate()
        .map(|(n, label)| {
            gtk::CheckButton::builder()
                .label(label.as_str())
                .active(n == active)
                .build()
        })
        .collect();
    for check in &checks[1..] {
        check.set_group(Some(&checks[0]));
    }
    checks
}

fn speed_menu() -> (gtk::MenuButton, Vec<gtk::CheckButton>) {
    let labels: Vec<String> = SPEEDS.iter().map(|&rate| speed_label(rate)).collect();
    let checks = radio_checks(&labels, NORMAL_SPEED);
    let content = popover_box();
    for check in &checks {
        content.append(check);
    }
    let menu = menu_button(
        "Playback speed",
        &gtk::Popover::builder().child(&content).build(),
    );
    menu.set_label(&labels[NORMAL_SPEED]);
    (menu, checks)
}

fn volume_menu(volume: f64, muted: bool) -> (gtk::MenuButton, gtk::Scale, gtk::ToggleButton) {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.05);
    scale.set_value(volume);
    scale.set_draw_value(false);
    scale.set_width_request(140);
    let mute = gtk::ToggleButton::builder()
        .icon_name("audio-volume-muted-symbolic")
        .tooltip_text("Mute (M)")
        .active(muted)
        .build();
    let content = row(6);
    content.append(&mute);
    content.append(&scale);
    let menu = menu_button("Volume", &gtk::Popover::builder().child(&content).build());
    menu.set_icon_name(volume_icon(volume, muted));
    (menu, scale, mute)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_switches_to_hours_past_one_hour() {
        assert_eq!(format_time(gst::ClockTime::ZERO), "0:00");
        assert_eq!(format_time(gst::ClockTime::from_seconds(75)), "1:15");
        assert_eq!(format_time(gst::ClockTime::from_seconds(3725)), "1:02:05");
    }

    #[test]
    fn speed_labels_drop_trailing_zeroes() {
        let labels: Vec<String> = SPEEDS.iter().map(|&r| speed_label(r)).collect();
        assert_eq!(labels[0], "0.25×");
        assert_eq!(labels[NORMAL_SPEED], "1×");
        assert_eq!(labels[7], "2×");
    }

    #[test]
    fn track_label_includes_language_when_known() {
        assert_eq!(track_label(0, Some("en")), "Track 1 (en)");
        assert_eq!(track_label(1, Some("")), "Track 2");
        assert_eq!(track_label(2, None), "Track 3");
    }

    #[test]
    fn volume_icon_reflects_level_and_mute() {
        assert_eq!(volume_icon(0.8, true), "audio-volume-muted-symbolic");
        assert_eq!(volume_icon(0.0, false), "audio-volume-muted-symbolic");
        assert_eq!(volume_icon(0.2, false), "audio-volume-low-symbolic");
        assert_eq!(volume_icon(0.5, false), "audio-volume-medium-symbolic");
        assert_eq!(volume_icon(1.0, false), "audio-volume-high-symbolic");
    }
}
