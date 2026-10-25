//! GPS positions typed, pasted, or copied as "latitude, longitude" text.

/// A valid WGS84 position in decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

impl GeoPoint {
    pub fn new(lat: f64, lon: f64) -> Option<Self> {
        let valid = lat.is_finite()
            && lon.is_finite()
            && (-90.0..=90.0).contains(&lat)
            && (-180.0..=180.0).contains(&lon);
        valid.then_some(Self { lat, lon })
    }

    /// Accepts "lat, lon", "lat lon", or "lat; lon", optionally wrapped in parentheses.
    pub fn parse(text: &str) -> Option<Self> {
        let inner = text.trim().trim_start_matches('(').trim_end_matches(')');
        let parts: Vec<&str> = inner
            .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
            .filter(|part| !part.is_empty())
            .collect();
        match parts.as_slice() {
            [lat, lon] => Self::new(lat.parse().ok()?, lon.parse().ok()?),
            _ => None,
        }
    }

    pub fn display(&self) -> String {
        format!("{:.6}, {:.6}", self.lat, self.lon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_coordinate_formats() {
        let expected = GeoPoint::new(38.7223, -9.1393);
        assert_eq!(GeoPoint::parse("38.7223, -9.1393"), expected);
        assert_eq!(GeoPoint::parse("  38.7223 -9.1393 "), expected);
        assert_eq!(GeoPoint::parse("(38.7223;-9.1393)"), expected);
    }

    #[test]
    fn rejects_malformed_or_out_of_range_text() {
        assert_eq!(GeoPoint::parse(""), None);
        assert_eq!(GeoPoint::parse("38.7"), None);
        assert_eq!(GeoPoint::parse("Lisbon, Portugal"), None);
        assert_eq!(GeoPoint::parse("91, 0"), None);
        assert_eq!(GeoPoint::parse("0, 181"), None);
        assert_eq!(GeoPoint::parse("1, 2, 3"), None);
        assert_eq!(GeoPoint::parse("NaN, 0"), None);
    }

    #[test]
    fn display_round_trips_through_parse() {
        let point = GeoPoint::new(-33.865143, 151.2099).unwrap();
        assert_eq!(point.display(), "-33.865143, 151.209900");
        assert_eq!(GeoPoint::parse(&point.display()), Some(point));
    }
}
