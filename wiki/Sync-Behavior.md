# Sync Behavior

Mimick syncs each watch folder in two ways: a catch-up scan when it starts (or when you press **Sync Now**), and live monitoring while it runs.

## Sync Methods

Each watch folder picks a **Sync Method** in its Folder Rules:

| Method | What it does |
| :--- | :--- |
| **Upload Only** (default) | Uploads new photos from the folder. Nothing is downloaded and local files are never changed. |
| **Download Only** | Downloads new photos from the folder's album into the folder. Nothing is uploaded. |
| **Full Sync** | Does both: uploads new local photos and downloads new album photos. |

Download Only and Full Sync also check the album every 5 minutes, so photos added in Immich reach the folder without a restart.

Deletions are mirrored only when you turn them on in Folder Rules:

- **Delete from Immich Too** moves a photo's copy in Immich to the trash when you delete it from the folder.
- **Delete from This Folder Too** is not available yet; it waits on a fix to the Flatpak trash portal.

## Startup Catch-Up

When Mimick launches, it scans the watch folders for supported media. **When Mimick Starts, Check** controls how much is scanned (see [Performance Tuning](Performance-Tuning#when-mimick-starts-check)).

A local sync index lets the scan:

- skip unchanged files that are already synced
- detect files whose content changed and upload them again
- detect when a folder's target album changed

The index is stored at `~/.local/share/mimick/synced_index.json` (Flatpak: `~/.var/app/dev.nicx.mimick/data/mimick/synced_index.json`), so clearing the cache doesn't reset it.

## Live Monitoring

While Mimick runs, it watches the folders for new or changed files. Before uploading, it:

1. waits for the file to finish writing
2. computes a SHA-1 checksum
3. uploads the file to Immich, along with its `.xmp` sidecar when **Upload XMP Sidecars** is on
4. adds the asset to the target album

Temporary files and files excluded by folder rules never enter the queue.

## Nested Folders

When watch folders are nested, the most specific match wins. A file in `~/Pictures/iPhone` uses the album and rules of `~/Pictures/iPhone`, not those of `~/Pictures`.

## Album Reassociation

If you point a watch folder at a different album, Mimick adds unchanged files to the new album on the next scan without uploading them again.

If the old album was deleted or its stored ID is stale, Mimick looks the album up again by name and recreates it if needed.

## Retries and Duplicates

- Failed uploads are saved to `~/.cache/mimick/retries.json` and retried on the next run, or from the Queue Inspector.
- Before uploading, Mimick asks Immich which checksums it already has, so existing assets are reused instead of uploaded again.

## Library Album Sync

In the Library View, an album linked to a folder has a **Sync** button that compares the two by checksum on demand. A dialog shows what would change and lets you pick which directions to apply: uploads to the album, downloads to the folder, and moving extra items to the trash on either side. Downloaded files that clash with an existing name get a numeric suffix, such as `file (1).jpg`. See [Album Sync](Library-View-User-Guide#album-sync-bidirectional).
