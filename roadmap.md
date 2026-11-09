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

### 9.12.0 Library Release
- [x] **Photos Timeline** — Date headings and a year and month scrubber; only the months you scroll to are loaded.
- [x] **Video Playback** — In-app playback with hardware decoding, audio and subtitle tracks, and fullscreen (#80).
- [x] **Fullscreen Lightbox** — Photos and videos edge to edge (F11).
- [x] **Recognized Text (OCR)** — Select and copy text Immich found in photos (#85).
- [x] **Map** — Map page with clustered photos, and a location mini map in the lightbox (#86).
- [x] **Set Location** — Give one or many photos a location (#87).
- [x] **Partner Photos** — Browse a partner's photos, and show them in your Photos timeline (#236).
- [x] **Keyboard Shortcuts Dialog** — Every shortcut in one place (Ctrl+?).
- [x] **Star Ratings** — Rate photos and videos from 1 to 5 in the lightbox details pane.
- [x] **People in Photos** — See who is in a photo in the lightbox details pane, and open their photos.
- [x] **Immich Edits** — Photos cropped or rotated in Immich show their edited version.
- [x] **Faster Scrolling** — Thumbnails load where scrolling stops, and the library gives its memory back when closed.
- [x] **LibRaw 0.21.3** — Support for newer cameras such as the Sony A7 IV and Canon EOS R7.
- [x] **Album Management** — Add and remove photos, set covers, album options, delete albums, leave shared albums, download albums as zip, and see which albums a photo is in (#259).
- [x] **Change Dates** — Set or shift the date and time zone of many photos at once.
- [x] **Settings Redesign** — Plainer sections and labels, a Status headline, and short messages instead of dialogs.
- [x] **Search Filters Panel** — Filters open in a side panel beside the results, or slide in on phones.
- [x] **New App Icon** — A layered lotus redesign, with a symbolic version.
- [x] **Swipe Between Photos** — Photos follow touch and touchpad swipes in the lightbox and spring to the next one (#282).
- [x] **Large Photos** — Even 200-megapixel photos open quickly; full resolution loads when you zoom in (#278).
- [x] **Memory Settings** — A Memory Limit, and grid thumbnails decoded at tile size (#280, #281).

### 9.13.0 Everyday Immich Release
- [x] **Edit Photo** — Crop, rotate, flip, and straighten; adjust light and colour, filters, and redaction, saved as a stacked copy (#303).
- [x] **Tag People** — Name, move, remove, or draw faces; point at a face to see who it is (#304).
- [x] **Memories** — "On this day" and birthday memories above Photos, opening as a slideshow (#301).
- [x] **Live Photos** — Live and motion photos play their clip in the lightbox (#300).
- [x] **Stacks** — Bursts and RAW+JPEG pairs as one tile, with a stack strip in the lightbox (#302).
- [x] **Slideshows** — Crossfade, shuffle, and repeat (#306).
- [x] **Duplicates** — A page to review and resolve look-alike photos (#305).
- [x] **Similar Photos and Timeline** — View Similar Photos and View in Timeline in the photo menu (#307, #308).
- [x] **Album Activity** — Likes and comments on shared albums, in the album and the lightbox.
- [x] **Place Search** — Search places on the Map page and in Set Location, powered by Photon.
- [x] **Satellite Map** — Imagery and place names from VersaTiles.
- [x] **More Map Styles** — 11 more VersaTiles styles, an Appearance choice to keep maps light or dark, and styles that ship with Mimick so changes on VersaTiles' side can't break them.
- [x] **Map Credits** — Linked map attribution, and credits for map data and libraries in the About dialog.
- [x] **Real-Pixel Zoom** — Lightbox zoom shown in real pixels, reaching 2000% for high-resolution photos.
- [x] **Text Selection** — Select recognized text down to a word or a few characters.
- [x] **Touch and Keyboard** — Context menus open with a long-press, and the photo grid works from the keyboard (#293).
- [x] **Translatable Interface** — Ready for translations, with a progress chart in the README (#220).
- [x] **AUR Package** — `mimick` on the Arch User Repository (#295).
- [x] **AppImages** — x86_64 and aarch64 AppImages attached to each release (#295).

---

## In Pipeline

Finished on `main` and coming in the next release (see the Unreleased section of [CHANGELOG.md](CHANGELOG.md)).

- **Map Settings and album maps** — Show favorites, archived, partners', or shared albums' photos and limit the map by date; show one album's photos on the map; name the place under the pin in Set Location.
- **Refresh actions and 360° photos** — Refresh faces, metadata, thumbnails, or video from the photo menu; look around 360° photos in the viewer.
- **Albums** — Add photos to several albums at once, and see album counts on the Albums page.
- **Search** — Pick several people, tags, and albums, choose cameras and places from what's in your library, and find anyone on the People page.
- **Shared links** — QR codes, and adding photos to or taking them out of links made for some photos.
- **Account and notifications** — Profile picture, signed-in devices, API keys with Rotate, and Immich's notifications inbox with desktop notifications.
- **Photo calendar** — A square for each day of the year on Explore, by when photos were taken or uploaded.
- **Selection** — Ctrl+A selects the whole view; favorite, describe, and refresh many photos at once.
- **Folders** — Expand folders in place, copy a folder's path, and jump to a typed or pasted path (Ctrl+L).
- **Look and help** — Follow System, Light, or Dark, with Warm White, Lavender, Mint, Black, Midnight, and Mocha palettes; About with What's New and debug info; detailed logs and a redacted log in diagnostics; bug and feature forms on GitHub.
- **Timelines everywhere** — Albums, people, tags, Favorites, Archive, and Trash scroll like Photos, with a scrollbar that knows the full height; Missing Location, places, and map clusters too.
- **Utilities** — Sizes and filters on Large Files; dates, Add Location, and places taken nearby in Missing Location; and Unusual Dates to fix wrong dates.
- **Stacks** — Keep one photo and trash the rest, or upload files into a photo's stack.
- **Copy and paste** — Ctrl+V uploads copied files and screenshots to the open album or Photos; Ctrl+C copies photos, and pasting them into another album adds them there.
- **Phones** — A search button in the header, bottom-bar tabs that don't pile up Back steps, and album pages that fit a 360 px screen.
- **Smaller touches** — Crop a new profile picture, a play/pause badge and Up/Down volume in videos, OpenTopoMap and Humanitarian map styles, icons in menus, and the library view on by default.

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
- [x] **Arch AUR submission** — Published to the AUR as `mimick`.
- [x] **ARM64 AppImage** — Built on an ARM runner alongside the x86_64 AppImage, for Raspberry Pi, ARM desktops, and phones.

### Longer-Term Product Ideas

- [x] **Complete folder sync** — The Full sync method reflects remote additions locally, and deletion mirroring reflects deletions.
- [ ] **Exponential backoff** — Smarter retry scheduling instead of immediate replay on restart.
- [x] **Progress notifications** — One summary notification per sync batch, with counts and outcomes.
- [ ] **Tray icon dynamic states** — Distinct icons for idle, syncing, paused, and error conditions.
- [x] **Background Sync Toggle** — Make background sync optional via a toggle in Settings > Behavior; enabled by default.
- [x] **Library View** — Browse Immich in the app, switching between Remote, Local, and Unified sources (9.5.0).
