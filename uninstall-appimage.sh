#!/bin/bash
# uninstall-appimage.sh - Removes the Mimick AppImage for the current user

APP_NAME="mimick"
USER_BIN="$HOME/.local/bin"
USER_APPS="$HOME/.local/share/applications"
USER_ICONS="$HOME/.local/share/icons/hicolor/256x256/apps"
USER_ICONS_SCALABLE="$HOME/.local/share/icons/hicolor/scalable/apps"
TARGET_APPIMAGE="$USER_BIN/mimick.AppImage"
AUTOSTART_DIR="$HOME/.config/autostart"

echo "=== Uninstalling Mimick AppImage ==="

if [ -f "$TARGET_APPIMAGE" ]; then
    rm "$TARGET_APPIMAGE"
    echo "Removed executable."
fi

# Remove old and new desktop entries
for ID in "mimick" "com.nickcardoso.mimick" "io.github.nicx17.mimick"; do
    if [ -f "$USER_APPS/$ID.desktop" ]; then
        rm "$USER_APPS/$ID.desktop"
        echo "Removed desktop entry ($ID)."
    fi

    if [ -f "$AUTOSTART_DIR/$ID.desktop" ]; then
        rm "$AUTOSTART_DIR/$ID.desktop"
        echo "Removed autostart entry ($ID)."
    fi

    if [ -f "$USER_ICONS/$ID.png" ]; then
        rm "$USER_ICONS/$ID.png"
        echo "Removed PNG icon ($ID)."
    fi

    if [ -f "$USER_ICONS_SCALABLE/$ID.svg" ]; then
        rm "$USER_ICONS_SCALABLE/$ID.svg"
        echo "Removed SVG icon ($ID)."
    fi
done

gtk-update-icon-cache "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

echo "Uninstallation Complete."
