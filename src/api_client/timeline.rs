//! Photos timeline: month counts, and one month's photos at a time.

use chrono::{Duration, Months, NaiveDate};

use super::{ImmichApiClient, LibraryAsset, MetadataSearchFilters, SortOrder};

const MONTH_PAGE_SIZE: u32 = 1000;
const MAX_MONTH_PAGES: u32 = 200;
const ZONE_SLACK_HOURS: i64 = 14;

/// One month of the timeline, from `GET /api/timeline/buckets`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct TimeBucket {
    #[serde(rename = "timeBucket")]
    pub month: String,
    pub count: u32,
}

#[derive(serde::Deserialize)]
struct BucketIds {
    id: Vec<String>,
}

impl ImmichApiClient {
    pub async fn fetch_time_buckets(
        &self,
        user_id: Option<&str>,
    ) -> Result<Vec<TimeBucket>, String> {
        let path = format!("/api/timeline/buckets?{}", timeline_query(user_id, None));
        self.send_json(reqwest::Method::GET, &path, None)
            .await?
            .json()
            .await
            .map_err(|err| err.to_string())
    }

    pub async fn fetch_user_month_assets(
        &self,
        user_id: &str,
        month: &str,
    ) -> Result<Vec<LibraryAsset>, String> {
        let path = format!(
            "/api/timeline/bucket?{}",
            timeline_query(Some(user_id), Some(month))
        );
        let bucket: BucketIds = self
            .send_json(reqwest::Method::GET, &path, None)
            .await?
            .json()
            .await
            .map_err(|err| err.to_string())?;
        let assets = self.fetch_assets_by_ids(&bucket.id).await?;
        Ok(keep_local_month(assets, month))
    }

    pub async fn fetch_user_month_page(
        &self,
        user_id: &str,
        page: u32,
    ) -> Result<(Vec<LibraryAsset>, bool), String> {
        let buckets = self.fetch_time_buckets(Some(user_id)).await?;
        let index = page.saturating_sub(1) as usize;
        let Some(bucket) = buckets.get(index) else {
            return Ok((Vec::new(), false));
        };
        let assets = self.fetch_user_month_assets(user_id, &bucket.month).await?;
        Ok((assets, index + 1 < buckets.len()))
    }

    /// Photos taken in `month` (local time), newest first.
    pub async fn fetch_month_assets(&self, month: &str) -> Result<Vec<LibraryAsset>, String> {
        let (taken_after, taken_before) =
            month_query_range(month).ok_or_else(|| format!("Invalid month {month}"))?;
        let filters = MetadataSearchFilters {
            visibility: Some("timeline".to_string()),
            order: Some(SortOrder::Desc),
            taken_after: Some(taken_after),
            taken_before: Some(taken_before),
            ..MetadataSearchFilters::default()
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
        Ok(keep_local_month(assets, month))
    }
}

fn timeline_query(user_id: Option<&str>, month: Option<&str>) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("visibility", "timeline")
        .append_pair("order", "desc");
    match user_id {
        Some(user_id) => query.append_pair("userId", user_id),
        None => query.append_pair("withPartners", "true"),
    };
    if let Some(month) = month {
        query.append_pair("timeBucket", month);
    }
    query.finish()
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

pub(crate) fn keep_local_month(assets: Vec<LibraryAsset>, month: &str) -> Vec<LibraryAsset> {
    let Some(prefix) = month.get(..7) else {
        return Vec::new();
    };
    let mut kept: Vec<LibraryAsset> = assets
        .into_iter()
        .filter(|asset| local_time(asset).get(..7) == Some(prefix))
        .collect();
    kept.sort_by(|a, b| local_time(b).cmp(local_time(a)));
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
            timeline_query(None, None),
            "visibility=timeline&order=desc&withPartners=true"
        );
        assert_eq!(
            timeline_query(Some("u2"), Some("2024-01-01")),
            "visibility=timeline&order=desc&userId=u2&timeBucket=2024-01-01"
        );
    }

    #[test]
    fn bucket_reads_only_the_ids() {
        let bucket: BucketIds =
            serde_json::from_str(r#"{"id":["a","b"],"isImage":[true,false]}"#).unwrap();
        assert_eq!(bucket.id, ["a", "b"]);
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
    fn only_the_local_month_is_kept_newest_first() {
        let kept = keep_local_month(
            vec![
                asset("feb", Some("2026-02-28T22:00:00.000Z")),
                asset("early", Some("2026-03-02T08:00:00.000Z")),
                asset("apr", Some("2026-04-01T00:10:00.000Z")),
                asset("late", Some("2026-03-31T23:30:00.000Z")),
            ],
            "2026-03-01",
        );
        let ids: Vec<&str> = kept.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["late", "early"]);
    }

    #[test]
    fn assets_without_a_local_time_fall_back_to_the_file_date() {
        let kept = keep_local_month(vec![asset("plain", None)], "2026-03-01");
        assert_eq!(kept.len(), 1);
    }
}
