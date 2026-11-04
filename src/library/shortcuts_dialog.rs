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
    key("Search", "<Control>k"),
    hint("Go back", "<Alt>Left", "Or the mouse back button"),
    hint("Go forward", "<Alt>Right", "Or the mouse forward button"),
    hint(
        "Open context menu",
        "<Shift>F10",
        "Or the Menu key; in the grid, the viewer, and on album tiles",
    ),
];

const GO_TO: &[Shortcut] = &[
    key("Photos", "<Control>1"),
    key("Explore", "<Control>2"),
    key("Map", "<Control>3"),
    key("People", "<Control>4"),
    key("Favorites", "<Control>5"),
    key("Archive", "<Control>6"),
    key("Trash", "<Control>7"),
    key("Albums", "<Control>8"),
    key("Sharing", "<Control>9"),
];

const GRID: &[Shortcut] = &[
    hint(
        "Move between photos",
        "Up Down Left Right",
        "After clicking a photo or tabbing to the grid",
    ),
    key("First or last photo", "Home End"),
    key("Open photo", "Return"),
    key("Select or deselect photo", "space"),
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
    hint("Select all", "<Control>a", "Every photo loaded in the grid"),
    key("Exit selection mode", "Escape"),
];

const TIMELINE: &[Shortcut] = &[
    hint(
        "Next or previous day",
        "d <Shift>d",
        "In Photos, sorted newest first",
    ),
    key("Next or previous month", "m <Shift>m"),
    key("Next or previous year", "y <Shift>y"),
];

const LIGHTBOX: &[Shortcut] = &[
    key("Previous asset", "Left"),
    key("Next asset", "Right"),
    key("Toggle details", "i"),
    key("Toggle favorite", "f"),
    key("Archive or unarchive", "a"),
    hint("Rate", "1 2 3 4 5", "0 clears the rating"),
    key("Edit info", "e"),
    key("Download", "<Shift>d"),
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

const MAP: &[Shortcut] = &[
    key("Search places", "<Control>f"),
    key("Zoom in", "plus"),
    key("Zoom out", "minus"),
    key("Rotate left", "bracketleft"),
    key("Rotate right", "bracketright"),
    key("Point north", "n"),
    key("Show my location", "<Control>l"),
    hint(
        "Clear the search",
        "Escape",
        "While typing in the search field",
    ),
    hint(
        "Move the map",
        "Up Down Left Right",
        "After clicking the map",
    ),
];

const SECTIONS: &[(&str, &[Shortcut])] = &[
    ("General", GENERAL),
    ("Go To", GO_TO),
    ("Grid", GRID),
    ("Selection", SELECTION),
    ("Timeline", TIMELINE),
    ("Map", MAP),
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
