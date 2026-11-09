use crate::i18n::gettext_noop;
use gettextrs::gettext;
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
    key(gettext_noop("Show keyboard shortcuts"), "<Control>question"),
    key(gettext_noop("Open Settings"), "<Control>comma"),
    key(gettext_noop("Toggle sidebar"), "F9"),
    key(gettext_noop("Refresh"), "F5"),
    key(gettext_noop("Search"), "<Control>k"),
    hint(
        gettext_noop("Paste photos"),
        "<Control>v",
        gettext_noop("Into the open album, or Photos"),
    ),
    hint(
        gettext_noop("Go back"),
        "<Alt>Left",
        gettext_noop("Or the mouse back button"),
    ),
    hint(
        gettext_noop("Go forward"),
        "<Alt>Right",
        gettext_noop("Or the mouse forward button"),
    ),
    hint(
        gettext_noop("Open context menu"),
        "<Shift>F10",
        gettext_noop("Or the Menu key; in the grid, the viewer, and on album tiles"),
    ),
];

const GO_TO: &[Shortcut] = &[
    key(gettext_noop("Photos"), "<Control>1"),
    key(gettext_noop("Explore"), "<Control>2"),
    key(gettext_noop("Map"), "<Control>3"),
    key(gettext_noop("People"), "<Control>4"),
    key(gettext_noop("Favorites"), "<Control>5"),
    key(gettext_noop("Archive"), "<Control>6"),
    key(gettext_noop("Trash"), "<Control>7"),
    key(gettext_noop("Albums"), "<Control>8"),
    key(gettext_noop("Sharing"), "<Control>9"),
];

const GRID: &[Shortcut] = &[
    hint(
        gettext_noop("Move between photos"),
        "Up Down Left Right",
        gettext_noop("After clicking a photo or tabbing to the grid"),
    ),
    key(gettext_noop("First or last photo"), "Home End"),
    key(gettext_noop("Open photo"), "Return"),
    key(gettext_noop("Select or deselect photo"), "space"),
];

const SELECTION: &[Shortcut] = &[
    hint(
        gettext_noop("Select assets"),
        "Control_L",
        gettext_noop("Hold and click to add or remove assets"),
    ),
    hint(
        gettext_noop("Select a range"),
        "Shift_L",
        gettext_noop("Hold and click; add Ctrl to extend the selection"),
    ),
    hint(
        gettext_noop("Move selection to trash"),
        "Delete",
        gettext_noop("Deletes permanently in the Trash view"),
    ),
    hint(
        gettext_noop("Select all"),
        "<Control>a",
        gettext_noop("Every photo in the view, loading the rest first"),
    ),
    hint(
        gettext_noop("Select a whole day"),
        "<Shift>space",
        gettext_noop("The focused photo's day or month; or tap its date heading while selecting"),
    ),
    hint(
        gettext_noop("Copy selection"),
        "<Control>c",
        gettext_noop("Paste into an album to add them; one photo copies as a picture"),
    ),
    key(gettext_noop("Exit selection mode"), "Escape"),
];

const TIMELINE: &[Shortcut] = &[
    hint(
        gettext_noop("Next or previous day"),
        "d <Shift>d",
        gettext_noop("In Photos, sorted newest first"),
    ),
    key(gettext_noop("Next or previous month"), "m <Shift>m"),
    key(gettext_noop("Next or previous year"), "y <Shift>y"),
];

const LIGHTBOX: &[Shortcut] = &[
    key(gettext_noop("Previous asset"), "Left"),
    key(gettext_noop("Next asset"), "Right"),
    key(gettext_noop("Toggle details"), "i"),
    key(
        gettext_noop("Show or hide all actions for the photo"),
        "F10",
    ),
    key(gettext_noop("Toggle favorite"), "f"),
    key(gettext_noop("Archive or unarchive"), "a"),
    hint(
        gettext_noop("Rate"),
        "1 2 3 4 5",
        gettext_noop("0 clears the rating"),
    ),
    key(gettext_noop("Edit info"), "e"),
    key(gettext_noop("Download"), "<Shift>d"),
    hint(
        gettext_noop("Copy the photo"),
        "<Control>c",
        gettext_noop("While recognized text shows, copies the text instead"),
    ),
    hint(
        gettext_noop("Show or hide recognized text"),
        "t",
        gettext_noop("Only when Immich found text in the photo"),
    ),
    hint(
        gettext_noop("Select all recognized text"),
        "<Control>a",
        gettext_noop("Drag across text to select it; Ctrl-click adds or removes a box"),
    ),
    hint(
        gettext_noop("Copy recognized text"),
        "<Control>c",
        gettext_noop("Copies the selection, or all text when nothing is selected"),
    ),
    key(gettext_noop("Move to trash"), "Delete"),
    hint(
        gettext_noop("Zoom in"),
        "<Control>plus",
        gettext_noop("Or hold Ctrl and scroll"),
    ),
    key(gettext_noop("Zoom out"), "<Control>minus"),
    key(gettext_noop("Reset zoom"), "<Control>0"),
    hint(
        gettext_noop("Toggle fullscreen"),
        "F11",
        gettext_noop("Or the header button; double-click a video; Esc exits"),
    ),
    hint(
        gettext_noop("Play or stop a live photo"),
        "space",
        gettext_noop("Only on live and motion photos"),
    ),
    key(gettext_noop("Start slideshow"), "s"),
    key(gettext_noop("Close"), "Escape"),
];

