use super::{ImmichApiClient, UserSummary};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartnerDirection {
    SharedBy,
    SharedWith,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Partner {
    #[serde(flatten)]
    pub user: UserSummary,
    #[serde(rename = "inTimeline", default)]
    pub in_timeline: bool,
}

fn partners_path(direction: PartnerDirection) -> &'static str {
    match direction {
        PartnerDirection::SharedBy => "/api/partners?direction=shared-by",
        PartnerDirection::SharedWith => "/api/partners?direction=shared-with",
    }
}

impl ImmichApiClient {
    pub async fn fetch_partners(
        &self,
        direction: PartnerDirection,
    ) -> Result<Vec<Partner>, String> {
        let resp = self
            .send_json(reqwest::Method::GET, partners_path(direction), None)
            .await?;
        let mut partners: Vec<Partner> = resp.json().await.map_err(|err| err.to_string())?;
        partners.sort_by_key(|p| p.user.name.to_lowercase());
        Ok(partners)
    }

    pub async fn set_partner_in_timeline(
        &self,
        user_id: &str,
        in_timeline: bool,
    ) -> Result<(), String> {
        let path = format!("/api/partners/{user_id}");
        let body = serde_json::json!({ "inTimeline": in_timeline });
        self.send_json(reqwest::Method::PUT, &path, Some(body))
            .await
            .map(|_| ())
    }

    pub async fn add_partner(&self, user_id: &str) -> Result<(), String> {
        let body = serde_json::json!({ "sharedWithId": user_id });
        self.send_json(reqwest::Method::POST, "/api/partners", Some(body))
            .await
            .map(|_| ())
    }

    pub async fn remove_partner(&self, user_id: &str) -> Result<(), String> {
        let path = format!("/api/partners/{user_id}");
        self.send_json(reqwest::Method::DELETE, &path, None)
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partners_path_names_the_direction() {
        assert!(partners_path(PartnerDirection::SharedBy).ends_with("shared-by"));
        assert!(partners_path(PartnerDirection::SharedWith).ends_with("shared-with"));
    }

    #[test]
    fn partner_json_reads_the_user_and_timeline_flag() {
        let partner: Partner = serde_json::from_value(serde_json::json!({
            "id": "u2",
            "name": "Ana",
            "email": "ana@example.com",
            "inTimeline": true,
            "avatarColor": "primary",
            "profileImagePath": "",
            "profileChangedAt": "2024-01-01T00:00:00.000Z"
        }))
        .unwrap();
        assert_eq!(partner.user.email, "ana@example.com");
        assert!(partner.in_timeline);
    }

    #[test]
    fn shared_by_partners_without_the_flag_read_as_hidden() {
        let partner: Partner =
            serde_json::from_value(serde_json::json!({ "id": "u3", "name": "Bo" })).unwrap();
        assert!(!partner.in_timeline);
    }
}
