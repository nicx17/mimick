use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::LibraryAlbum;
use crate::library::LibraryWindowUi;

use super::{ActivityFeed, stats_summary};

pub(in crate::library) fn header_button(
    ui: &Rc<LibraryWindowUi>,
    album: &LibraryAlbum,
) -> gtk::Button {
    let count = gtk::Label::builder().visible(false).build();
    let content = gtk::Box::builder().spacing(6).build();
    content.append(&gtk::Image::from_icon_name("mimick-comment-symbolic"));
    content.append(&count);
    let button = gtk::Button::builder()
        .child(&content)
        .tooltip_text("Likes and comments")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let refresh = counts_refresher(ui, &album.id, &button, &count);
    refresh();
    let (ui, album) = (ui.clone(), album.clone());
    button.connect_clicked(move |_| {
        let refresh = refresh.clone();
        present(&ui, &album, Box::new(move || refresh()));
    });
    button
}

fn counts_refresher(
    ui: &Rc<LibraryWindowUi>,
    album_id: &str,
    button: &gtk::Button,
    count: &gtk::Label,
) -> Rc<dyn Fn()> {
    let (ui, album_id) = (ui.clone(), album_id.to_string());
    let (button, count) = (button.downgrade(), count.downgrade());
    Rc::new(move || {
        let (ui, album_id) = (ui.clone(), album_id.clone());
        let (button, count) = (button.clone(), count.clone());
        glib::MainContext::default().spawn_local(async move {
            let stats = ui.ctx.api_client.activity_statistics(&album_id, None).await;
            let (Ok(stats), Some(button), Some(count)) = (stats, button.upgrade(), count.upgrade())
            else {
                return;
            };
            let total = stats.likes + stats.comments;
            count.set_label(&total.to_string());
            count.set_visible(total > 0);
            button.set_tooltip_text(Some(&stats_summary(stats)));
        });
    })
}

fn present(ui: &Rc<LibraryWindowUi>, album: &LibraryAlbum, on_change: Box<dyn Fn()>) {
    let feed = ActivityFeed::new(ui, album, None, on_change);
    set_margins(&feed.list, 6, 12);
    set_margins(&feed.composer, 6, 6);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&feed.list)
        .build();
    scroller
        .vadjustment()
        .connect_upper_notify(|adj| adj.set_value(adj.upper()));
    let toolbar = libadwaita::ToolbarView::new();
    let header = libadwaita::HeaderBar::builder()
        .title_widget(&libadwaita::WindowTitle::new("Activity", &album.album_name))
        .build();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroller));
    toolbar.add_bottom_bar(&feed.composer);
    let dialog = libadwaita::Dialog::builder()
        .content_width(420)
        .content_height(560)
        .child(&toolbar)
        .build();
    feed.load();
    let keep = RefCell::new(Some(feed));
    dialog.connect_closed(move |_| {
        keep.take();
    });
    dialog.present(Some(&ui.window));
}

fn set_margins(widget: &impl IsA<gtk::Widget>, top: i32, bottom: i32) {
    widget.set_margin_top(top);
    widget.set_margin_bottom(bottom);
    widget.set_margin_start(12);
    widget.set_margin_end(12);
}
