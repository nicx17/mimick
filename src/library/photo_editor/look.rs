//! How an edited photo looks: the Adjust sliders and the filters, folded into one `Recipe`.

use gettextrs::gettext;

use crate::i18n::gettext_noop;

/// The Adjust tool's sliders, in the order they're offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::library) enum Slider {
    Exposure,
    Brightness,
    Contrast,
    Highlights,
    Shadows,
    Saturation,
    Warmth,
    Tint,
    Vignette,
    Sharpen,
}

pub(in crate::library) const SLIDERS: [Slider; 10] = [
    Slider::Exposure,
    Slider::Brightness,
    Slider::Contrast,
    Slider::Highlights,
    Slider::Shadows,
    Slider::Saturation,
    Slider::Warmth,
    Slider::Tint,
    Slider::Vignette,
    Slider::Sharpen,
];

/// Each slider's name, in `SLIDERS` order.
const SLIDER_NAMES: [&str; 10] = [
    gettext_noop("Exposure"),
    gettext_noop("Brightness"),
    gettext_noop("Contrast"),
    gettext_noop("Highlights"),
    gettext_noop("Shadows"),
    gettext_noop("Saturation"),
    gettext_noop("Warmth"),
    gettext_noop("Tint"),
    gettext_noop("Vignette"),
    gettext_noop("Sharpen"),
];

impl Slider {
    pub(in crate::library) fn name(self) -> String {
        gettext(SLIDER_NAMES[self as usize])
    }

    /// The lowest value; every slider goes up to 100 and rests at 0.
    pub(in crate::library) fn min(self) -> f64 {
        if self == Slider::Sharpen { 0.0 } else { -100.0 }
    }
}

/// Every slider's value, from `Slider::min` to 100; all zero leaves the photo alone.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(in crate::library) struct Adjustments([f32; 10]);

impl Adjustments {
    pub(in crate::library) fn get(&self, slider: Slider) -> f64 {
        f64::from(self.0[slider as usize])
    }

    pub(in crate::library) fn set(&mut self, slider: Slider, value: f64) {
        self.0[slider as usize] = value.clamp(slider.min(), 100.0) as f32;
    }
}

/// The Filters tool's looks, in the order they're offered; `None` is the photo as it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::library) enum Filter {
    #[default]
    None,
    Vivid,
    Warm,
    Cool,
    Dramatic,
    Fade,
    Vintage,
    Mono,
    Noir,
    Sepia,
}

/// What each pixel goes through, every amount from -1 to 1 (`mono`, `sepia`, and `fade`
/// from 0 to 1). Sliders and filters both become one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::library) struct Recipe {
    pub exposure: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub saturation: f32,
    pub warmth: f32,
    pub tint: f32,
    pub vignette: f32,
    pub sharpen: f32,
    /// How far colours go to grey.
    pub mono: f32,
    /// How far the grey goes to brown.
    pub sepia: f32,
    /// How far black lifts towards grey.
    pub fade: f32,
}

const NEUTRAL: Recipe = Recipe {
    exposure: 0.0,
    brightness: 0.0,
    contrast: 0.0,
    highlights: 0.0,
    shadows: 0.0,
    saturation: 0.0,
    warmth: 0.0,
    tint: 0.0,
    vignette: 0.0,
    sharpen: 0.0,
    mono: 0.0,
    sepia: 0.0,
    fade: 0.0,
};

