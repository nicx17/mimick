//! Status page: a headline with the main sync actions, anything that needs attention,
//! each watched folder's state, and the server connection.

use adw::prelude::*;
use gtk::{Button, ProgressBar};
use libadwaita as adw;

pub struct StatusWidgets {
    pub icon: gtk::Image,
    pub title: gtk::Label,
    pub subtitle: gtk::Label,
    pub progress_bar: ProgressBar,
    pub sync_now_btn: Button,
    pub pause_btn: Button,
    pub queue_btn: Button,
    pub attention_group: adw::PreferencesGroup,
    pub failed_row: adw::ActionRow,
    pub retry_btn: Button,
    pub error_row: adw::ActionRow,
    pub folders_list: gtk::ListBox,
    pub route_row: adw::ActionRow,
    pub last_sync_row: adw::ActionRow,
}

pub fn build_status_group(status_page: &adw::PreferencesPage) -> StatusWidgets {
    let hero = Hero::build();
    let hero_group = adw::PreferencesGroup::new();
    hero_group.add(&hero.root);
    status_page.add(&hero_group);

    let (attention_group, failed_row, retry_btn, error_row) = attention_group();
    status_page.add(&attention_group);

    let folders_group = adw::PreferencesGroup::builder()
        .title("Watched Folders")
        .build();
    let folders_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    folders_group.add(&folders_list);
    status_page.add(&folders_group);

    let connection_group = adw::PreferencesGroup::builder().title("Connection").build();
    let route_row = action_row("Server", "Checking…");
    let last_sync_row = action_row("Last Upload", "Nothing uploaded yet");
    connection_group.add(&route_row);
    connection_group.add(&last_sync_row);
    status_page.add(&connection_group);

    StatusWidgets {
        icon: hero.icon,
        title: hero.title,
        subtitle: hero.subtitle,
        progress_bar: hero.progress_bar,
        sync_now_btn: hero.sync_now_btn,
        pause_btn: hero.pause_btn,
        queue_btn: hero.queue_btn,
        attention_group,
        failed_row,
        retry_btn,
        error_row,
        folders_list,
        route_row,
        last_sync_row,
    }
}

struct Hero {
    root: gtk::Box,
    icon: gtk::Image,
    title: gtk::Label,
    subtitle: gtk::Label,
    progress_bar: ProgressBar,
    sync_now_btn: Button,
    pause_btn: Button,
    queue_btn: Button,
}

impl Hero {
    fn build() -> Self {
        let icon = gtk::Image::builder()
            .icon_name("mimick-check-circle-symbolic")
            .pixel_size(48)
            .css_classes(["dim-label"])
            .build();
        let title = gtk::Label::builder()
            .label("Up to Date")
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["title-2"])
            .build();
        let subtitle = gtk::Label::builder()
            .wrap(true)
            .justify(gtk::Justification::Center)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .css_classes(["dim-label"])
            .build();
        let progress_bar = ProgressBar::builder()
            .visible(false)
            .margin_start(24)
            .margin_end(24)
            .build();
        let sync_now_btn = pill("Sync Now", true);
        let pause_btn = pill("Pause", false);
        let queue_btn = pill("Queue…", false);
        let buttons = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(1)
            .max_children_per_line(3)
            .column_spacing(8)
            .row_spacing(8)
            .halign(gtk::Align::Center)
            .margin_top(6)
            .build();
        for button in [&sync_now_btn, &pause_btn, &queue_btn] {
            buttons.insert(button, -1);
        }
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_top(12)
            .margin_bottom(6)
            .build();
        for child in [
            icon.upcast_ref::<gtk::Widget>(),
            title.upcast_ref(),
            subtitle.upcast_ref(),
            progress_bar.upcast_ref(),
            buttons.upcast_ref(),
        ] {
            root.append(child);
        }
        Self {
            root,
            icon,
            title,
            subtitle,
            progress_bar,
            sync_now_btn,
            pause_btn,
            queue_btn,
        }
    }
}

fn pill(label: &str, suggested: bool) -> Button {
    let button = Button::builder().label(label).css_classes(["pill"]).build();
    if suggested {
        button.add_css_class("suggested-action");
    }
    button
}

/// Failed uploads (with Retry) and the latest error; hidden while everything is fine.
fn attention_group() -> (
    adw::PreferencesGroup,
    adw::ActionRow,
    Button,
    adw::ActionRow,
) {
    let group = adw::PreferencesGroup::builder()
        .title("Needs Attention")
        .visible(false)
        .build();
    let failed_row = adw::ActionRow::builder()
        .title("Failed uploads")
        .visible(false)
        .build();
    failed_row.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
    let retry_btn = Button::builder()
        .label("Retry")
        .valign(gtk::Align::Center)
        .build();
    failed_row.add_suffix(&retry_btn);
    let error_row = adw::ActionRow::builder()
        .subtitle_lines(3)
        .visible(false)
        .build();
    error_row.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
    group.add(&failed_row);
    group.add(&error_row);
    (group, failed_row, retry_btn, error_row)
}

fn action_row(title: &str, subtitle: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build()
}
