//! Bucket selection policy for the masonry grid.
//!
//! Maps a `GridQuality` choice (auto / explicit) and a row's rendered height
//! to a concrete `ThumbnailSize`. `fallback_bucket` defines the degradation
//! chain used when a server lacks higher buckets (e.g. no fullsize).

use crate::api_client::ThumbnailSize;

pub(crate) const PREVIEW_BUCKET_THRESHOLD: f32 = 600.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridQuality {
    Auto,
    #[default]
    Thumbnail,
    Preview,
    Fullsize,
}

impl GridQuality {
    pub fn parse(s: &str) -> Self {
        match s {
            "thumbnail" => Self::Thumbnail,
            "preview" => Self::Preview,
            "fullsize" => Self::Fullsize,
            _ => Self::Auto,
        }
    }
}

pub(crate) fn bucket_for_row_height(h: f32, quality: GridQuality) -> ThumbnailSize {
    match quality {
        GridQuality::Thumbnail => ThumbnailSize::Thumbnail,
        GridQuality::Preview => ThumbnailSize::Preview,
        GridQuality::Fullsize => ThumbnailSize::Fullsize,
        GridQuality::Auto => {
            if h <= PREVIEW_BUCKET_THRESHOLD {
                ThumbnailSize::Thumbnail
            } else {
                ThumbnailSize::Preview
            }
        }
    }
}

const FIT_EDGES: [u32; 6] = [320, 480, 640, 960, 1280, 1920];

/// Smallest decode edge covering a tile `tile_px` wide or tall; `None` past the largest step.
pub(crate) fn fit_edge(tile_px: f32) -> Option<u32> {
    FIT_EDGES
        .iter()
        .copied()
        .find(|&edge| edge as f32 >= tile_px)
}

/// Edges a fitted tile may already be cached at: its own, the neighbouring steps, then the bucket.
pub(crate) fn fit_candidates(fit: Option<u32>) -> impl Iterator<Item = Option<u32>> {
    fit.and_then(|edge| FIT_EDGES.iter().position(|&step| step == edge))
        .into_iter()
        .flat_map(|i| [Some(i), i.checked_sub(1), Some(i + 1)])
        .flatten()
        .filter_map(|i| FIT_EDGES.get(i).copied())
        .map(Some)
        .chain(std::iter::once(None))
}

/// Smaller size to try when a requested bucket isn't available on the server.
pub(crate) fn fallback_bucket(size: ThumbnailSize) -> Option<ThumbnailSize> {
    match size {
        ThumbnailSize::Fullsize => Some(ThumbnailSize::Preview),
        ThumbnailSize::Preview | ThumbnailSize::Thumbnail => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_thumbnail_under_threshold() {
        assert!(matches!(
            bucket_for_row_height(200.0, GridQuality::Auto),
            ThumbnailSize::Thumbnail
        ));
        assert!(matches!(
            bucket_for_row_height(PREVIEW_BUCKET_THRESHOLD, GridQuality::Auto),
            ThumbnailSize::Thumbnail
        ));
        assert!(matches!(
            bucket_for_row_height(PREVIEW_BUCKET_THRESHOLD + 1.0, GridQuality::Auto),
            ThumbnailSize::Preview
        ));
    }

    #[test]
    fn explicit_quality_overrides_row_height() {
        assert!(matches!(
            bucket_for_row_height(2000.0, GridQuality::Thumbnail),
            ThumbnailSize::Thumbnail
        ));
        assert!(matches!(
            bucket_for_row_height(50.0, GridQuality::Fullsize),
            ThumbnailSize::Fullsize
        ));
    }

    #[test]
    fn fit_edge_rounds_up_to_the_next_step() {
        assert_eq!(fit_edge(200.0), Some(320));
        assert_eq!(fit_edge(320.0), Some(320));
        assert_eq!(fit_edge(321.0), Some(480));
        assert_eq!(fit_edge(1900.0), Some(1920));
        assert_eq!(fit_edge(2000.0), None);
    }

    #[test]
    fn fit_candidates_try_the_edge_its_neighbours_then_the_bucket() {
        let all = |fit| fit_candidates(fit).collect::<Vec<_>>();
        assert_eq!(all(Some(480)), [Some(480), Some(320), Some(640), None]);
        assert_eq!(all(Some(320)), [Some(320), Some(480), None]);
        assert_eq!(all(Some(1920)), [Some(1920), Some(1280), None]);
        assert_eq!(all(None), [None]);
    }

    #[test]
    fn fallback_chain_degrades_fullsize_to_preview() {
        assert_eq!(
            fallback_bucket(ThumbnailSize::Fullsize),
            Some(ThumbnailSize::Preview)
        );
        assert_eq!(fallback_bucket(ThumbnailSize::Preview), None);
        assert_eq!(fallback_bucket(ThumbnailSize::Thumbnail), None);
    }
}