/// Each filter's name and recipe, in the order they're offered.
pub(in crate::library) const FILTERS: [(Filter, &str, Recipe); 10] = [
    (Filter::None, gettext_noop("Original"), NEUTRAL),
    (
        Filter::Vivid,
        gettext_noop("Vivid"),
        Recipe {
            saturation: 0.35,
            contrast: 0.15,
            ..NEUTRAL
        },
    ),
    (
        Filter::Warm,
        gettext_noop("Warm"),
        Recipe {
            warmth: 0.4,
            saturation: 0.1,
            ..NEUTRAL
        },
    ),
    (
        Filter::Cool,
        gettext_noop("Cool"),
        Recipe {
            warmth: -0.4,
            ..NEUTRAL
        },
    ),
    (
        Filter::Dramatic,
        gettext_noop("Dramatic"),
        Recipe {
            contrast: 0.35,
            highlights: -0.3,
            shadows: 0.2,
            saturation: -0.15,
            vignette: 0.3,
            ..NEUTRAL
        },
    ),
    (
        Filter::Fade,
        gettext_noop("Fade"),
        Recipe {
            fade: 0.12,
            contrast: -0.15,
            saturation: -0.2,
            ..NEUTRAL
        },
    ),
    (
        Filter::Vintage,
        gettext_noop("Vintage"),
        Recipe {
            warmth: 0.25,
            fade: 0.1,
            saturation: -0.3,
            vignette: 0.25,
            ..NEUTRAL
        },
    ),
    (
        Filter::Mono,
        gettext_noop("Mono"),
        Recipe {
            mono: 1.0,
            ..NEUTRAL
        },
    ),
    (
        Filter::Noir,
        gettext_noop("Noir"),
        Recipe {
            mono: 1.0,
            contrast: 0.45,
            shadows: -0.2,
            vignette: 0.35,
            ..NEUTRAL
        },
    ),
    (
        Filter::Sepia,
        gettext_noop("Sepia"),
        Recipe {
            mono: 1.0,
            sepia: 1.0,
            fade: 0.06,
            ..NEUTRAL
        },
    ),
];

/// Sliders, a filter, and how strongly the filter shows, 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::library) struct Look {
    pub adjust: Adjustments,
    pub filter: Filter,
    pub strength: f32,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            adjust: Adjustments::default(),
            filter: Filter::None,
            strength: 1.0,
        }
    }
}

impl Look {
    /// The filter at its strength with the sliders on top; amounts stay within their ranges.
    pub(in crate::library) fn recipe(&self) -> Recipe {
        let base = FILTERS
            .iter()
            .find(|(f, _, _)| *f == self.filter)
            .map_or(NEUTRAL, |(_, _, recipe)| *recipe);
        let s = self.strength.clamp(0.0, 1.0);
        let slider = |which: Slider| self.adjust.get(which) as f32 / 100.0;
        let mix = |base: f32, which: Slider| (base * s + slider(which)).clamp(-1.0, 1.0);
        Recipe {
            exposure: mix(base.exposure, Slider::Exposure),
            brightness: mix(base.brightness, Slider::Brightness),
            contrast: mix(base.contrast, Slider::Contrast),
            highlights: mix(base.highlights, Slider::Highlights),
            shadows: mix(base.shadows, Slider::Shadows),
            saturation: mix(base.saturation, Slider::Saturation),
            warmth: mix(base.warmth, Slider::Warmth),
            tint: mix(base.tint, Slider::Tint),
            vignette: mix(base.vignette, Slider::Vignette),
            sharpen: mix(base.sharpen, Slider::Sharpen),
            mono: base.mono * s,
            sepia: base.sepia * s,
            fade: base.fade * s,
        }
    }

    /// Nothing to draw: rendering would give back the same pixels.
    pub(in crate::library) fn is_neutral(&self) -> bool {
        self.recipe().is_neutral()
    }
}

impl Recipe {
    pub(in crate::library) fn is_neutral(&self) -> bool {
        *self == NEUTRAL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untouched_looks_are_neutral() {
        assert!(Look::default().is_neutral());
        let half_gone = Look {
            filter: Filter::Mono,
            strength: 0.0,
            ..Look::default()
        };
        assert!(half_gone.is_neutral());
    }

    #[test]
    fn sliders_add_to_the_filter_at_its_strength() {
        let mut look = Look {
            filter: Filter::Vivid,
            strength: 0.5,
            ..Look::default()
        };
        look.adjust.set(Slider::Saturation, 50.0);
        let recipe = look.recipe();
        assert!((recipe.saturation - (0.175 + 0.5)).abs() < 1e-6);
        assert!((recipe.contrast - 0.075).abs() < 1e-6);
    }

    #[test]
    fn sliders_stay_in_range() {
        let mut adjust = Adjustments::default();
        adjust.set(Slider::Sharpen, -40.0);
        adjust.set(Slider::Exposure, 250.0);
        assert_eq!(adjust.get(Slider::Sharpen), 0.0);
        assert_eq!(adjust.get(Slider::Exposure), 100.0);
    }

    #[test]
    fn every_slider_and_filter_has_a_name() {
        assert_eq!(SLIDERS.len(), SLIDER_NAMES.len());
        for (index, slider) in SLIDERS.iter().enumerate() {
            assert_eq!(*slider as usize, index);
        }
        assert!(FILTERS.iter().all(|(_, name, _)| !name.is_empty()));
    }
}
