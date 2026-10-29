# Library View

The library view is an optional in-app browser for your Immich server's assets, albums, and Explore categories. It replaces the default settings window as the main window when enabled.

---

## Enabling the Library View

1. Open **Settings → Behavior → Enable Library View**.
2. Save and restart Mimick.

The library window opens instead of the settings window on the next launch. Settings remain accessible from the header bar gear button.

|                                                     Photos (Light)                                                     |                                                       Photos (Dark)                                                        |
| :--------------------------------------------------------------------------------------------------------------------: | :------------------------------------------------------------------------------------------------------------------------: |
| ![Photos light](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/photos_page_view_sidebar_on.png) | ![Photos dark](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/photos_page_view_dark_sidebar_on.png) |

## Layout

The window uses a sidebar + content split.

**Sidebar** (toggle with **F9** or the sidebar button in the header), with **Search** at the top and then three groups:

- **Search** — opens the search form above the grid
- **Library**
  - **Photos** — opens the timeline grid
  - **Explore** — People, Places, Things
  - **Map** — every photo with a location (see [Map](#map))
  - **People** — everyone Immich has recognized (see [People](#people))
  - **Favorites** — assets you've marked as favorites (see [Favorites](#favorites))
  - **Archive** — archived assets, hidden from Photos (see [Archive](#archive))
  - **Trash** — assets in the Immich trash, which you can restore or delete permanently (see [Trash](#trash))
- **Albums & Sharing**
  - **Albums** — album landing page (see [Editing Albums](#editing-albums))
  - **Shared Albums** — albums shared with you and by you (see [Shared Albums](#shared-albums))
  - **Shared Links** — every public link you've created (see [Shared Links](#shared-links))
  - **Sharing** — partner sharing (see [Partner Sharing](#partner-sharing))
- **Browse**
  - **Tags** — your tag tree (see [Tags page](#tags-page))
  - **Folders** — server photos by their original folder (see [Folders](#folders))
- the arrow on **Albums** expands your three most recently updated albums
- **All Albums** lists every album for quick navigation; click its heading to collapse it
- Bottom footer contains the connection row (clicking it opens the **Server Statistics** dialog) and the server row (clicking it opens **About**: Immich and component versions, source/build links, and the server's version history)

**View header:** every photo view opens with a header that scrolls away with the photos. It shows the view's title and, where Immich has them, details and actions:

- **Albums**: item count, date range, who it's shared with or by, description, Share, Download album, and Create share link buttons, a **⋯** menu with **Rename…**, **Edit Description…**, and **Options…** (your own albums) or **Leave Album…** (albums shared with you), and the linked-folder row with **Link folder** / **Sync**
- **People**: face, name, photo count, age and birthday, a heart to favorite them, and the person's **⋯** menu (Rename, Set Birthday, Hide, Merge People)
- **Tags**: color, photo count, parent tags, and the tag's **⋯** menu
- **Places**, map areas, **Folders**, **Favorites**, **Archive**, and searches: the item count where Immich can count it; folders also show their full path
- **Trash**: the retention note with **Restore All** and **Empty Trash**

**Back and Forward:** the header's back button, **Alt+Left**, the mouse back button, or a quick swipe right on a touch screen returns to the previous view, including pages such as Albums, People, Explore, and each level inside Tags and Folders. **Alt+Right** or the mouse forward button goes forward again. In the lightbox, the mouse back button closes it. A swipe from the very left edge still opens the sidebar.

**Top bar title:** shows the current view's name (an album, a person, a tag, Favorites, and so on).

**Mobile:** on narrow windows (under 600 px) a bar at the bottom switches between **Photos**, **Explore**, **Map**, **Albums**, and **People**. It hides while you're selecting photos.

**Turned-off features:** when the library opens (and on refresh) it reads Immich's server features and your account's **Features** settings, and hides what's turned off:

| Turned off                                    | Hidden in Mimick                                                   |
| :-------------------------------------------- | :----------------------------------------------------------------- |
| Trash (server)                                | **Trash**; deleting then warns that it's permanent                 |
| Face recognition (server) or People (account) | **People**, the bottom-bar People button, Explore's people row, and the lightbox's People section |
| Smart search (server)                         | the **Smart Search** mode and Explore's Things                     |
| OCR (server)                                  | the **OCR** search mode and the OCR filter                         |
| Reverse geocoding (server)                    | Explore's Places                                                   |
| Map (server)                                  | **Map** and the lightbox's mini map                                |
| Search (server)                               | **Search**                                                         |
| Tags (account)                                | **Tags**, the lightbox's Tags section, and the Tag search filter   |
| Folders / Shared Links (account)              | **Folders** / **Shared Links**                                     |
| Ratings (account)                             | the lightbox's Rating section and the Rating search filter         |

If Mimick can't read these settings (for example, an API key without `userPreference.read`), everything stays visible.

**Controls row** (one row above the content, at every window width):

- source selector (**Remote / Local / Unified**), inside albums only
- sort selector (**Newest / Filename / File Type**, or the album sorts on the Albums page), where there's something to sort
- **Upload** button (invokes the manual multi-file picker)

|                                                        Search Options                                                         |                                                                         View Options                                                                         |
| :---------------------------------------------------------------------------------------------------------------------------: | :----------------------------------------------------------------------------------------------------------------------------------------------------------: |
| ![Search options](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/search_options_menu_library_view.png) | ![View options](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/library_view_showing_view_options_for_albums_remote_local_unified.png) |

---

## Browsing Assets

### Grid and Pagination

Assets load in pages of 50. Scroll to the bottom of the grid to load the next page. The footer label shows the current count.

Thumbnails are loaded asynchronously. A placeholder is shown while the thumbnail downloads. Decoded thumbnails are cached in RAM (up to 20% of your RAM, between 500 MB and 3 GB) and freed when you close the library window.

### Sorting

Use the sort dropdown to order the current view by:

- **Newest** — most recently created first
- **Oldest** — earliest first; albums set to oldest first in **Album Options** open this way
- **Filename** — alphabetical
- **File Type** — grouped by MIME type
- **Sync State** — local-only assets first, then synced

Sort applies to the current source and page.

### Photos Timeline

When **Photos** (Remote) is sorted by **Newest**, it shows your whole library as a timeline, the same way Immich's web app does:

- Photos are grouped under date headings, by day (`Mon, Oct 26`; the year is added for other years) or by month, or with no headings — see **Date Headings** under [Library Settings](#library-settings).
- The full height of the library is known straight away, so the scrollbar never grows or jumps. Months that haven't loaded yet show as grey blocks.
- Only what you scroll to is loaded: while you fling or scrub, tiles stay as placeholders, and the photos where you stop load first.
- **Scrubber** (right edge): years and a dot per month, spaced by how many photos each month has. Hover to see the month under the pointer; click or drag to jump there.
- **Phone-width windows**: instead of the scrubber, a small handle appears on the right edge while you scroll. Drag it to move through the library quickly; the month shows beside it.

Sorting by **Filename** or **File Type**, and the Local and Unified sources, use the regular grid without headings or scrubber.

---

## Sources

The source dropdown controls which assets the grid shows.

| Source      | What it shows                                               |
| :---------- | :---------------------------------------------------------- |
| **Remote**  | Assets fetched from the Immich server                       |
| **Local**   | Files in your configured watch folders, enumerated directly |
| **Unified** | Remote assets merged with local sync state                  |

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

| Mode             | What it searches                                | Server requirement        |
| :--------------- | :---------------------------------------------- | :------------------------ |
| **Filename**     | Filename and EXIF metadata fields               | None                      |
| **Smart Search** | CLIP-based semantic/natural-language similarity | Immich ML service running |
| **OCR**          | Text extracted from images                      | Immich ML service running |

- Smart Search and OCR both require the Immich machine-learning service to be enabled and healthy on your server. Queries against a server without ML will return empty results or an error.
- Filename mode works without ML and is the fastest option.
- Clearing the search entry returns to the previous non-search source.

### Filters

Press **Filters** under the search entry to open the filters panel: it docks on the right of the results on wide windows and slides in over them on phones (swipe it away or press the close button). The panel has the text, type and flags, tag, date, camera, and location groups, with **Clear Filters** and **Search** at the bottom; on phones **Search** also closes the panel so you see the results. The **Filters** button shows how many filters are set, for example **Filters · 3**. The **Clear** button next to **Search** clears the query and the filters.

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/search_view_with_thequickbrownfox_searched_ocr_mode.png" width="80%" alt="OCR search example" />
</div>

|                                                     Advanced Filters                                                     |                                                      Advanced Filters (More Options)                                                       |
| :----------------------------------------------------------------------------------------------------------------------: | :----------------------------------------------------------------------------------------------------------------------------------------: |
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

## Map

Select **Map** in the sidebar to see every photo and video with a location.

- Photos close together are grouped into a bubble showing how many there are; zoom in and the bubbles split apart. A single photo shows as a round thumbnail. Hover a bubble to see where it is.
- Click a bubble or thumbnail to list those photos beside the map, titled after the most common place among them (below the map on narrow windows). The sidebar hides to make room and comes back when you close the list; drag the divider to resize. The lightbox, selection, and bulk actions work in the list as in any grid. The **×** in the list's header or **Back** returns to the map alone, and the bubble you opened stays highlighted until then.
- The first visit zooms to fit all your photos. The map remembers where you were until you refresh. You can't zoom out past the point where the world fills the map's height.
- Archived photos are not shown, matching Immich's web map.

**Buttons over the map** (top right):

| Button              | What it does                                                                           |
| :------------------ | :------------------------------------------------------------------------------------- |
| **+** / **−**       | Zoom in and out (scrolling, pinching, and double-clicking work too)                    |
| Rotate left / right | Turn the map 45°; two-finger rotation works on touchpads and touch screens             |
| Compass             | Appears while the map is turned; click it to point north again                         |
| Photos              | **Show photos in this area** lists every photo currently on screen                     |
| Location            | **Show my location** asks the desktop's location service for your position (see below) |
| Layers              | The map style                                                                          |

When the map is short (for example on a phone, with photos listed below it), the zoom and rotate buttons are hidden; pinch, scroll, and two-finger rotation still work, and the compass still appears while the map is turned.

**Map styles:** **Immich** uses the style set on your server (**Administration → Settings → Map**), the same as Immich's web app. **Colorful**, **Natural**, **Muted**, **Gray**, and **Toner** are the free VersaTiles styles (also used by Home Assistant), and **OpenStreetMap** is the standard OpenStreetMap map. Every style except OpenStreetMap switches between light and dark with the system style. The choice applies to every map in Mimick and is remembered. If a style can't be loaded, the map shows OpenStreetMap instead.

**Your location:** the first time, your desktop asks whether Mimick may see your location. Location services must be turned on (in GNOME, **Settings → Privacy → Location**). Mimick asks for your position only when you press the button, and only to show it on the map.

### Setting a Location

**Set Location…** (in the lightbox details pane, the right-click menu, or the selection bar's location button for many photos at once) opens a dialog with three ways to choose a spot:

- **Search places** — type a city or town; results come from your Immich server. Click one to move the pin there.
- **Click the map** — drops the pin where you click. Drag to pan and scroll to zoom as usual.
- **Latitude, Longitude** — type or paste coordinates such as `38.7223, -9.1393`. The paste button reads them from the clipboard. To copy the location of another photo, use the copy button next to its **Coordinates** in the details pane, then paste it here.

**Save** sends the location to Immich for every chosen photo. Immich fills in the city and country shortly after, once it has looked the place up.

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

|                                               Selected Album (Dark)                                               |                                                         Album View (Sidebar Off)                                                          |
| :---------------------------------------------------------------------------------------------------------------: | :---------------------------------------------------------------------------------------------------------------------------------------: |
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

- **Add to album** — opens the album picker (see [Managing Albums](#managing-albums))
- **Remove from album** — inside an album you own or edit, takes the selected photos out of it (see [Managing Albums](#managing-albums))
- **Download** — saves selected remote assets to the configured download folder (local-only assets are skipped)
- **Change date** — sets or shifts the capture date and time zone of every selected photo you own (see [Changing Dates](#changing-dates))
- **Set location** — gives every selected photo you own the same location; photos other people own (from partners or shared albums) are left unchanged (see [Setting a Location](#setting-a-location))
- **Move to trash** — moves selected remote assets to the Immich trash after a confirmation dialog. They can be restored from the [Trash](#trash) view or the Immich web UI. Local copies in your watch folders are kept, and local-only assets are skipped (the dialog says how many).
- **Clear** — deselects all items without taking action

Pressing **Delete** while the grid has focus does the same as **Move to trash** for the current selection.

On phone-width windows, Add to album, Download, and Move to trash stay as buttons; Create share link, Archive, Set location, Change date, and Remove from album are in the **⋯** (More actions) menu.

In the [Trash](#trash) view the bar shows **Restore** and **Delete Permanently** instead of Download and Move to trash.

Selection mode exits automatically when all items are deselected.

---

## Lightbox and Asset Details

Click any asset in the grid to open it in the lightbox.

|                                                      Lightbox (Details On)                                                      |                                                      Lightbox (Details Off)                                                       |
| :-----------------------------------------------------------------------------------------------------------------------------: | :-------------------------------------------------------------------------------------------------------------------------------: |
| ![Lightbox details on](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/lightbox_view_details_pane_on.png) | ![Lightbox details off](https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/lightbox_view_details_pane_off.png) |

- The lightbox shows a preview image. If **Full Resolution Preview** is enabled in Settings → Behavior, it loads the original file instead of a server-generated proxy.
- **Edited photos**: Photos edited in Immich (cropped or rotated) show the edited version. Download and drag-out still give the original file.
- **RAW Previews**: By default, RAW files display using their fast embedded previews. The extractor first scans the file for the largest embedded JPEG (catching the full-resolution preview on cameras that store both a tiny thumb and a full-res one — e.g. Sony ARW); if the file contains no JPEG at all (Samsung DNG, OnePlus DNG, and other phone DNGs that store their preview as uncompressed TIFF strips), it falls back to libraw's bitmap thumbnail with container orientation applied. You can toggle **Full RAW Decoding** in settings to perform a full sensor demosaic instead.
- **Videos**: Clicking a video opens it in the lightbox with its thumbnail as a poster and a centered play badge. Click the badge (or press **Space**) to play it in the app. Local and synced videos play from disk; server videos stream from Immich, which serves its transcoded copy when one exists. Decoding uses the GPU (VA-API) when the codec is supported.
  - The control bar has play/pause, stop, back/forward 10 seconds, a seek bar with elapsed and total time, loop, playback speed (0.25×–2×), volume and mute, and fullscreen. An audio and subtitle menu appears when the file has more than one audio track or any subtitles; subtitles start off.
  - The controls fade out after 3 seconds of playback without mouse movement; move the pointer or tap the video to bring them back. Double-click (or double-tap) the video or press **F11** for fullscreen.
  - On narrow windows (phones) the ±10 s buttons are hidden to fit the bar (**J** / **L** still work), and swiping on a playing video moves to the previous or next asset.
  - **Open with…** in the action bar opens the video in another app you pick (remote videos download to the cache first).
  - Volume and mute are remembered between sessions. Zoom, resolution toggle, and download controls are hidden for videos.
- EXIF metadata is fetched (including for local files) and displayed alongside the asset.
- **Fullscreen** (header button or **F11**) shows the asset edge to edge on black. Move the pointer or tap to bring the header back (it hides again after 3 seconds); press **Esc**, **F11**, or the header button to leave. Left/Right and swipes still change assets.
- **Download** saves the original file to the configured download folder.
- **Move to trash** (trash button or **Delete**) moves the asset to the Immich trash after a confirmation, then shows the next asset. If it was the last one the lightbox steps back; if nothing is left it closes. Local copies are kept.
- **Edit Info…** in the details pane (also in the right-click menu) edits the description, the date taken, and the favorite flag (heart toggle). For the date, click the calendar button: pick a day, set the hour, minute, and second, then **Set Date** (local time). Opened from the right-click menu, **Edit Info…** also has a searchable **Time zone** row and shows the date in the photo's own time zone; the date is saved as shown, in the zone you pick. Only changed fields are sent to Immich. If you change the date, the grid reloads when you close the lightbox, since the asset moves in the timeline.
- **Location**: photos with GPS coordinates show a small map with a pin under **Coordinates**; click it to open the [Map](#map) there. The copy button next to the coordinates copies them for pasting elsewhere. **Set Location…** next to **Edit Info…** sets or changes the location (see [Setting a Location](#setting-a-location)). As in Immich, **Edit Info…** and **Set Location…** only appear on photos you own.
- Right-clicking the picture opens the same context menu as the grid: Copy, Download, Open In, **Edit Info…**, **Set Location…**, and **Move to Trash**.
- **Recognized text**: when Immich found text in a photo (OCR), a text button appears in the action bar. Click it (or press **T**) to outline each piece of text and show a **Copy All Text** button over the photo.
  - With a mouse, the pointer becomes a text cursor over a box. Click a box to select it, drag across boxes to select everything between them in reading order, Shift-click to extend the selection, and Ctrl-click to add or remove one box. Hover a box to read it. **Ctrl+A** selects all.
  - On a touch screen, tap a box to add or remove it; dragging and pinching still move and zoom the photo.
  - Once something is selected the button reads **Copy Selected Text**. The button (or **Ctrl+C**) copies the selection, or all text when nothing is selected, one box per line in reading order.

### Tags in the Details Pane

Server assets show a **Tags** section in the details pane:

- Each tag is a rounded pill showing the tag's color (set in Immich; the accent color otherwise), its parent path dimmed and its own name in bold (e.g. Trips/**2024**). Click the pill to see every asset with that tag (the lightbox closes and the search form opens with the Tag filter set). Click **×** to remove it from the asset. Assets without tags show "No tags yet".
- **+** opens a picker of existing tags; typing narrows the list. If no tag matches what you typed, choose **Create “…”** (or press Enter) to create it. Nested tags such as `Trips/2024` work, and Immich creates any missing parents.
- Requires `tag.read`; adding and removing also need `tag.asset`, and creating new tags needs `tag.create`.

### Rating in the Details Pane

When **Ratings** is turned on in your Immich account settings (Account Settings → Features; Immich turns it off by default), server assets show a **Rating** section above the tags:

- Click a star to rate the photo or video from 1 to 5; hovering previews the rating. **Clear** removes it.
- The rating is saved right away. If Immich can't save it, the stars go back to the previous rating and an error explains why.
- As in Immich, only your own photos can be rated. Photos shared with you show their rating without letting you change it, and no section when they have none.
- Requires `asset.update`.

### Appears In

Photos on the server show an **Appears In** section listing every album they're in, with its cover and item count. Click an album to close the lightbox and open it. See [Managing Albums](#managing-albums).

### People in the Details Pane

Your own photos show a **People** section below the rating with everyone Immich recognized in them: their face and name ("Unnamed" when they have none), and, when a birth date is set in Immich, their age when the photo was taken ("Age 3 months", "Age 1 year, 2 months", "Age 34"). Click someone to close the lightbox and see all their photos. People you've hidden in Immich are left out, and the section only appears when face recognition is on (see the table above). Requires `person.read`.

**Download folder:**

- On the first download, a folder picker opens and the chosen path is saved to `download_target_path` in `config.json`.
- Subsequent downloads go to the same folder without prompting.
- If the target folder is missing at download time, the picker opens again.
- Filename collisions are resolved by appending a numeric suffix (e.g. `photo (1).jpg`).

---

## Album Sync (Bidirectional)

When viewing an album, its header shows the linked local watch folder (if any) and two action buttons.

| Button          | Action                                                            |
| :-------------- | :---------------------------------------------------------------- |
| **Link folder** | Opens a picker to associate a local watch folder with this album  |
| **Sync**        | Runs a bidirectional sync between the linked folder and the album |

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

| Setting                     | Default | Effect                                                                                                                                 |
| :-------------------------- | :------ | :------------------------------------------------------------------------------------------------------------------------------------- |
| **Full-Resolution Preview** | Off     | When on, the lightbox loads the original file instead of the ~1440px server-generated preview. Uses more bandwidth.                    |
| **Full RAW Decoding**       | Off     | Decode high-resolution sensor data instead of using fast embedded camera previews (slower).                                            |
| **Cache Decoded RAW Files** | Off     | Store demosaiced RAW images on disk so re-opens are instant. Only applicable when Full RAW Decoding is enabled.                        |
| **Show unnamed faces**      | On      | Show people with no assigned name in the Explore view.                                                                                 |
| **Show hidden faces**       | Off     | Include hidden people in the Explore view.                                                                                             |
| **Date Headings**           | By Day  | Dates between photos in the Photos timeline: **By Day**, **By Month**, or **Off**. Changes apply to an open library window right away. |
| **Download Folder**         | Not set | First download opens a folder picker; the chosen path is saved and reused for subsequent downloads.                                    |

See [Performance Tuning](Performance-Tuning) for guidance on choosing values for these settings.

---

## Shared Albums

- The **Shared Albums** sidebar entry shows **Shared by you** (your albums that have other members) and **Shared with you**. Click an album to open it.
- Right-click one of your own albums (here or on the Albums page) and choose **Share…**:
  - **Shared with** lists everyone in the album. Change a person's role with the **Viewer / Editor** dropdown, or remove them with the trash button. The owner is shown but can't be changed.
  - **Add people** lists users on the server who aren't in the album yet; type to filter by name or email, then **Add** (they join as Viewer).
- Every change is saved to Immich immediately. Requires `albumUser.create`, `albumUser.update`, `albumUser.delete`, and `user.read` (without `user.read`, the Add people list stays empty).

---

## Shared Links

- Create a public link that anyone with the URL can open in a browser:
  - **An album:** right-click its tile on the Albums or Shared Albums page and choose **Create Share Link…**.
  - **A selection:** select assets, then click the share button in the selection bar.
  - **One asset:** right-click it and choose **Create Share Link…**.
- The dialog sets a **Description**, when the link **Expires** (Never, 1 hour, 1 day, 7 days, 30 days), an optional **Password**, and whether visitors can download originals, upload into the album (album links only), and see metadata. **Create** copies the URL to the clipboard.
- The **Shared Links** sidebar entry lists every link with its cover photo, its expiry (red once expired), the album name (or "Individual share" for selected photos), its description, and what visitors may do (Upload, Download, Exif, Password). Click a link to edit it in the same dialog (leave the password empty to keep it). Each row also has **Copy link**, which shows a checkmark once the URL is on the clipboard, and **Delete** (asks first).
- The URL uses the server's **External domain** (Immich Administration → Settings → Server) when set, and otherwise the address Mimick is connected through. Requires `sharedLink.read`, `sharedLink.create`, `sharedLink.update`, and `sharedLink.delete`.

---

## Favorites

- The heart button in the lightbox action bar (or **F**) marks the current asset as a favorite; it fills in at once and saves to Immich in the background. If Immich refuses (for example, a key without `asset.update`), the heart reverts and an error is shown.
- The right-click menu has **Add to Favorites** / **Remove from Favorites**, and **Edit Info…** has a heart toggle.
- Favorited thumbnails show a small white heart in the bottom-right corner.
- The **Favorites** sidebar entry lists every favorite. Removing a favorite there drops it from the list (the lightbox moves on to the next asset).

---

## People

- The **People** sidebar entry shows every recognized face. Type in the search box to filter by name; the eye button also shows people you've hidden (dimmed).
- Click a face to see that person's photos.
- The **⋯** button on each face has:
  - **Rename…** (or **Add Name…**)
  - **Add to Favorites** / **Remove from Favorites**: favorites get a heart on their face and are listed first.
  - **Set Birthday…**: pick a date, or **Remove** it.
  - **Hide** / **Unhide**: hidden people don't appear in People or Explore.
  - **Merge People…**: tick the people who are really this person; their faces move to them. This can't be undone.

---

## Editing Albums

- Rename an album or edit its description from the album's header (**⋯** menu) or by right-clicking its tile on the Albums page. Only your own albums can be edited.
- Renaming keeps linked watch folders linked: Mimick updates the folder's album name in your settings.

## Managing Albums

- **Add to album**: select photos and press the **+** button in the selection bar, right-click a photo, or press **Add to Album…** in the lightbox details pane. The picker lists your albums and albums shared with you where you're an editor, most recently changed first. Type to filter; when nothing matches, the first row creates an album with the name you typed (press **Enter** to pick the first row). A message says how many were added (and how many were already there), with **Open** to jump to the album. Requires `albumAsset.create` (and `album.create` for new albums).
- **Remove from album**: inside an album you own or edit, right-click a photo or use the selection bar. Photos stay in your library; the message offers **Undo**. Editors can remove only their own photos, and the message says how many were left. Requires `albumAsset.delete`.
- **Set as album cover**: right-click a photo inside one of your albums. Requires `album.update`.
- **Delete album**: in your album's **⋯** menu or its tile's right-click menu, **Delete Album…** deletes it from Immich; its photos stay in your library. The confirmation says when it's also removed for people it's shared with and when its shared links stop working. Because Mimick re-creates a missing album by name, folders that would bring it back are handled too:
  - Watch folders that sync with the album (including folders linked with **Link folder…**) are listed, and you choose **Keep backing them up to your library** (they keep uploading, without an album, and two-way sync and deletion mirroring turn off) or **Stop watching these folders**.
  - Subfolders of a watch folder set to **Default (Folder Name)** whose photos made this album get their own library-only watch folder entry, so the rest of the parent folder still makes albums from its folders.
  - Deleting waits while uploads are running, and while the Settings window is open (saving it would undo the folder changes). Requires `album.delete`.
- **Leave album**: for an album someone shared with you, use **Leave Album…** in its header's **⋯** menu or right-click its tile on the Albums or Shared Albums page. Requires `albumUser.delete`.
- **Album options**: in your album's **⋯** menu, **Options…** sets the **Display order** (newest or oldest first, which Mimick and Immich use when the album opens) and turns **Comments and likes** on or off for the Immich apps. Changes save immediately. Requires `album.update`.
- **Download album**: the download button in the album's header, or **Download** on its tile, saves the whole album as `Album name.zip` in your download folder. Immich splits large albums (4 GiB by default, set in Immich's user settings), which download as `Album name (part 1 of 3).zip` and so on. Requires `asset.download`.
- **Appears in**: the lightbox details pane lists the albums a photo is in; click one to open it. Requires `album.read`.

## Changing Dates

Select photos and choose **Change date** in the selection bar (in **⋯** on phones). Only photos you own change; the dialog says when some of the selection belongs to someone else.

- **Set** gives every selected photo the same date, time, and **Time zone**. The zone list is searchable (type part of a city, e.g. "lisb") and starts at your computer's time zone; the line below shows the exact time that will be saved.
- **Shift** moves each photo by days, hours, and minutes (negative numbers move them earlier), keeping the time between them. Pick a **Time zone** to also move them into that zone, or leave **Keep current zone**. Shifting by zero with a new zone only corrects the zone.

The timeline reloads afterwards so the photos appear in their new place. Requires `asset.update`.

---

## Tags page

- The **Tags** sidebar entry lists your top-level tags. Click a tag to see its photos; the **›** button opens the tags inside it, and the path at the top takes you back up.
- **+** creates a tag at the current level.
- Each tag's **⋯** menu has **Rename…**, **Change Color…** (a preset or a custom color), and **Delete…**.
- Renaming tags needs a newer Immich than v3.0.0; older servers show an error instead.

---

## Folders

- The **Folders** sidebar entry shows the folders your photos came from on the server. It opens at the first level that has more than one folder; the path at the top takes you back up.
- Click a folder to open it. A folder without subfolders opens its photos directly; otherwise **Photos in …** at the top of the list shows the photos in that folder itself.

---

## Partner Sharing

- The **Sharing** sidebar entry lists the partners you share your library with, and the people who share theirs with you.
- **+** opens **Add Partner**: search for someone and press **Add**. Partners can see all your photos and videos. Listing people needs the `user.read` permission.
- The trash button next to a partner stops sharing with them.
- Click someone under **Sharing With You** to browse their photos as a timeline.
- The switch next to someone under **Sharing With You** also shows their photos in your **Photos** timeline.

---

## Archive

- Archiving hides an asset from the **Photos** timeline without deleting it.
- Archive from the right-click menu, the lightbox's archive button (or **A**), or the selection bar.
- The **Archive** sidebar entry lists archived assets; **Unarchive** there puts them back in Photos.

---

## Trash

The **Trash** sidebar entry lists everything in the Immich trash in the normal grid.

- **Restore All** and **Empty Trash** sit in the Trash header. Emptying asks for confirmation and can't be undone.
- Select assets to **Restore** them or **Delete Permanently** from the selection bar. **Delete** with a selection also deletes permanently, after confirmation.
- Right-click an asset (in the grid or the lightbox) for **Restore** and **Delete Permanently**.
- Immich deletes trashed assets for good after its retention period (30 days by default, configurable on the server).
- Deleting only affects the server; local copies in your watch folders are kept.
- Requires the `asset.delete` API key permission.

---

## Keyboard Shortcuts

| Key                           | Action                                                                                                        |
| :---------------------------- | :------------------------------------------------------------------------------------------------------------ |
| **Ctrl+?**                    | Show keyboard shortcuts (also **Keyboard Shortcuts** in the main menu)                                        |
| **F9**                        | Toggle sidebar                                                                                                |
| **Esc**                       | Exit selection mode                                                                                           |
| **Delete**                    | Move the selection (grid) or the current asset (lightbox) to the trash; in the Trash view, delete permanently |
| **F**                         | Toggle favorite for the current asset (lightbox)                                                              |
| **A**                         | Archive or unarchive the current asset (lightbox)                                                             |
| **T**                         | Show or hide recognized text (lightbox, when Immich found text)                                               |
| **Ctrl+A**                    | Select all recognized text (lightbox, while shown)                                                            |
| **Ctrl+C**                    | Copy the selected recognized text, or all of it when nothing is selected (lightbox)                           |
| **Space**                     | Play or pause the current video (lightbox)                                                                    |
| **J** / **L**                 | Skip back / forward 10 seconds (lightbox video)                                                               |
| **,** / **.**                 | Step one frame back / forward (lightbox video)                                                                |
| **M**                         | Mute or unmute (lightbox video)                                                                               |
| **F11**                       | Toggle fullscreen (lightbox); **Esc** also exits                                                              |
| **Alt+Left** / mouse back     | Go back to the previous view (mouse back also closes the lightbox)                                            |
| **Alt+Right** / mouse forward | Go forward again after going back                                                                             |
