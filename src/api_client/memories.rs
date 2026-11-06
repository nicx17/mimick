//! Memories ("On this day" and birthdays) that Immich builds each night.

use chrono::{DateTime, FixedOffset};

use super::{ImmichApiClient, LibraryAsset};

/// One memory and its photos, from `GET /api/memories`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: String,
    /// `on_this_day` or `birthday`.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub data: MemoryData,
    #[serde(default)]
    pub assets: Vec<LibraryAsset>,
    #[serde(default)]
    pub seen_at: Option<String>,
    pub memory_at: String,
}

/// `year` is when the photos were taken; birthdays (v3.3+) also name the person.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryData {
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub person_name: Option<String>,
}

/// `for` as v3.1+ wants it (a plain date), then as v3.0 wanted it (a timestamp).
fn for_values(now: &DateTime<FixedOffset>) -> [String; 2] {
    [now.format("%Y-%m-%d").to_string(), now.to_rfc3339()]
}

impl ImmichApiClient {
    /// Memories showing at `now`, the ones Immich web lists above the timeline.
    pub async fn fetch_memories(&self, now: &DateTime<FixedOffset>) -> Result<Vec<Memory>, String> {
        let [date, timestamp] = for_values(now);
        // Each version's validator rejects the other's format with a 400.
        let resp = match self.get_memories(&date).await {
            Err(err) if err.starts_with("HTTP 400") => self.get_memories(&timestamp).await,
            other => other,
        }?;
        let memories: Vec<Memory> = resp.json().await.map_err(|err| err.to_string())?;
        Ok(memories
            .into_iter()
            .filter(|memory| !memory.assets.is_empty())
            .collect())
    }

    async fn get_memories(&self, day: &str) -> Result<reqwest::Response, String> {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("for", day)
            .finish();
        self.send_json(
            reqwest::Method::GET,
            &format!("/api/memories?{query}"),
            None,
        )
        .await
    }

    pub async fn fetch_memory(&self, id: &str) -> Result<Memory, String> {
        let resp = self
            .send_json(reqwest::Method::GET, &format!("/api/memories/{id}"), None)
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn mark_memory_seen(&self, id: &str, at: &str) -> Result<(), String> {
        let body = serde_json::json!({ "seenAt": at });
        self.send_json(
            reqwest::Method::PUT,
            &format!("/api/memories/{id}"),
            Some(body),
        )
        .await
        .map(|_| ())
    }

    pub async fn remove_memory_assets(&self, id: &str, asset_ids: &[String]) -> Result<(), String> {
        let body = serde_json::json!({ "ids": asset_ids });
        self.send_json(
            reqwest::Method::DELETE,
            &format!("/api/memories/{id}/assets"),
            Some(body),
        )
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn asks_for_today_as_a_date_then_as_a_v3_0_timestamp() {
        let now = chrono::FixedOffset::east_opt(5 * 3600 + 1800)
            .unwrap()
            .with_ymd_and_hms(2026, 11, 6, 23, 52, 0)
            .unwrap();
        assert_eq!(
            for_values(&now),
            [
                "2026-11-06".to_string(),
                "2026-11-06T23:52:00+05:30".to_string()
            ]
        );
    }

    #[test]
    fn memories_read_year_person_and_seen_state() {
        let memory: Memory = serde_json::from_value(serde_json::json!({
            "id": "m1",
            "type": "birthday",
            "data": { "year": 2021, "personName": "Ana", "personId": "p1" },
            "assets": [],
            "seenAt": "2026-11-06T08:00:00.000Z",
            "memoryAt": "2021-11-06T00:00:00.000Z",
            "isSaved": false,
            "ownerId": "u1",
            "createdAt": "2026-11-06T00:00:00.000Z",
            "updatedAt": "2026-11-06T00:00:00.000Z"
        }))
        .unwrap();
        assert_eq!(memory.kind, "birthday");
        assert_eq!(memory.data.year, Some(2021));
        assert_eq!(memory.data.person_name.as_deref(), Some("Ana"));
        assert!(memory.seen_at.is_some());
    }
}
