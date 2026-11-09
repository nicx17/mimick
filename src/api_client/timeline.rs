//! Photos timeline: month counts, and one month's photos at a time.

use crate::i18n::fill;
use chrono::{Duration, Months, NaiveDate};
use gettextrs::gettext;

use std::collections::HashMap;

use super::{AssetStack, ImmichApiClient, LibraryAsset, MetadataSearchFilters, SortOrder};

const MONTH_PAGE_SIZE: u32 = 1000;
const MAX_MONTH_PAGES: u32 = 200;
const ZONE_SLACK_HOURS: i64 = 14;

/// Which photos a timeline counts and lists: `/timeline/buckets` filters, and the order.
#[derive(Clone, Debug, PartialEq)]
pub struct BucketScope {
    pub visibility: Option<&'static str>,
    pub order: SortOrder,
    /// Lists only each stack's cover, as Photos does.
    pub with_stacked: bool,
    pub user_id: Option<String>,
    pub with_partners: bool,
    pub album_id: Option<String>,
    pub person_id: Option<String>,
    pub tag_id: Option<String>,
    pub is_favorite: bool,
    pub is_trashed: bool,
}

impl BucketScope {
    /// Everything with no filter, in `order`; fill in the filter a source needs.
    pub fn all(order: SortOrder) -> Self {
        Self {
            visibility: None,
            order,
            with_stacked: false,
            user_id: None,
            with_partners: false,
            album_id: None,
            person_id: None,
            tag_id: None,
            is_favorite: false,
            is_trashed: false,
        }
    }

    /// Photos: the user's timeline and their partners', stacks folded.
    pub fn photos() -> Self {
        Self {
            visibility: Some("timeline"),
            with_stacked: true,
            with_partners: true,
            ..Self::all(SortOrder::Desc)
        }
    }

    /// One partner's timeline, stacks folded.
    pub fn partner(user_id: &str) -> Self {
        Self {
            visibility: Some("timeline"),
            with_stacked: true,
            user_id: Some(user_id.to_string()),
            ..Self::all(SortOrder::Desc)
        }
    }

    /// The query string, for `month` alone when given; unset filters are left out.
    fn query(&self, month: Option<&str>) -> String {
        let order = match self.order {
            SortOrder::Asc => "asc",
            SortOrder::Desc => "desc",
        };
        let flag = |on: bool| on.then(|| "true".to_string());
        let pairs = [
            ("visibility", self.visibility.map(str::to_string)),
            ("order", Some(order.to_string())),
            ("withStacked", flag(self.with_stacked)),
            ("userId", self.user_id.clone()),
            ("withPartners", flag(self.with_partners)),
            ("albumId", self.album_id.clone()),
            ("personId", self.person_id.clone()),
            ("tagId", self.tag_id.clone()),
            ("isFavorite", flag(self.is_favorite)),
            ("isTrashed", flag(self.is_trashed)),
            ("timeBucket", month.map(str::to_string)),
        ];
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in pairs {
            if let Some(value) = value {
                query.append_pair(key, &value);
            }
        }
        query.finish()
    }
}

/// One month of the timeline, from `GET /api/timeline/buckets`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct TimeBucket {
    #[serde(rename = "timeBucket")]
    pub month: String,
    pub count: u32,
}

/// A month's photos as columns; with `withStacked` only stack covers are listed.
#[derive(serde::Deserialize)]
struct BucketIds {
    id: Vec<String>,
    /// Per photo, `[stackId, assetCount]` for a stack's cover, else null.
    #[serde(default)]
    stack: Vec<Option<Vec<String>>>,
}

impl BucketIds {
    /// Keeps the photos this bucket lists (dropping hidden stack members) and marks stack covers.
    fn apply_stacks(&self, assets: Vec<LibraryAsset>) -> Vec<LibraryAsset> {
        let stacks: HashMap<&str, Option<(&str, u32)>> = self
            .id
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), self.stack.get(i).and_then(stack_entry)))
            .collect();
        assets
            .into_iter()
            .filter_map(|mut asset| {
                let entry = *stacks.get(asset.id.as_str())?;
                if let Some((id, count)) = entry {
                    asset.stack = Some(AssetStack {
                        id: id.to_string(),
                        primary_asset_id: asset.id.clone(),
                        asset_count: count,
                    });
                }
                Some(asset)
            })
            .collect()
    }
}

fn stack_entry(entry: &Option<Vec<String>>) -> Option<(&str, u32)> {
    match entry.as_deref()? {
        [id, count] => Some((id.as_str(), count.parse().ok()?)),
        _ => None,
    }
}

