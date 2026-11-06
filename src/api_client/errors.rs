//! HTTP and network error classification for user-facing diagnostics.
//!
//! Maps HTTP status codes and request context into structured `ApiIssue`
//! values with a summary and guidance string. The settings window and
//! health dashboard display these to help users self-diagnose problems.

use super::ApiIssue;
use crate::i18n::fill;
use gettextrs::gettext;

/// Identifies the operational context in which an Immich request was made.
#[derive(Debug, Clone, Copy)]
pub(super) enum RequestContext {
    Upload,
    Albums,
    AlbumCreate,
    AlbumAssign,
    ThumbnailFetch,
    AssetList,
    SmartSearch,
    MetadataSearch,
    AssetDownload,
    ServerStats,
    ServerAbout,
}

/// Map an HTTP status code to an actionable user-facing `ApiIssue`.
pub(super) fn classify_http_issue(
    context: RequestContext,
    status: u16,
    subject: Option<&str>,
) -> ApiIssue {
    known_status_issue(context, status).unwrap_or_else(|| ApiIssue {
        summary: failure_summary(context, subject),
        guidance: fill(gettext("The server responded with HTTP {status}. Check the server logs and retry after confirming the current configuration."), &[("status", &status.to_string())]),
    })
}

/// Statuses with a specific cause and fix; `None` falls back to a per-context summary.
fn known_status_issue(context: RequestContext, status: u16) -> Option<ApiIssue> {
    let (summary, guidance) = match status {
        401 | 403 => (
            &gettext("Immich rejected the API key"),
            &gettext(
                "Update the API key in Settings and confirm it still has the required Asset and Album permissions (see README for the full list).",
            ),
        ),
        404 if matches!(
            context,
            RequestContext::AlbumAssign | RequestContext::AlbumCreate
        ) =>
        {
            (
                &gettext("An album reference is no longer valid"),
                &gettext("Refresh the album list or choose a different album before retrying."),
            )
        }
        413 => (
            &gettext("Immich rejected a file as too large"),
            &gettext(
                "Reduce the file size, raise the server upload limit, or skip oversized files with folder rules.",
            ),
        ),
        429 => (
            &gettext("Immich rate-limited the request"),
            &gettext(
                "Wait a moment and retry. If this happens often, lower upload concurrency or check reverse proxy limits.",
            ),
        ),
        502..=504 => (
            &gettext("Immich is temporarily unavailable"),
            &gettext(
                "Wait a moment and retry. If it keeps happening, inspect the server and reverse proxy logs.",
            ),
        ),
        _ => return None,
    };
    Some(ApiIssue {
        summary: summary.to_string(),
        guidance: guidance.to_string(),
    })
}

/// What failed, phrased for the request context.
fn failure_summary(context: RequestContext, subject: Option<&str>) -> String {
    match context {
        RequestContext::Upload => {
            let subject = subject.map_or_else(|| gettext("the upload"), ToString::to_string);
            fill(
                gettext("Immich could not accept {subject}"),
                &[("subject", &subject)],
            )
        }
        RequestContext::AlbumCreate => {
            let album = subject.map_or_else(|| gettext("Unnamed"), ToString::to_string);
            fill(
                gettext("Immich could not create album '{album}'"),
                &[("album", &album)],
            )
        }
        RequestContext::Albums => gettext("Immich could not load the album list"),
        RequestContext::AlbumAssign => {
            gettext("Immich could not add the asset to the selected album")
        }
        RequestContext::ThumbnailFetch => gettext("Immich could not load a library thumbnail"),
        RequestContext::AssetList => gettext("Immich could not load library assets"),
        RequestContext::SmartSearch => gettext("Immich could not run the smart library search"),
        RequestContext::MetadataSearch => {
            gettext("Immich could not run the metadata library search")
        }
        RequestContext::AssetDownload => gettext("Immich could not download the selected asset"),
        RequestContext::ServerStats => gettext("Immich could not load library statistics"),
        RequestContext::ServerAbout => gettext("Immich could not load server version information"),
    }
}

/// Map a reqwest connection or timeout error to an actionable user-facing `ApiIssue`.
pub(super) fn classify_network_issue(context: RequestContext, error: &reqwest::Error) -> ApiIssue {
    if error.is_timeout() {
        ApiIssue {
            summary: gettext("The Immich request timed out"),
            guidance: gettext("Check network quality and server responsiveness, then retry."),
        }
    } else if error.is_connect() {
        ApiIssue {
            summary: gettext("Could not reach the Immich server"),
            guidance: gettext(
                "Check the configured URLs, your network connection, and whether the server is online.",
            ),
        }
    } else {
        ApiIssue {
            summary: match context {
                RequestContext::Upload => gettext("The upload request failed before completion"),
                RequestContext::Albums => gettext("The album request failed before completion"),
                RequestContext::AlbumCreate => {
                    gettext("The album creation request failed before completion")
                }
                RequestContext::AlbumAssign => {
                    gettext("The album assignment request failed before completion")
                }
                RequestContext::ThumbnailFetch => {
                    gettext("The thumbnail request failed before completion")
                }
                RequestContext::AssetList => {
                    gettext("The library asset request failed before completion")
                }
                RequestContext::SmartSearch => {
                    gettext("The smart search request failed before completion")
                }
                RequestContext::MetadataSearch => {
                    gettext("The metadata search request failed before completion")
                }
                RequestContext::AssetDownload => {
                    gettext("The asset download request failed before completion")
                }
                RequestContext::ServerStats => {
                    gettext("The library statistics request failed before completion")
                }
                RequestContext::ServerAbout => {
                    gettext("The server version request failed before completion")
                }
            },
            guidance: gettext(
                "Retry the request after checking network connectivity and server health.",
            ),
        }
    }
}
