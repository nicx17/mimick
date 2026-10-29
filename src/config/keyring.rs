use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::Config;

const KEYRING_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
static KEYRING_UNRESPONSIVE: AtomicBool = AtomicBool::new(false);

impl Config {
    /// Look up the API key, preferring the file fallback over the keyring.
    ///
    /// The fallback file only exists when the most recent save could not reach
    /// the keyring, so it is newer than anything the keyring holds.
    pub fn get_api_key(&self) -> Option<String> {
        let key = pick_api_key(read_api_key_file_fallback(&self.config_file), || {
            self.get_api_key_from_keyring()
        });
        if key.is_none() {
            log::warn!(
                "No API key found in keyring or file fallback. \
                 User must configure it in Settings."
            );
        }
        key
    }

    fn get_api_key_from_keyring(&self) -> Option<String> {
        log::debug!("Attempting to read API key from keyring...");
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let keyring = keyring_with_dbus_fallback().await?;
                let account = crate::profile::keyring_account();
                let attributes: Vec<(&str, &str)> =
                    vec![("service", "mimick"), ("account", account.as_str())];
                let items = keyring.search_items(&attributes).await?;
                if let Some(item) = items.first() {
                    let secret = item.secret().await?;
                    let key = String::from_utf8_lossy(&secret).trim().to_string();
                    if !key.is_empty() {
                        return Ok::<Option<String>, KeyringError>(Some(key));
                    }
                }
                Ok(None)
            })
        });

        match result {
            Ok(key) => {
                if key.is_some() {
                    log::debug!("API key retrieved via oo7 keyring.");
                }
                key
            }
            Err(e) => {
                log::warn!(
                    "Keyring lookup failed ({}): {:?}",
                    keyring_error_hint(&e),
                    e
                );
                None
            }
        }
    }

    /// Store the API key in the desktop keyring, falling back to a file.
    ///
    /// Returns where the key ended up, or user-facing guidance if both failed.
    pub fn set_api_key(&self, key: &str) -> Result<ApiKeyStorage, String> {
        log::debug!("Attempting to save API key to keyring...");
        let secret = key.to_string();
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let keyring = keyring_with_dbus_fallback().await?;
                let account = crate::profile::keyring_account();
                let attributes: Vec<(&str, &str)> =
                    vec![("service", "mimick"), ("account", account.as_str())];
                let label = match crate::profile::name() {
                    Some(profile) => format!("Mimick API Key ({})", profile),
                    None => "Mimick API Key".to_string(),
                };
                keyring
                    .create_item(&label, &attributes, secret.as_bytes(), true)
                    .await?;
                Ok::<(), KeyringError>(())
            })
        });

        match result {
            Ok(()) => {
                log::info!("API key saved via oo7 keyring.");
                remove_api_key_file_fallback(&self.config_file);
                Ok(ApiKeyStorage::Keyring)
            }
            Err(e) => {
                log::error!(
                    "Failed to save API key via oo7 keyring: {:?} -- using file fallback",
                    e
                );
                match write_api_key_file_fallback(&self.config_file, key) {
                    Ok(()) => {
                        log::info!("API key saved via file fallback.");
                        Ok(ApiKeyStorage::File(api_key_fallback_path(
                            &self.config_file,
                        )))
                    }
                    Err(file_err) => {
                        log::error!("File fallback also failed: {}", file_err);
                        Err(format!(
                            "{}\n\nFile fallback also failed: {}\n\nTechnical detail: {}",
                            keyring_error_message(&e),
                            file_err,
                            e
                        ))
                    }
                }
            }
        }
    }
}

/// Where `Config::set_api_key` stored the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeyStorage {
    Keyring,
    File(PathBuf),
}

#[derive(Debug)]
pub(super) enum KeyringError {
    Backend(Box<oo7::Error>),
    Timeout(&'static str),
}

impl From<oo7::Error> for KeyringError {
    fn from(e: oo7::Error) -> Self {
        Self::Backend(Box::new(e))
    }
}

impl From<oo7::dbus::Error> for KeyringError {
    fn from(e: oo7::dbus::Error) -> Self {
        Self::Backend(Box::new(oo7::Error::from(e)))
    }
}

impl std::fmt::Display for KeyringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend(e) => write!(f, "{}", e),
            Self::Timeout(step) => write!(
                f,
                "{} timed out after {}s",
                step,
                KEYRING_CONNECT_TIMEOUT.as_secs()
            ),
        }
    }
}

pub(super) fn pick_api_key(
    file_key: Option<String>,
    keyring_key: impl FnOnce() -> Option<String>,
) -> Option<String> {
    if file_key.is_some() {
        log::info!("API key loaded from file fallback.");
        return file_key;
    }
    keyring_key()
}

