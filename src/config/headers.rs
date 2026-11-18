use gettextrs::gettext;
use serde::{Deserialize, Serialize};

/// Target network route for a custom HTTP header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeaderScope {
    /// Send only to the external (WAN) server address (e.g. Cloudflare Access, reverse proxies).
    #[default]
    Wan,
    /// Send to both internal (LAN) and external (WAN) server addresses.
    All,
    /// Send only to the internal (LAN) server address.
    Lan,
}

impl HeaderScope {
    pub const ALL: [HeaderScope; 3] = [HeaderScope::Wan, HeaderScope::All, HeaderScope::Lan];

    /// User-facing label in the selection combo.
    pub fn label(&self) -> String {
        match self {
            Self::Wan => gettext("WAN only (External)"),
            Self::All => gettext("All (LAN & WAN)"),
            Self::Lan => gettext("LAN only (Internal)"),
        }
    }

    /// Short label displayed in header list badges.
    pub fn short_label(&self) -> String {
        match self {
            Self::Wan => gettext("WAN"),
            Self::All => gettext("ALL"),
            Self::Lan => gettext("LAN"),
        }
    }

    /// Whether this scope applies given whether the destination URL is external (WAN).
    pub fn matches_route(&self, is_external: bool) -> bool {
        match self {
            Self::All => true,
            Self::Wan => is_external,
            Self::Lan => !is_external,
        }
    }
}

/// A custom HTTP request header attached to Immich server requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomHeader {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub scope: HeaderScope,
}

impl CustomHeader {
    /// Create a new custom header with validation.
    pub fn new(name: impl Into<String>, value: impl Into<String>, scope: HeaderScope) -> Self {
        Self {
            name: name.into().trim().to_string(),
            value: value.into().trim().to_string(),
            scope,
        }
    }

    /// Check if the header name is valid ASCII and non-empty.
    pub fn is_valid_name(&self) -> bool {
        !self.name.is_empty()
            && self
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }
}
