#!/usr/bin/env bash
# Builds a self-contained, SquashFS-based Mimick AppImage with quick-sharun
# (pkgforge-dev/Anylinux-AppImages) and AppImageKit's appimagetool.
#
# It bundles glibc and the dynamic loader along with GTK, libadwaita and GStreamer, so it runs on
# distros far older than the GTK 4.24 / libadwaita 1.10 the app needs, and on musl systems.
# Run as root inside ghcr.io/pkgforge-dev/archlinux, from the repository root. Writes
# dist/Mimick-<version>-<arch>.AppImage and its .zsync.

set -euo pipefail

# quick-sharun pins the checksums of everything it downloads itself (sharun, the AppImage
# runtime, ...); pin its scripts and the final AppImage builder as well. Pick a commit at least
# two weeks old.
ANYLINUX_COMMIT=e97c4932e3b445a2de8136253e4cf41537896723
QUICK_SHARUN_SHA256=33b661316746a8152cef2e5b65cc8a83da2e7512db970d20821b0eccc86e5eb1
GET_DEBLOATED_PKGS_SHA256=217ff68aafe085d2d4ab34a50e267805af43736ad8ef880c01ef88e52db8589f
APPIMAGETOOL_X86_64_SHA256=df3baf5ca5facbecfc2f3fa6713c29ab9cefa8fd8c1eac5d283b79cab33e4acb
APPIMAGETOOL_AARCH64_SHA256=334e77beb67fc1e71856c29d5f3f324ca77b0fde7a840fdd14bd3b88c25c341f

ARCH=$(uname -m)
VERSION=$(awk -F '"' '/^\[package\]/{p=1} p && /^version/ {print $2; exit}' Cargo.toml)
if [ -z "$VERSION" ]; then
    echo "Could not read the version from Cargo.toml" >&2
    exit 1
fi

tools=$(mktemp -d)

# Downloads an Anylinux helper script and refuses to run it if its checksum changed.
fetch_anylinux_tool() {
    local name=$1 sha=$2
    curl -fsSL --retry 5 \
        "https://raw.githubusercontent.com/pkgforge-dev/Anylinux-AppImages/${ANYLINUX_COMMIT}/useful-tools/${name}.sh" \
        -o "$tools/$name"
    echo "$sha  $tools/$name" | sha256sum --check --strict
    chmod +x "$tools/$name"
}

# quick-sharun's bundled appimagetool intentionally produces DwarFS images. The AppImage catalog
# currently requires SquashFS, so build the already-prepared AppDir with the standard Type 2
# appimagetool instead. These immutable release assets provide a native builder for each CI arch.
fetch_squashfs_appimagetool() {
    local name sha
    case "$ARCH" in
        x86_64)
            name=obsolete-appimagetool-x86_64.AppImage
            sha=$APPIMAGETOOL_X86_64_SHA256
            ;;
        aarch64)
            name=obsolete-appimagetool-aarch64.AppImage
            sha=$APPIMAGETOOL_AARCH64_SHA256
            ;;
        *)
            echo "Unsupported AppImage architecture: $ARCH" >&2
            exit 1
            ;;
    esac
    curl -fsSL --retry 5 "https://github.com/AppImage/AppImageKit/releases/download/13/$name" \
        -o "$tools/appimagetool"
    echo "$sha  $tools/appimagetool" | sha256sum --check --strict
    chmod +x "$tools/appimagetool"
}

# The same build and runtime packages as packaging/aur/mimick/PKGBUILD, plus the tools quick-sharun
# uses. quick-sharun only bundles GStreamer plugins that are installed here, so the playback,
# video sink and codec plugins must be listed explicitly.
install_dependencies() {
    pacman-key --init
    pacman -Syy --noconfirm archlinux-keyring
    pacman -Syu --needed --noconfirm \
        base-devel clang cmake curl dbus gettext git nasm patchelf pkgconf rust strace wget \
        xorg-server-xvfb zsync gtk-update-icon-cache appstream \
        adwaita-icon-theme glib-networking gtk4 libadwaita libheif libshumate openssl \
        gstreamer gst-libav gst-plugin-gtk4 gst-plugins-bad-libs gst-plugins-base gst-plugins-good

    # Swaps in slimmed-down GTK, Mesa and FFmpeg builds. Must run after the last `pacman -S`,
    # or pacman puts the full packages back.
    fetch_anylinux_tool get-debloated-pkgs "$GET_DEBLOATED_PKGS_SHA256"
    "$tools/get-debloated-pkgs" --add-common --prefer-nano ffmpeg-mini
}

