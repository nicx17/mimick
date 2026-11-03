# Mimick - Immich Desktop Client for Linux

<div align="center">

<img src="src/assets/icon.png" width="256" alt="Mimick Logo">

![Status](https://img.shields.io/badge/Status-stable-FF90C3?style=for-the-badge)
[![GitHub Release](https://img.shields.io/github/v/release/nicx17/mimick?include_prereleases&style=for-the-badge&logo=github&color=FF90C3)](https://github.com/nicx17/mimick/releases/latest)
[![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)](https://github.com/rust-lang/rust)
[![Linux](https://img.shields.io/badge/Linux-FCC624?style=for-the-badge&logo=linux&logoColor=black)](https://github.com/torvalds/linux)
[![GTK4](https://img.shields.io/badge/GTK4-7DF12B?style=for-the-badge&logo=gtk&logoColor=white)](https://gitlab.gnome.org/GNOME/gtk)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-green.svg?style=for-the-badge)](LICENSE)

[![Flathub Version](https://img.shields.io/flathub/v/dev.nicx.mimick?style=for-the-badge&logoSize=auto&color=%23e9b24c)](https://flathub.org/en/apps/dev.nicx.mimick)
[![Flathub Downloads](https://img.shields.io/flathub/downloads/dev.nicx.mimick?style=for-the-badge&color=%23e9b24c)](https://flathub.org/en/apps/dev.nicx.mimick)
![Sonar Quality Gate](https://img.shields.io/sonar/quality_gate/nicx17_mimick?server=https%3A%2F%2Fsonarcloud.io&style=for-the-badge)
[![Maintainability Rating](https://sonarcloud.io/api/project_badges/measure?project=nicx17_mimick&metric=sqale_rating)](https://sonarcloud.io/summary/new_code?id=nicx17_mimick)
[![Reliability Rating](https://sonarcloud.io/api/project_badges/measure?project=nicx17_mimick&metric=reliability_rating)](https://sonarcloud.io/summary/new_code?id=nicx17_mimick)
[![Security Rating](https://sonarcloud.io/api/project_badges/measure?project=nicx17_mimick&metric=security_rating)](https://sonarcloud.io/summary/new_code?id=nicx17_mimick)
[![SonarQube Cloud](https://sonarcloud.io/images/project_badges/sonarcloud-light.svg)](https://sonarcloud.io/summary/new_code?id=nicx17_mimick)

</div>

<div align="center">
  <img src="assets/mimick-project-card-3x.png" alt="Mimick Project Card" width="100%">
</div>

Mimick is an unofficial Immich desktop client for Linux. It syncs your photo and video folders to a self-hosted Immich server in the background, and has an optional library browser for viewing, searching, and managing your photos from the desktop.

> This is a community-developed project and is not affiliated with or endorsed by the Immich project.

**Status:** Supports Immich v3.0.0+.

## Table of Contents

- [![Screenshots](https://img.shields.io/badge/Screenshots-FF90C3?style=flat-square)](#screenshots)
- [![Features](https://img.shields.io/badge/Features-1F6FEB?style=flat-square)](#features)
- [![Installation](https://img.shields.io/badge/Installation-2E8B57?style=flat-square)](#installation)
- [![Getting Started](https://img.shields.io/badge/Getting_Started-8A2BE2?style=flat-square)](#getting-started)
- [![Documentation](https://img.shields.io/badge/Documentation-444444?style=flat-square)](#documentation)
- [![Building from Source](https://img.shields.io/badge/Building_from_Source-B8860B?style=flat-square)](#building-from-source)
- [![Credits](https://img.shields.io/badge/Credits-0E7490?style=flat-square)](#credits)
- [![License](https://img.shields.io/badge/License-green?style=flat-square)](#license)

## Screenshots

|                                    Library View (Light)                                    |                                                Explore View                                                |
| :----------------------------------------------------------------------------------------: | :--------------------------------------------------------------------------------------------------------: |
|           ![Photos page light](docs/screenshots/photos_page_view_sidebar_on.png)           |          ![Explore View](docs/screenshots/explore_page_view_showing_people_places_sidebar_on.png)          |
|                                 **Sync Status Dashboard**                                  |                                        **Settings & Watch Folders**                                        |
| ![Status dashboard](docs/screenshots/status_pane_sync_status_health_dashboard_actions.png) | ![Settings & Watch Folders](docs/screenshots/settings_pane_showing_library_settings_and_watch_folders.png) |

<div align="center">

[![View All Screenshots](https://img.shields.io/badge/View_All-Screenshots-E05D44?style=for-the-badge&labelColor=E05D44)](https://github.com/nicx17/mimick/wiki/Screenshots)

</div>

## Features

- **Background sync:** watches your folders and uploads new photos and videos as they appear, with 1–10 parallel uploads.
- **No duplicate uploads:** SHA-1 checksums skip files already on the server, and a local index keeps startup scans fast.
- **Per-folder control:** upload to your library, a chosen album, or an album named after the folder. Each folder can skip hidden files, cap file size, allow only some file types, and use Upload Only, Download Only, or Full sync.
- **Reliable:** failed uploads are saved and retried, Mimick switches between your LAN and WAN addresses automatically, and the Queue Inspector shows and retries failures.
- **Battery and network aware:** quiet hours, and optional pausing on metered networks or battery power.
- **Desktop integration:** tray icon, Run on Startup, **Open With Mimick** from your file manager, and drag and drop to upload or export.
- **Private by design:** the Flatpak can only read folders you pick, and your API key is kept in the system keyring.
- **Library browser (optional):** a Photos timeline, albums, Explore, metadata, smart, and text (OCR) search, people, tags, folders, a map, video playback, sharing, archive, and trash.
- **Works on Linux phones:** the layout adapts down to 360 px wide, with a bottom navigation bar.

See [Current App Highlights](https://github.com/nicx17/mimick/wiki/Home#current-app-highlights) for the full list, and the [Library View User Guide](https://github.com/nicx17/mimick/wiki/Library-View-User-Guide) for the browser (or press **Ctrl+?** in the app for shortcuts).

## Installation

<div align="left">
  <a href="https://flathub.org/apps/dev.nicx.mimick">
    <img width="240" alt="Get it on Flathub" src="https://flathub.org/api/badge?locale=en">
  </a>
</div>

Mimick is distributed on Flathub, which keeps it updated automatically:

```bash
flatpak install flathub dev.nicx.mimick
```

> **Note:** If you haven't set up Flathub yet, follow the guide for your distribution at [flathub.org/setup](https://flathub.org/setup).

## Getting Started

The settings window opens the first time you launch Mimick.

1. Under **Server**, enter your Immich **Internal URL** (LAN, e.g. `http://192.168.1.50:2283`), **External URL** (e.g. `https://photos.example.com`), or both.
2. In Immich, create an API key under **Account Settings → API Keys**, paste it into Mimick, and press **Save and Connect**.
3. Under **Watch Folders**, press **Add Folder…** and choose where each folder uploads to.
4. Turn on **Background Sync** to keep syncing after you close the window, and **Run on Startup** to start Mimick when you log in.

Core sync needs only these API key permissions; `all` enables every feature:

| Permission     | Why                                         |
| -------------- | ------------------------------------------- |
| `user.read`    | Establish current user session and identity |
| `asset.upload` | Send media to the server                    |
| `asset.update` | Apply correct timezone metadata after upload |

The [Configuration and First Run](https://github.com/nicx17/mimick/wiki/Configuration-and-First-Run) guide covers every setting and the [permissions each feature needs](https://github.com/nicx17/mimick/wiki/Configuration-and-First-Run#api-key-security--required-permissions).

## Documentation

<div align="center">

[![Wiki Home](https://img.shields.io/badge/Wiki-Home-444444?style=for-the-badge&labelColor=444444)](https://github.com/nicx17/mimick/wiki)
[![Installation](https://img.shields.io/badge/Installation-Guide-1F6FEB?style=for-the-badge&labelColor=1F6FEB)](https://github.com/nicx17/mimick/wiki/Installation)
[![User Guide](https://img.shields.io/badge/User-Guide-2E8B57?style=for-the-badge&labelColor=2E8B57)](https://github.com/nicx17/mimick/wiki/Configuration-and-First-Run)
[![Library View](https://img.shields.io/badge/Library-View-FF90C3?style=for-the-badge&labelColor=FF90C3)](https://github.com/nicx17/mimick/wiki/Library-View-User-Guide)
[![Troubleshooting](https://img.shields.io/badge/Troubleshooting-Guide-CB4B16?style=for-the-badge&labelColor=CB4B16)](https://github.com/nicx17/mimick/wiki/Troubleshooting)
[![Development](https://img.shields.io/badge/Development-Guide-B8860B?style=for-the-badge&labelColor=B8860B)](https://github.com/nicx17/mimick/wiki/Development)
[![Testing](https://img.shields.io/badge/Testing-Guide-0E7490?style=for-the-badge&labelColor=0E7490)](https://github.com/nicx17/mimick/wiki/Testing)
[![API Docs](https://img.shields.io/badge/API-Rustdoc-F74C00?style=for-the-badge&logo=rust&logoColor=white&labelColor=F74C00)](https://mimick.nicx.dev/docs/)
[![Security](https://img.shields.io/badge/Security-Policy-5B8C5A?style=for-the-badge&labelColor=5B8C5A)](SECURITY.md)

</div>

## Building from Source

You need a Rust toolchain, GTK 4.24, libadwaita 1.10, libheif 1.21, libshumate 1.7, GStreamer with the GTK 4 video sink, and libclang. The [Development guide](https://github.com/nicx17/mimick/wiki/Development) lists the packages for each distribution.

```bash
git clone https://github.com/nicx17/mimick.git
cd mimick
cargo run -- --settings      # native build, opens the settings window

flatpak-builder --user --install --force-clean build-dir dev.nicx.mimick.local.yml
flatpak run dev.nicx.mimick  # local Flatpak build
```

## Contributing

Pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for commit and style guidelines. Releases ship with checksums, and CI runs formatting, linting, tests, dependency audits, and CodeQL analysis.

## Credits

- **Icon design:** illustration by [Round Icons](https://unsplash.com/illustrations/a-white-and-orange-flower-on-a-white-background-IkQ_WrJzZOM) on Unsplash.
- **Map data and styles:** © [OpenStreetMap contributors](https://www.openstreetmap.org/copyright) and [VersaTiles](https://versatiles.org).
- **Special thanks:** [Immich](https://immich.app).
- **Built with:** [GTK](https://www.gtk.org), [libadwaita](https://gnome.pages.gitlab.gnome.org/libadwaita/), [libshumate](https://gitlab.gnome.org/GNOME/libshumate), [GStreamer](https://gstreamer.freedesktop.org), [libheif](https://github.com/strukturag/libheif), [LibRaw](https://www.libraw.org), [libjpeg-turbo](https://libjpeg-turbo.org), [libwebp](https://chromium.googlesource.com/webm/libwebp), and [OpenJPEG](https://www.openjpeg.org).

Third-party license texts are in [THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt).

## License

GNU General Public License v3.0 — see [LICENSE](LICENSE).
