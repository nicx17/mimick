//! Library view CSS: subtle pulsing placeholder for loading thumbnails,
//! a muted error tile, and a rounded transfer progress treatment.
//!
//! Registered exactly once per process via `std::sync::OnceLock`.

use std::sync::OnceLock;

use gtk::CssProvider;
use gtk::gdk;
use gtk::style_context_add_provider_for_display;

const LIBRARY_CSS: &str = include_str!("library.css");

static REGISTERED: OnceLock<()> = OnceLock::new();

/// Install the library-view stylesheet on the default display. Idempotent.
pub fn ensure_registered() {
    REGISTERED.get_or_init(|| {
        let Some(display) = gdk::Display::default() else {
            log::warn!("No default GDK display; library CSS not registered");
            return;
        };

        let provider = CssProvider::new();
        provider.load_from_string(LIBRARY_CSS);
        style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
}
