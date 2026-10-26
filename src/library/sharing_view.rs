use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{Partner, PartnerDirection, UserSummary};

use super::LibraryWindowUi;
use super::album_share_dialog::{addable_users, display_name};
use super::context_menu::show_alert_dialog;
use super::dialogs::confirm;
use super::state::LibrarySource;

pub(super) struct SharingView {
    pub root: gtk::ScrolledWindow,
    shared_by: libadwaita::PreferencesGroup,
    shared_with: libadwaita::PreferencesGroup,
    add: gtk::Button,
    rows: RefCell<Vec<(libadwaita::PreferencesGroup, gtk::Widget)>>,
    partners: RefCell<Vec<Partner>>,
}

pub(super) fn build_sharing_view() -> SharingView {
    let add = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add partner")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let shared_by = libadwaita::PreferencesGroup::builder()
        .title("You Share With")
        .description("Partners can see all your photos and videos.")
        .header_suffix(&add)
        .build();
    let shared_with = libadwaita::PreferencesGroup::builder()
        .title("Sharing With You")
        .description(
            "People who added you as a partner. Switch one on to see their photos in Photos.",
        )
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&shared_by);
    content.append(&shared_with);
    let clamp = libadwaita::Clamp::builder()
        .maximum_size(820)
        .child(&content)
        .build();
    let root = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    SharingView {
        root,
        shared_by,
        shared_with,
        add,
        rows: RefCell::default(),
        partners: RefCell::default(),
    }
}

pub(super) fn connect_sharing_view(ui: Rc<LibraryWindowUi>) {
    let ui_add = ui.clone();
    ui.sharing
        .add
        .connect_clicked(move |_| present_add_dialog(&ui_add));
}

pub(super) fn show(ui: &Rc<LibraryWindowUi>) {
    ui.content_stack.set_visible_child_name("sharing");
    reload(ui.clone());
}

fn reload(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(async move {
        let api = &ui.ctx.api_client;
        let shared_by = api.fetch_partners(PartnerDirection::SharedBy).await;
        let shared_with = api.fetch_partners(PartnerDirection::SharedWith).await;
        match (shared_by, shared_with) {
            (Ok(shared_by), Ok(shared_with)) => render(&ui, shared_by, &shared_with),
            (Err(err), _) | (_, Err(err)) => show_alert_dialog(
                &ui,
                "Could Not Load Partners",
                &format!("{err}\n\nYour API key needs the 'partner.read' permission."),
            ),
        }
    });
}

fn render(ui: &Rc<LibraryWindowUi>, shared_by: Vec<Partner>, shared_with: &[Partner]) {
    let view = &ui.sharing;
    for (group, row) in view.rows.borrow_mut().drain(..) {
        group.remove(&row);
    }
    let mut rows = view.rows.borrow_mut();
    for partner in &shared_by {
        let row = partner_row(&partner.user);
        row.add_suffix(&remove_button(ui, &partner.user));
        view.shared_by.add(&row);
        rows.push((view.shared_by.clone(), row.upcast()));
    }
    if shared_by.is_empty() {
        let row = placeholder_row("Use + to share your library with someone.");
        view.shared_by.add(&row);
        rows.push((view.shared_by.clone(), row.upcast()));
    }
    for partner in shared_with {
        let row = shared_with_row(ui, partner);
        view.shared_with.add(&row);
        rows.push((view.shared_with.clone(), row.upcast()));
    }
    if shared_with.is_empty() {
        let row = placeholder_row("No one is sharing with you.");
        view.shared_with.add(&row);
        rows.push((view.shared_with.clone(), row.upcast()));
    }
    *view.partners.borrow_mut() = shared_by;
}

fn partner_row(user: &UserSummary) -> libadwaita::ActionRow {
    let name = display_name(&user.name, &user.email);
    let row = libadwaita::ActionRow::builder()
        .title(&name)
        .subtitle(&user.email)
        .use_markup(false)
        .title_lines(1)
        .subtitle_lines(1)
        .build();
    row.add_prefix(&libadwaita::Avatar::new(32, Some(&name), true));
    row
}

fn shared_with_row(ui: &Rc<LibraryWindowUi>, partner: &Partner) -> libadwaita::ActionRow {
    let row = partner_row(&partner.user);
    row.add_suffix(&timeline_switch(ui, partner));
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row.set_activatable(true);
    let (ui, id) = (ui.clone(), partner.user.id.clone());
    let name = display_name(&partner.user.name, &partner.user.email);
    row.connect_activated(move |_| {
        super::navigation::navigate_source(
            &ui,
            LibrarySource::Partner {
                id: id.clone(),
                name: name.clone(),
            },
        );
    });
    row
}

fn timeline_switch(ui: &Rc<LibraryWindowUi>, partner: &Partner) -> gtk::Switch {
    let switch = gtk::Switch::builder()
        .active(partner.in_timeline)
        .tooltip_text("Show in Photos")
        .valign(gtk::Align::Center)
        .build();
    let (ui, id) = (ui.clone(), partner.user.id.clone());
    switch.connect_active_notify(move |switch| {
        let (ui, id, show) = (ui.clone(), id.clone(), switch.is_active());
        glib::MainContext::default().spawn_local(async move {
            if let Err(err) = ui.ctx.api_client.set_partner_in_timeline(&id, show).await {
                show_alert_dialog(
                    &ui,
                    "Could Not Change Partner",
                    &format!("{err}\n\nYour API key needs the 'partner.update' permission."),
                );
                reload(ui);
            }
        });
    });
    switch
}

