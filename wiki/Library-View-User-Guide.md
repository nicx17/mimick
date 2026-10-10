# Library View

The library view is an optional in-app browser for your Immich server's assets, albums, and Explore categories. It replaces the default settings window as the main window when enabled.

---

## Enabling the Library View

1. Open **Settings → Behavior → Enable Library View**.
2. Save and restart Mimick.

The library window opens instead of the settings window on the next launch. Settings remain accessible from the header bar gear button.

| Photos (Light) | Photos (Dark) |
| :---: | :---: |
| ![Photos light](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/photos_page_view_sidebar_on.png) | ![Photos dark](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/photos_page_view_dark_sidebar_on.png) |

## Layout

The window uses a sidebar + content split.

**Sidebar** (toggle with **F9** or the sidebar button in the header):

- **Photos** — opens the timeline grid
- **Search** — opens the search form above the grid
- **Explore** — People, Places, Things
- **Albums** — album landing page
- **Trash** — assets in the Immich trash, which you can restore or delete permanently (see [Trash](#trash))
- album entries listed below for quick navigation
- Bottom footer contains the server connection row (clicking it opens the **Server Statistics** dialog)

**Header bar controls (right side):**

- source selector dropdown (**Remote / Local / Unified**)
- sort selector (**Newest / Filename / File Type / Sync State**)
- search entry with search mode selector
- Timeline toggle button
- select-mode toggle button
- Upload button (invokes the manual multi-file picker)
- refresh button
- gear button (opens Settings)

| Search Options | View Options |
| :---: | :---: |
| ![Search options](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/search_options_menu_library_view.png) | ![View options](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/library_view_showing_view_options_for_albums_remote_local_unified.png) |

---

## Browsing Assets

### Grid and Pagination

Assets load in pages of 50. Scroll to the bottom of the grid to load the next page. The footer label shows the current count.

Thumbnails are loaded asynchronously. A placeholder is shown while the thumbnail downloads. Decoded thumbnails are cached in RAM up to the configured limit (`library_thumbnail_cache_mb`, default 80 MB).

### Sorting

Use the sort dropdown to order the current view by:

- **Newest** — most recently created first
- **Filename** — alphabetical
- **File Type** — grouped by MIME type
- **Sync State** — local-only assets first, then synced

Sort applies to the current source and page.

### Timeline View

The **Timeline** button in the header switches the Remote source between a standard paged grid and a timeline layout that groups assets by date. Timeline is only available for the Remote source; it is hidden when Local or Unified is active.

---

## Sources

The source dropdown controls which assets the grid shows.

| Source | What it shows |
| :--- | :--- |
| **Remote** | Assets fetched from the Immich server |
| **Local** | Files in your configured watch folders, enumerated directly |
| **Unified** | Remote assets merged with local sync state |

Switching sources clears any active search and reloads from page 1.

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/library_view_showing_view_options_for_albums_remote_local_unified.png" width="40%" alt="Source view options" />
</div>

**Local source notes:**

- Local enumeration walks your watch folders using the same extension filter as the sync engine.
- No checksum is computed during enumeration, so Local mode does not indicate whether a file has been uploaded — use Unified for that.
- Files matched via the sync index show their album assignment and sync state.

---

## Searching

The search entry appears in the header bar. Enter a query and press Enter or wait for the field to commit.

### Search Modes (Remote only)

Use the mode dropdown next to the search entry:

| Mode | What it searches | Server requirement |
| :--- | :--- | :--- |
| **Filename** | Filename and EXIF metadata fields | None |
| **Smart Search** | CLIP-based semantic/natural-language similarity | Immich ML service running |
| **OCR** | Text extracted from images | Immich ML service running |

- Smart Search and OCR both require the Immich machine-learning service to be enabled and healthy on your server. Queries against a server without ML will return empty results or an error.
- Filename mode works without ML and is the fastest option.
- Clearing the search entry returns to the previous non-search source.

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/search_view_with_thequickbrownfox_searched_ocr_mode.png" width="80%" alt="OCR search example" />
</div>

| Advanced Filters | Advanced Filters (More Options) |
| :---: | :---: |
| ![Filters](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/advanced_filters_menu_library_view.png) | ![Filters more](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/advanced_filters_menu_library_view_more_options.png) |

### Search by Tag

- The **Tag** filter in the advanced filters (Tags group) limits results to assets carrying one tag. Its list is loaded from the server each time you open Search.
- Clicking a tag in the lightbox details pane runs the same search directly (see [Tags in the Details Pane](#tags-in-the-details-pane)).
- Requires the `tag.read` API key permission; without it the list stays at **Any**.

**Local and Unified search** always uses filename matching regardless of the mode selector. The mode selector is hidden when Local or Unified is active.

---

## Explore

Select **Explore** in the sidebar to open the Explore view.

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/explore_page_view_showing_people_places_sidebar_on.png" width="80%" alt="Explore page" />
</div>

Three sections are populated from the Immich server:

- **People** — a horizontal row of recognised person tiles with their name and a representative thumbnail. Requires face recognition to be enabled on the server.
- **Places** — city/location tiles for assets with geolocation EXIF data.
- **Things** — tag tiles for object-recognition labels (animals, vehicles, etc.). Requires ML object tagging on the server.

Clicking a tile filters the grid to assets belonging to that person, place, or tag. The view will persist your active filter selection even if you navigate away and return.

Use the **Refresh** button in the header to reload the Explore data. Sections that return no results from the server are hidden automatically.

**Face Visibility:** By default, unnamed faces are shown and hidden faces are excluded. You can toggle these independently in Library Settings.

---

## Albums

Select **Albums** in the sidebar to open the album landing page.

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/album_page_view_showing_albums_recent_youralbums.png" width="80%" alt="Album page" />
</div>

Three sections are shown:

- **Recent** — recently accessed albums
- **Your albums** — albums you own
- **Shared with you** — albums shared by other users

Click an album tile to open it in the grid view. The album is also added to the sidebar list for quick re-access.

| Selected Album (Dark) | Album View (Sidebar Off) |
| :---: | :---: |
| ![Album dark](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/selected_album_view_dark.png) | ![Album sidebar off](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/name_dark_album_selected_view_sidebar_off.png) |

### Creating an Album

Click **Create album** (top right of the Albums page). Enter a name and confirm. The new album appears in the **Your albums** section immediately.

To assign a new or existing album to a watch folder for automatic upload, use **Settings → Watch Folders → album dropdown** on the relevant folder row.

### Manual Upload

You can upload files directly to the library or to a specific album using the **Upload** button in the header bar. This opens a multi-file picker and enqueues the selections for upload without requiring a watch folder.

---

## Selection Mode

Click the checkbox icon in the header bar (or use **Esc** to exit) to toggle selection mode.

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/library_view_photos_page_showing_selected_assets_with_checkboxes.png" width="80%" alt="Multi-select with checkboxes" />
</div>

In selection mode:

- each grid cell shows a checkbox
- clicking a cell toggles its selection state
- a bulk action bar appears at the bottom showing the selection count

**Bulk actions available:**

- **Download** — saves selected remote assets to the configured download folder (local-only assets are skipped)
- **Move to trash** — moves selected remote assets to the Immich trash after a confirmation dialog. They can be restored from the [Trash](#trash) view or the Immich web UI. Local copies in your watch folders are kept, and local-only assets are skipped (the dialog says how many).
- **Clear** — deselects all items without taking action

Pressing **Delete** while the grid has focus does the same as **Move to trash** for the current selection.

In the [Trash](#trash) view the bar shows **Restore** and **Delete Permanently** instead of Download and Move to trash.

Selection mode exits automatically when all items are deselected.

---

## Lightbox and Asset Details

Click any asset in the grid to open it in the lightbox.

| Lightbox (Details On) | Lightbox (Details Off) |
| :---: | :---: |
| ![Lightbox details on](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/lightbox_view_details_pane_on.png) | ![Lightbox details off](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/lightbox_view_details_pane_off.png) |

- The lightbox shows a preview image. If **Full Resolution Preview** is enabled in Settings → Behavior, it loads the original file instead of a server-generated proxy.
- **RAW Previews**: By default, RAW files display using their fast embedded previews. The extractor first scans the file for the largest embedded JPEG (catching the full-resolution preview on cameras that store both a tiny thumb and a full-res one — e.g. Sony ARW); if the file contains no JPEG at all (Samsung DNG, OnePlus DNG, and other phone DNGs that store their preview as uncompressed TIFF strips), it falls back to libraw's bitmap thumbnail with container orientation applied. You can toggle **Full RAW Decoding** in settings to perform a full sensor demosaic instead.
- **Videos**: Navigating to a video asset in the lightbox shows the video's still thumbnail as a poster with a centered play badge. Clicking the badge launches the same external player flow used by the grid (system default app for local files; downloaded to cache then opened for remote files). Zoom, resolution toggle, and download controls are hidden for the video case.
- EXIF metadata is fetched (including for local files) and displayed alongside the asset.
- **Download** saves the original file to the configured download folder.
- **Move to trash** (trash button or **Delete**) moves the asset to the Immich trash after a confirmation, then shows the next asset. If it was the last one the lightbox steps back; if nothing is left it closes. Local copies are kept.
- **Edit Info…** in the details pane (also in the right-click menu) edits the description, the date taken, and the favorite flag. The date uses local time as `YYYY-MM-DD HH:MM:SS`; Save stays disabled while it doesn't parse. Only changed fields are sent to Immich. If you change the date, the grid reloads when you close the lightbox, since the asset moves in the timeline.
- Right-clicking the picture opens the same context menu as the grid: Copy, Download, Open In, **Edit Info…**, and **Move to Trash**.

### Tags in the Details Pane

Server assets show a **Tags** section in the details pane:

- Each tag is a chip. Click its name to see every asset with that tag (the lightbox closes and the search form opens with the Tag filter set). Click **×** to remove it from the asset.
- **+** opens a picker of existing tags; typing narrows the list. If no tag matches what you typed, choose **Create “…”** (or press Enter) to create it. Nested tags such as `Trips/2024` work, and Immich creates any missing parents.
- Requires `tag.read`; adding and removing also need `tag.asset`, and creating new tags needs `tag.create`.

**Download folder:**

- On the first download, a folder picker opens and the chosen path is saved to `download_target_path` in `config.json`.
- Subsequent downloads go to the same folder without prompting.
- If the target folder is missing at download time, the picker opens again.
- Filename collisions are resolved by appending a numeric suffix (e.g. `photo (1).jpg`).

---

## Album Sync (Bidirectional)

When viewing an album, a footer row shows the linked local watch folder (if any) and two action buttons.

| Button | Action |
| :--- | :--- |
| **Link folder** | Opens a picker to associate a local watch folder with this album |
| **Sync** | Runs a bidirectional sync between the linked folder and the album |

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/library_view_showing_sync_album_dialog_upload_download_apply_cancel.png" width="80%" alt="Album sync dialog" />
</div>

**Sync steps:**

1. Mimick computes SHA-1 checksums for local files in the linked folder.
2. Files present locally but missing from the remote album are uploaded.
3. Assets in the remote album missing from the local folder are downloaded.
4. Name collisions during download are resolved with a numeric suffix.

Album sync is on-demand — it runs only when you press **Sync** and does not run automatically in the background. It respects the same file extension filters as the main sync engine.

---

## Library Settings

These options are in **Settings → Behavior** and apply to the library view.

| Setting | Default | Effect |
| :--- | :--- | :--- |
| **Full-Resolution Preview** | Off | When on, the lightbox loads the original file instead of the ~1440px server-generated preview. Uses more bandwidth. |
| **Full RAW Decoding** | Off | Decode high-resolution sensor data instead of using fast embedded camera previews (slower). |
| **Cache Decoded RAW Files** | Off | Store demosaiced RAW images on disk so re-opens are instant. Only applicable when Full RAW Decoding is enabled. |
| **Show unnamed faces** | On | Show people with no assigned name in the Explore view. |
| **Show hidden faces** | Off | Include hidden people in the Explore view. |
| **Thumbnail Memory Cache (MB)** | 80 | RAM cap for decoded thumbnails. Increase to reduce re-fetches when scrolling through large grids. |
| **Download Folder** | Not set | First download opens a folder picker; the chosen path is saved and reused for subsequent downloads. |

See [Performance Tuning](Performance-Tuning) for guidance on choosing values for these settings.

---

## Trash

The **Trash** sidebar entry lists everything in the Immich trash in the normal grid.

- **Restore All** and **Empty Trash** sit in a bar above the grid. Emptying asks for confirmation and can't be undone.
- Select assets to **Restore** them or **Delete Permanently** from the selection bar. **Delete** with a selection also deletes permanently, after confirmation.
- Right-click an asset (in the grid or the lightbox) for **Restore** and **Delete Permanently**.
- Immich deletes trashed assets for good after its retention period (30 days by default, configurable on the server).
- Deleting only affects the server; local copies in your watch folders are kept.
- Requires the `asset.delete` API key permission.

---

## Keyboard Shortcuts

| Key | Action |
| :--- | :--- |
| **F9** | Toggle sidebar |
| **Esc** | Exit selection mode |
| **Delete** | Move the selection (grid) or the current asset (lightbox) to the trash; in the Trash view, delete permanently |
