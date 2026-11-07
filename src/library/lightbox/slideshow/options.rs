//! How a slideshow runs: its pace, order, and transition, from the settings or a memory.

use gettextrs::gettext;
use std::time::Duration;

use crate::config::ConfigData;

pub(super) const MIN_SECONDS: u32 = 2;
pub(super) const MAX_SECONDS: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::library) enum Transition {
    Crossfade,
    Slide,
    None,
}

impl Transition {
    pub(super) const ALL: [Transition; 3] =
        [Transition::Crossfade, Transition::Slide, Transition::None];

    pub(super) fn from_key(key: &str) -> Self {
        match key {
            "slide" => Transition::Slide,
            "none" => Transition::None,
            _ => Transition::Crossfade,
        }
    }

    pub(super) fn key(self) -> &'static str {
        match self {
            Transition::Crossfade => "crossfade",
            Transition::Slide => "slide",
            Transition::None => "none",
        }
    }

    pub(super) fn label(self) -> String {
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
    pub(super) fn from_config(data: &ConfigData) -> Self {
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

pub(super) fn clamp_seconds(seconds: u32) -> u32 {
    seconds.clamp(MIN_SECONDS, MAX_SECONDS)
}

/// A random position in `0..n` other than `current`; `n` must be at least 2.
pub(super) fn random_other(current: u32, n: u32) -> u32 {
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
