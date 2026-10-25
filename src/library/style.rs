//! Library view CSS: subtle pulsing placeholder for loading thumbnails,
//! a muted error tile, and a rounded transfer progress treatment.
//!
//! Registered exactly once per process via `std::sync::OnceLock`.

use std::sync::OnceLock;

use gtk::CssProvider;
use gtk::gdk;
use gtk::style_context_add_provider_for_display;

const LIBRARY_CSS: &str = r#"
@keyframes mimick-pulse {
    0%   { background-color: alpha(@view_fg_color, 0.06); }
    50%  { background-color: alpha(@view_fg_color, 0.14); }
    100% { background-color: alpha(@view_fg_color, 0.06); }
}

picture.mimick-thumbnail-loading {
    background-color: alpha(@view_fg_color, 0.08);
    animation: mimick-pulse 1.4s ease-in-out infinite;
}

picture.mimick-thumbnail-loaded {
}

picture.mimick-thumbnail-error {
    background-color: alpha(@error_color, 0.18);
}

overlay.mimick-cell {
}

box.mimick-empty {
    padding: 32px;
}

label.mimick-empty-title {
    font-size: 1.2em;
    font-weight: bold;
}

label.mimick-empty-subtitle {
    opacity: 0.65;
}

label.mimick-timeline-banner {
    font-size: 1.05em;
    font-weight: 600;
    padding: 4px 8px;
    background-color: alpha(@accent_bg_color, 0.10);
    border-bottom: 1px solid alpha(@view_fg_color, 0.10);
}

image.mimick-status-badge {
    opacity: 0.90;
    color: white;
    -gtk-icon-shadow: 0 1px 3px rgba(0, 0, 0, 0.8);
}

image.mimick-video-badge {
    opacity: 0.92;
    color: white;
    -gtk-icon-shadow: 0 2px 4px rgba(0, 0, 0, 0.8);
}

picture.mimick-person-avatar {
    border-radius: 9999px;
    background-color: alpha(@view_fg_color, 0.08);
}

picture.mimick-explore-tile,
overlay.mimick-explore-tile {
    border-radius: 6px;
    background-color: alpha(@view_fg_color, 0.08);
}

box.mimick-explore-spacer {
    min-width: 130px;
    min-height: 100px;
}

window.mimick-wide box.mimick-explore-spacer {
    min-width: 220px;
    min-height: 150px;
}

overlay.mimick-see-more-tile {
    border-radius: 6px;
    background-color: alpha(@view_fg_color, 0.06);
    border: 1px dashed alpha(@view_fg_color, 0.2);
}

box.mimick-stat-card {
    border-radius: 12px;
    padding: 14px;
    background: alpha(@accent_bg_color, 0.08);
}

