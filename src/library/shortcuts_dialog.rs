use gtk::prelude::*;
use libadwaita::prelude::*;

struct Shortcut {
    title: &'static str,
    accelerator: &'static str,
    subtitle: &'static str,
}

const fn key(title: &'static str, accelerator: &'static str) -> Shortcut {
    Shortcut {
        title,
        accelerator,
        subtitle: "",
    }
}

const fn hint(title: &'static str, accelerator: &'static str, subtitle: &'static str) -> Shortcut {
    Shortcut {
        title,
        accelerator,
        subtitle,
    }
}

const GENERAL: &[Shortcut] = &[
    key("Show keyboard shortcuts", "<Control>question"),
    key("Toggle sidebar", "F9"),
    key("Refresh", "F5"),
    hint("Go back", "<Alt>Left", "Or the mouse back button"),
    hint("Go forward", "<Alt>Right", "Or the mouse forward button"),
];

const SELECTION: &[Shortcut] = &[
    hint(
        "Select assets",
        "Control_L",
        "Hold and click to add or remove assets",
    ),
    hint(
        "Select a range",
        "Shift_L",
        "Hold and click; add Ctrl to extend the selection",
    ),
    hint(
        "Move selection to trash",
        "Delete",
        "Deletes permanently in the Trash view",
    ),
    key("Exit selection mode", "Escape"),
];

const LIGHTBOX: &[Shortcut] = &[
    key("Previous asset", "Left"),
    key("Next asset", "Right"),
    key("Toggle details", "i"),
    key("Toggle favorite", "f"),
    key("Archive or unarchive", "a"),
    hint(
        "Show or hide recognized text",
        "t",
        "Only when Immich found text in the photo",
    ),
    hint(
        "Select all recognized text",
        "<Control>a",
        "Drag across text to select it; Ctrl-click adds or removes a box",
    ),
    hint(
        "Copy recognized text",
        "<Control>c",
        "Copies the selection, or all text when nothing is selected",
    ),
    key("Move to trash", "Delete"),
    hint("Zoom in", "<Control>plus", "Or hold Ctrl and scroll"),
    key("Zoom out", "<Control>minus"),
    key("Reset zoom", "<Control>0"),
    hint(
        "Toggle fullscreen",
        "F11",
        "Or the header button; double-click a video; Esc exits",
    ),
    key("Close", "Escape"),
];

const VIDEO: &[Shortcut] = &[
    key("Play or pause", "space"),
    key("Back 10 seconds", "j"),
    key("Forward 10 seconds", "l"),
    hint("Previous frame", "comma", "Pauses playback"),
    hint("Next frame", "period", "Pauses playback"),
    key("Mute or unmute", "m"),
];

const SECTIONS: &[(&str, &[Shortcut])] = &[
    ("General", GENERAL),
    ("Selection", SELECTION),
    ("Viewer", LIGHTBOX),
    ("Video", VIDEO),
];

pub(super) fn present(parent: &impl IsA<gtk::Widget>) {
    let dialog = libadwaita::ShortcutsDialog::new();
    for (title, shortcuts) in SECTIONS {
        let section = libadwaita::ShortcutsSection::new(Some(title));
        for shortcut in *shortcuts {
            let item = libadwaita::ShortcutsItem::new(shortcut.title, shortcut.accelerator);
            if !shortcut.subtitle.is_empty() {
                item.set_subtitle(shortcut.subtitle);
            }
            section.add(item);
        }
        dialog.add(section);
    }
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_accelerator_names_a_real_key() {
        for (_, shortcuts) in SECTIONS {
            for shortcut in *shortcuts {
                for combo in shortcut.accelerator.split([' ', '&']) {
                    let name = combo.rsplit('>').next().unwrap_or(combo);
                    assert!(
                        gtk::gdk::Key::from_name(name).is_some(),
                        "unknown key {name:?} for {:?}",
                        shortcut.title
                    );
                }
            }
        }
    }
}
