//! Map styles to pick from: the server's own, VersaTiles palettes, or OpenStreetMap.

const VERSATILES_STYLES: &str = "https://tiles.versatiles.org/assets/styles";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapStyle {
    Immich,
    Colorful,
    Natural,
    Muted,
    Gray,
    Toner,
    OpenStreetMap,
}

impl MapStyle {
    pub const ALL: [Self; 7] = [
        Self::Immich,
        Self::Colorful,
        Self::Natural,
        Self::Muted,
        Self::Gray,
        Self::Toner,
        Self::OpenStreetMap,
    ];

    /// Saved in the config; the palette ids double as VersaTiles style names.
    pub fn id(self) -> &'static str {
        match self {
            Self::Immich => "immich",
            Self::Colorful => "colorful",
            Self::Natural => "natural",
            Self::Muted => "muted",
            Self::Gray => "gray",
            Self::Toner => "toner",
            Self::OpenStreetMap => "osm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Immich => "Immich",
            Self::Colorful => "Colorful",
            Self::Natural => "Natural",
            Self::Muted => "Muted",
            Self::Gray => "Gray",
            Self::Toner => "Toner",
            Self::OpenStreetMap => "OpenStreetMap",
        }
    }

    /// Unknown or empty ids fall back to the server's style.
    pub fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|style| style.id() == id)
            .unwrap_or(Self::Immich)
    }

    /// Light and dark style URLs of a VersaTiles palette.
    pub fn palette_urls(self) -> Option<(String, String)> {
        if matches!(self, Self::Immich | Self::OpenStreetMap) {
            return None;
        }
        let name = self.id();
        Some((
            format!("{VERSATILES_STYLES}/{name}/style.json"),
            format!("{VERSATILES_STYLES}/{name}-dark/style.json"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_unknown_falls_back() {
        for style in MapStyle::ALL {
            assert_eq!(MapStyle::from_id(style.id()), style);
        }
        assert_eq!(MapStyle::from_id(""), MapStyle::Immich);
        assert_eq!(MapStyle::from_id("satellite"), MapStyle::Immich);
    }

    #[test]
    fn only_palettes_have_versatiles_urls() {
        let (light, dark) = MapStyle::Gray.palette_urls().unwrap();
        assert_eq!(
            light,
            "https://tiles.versatiles.org/assets/styles/gray/style.json"
        );
        assert_eq!(
            dark,
            "https://tiles.versatiles.org/assets/styles/gray-dark/style.json"
        );
        assert_eq!(MapStyle::Immich.palette_urls(), None);
        assert_eq!(MapStyle::OpenStreetMap.palette_urls(), None);
    }
}
