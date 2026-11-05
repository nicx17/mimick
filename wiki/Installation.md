# Installation

<div align="center">
  <a href="https://flathub.org/apps/dev.nicx.mimick">
    <img width="240" alt="Get it on Flathub" src="https://flathub.org/api/badge?locale=en">
  </a>
</div>

The recommended install method is via Flathub.

## Prerequisites

- **Flatpak** must be installed on your system.
- The **[Flathub](https://flathub.org/setup)** remote must be configured. Follow the setup guide for your distribution at [flathub.org/setup](https://flathub.org/setup).

## Install Mimick

### Command Line Install

```bash
flatpak install flathub dev.nicx.mimick
```

*(Alternatively, you can install it graphically through your system's software center if it has Flathub integration, like GNOME Software or KDE Discover).*

### Arch Linux (AUR)

Arch users can install a native build from the AUR with an AUR helper such as `yay` or `paru`:

```bash
yay -S mimick
```

The native build runs as `mimick` (and `mimick --settings`) instead of `flatpak run`. For video playback beyond the basics, also install `gst-plugins-good` and `gst-libav`.

### AppImage

Each [GitHub release](https://github.com/nicx17/mimick/releases) includes an AppImage for 64-bit PCs (`Mimick-<version>-x86_64.AppImage`) and for 64-bit ARM devices such as phones (`Mimick-<version>-aarch64.AppImage`). It is a single file that runs on most distributions without installing anything. It carries its own copy of every library it needs, so it also works on distributions too old to have the GTK and libadwaita versions Mimick requires.

```bash
chmod +x Mimick-*.AppImage
./Mimick-*.AppImage
```

- An AppImage doesn't add Mimick to your app launcher or to the file manager's **Open With** menu. Tools such as [Gear Lever](https://flathub.org/apps/it.mijorus.gearlever) or AppImageLauncher do that, and can update the AppImage when a new release comes out.
- **Run on Startup** points at the AppImage file's current location. If you move or rename the file, turn the option off and on again.

## Running

Run the app with:

```bash
flatpak run dev.nicx.mimick
```

Open the settings window directly with:

```bash
flatpak run dev.nicx.mimick --settings
```

## Building from Source

To build Mimick yourself, natively or as a local Flatpak, see [Development](Development).

## What Gets Installed

- Application ID: `dev.nicx.mimick`
- Binary: `mimick`
- Config file: `~/.config/mimick/config.json` (native) or `~/.var/app/dev.nicx.mimick/config/mimick/config.json` (Flatpak)
- Cache directory: `~/.cache/mimick/` (native) or `~/.var/app/dev.nicx.mimick/cache/mimick/` (Flatpak)
- Data directory (sync index): `~/.local/share/mimick/` (native) or `~/.var/app/dev.nicx.mimick/data/mimick/` (Flatpak)
- Keyring: Managed by `oo7` -- encrypted file inside the sandbox (Flatpak) or D-Bus Secret Service (native)
