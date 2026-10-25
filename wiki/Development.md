# Development

## Prerequisites

- Rust toolchain via `rustup`
- GTK4 development packages (>= 4.24 required)
- Libadwaita development packages (>= 1.10 required)
- libheif development packages (>= 1.21 required)
- libshumate development packages (>= 1.7 required), for the maps
- GStreamer and GstPlay development packages, plus the GTK 4 video sink plugin (`gtk4paintablesink`) for video playback
- libclang (Clang development files), used at build time to generate the bundled LibRaw 0.21 bindings

### Ubuntu / Debian

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev libglib2.0-dev pkg-config build-essential cmake libheif-dev libshumate-dev libgstreamer1.0-dev libgstreamer-plugins-bad1.0-dev gstreamer1.0-gtk4 libclang-dev
```

### Fedora

```bash
sudo dnf install gtk4-devel libadwaita-devel pkg-config cmake libheif-devel libshumate-devel gstreamer1-devel gstreamer1-plugins-bad-free-devel gstreamer1-plugin-gtk4 clang-devel
```

### Arch Linux

```bash
sudo pacman -S gtk4 libadwaita pkgconf base-devel cmake libheif libshumate gstreamer gst-plugins-bad-libs gst-plugin-gtk4 clang
```

### Flatpak Packaging Prerequisites

If you plan to build or test the Flatpak bundle locally using `flatpak-builder`, you must install the following from [Flathub](https://flathub.org/setup):

- **GNOME Platform 51 Runtime** (`org.gnome.Platform//51`)
- **GNOME SDK 51** (`org.gnome.Sdk//51`)
- **Freedesktop Rust Extension** (`org.freedesktop.Sdk.Extension.rust-stable//26.08`)
- **Freedesktop LLVM Extension** (`org.freedesktop.Sdk.Extension.llvm22//26.08`), which provides the libclang that `rsraw-sys` needs to generate its LibRaw bindings

## Build and Run

```bash
cargo check
cargo run                   # start in background mode
cargo run -- --settings     # open the settings window immediately
cargo build --release       # build optimized release binary
```

### Desktop Launcher Integration (Native)

To integrate a native build with your desktop application launcher:

```bash
cp setup/dev.nicx.mimick.desktop ~/.local/share/applications/
cp setup/icons/dev.nicx.mimick.svg ~/.local/share/icons/hicolor/scalable/apps/
update-desktop-database ~/.local/share/applications/
```

## API Documentation (Rustdoc)

Rustdoc is generated and deployed to [mimick.nicx.dev/docs](https://mimick.nicx.dev/docs/) on each release. To generate the docs locally:

```bash
cargo doc --no-deps --open
```

## Profile Switcher

Mimick supports isolated runtime profiles via the `MIMICK_PROFILE` environment
variable. Each profile gets its own config, sync index, retry queue, thumbnail
cache, and keyring entry — letting you run a dev Immich instance without
touching your personal library.

### Rules

| Rule | Detail |
|---|---|
| **Name format** | `[A-Za-z][A-Za-z0-9_-]{0,31}` — must start with a letter (D-Bus name requirement) |
| **Default** | No env var → uses `mimick/` dirs and `api_key` keyring entry |
| **Named** | `MIMICK_PROFILE=dev` → uses `mimick-dev/` dirs and `api_key-dev` keyring entry |

### State isolation

| State | Default path | Named-profile path |
|---|---|---|
| Config | `~/.config/mimick/config.json` | `~/.config/mimick-dev/config.json` |
| Sync index | `~/.local/share/mimick/synced_index.json` | `~/.local/share/mimick-dev/synced_index.json` |
| Retries | `~/.cache/mimick/retries.json` | `~/.cache/mimick-dev/retries.json` |
| Logs | `~/.cache/mimick/mimick.log` | `~/.cache/mimick-dev/mimick.log` |
| Thumbnails | `~/.cache/mimick/thumbnails/` | `~/.cache/mimick-dev/thumbnails/` |
| Keyring entry | `account=api_key` | `account=api_key-dev` |

Inside the Flatpak all paths sit under `~/.var/app/dev.nicx.mimick/` — the
table above shows relative structure within that root.

### Running a named profile

```bash
# Local build
MIMICK_PROFILE=dev cargo run

# Installed Flatpak
flatpak run --env=MIMICK_PROFILE=dev dev.nicx.mimick
```

Both profiles can run simultaneously. The GTK `application_id` is set to
`dev.nicx.mimick.{profile}` for named profiles, so they register as separate
D-Bus instances and don't activate each other's windows.

### What is NOT isolated

| Resource | Behaviour |
|---|---|
| **Portal folder grants** | Shared by Flatpak app-id (`dev.nicx.mimick`). A folder granted to prod is accessible to dev — convenient, not a security boundary. |
| **XDG autostart entry** | Always refers to the default profile's launch command. |
| **Installed icons / resources** | Both profiles read from the same Flatpak `/app/` mount. |

A startup log line confirms the active profile:

```
Active profile: dev (state dirs use segment 'mimick-dev')
```

---

## Logging

Mimick uses `flexi_logger` and writes logs to both:

- stdout
- `~/.cache/mimick/mimick.log` (or `~/.cache/mimick-{profile}/mimick.log` for named profiles)

Detailed timestamps are enabled for both outputs.
Console logs are colorized by level (error/warn/info/debug/trace).
File logging rotates automatically (approximately 2 MB per file, 5 files kept).

Increase verbosity with:

```bash
RUST_LOG=debug cargo run
```

## Library View & Settings UX

- The `adw::PreferencesWindow` now serves as a unified interface for both the **Library View** (Photos, Explore, Albums, Search) and the **Settings** panel.
- Most UI changes in the Settings section are applied live (auto-apply). This includes:
	- upload worker count
	- quiet hours start/end
	- folder add/remove
	- per-folder album target and folder rules

- Connectivity fields (API Key, Internal/External server URLs) are treated as save-only. Changes to these fields are applied only when the user clicks **Save** in the Connectivity section to avoid partially-applied network credentials during configuration edits.

## Notifications

- Per-upload notifications were noisy when many workers were active. Mimick now aggregates worker outcomes into a single batch summary that states how many files were processed successfully and how many failed for the batch. Connectivity-related notifications (such as "Connection Lost") still fire independently to alert the user of network failures.


## Packaging

For local Flatpak work:

```bash
flatpak-builder --user --install --force-clean build-dir dev.nicx.mimick.local.yml
```

Run the staged Flatpak without installing:

```bash
flatpak-builder --run build-dir dev.nicx.mimick.local.yml mimick --settings
```

### Regenerating Cargo Sources for Flatpak

After updating `Cargo.toml` or `Cargo.lock`, regenerate the Flatpak cargo sources manifest:

```bash
python3 flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json
```