# Adds what quick-sharun can't find by tracing the binary: the app's own data files, and the
# Adwaita icons for the standard symbolic icons, which hosts without GNOME may not have.
add_app_data() {
    local icons="$APPDIR/share/icons"
    scripts/i18n.sh merge setup/metainfo/dev.nicx.mimick.metainfo.xml "$APPDIR/share/metainfo/dev.nicx.mimick.metainfo.xml"
    mkdir -p "$APPDIR/usr/share/metainfo"
    cp "$APPDIR/share/metainfo/dev.nicx.mimick.metainfo.xml" "$APPDIR/usr/share/metainfo/dev.nicx.mimick.metainfo.xml"
    cp "$APPDIR/share/metainfo/dev.nicx.mimick.metainfo.xml" "$APPDIR/usr/share/metainfo/dev.nicx.mimick.appdata.xml"
    install -Dm644 src/assets/256x256/apps/dev.nicx.mimick.png -t "$icons/hicolor/256x256/apps/"
    install -Dm644 src/assets/512x512/apps/dev.nicx.mimick.png -t "$icons/hicolor/512x512/apps/"
    install -Dm644 src/assets/scalable/apps/dev.nicx.mimick.svg -t "$icons/hicolor/scalable/apps/"
    install -Dm644 src/assets/symbolic/apps/dev.nicx.mimick-symbolic.svg -t "$icons/hicolor/symbolic/apps/"
    install -Dm644 LICENSE -t "$APPDIR/share/licenses/mimick/"
    scripts/i18n.sh install "$APPDIR/share/locale"

    # Cursors are 12 of the theme's 15 MB, and the host's cursor theme is used anyway.
    if [ ! -d "$icons/Adwaita" ]; then
        cp -r /usr/share/icons/Adwaita "$icons/"
        rm -rf "$icons/Adwaita/cursors"
    fi
    gtk-update-icon-cache -f -t "$icons/Adwaita" || :
    gtk-update-icon-cache -f -t "$icons/hicolor" || :

    # GLib would send notifications through org.gtk.Notifications, which GNOME Shell rejects for
    # an app ID with no installed desktop file, and an AppImage installs none.
    echo 'GNOTIFICATION_BACKEND=freedesktop' >> "$APPDIR/.env"
}

# Fails the build if quick-sharun silently stopped bundling something. The AppImage would still
# work on this container, which has everything installed, and break only on users' machines.
check_bundled() {
    local missing=0 pattern found
    # GTK and glibc, GStreamer's playbin, video sink and FFmpeg decoders, the TLS module for map
    # tiles (libsoup), and libheif.
    for pattern in 'libgtk-4.so*' 'libadwaita-1.so*' 'libc.so.6' 'ld-linux-*.so*' \
        'libgstplayback.so' 'libgstgtk4.so' 'libgstlibav.so' 'libgiognutls.so' 'libheif.so*'; do
        found=$(find "$APPDIR" -name "$pattern" -print -quit)
        if [ -z "$found" ]; then
            echo "Not bundled: $pattern" >&2
            missing=1
        else
            echo "Bundled: ${found#"$APPDIR"/}"
        fi
    done
    return "$missing"
}

install_dependencies

cargo build --release --locked

fetch_anylinux_tool quick-sharun "$QUICK_SHARUN_SHA256"
fetch_squashfs_appimagetool
export APPDIR="$PWD/AppDir"
export OUTPATH="$PWD/dist"
export OUTNAME="Mimick-$VERSION-$ARCH.AppImage"
# Lets AppImage updaters (Gear Lever, AppImageUpdate) fetch the newest release's .zsync.
export UPINFO="gh-releases-zsync|nicx17|mimick|latest|Mimick-*$ARCH.AppImage.zsync"
scripts/i18n.sh merge setup/dev.nicx.mimick.desktop "$tools/dev.nicx.mimick.desktop"
export DESKTOP="$tools/dev.nicx.mimick.desktop"
export ICON="$PWD/src/assets/scalable/apps/dev.nicx.mimick.svg"
# quick-sharun runs the app to record what it dlopens; give the settings window time to draw.
export STRACE_TIME=10
export DEPLOY_GSTREAMER=1
rm -rf "$APPDIR" "$OUTPATH"

# The container has no session bus, and without one GApplication fails to register and exits
# before GTK loads its renderer, so the trace would miss the libraries it dlopens.
dbus-run-session -- "$tools/quick-sharun" target/release/mimick
add_app_data
check_bundled

# AppImageKit's tool uses SquashFS, unlike quick-sharun's bundled DwarFS-only packager. Running
# in extract-and-run mode avoids requiring FUSE while building and testing in the CI container.
export APPIMAGE_EXTRACT_AND_RUN=1
mkdir -p "$OUTPATH"
"$tools/appimagetool" --updateinformation "$UPINFO" "$APPDIR" "$OUTPATH/$OUTNAME"
# Starts the AppImage under a virtual display and fails if it exits within 12 seconds.
dbus-run-session -- "$tools/quick-sharun" --test "$OUTPATH/$OUTNAME"
ls -l "$OUTPATH"
