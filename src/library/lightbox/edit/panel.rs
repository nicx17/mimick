//! Edit mode's widgets: the header, and a page of controls per tool under the photo, with a
//! switcher between the tools at the bottom where a thumb reaches it.

use gettextrs::gettext;

use gtk::prelude::*;

use super::pages;
use super::{TOOLS, Tool};

/// A slider or wheel with its value beside it and a button that puts it back to its rest.
pub(super) struct Dial {
    pub(super) root: gtk::Box,
    /// The value, which the slider or wheel shows and moves.
    pub(super) adjustment: gtk::Adjustment,
    /// The slider or wheel, named for screen readers.
    pub(super) control: gtk::Widget,
    value: gtk::Label,
    pub(super) reset: gtk::Button,
}

impl Dial {
    /// A slider from `min` to `max` in whole steps, marked where it rests.
    pub(super) fn new(name: &str, (min, max): (f64, f64), rest: f64) -> Self {
        let adjustment = gtk::Adjustment::new(rest, min, max, 1.0, 10.0, 0.0);
        let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
        scale.set_hexpand(true);
        scale.set_draw_value(false);
        scale.add_mark(rest, gtk::PositionType::Bottom, None);
        scale.update_property(&[gtk::accessible::Property::Label(name)]);
        Self::around(scale.upcast(), adjustment)
    }

    /// A thumbwheel for fine turns: `step` per tick of movement, `page` with Shift.
    pub(super) fn wheel(
        name: &str,
        (min, max): (f64, f64),
        rest: f64,
        (step, page): (f64, f64),
    ) -> Self {
        let adjustment = gtk::Adjustment::new(rest, min, max, step, page, 0.0);
        let wheel = super::wheel::thumbwheel(&adjustment, name, rest);
        Self::around(wheel.upcast(), adjustment)
    }

    fn around(control: gtk::Widget, adjustment: gtk::Adjustment) -> Self {
        let value = gtk::Label::builder()
            .width_chars(5)
            .xalign(1.0)
            .css_classes(["numeric", "caption"])
            .build();
        let reset = gtk::Button::builder()
            .icon_name("edit-undo-symbolic")
            .tooltip_text(gettext("Reset"))
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        let root = gtk::Box::builder().spacing(6).hexpand(true).build();
        root.append(&control);
        root.append(&value);
        root.append(&reset);
        Self {
            root,
            adjustment,
            control,
            value,
            reset,
        }
    }

    pub(super) fn show_value(&self, text: &str) {
        self.value.set_label(text);
    }
}

/// Every control edit mode has, built once with the lightbox.
pub(super) struct EditWidgets {
    /// Stands in for the lightbox header while editing.
    pub(super) header: libadwaita::HeaderBar,
    /// Stands in for the action bar under the photo while editing.
    pub(super) panel: gtk::Box,
    pub(super) title: libadwaita::WindowTitle,
    pub(super) cancel: gtk::Button,
    pub(super) save: gtk::Button,
    pub(super) reset: gtk::Button,
    pub(super) revert: gtk::Button,
    /// A saved copy keeps the original's metadata and albums; off, it's a new photo.
    pub(super) keep_details: gtk::CheckButton,
    /// A saved copy becomes the cover of a stack with the original.
    pub(super) stack_copy: gtk::CheckButton,
    pub(super) tools: libadwaita::ViewStack,
    pub(super) switcher: libadwaita::ViewSwitcher,
    pub(super) crop: pages::CropPage,
    pub(super) adjust: pages::AdjustPage,
    pub(super) filters: pages::FiltersPage,
    pub(super) redact: pages::RedactPage,
}

