use super::{ImmichApiClient, Person};

#[derive(Debug, serde::Deserialize)]
struct PersonStatistics {
    assets: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_hidden: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub birth_date: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_favorite: Option<bool>,
    /// Photo whose face becomes the person's cover picture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_face_asset_id: Option<String>,
}

impl ImmichApiClient {
    pub async fn update_person(
        &self,
        person_id: &str,
        update: &PersonUpdate,
    ) -> Result<(), String> {
        let body = serde_json::to_value(update).map_err(|err| err.to_string())?;
        let path = format!("/api/people/{person_id}");
        self.send_json(reqwest::Method::PUT, &path, Some(body))
            .await
            .map(|_| ())
    }

    pub async fn fetch_person(&self, person_id: &str) -> Result<Person, String> {
        let path = format!("/api/people/{person_id}");
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    /// Everyone whose name matches `name`, hidden people too, across the whole server.
    pub async fn search_people(&self, name: &str) -> Result<Vec<Person>, String> {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("name", name)
            .append_pair("withHidden", "true")
            .finish();
        let path = format!("/api/search/person?{query}");
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn person_asset_count(&self, person_id: &str) -> Result<u64, String> {
        let path = format!("/api/people/{person_id}/statistics");
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        let stats: PersonStatistics = resp.json().await.map_err(|err| err.to_string())?;
        Ok(stats.assets)
    }

    /// Merges `merge_ids` into `into_id` with `POST /people/merge` (v3.2.1+), falling back
    /// to the deprecated `/people/{id}/merge` on older servers, which answer 404.
    pub async fn merge_people(&self, into_id: &str, merge_ids: &[String]) -> Result<(), String> {
        let body = merge_body(into_id, merge_ids);
        match self
            .send_json(reqwest::Method::POST, "/api/people/merge", Some(body))
            .await
        {
            Err(err) if err.starts_with("HTTP 404") => {
                let path = format!("/api/people/{into_id}/merge");
                let body = serde_json::json!({ "ids": merge_ids });
                self.send_json(reqwest::Method::POST, &path, Some(body))
                    .await
                    .map(|_| ())
            }
            result => result.map(|_| ()),
        }
    }
}

/// The new endpoint keeps the first person of an ordered list, so the target goes first.
fn merge_body(into_id: &str, merge_ids: &[String]) -> serde_json::Value {
    let ids: Vec<&str> = std::iter::once(into_id)
        .chain(merge_ids.iter().map(String::as_str))
        .collect();
    serde_json::json!({ "ids": ids })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_body_puts_the_kept_person_first() {
        let merged = ["b".to_string(), "c".to_string()];
        assert_eq!(
            merge_body("a", &merged),
            serde_json::json!({ "ids": ["a", "b", "c"] })
        );
    }

    #[test]
    fn update_sends_only_set_fields() {
        let rename = PersonUpdate {
            name: Some("Ana".into()),
            ..PersonUpdate::default()
        };
        assert_eq!(
            serde_json::to_value(&rename).unwrap(),
            serde_json::json!({ "name": "Ana" })
        );
    }

    #[test]
    fn clearing_a_birthday_sends_null() {
        let clear = PersonUpdate {
            birth_date: Some(None),
            ..PersonUpdate::default()
        };
        assert_eq!(
            serde_json::to_value(&clear).unwrap(),
            serde_json::json!({ "birthDate": null })
        );
    }

    #[test]
    fn cover_update_sends_feature_face_asset_id() {
        let cover = PersonUpdate {
            feature_face_asset_id: Some("a1".into()),
            ..PersonUpdate::default()
        };
        assert_eq!(
            serde_json::to_value(&cover).unwrap(),
            serde_json::json!({ "featureFaceAssetId": "a1" })
        );
    }

    #[test]
    fn favorite_update_sends_is_favorite() {
        let favorite = PersonUpdate {
            is_favorite: Some(true),
            ..PersonUpdate::default()
        };
        assert_eq!(
            serde_json::to_value(&favorite).unwrap(),
            serde_json::json!({ "isFavorite": true })
        );
    }

    #[test]
    fn person_json_reads_birth_date() {
        let person: super::super::Person = serde_json::from_value(serde_json::json!({
            "id": "p1",
            "name": "Ana",
            "isHidden": true,
            "birthDate": "1990-05-04"
        }))
        .unwrap();
        assert!(person.is_hidden);
        assert_eq!(person.birth_date.as_deref(), Some("1990-05-04"));
    }
}
