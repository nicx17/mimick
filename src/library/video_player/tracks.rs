use std::rc::Rc;

use gst_play::prelude::*;
use gst_play::{PlayAudioInfo, PlayMediaInfo, PlaySubtitleInfo};
use gstreamer_play as gst_play;
use gtk::prelude::*;

use super::VideoPlayer;
use super::controls::{popover_box, track_label, track_section};

impl VideoPlayer {
    pub(super) fn track_popover(self: &Rc<Self>, info: &PlayMediaInfo) -> gtk::Popover {
        let content = popover_box();
        self.add_audio_tracks(&content, &info.audio_streams());
        self.add_subtitle_tracks(&content, &info.subtitle_streams());
        gtk::Popover::builder().child(&content).build()
    }

    fn add_audio_tracks(self: &Rc<Self>, content: &gtk::Box, audio: &[PlayAudioInfo]) {
        if audio.len() < 2 {
            return;
        }
        let current = self.play.current_audio_track().map(|t| t.index());
        let labels: Vec<String> = audio
            .iter()
            .enumerate()
            .map(|(n, t)| track_label(n, t.language().as_deref()))
            .collect();
        let active = audio
            .iter()
            .position(|t| Some(t.index()) == current)
            .unwrap_or(0);
        let checks = track_section(content, "Audio", &labels, active);
        for (check, track) in checks.iter().zip(audio) {
            let index = track.index();
            check.connect_toggled(self.on_picked(move |p| {
                if let Err(err) = p.play.set_audio_track(index) {
                    log::warn!("Audio track switch failed: {}", err);
                }
            }));
        }
    }

    fn add_subtitle_tracks(self: &Rc<Self>, content: &gtk::Box, subtitles: &[PlaySubtitleInfo]) {
        if subtitles.is_empty() {
            return;
        }
        let labels: Vec<String> = std::iter::once("Off".to_string())
            .chain(
                subtitles
                    .iter()
                    .enumerate()
                    .map(|(n, t)| track_label(n, t.language().as_deref())),
            )
            .collect();
        let checks = track_section(content, "Subtitles", &labels, 0);
        checks[0].connect_toggled(self.on_picked(|p| p.play.set_subtitle_track_enabled(false)));
        for (check, track) in checks[1..].iter().zip(subtitles) {
            let index = track.index();
            check.connect_toggled(self.on_picked(move |p| {
                if let Err(err) = p.play.set_subtitle_track(index) {
                    log::warn!("Subtitle track switch failed: {}", err);
                }
                p.play.set_subtitle_track_enabled(true);
            }));
        }
    }
}
