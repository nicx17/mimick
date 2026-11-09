//! How many photos each day has, from `GET /api/users/me/calendar-heatmap` (Immich v3).

use chrono::NaiveDate;

use super::ImmichApiClient;

#[derive(serde::Deserialize)]
struct HeatmapResponse {
    #[serde(default)]
    series: Vec<HeatmapDay>,
}

#[derive(serde::Deserialize)]
struct HeatmapDay {
    date: String,
    count: u32,
}

impl ImmichApiClient {
    /// One count per day from `from` to `to` (UTC days), by when photos were taken or uploaded.
    pub async fn fetch_calendar_heatmap(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        taken: bool,
    ) -> Result<Vec<(NaiveDate, u32)>, String> {
        let kind = if taken { "Taken" } else { "Upload" };
        let path = format!("/api/users/me/calendar-heatmap?from={from}&to={to}&type={kind}");
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        let body: HeatmapResponse = resp.json().await.map_err(|err| err.to_string())?;
        Ok(parse_days(body.series))
    }
}

fn parse_days(series: Vec<HeatmapDay>) -> Vec<(NaiveDate, u32)> {
    series
        .into_iter()
        .filter_map(|day| {
            let date = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d").ok()?;
            Some((date, day.count))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_parse_and_bad_dates_are_skipped() {
        let body: HeatmapResponse = serde_json::from_value(serde_json::json!({
            "from": "2026-01-01", "to": "2026-01-02", "totalCount": 3,
            "series": [
                { "date": "2026-01-01", "count": 3 },
                { "date": "nonsense", "count": 1 }
            ]
        }))
        .unwrap();
        let days = parse_days(body.series);
        assert_eq!(days, [(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), 3)]);
    }
}