impl ImmichApiClient {
    pub async fn fetch_time_buckets(&self, scope: &BucketScope) -> Result<Vec<TimeBucket>, String> {
        let path = format!("/api/timeline/buckets?{}", scope.query(None));
        self.send_json(reqwest::Method::GET, &path, None)
            .await?
            .json()
            .await
            .map_err(|err| err.to_string())
    }

    /// The photos `scope`'s bucket lists for `month`, fetched one by one, as for a partner.
    pub async fn fetch_bucket_month_assets(
        &self,
        scope: &BucketScope,
        month: &str,
    ) -> Result<Vec<LibraryAsset>, String> {
        let bucket = self.fetch_bucket(scope, month).await?;
        let assets = self.fetch_assets_by_ids(&bucket.id).await?;
        Ok(keep_local_month(assets, month, scope.order))
    }

    pub async fn fetch_user_month_page(
        &self,
        user_id: &str,
        page: u32,
    ) -> Result<(Vec<LibraryAsset>, bool), String> {
        let scope = BucketScope::partner(user_id);
        let buckets = self.fetch_time_buckets(&scope).await?;
        let index = page.saturating_sub(1) as usize;
        let Some(bucket) = buckets.get(index) else {
            return Ok((Vec::new(), false));
        };
        let assets = self
            .fetch_bucket_month_assets(&scope, &bucket.month)
            .await?;
        Ok((assets, index + 1 < buckets.len()))
    }

    /// The photos `filters` match that were taken in `month` (local time), in `scope`'s order.
    pub async fn fetch_month_assets(
        &self,
        scope: &BucketScope,
        filters: &MetadataSearchFilters,
        month: &str,
    ) -> Result<Vec<LibraryAsset>, String> {
        let (taken_after, taken_before) = month_query_range(month)
            .ok_or_else(|| fill(gettext("Invalid month {month}"), &[("month", month)]))?;
        let filters = MetadataSearchFilters {
            order: Some(scope.order),
            taken_after: Some(taken_after),
            taken_before: Some(taken_before),
            ..filters.clone()
        };
        let mut assets = Vec::new();
        for page in 1..=MAX_MONTH_PAGES {
            let (items, has_more) = self
                .search_metadata_with_filters(&filters, page, MONTH_PAGE_SIZE)
                .await?;
            assets.extend(items);
            if !has_more {
                break;
            }
        }
        if scope.with_stacked {
            assets = self.fold_stacks(scope, month, assets).await;
        }
        Ok(keep_local_month(assets, month, scope.order))
    }

    /// Search can't hide stack members, so the bucket says which photos the timeline shows.
    async fn fold_stacks(
        &self,
        scope: &BucketScope,
        month: &str,
        assets: Vec<LibraryAsset>,
    ) -> Vec<LibraryAsset> {
        match self.fetch_bucket(scope, month).await {
            Ok(bucket) => bucket.apply_stacks(assets),
            Err(err) => {
                log::debug!("Stacks unavailable for {month}: {err}");
                assets
            }
        }
    }

    async fn fetch_bucket(&self, scope: &BucketScope, month: &str) -> Result<BucketIds, String> {
        let path = format!("/api/timeline/bucket?{}", scope.query(Some(month)));
        self.send_json(reqwest::Method::GET, &path, None)
            .await?
            .json()
            .await
            .map_err(|err| err.to_string())
    }
}

