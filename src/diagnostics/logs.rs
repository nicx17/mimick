//! The end of Mimick's log for a diagnostics bundle, with the API key, server addresses,
//! and the home folder replaced so the file can be attached to a public issue.

use std::fs;
use std::path::Path;

use crate::config::Config;

/// Enough for the last few sessions without making the bundle large.
const TAIL_LINES: usize = 2000;
/// The current log: `mimick.log`, or the name older flexi_logger builds used.
const LOG_NAMES: [&str; 2] = ["mimick.log", "mimick_rCURRENT.log"];

/// The last lines of the log in `log_dir`, redacted; empty when there's no log yet.
pub(super) fn redacted_tail(log_dir: &Path, config: &Config, api_key: &str) -> String {
    let text = LOG_NAMES
        .iter()
        .find_map(|name| fs::read_to_string(log_dir.join(name)).ok())
        .unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n");
    redact(&tail, &secrets(config, api_key))
}

/// What must not leave the machine, and what replaces it.
fn secrets(config: &Config, api_key: &str) -> Vec<(String, &'static str)> {
    let mut secrets = vec![(api_key.to_string(), "[api key]")];
    for url in [&config.data.internal_url, &config.data.external_url] {
        secrets.push((url.trim_end_matches('/').to_string(), "[server]"));
        // Addresses also show up without their scheme, as host:port.
        if let Ok(parsed) = reqwest::Url::parse(url)
            && let Some(host) = parsed.host_str()
        {
            secrets.push((host.to_string(), "[server]"));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        secrets.push((home.to_string_lossy().into_owned(), "~"));
    }
    secrets
}

/// Replace each secret, longest first so a full address goes before its bare host.
fn redact(text: &str, secrets: &[(String, &str)]) -> String {
    let mut ordered: Vec<&(String, &str)> = secrets
        .iter()
        .filter(|(value, _)| value.len() > 1)
        .collect();
    ordered.sort_by_key(|(value, _)| std::cmp::Reverse(value.len()));
    ordered
        .iter()
        .fold(text.to_string(), |text, (value, label)| {
            text.replace(value.as_str(), label)
        })
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn redact_replaces_the_full_address_before_its_host() {
        let secrets = vec![
            ("photos.example.com".to_string(), "[server]"),
            ("https://photos.example.com".to_string(), "[server]"),
            ("abc123".to_string(), "[api key]"),
            ("/home/ana".to_string(), "~"),
        ];
        let line = "GET https://photos.example.com/api key=abc123 from /home/ana/Pictures";
        assert_eq!(
            redact(line, &secrets),
            "GET [server]/api key=[api key] from ~/Pictures"
        );
    }

    #[test]
    fn redact_skips_empty_secrets() {
        let secrets = vec![(String::new(), "[api key]")];
        assert_eq!(redact("nothing to hide", &secrets), "nothing to hide");
    }
}
