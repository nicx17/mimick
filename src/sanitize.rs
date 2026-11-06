//! Input sanitisation for file paths and URLs.
//!
//! Guards against:
//! * Directory traversal in filenames originating from Immich API responses
//! * Non-HTTP(S) URL schemes in user-provided server addresses

use crate::i18n::fill;
use gettextrs::gettext;
use std::path::Path;

/// Strip a filename to its final component and reject traversal attempts.
///
/// Returns `None` if the input is empty, contains only path separators, or
/// resolves to `.` / `..` after extraction.
///
/// # Examples
/// ```ignore
/// assert_eq!(safe_filename("photo.jpg"), Some("photo.jpg".into()));
/// assert_eq!(safe_filename("../../../etc/passwd"), Some("passwd".into()));
/// assert_eq!(safe_filename("sub/dir/file.jpg"), Some("file.jpg".into()));
/// ```
pub fn safe_filename(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    let p = Path::new(name);
    let stem = p.file_name()?;
    let s = stem.to_string_lossy();

    // Reject hidden traversal through the file_name component itself.
    if s == "." || s == ".." || s.is_empty() {
        return None;
    }

    Some(s.into_owned())
}

/// Validate that a URL string uses an HTTP or HTTPS scheme.
///
/// Returns the parsed, normalised URL on success, or a human-readable error
/// string suitable for display in the settings UI.
pub fn validate_http_url(raw: &str) -> Result<url::Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(gettext("URL is empty"));
    }

    let parsed = url::Url::parse(trimmed)
        .map_err(|e| fill(gettext("Invalid URL: {err}"), &[("err", &e.to_string())]))?;

    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        other => Err(format!(
            "Unsupported URL scheme '{}'. Only http:// and https:// are allowed.",
            other
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- safe_filename --

    #[test]
    fn safe_filename_extracts_basename() {
        assert_eq!(safe_filename("photo.jpg"), Some("photo.jpg".into()));
    }

    #[test]
    fn safe_filename_strips_directory_components() {
        assert_eq!(safe_filename("sub/dir/file.jpg"), Some("file.jpg".into()));
    }

    #[test]
    fn safe_filename_strips_traversal_to_basename() {
        // The defense is that traversal components are stripped, leaving only
        // the final filename component.
        assert_eq!(safe_filename("../../../etc/passwd"), Some("passwd".into()));
    }

    #[test]
    fn safe_filename_rejects_empty() {
        assert_eq!(safe_filename(""), None);
        assert_eq!(safe_filename("   "), None);
    }

    #[test]
    fn safe_filename_rejects_dot_dot() {
        assert_eq!(safe_filename(".."), None);
        assert_eq!(safe_filename("."), None);
    }

    // -- validate_http_url --

    #[test]
    fn validate_accepts_https() {
        let result = validate_http_url("https://immich.example.com");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().scheme(), "https");
    }

    #[test]
    fn validate_accepts_http() {
        let result = validate_http_url("http://192.168.1.1:2283");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_rejects_file_scheme() {
        let result = validate_http_url("file:///etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("file"));
    }

    #[test]
    fn validate_rejects_javascript_scheme() {
        let result = validate_http_url("javascript:alert(1)");
        assert!(result.is_err());
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(validate_http_url("").is_err());
        assert!(validate_http_url("  ").is_err());
    }

    #[test]
    fn validate_trims_whitespace() {
        let result = validate_http_url("  https://immich.local  ");
        assert!(result.is_ok());
    }
}
