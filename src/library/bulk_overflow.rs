//! Phone-width "More actions" menu that mirrors the selection bar's secondary buttons.

use gtk::prelude::*;

use super::header_format::capitalize;

const MINOR_WORDS: [&str; 10] = [
    "a", "an", "the", "and", "for", "from", "in", "of", "on", "to",
];

/// Menu button whose items click `sources` and follow their tooltip and visibility.
pub(super) fn build(sources: &[gtk::Button]) -> gtk::MenuButton {
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
        .tooltip_text("More actions")
        .direction(gtk::ArrowType::Up)
        .popover(&popover)
        .build();
    for source in sources {
        content.append(&mirror(source, &popover));
    }
    let sync = {
        let (menu, sources) = (menu.clone(), sources.to_vec());
        move || menu.set_visible(sources.iter().any(|b| b.is_visible()))
    };
    for source in sources {
        let sync = sync.clone();
        source.connect_visible_notify(move |_| sync());
    }
    sync();
    menu
}

fn mirror(source: &gtk::Button, popover: &gtk::Popover) -> gtk::Button {
    let label = gtk::Label::builder().xalign(0.0).build();
    let item = gtk::Button::builder()
        .child(&label)
        .css_classes(["flat"])
        .build();
    let set_label = move |button: &gtk::Button| {
        label.set_label(&overflow_label(&button.tooltip_text().unwrap_or_default()))
    };
    set_label(source);
    source.connect_tooltip_text_notify(set_label);
    source
        .bind_property("visible", &item, "visible")
        .sync_create()
        .build();
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
