//! The selection bar's "More actions" menu: actions that only live there, plus the bar's
//! secondary buttons on phone widths.

use gettextrs::gettext;
use gtk::prelude::*;

use super::header_format::capitalize;

const MINOR_WORDS: [&str; 10] = [
    "a", "an", "the", "and", "for", "from", "in", "of", "on", "to",
];

/// Menu button for the selection bar's other actions. `inline` buttons show here only while
/// `inline_row` is hidden (phone widths); `extras` live only here. Each item clicks its source
/// button and follows its icon, tooltip, and visibility.
pub(super) fn build(
    inline: &[gtk::Button],
    inline_row: &gtk::Box,
    extras: &[gtk::Button],
) -> gtk::MenuButton {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    let popover = gtk::Popover::builder().child(&content).build();
    let menu = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(gettext("More actions"))
        .direction(gtk::ArrowType::Up)
        .popover(&popover)
        .build();
    let items: Vec<gtk::Button> = inline
        .iter()
        .map(|source| mirror(source, &popover, Some(inline_row)))
        .chain(extras.iter().map(|source| mirror(source, &popover, None)))
        .collect();
    for item in &items {
        content.append(item);
    }
    // The visible property, not `is_visible`: that also asks every ancestor, and the items
    // sit in a closed popover, so it would hide the menu for good.
    let sync = {
        let (menu, items) = (menu.clone(), items.clone());
        move || menu.set_visible(items.iter().any(WidgetExt::get_visible))
    };
    for item in &items {
        let sync = sync.clone();
        item.connect_visible_notify(move |_| sync());
    }
    sync();
    menu
}

/// A menu item for `source`, hidden while `inline_row` shows the button itself.
fn mirror(
    source: &gtk::Button,
    popover: &gtk::Popover,
    inline_row: Option<&gtk::Box>,
) -> gtk::Button {
    let icon = gtk::Image::new();
    let label = gtk::Label::builder().xalign(0.0).hexpand(true).build();
    let row = gtk::Box::builder().spacing(12).build();
    row.append(&icon);
    row.append(&label);
    let item = gtk::Button::builder()
        .child(&row)
        .css_classes(["flat"])
        .build();
    let set_label = move |button: &gtk::Button| {
        label.set_label(&overflow_label(&button.tooltip_text().unwrap_or_default()))
    };
    set_label(source);
    source.connect_tooltip_text_notify(set_label);
    source
        .bind_property("icon-name", &icon, "icon-name")
        .sync_create()
        .build();
    let show = {
        let (source, item, inline_row) = (source.clone(), item.clone(), inline_row.cloned());
        move || {
            // Properties again: a button in the hidden row would never count as shown.
            let inline_shown = inline_row.as_ref().is_some_and(WidgetExt::get_visible);
            item.set_visible(source.get_visible() && !inline_shown);
        }
    };
    show();
    source.connect_visible_notify({
        let show = show.clone();
        move |_| show()
    });
    if let Some(row) = inline_row {
        row.connect_visible_notify(move |_| show());
    }
    let (source, popover) = (source.clone(), popover.clone());
    item.connect_clicked(move |_| {
        popover.popdown();
        source.emit_clicked();
    });
    item
}

/// Menu-item text for a selection-bar tooltip, e.g. "Archive selected" → "Archive".
pub(super) fn overflow_label(tooltip: &str) -> String {
    let action = tooltip
        .strip_suffix(" for selected")
        .or_else(|| tooltip.strip_suffix(" selected"))
        .unwrap_or(tooltip);
    action
        .split(' ')
        .enumerate()
        .map(|(i, word)| {
            if i > 0 && MINOR_WORDS.contains(&word) {
                word.to_string()
            } else {
                capitalize(word)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::overflow_label;

    #[test]
    fn overflow_labels_drop_the_selection_suffix() {
        assert_eq!(
            overflow_label("Create share link for selected"),
            "Create Share Link"
        );
        assert_eq!(overflow_label("Archive selected"), "Archive");
        assert_eq!(overflow_label("Unarchive selected"), "Unarchive");
        assert_eq!(overflow_label("Set location for selected"), "Set Location");
        assert_eq!(overflow_label("Change date for selected"), "Change Date");
        assert_eq!(overflow_label("Remove from album"), "Remove from Album");
    }
}