async fn with_connect_timeout<T, E: Into<KeyringError>>(
    step: &'static str,
    fut: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, KeyringError> {
    match tokio::time::timeout(KEYRING_CONNECT_TIMEOUT, fut).await {
        Ok(result) => result.map_err(Into::into),
        Err(_) => {
            KEYRING_UNRESPONSIVE.store(true, Ordering::Relaxed);
            log::error!(
                "{} timed out after {}s; skipping the keyring for this session",
                step,
                KEYRING_CONNECT_TIMEOUT.as_secs()
            );
            Err(KeyringError::Timeout(step))
        }
    }
}

/// Try `oo7::Keyring::new()`, which auto-selects portal (Flatpak) or
/// D-Bus (native), falling back to the D-Bus Secret Service if the portal fails.
async fn keyring_with_dbus_fallback() -> Result<oo7::Keyring, KeyringError> {
    if KEYRING_UNRESPONSIVE.load(Ordering::Relaxed) {
        log::debug!("Keyring timed out earlier this session; not retrying.");
        return Err(KeyringError::Timeout("Keyring connection"));
    }

    match with_connect_timeout("Keyring connection", oo7::Keyring::new()).await {
        Err(KeyringError::Backend(e)) if matches!(*e, oo7::Error::File(_)) => {
            log::info!(
                "oo7 portal backend unavailable ({}), trying D-Bus Secret Service fallback",
                e
            );
            let service =
                with_connect_timeout("D-Bus Secret Service connection", oo7::dbus::Service::new())
                    .await?;
            let collection = with_connect_timeout(
                "D-Bus Secret Service default collection",
                service.default_collection(),
            )
            .await?;
            Ok(oo7::Keyring::DBus(collection))
        }
        other => other,
    }
}

/// Return a user-facing error message for a keyring failure, tailored
/// to the specific backend that failed.
pub(super) fn keyring_error_message(e: &KeyringError) -> &'static str {
    match e {
        KeyringError::Backend(backend) => match **backend {
            oo7::Error::File(_) => {
                "Your desktop's Secret portal is not configured.\n\
                 This typically happens on Hyprland, Sway, XFCE, and other \
                 non-GNOME/KDE desktops.\n\n\
                 To fix this, ensure gnome-keyring (or kwallet) is installed \
                 and add an entry for org.freedesktop.impl.portal.Secret in \
                 your portal configuration.\n\n\
                 See: https://github.com/nicx17/mimick/wiki/Keyring-Setup"
            }
            oo7::Error::DBus(_) => {
                "The D-Bus Secret Service is not available.\n\
                 Ensure gnome-keyring-daemon or KWallet is installed \
                 and running.\n\n\
                 See: https://github.com/nicx17/mimick/wiki/Keyring-Setup"
            }
        },
        KeyringError::Timeout(_) => {
            "The keyring did not respond within 5 seconds.\n\
             Your Secret portal or keyring service may be misconfigured.\n\n\
             See: https://github.com/nicx17/mimick/wiki/Keyring-Setup"
        }
    }
}

/// Short diagnostic label for log messages.
fn keyring_error_hint(e: &KeyringError) -> &'static str {
    match e {
        KeyringError::Backend(backend) => match **backend {
            oo7::Error::File(_) => "portal backend error",
            oo7::Error::DBus(_) => "D-Bus Secret Service error",
        },
        KeyringError::Timeout(_) => "keyring timeout",
    }
}

// ---------------------------------------------------------------------------
// File-based API key fallback
// ---------------------------------------------------------------------------

/// Derive the fallback file path from the config file location.
///
/// The file is placed alongside the config JSON so it respects profile
/// isolation and is covered by the same backup/permission scope.
pub(super) fn api_key_fallback_path(config_file: &Path) -> PathBuf {
    let account = crate::profile::keyring_account();
    config_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".{}.key", account))
}

/// Read the API key from the base64-encoded fallback file.
pub(super) fn read_api_key_file_fallback(config_file: &Path) -> Option<String> {
    use base64::Engine;
    let path = api_key_fallback_path(config_file);
    match fs::read_to_string(&path) {
        Ok(encoded) => {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .ok()?;
            let key = String::from_utf8(decoded).ok()?;
            let trimmed = key.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        Err(_) => None,
    }
}

/// Write the API key to the base64-encoded fallback file with
/// restrictive permissions (owner read/write only).
pub(super) fn write_api_key_file_fallback(config_file: &Path, key: &str) -> Result<(), String> {
    use base64::Engine;
    let path = api_key_fallback_path(config_file);

    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create dir: {}", e))?;
    }

    let encoded = base64::engine::general_purpose::STANDARD.encode(key.as_bytes());
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary_path = path.with_extension(format!("key.tmp.{}.{}", std::process::id(), nonce));

    let write_result = (|| -> Result<(), String> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut temporary = options
            .open(&temporary_path)
            .map_err(|e| format!("cannot create {}: {}", temporary_path.display(), e))?;
        temporary
            .write_all(encoded.as_bytes())
            .map_err(|e| format!("cannot write {}: {}", temporary_path.display(), e))?;
        temporary
            .sync_all()
            .map_err(|e| format!("cannot sync {}: {}", temporary_path.display(), e))?;
        fs::rename(&temporary_path, &path)
            .map_err(|e| format!("cannot replace {}: {}", path.display(), e))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result?;

    // Ensure pre-existing files are also owner-readable and owner-writable only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        fs::set_permissions(&path, perms)
            .map_err(|e| format!("cannot secure {}: {}", path.display(), e))?;
    }

    Ok(())
}

/// Remove the fallback file when the keyring is working again.
pub(super) fn remove_api_key_file_fallback(config_file: &Path) {
    let path = api_key_fallback_path(config_file);
    if path.exists() {
        if let Err(e) = fs::remove_file(&path) {
            log::debug!("Could not remove fallback key file: {}", e);
        } else {
            log::info!("Removed file fallback key (keyring is now working).");
        }
    }
}
