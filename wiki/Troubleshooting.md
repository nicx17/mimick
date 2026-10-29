# Troubleshooting

## Installation Errors

### "Runtime Not Found" (`org.gnome.Platform`)

This occurs when Flatpak cannot find the GNOME Platform runtime required by Mimick. It typically happens on fresh installations of Ubuntu 25+, Fedora, or systems where Flathub is not enabled.

**Fix**: This means the Flathub remote is missing from your system, which Flatpak needs in order to download the runtime dependency. Please follow the instructions at [flathub.org/setup](https://flathub.org/setup) to add the Flathub remote to your system, and then retry the installation.

## API Key / Keyring Errors

### "Could Not Save API Key"

Mimick stores the API key using the oo7 keyring library. Inside Flatpak, this uses an encrypted file backend. Outside Flatpak, it uses the D-Bus Secret Service (GNOME Keyring or KWallet).

If you see this error:

1. Make sure your desktop keyring daemon is running (GNOME Keyring, KWallet, etc.)
2. If running inside Flatpak, check that the keyring file is not corrupted (see below)
3. **On Hyprland, Sway, XFCE, i3, and other non-GNOME/KDE desktops**, the Secret portal is often not configured by default. See the [Keyring Setup](Keyring-Setup) guide for step-by-step instructions.

### "IncorrectSecret" or "PartiallyCorruptedKeyring"

This can happen when upgrading from an older version of Mimick, or if the Flatpak portal secret has rotated (e.g., after reinstalling or rebuilding the Flatpak).

**Fix**: Delete the stale keyring file and relaunch. Mimick will create a fresh one:

```bash
rm -f ~/.var/app/dev.nicx.mimick/data/keyrings/default.keyring
flatpak run dev.nicx.mimick
```

Then re-enter and save your API key from the settings window.

### "Immich rejected the API key" / 401 / 403

The key is valid but missing one or more permissions Mimick uses. Confirm in Immich (Account Settings → API Keys → edit) that the key has at least:

- Base sync: `user.read`, `asset.upload`, `asset.update`, `album.read`, `album.create`, `albumAsset.create`
- Add `asset.read`, `asset.view`, `asset.download`, and `person.read` if you use Library view or Download Only / Full sync method
- Add `asset.delete` and `albumAsset.delete` if you enabled **Delete from Immich Too** in any folder's rules

See Configuration & First Run → "API Key Security & Required Permissions" for the full feature/permission mapping.

### Keyring Access Issues (Headless Servers)
If you are running on a server without a desktop session (e.g., via SSH only), the native Keyring might fail to unlock the login keyring.
- **Solution:** Use `dbus-run-session` or configure `pam_gnome_keyring` to unlock on login.

## Display & UI Issues

### Tray Icon Missing
Some desktops restrict or hide legacy tray icons.
- **Wayland (GNOME/KDE) & Ubuntu 24+:** Modern desktop environments deprecate or heavily restrict legacy system trays. The app uses `ksni` (StatusNotifierItem via D-Bus).
- GNOME often needs the **AppIndicator/KStatusNotifierItem Support** extension installed.
- If tray support is unavailable, launching Mimick again should still intelligently detect the running instance and open the settings window instead.

### Notifications Look Wrong / No Progress Bars
If you see multiple individual notifications instead of a single updating bar:
- Some lightweight notification daemons do not support the `x-canonical-private-synchronous` hint, replacement, or progress hints well.
- **Solution:** Install a full-featured notification daemon like `dunst` (configured appropriately) or use a desktop environment like GNOME or KDE Plasma.

### `Gdk-WARNING: Duplicate call to gdk_frame_timings_presented()`
This GTK warning can appear once per run on Wayland, typically when the library view first draws, and is harmless. GTK estimates when each frame reached the screen and later receives the compositor's measured time; on high-refresh displays the two can differ by exactly one refresh (e.g. ~11.1 ms at 90 Hz). Nothing in Mimick causes or needs to fix it.

## Video Playback Issues

### Videos Play Without Sound
- **Flatpak:** Mimick needs the PulseAudio socket (also served by PipeWire through `pipewire-pulse`). Check with `flatpak info --show-permissions dev.nicx.mimick`; if `pulseaudio` is missing (for example after a manual override), restore it with `flatpak override --user --socket=pulseaudio dev.nicx.mimick`.
- Make sure the video isn't muted in the control bar's volume menu (**M** toggles mute). The volume and mute state are remembered between sessions.

### "Playback Unavailable"
The GStreamer GTK 4 video sink is missing. The Flatpak always includes it; on native installs, install your distribution's `gst-plugin-gtk4` package (named `gstreamer1.0-gtk4` on Debian/Ubuntu and `gstreamer1-plugin-gtk4` on Fedora).

### "Playback Failed"
- **"Not connected to the Immich server":** the lightbox couldn't reach either configured server URL; check connectivity as for syncing.
- **Codec errors:** Immich streams its transcoded copy when one exists, otherwise the original file. On native installs, install your distribution's GStreamer FFmpeg plugin (often `gst-libav` or `gstreamer1.0-libav`) for formats such as HEVC. The Flatpak gets FFmpeg codecs from the `org.freedesktop.Platform.codecs-extra` extension, which Flatpak installs automatically.

### Check Hardware Decoding
Run Mimick with GStreamer's VA-API logging and play a video:

```bash
GST_DEBUG=va*:4 flatpak run dev.nicx.mimick
```

Lines mentioning `vah264dec`, `vah265dec`, `vavp9dec`, or `vaav1dec` mean the GPU is decoding. If none appear, the GPU or its VA-API driver doesn't support that codec and GStreamer falls back to software decoding (playback still works, using more CPU).

## Lightbox Issues

### No Recognized-Text Button

The text button in the lightbox action bar only appears when Immich has recognized text in that photo. Check that OCR is enabled in Immich's machine-learning settings and that the OCR job has run for the photo. Local-only files and videos never show it.

### Edited Photo Shows an Older Version

Mimick caches an edited photo the first time you open it. If you edit the photo again in Immich, use **Clear Cache** in the settings window to load the new edit. When the edited version can't be fetched (for example while offline), a photo that is also on disk shows the unedited local copy instead.

## Map Issues

### No Map in the Sidebar or Details Pane

The **Map** page and the lightbox's mini map are hidden when the map is turned off on the Immich server (**Administration → Settings → Map**). The mini map also needs the photo to have GPS coordinates.

### Map Shows OpenStreetMap Instead of the Chosen Style

With the **Immich** style (the default; change it with the layers button on the map), Mimick uses the map style set on your Immich server. It shows OpenStreetMap instead when the server has no style set, when the style or its tiles can't be downloaded, or when the server was unreachable as the map opened (Mimick asks again the next time a map opens). Run Mimick from a terminal to see the reason in the log. Layers of a custom style that libshumate can't draw are skipped and logged, so some labels or icons may be missing.

### Map Tiles Stay Blank

Tiles download from the style's tile server (or OpenStreetMap) directly, not through Immich. Check that the computer can reach it, then use **Refresh** in the main menu to retry tiles that failed.

### "Location Unavailable"

**Show my location** asks the desktop's location service (the XDG Location portal, backed by GeoClue). Turn on location services (in GNOME, **Settings → Privacy → Location**) and allow Mimick when asked. If you declined before, re-enable Mimick in the same settings page. On desktops without the location portal or GeoClue, the button can't find you.

## Startup & Autostart Issues

### App Opens a Window on Login Despite Background Sync (KDE)

If you are using KDE Plasma and the app opens a window every time you log in, even when background sync is enabled, this is likely caused by KDE's session restore feature.

KDE's "Restore previous session" feature re-launches the app independently of the standard autostart mechanism. When it does this, it bypasses the background sync setting and unconditionally opens a window.

**Fix**: Set KDE to start with an empty session instead of restoring the previous one.
1. Open KDE System Settings
2. Go to **Session** → **Desktop Session**
3. Under "On Login", select **Start with an empty session**

### Two Tray Icons Appear (Flatpak on KDE)

If you are running the Flatpak version on KDE and see two tray icons, this is a known interaction between the XDG Background portal, KDE's session restore, and the Flatpak sandbox. Following the fix above (starting with an empty session) will usually resolve the duplicate tray icon as well.

## Syncing & Upload Issues

### Files Are Not Syncing
If a file seems to be ignored completely, check:
1. the watch folder was selected through the app
2. the file extension is supported (Only Immich-compatible image and video formats are recognized)
3. the file finished writing to disk (temporary files are ignored)
4. the API key and server URLs are valid, and the key has the required permissions (`user.read`, `asset.upload`, `asset.update`, `album.read`, `album.create`, `albumAsset.create` — see Configuration & First Run for the full table)
5. folder rules are not excluding the file (hidden files, or max-size restrictions)

> **Check the Queue Inspector**
> The built-in Queue Inspector can tell you instantly if files are failing to upload.

> **Test Connection**
> If you suspect network issues, use the Ping Test dialog to test server reachability.

### Checksums / Deduplication Failures
If Immich re-uploads existing files:
- Ensure the server has finished processing existing assets.
- The app checks for `.device_asset_id` uniqueness from the server using a full 40-character SHA1 hex string. Verify that `sha1` checksums match.

### Mimick Stays Paused
If uploads do not resume on their own:
- Open the **Status** page and check the headline. Mimick shows the pause reason below it.
- If you manually paused it, use **Pause / Resume** from the tray or settings window.
- If **Pause on Metered Networks** is enabled, Mimick may pause while the system reports a metered connection (through the network portal in the Flatpak).
- If **Pause on Battery** is enabled, Mimick may pause while the system appears to be running on battery according to `/sys/class/power_supply`.

## Logs & Diagnostics

### Clearing the Upload Queue (Local Cache)
If the application gets permanently stuck constantly trying to upload a corrupt or broken file on every start causing a queue blockage, you can manually delete the retry cache offline:
```bash
rm -f ~/.cache/mimick/retries.json
```
*(In Flatpak, use `~/.var/app/dev.nicx.mimick/cache/mimick/retries.json`)*

### Export a Diagnostics Bundle
Use `Export Diagnostics` from the Status page to collect:
- `summary.txt`
- `privacy-note.txt`
- `config.redacted.json`
- `status.redacted.json`
- `retries.redacted.json`
- `synced_index.redacted.json`

API keys, raw logs, full local paths, and raw server URLs are intentionally omitted. The bundle is written to a timestamped `mimick-diagnostics-*` folder.

### Useful Logs

Flatpak log:
```bash
tail -f ~/.var/app/dev.nicx.mimick/cache/mimick/mimick.log
```

Native log:
```bash
tail -f ~/.cache/mimick/mimick.log
```

If running as a systemd service (Native):
```bash
journalctl --user -u mimick -f
```

Terminal run:
```bash
cargo run
```

Both terminal and file logs include timestamps, levels, and source modules.

### Check Configuration Validity
Verify your config file is valid JSON (using native path example):
```bash
cat ~/.config/mimick/config.json | jq .
```
If `jq` reports an error, the file is malformed.

### Cache Files (Advanced)

Important runtime files (Flatpak paths shown, replace with `~/.cache/mimick/` for native):
- `~/.var/app/dev.nicx.mimick/cache/mimick/mimick.log`
- `~/.var/app/dev.nicx.mimick/cache/mimick/retries.json`
- `~/.var/app/dev.nicx.mimick/cache/mimick/synced_index.json`
- `~/.var/app/dev.nicx.mimick/cache/mimick/status.json`
