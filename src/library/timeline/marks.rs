use super::dates::month_year;
use super::index::Month;
use super::layout::LaidSection;

#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Year { y: f32, year: i32 },
    Month { y: f32 },
}

impl Mark {
    pub fn y(&self) -> f32 {
        match self {
            Mark::Year { y, .. } | Mark::Month { y } => *y,
        }
    }
}

/// Year labels and month dots on a track `track_h` tall, thinned to `min_gap` apart.
pub fn marks(
    months: &[Month],
    sections: &[LaidSection],
    canvas_h: f32,
    track_h: f32,
    min_gap: f32,
) -> Vec<Mark> {
    let mut kept: Vec<Mark> = Vec::new();
    let mut last_year = None;
    for (month, section) in months.iter().zip(sections) {
        let y = canvas_to_track(section.y, track_h, canvas_h);
        let year = month_year(&month.key);
        let mark = match year {
            Some(year) if last_year != Some(year) => Mark::Year { y, year },
            _ => Mark::Month { y },
        };
        last_year = year.or(last_year);
        match kept.last() {
            Some(previous) if y - previous.y() < min_gap => {
                if matches!(mark, Mark::Year { .. }) && matches!(previous, Mark::Month { .. }) {
                    kept.pop();
                    kept.push(mark);
                }
            }
            _ => kept.push(mark),
        }
    }
    kept
}

pub fn track_to_canvas(track_y: f32, track_h: f32, canvas_h: f32) -> f32 {
    if track_h <= 0.0 {
        return 0.0;
    }
    track_y / track_h * canvas_h
}

/// Top of a `handle_h` handle travelling the track as the view scrolls end to end.
pub fn handle_top(value: f64, upper: f64, page: f64, track_h: f32, handle_h: f32) -> f32 {
    let range = upper - page;
    if range <= 0.0 {
        return 0.0;
    }
    (value / range).clamp(0.0, 1.0) as f32 * (track_h - handle_h).max(0.0)
}

pub fn canvas_to_track(canvas_y: f32, track_h: f32, canvas_h: f32) -> f32 {
    if canvas_h <= 0.0 {
        return 0.0;
    }
    canvas_y / canvas_h * track_h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn month(key: &str) -> Month {
        Month {
            key: key.into(),
            count: 1,
        }
    }

    fn sec(y: f32) -> LaidSection {
        LaidSection {
            y,
            h: 100.0,
            loaded: false,
        }
    }

    #[test]
    fn years_label_their_newest_month() {
        let months = [
            month("2026-10-01"),
            month("2026-09-01"),
            month("2025-12-01"),
        ];
        let got = marks(
            &months,
            &[sec(0.0), sec(100.0), sec(200.0)],
            300.0,
            300.0,
            4.0,
        );
        assert_eq!(
            got,
            [
                Mark::Year { y: 0.0, year: 2026 },
                Mark::Month { y: 100.0 },
                Mark::Year {
                    y: 200.0,
                    year: 2025
                }
            ]
        );
    }

    #[test]
    fn crowded_marks_are_thinned_keeping_years() {
        let months = [
            month("2022-01-01"),
            month("2021-12-01"),
            month("2021-11-01"),
        ];
        let got = marks(
            &months,
            &[sec(0.0), sec(1.0), sec(2.0)],
            1000.0,
            1000.0,
            12.0,
        );
        assert_eq!(got, [Mark::Year { y: 0.0, year: 2022 }]);
    }

    #[test]
    fn a_year_close_after_a_month_replaces_it() {
        let months = [
            month("2022-03-01"),
            month("2022-02-01"),
            month("2021-12-01"),
        ];
        let got = marks(
            &months,
            &[sec(0.0), sec(50.0), sec(55.0)],
            1000.0,
            1000.0,
            12.0,
        );
        assert_eq!(
            got,
            [
                Mark::Year { y: 0.0, year: 2022 },
                Mark::Year {
                    y: 55.0,
                    year: 2021
                }
            ]
        );
    }

    #[test]
    fn handle_travels_the_whole_track() {
        assert_eq!(handle_top(0.0, 1100.0, 100.0, 544.0, 44.0), 0.0);
        assert_eq!(handle_top(1000.0, 1100.0, 100.0, 544.0, 44.0), 500.0);
        assert_eq!(handle_top(500.0, 1100.0, 100.0, 544.0, 44.0), 250.0);
        assert_eq!(handle_top(0.0, 100.0, 100.0, 544.0, 44.0), 0.0);
    }

    #[test]
    fn track_and_canvas_positions_round_trip() {
        assert_eq!(track_to_canvas(50.0, 200.0, 10_000.0), 2500.0);
        assert_eq!(canvas_to_track(2500.0, 200.0, 10_000.0), 50.0);
    }
}
