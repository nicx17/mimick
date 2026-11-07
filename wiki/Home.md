# Mimick Wiki

Welcome to the Mimick project wiki.

Mimick is a Linux background app that watches selected folders and syncs photos and videos to an Immich server. It supports native Linux installs and Flatpak, uses a GTK4/Libadwaita settings window, and keeps syncing reliable with retries, startup catch-up scans, and duplicate-aware uploads.

<div align="center">
  <img src="https://raw.githubusercontent.com/nicx17/mimick/main/docs/screenshots/photos-timeline.png" width="80%" alt="Mimick Library View" />
</div>

<div align="center">
  <a href="https://flathub.org/apps/dev.nicx.mimick">
    <img width="240" alt="Get it on Flathub" src="https://flathub.org/api/badge?locale=en">
  </a>
</div>

## Start Here

<div align="center">

[![Installation](https://img.shields.io/badge/Installation-Guide-1F6FEB?style=for-the-badge&labelColor=1F6FEB)](Installation)
[![Configuration](https://img.shields.io/badge/Configuration-First_Run-2E8B57?style=for-the-badge&labelColor=2E8B57)](Configuration-and-First-Run)
[![Library View](https://img.shields.io/badge/Library-View-FF90C3?style=for-the-badge&labelColor=FF90C3)](Library-View-User-Guide)
[![Sync Behavior](https://img.shields.io/badge/Sync-Behavior-B8860B?style=for-the-badge&labelColor=B8860B)](Sync-Behavior)
[![Performance](https://img.shields.io/badge/Performance-Tuning-0E7490?style=for-the-badge&labelColor=0E7490)](Performance-Tuning)
[![Screenshots](https://img.shields.io/badge/Screenshots-Gallery-E05D44?style=for-the-badge&labelColor=E05D44)](Screenshots)
[![Flatpak](https://img.shields.io/badge/Flatpak-Permissions-6366F1?style=for-the-badge&labelColor=6366F1)](Flatpak-and-Permissions)
[![Keyring](https://img.shields.io/badge/Keyring-Setup-8B5CF6?style=for-the-badge&labelColor=8B5CF6)](Keyring-Setup)
[![Troubleshooting](https://img.shields.io/badge/Troubleshooting-Help-CB4B16?style=for-the-badge&labelColor=CB4B16)](Troubleshooting)

</div>

## Developers and Contributors

<div align="center">

[![Architecture](https://img.shields.io/badge/Architecture-Overview-444444?style=for-the-badge&labelColor=444444)](Architecture)
[![Development](https://img.shields.io/badge/Development-Guide-444444?style=for-the-badge&labelColor=444444)](Development)
[![Testing](https://img.shields.io/badge/Testing-Guide-444444?style=for-the-badge&labelColor=444444)](Testing)
[![Release Ops](https://img.shields.io/badge/Release-Operations-444444?style=for-the-badge&labelColor=444444)](Release-Operations)
[![Repo Automation](https://img.shields.io/badge/Repository-Automation-444444?style=for-the-badge&labelColor=444444)](Repository-Automation)
[![API Docs](https://img.shields.io/badge/API-Rustdoc-F74C00?style=for-the-badge&logo=rust&logoColor=white&labelColor=F74C00)](https://mimick.nicx.dev/docs/)

</div>

## Project Notes

- By default a watch folder only uploads to Immich and never changes local files. Its **Sync Method** can also be **Download Only** or **Full Sync**, which download its album's photos into the folder. See [Sync Behavior](Sync-Behavior).
- API keys are stored in the desktop keyring. Logs and the retry queue live in `~/.cache/mimick/`, and the sync index in `~/.local/share/mimick/`.
- Startup rescans use the sync index, so unchanged files that are already synced are skipped quickly.

## Current App Highlights

- Two-page `Settings` / `Status` settings window
- Upload each watched folder to the Immich library, an album you pick, or an album named after the folder
- Per-folder sync methods (Upload Only, Download Only, Full) with optional deletion mirroring in either direction
- Optional in-app library viewer: a Photos timeline with date headings, a scrubber, Memories, and stacks, albums, Explore, search, People, Tags, Folders, Favorites, Archive, Trash, and Duplicates
- In-app video playback (streamed from Immich, hardware-decoded), live photos, slideshows, and fullscreen viewing in the lightbox
- Select and copy text Immich recognized in photos (OCR), down to a word or a few characters, and tag the people in a photo and see who is who by pointing at their faces, in the lightbox
- Edit photos without touching the original: crop, rotate, flip, and straighten, adjust light and colour, apply filters, and redact faces or number plates, find similar photos, and jump from any view to a photo's place in the timeline
- Map of your photos with place search, a location mini map in the lightbox, and setting the location of one or many photos
- Shared albums with Viewer and Editor roles, likes and comments, public shared links, and partner sharing, including browsing a partner's photos
- Album management: add and remove photos, set covers, album options, delete albums, leave shared albums, download albums as zip, and see which albums a photo is in
- Edit asset info (description, date taken, favorite), change the date and time zone of many photos at once, rate assets with stars, archive, and move assets to the Immich trash from the library
- RAW previews and decoding, and every media format Immich supports
- Drag files in to upload, drag photos out to export, and **Open With Mimick** from your file manager
- Works on Linux phones, down to 360 px wide, with a bottom navigation bar
- Keyboard Shortcuts dialog (**Ctrl+?** or the main menu)
- Queue inspector with retry actions
- Per-folder rules for hidden paths, size limits, and extension filters
- Diagnostics bundle export for support and bug reports
- Quiet hours, and metered-network and battery-aware upload deferral
