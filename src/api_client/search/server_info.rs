//! Server-level statistics, about information, and version history.

use std::time::Duration;

use super::super::errors::{RequestContext, classify_http_issue, classify_network_issue};
use super::super::{
    ImmichApiClient, ServerAbout, ServerStatistics, ServerStats, VersionHistoryEntry,
};

impl ImmichApiClient {
    /// Retrieve total images, videos, and overall asset count statistics from the server.
    pub async fn fetch_server_stats(&self) -> Result<ServerStats, String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let url = format!("{}/api/assets/statistics", base_url);

        match self
            .client
            .get(&url)
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let stats = resp
                    .json::<ServerStats>()
                    .await
                    .map_err(|err| err.to_string())?;
                self.clear_issue().await;
                Ok(stats)
            }
            Ok(resp) => {
                self.set_issue(classify_http_issue(
                    RequestContext::ServerStats,
                    resp.status().as_u16(),
                    None,
                ))
                .await;
                Err(format!("HTTP {}", resp.status()))
            }
            Err(err) => {
                *self.active_url.lock().await = None;
                self.set_issue(classify_network_issue(RequestContext::ServerStats, &err))
                    .await;
                Err(err.to_string())
            }
        }
    }

    /// Fetch server-wide statistics including per-user usage breakdown.
    ///
    /// Admin-only endpoint; non-admin sessions will receive an HTTP 403 and an
    /// `Err` is returned. Caller should fall back to per-user asset counts.
    pub async fn fetch_server_statistics(&self) -> Result<ServerStatistics, String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let url = format!("{}/api/server/statistics", base_url);

        match self
            .client
            .get(&url)
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => resp
                .json::<ServerStatistics>()
                .await
                .map_err(|err| err.to_string()),
            Ok(resp) => Err(format!("HTTP {}", resp.status())),
            Err(err) => Err(err.to_string()),
        }
    }

    /// Versions this server has run, newest first.
    pub async fn fetch_version_history(&self) -> Result<Vec<VersionHistoryEntry>, String> {
        let resp = self
            .send_json(reqwest::Method::GET, "/api/server/version-history", None)
            .await?;
        let mut history: Vec<VersionHistoryEntry> =
            resp.json().await.map_err(|err| err.to_string())?;
        history.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(history)
    }

    /// Retrieve detailed Immich server system information (e.g. version).
    pub async fn fetch_server_about(&self) -> Result<ServerAbout, String> {
        let base_url = self
            .get_active_url()
            .await
            .ok_or_else(|| "No active connection".to_string())?;
        let settings = self.settings_snapshot();
        let url = format!("{}/api/server/about", base_url);

        match self
            .client
            .get(&url)
            .header("x-api-key", &settings.api_key)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let about = resp
                    .json::<ServerAbout>()
                    .await
                    .map_err(|err| err.to_string())?;
                self.clear_issue().await;
                Ok(about)
            }
            Ok(resp) => {
                self.set_issue(classify_http_issue(
                    RequestContext::ServerAbout,
                    resp.status().as_u16(),
                    None,
                ))
                .await;
                Err(format!("HTTP {}", resp.status()))
            }
            Err(err) => {
                *self.active_url.lock().await = None;
                self.set_issue(classify_network_issue(RequestContext::ServerAbout, &err))
                    .await;
                Err(err.to_string())
            }
        }
    }
}
