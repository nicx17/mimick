//! Album sharing: album members, the user directory, and member roles.

use super::{ImmichApiClient, LibraryAlbum, UserSummary};

/// Role a member has in a shared album.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumRole {
    Viewer,
    Editor,
}

impl AlbumRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Editor => "editor",
        }
    }

    /// `None` for the owner (who can't be re-roled) or an unknown role.
    pub fn parse(role: &str) -> Option<Self> {
        match role {
            "viewer" => Some(Self::Viewer),
            "editor" => Some(Self::Editor),
            _ => None,
        }
    }
}

/// Body for `PUT /api/albums/{id}/users`.
fn add_users_body(members: &[(String, AlbumRole)]) -> serde_json::Value {
    let album_users: Vec<serde_json::Value> = members
        .iter()
        .map(|(user_id, role)| serde_json::json!({ "userId": user_id, "role": role.as_str() }))
        .collect();
    serde_json::json!({ "albumUsers": album_users })
}

impl ImmichApiClient {
    /// One album with its members (names and emails), without its assets.
    pub async fn fetch_album_detail(&self, album_id: &str) -> Result<LibraryAlbum, String> {
        let path = format!("/api/albums/{album_id}");
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    /// Everyone on the server the key can see (needs `user.read`).
    pub async fn list_users(&self) -> Result<Vec<UserSummary>, String> {
        let resp = self
            .send_json(reqwest::Method::GET, "/api/users", None)
            .await?;
        let mut users: Vec<UserSummary> = resp.json().await.map_err(|err| err.to_string())?;
        users.sort_by_key(|u| u.name.to_lowercase());
        Ok(users)
    }

    pub async fn add_album_users(
        &self,
        album_id: &str,
        members: &[(String, AlbumRole)],
    ) -> Result<(), String> {
        let path = format!("/api/albums/{album_id}/users");
        self.send_json(reqwest::Method::PUT, &path, Some(add_users_body(members)))
            .await
            .map(|_| ())
    }

    pub async fn set_album_user_role(
        &self,
        album_id: &str,
        user_id: &str,
        role: AlbumRole,
    ) -> Result<(), String> {
        let path = format!("/api/albums/{album_id}/user/{user_id}");
        let body = serde_json::json!({ "role": role.as_str() });
        self.send_json(reqwest::Method::PUT, &path, Some(body))
            .await
            .map(|_| ())
    }

    pub async fn leave_album(&self, album_id: &str) -> Result<(), String> {
        self.remove_album_user(album_id, "me").await
    }

    pub async fn remove_album_user(&self, album_id: &str, user_id: &str) -> Result<(), String> {
        let path = format!("/api/albums/{album_id}/user/{user_id}");
        self.send_json(reqwest::Method::DELETE, &path, None)
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_users_body_lists_ids_and_roles() {
        let body = add_users_body(&[
            ("u1".to_string(), AlbumRole::Viewer),
            ("u2".to_string(), AlbumRole::Editor),
        ]);
        assert_eq!(
            body,
            serde_json::json!({ "albumUsers": [
                { "userId": "u1", "role": "viewer" },
                { "userId": "u2", "role": "editor" }
            ]})
        );
    }

    #[test]
    fn album_role_parse_round_trips_and_skips_owner() {
        for role in [AlbumRole::Viewer, AlbumRole::Editor] {
            assert_eq!(AlbumRole::parse(role.as_str()), Some(role));
        }
        assert_eq!(AlbumRole::parse("owner"), None);
    }

    #[test]
    fn album_detail_reads_member_names() {
        let album: LibraryAlbum = serde_json::from_value(serde_json::json!({
            "id": "a1",
            "albumName": "Trips",
            "assetCount": 3,
            "albumThumbnailAssetId": null,
            "createdAt": "2024-01-01T00:00:00.000Z",
            "updatedAt": "2024-01-01T00:00:00.000Z",
            "albumUsers": [
                { "role": "owner", "user": { "id": "me", "name": "Me", "email": "me@x" } },
                { "role": "viewer", "user": { "id": "u2", "name": "Ana", "email": "ana@x" } }
            ]
        }))
        .unwrap();
        assert_eq!(album.album_users[1].user.name, "Ana");
        assert!(album.is_shared());
        assert_eq!(album.owner_id(), "me");
    }
}
