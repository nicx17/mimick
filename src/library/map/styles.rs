//! Map styles to pick from: the server's own, bundled VersaTiles themes, satellite imagery, or OpenStreetMap.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapStyle {
    Immich,
    Colorful,
    Natural,
    Muted,
    Gray,
    Toner,
    Positrino,
    Protocol,
    Protostar,
    Protozoa,
    Fnord,
    Classic,
    Googol,
    Freedom,
    Macbob,
    Mactaylor,
    Bingo,
    Satellite,
    OpenStreetMap,
}

impl MapStyle {
    pub const ALL: [Self; 19] = [
        Self::Immich,
        Self::Colorful,
        Self::Natural,
        Self::Muted,
        Self::Gray,
        Self::Toner,
        Self::Positrino,
        Self::Protocol,
        Self::Protostar,
        Self::Protozoa,
        Self::Fnord,
        Self::Classic,
        Self::Googol,
        Self::Freedom,
        Self::Macbob,
        Self::Mactaylor,
        Self::Bingo,
        Self::Satellite,
        Self::OpenStreetMap,
    ];

    /// VersaTiles' own palettes, listed before the themes that resemble other maps.
    pub const PALETTES: [Self; 5] = [
        Self::Colorful,
        Self::Natural,
        Self::Muted,
        Self::Gray,
        Self::Toner,
    ];

    pub const LOOKALIKES: [Self; 11] = [
        Self::Positrino,
        Self::Protocol,
        Self::Protostar,
        Self::Protozoa,
        Self::Fnord,
        Self::Classic,
        Self::Googol,
        Self::Freedom,
        Self::Macbob,
        Self::Mactaylor,
        Self::Bingo,
    ];

    /// Saved in the config; the theme ids double as bundled style file names.
    pub fn id(self) -> &'static str {
        self.names().0
    }

    pub fn label(self) -> &'static str {
        self.names().1
    }

    /// Id and label for each style, in `ALL` order.
    const NAMES: [(&str, &str); Self::ALL.len()] = [
        ("immich", "Immich"),
        ("colorful", "Colorful"),
        ("natural", "Natural"),
        ("muted", "Muted"),
        ("gray", "Gray"),
        ("toner", "Toner"),
        ("positrino", "Positrino"),
        ("protocol", "Protocol"),
        ("protostar", "Protostar"),
        ("protozoa", "Protozoa"),
        ("fnord", "Fnord"),
        ("classic", "Classic"),
        ("googol", "Googol"),
        ("freedom", "Freedom"),
        ("macbob", "Macbob"),
        ("mactaylor", "Mactaylor"),
        ("bingo", "Bingo"),
        ("satellite", "Satellite"),
        ("osm", "OpenStreetMap"),
    ];

    fn names(self) -> (&'static str, &'static str) {
        Self::NAMES[self as usize]
    }

    /// Unknown or empty ids fall back to the server's style.
    pub fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|style| style.id() == id)
            .unwrap_or(Self::Immich)
    }

    /// The bundled themes for light and dark mode; a theme without a dark twin is used for both.
    pub fn themes(self) -> Option<(String, String)> {
        let id = self.id();
        match self {
            Self::Immich | Self::Satellite | Self::OpenStreetMap => None,
            Self::Colorful
            | Self::Natural
            | Self::Muted
            | Self::Gray
            | Self::Toner
            | Self::Positrino
            | Self::Protocol
            | Self::Protostar => Some((id.to_string(), format!("{id}-dark"))),
            _ => Some((id.to_string(), id.to_string())),
        }
    }

    /// The bundled labels-only theme drawn over the style's imagery.
    pub fn labels(self) -> Option<&'static str> {
        (self == Self::Satellite).then_some("satellite-labels")
    }
}

/// Whether maps follow the system's light or dark style, or keep one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapAppearance {
    System,
    Light,
    Dark,
}

impl MapAppearance {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Follow System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    /// Unknown or empty ids follow the system.
    pub fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|appearance| appearance.id() == id)
            .unwrap_or(Self::System)
    }

    pub fn is_dark(self, system_dark: bool) -> bool {
        match self {
            Self::System => system_dark,
            Self::Light => false,
            Self::Dark => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearances_round_trip_and_decide_dark() {
        for appearance in MapAppearance::ALL {
            assert_eq!(MapAppearance::from_id(appearance.id()), appearance);
        }
        assert_eq!(MapAppearance::from_id(""), MapAppearance::System);
        assert!(MapAppearance::System.is_dark(true));
        assert!(!MapAppearance::System.is_dark(false));
        assert!(!MapAppearance::Light.is_dark(true));
        assert!(MapAppearance::Dark.is_dark(false));
    }

    #[test]
    fn all_lists_styles_in_declaration_order() {
        for (index, style) in MapStyle::ALL.into_iter().enumerate() {
            assert_eq!(style as usize, index, "{style:?}");
        }
        assert_eq!(MapStyle::OpenStreetMap.names(), ("osm", "OpenStreetMap"));
    }

    #[test]
    fn ids_round_trip_and_unknown_falls_back() {
        for style in MapStyle::ALL {
            assert_eq!(MapStyle::from_id(style.id()), style);
        }
        assert_eq!(MapStyle::from_id(""), MapStyle::Immich);
        assert_eq!(MapStyle::from_id("terrain"), MapStyle::Immich);
    }

    #[test]
    fn themes_pair_light_and_dark() {
        let pair = |style: MapStyle| style.themes().unwrap();
        assert_eq!(pair(MapStyle::Gray), ("gray".into(), "gray-dark".into()));
        assert_eq!(pair(MapStyle::Googol), ("googol".into(), "googol".into()));
        assert_eq!(pair(MapStyle::Fnord), ("fnord".into(), "fnord".into()));
        assert_eq!(MapStyle::Immich.themes(), None);
        assert_eq!(MapStyle::Satellite.themes(), None);
        assert_eq!(MapStyle::Satellite.labels(), Some("satellite-labels"));
        assert_eq!(MapStyle::Gray.labels(), None);
        assert_eq!(MapStyle::OpenStreetMap.themes(), None);
    }

    #[test]
    fn every_bundled_style_is_in_one_menu_group() {
        let grouped: Vec<MapStyle> = [MapStyle::Immich]
            .into_iter()
            .chain(MapStyle::PALETTES)
            .chain(MapStyle::LOOKALIKES)
            .chain([MapStyle::Satellite, MapStyle::OpenStreetMap])
            .collect();
        assert_eq!(grouped, MapStyle::ALL);
    }
}