fn placeholder_row(text: &str) -> libadwaita::ActionRow {
    libadwaita::ActionRow::builder()
        .title(text)
        .use_markup(false)
        .css_classes(["dim-label"])
        .build()
}

fn remove_button(ui: &Rc<LibraryWindowUi>, partner: &UserSummary) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("Stop sharing")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let (ui, partner) = (ui.clone(), partner.clone());
    button.connect_clicked(move |_| {
        let name = display_name(&partner.name, &partner.email);
        let (ui_remove, id) = (ui.clone(), partner.id.clone());
        confirm(
            &ui,
            &format!("Stop sharing with {name}?"),
            "They'll no longer see your photos and videos.",
            "Stop Sharing",
            move || change(&ui_remove, "Could Not Remove Partner", false, id.clone()),
        );
    });
    button
}

fn change(ui: &Rc<LibraryWindowUi>, heading: &'static str, add: bool, user_id: String) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let api = &ui.ctx.api_client;
        let result = if add {
            api.add_partner(&user_id).await
        } else {
            api.remove_partner(&user_id).await
        };
        match result {
            Ok(()) => reload(ui),
            Err(err) => show_alert_dialog(&ui, heading, &err),
        }
    });
}

#[derive(Clone)]
struct AddPartnerDialog {
    ui: Rc<LibraryWindowUi>,
    dialog: libadwaita::Dialog,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    status: gtk::Label,
    users: Rc<RefCell<Vec<UserSummary>>>,
    excluded: Rc<RefCell<Vec<String>>>,
}

fn present_add_dialog(ui: &Rc<LibraryWindowUi>) {
    let add = AddPartnerDialog::build(ui);
    let add_for_search = add.clone();
    add.search
        .connect_search_changed(move |_| add_for_search.fill());
    add.load();
    add.dialog.present(Some(&ui.window));
}

impl AddPartnerDialog {
    fn build(ui: &Rc<LibraryWindowUi>) -> Self {
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Find people by name or email")
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .valign(gtk::Align::Start)
            .build();
        let status = gtk::Label::builder()
            .label("Loading people…")
            .wrap(true)
            .css_classes(["dim-label"])
            .build();
        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(6)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        body.append(&search);
        body.append(&status);
        body.append(&list);
        let excluded = ui
            .sharing
            .partners
            .borrow()
            .iter()
            .map(|p| p.user.id.clone())
            .collect();
        Self {
            ui: ui.clone(),
            dialog: dialog_with_body(&body),
            search,
            list,
            status,
            users: Rc::default(),
            excluded: Rc::new(RefCell::new(excluded)),
        }
    }

    fn fill(&self) {
        self.list.remove_all();
        let users = self.users.borrow();
        let candidates = addable_users(&users, &self.excluded.borrow(), &self.search.text());
        for user in &candidates {
            self.list.append(&add_row(&self.ui, &self.dialog, user));
        }
        self.status.set_visible(candidates.is_empty());
        self.status.set_label(if users.is_empty() {
            "No other people found. Listing people needs the 'user.read' permission."
        } else {
            "No one matches your search."
        });
        self.list.set_visible(!candidates.is_empty());
    }

    fn load(&self) {
        let add = self.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Some(me) = super::ensure_current_user_id(&add.ui.ctx).await {
                add.excluded.borrow_mut().push(me);
            }
            match add.ui.ctx.api_client.list_users().await {
                Ok(list) => *add.users.borrow_mut() = list,
                Err(err) => log::warn!("Could not list users for partners: {}", err),
            }
            add.fill();
        });
    }
}

fn dialog_with_body(body: &gtk::Box) -> libadwaita::Dialog {
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .vexpand(true)
        .child(body)
        .build();
    let toolbar = libadwaita::ToolbarView::new();
    toolbar.add_top_bar(&libadwaita::HeaderBar::new());
    toolbar.set_content(Some(&scroller));
    libadwaita::Dialog::builder()
        .title("Add Partner")
        .content_width(420)
        .content_height(520)
        .child(&toolbar)
        .build()
}

fn add_row(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::Dialog,
    user: &UserSummary,
) -> libadwaita::ActionRow {
    let row = partner_row(user);
    let add = gtk::Button::builder()
        .label("Add")
        .valign(gtk::Align::Center)
        .build();
    row.add_suffix(&add);
    let (ui, dialog, user) = (ui.clone(), dialog.clone(), user.clone());
    add.connect_clicked(move |_| {
        let name = display_name(&user.name, &user.email);
        let (ui_add, dialog, id) = (ui.clone(), dialog.clone(), user.id.clone());
        let share = libadwaita::AlertDialog::builder()
            .heading(format!("Share your library with {name}?"))
            .body("They'll be able to see all your photos and videos. You can stop sharing at any time.")
            .build();
        share.add_response("cancel", "Cancel");
        share.add_response("share", "Share");
        share.set_response_appearance("share", libadwaita::ResponseAppearance::Suggested);
        share.set_default_response(Some("share"));
        share.set_close_response("cancel");
        share.connect_response(None, move |_, response| {
            if response == "share" {
                dialog.close();
                change(&ui_add, "Could Not Add Partner", true, id.clone());
            }
        });
        share.present(Some(&ui.window));
    });
    row
}
