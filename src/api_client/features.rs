use serde_json::Value;

use super::ImmichApiClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryFeatures {
    pub trash: bool,
    pub people: bool,
    pub smart_search: bool,
    pub ocr: bool,
    pub places: bool,
    pub search: bool,
    pub tags: bool,
    pub folders: bool,
    pub shared_links: bool,
    pub ratings: bool,
    pub map: bool,
}

impl Default for LibraryFeatures {
    fn default() -> Self {
        Self::from_responses(None, None)
    }
}

impl LibraryFeatures {
    pub fn from_responses(server: Option<&Value>, preferences: Option<&Value>) -> Self {
        let flag = |value: Option<&Value>, pointer: &str| {
            value
                .and_then(|value| value.pointer(pointer))
                .and_then(Value::as_bool)
                .unwrap_or(true)
        };
        let server_flag = |pointer| flag(server, pointer);
        let preference = |pointer| flag(preferences, pointer);
        Self {
            trash: server_flag("/trash"),
            people: server_flag("/facialRecognition") && preference("/people/enabled"),
            smart_search: server_flag("/smartSearch"),
            ocr: server_flag("/ocr"),
            places: server_flag("/reverseGeocoding"),
            search: server_flag("/search"),
            tags: preference("/tags/enabled"),
            folders: preference("/folders/enabled"),
            shared_links: preference("/sharedLinks/enabled"),
            ratings: preference("/ratings/enabled"),
            map: server_flag("/map"),
        }
    }
}

impl ImmichApiClient {
    pub async fn fetch_library_features(&self) -> LibraryFeatures {
        let server = self.fetch_json("/api/server/features").await;
        let preferences = self.fetch_json("/api/users/me/preferences").await;
        LibraryFeatures::from_responses(server.as_ref(), preferences.as_ref())
    }

    async fn fetch_json(&self, path: &str) -> Option<Value> {
        match self.send_json(reqwest::Method::GET, path, None).await {
            Ok(resp) => resp.json().await.ok(),
            Err(err) => {
                log::debug!("Could not read {path}: {err}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_responses_enable_everything() {
        let features = LibraryFeatures::default();
        assert!(features.trash && features.people && features.tags && features.ratings);
    }

    #[test]
    fn server_and_account_flags_turn_features_off() {
        let server = serde_json::json!({
            "trash": false,
            "facialRecognition": true,
            "smartSearch": false,
            "ocr": false,
            "reverseGeocoding": true,
            "search": true,
            "map": false
        });
        let preferences = serde_json::json!({
            "people": { "enabled": false, "sidebarWeb": true },
            "tags": { "enabled": true, "sidebarWeb": false },
            "ratings": { "enabled": false }
        });
        let features = LibraryFeatures::from_responses(Some(&server), Some(&preferences));
        assert!(!features.trash);
        assert!(!features.people);
        assert!(!features.smart_search);
        assert!(!features.ocr);
        assert!(features.places);
        assert!(features.tags);
        assert!(features.folders);
        assert!(!features.ratings);
        assert!(!features.map);
    }
}
