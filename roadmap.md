# Mimick for Linux: Feature Roadmap

Per-release highlights and live open issues are on the [roadmap page](https://mimick.nicx.dev/roadmap.html).

## Completed

### Core Sync Engine
- [x] Monitor directories via Linux `inotify` (`notify` crate).
- [x] File write-completion detection (size stabilisation over 3 consecutive polls).
- [x] SHA-1 checksumming per file for Immich deduplication (64KB chunked, low RAM).
- [x] Upload-only sync by default; per-folder Download Only and Full sync methods since 9.5.4.
- [x] File type whitelist (JPG, PNG, HEIC, MP4, MOV, GIF, WEBP, TIFF, RAW, ARW, DNG). Sidecars ignored.
- [x] Configurable streaming upload workers (1-10, default 3) with constant RAM use regardless of file size.
- [x] Persistent retry queue (`~/.cache/mimick/retries.json`) — failed uploads survive reboots.
- [x] **Offline Sync Stability** — Prevented already-synced files from being re-queued for reassociation when the application is offline or unable to reach the server.
- [x] **Accurate Progress Tracking** — Fixed the processed file counter to only increment on successful uploads, preventing "ghost" progress during network outages.

### Immich API Client
- [x] Smart URL routing — LAN first, WAN fallback.
- [x] Pre-upload deduplication via SHA-1 + 409 Conflict detection.
- [x] Multipart streaming upload (disk → network, no full RAM load).
- [x] ISO 8601 UTC timestamps (no chrono dependency, pure arithmetic).
- [x] Album auto-creation from local folder name.
- [x] Custom album selection per watch folder (existing or new).
- [x] HTTP error handling (413, 502, 504).

### Configuration & Security
- [x] Config file at `~/.config/mimick/config.json` (serde_json).
- [x] API key stored via `oo7` keyring crate (encrypted portal file in Flatpak, D-Bus Secret Service native) -- never written to disk in plain text.
- [x] Multiple watch directories with per-folder album config.
- [x] `WatchPathEntry` supports both plain path strings and per-folder album configs.

### Settings UI
- [x] GTK4 + Libadwaita native UI with `adw::PreferencesWindow` for native dark mode and adaptive layouts.
- [x] Adaptive Settings Window — Refactored from `adw::ApplicationWindow` to target 350-360px minimum width, ensuring full support for mobile displays like Phosh.
- [x] Internal/External URL fields with toggles (at least one must stay enabled — validated).
- [x] Test Connection button (async ping, no UI freeze).
- [x] Watch folders list with per-row album picker button (searchable modal dialog, create-new inline).
- [x] Live sync status row and progress bar (polling `status.json`).
- [x] Save and Restart flow.
- [x] Split Setup / Controls Window — Separated configuration from live actions and kept footer-style actions visible while scrolling.
- [x] Adaptive Action Buttons — Layout uses `gtk::FlowBox` to auto-stack action buttons (Sync, Pause, Inspector, etc.) vertically on narrow screens.
- [x] Expander-based Folder Rows — Folder rows use `adw::ExpanderRow` to collapse advanced settings and maximize mobile real estate.

### System Tray
- [x] StatusNotifierItem tray via `ksni` crate.
- [x] Graceful fallback when `org.kde.StatusNotifierWatcher` is unavailable (GNOME without extension).

### Desktop Integration
- [x] `.desktop` file with Settings and Quit actions (`setup/dev.nicx.mimick.desktop`).
- [x] Native desktop notifications (`gio::Notification` via XDG notification portal).

### Rust Port (v2.0)
- [x] Full rewrite from Python to Rust (Tokio + GTK4-rs + Libadwaita-rs).
- [x] No Python runtime dependency — single statically-linked binary.
- [x] 47 unit tests across 13 modules (`api_client`, `autostart`, `config`, `diagnostics`, `main`, `monitor`, `queue_manager`, `runtime_env`, `settings_window`, `startup_scan`, `state_manager`, `sync_index`, `watch_path_display`).
- [x] All GTK4 widgets updated to 4.10+ standards (no deprecated `ComboBoxText`, `MessageDialog`).

---

## In Pipeline

Finished on `main` and coming in the next release (see the Unreleased section of [CHANGELOG.md](CHANGELOG.md)).

- [x] **Photos Timeline** — Date headings and a year and month scrubber; only the months you scroll to are loaded.
- [x] **Video Playback** — In-app playback with hardware decoding, audio and subtitle tracks, and fullscreen (#80).
- [x] **Fullscreen Lightbox** — Photos and videos edge to edge (F11).
- [x] **Recognized Text (OCR)** — Select and copy text Immich found in photos (#85).
- [x] **Map** — Map page with clustered photos, and a location mini map in the lightbox (#86).
- [x] **Set Location** — Give one or many photos a location (#87).
- [x] **Partner Photos** — Browse a partner's photos, and show them in your Photos timeline (#236).
- [x] **Keyboard Shortcuts Dialog** — Every shortcut in one place (Ctrl+?).
- [x] **Star Ratings** — Rate photos and videos from 1 to 5 in the lightbox details pane.
- [x] **Immich Edits** — Photos cropped or rotated in Immich show their edited version.
- [x] **Faster Scrolling** — Thumbnails load where scrolling stops, and the library gives its memory back when closed.
- [x] **LibRaw 0.21.3** — Support for newer cameras such as the Sony A7 IV and Canon EOS R7.

---

## Planned

### Priority Now

- [x] **Queue Inspector** — Failed items and recent queue activity are visible from the Controls page, with retry tools for recovery.
- [x] **Retry Controls** — `Retry all`, single-item `Retry`, and `Clear failed queue` are available from the queue inspector.
- [x] **Pause / Resume Sync** — Global pause works from the tray and settings window, with visible paused status and reason.
- [x] **Sync Now** — Manual rescan is available from the Controls page and tray menu.
- [x] **Per-Folder Rules** — Hidden-path filtering, extension allowlists, temp-file ignores, and optional max file size limits are supported.
- [x] **Diagnostics Bundle** — Logs, config/state snapshots, and recent queue details can be exported into one support-friendly bundle.
- [x] **Network / Power Awareness** — Uploads can defer on metered connections or while running on battery power.

### Next Wave
- [x] **Sync Deletes** — **Mirror Folder Deletions to Album** and **Mirror Album Deletions to Folder** toggles in the per-folder rules.
- [x] **fix album lable type** currently the ablum even if selected from the available album menu is marked as custom album on restart .. revise this
- [x] **Notifications That Matter** — Summaries for sync success/failure and connectivity loss replaces per-file spam.
- [x] **Upload Limits** — User-configurable concurrency limit and quiet hours are now supported in the settings window.
- [x] **Health Dashboard** — Show last successful sync, current server route, watched folder count, pending queue size, retry count, and latest error.
- [x] **Permission Health Checks** — Detect broken Flatpak portal access or lost folder permissions and guide the user to reauthorize them.
- [x] **Safer Startup Catch-Up Controls** — Let users choose full catch-up, recent-only catch-up, or new-files-only behavior.
- [x] **Per-Folder Status** — Track last sync time, pending count, target album, and last error per watch path.

### UX Improvements

- [x] **First-Run Wizard** — Opens settings automatically when no API key is configured, shows a welcome banner, and disables "Save & Restart" until valid credentials are entered.
- [x] **Better Album Picker** — Searchable modal dialog with inline create-new and a clear "use folder name" default option replaces the old dropdown.
- [x] **Split Setup / Controls Window** — Separate configuration from live actions and keep footer actions visible while scrolling.
- [ ] **Status in Tray** — Surface idle / syncing / paused / offline / error state directly in the tray menu and tooltip.
- [x] **Actionable Errors** — Translate generic failures into concrete guidance like invalid API key, missing album, network timeout, or folder access loss.
- [ ] **Dry Run / Preview Mode** — Show what would be uploaded before enabling a new folder or changing rules.
- [ ] **Verify Existing Remote State** — Audit a folder against the local sync index and highlight drift or mismatches.

### Platform & Packaging

- [ ] **Flatpak manifest hardening** — Keep improving portal-first packaging and release reliability for the hosted Flatpak repo.
- [ ] **Arch AUR submission** — Publish PKGBUILD to AUR as `mimick`.
- [ ] **ARM64 AppImage** — Cross-compile and package for Raspberry Pi / ARM desktops.

### Longer-Term Product Ideas

- [x] **Complete folder sync** — The Full sync method reflects remote additions locally, and deletion mirroring reflects deletions.
- [ ] **Exponential backoff** — Smarter retry scheduling instead of immediate replay on restart.
- [x] **Progress notifications** — One summary notification per sync batch, with counts and outcomes.
- [ ] **Tray icon dynamic states** — Distinct icons for idle, syncing, paused, and error conditions.
- [x] **Background Sync Toggle** — Make background sync optional via a toggle in Settings > Behavior; enabled by default.
- [x] **Library View** — Browse Immich in the app, switching between Remote, Local, and Unified sources (9.5.0).
