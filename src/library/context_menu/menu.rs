//! A menu of actions, shown as a popover next to what it acts on or as a sheet of icons that
//! slides up from the bottom of the window.

use gettextrs::gettext;
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

/// Popover content plus a helper for full-width buttons that close the menu first.
pub(in crate::library) struct Menu {
    content: gtk::Box,
    popover: gtk::Popover,
    entries: RefCell<Vec<Entry>>,
}

/// One choice, kept so the menu can open as a popover or as a bottom sheet.
struct Entry {
    icon: Option<&'static str>,
    label: String,
    destructive: bool,
    action: Rc<dyn Fn()>,
}

impl Menu {
    pub(in crate::library) fn new(
        parent: &impl gtk::prelude::IsA<gtk::Widget>,
        x: f64,
        y: f64,
    ) -> Self {
        let popover = gtk::Popover::builder()
            .has_arrow(true)
            .autohide(true)
            .build();
        popover.set_parent(parent);
        popover.connect_closed(|popover| {
            let popover = popover.clone();
            glib::idle_add_local_once(move || popover.unparent());
        });
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();
        Self {
            content,
            popover,
            entries: RefCell::default(),
        }
    }

    /// Every choice as an icon tile in a sheet that slides up from the bottom of the window.
    /// The tiles wrap into rows on narrow windows, so all of them show without scrolling.
    pub(in crate::library) fn present_sheet(self, parent: &impl IsA<gtk::Widget>) {
        self.popover.unparent();
        let dialog = libadwaita::Dialog::builder()
            .title(gettext("Photo Actions"))
            .presentation_mode(libadwaita::DialogPresentationMode::BottomSheet)
            .follows_content_size(true)
            .build();
        // Five 64 px tiles fit a 360 px phone, so a typical menu takes about three rows.
        let tiles = libadwaita::WrapBox::builder()
            .child_spacing(4)
            .line_spacing(10)
            .halign(gtk::Align::Center)
            .margin_top(18)
            .margin_bottom(12)
            .margin_start(8)
            .margin_end(8)
            .build();
        for entry in self.entries.take() {
            tiles.append(&sheet_tile(&dialog, entry));
        }
        // Scrolls only when even the rows don't fit, as on a phone held sideways.
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_width(true)
            .propagate_natural_height(true)
            .child(&tiles)
            .build();
        dialog.set_child(Some(&scroller));
        dialog.add_controller(close_on_f10(&dialog));
        dialog.present(Some(parent));
    }

    /// Scrollable, so GTK lets the compositor shrink a menu taller than the screen.
    pub(in crate::library) fn popup(self) {
        let scroller = gtk::ScrolledWindow::builder()
            .child(&self.content)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(true)
            .propagate_natural_height(true)
            .build();
        self.popover.set_child(Some(&scroller));
        self.popover.popup();
    }

    /// Runs `f` when the menu closes, however it was dismissed.
    pub(in crate::library) fn on_closed(&self, f: impl Fn() + 'static) {
        self.popover.connect_closed(move |_| f());
    }

    /// A bold line above the buttons naming what the menu acts on.
    pub(in crate::library) fn heading(&self, text: &str) {
        let label = gtk::Label::builder()
            .label(text)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(28)
            .margin_start(6)
            .margin_end(6)
            .margin_top(2)
            .margin_bottom(2)
            .css_classes(["heading"])
            .build();
        self.content.append(&label);
    }

    pub(in crate::library) fn add(
        &self,
        label: &str,
        destructive: bool,
        on_click: impl Fn() + 'static,
    ) {
        self.add_entry(None, label, destructive, Rc::new(on_click));
    }

    /// Like `add`, with the icon the bottom sheet shows; the popover stays text-only.
    pub(in crate::library) fn add_with_icon(
        &self,
        icon: &'static str,
        label: &str,
        destructive: bool,
        on_click: impl Fn() + 'static,
    ) {
        self.add_entry(Some(icon), label, destructive, Rc::new(on_click));
    }

    fn add_entry(
        &self,
        icon: Option<&'static str>,
        label: &str,
        destructive: bool,
        action: Rc<dyn Fn()>,
    ) {
        let button = gtk::Button::builder()
            .label(label)
            .halign(gtk::Align::Fill)
            .build();
        if destructive {
            button.add_css_class("destructive-action");
        }
        let (popover, on_click) = (self.popover.clone(), action.clone());
        button.connect_clicked(move |_| {
            popover.popdown();
            on_click();
        });
        self.content.append(&button);
        self.entries.borrow_mut().push(Entry {
            icon,
            label: label.to_string(),
            destructive,
            action,
        });
    }
}

/// F10 opens the sheet from the lightbox, so pressing it again closes it.
fn close_on_f10(dialog: &libadwaita::Dialog) -> gtk::ShortcutController {
    let dialog = dialog.downgrade();
    let shortcut = gtk::Shortcut::builder()
        .trigger(&gtk::ShortcutTrigger::parse_string("F10").expect("valid shortcut"))
        .action(&gtk::CallbackAction::new(move |_, _| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
            glib::Propagation::Stop
        }))
        .build();
    let controller = gtk::ShortcutController::new();
    controller.add_shortcut(shortcut);
    controller
}

/// An icon in a round well over its name; picking it closes the sheet first.
fn sheet_tile(dialog: &libadwaita::Dialog, entry: Entry) -> gtk::Button {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .valign(gtk::Align::Start)
        .build();
    content.append(
        &gtk::Image::builder()
            .icon_name(entry.icon.unwrap_or("view-more-symbolic"))
            .pixel_size(20)
            .halign(gtk::Align::Center)
            .css_classes(["mimick-sheet-icon"])
            .build(),
    );
    content.append(
        &gtk::Label::builder()
            .label(&entry.label)
            .justify(gtk::Justification::Center)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(3)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(9)
            .css_classes(["caption"])
            .build(),
    );
    let button = gtk::Button::builder()
        .child(&content)
        .css_classes(["flat", "mimick-sheet-tile"])
        .build();
    if entry.destructive {
        button.add_css_class("mimick-sheet-destructive");
    }
    let (dialog, action) = (dialog.clone(), entry.action);
    button.connect_clicked(move |_| {
        dialog.close();
        action();
    });
    button
}
