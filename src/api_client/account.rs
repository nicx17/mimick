//! Your Immich account: profile picture, signed-in devices, and API keys.

use super::ImmichApiClient;

/// The signed-in user, from `GET /api/users/me`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
    /// Empty when there's no profile picture.
    #[serde(default)]
    pub profile_image_path: String,
}

/// A browser or app signed in to the account, from `GET /api/sessions`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    #[serde(default, rename = "deviceOS")]
    pub device_os: String,
    #[serde(default)]
    pub device_type: String,
    #[serde(default)]
    pub app_version: Option<String>,
    #[serde(default)]
    pub updated_at: String,
}

/// An API key, without its secret, from `GET /api/api-keys`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKey {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(serde::Deserialize)]
struct RotatedKey {
    secret: String,
}

impl ImmichApiClient {
    pub async fn fetch_account(&self) -> Result<Account, String> {
        let resp = self
            .send_json(reqwest::Method::GET, "/api/users/me", None)
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn fetch_profile_image(&self, user_id: &str) -> Result<Vec<u8>, String> {
        let path = format!("/api/users/{user_id}/profile-image");
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        let bytes = resp.bytes().await.map_err(|err| err.to_string())?;
        Ok(bytes.to_vec())
    }

    /// Immich crops nothing itself; it shows the picture in a circle.
    pub async fn set_profile_image(
        &self,
        bytes: Vec<u8>,
        filename: &str,
        mime: &str,
    ) -> Result<(), String> {
        let base_url = self.require_active_url().await?;
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str(mime)
            .map_err(|err| err.to_string())?;
        let form = reqwest::multipart::Form::new().part("file", part);
        let resp = self
            .client
            .post(format!("{base_url}/api/users/profile-image"))
            .header("x-api-key", self.settings_snapshot().api_key)
            .timeout(std::time::Duration::from_secs(60))
            .multipart(form)
            .send()
            .await
            .map_err(|err| err.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        Ok(())
    }

    pub async fn remove_profile_image(&self) -> Result<(), String> {
        self.send_json(reqwest::Method::DELETE, "/api/users/profile-image", None)
            .await
            .map(|_| ())
    }

    /// Most recently used first.
    pub async fn fetch_sessions(&self) -> Result<Vec<Session>, String> {
        let resp = self
            .send_json(reqwest::Method::GET, "/api/sessions", None)
            .await?;
        let mut sessions: Vec<Session> = resp.json().await.map_err(|err| err.to_string())?;
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    pub async fn sign_out_session(&self, session_id: &str) -> Result<(), String> {
        let path = format!("/api/sessions/{session_id}");
        self.send_json(reqwest::Method::DELETE, &path, None)
            .await
            .map(|_| ())
    }

    pub async fn fetch_api_keys(&self) -> Result<Vec<ApiKey>, String> {
        let resp = self
            .send_json(reqwest::Method::GET, "/api/api-keys", None)
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    /// The key Mimick signs in with.
    pub async fn fetch_current_api_key(&self) -> Result<ApiKey, String> {
        let resp = self
            .send_json(reqwest::Method::GET, "/api/api-keys/me", None)
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn delete_api_key(&self, key_id: &str) -> Result<(), String> {
        let path = format!("/api/api-keys/{key_id}");
        self.send_json(reqwest::Method::DELETE, &path, None)
            .await
            .map(|_| ())
    }

    /// A new secret for the key (Immich v3.2.0+); the old one stops working at once.
    pub async fn rotate_api_key(&self, key_id: &str) -> Result<String, String> {
        let path = format!("/api/api-keys/{key_id}/rotate");
        let resp = self.send_json(reqwest::Method::POST, &path, None).await?;
        let rotated: RotatedKey = resp.json().await.map_err(|err| err.to_string())?;
        Ok(rotated.secret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_read_immich_field_names() {
        let session: Session = serde_json::from_value(serde_json::json!({
            "id": "s1",
            "deviceOS": "Android",
            "deviceType": "Mobile",
            "appVersion": "2.1.0",
            "createdAt": "2026-01-01T00:00:00.000Z",
            "updatedAt": "2026-02-01T00:00:00.000Z",
            "current": false,
            "isPendingSyncReset": false
        }))
        .unwrap();
        assert_eq!(session.device_os, "Android");
        assert_eq!(session.app_version.as_deref(), Some("2.1.0"));
    }

    #[test]
    fn accounts_without_a_picture_have_an_empty_path() {
        let account: Account = serde_json::from_value(serde_json::json!({
            "id": "u1", "name": "Ana", "email": "ana@example.com", "profileImagePath": ""
        }))
        .unwrap();
        assert!(account.profile_image_path.is_empty());
    }
}
