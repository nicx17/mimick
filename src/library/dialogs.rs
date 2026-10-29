use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use super::LibraryWindowUi;

pub(super) struct TextPrompt<'a> {
    pub heading: &'a str,
    pub body: &'a str,
    pub initial: &'a str,
    pub placeholder: &'a str,
    pub accept: &'a str,
}

pub(super) fn prompt_text(
    ui: &LibraryWindowUi,
    prompt: TextPrompt<'_>,
    on_accept: impl Fn(String) + 'static,
) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading(prompt.heading)
        .body(prompt.body)
        .build();
    let entry = gtk::Entry::builder()
        .text(prompt.initial)
        .placeholder_text(prompt.placeholder)
        .activates_default(true)
        .build();
    dialog.set_extra_child(Some(&entry));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("accept", prompt.accept);
    dialog.set_response_appearance("accept", libadwaita::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("accept"));
    dialog.set_close_response("cancel");
    dialog.set_response_enabled("accept", !prompt.initial.trim().is_empty());
    let dialog_for_entry = dialog.clone();
    entry.connect_changed(move |entry| {
        dialog_for_entry.set_response_enabled("accept", !entry.text().trim().is_empty());
    });
    let entry_for_choice = entry.clone();
    dialog.connect_response(None, move |_, response| {
        let text = entry_for_choice.text().trim().to_string();
        if response == "accept" && !text.is_empty() {
            on_accept(text);
        }
    });
    dialog.present(Some(&ui.window));
    entry.grab_focus();
}

pub(super) fn prompt_multiline(
    ui: &LibraryWindowUi,
    heading: &str,
    initial: &str,
    on_accept: Rc<dyn Fn(String)>,
) {
    let view = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    view.buffer().set_text(initial);
    let frame = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(120)
        .child(&view)
        .css_classes(["card"])
        .build();
    let dialog = libadwaita::AlertDialog::builder()
        .heading(heading)
        .extra_child(&frame)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("accept", "Save");
    dialog.set_response_appearance("accept", libadwaita::ResponseAppearance::Suggested);
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_, response| {
        if response == "accept" {
            let buffer = view.buffer();
            let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            on_accept(text.trim().to_string());
        }
    });
    dialog.present(Some(&ui.window));
}

pub(super) fn confirm(
    ui: &LibraryWindowUi,
    heading: &str,
    body: &str,
    accept: &str,
    on_accept: impl Fn() + 'static,
) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("accept", accept);
    dialog.set_response_appearance("accept", libadwaita::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_, response| {
        if response == "accept" {
            on_accept();
        }
    });
    dialog.present(Some(&ui.window));
}

pub(super) fn popover_menu(button: &gtk::MenuButton, items: Vec<(String, Box<dyn Fn()>)>) {
    let popover = gtk::Popover::new();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    for (label, on_click) in items {
        let item = gtk::Button::builder()
            .child(&gtk::Label::builder().label(&label).xalign(0.0).build())
            .css_classes(["flat"])
            .build();
        let popover_for_item = popover.clone();
        item.connect_clicked(move |_| {
            popover_for_item.popdown();
            on_click();
        });
        content.append(&item);
    }
    popover.set_child(Some(&content));
    button.set_popover(Some(&popover));
}

pub(super) fn toast_with_action(
    title: &str,
    button: &str,
    on_click: impl Fn() + 'static,
) -> libadwaita::Toast {
    let toast = libadwaita::Toast::builder()
        .title(title)
        .button_label(button)
        .build();
    toast.connect_button_clicked(move |_| on_click());
    toast
}