const SLIDESHOW: &[Shortcut] = &[
    key(gettext_noop("Pause or resume"), "space"),
    hint(
        gettext_noop("Previous or next photo"),
        "Left Right",
        gettext_noop("The next photo then gets the full interval"),
    ),
    hint(
        gettext_noop("End slideshow"),
        "Escape",
        gettext_noop("Or S, or leave fullscreen"),
    ),
];

const PANORAMA: &[Shortcut] = &[
    hint(
        gettext_noop("Switch the 360° view"),
        "p",
        gettext_noop("Only on 360° photos, which open in it"),
    ),
    key(gettext_noop("Tilt up or down"), "Up Down"),
    key(
        gettext_noop("Turn left or right"),
        "<Shift>Left <Shift>Right",
    ),
    hint(
        gettext_noop("Zoom in or out"),
        "plus minus",
        gettext_noop("Or scroll, or pinch"),
    ),
];

const VIDEO: &[Shortcut] = &[
    key(gettext_noop("Play or pause"), "space"),
    key(gettext_noop("Back 10 seconds"), "j"),
    key(gettext_noop("Forward 10 seconds"), "l"),
    hint(
        gettext_noop("Previous frame"),
        "comma",
        gettext_noop("Pauses playback"),
    ),
    hint(
        gettext_noop("Next frame"),
        "period",
        gettext_noop("Pauses playback"),
    ),
    key(gettext_noop("Mute or unmute"), "m"),
    key(gettext_noop("Volume up"), "Up"),
    key(gettext_noop("Volume down"), "Down"),
];

const FOLDERS: &[Shortcut] = &[key(gettext_noop("Enter a folder path"), "<Control>l")];

const MAP: &[Shortcut] = &[
    key(gettext_noop("Search places"), "<Control>f"),
    key(gettext_noop("Zoom in"), "plus"),
    key(gettext_noop("Zoom out"), "minus"),
    key(gettext_noop("Rotate left"), "bracketleft"),
    key(gettext_noop("Rotate right"), "bracketright"),
    key(gettext_noop("Point north"), "n"),
    key(gettext_noop("Show my location"), "<Control>l"),
    hint(
        gettext_noop("Clear the search"),
        "Escape",
        gettext_noop("While typing in the search field"),
    ),
    hint(
        gettext_noop("Move the map"),
        "Up Down Left Right",
        gettext_noop("After clicking the map"),
    ),
];

const SECTIONS: &[(&str, &[Shortcut])] = &[
    (gettext_noop("General"), GENERAL),
    (gettext_noop("Go To"), GO_TO),
    (gettext_noop("Grid"), GRID),
    (gettext_noop("Selection"), SELECTION),
    (gettext_noop("Timeline"), TIMELINE),
    (gettext_noop("Map"), MAP),
    (gettext_noop("Folders"), FOLDERS),
    (gettext_noop("Viewer"), LIGHTBOX),
    (gettext_noop("Slideshow"), SLIDESHOW),
    (gettext_noop("360° Photos"), PANORAMA),
    (gettext_noop("Video"), VIDEO),
];

pub(super) fn present(parent: &impl IsA<gtk::Widget>) {
    let dialog = libadwaita::ShortcutsDialog::new();
    for (title, shortcuts) in SECTIONS {
        let section = libadwaita::ShortcutsSection::new(Some(&gettext(*title)));
        for shortcut in *shortcuts {
            let item =
                libadwaita::ShortcutsItem::new(&gettext(shortcut.title), shortcut.accelerator);
            if !shortcut.subtitle.is_empty() {
                item.set_subtitle(&gettext(shortcut.subtitle));
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
