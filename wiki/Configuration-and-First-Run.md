# Configuration and First Run

Welcome to Mimick for Linux! This guide provides detailed instructions on how to configure and use the application to automatically back up your local photo and video directories to your Immich server.

---

## 1. Getting Started

### The System Tray Icon

Once the application is running, the Mimick tray icon will appear in your system tray (usually at the top right on GNOME/KDE).

*If you are using GNOME and don't see system tray icons, ensure you have the "AppIndicator and KStatusNotifierItem Support" GNOME extension enabled. Stock GNOME does not support StatusNotifier tray icons out of the box.*

Clicking on the tray icon reveals a menu:

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/tray_icon_menu.png" width="40%" alt="Tray icon menu" />
</div>

* **Settings**: Opens the configuration and status window.
* **Pause / Resume**: Temporarily stop uploads without quitting the app, then continue later.
* **Sync Now**: Trigger an immediate rescan of watched folders and queue any eligible files right away.
* **Quit**: Safely shuts down the application and stops all background syncing.

You can also use the launcher action for **Quit Mimick** to stop the already-running app without opening the settings window.

The tray icon uses a single static icon regardless of sync state. To check current status, open the Settings window and look at the Status page. Dynamic tray icon states (idle / syncing / paused / error) are not yet implemented.

---

## 2. Configuring the Application

### Accessing Settings

Right-click the tray icon and select **Settings**, or launch with `mimick --settings`.

The settings window is split into two pages:

