//! What a timeline shows: months Immich counts in its buckets, or pages of a counted total.

use std::rc::Rc;

use super::index::Month;
use crate::api_client::{
    BucketScope, ImmichApiClient, LibraryAsset, MetadataSearchFilters, SortOrder,
};
use crate::library::source_loading::{fetch_source_page, remote_filters};
use crate::library::state::{LibrarySortMode, LibrarySource};
use crate::library::{LibraryWindowUi, MAP_AREA_PAGE_SIZE, PAGE_SIZE};

/// How a timeline counts and fetches its sections.
pub(in crate::library) enum Plan {
    /// Months from `/timeline/buckets`, each found by searching `filters` over its dates,
    /// or without filters (a partner's photos) by the IDs its bucket lists.
    Months {
        scope: BucketScope,
        filters: Option<Box<MetadataSearchFilters>>,
    },
    /// Pages of `size`, from a total counted up front, for photos with no month buckets.
    Pages {
        source: LibrarySource,
        order: SortOrder,
        size: u32,
    },
}

impl Plan {
    /// Photos and a partner's photos fold stacks and keep the month scrubber.
    pub(super) fn is_photos(&self) -> bool {
        matches!(self, Plan::Months { scope, .. } if scope.with_stacked)
    }

    /// Months get date headings; pages don't.
    pub(super) fn dated(&self) -> bool {
        matches!(self, Plan::Months { .. })
    }
}

/// The timeline for `source` in `sort`; `None` pages through it as it scrolls.
pub(super) fn plan(source: &LibrarySource, sort: &LibrarySortMode) -> Option<Plan> {
    let order = sort.server_order()?;
    if let Some(scope) = bucket_scope(source, order) {
        let filters = month_filters(source).map(Box::new);
        return Some(Plan::Months { scope, filters });
    }
    let counted = matches!(
        source,
        LibrarySource::MissingLocation { .. }
            | LibrarySource::UnusualDates { .. }
            | LibrarySource::Place { .. }
            | LibrarySource::MapArea { .. }
    );
    counted.then(|| Plan::Pages {
        source: source.clone(),
        order,
        size: page_size(source),
    })
}

/// Photos keeps its newest-first scrubber; other timelines run either way.
fn bucket_scope(source: &LibrarySource, order: SortOrder) -> Option<BucketScope> {
    let newest = order == SortOrder::Desc;
    match source {
        LibrarySource::AllAssets => newest.then(BucketScope::photos),
        LibrarySource::Partner { id, .. } => newest.then(|| BucketScope::partner(id)),
        other => id_scope(other)
            .or_else(|| flag_scope(other))
            .map(|scope| BucketScope { order, ..scope }),
    }
}

fn id_scope(source: &LibrarySource) -> Option<BucketScope> {
    let all = BucketScope::all(SortOrder::Desc);
    let scope = match source {
        LibrarySource::Album { id, .. } => BucketScope {
            album_id: Some(id.clone()),
            ..all
        },
        LibrarySource::Person { id, .. } => BucketScope {
            person_id: Some(id.clone()),
            ..all
        },
        LibrarySource::Tag { id, .. } => BucketScope {
            tag_id: Some(id.clone()),
            ..all
        },
        _ => return None,
    };
    Some(scope)
}

fn flag_scope(source: &LibrarySource) -> Option<BucketScope> {
    let all = BucketScope::all(SortOrder::Desc);
    let scope = match source {
        LibrarySource::Favorites => BucketScope {
            is_favorite: true,
            ..all
        },
        LibrarySource::Archive => BucketScope {
            visibility: Some("archive"),
            ..all
        },
        LibrarySource::Trash => BucketScope {
            is_trashed: true,
            ..all
        },
        _ => return None,
    };
    Some(scope)
}

/// The search that finds one month's photos; a partner's come from their bucket instead.
fn month_filters(source: &LibrarySource) -> Option<MetadataSearchFilters> {
    match source {
        LibrarySource::Partner { .. } => None,
        LibrarySource::Album { id, .. } => Some(MetadataSearchFilters {
            album_ids: Some(vec![id.clone()]),
            ..MetadataSearchFilters::default()
        }),
        other => remote_filters(other, None),
    }
}

fn page_size(source: &LibrarySource) -> u32 {
    match source {
        LibrarySource::MapArea { .. } => MAP_AREA_PAGE_SIZE,
        _ => PAGE_SIZE,
    }
}

