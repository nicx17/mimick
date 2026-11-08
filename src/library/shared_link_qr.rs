//! A shared link as a QR code, for opening it on a phone held up to the screen.

use gettextrs::gettext;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{SharedLink, share_url};

use super::LibraryWindowUi;
use super::context_menu::show_error_toast;

/// Blank modules around the code; scanners need this margin to find it.
const QUIET_ZONE: usize = 4;
const MIN_SIZE: i32 = 220;
const COPIED_FOR: std::time::Duration = std::time::Duration::from_millis(1500);

/// The modules of `url`'s code, `true` for dark, row by row, and the side length.
fn modules(url: &str) -> Option<(Vec<bool>, usize)> {
    let code = qrcode::QrCode::new(url.as_bytes()).ok()?;
    let dark = code
        .to_colors()
        .into_iter()
        .map(|color| color == qrcode::Color::Dark)
        .collect();
    Some((dark, code.width()))
}

pub(super) fn present(ui: &Rc<LibraryWindowUi>, link: &SharedLink) {
    let (ui, link) = (ui.clone(), link.clone());
    glib::MainContext::default().spawn_local(async move {
        let base = match ui.ctx.api_client.shared_link_base().await {
            Ok(base) => base,
            Err(err) => return show_error_toast(&ui, &gettext("Couldn't make the QR code"), &err),
        };
        let url = share_url(&base, &link);
        match modules(&url) {
            Some(code) => show(&ui, &url, code),
            None => show_error_toast(
                &ui,
                &gettext("Couldn't make the QR code"),
                &gettext("The link is too long for a QR code."),
            ),
        }
    });
}

fn show(ui: &Rc<LibraryWindowUi>, url: &str, code: (Vec<bool>, usize)) {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    content.append(&code_area(code));
    content.append(&url_label(url));
    content.append(&copy_button(url));
    let toolbar = libadwaita::ToolbarView::builder().content(&content).build();
    toolbar.add_top_bar(&libadwaita::HeaderBar::new());
    let dialog = libadwaita::Dialog::builder()
        .title(gettext("QR Code"))
        .content_width(360)
        .child(&toolbar)
        .build();
    dialog.present(Some(&ui.window));
}

/// Black on white whatever the theme, which every scanner reads; square at any width.
fn code_area((dark, width): (Vec<bool>, usize)) -> gtk::AspectFrame {
    let area = gtk::DrawingArea::builder()
        .content_width(MIN_SIZE)
        .content_height(MIN_SIZE)
        .hexpand(true)
        .vexpand(true)
        .build();
    area.update_property(&[gtk::accessible::Property::Label(&gettext(
        "QR code of the link",
    ))]);
    area.set_draw_func(move |_, cr, w, h| {
        let side = f64::from(w.min(h));
        let cell = side / (width + 2 * QUIET_ZONE) as f64;
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.paint().ok();
        cr.set_source_rgb(0.0, 0.0, 0.0);
        for (index, _) in dark.iter().enumerate().filter(|(_, dark)| **dark) {
            let (x, y) = (index % width + QUIET_ZONE, index / width + QUIET_ZONE);
            cr.rectangle(x as f64 * cell, y as f64 * cell, cell, cell);
        }
        cr.fill().ok();
    });
    gtk::AspectFrame::builder()
        .ratio(1.0)
        .obey_child(false)
        .child(&area)
        .css_classes(["mimick-qr-code"])
        .build()
}

fn url_label(url: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(url)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::Char)
        .justify(gtk::Justification::Center)
        .css_classes(["dim-label", "caption"])
        .build()
}

fn copy_button(url: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .label(gettext("Copy Link"))
        .halign(gtk::Align::Center)
        .css_classes(["pill"])
        .build();
    let url = url.to_string();
    // The label says it worked, since a toast would sit behind the dialog.
    button.connect_clicked(move |button| {
        if let Some(display) = gdk4::Display::default() {
            display.clipboard().set_text(&url);
        }
        button.set_label(&gettext("Copied"));
        let button = button.downgrade();
        glib::timeout_add_local_once(COPIED_FOR, move || {
            if let Some(button) = button.upgrade() {
                button.set_label(&gettext("Copy Link"));
            }
        });
    });
    button
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_become_square_codes() {
        let (dark, width) = modules("https://photos.example/share/abc123").unwrap();
        assert_eq!(dark.len(), width * width);
        assert!(dark.iter().any(|d| *d) && dark.iter().any(|d| !*d));
    }
}
