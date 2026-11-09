//! Handles persistent configuration loading and provides desktop keyring access helpers.
//!
//! Configuration lives in a JSON file under the XDG config directory.
//! Watch-path entries support both simple paths and extended per-folder
//! rules (sync method, extension filters, size limits). The API key is
//! stored in the desktop keyring via `oo7` when available, with an
//! automatic file-based fallback (base64-encoded, `0600` permissions)
//! for desktops where no keyring service is running.

mod folders;
pub use folders::*;
mod schema;
pub use schema::{ConfigData, DateHeadings, MapDateRange, MapFilters};
mod keyring;
pub use keyring::ApiKeyStorage;
#[cfg(test)]
use keyring::{
    KeyringError, api_key_fallback_path, keyring_error_message, pick_api_key,
    read_api_key_file_fallback, remove_api_key_file_fallback, write_api_key_file_fallback,
};
use std::fs;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

/// Persistent config container wrapping loaded schema and file source info.
#[derive(Clone)]
pub struct Config {
    /// Active live configuration switches.
    pub data: ConfigData,
    /// Path to config file source.
    pub config_file: PathBuf,
}

impl Config {
    /// Load the config from the standard Mimick config path, creating a default file if missing.
    pub fn new() -> Self {
        let config_dir = crate::profile::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config").join(crate::profile::dir_segment()));

        let config_file = config_dir.join("config.json");

        let mut config = Config {
            data: ConfigData::default(),
            config_file,
        };

        config.load();
        config
    }

    /// Load or parse configuration state from standard path source.
    pub fn load(&mut self) -> bool {
        if self.config_file.exists() {
            if let Ok(content) = fs::read_to_string(&self.config_file) {
                if let Ok(data) = serde_json::from_str(&content) {
                    self.data = data;
                    log::info!("Config loaded from: {}", self.config_file.display());
                    return true;
                }
                log::warn!("Config parse failed: {}", self.config_file.display());
            }
        } else {
            log::info!(
                "No config found, creating default at: {}",
                self.config_file.display()
            );
            self.save();
        }
        false
    }

    /// Atomically write current configuration values back to disk.
    pub fn save(&self) -> bool {
        if let Ok(content) = serde_json::to_string_pretty(&self.data) {
            match crate::util::atomic_write(&self.config_file, content.as_bytes()) {
                Ok(()) => {
                    log::info!("Config saved to: {}", self.config_file.display());
                    true
                }
                Err(err) => {
                    log::error!(
                        "Failed to write config {}: {}",
                        self.config_file.display(),
                        err
                    );
                    false
                }
            }
        } else {
            false
        }
    }

    /// Return all configured watch paths as plain strings for the live monitor.
    pub fn watch_path_strings(&self) -> Vec<String> {
        self.data
            .watch_paths
            .iter()
            .map(|e| e.path().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests;
