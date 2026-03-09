# Building the Mimick AppImage

This document details the process for manually building a self-contained AppImage for the Mimick Rust application. This packages the compiled binary along with its icons and desktop files.

> Note: For Rust binaries, we do not package the entire GTK4 dynamic library suite inside the AppImage. The `AppRun` script mounts the AppImage and executes the statically linked parts, but relies on the host system to provide GTK4 and Libadwaita. This keeps the file size around 2MB instead of >100MB while still providing a portable single-file executable.

## Prerequisites

1. A Linux environment (tested on x86_64).
2. Rust toolchain (`cargo`).
3. GTK4 and Libadwaita development headers.
4. `wget` and standard GNU utilities (`cp`, `mkdir`, `chmod`).

---

## Step 1: Prepare the Build Tool

We use the official `appimagetool` to generate the final AppImage.

```bash
# Download the continuous build of AppImageTool
wget -c "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"

# Make it executable
chmod +x appimagetool-x86_64.AppImage
```

---

## Step 2: Build the Release Binary

Compile the Rust application with optimizations enabled. The `Cargo.toml` is configured to strip debug symbols and use Link-Time Optimization (LTO) to keep the binary size minimal.

```bash
cargo build --release
```

---

## Step 3: Create the `AppDir` Structure

AppImages are built from a specifically structured folder named `AppDir`.

```bash
# Create the standard directory tree
mkdir -p AppDir/usr/bin
mkdir -p AppDir/usr/share/applications
mkdir -p AppDir/usr/share/icons/hicolor/256x256/apps
mkdir -p AppDir/usr/share/icons/hicolor/scalable/apps
mkdir -p AppDir/usr/share/metainfo
```

---

## Step 4: Migrate Assets and Code

Copy the compiled binary and graphical assets into the standard locations inside the `AppDir`.

```bash
# Copy binary
cp target/release/mimick AppDir/usr/bin/mimick
chmod +x AppDir/usr/bin/mimick

# Copy icons (root location for AppImage standard, standard location for system)
cp src/assets/icon.png AppDir/mimick.png
cp src/assets/icon.png AppDir/usr/share/icons/hicolor/256x256/apps/mimick.png
```

---

## Step 5: Define Application Metadata

Create the standard Desktop entry inside the `AppDir`. This gives the AppImage a name, an icon, and instructions on what to run.

```bash
cat << 'EOF' > AppDir/usr/share/applications/com.nickcardoso.mimick.desktop
[Desktop Entry]
Name=Mimick
Comment=Automatically upload photos to Immich
Exec=mimick
Icon=mimick
Type=Application
Categories=Utility;
Actions=Settings;
Terminal=false
Keywords=Photo;Sync;Backup;Immich;

[Desktop Action Settings]
Name=Open Settings
Exec=mimick --settings
EOF

# Provide a root copy for AppImageTool metadata extraction
cp AppDir/usr/share/applications/com.nickcardoso.mimick.desktop AppDir/com.nickcardoso.mimick.desktop
```

---

## Step 6: Create the `AppRun` Hook

`AppRun` is the entry point for the AppImage. Since this is a compiled Rust binary, we simply execute it.

```bash
cat << 'EOF' > AppDir/AppRun
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/mimick" "$@"
EOF

# Make the hook executable
chmod +x AppDir/AppRun
```

---

## Step 7: Build the AppImage

Invoke the `appimagetool` to compress the `AppDir` directory into a monolithic `.AppImage` file using `squashfs`.

```bash
# Extract version from Cargo.toml
VERSION=$(grep '^version' Cargo.toml | head -1 | cut -d '"' -f2)

# Build the AppImage
ARCH=x86_64 ./appimagetool-x86_64.AppImage AppDir "Mimick-$VERSION-x86_64.AppImage"

chmod +x "Mimick-$VERSION-x86_64.AppImage"
```

---

## Step 8: Clean Up

Remove the temporary utilities and directories.

```bash
rm -rf AppDir
rm appimagetool-x86_64.AppImage
```

The resulting `Mimick-<VERSION>-x86_64.AppImage` is now ready for distribution. Or simply use the bundled `./build_test_appimage.sh` script to automate all of the above!
