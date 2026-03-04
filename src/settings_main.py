import sys
import logging
from log_setup import setup_logging

setup_logging()

from PySide6.QtWidgets import QApplication
from PySide6.QtCore import Qt
from settings_window import SettingsWindow

def main():
    logging.info("Starting Settings Window (Standalone Process)...")
    
    # In Qt 6 (PySide6), High DPI scaling is enabled by default.
    # No manual attribute setting is required.

    # Set Metadata for DE Integration (GNOME/KDE) MUST be done before QApplication
    # ApplicationName is used for WM_CLASS on some platforms
    QApplication.setApplicationName("immich-sync")
    QApplication.setApplicationDisplayName("Immich Auto-Sync")
    QApplication.setDesktopFileName("immich-sync")

    app = QApplication(sys.argv)
    
    # Run the window
    window = SettingsWindow()
    window.show()

    if "--about" in sys.argv:
        window.show_about_dialog()
    
    sys.exit(app.exec())

if __name__ == "__main__":
    main()