* **Settings**: Server, Watch Folders (with each folder's rules), Uploads, App, the library sections when the Library View is on, and Troubleshooting
* **Status**: a headline with **Sync Now**, **Pause**, and **Queue…**, each watched folder's state, and failed uploads and errors when there are any

New users see three numbered steps at the top of Settings. On narrow windows the Status/Settings switcher moves to the bottom.

The window follows your desktop appearance preference, so it can render in either light mode or dark mode depending on the system theme.

### Server

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/settings_pane_showing_connnectivity_config_details.png" width="60%" alt="Connectivity settings" />
</div>

1. **Internal URL (LAN)**: Enter the local IP address of your Immich server (e.g., `http://192.168.1.10:2283`). Can be toggled on/off.
2. **External URL (WAN)**: Enter the public address (e.g., `https://immich.yourdomain.com`). Can be toggled on/off. At least one URL must always remain enabled.
3. **API Key**:
    * Open your Immich Web Interface in a browser.
    * Go to **Account Settings** → **API Keys**.
    * Click **New API Key**, give it a name (like "Linux Desktop"), and click Create.
    * Make sure the key includes the required scopes for your setup, or grant `all` for full functionality. See [API Key Security & Required Permissions](#api-key-security--required-permissions) for the granular per-feature mapping.
    * Copy the key and paste it into the API Key field in Mimick.
    * *The key is stored securely using the `oo7` crate (encrypted portal file in Flatpak, or D-Bus Secret Service native). On non-GNOME/KDE desktops you may need to configure the Secret portal first -- see [Keyring Setup](Keyring-Setup).*

**Test Connection**: Verifies connectivity by pinging the Immich `/api/server/ping` endpoint, confirming a valid `{"res": "pong"}` JSON response to ensure you are talking to an actual Immich server rather than a captive portal. The result shows in the **Connection** row.

**Save and Connect**: Saves the URLs and API key and connects with them. These are the only settings that wait for a button; see [Saving Changes](#saving-changes).

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/settings_pane_connection_successful_dialog.png" width="60%" alt="Connection test successful" />
</div>

### Choosing Folders to Watch

1. Under **Watch Folders**, click **Add Folder…**.
2. Select a local directory (e.g., `~/Pictures`, `~/Videos/Exports`).
3. The application monitors these folders recursively.
4. **Upload Target**: Expand a folder's row and press the **Upload Target** button. Pick **Library (No Album)**, **Default (Folder Name)** to name the album after each folder, or an existing album; type a name to filter, or to create a new album.
5. **Folder Rules**: In the expanded row, press **Rules** next to **Folder Rules** to open the rules dialog:
    * **Sync**: **Sync Method** (**Full Sync**, **Upload Only**, or **Download Only**) and the folder's own **When Mimick Starts, Check**
    * **Deletions**: **Delete from Immich Too** and **Delete from This Folder Too** (not available yet)
    * **Files**: **Skip Hidden Files**, **Upload XMP Sidecars**, **Largest File in MB**, and **Only These Types**, a comma-separated allowlist like `jpg, png, mp4`

| Watch Folders List | Album Selection | Album Search |
| :---: | :---: | :---: |
| ![Watch folders](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/watching_folders_list_in_settings_showing_folder_details.png) | ![Album selection](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/album_selection_menu_for_watching_folder.png) | ![Album search](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/watching_folder_album_selection_menu_with_search_feat.png) |

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/album_specific_rule_dialog.png" width="50%" alt="Folder rules dialog" />
</div>

Flatpak builds only have access to folders that you add through this picker. If you are upgrading from an older build that had wider filesystem access, remove and re-add existing watch folders once.

Portal-backed folders may appear by folder name in the UI and logs instead of showing the raw `/run/user/.../doc/...` sandbox path.

### Nested Watch Folders

You can add both a parent folder and a subfolder as separate watch entries. Mimick uses the most specific matching path when deciding which album and rules to apply to a file.

For example, if you watch `~/Pictures` (album: **All Photos**) and also `~/Pictures/iPhone` (album: **iPhone**), files inside `~/Pictures/iPhone` use the **iPhone** album and its rules. Files elsewhere under `~/Pictures` use the **All Photos** album.

There is no limit on nesting depth. If a file matches multiple watch paths, the longest matching prefix wins.

### Startup Behavior

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/settings_pane_showing_behaviour_settings.png" width="60%" alt="Behaviour settings" />
</div>

Use the **Run on Startup** switch in the **App** section if you want Mimick to launch automatically when you log in.

* Flatpak builds ask the desktop for permission using the background portal.
* Native builds create `~/.config/autostart/dev.nicx.mimick.desktop`.

In the **Uploads** section you can also turn on:

* **Pause on Metered Networks**: Mimick defers uploads while the system reports a metered connection (through the network portal in the Flatpak).
* **Pause on Battery**: Mimick defers uploads while the system appears to be running on battery.

### Background Sync

The **Background Sync** switch controls whether Mimick continues running when the settings window is closed.

* **Enabled**: closing the window hides it. Mimick keeps syncing in the background and remains accessible from the tray.
* **Disabled (default)**: closing the window quits the application entirely if no upload is in progress. Mimick only runs while the window is open.

If you disable Background Sync and also enable Run on Startup, Mimick will launch on login but quit as soon as you close the window. Re-opening it from the launcher starts it again.

### Quiet Hours

The **Quiet Hours** switch in the Uploads section pauses uploads during a nightly window. Set **From** and **Until** to whole hours, shown as times like 22:00 (using your local clock).

* Uploads in progress when the quiet window begins are paused at the next worker cycle, not interrupted mid-file.
* Uploads resume automatically when the local clock passes the **Until** hour.
* Wrapping windows are supported: **From** 22:00 **Until** 06:00 pauses until 06:00 the next morning.
* Setting **From** and **Until** to the same hour disables the window even if the switch is on.
* Quiet hours interact with other pause conditions — if the app is also manually paused or paused by a metered-network policy, it stays paused for the other reason after the quiet window ends.

The quiet hours check runs on the local system clock. Timezone changes while the app is running take effect on the next worker cycle.

### When Mimick Starts, Check

**When Mimick Starts, Check** in the Uploads section controls how thoroughly Mimick rescans watch folders when it launches. Each folder can choose its own in its Folder Rules.

| Mode | Behaviour |
| :--- | :--- |
| **All Files** | Scans every file in every watch folder regardless of when it was last modified. Suitable if you want a complete audit on each launch. Slowest on large folders. |
| **Changed in the Last 7 Days** | Scans files modified in the last 7 days. Faster than All Files but may miss older files added while the app was not running. |
| **New Files Only** | Only processes files not already present in the local sync index. Fastest; does not re-check files that were previously seen even if their content changed. |

The default is **All Files**. If startup is slow on a large library you can switch to **New Files Only** once the initial sync is complete.

### Saving Changes

Settings save as soon as you change them and apply to the running app immediately, including watch folders, upload targets, folder rules, parallel uploads, quiet hours, and pause policies.

Server details (API key and server URLs) are the exception: they take effect only when you press **Save and Connect**, so a half-typed address or key is never used.

### Closing vs Quitting

* Closing the window keeps Mimick running in the background when **Background Sync** is on, and quits it when it's off (see [Background Sync](#background-sync)).
* **Quit Mimick** in the main menu fully exits Mimick.

### Status Page

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/status_pane_sync_status_health_dashboard_actions.png" width="60%" alt="Status and health dashboard" />
</div>

The **Status** page groups the live actions you may want while Mimick is already running:

* **Sync Now** to trigger an immediate watched-folder scan
* **Pause / Resume** to stop and continue uploads manually
* **Queue…** to open the Queue Inspector for failure recovery
* **Retry** next to failed uploads, shown only when some failed

**Export Diagnostics** for support bundles is in **Settings → Troubleshooting**.

### Queue Inspector and Recovery

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/queue_inspector_dialog.png" width="60%" alt="Queue inspector" />
</div>

Inside it you can:

* review recent queue events from the current session
* see failed items waiting to be retried
* retry one failed item
* retry all failed items
* clear the failed queue

This is useful when a server outage, permission issue, or bad file temporarily blocks uploads.

### Diagnostics Export

Use **Export Diagnostics** in the settings window when you need a support snapshot.

The export creates a timestamped `mimick-diagnostics-*` folder containing redacted support files:

* `summary.txt`
* `config.redacted.json`
* `status.redacted.json`
* `retries.redacted.json`
* `synced_index.redacted.json`
* `privacy-note.txt`

API keys, raw logs, full local paths, and raw server URLs are intentionally omitted.

---

## Library View (Optional)

Mimick includes an opt-in library browser for albums, Explore, and search.

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/settings_pane_showing_library_settings_and_watch_folders.png" width="60%" alt="Library settings" />
</div>

Turn on **Settings → App → Library View** (the tray menu entry appears after a restart). See the [Library View user guide](Library-View-User-Guide) for full usage documentation.

| Photos Page | Explore Page |
| :---: | :---: |
| ![Photos](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/photos_page_view_sidebar_on.png) | ![Explore](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/explore_page_view_showing_people_places_sidebar_on.png) |

**Extra permissions:** Library browsing requires **Asset Read** and downloads require **Asset Download**.

---

## 3. How Syncing Works

### Automatic Detection

Once configured, the application runs silently in the background. It handles syncing in two ways:

1. On startup, Mimick rescans watched folders for media that has not been synced yet.
2. While running, Mimick watches those folders for newly added or changed media.

For live changes, `mimick` detects files via filesystem monitoring:

1. Waits for the file size to stabilise (file is fully written to disk).
2. Calculates a SHA-1 checksum for deduplication.
3. Streams the file to Immich using the standard asset API.
4. Adds the asset to the configured album.

### Existing Files and Reassignment

Mimick keeps a local sync index so it can avoid reprocessing files that are already known to be synced.

* Unchanged files that were already synced are skipped during startup rescans.
* Files whose content changed are rehashed and uploaded again.
* If you change the target album for a watched folder, Mimick can reassociate unchanged files to the new album on a later startup without needing to reupload the media data.
* If the previously targeted album was deleted, Mimick refreshes the album mapping and retries using the current configured album name.

### Sync Status

The headline at the top of the **Status** page shows what is currently happening:

* **Up to Date** — Nothing is uploading. Shows when the last upload finished.
* **Uploading 3 of 40** — Shows the current filename and a progress bar for the active batch.
* **Paused** — Mimick is intentionally holding uploads. The line below shows the pause reason, such as a manual pause, quiet hours, metered network, or battery-power policy.
* **Waiting for Immich** — Some uploads failed; they retry when the server is reachable.

### Offline Reliability

If an upload fails, the file is saved to `~/.cache/mimick/retries.json`. On the next launch, any persisted retries are automatically re-queued and uploaded.

Files blocked by folder rules are skipped before they ever enter the queue. Temporary files are also ignored until the final media file exists.

---

## 4. Advanced Configuration 

### Manual Configuration (JSON)

The configuration is stored in a JSON file located at:

`~/.config/mimick/config.json`

(Inside flatpak, this path is `~/.var/app/dev.nicx.mimick/config/mimick/config.json`).

### File Structure

```json
{
    "watch_paths": [
        "/home/user/Pictures",
        {
            "path": "/home/user/DCIM",
            "album_name": "Phone Uploads",
            "rules": {
                "ignore_hidden": true,
                "max_file_size_mb": 500,
                "allowed_extensions": ["jpg", "png", "mp4"]
            }
        }
    ],
    "internal_url": "http://192.168.1.10:2283",
    "external_url": "https://immich.example.com",
    "internal_url_enabled": true,
    "external_url_enabled": true,
    "run_on_startup": false,
    "pause_on_metered_network": false,
    "pause_on_battery_power": false
}
```

### Properties

| Key | Description | Example |
| :--- | :--- | :--- |
| `watch_paths` | A list of selected directories to monitor recursively. Entries may be plain strings for older configs or objects containing `path`, optional album targeting fields, and `rules`. In Flatpak builds, add them from the settings window so portal access is granted; they may be stored as portal-backed paths under `/run/user/.../doc/...`. | `["/home/user/Screenshots"]` |
| `internal_url` | The LAN IP/Hostname of your Immich instance. Used when local connectivity is detected. | `http://192.168.1.10:2283` |
| `external_url` | The WAN/Public URL (reverse proxy). Used when away from home. | `https://photos.mydomain.com` |
| `internal_url_enabled` | Toggle allowing the Daemon to attempt LAN connectivity. | `true` |
| `external_url_enabled` | Toggle allowing the Daemon to attempt WAN connectivity. | `true` |
| `run_on_startup` | Whether Mimick should register itself for automatic login startup. | `false` |
| `pause_on_metered_network` | Whether uploads should pause while the active network connection appears metered. | `false` |
| `pause_on_battery_power` | Whether uploads should pause while the system appears to be running on battery. | `false` |
| `library_view_enabled` | Whether the in-app library view opens instead of the settings window. | `false` |
| `download_target_path` | Target folder for library downloads (chosen on first download). | `"/home/user/Pictures/Downloads"` |
| `library_preview_full_resolution` | Load full-resolution originals in the lightbox instead of previews. | `false` |
| `video_volume` | Lightbox video playback volume (0.0–1.0), saved when the lightbox closes. | `1.0` |
| `video_muted` | Whether lightbox video playback starts muted. | `false` |
| `raw_full_decode` | Decode high-resolution sensor data for RAW files instead of using fast embedded previews. | `false` |
| `raw_decode_cache_enabled` | Cache decoded RAW images on disk (applicable when `raw_full_decode` is true). | `false` |
| `show_unnamed_faces` | Show people with no assigned name in the Explore view. | `true` |
| `show_hidden_faces` | Include hidden people in the Explore view. | `false` |
| `background_sync_enabled` | Whether automatic background monitoring/upload discovery is enabled. | `false` |
| `notifications_enabled` | Whether desktop notifications are shown (sync summary, connectivity lost, etc.). | `true` |
| `startup_catchup_mode` | Default catch-up scanning strategy when background scan starts (`Full`, `RecentOnly`, `NewFilesOnly`). | `"Full"` |
| `upload_concurrency` | Number of parallel upload workers (1–10). | `3` |
| `quiet_hours_start` | Quiet-hours window start (local clock hour, 0-23). | `22` |
| `quiet_hours_end` | Quiet-hours window end (local clock hour, 0-23, exclusive). | `6` |

### `watch_paths` Object Form

When a watch path is stored as an object, these fields can appear:

| Key | Description |
| :--- | :--- |
| `path` | Absolute or portal-backed directory path being watched. |
| `album_id` | Optional cached Immich album ID. |
| `album_name` | Optional album name or user-entered target label. |
| `rules.ignore_hidden` | Skip any file inside a hidden path component such as `.cache` or `.stfolder`. |
| `rules.max_file_size_mb` | Optional maximum file size in megabytes. Files larger than this are skipped before queueing. |
| `rules.allowed_extensions` | Optional allowlist of extensions. Values are normalized case-insensitively and leading dots are ignored. |
| `rules.sync_method` | Selected synchronization direction (`Full`, `UploadOnly`, `DownloadOnly`). |
| `rules.startup_catchup_mode` | Override the global startup catch-up mode for this specific folder. |
| `rules.delete_folder_to_album` | Delete the local file when its corresponding remote asset is removed. |
| `rules.delete_album_to_folder` | Delete the remote asset when its corresponding local file is removed. |

### API Key Security & Required Permissions

When generating an API Key in the Immich Web UI (Account Settings → API Keys), you can restrict its permissions for least-privilege use. Mimick maps to Immich's permission scopes as follows.

**Base — required for any sync to work:**

| Permission | Why |
|---|---|
| `user.read` | Establish current user session and identity |
| `asset.upload` | Send media to the server |
| `asset.update` | Apply correct timezone metadata after upload |

**Optional — only required for the features you enable:**

| Feature | Additional permissions |
|---|---|
| Album-targeted watch folders | `album.read`, `album.create`, `albumAsset.create` |
| Server Stats Dialog (click server name in Settings) | `server.about`, `server.versionCheck`, `server.statistics`, `asset.statistics` |
| Library Footer Statistics (photo/video counts) | `asset.statistics` |
| Server About dialog (click server row in library sidebar) | `server.about`, `server.versionCheck` |
| Shared Albums (share your albums, change roles, remove people) | `album.read`, `albumUser.create`, `albumUser.update`, `albumUser.delete`, `user.read` |
| Shared Links (create, copy, edit, delete public links) | `sharedLink.read`, `sharedLink.create`, `sharedLink.update`, `sharedLink.delete` |
| Library / Explore view (browse photos inside Mimick) | `asset.read`, `asset.view`, `asset.download`, `person.read` |
| Edit info, set location, change dates, rate, or move to trash from the library | `asset.update`, `asset.delete` |
| Add or remove photos in albums from the library | `albumAsset.create`, `albumAsset.delete` |
| Leave albums shared with you | `albumUser.delete` |
| Delete albums | `album.delete` |
| Download albums as zip | `asset.download` |
| Map page | `map.read` |
| Trash view (restore, delete permanently, empty trash) | `asset.read`, `asset.delete` |
| Tags in the details pane and tag search | `tag.read`; adding/removing tags also needs `tag.asset`, and creating new tags `tag.create` |
| Archive (archive / unarchive from the library) | `asset.update` |
| People page (rename, favorite, hide, birthday, merge) | `person.read`, `person.update`, `person.merge`; photo counts in a person's header need `person.statistics` |
| Tags page (create, rename, recolor, delete tags) | `tag.read`, `tag.create`, `tag.update`, `tag.delete` |
| Folders view | `folder.read` |
| Partner sharing (add or remove partners) | `partner.read`, `partner.create`, `partner.delete`, `user.read` |
| Browse a partner's photos | `partner.read`, plus the Library / Explore view permissions |
| Show a partner's photos in Photos | `partner.update` |
| Rename albums, edit descriptions, set covers, and change album options | `album.update` |
| Counts and details in view headers | `asset.statistics`, `album.read`, `person.statistics` |
| Hide pages for features turned off in your account | `userPreference.read` (without it, every page stays visible) |
| **Sync Method** set to **Full** or **Download Only** (folder rules) | `asset.read`, `asset.download` |
| **Delete from Immich Too** (folder rules toggle) | `asset.delete` and `albumAsset.delete` (the latter is used when the same asset is referenced by another watch folder, so we just unlink instead of trashing) |
| **Delete from This Folder Too** (folder rules toggle) | No additional remote permissions — the album listing is already covered by `album.read`, the trash happens locally |

If you grant `all`, every feature works without further configuration. The granular list above is for users who prefer scoped keys.

The bulk-upload-check endpoint Mimick uses for pre-flight dedup is covered by `asset.upload` (same scope as the upload it replaces).

### Systemd Service Configuration (Native Only)

If running native, the application can run as a user service. The service file is located at `~/.config/systemd/user/mimick.service`.

**Environment Variables:**
Ideally, configure environment variables in `~/.config/environment.d/mimick.conf`.

- `DISPLAY`: Usually `:0`
- `XDG_RUNTIME_DIR`: Required for DBus session bus access.

---

## 5. Frequently Asked Questions

**Q: Will this delete my local files?**
No. Mimick is strictly one-way (backup mode). It reads local files and uploads them. It never modifies or deletes files on your local machine.

**Q: Are sidecar files supported?**
Currently, Mimick ignores metadata sidecar files (`.xmp`, etc.). Immich has limited sidecar support via the standard API, so they are filtered to prevent clutter.

**Q: What happens if my server is offline?**
The upload will fail gracefully and the file is saved to the retry queue (`~/.cache/mimick/retries.json`). On next launch, it will be automatically retried.

**Q: Why is Mimick paused even though I did not click Pause?**
Check the **Uploads** section. If **Pause on Metered Networks**, **Pause on Battery**, or **Quiet Hours** is enabled, Mimick can pause itself automatically when those conditions are detected.

**Q: What does Sync Now do?**
It reruns the watched-folder scan immediately so you do not need to restart Mimick to pick up missed or newly eligible files.

**Q: The tray icon does not appear on GNOME.**
GNOME requires the "AppIndicator and KStatusNotifierItem Support" extension. Install it from the GNOME Extensions website. Without it, the warning `Watcher(ServiceUnknown)` is expected and harmless — the app still runs fully in the background.

**Q: Mimick quits when I close the window.**
Background Sync is disabled. Turn on **Settings → App → Background Sync**. After that, closing the window hides it instead of exiting.

**Q: Uploads are paused but I did not set quiet hours or enable any pause policy.**
Open the Status page and check the status text — it shows the pause reason. Common causes: the quiet-hours window is active, a metered-network or battery-power policy triggered, or the app was paused manually from the tray.

**Q: The startup scan is slow.**
Set **When Mimick Starts, Check** to **New Files Only** in Settings → Uploads once your initial sync is complete. This skips files already in the sync index and only processes genuinely new additions.

**Q: I have two watch folders pointing at the same parent and subfolder. Which album does a file use?**
The most specific (longest) matching path wins. A file at `~/Pictures/iPhone/img.jpg` uses the album configured for `~/Pictures/iPhone`, not the one for `~/Pictures`.
