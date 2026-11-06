//! Faces on one photo: tagging, reassigning, and removing them.

use super::{ImmichApiClient, Person};

/// A face box on a photo, in pixels of the `image_width` × `image_height` image it was found on.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetFace {
    pub id: String,
    pub image_width: u32,
    pub image_height: u32,
    #[serde(rename = "boundingBoxX1")]
    pub x1: i64,
    #[serde(rename = "boundingBoxY1")]
    pub y1: i64,
    #[serde(rename = "boundingBoxX2")]
    pub x2: i64,
    #[serde(rename = "boundingBoxY2")]
    pub y2: i64,
    #[serde(default)]
    pub person: Option<Person>,
}

/// A face drawn by hand: its box in pixels of an `image_width` × `image_height` image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewFace {
    pub image_width: u32,
    pub image_height: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ImmichApiClient {
    pub async fn fetch_faces(&self, asset_id: &str) -> Result<Vec<AssetFace>, String> {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("id", asset_id)
            .finish();
        let resp = self
            .send_json(reqwest::Method::GET, &format!("/api/faces?{query}"), None)
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }

    pub async fn create_face(
        &self,
        asset_id: &str,
        person_id: &str,
        face: NewFace,
    ) -> Result<(), String> {
        let body = serde_json::json!({
            "assetId": asset_id,
            "personId": person_id,
            "imageWidth": face.image_width,
            "imageHeight": face.image_height,
            "x": face.x,
            "y": face.y,
            "width": face.width,
            "height": face.height,
        });
        self.send_json(reqwest::Method::POST, "/api/faces", Some(body))
            .await
            .map(|_| ())
    }

    /// Moves one face to `person_id`.
    pub async fn reassign_face(&self, face_id: &str, person_id: &str) -> Result<(), String> {
        let body = serde_json::json!({ "id": person_id });
        self.send_json(
            reqwest::Method::PUT,
            &format!("/api/faces/{face_id}"),
            Some(body),
        )
        .await
        .map(|_| ())
    }

    pub async fn delete_face(&self, face_id: &str) -> Result<(), String> {
        let body = serde_json::json!({ "force": false });
        self.send_json(
            reqwest::Method::DELETE,
            &format!("/api/faces/{face_id}"),
            Some(body),
        )
        .await
        .map(|_| ())
    }

    pub async fn create_person(&self, name: &str) -> Result<Person, String> {
        let body = serde_json::json!({ "name": name });
        let resp = self
            .send_json(reqwest::Method::POST, "/api/people", Some(body))
            .await?;
        resp.json().await.map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_read_boxes_and_an_optional_person() {
        let faces: Vec<AssetFace> = serde_json::from_value(serde_json::json!([
            {
                "id": "f1",
                "imageWidth": 1440,
                "imageHeight": 1080,
                "boundingBoxX1": 10, "boundingBoxY1": 20,
                "boundingBoxX2": 110, "boundingBoxY2": 140,
                "person": { "id": "p1", "name": "Ana" },
                "sourceType": "machine-learning"
            },
            {
                "id": "f2",
                "imageWidth": 1440,
                "imageHeight": 1080,
                "boundingBoxX1": 0, "boundingBoxY1": 0,
                "boundingBoxX2": 5, "boundingBoxY2": 5,
                "person": null
            }
        ]))
        .unwrap();
        assert_eq!(faces[0].person.as_ref().unwrap().name, "Ana");
        assert_eq!((faces[0].x2, faces[0].y2), (110, 140));
        assert!(faces[1].person.is_none());
    }
}