/// UTC search range covering every timezone's version of `month`.
pub(crate) fn month_query_range(month: &str) -> Option<(String, String)> {
    let start = NaiveDate::parse_from_str(month, "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)?;
    let end = start.checked_add_months(Months::new(1))?;
    let slack = Duration::hours(ZONE_SLACK_HOURS);
    let format = |at: chrono::NaiveDateTime| at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    Some((format(start - slack), format(end + slack)))
}

pub(crate) fn keep_local_month(
    assets: Vec<LibraryAsset>,
    month: &str,
    order: SortOrder,
) -> Vec<LibraryAsset> {
    let Some(prefix) = month.get(..7) else {
        return Vec::new();
    };
    let mut kept: Vec<LibraryAsset> = assets
        .into_iter()
        .filter(|asset| local_time(asset).get(..7) == Some(prefix))
        .collect();
    kept.sort_by(|a, b| local_time(b).cmp(local_time(a)));
    if order == SortOrder::Asc {
        kept.reverse();
    }
    kept
}

fn local_time(asset: &LibraryAsset) -> &str {
    asset
        .local_date_time
        .as_deref()
        .unwrap_or(&asset.created_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: &str, local: Option<&str>) -> LibraryAsset {
        LibraryAsset {
            id: id.into(),
            filename: format!("{id}.jpg"),
            mime_type: "image/jpeg".into(),
            created_at: local.unwrap_or("2026-03-15T12:00:00.000Z").into(),
            asset_type: "IMAGE".into(),
            thumbhash: None,
            width: None,
            height: None,
            checksum: None,
            exif_info: None,
            is_favorite: false,
            visibility: String::new(),
            is_edited: false,
            owner_id: String::new(),
            local_date_time: local.map(str::to_string),
            live_photo_video_id: None,
            stack: None,
        }
    }

    #[test]
    fn buckets_read_month_and_count() {
        let buckets: Vec<TimeBucket> =
            serde_json::from_str(r#"[{"timeBucket":"2024-01-01","count":42}]"#).unwrap();
        assert_eq!(
            buckets,
            [TimeBucket {
                month: "2024-01-01".into(),
                count: 42
            }]
        );
    }

    #[test]
    fn photos_count_partners_and_a_partner_counts_only_their_own() {
        assert_eq!(
            BucketScope::photos().query(None),
            "visibility=timeline&order=desc&withStacked=true&withPartners=true"
        );
        assert_eq!(
            BucketScope::partner("u2").query(Some("2024-01-01")),
            "visibility=timeline&order=desc&withStacked=true&userId=u2&timeBucket=2024-01-01"
        );
    }

    #[test]
    fn other_timelines_send_only_their_own_filter() {
        let album = BucketScope {
            album_id: Some("a1".into()),
            ..BucketScope::all(SortOrder::Asc)
        };
        assert_eq!(album.query(None), "order=asc&albumId=a1");
        let trash = BucketScope {
            is_trashed: true,
            ..BucketScope::all(SortOrder::Desc)
        };
        assert_eq!(trash.query(None), "order=desc&isTrashed=true");
    }

    #[test]
    fn bucket_reads_only_the_ids() {
        let bucket: BucketIds =
            serde_json::from_str(r#"{"id":["a","b"],"isImage":[true,false]}"#).unwrap();
        assert_eq!(bucket.id, ["a", "b"]);
    }

    #[test]
    fn buckets_hide_stack_members_and_mark_covers() {
        let bucket: BucketIds =
            serde_json::from_str(r#"{"id":["cover","plain"],"stack":[["s1","3"],null]}"#).unwrap();
        let kept = bucket.apply_stacks(vec![
            asset("cover", None),
            asset("member", None),
            asset("plain", None),
        ]);
        let ids: Vec<&str> = kept.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["cover", "plain"]);
        let stack = kept[0].stack.as_ref().unwrap();
        assert_eq!((stack.id.as_str(), stack.asset_count), ("s1", 3));
        assert_eq!(stack.primary_asset_id, "cover");
        assert!(kept[1].stack.is_none());
    }

    #[test]
    fn month_query_widens_by_fourteen_hours() {
        assert_eq!(
            month_query_range("2026-03-01"),
            Some((
                "2026-02-28T10:00:00.000Z".into(),
                "2026-04-01T14:00:00.000Z".into()
            ))
        );
        assert_eq!(
            month_query_range("2026-12-01"),
            Some((
                "2026-11-30T10:00:00.000Z".into(),
                "2027-01-01T14:00:00.000Z".into()
            ))
        );
        assert_eq!(month_query_range("2026-13-01"), None);
    }

    #[test]
    fn only_the_local_month_is_kept_in_order() {
        let assets = || {
            vec![
                asset("feb", Some("2026-02-28T22:00:00.000Z")),
                asset("early", Some("2026-03-02T08:00:00.000Z")),
                asset("apr", Some("2026-04-01T00:10:00.000Z")),
                asset("late", Some("2026-03-31T23:30:00.000Z")),
            ]
        };
        let ids =
            |kept: Vec<LibraryAsset>| -> Vec<String> { kept.into_iter().map(|a| a.id).collect() };
        let newest = keep_local_month(assets(), "2026-03-01", SortOrder::Desc);
        assert_eq!(ids(newest), ["late", "early"]);
        let oldest = keep_local_month(assets(), "2026-03-01", SortOrder::Asc);
        assert_eq!(ids(oldest), ["early", "late"]);
    }

    #[test]
    fn assets_without_a_local_time_fall_back_to_the_file_date() {
        let kept = keep_local_month(vec![asset("plain", None)], "2026-03-01", SortOrder::Desc);
        assert_eq!(kept.len(), 1);
    }
}
