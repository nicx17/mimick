//! Immich's notifications inbox: album invites, failed backups, and server messages.

use super::ImmichApiClient;

/// One notification, from `GET /api/notifications`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    /// `success`, `error`, `warning`, or `info`.
    #[serde(default)]
    pub level: String,
    /// `AlbumInvite`, `BackupFailed`, `SystemMessage`, and so on.
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub read_at: Option<String>,
    /// For album invites and updates, `{ "albumId": … }`.
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

impl Notification {
    pub fn is_unread(&self) -> bool {
        self.read_at.is_none()
    }

    /// The album an invite or update is about.
    pub fn album_id(&self) -> Option<&str> {
        self.data.as_ref()?.get("albumId")?.as_str()
    }
}

impl ImmichApiClient {
    /// Newest first; `unread_only` leaves out the ones already read.
    pub async fn fetch_notifications(
        &self,
        unread_only: bool,
    ) -> Result<Vec<Notification>, String> {
        let path = if unread_only {
            "/api/notifications?unread=true"
        } else {
            "/api/notifications"
        };
        let resp = self.send_json(reqwest::Method::GET, path, None).await?;
        let mut list: Vec<Notification> = resp.json().await.map_err(|err| err.to_string())?;
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(list)
    }

    pub async fn mark_notifications_read(&self, ids: &[String]) -> Result<(), String> {
        let body = serde_json::json!({ "ids": ids, "readAt": chrono::Utc::now().to_rfc3339() });
        self.send_json(reqwest::Method::PUT, "/api/notifications", Some(body))
            .await
            .map(|_| ())
    }

    pub async fn delete_notifications(&self, ids: &[String]) -> Result<(), String> {
        let body = serde_json::json!({ "ids": ids });
        self.send_json(reqwest::Method::DELETE, "/api/notifications", Some(body))
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invites_name_their_album() {
        let invite: Notification = serde_json::from_value(serde_json::json!({
            "id": "n1",
            "title": "Ana shared “Trip” with you",
            "level": "info",
            "type": "AlbumInvite",
            "createdAt": "2026-11-01T10:00:00.000Z",
            "data": { "albumId": "a1" }
        }))
        .unwrap();
        assert!(invite.is_unread());
        assert_eq!(invite.album_id(), Some("a1"));
    }
}