/// Every section with its photo count, so the grid knows its full height up front.
pub(super) async fn sections(api: &ImmichApiClient, plan: &Plan) -> Result<Vec<Month>, String> {
    match plan {
        Plan::Months { scope, .. } => {
            let buckets = api.fetch_time_buckets(scope).await?;
            let months = buckets.into_iter().map(|bucket| Month {
                key: bucket.month,
                count: bucket.count,
            });
            Ok(months.collect())
        }
        Plan::Pages { source, size, .. } => Ok(pages(count(api, source).await?, *size)),
    }
}

async fn count(api: &ImmichApiClient, source: &LibrarySource) -> Result<u64, String> {
    match source {
        LibrarySource::MapArea { ids, .. } => Ok(ids.0.len() as u64),
        LibrarySource::MissingLocation { range } => {
            let taken = crate::library::utilities::filters::taken_range(range);
            api.count_missing_location(&taken).await
        }
        other => {
            let filters = remote_filters(other, None).unwrap_or_default();
            api.search_statistics(&filters).await
        }
    }
}

/// `total` photos as pages of `size`, keyed by page number from 1.
fn pages(total: u64, size: u32) -> Vec<Month> {
    let total = u32::try_from(total).unwrap_or(u32::MAX);
    let size = size.max(1);
    (0..total.div_ceil(size))
        .map(|page| Month {
            key: (page + 1).to_string(),
            count: size.min(total - page * size),
        })
        .collect()
}

/// The photos in section `section`, keyed `key`.
pub(super) async fn fetch(
    ui: &Rc<LibraryWindowUi>,
    plan: &Plan,
    section: usize,
    key: &str,
) -> Result<Vec<LibraryAsset>, String> {
    let api = &ui.ctx.api_client;
    match plan {
        Plan::Months {
            scope,
            filters: Some(filters),
        } => api.fetch_month_assets(scope, filters, key).await,
        Plan::Months {
            scope,
            filters: None,
        } => api.fetch_bucket_month_assets(scope, key).await,
        Plan::Pages { source, order, .. } => {
            let page = u32::try_from(section + 1).unwrap_or(u32::MAX);
            let (assets, _) = fetch_source_page(ui, source.clone(), page, Some(*order)).await?;
            Ok(assets)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album() -> LibrarySource {
        LibrarySource::Album {
            id: "a1".into(),
            name: "Trip".into(),
        }
    }

    #[test]
    fn albums_and_collections_get_a_timeline_either_way_round() {
        for sort in [LibrarySortMode::NewestFirst, LibrarySortMode::OldestFirst] {
            assert!(plan(&album(), &sort).is_some_and(|plan| plan.dated()));
            assert!(plan(&LibrarySource::Trash, &sort).is_some());
        }
        assert!(plan(&album(), &LibrarySortMode::Filename).is_none());
    }

    #[test]
    fn only_newest_first_photos_get_the_scrubber() {
        let photos = plan(&LibrarySource::AllAssets, &LibrarySortMode::NewestFirst);
        assert!(photos.is_some_and(|plan| plan.is_photos()));
        assert!(plan(&LibrarySource::AllAssets, &LibrarySortMode::OldestFirst).is_none());
        let favorites = plan(&LibrarySource::Favorites, &LibrarySortMode::NewestFirst);
        assert!(favorites.is_some_and(|plan| !plan.is_photos()));
    }

    #[test]
    fn missing_location_pages_through_a_counted_total() {
        let missing = LibrarySource::MissingLocation {
            range: Default::default(),
        };
        let missing = plan(&missing, &LibrarySortMode::NewestFirst);
        assert!(missing.is_some_and(|plan| !plan.dated()));
        let smart = LibrarySource::SmartSearch {
            query: "dog".into(),
        };
        assert!(plan(&smart, &LibrarySortMode::NewestFirst).is_none());
    }

    #[test]
    fn a_total_splits_into_full_pages_and_a_last_partial_one() {
        let counts: Vec<u32> = pages(120, 50).iter().map(|page| page.count).collect();
        assert_eq!(counts, [50, 50, 20]);
        assert_eq!(pages(120, 50)[2].key, "3");
        assert!(pages(0, 50).is_empty());
    }
}