impl EditWidgets {
    pub(super) fn new() -> Self {
        let cancel = gtk::Button::with_label(&gettext("Cancel"));
        let save = gtk::Button::builder()
            .label(gettext("Save"))
            .sensitive(false)
            .css_classes(["suggested-action"])
            .build();
        let reset = menu_button_item(&gettext("Reset All"));
        let revert = menu_button_item(&gettext("Revert to Original…"));
        revert.add_css_class("mimick-edit-revert");
        let (keep_details, stack_copy) = copy_choices();
        let title = libadwaita::WindowTitle::new(&gettext("Edit Photo"), "");
        let header = edit_header(
            &title,
            &cancel,
            &save,
            &[&reset, &revert],
            &[&keep_details, &stack_copy],
        );
        let (crop, adjust) = (pages::crop_page(), pages::adjust_page());
        let (filters, redact) = (pages::filters_page(), pages::redact_page());
        let tools = tool_stack([&crop.root, &adjust.root, &filters.root, &redact.root]);
        let (panel, switcher) = tool_panel(&tools);
        Self {
            header,
            panel,
            title,
            cancel,
            save,
            reset,
            revert,
            keep_details,
            stack_copy,
            tools,
            switcher,
            crop,
            adjust,
            filters,
            redact,
        }
    }

    /// The tool whose page is showing.
    pub(super) fn tool(&self) -> Tool {
        let name = self.tools.visible_child_name();
        TOOLS
            .iter()
            .find(|(_, page, _, _)| name.as_deref() == Some(*page))
            .map_or(Tool::Adjust, |(tool, _, _, _)| *tool)
    }

    pub(super) fn show_tool(&self, tool: Tool) {
        if let Some((_, page, _, _)) = TOOLS.iter().find(|(t, _, _, _)| *t == tool) {
            self.tools.set_visible_child_name(page);
        }
    }
}

/// The ⋯ menu's choices for saved copies: keep the original's details, and stack with it.
fn copy_choices() -> (gtk::CheckButton, gtk::CheckButton) {
    let choice = |label: &str, explained: &str| {
        gtk::CheckButton::builder()
            .label(label)
            .tooltip_text(explained)
            .margin_start(6)
            .build()
    };
    (
        choice(
            &gettext("Keep Original Details"),
            &gettext(
                "A saved copy keeps the original's camera details, date, place, caption, \
                 rating, and albums. Off, the copy is a new photo of its own.",
            ),
        ),
        choice(
            &gettext("Stack with Original"),
            &gettext("A saved copy becomes the cover of a stack, with the original under it."),
        ),
    )
}

/// The panel under the photo: the tool's page, with the switcher between tools below it.
fn tool_panel(tools: &libadwaita::ViewStack) -> (gtk::Box, libadwaita::ViewSwitcher) {
    let switcher = libadwaita::ViewSwitcher::builder()
        .stack(tools)
        .policy(libadwaita::ViewSwitcherPolicy::Wide)
        .build();
    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .visible(false)
        .css_classes(["mimick-edit-panel"])
        .build();
    panel.append(tools);
    panel.append(&switcher);
    (panel, switcher)
}

/// One page per tool, in `TOOLS` order; pages keep their own height so the photo gets the rest.
fn tool_stack(pages: [&gtk::Box; 4]) -> libadwaita::ViewStack {
    let stack = libadwaita::ViewStack::builder()
        .vhomogeneous(false)
        .hhomogeneous(false)
        .build();
    for (page, (_, name, title, icon)) in pages.into_iter().zip(TOOLS) {
        stack.add_titled_with_icon(page, Some(name), &gettext(title), icon);
    }
    stack
}

/// Without the window buttons, so Cancel, the title, the menu, and Save fit a phone.
fn edit_header(
    title: &libadwaita::WindowTitle,
    cancel: &gtk::Button,
    save: &gtk::Button,
    items: &[&gtk::Button],
    choices: &[&gtk::CheckButton],
) -> libadwaita::HeaderBar {
    let header = libadwaita::HeaderBar::builder()
        .show_back_button(false)
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .title_widget(title)
        .visible(false)
        .build();
    let list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    for item in items {
        list.append(*item);
    }
    list.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    for choice in choices {
        list.append(*choice);
    }
    let popover = gtk::Popover::builder().child(&list).build();
    for item in items {
        let popover = popover.downgrade();
        item.connect_clicked(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        });
    }
    let menu = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(gettext("More"))
        .popover(&popover)
        .build();
    header.pack_start(cancel);
    header.pack_end(save);
    header.pack_end(&menu);
    header
}

fn menu_button_item(label: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .label(label)
        .css_classes(["flat"])
        .build();
    if let Some(label) = button.child().and_downcast::<gtk::Label>() {
        label.set_xalign(0.0);
    }
    button
}