box.mimick-stat-card.photo-card {
    background: alpha(#6a9fc7, 0.14);
}

box.mimick-stat-card.video-card {
    background: alpha(#d16c96, 0.14);
}

box.mimick-stat-card.storage-card {
    background: alpha(#7bb876, 0.14);
}

label.mimick-stat-value {
    font-size: 1.6em;
    font-weight: 800;
}

label.mimick-stat-label {
    font-size: 0.85em;
    font-weight: 600;
    opacity: 0.60;
}

progressbar.mimick-quota-bar trough {
    min-height: 6px;
    border-radius: 999px;
    background: alpha(@window_fg_color, 0.10);
}

progressbar.mimick-quota-bar progress {
    min-height: 6px;
    border-radius: 999px;
    background: mix(@accent_bg_color, #d16c96, 0.45);
}

box.mimick-version-badge {
    border-radius: 999px;
    padding: 4px 12px;
    background: alpha(@accent_bg_color, 0.12);
}

.mimick-masonry-canvas {
    background-color: @view_bg_color;
}

/* Hide the horizontal scrollbar on the search form.  hscrollbar_policy
   is Automatic (not Never) so the scrolled window's minimum width is
   decoupled from the wide AdwEntryRow children (~434px).  The scrollbar
   itself is never needed visually. */
scrolledwindow.mimick-search-scroll scrollbar.horizontal {
    min-height: 0;
    opacity: 0;
}

picture.mimick-grid-thumb {
    min-width: 80px;
    min-height: 60px;
}

window.mimick-wide picture.mimick-grid-thumb {
    min-width: 356px;
    min-height: 200px;
}

picture.mimick-thumbnail-square,
picture.mimick-thumbnail-square.mimick-thumbnail-loading,
picture.mimick-thumbnail-square.mimick-thumbnail-loaded,
picture.mimick-thumbnail-square.mimick-thumbnail-error {
    border-radius: 0;
}

/* ── Press feedback ───────────────────────────────────────────────
   Subtle scale-down on press for all interactive elements.
   GTK4 natively skips CSS transitions when gtk-enable-animations
   is off, so accessibility is respected automatically.

   Covers: header-bar buttons, dialog buttons, sidebar rows,
   settings switch/action/spin rows, lightbox actions, grid tiles,
   explore/album tiles, and any widget with .mimick-pressable.  */

button,
.mimick-pressable,
menubutton > button,
row.activatable {
    transition: all 150ms cubic-bezier(0.25, 0.46, 0.45, 0.94);
}

button:active,
.mimick-pressable:active,
menubutton > button:active,
row.activatable:active {
    transform: scale(0.97);
    opacity: 0.88;
}

overlay.mimick-cell {
    transition: transform 80ms ease-out, opacity 80ms ease-out;
}

overlay.mimick-cell:active {
    transform: scale(0.96);
    opacity: 0.90;
}

picture.mimick-explore-tile,
overlay.mimick-explore-tile,
picture.mimick-person-avatar {
    transition: transform 80ms ease-out, opacity 80ms ease-out;
}

picture.mimick-explore-tile:active,
overlay.mimick-explore-tile:active,
picture.mimick-person-avatar:active {
    transform: scale(0.95);
    opacity: 0.88;
}

/* Details-pane group accents. The icon classes paint just the prefix icon,
   keeping the row text in the standard foreground for legibility. */
.mimick-detail-icon {
    min-width: 28px;
    min-height: 28px;
    padding: 6px;
    border-radius: 10px;
    margin: 2px 0;
    -gtk-icon-size: 16px;
}

.mimick-accent-camera {
    color: @accent_color;
    background-color: alpha(@accent_bg_color, 0.18);
}

.mimick-accent-image {
    color: @success_color;
    background-color: alpha(@success_bg_color, 0.20);
}

.mimick-accent-location {
    color: @warning_color;
    background-color: alpha(@warning_bg_color, 0.22);
}

/* Tag pills in the details pane: a split chip. The left half (color dot + name)
   searches the tag; the right half is a full-height 32 px remove target. */
progressbar.mimick-transfer-mini > trough,
progressbar.mimick-transfer-mini > trough > progress {
    min-height: 4px;
}

image.mimick-person-favorite {
    padding: 6px;
    margin: 4px;
    border-radius: 999px;
    color: white;
    background-color: @red_3;
    box-shadow: 0 1px 3px alpha(black, 0.35);
}

flowboxchild.mimick-tag-slot,
flowboxchild.mimick-tag-slot:hover,
flowboxchild.mimick-tag-slot:focus {
    padding: 0;
    background: none;
    outline: none;
}

box.mimick-tag-chip {
    border-radius: 999px;
    background-color: alpha(@accent_bg_color, 0.14);
    border: 1px solid alpha(@accent_bg_color, 0.35);
}

box.mimick-tag-chip > button.mimick-tag-name {
    min-height: 32px;
    padding: 0 10px 0 12px;
    border-radius: 999px 0 0 999px;
}

box.mimick-tag-chip > button.mimick-tag-remove {
    min-width: 32px;
    min-height: 32px;
    padding: 0;
    border-radius: 0 999px 999px 0;
    border-left: 1px solid alpha(@accent_bg_color, 0.35);
    -gtk-icon-size: 16px;
}

box.mimick-tag-chip > button.mimick-tag-name:hover {
    background-color: alpha(@accent_bg_color, 0.22);
}

box.mimick-tag-chip > button.mimick-tag-remove:hover {
    background-color: alpha(@destructive_bg_color, 0.20);
    color: @destructive_color;
}

scrolledwindow.mimick-details-pane {
    background: alpha(@window_bg_color, 0.30);
}

scrolledwindow.mimick-details-pane > viewport > box {
    padding: 4px;
}

/* Lightbox image-load spinner. The Mimick app icon eases in from slow
   rotation, accelerates through the middle of the cycle, then eases back
   out — giving the breathing rhythm requested for "ramp up / ramp down". */
@keyframes mimick-icon-spin {
    0%   { transform: rotate(0deg); }
    20%  { transform: rotate(40deg); }
    50%  { transform: rotate(220deg); }
    80%  { transform: rotate(320deg); }
    100% { transform: rotate(360deg); }
}

image.mimick-loader-icon {
    opacity: 0.85;
    -gtk-icon-shadow: 0 2px 12px alpha(black, 0.45);
    animation: mimick-icon-spin 2.4s ease-in-out infinite;
}

picture.mimick-lightbox-picture {
    background-color: alpha(@view_fg_color, 0.05);
}

picture.mimick-video-surface {
    background-color: black;
}

box.mimick-fullscreen picture.mimick-lightbox-picture {
    background-color: black;
}

box.mimick-video-controls {
    padding: 4px 8px;
    border-radius: 12px;
}

box.mimick-preview-unavailable {
    min-width: 240px;
    padding: 20px;
    border-radius: 8px;
    background-color: alpha(@window_bg_color, 0.92);
    border: 1px solid alpha(@view_fg_color, 0.16);
}

/* ── Drag-and-drop overlay ───────────────────────────────────────
   Shown when files are dragged over the window.  */

box.mimick-drop-overlay {
    background: alpha(@accent_bg_color, 0.50);
    border: none;
    border-radius: 0;
    margin: 0;
    transition: opacity 200ms ease-in-out;
}

box.mimick-drop-overlay image {
    -gtk-icon-size: 48px;
    opacity: 0.85;
    color: @accent_fg_color;
}

box.mimick-drop-overlay label {
    font-size: 1.1em;
    font-weight: 600;
    opacity: 0.9;
    color: @accent_fg_color;
}

/* ── Drag badge (multi-file count) ─────────────────────────────── */

box.mimick-drag-badge {
    background: @accent_bg_color;
    color: @accent_fg_color;
    border-radius: 999px;
    padding: 2px 8px;
    font-weight: bold;
    font-size: 11px;
    min-width: 20px;
    min-height: 20px;
}

/* ── Album sidebar row drop-hover ──────────────────────────────── */

row.mimick-album-drop-hover {
    background: alpha(@accent_bg_color, 0.25);
    border-radius: 6px;
    transition: background 150ms ease-in-out, transform 150ms ease-in-out;
    transform: scale(1.03);
}

/* ── Map ───────────────────────────────────────────────────────── */

button.mimick-map-cluster {
    min-width: 34px;
    min-height: 34px;
    padding: 0 8px;
    border-radius: 999px;
    border: 2px solid white;
    background: @accent_bg_color;
    color: @accent_fg_color;
    font-weight: bold;
    font-size: 0.9em;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.35);
}

button.mimick-map-cluster-m {
    min-width: 40px;
    min-height: 40px;
}

button.mimick-map-cluster-l {
    min-width: 46px;
    min-height: 46px;
    font-size: 1em;
}

button.mimick-map-cluster-xl {
    min-width: 52px;
    min-height: 52px;
    font-size: 1.05em;
}

button.mimick-map-cluster:hover {
    background: mix(@accent_bg_color, white, 0.2);
}

button.mimick-map-photo {
    padding: 0;
    border-radius: 999px;
    border: 2px solid @accent_bg_color;
    background: @accent_bg_color;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
}

button.mimick-map-photo:hover {
    border-color: white;
}

button.mimick-map-cluster.selected,
button.mimick-map-photo.selected {
    border-color: white;
    box-shadow: 0 0 0 4px alpha(@accent_bg_color, 0.5), 0 1px 6px rgba(0, 0, 0, 0.5);
}

box.mimick-map-here {
    min-width: 16px;
    min-height: 16px;
    border-radius: 999px;
    border: 3px solid white;
    background: #3584e4;
    box-shadow: 0 0 0 6px alpha(#3584e4, 0.25), 0 1px 4px rgba(0, 0, 0, 0.4);
}

box.mimick-map-pin {
    min-width: 18px;
    min-height: 18px;
    border-radius: 999px;
    border: 3px solid white;
    background: @accent_bg_color;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.45);
}

button.mimick-minimap {
    padding: 0;
    border-radius: 12px;
}

.mimick-location-map {
    border-radius: 12px;
}

.mimick-map-status {
    margin: 24px;
    padding: 12px 24px;
}
"#;

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
