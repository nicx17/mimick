//! Asset maintenance jobs: `POST /api/assets/jobs` asks Immich to redo work on some assets.

use super::ImmichApiClient;

/// A job Immich can run again on assets; it needs `job.create` and the assets' owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetJob {
    RefreshFaces,
    RefreshMetadata,
    RegenerateThumbnail,
    TranscodeVideo,
}

impl AssetJob {
    /// The `AssetJobName` Immich expects.
    pub fn name(self) -> &'static str {
        match self {
            AssetJob::RefreshFaces => "refresh-faces",
            AssetJob::RefreshMetadata => "refresh-metadata",
            AssetJob::RegenerateThumbnail => "regenerate-thumbnail",
            AssetJob::TranscodeVideo => "transcode-video",
        }
    }
}

impl ImmichApiClient {
    /// Queue `job` for `ids`; Immich runs it in the background.
    pub async fn run_asset_job(&self, job: AssetJob, ids: &[String]) -> Result<(), String> {
        let body = serde_json::json!({ "name": job.name(), "assetIds": ids });
        self.send_json(reqwest::Method::POST, "/api/assets/jobs", Some(body))
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_names_match_immich() {
        assert_eq!(AssetJob::RefreshFaces.name(), "refresh-faces");
        assert_eq!(AssetJob::RegenerateThumbnail.name(), "regenerate-thumbnail");
    }
}
