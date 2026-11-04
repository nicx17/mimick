use super::{ImmichApiClient, UserSummary};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityKind {
    Comment,
    Like,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Activity {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: ActivityKind,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(rename = "assetId", default)]
    pub asset_id: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub user: UserSummary,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
pub struct ActivityStatistics {
    pub comments: u64,
    pub likes: u64,
}

fn activity_query(album_id: &str, asset_id: Option<&str>) -> String {
    match asset_id {
        Some(asset_id) => format!("albumId={album_id}&assetId={asset_id}"),
        None => format!("albumId={album_id}"),
    }
}

#[derive(Debug, serde::Serialize)]
pub struct NewActivity<'a> {
    #[serde(rename = "albumId")]
    pub album_id: &'a str,
    #[serde(rename = "assetId", skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<&'a str>,
    #[serde(rename = "type")]
    pub kind: ActivityKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<&'a str>,
}

impl ImmichApiClient {
    pub async fn fetch_activities(
        &self,
        album_id: &str,
        asset_id: Option<&str>,
    ) -> Result<Vec<Activity>, String> {
        let path = format!("/api/activities?{}", activity_query(album_id, asset_id));
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn activity_statistics(
        &self,
        album_id: &str,
        asset_id: Option<&str>,
    ) -> Result<ActivityStatistics, String> {
        let path = format!(
            "/api/activities/statistics?{}",
            activity_query(album_id, asset_id)
        );
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn add_activity(&self, activity: &NewActivity<'_>) -> Result<Activity, String> {
        let body = serde_json::to_value(activity).map_err(|err| err.to_string())?;
        let resp = self
            .send_json(reqwest::Method::POST, "/api/activities", Some(body))
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn delete_activity(&self, activity_id: &str) -> Result<(), String> {
        let path = format!("/api/activities/{activity_id}");
        self.send_json(reqwest::Method::DELETE, &path, None)
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_adds_the_asset_only_when_given() {
        assert_eq!(activity_query("al", None), "albumId=al");
        assert_eq!(activity_query("al", Some("as")), "albumId=al&assetId=as");
    }

    #[test]
    fn new_activity_leaves_out_missing_fields() {
        let like = NewActivity {
            album_id: "al",
            asset_id: None,
            kind: ActivityKind::Like,
            comment: None,
        };
        assert_eq!(
            serde_json::to_value(&like).unwrap(),
            serde_json::json!({ "albumId": "al", "type": "like" })
        );
        let comment = NewActivity {
            asset_id: Some("as"),
            kind: ActivityKind::Comment,
            comment: Some("Nice"),
            ..like
        };
        assert_eq!(
            serde_json::to_value(&comment).unwrap(),
            serde_json::json!({
                "albumId": "al",
                "assetId": "as",
                "type": "comment",
                "comment": "Nice"
            })
        );
    }

    #[test]
    fn activity_parses_immich_response() {
        let json = serde_json::json!({
            "id": "a1",
            "type": "comment",
            "comment": "Great shot",
            "assetId": null,
            "createdAt": "2024-01-01T00:00:00.000Z",
            "user": {
                "id": "u1",
                "name": "Ana",
                "email": "ana@example.com",
                "avatarColor": "primary",
                "profileImagePath": "",
                "profileChangedAt": "2024-01-01T00:00:00.000Z"
            }
        });
        let activity: Activity = serde_json::from_value(json).unwrap();
        assert_eq!(activity.kind, ActivityKind::Comment);
        assert_eq!(activity.comment.as_deref(), Some("Great shot"));
        assert_eq!(activity.asset_id, None);
        assert_eq!(activity.user.name, "Ana");
    }
}
