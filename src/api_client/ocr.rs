//! Text Immich recognized in photos (OCR).

use super::ImmichApiClient;

/// One recognized piece of text; corners run top-left, top-right, bottom-right, bottom-left,
/// normalized 0–1 to the edited, orientation-corrected image.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct OcrBox {
    pub text: String,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub x3: f64,
    pub y3: f64,
    pub x4: f64,
    pub y4: f64,
}

impl OcrBox {
    pub fn corners(&self) -> [(f64, f64); 4] {
        [
            (self.x1, self.y1),
            (self.x2, self.y2),
            (self.x3, self.y3),
            (self.x4, self.y4),
        ]
    }
}

impl ImmichApiClient {
    /// OCR boxes for an asset (`GET /api/assets/{id}/ocr`).
    pub async fn fetch_asset_ocr(&self, asset_id: &str) -> Result<Vec<OcrBox>, String> {
        let path = format!("/api/assets/{asset_id}/ocr");
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        resp.json().await.map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_box_parses_immich_response() {
        let json = serde_json::json!([{
            "id": "o1",
            "assetId": "a1",
            "text": "EXIT",
            "textScore": 0.98,
            "boxScore": 0.91,
            "x1": 0.125,
            "y1": 0.25,
            "x2": 0.5,
            "y2": 0.25,
            "x3": 0.5,
            "y3": 0.5,
            "x4": 0.125,
            "y4": 0.5
        }]);
        let boxes: Vec<OcrBox> = serde_json::from_value(json).unwrap();
        assert_eq!(boxes[0].text, "EXIT");
        assert_eq!(
            boxes[0].corners(),
            [(0.125, 0.25), (0.5, 0.25), (0.5, 0.5), (0.125, 0.5)]
        );
    }
}
