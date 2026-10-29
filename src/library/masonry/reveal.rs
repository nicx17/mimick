//! Fade and un-blur for grid thumbnails as they arrive.

pub const REVEAL_DURATION_US: i64 = 220_000;
pub const REVEAL_MAX_BLUR: f32 = 10.0;

/// Eased opacity for a reveal that started `elapsed_us` ago, or `None` once it has finished.
pub fn reveal_progress(elapsed_us: i64) -> Option<f32> {
    if elapsed_us >= REVEAL_DURATION_US {
        return None;
    }
    let t = elapsed_us.max(0) as f32 / REVEAL_DURATION_US as f32;
    Some(1.0 - (1.0 - t).powi(3))
}

pub fn animations_enabled() -> bool {
    gtk::Settings::default().is_some_and(|settings| settings.is_gtk_enable_animations())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_starts_clear_and_finishes() {
        assert_eq!(reveal_progress(0), Some(0.0));
        assert_eq!(reveal_progress(REVEAL_DURATION_US), None);
        assert_eq!(reveal_progress(-5), Some(0.0));
    }

    #[test]
    fn reveal_eases_out_monotonically() {
        let half = reveal_progress(REVEAL_DURATION_US / 2).unwrap();
        assert!(half > 0.5 && half < 1.0);
        let steps: Vec<f32> = (0..10)
            .map(|i| reveal_progress(i * REVEAL_DURATION_US / 10).unwrap())
            .collect();
        assert!(steps.windows(2).all(|w| w[0] <= w[1]));
    }
}
