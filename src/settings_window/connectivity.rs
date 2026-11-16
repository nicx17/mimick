use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{Button, Entry, PasswordEntry, Switch};
use libadwaita as adw;

use crate::config::{CustomHeader, HeaderScope};

pub struct ConnectivityWidgets {
    pub internal_switch: Switch,
    pub external_switch: Switch,
    pub internal_entry: Entry,
    pub external_entry: Entry,
    pub api_key_entry: PasswordEntry,
    pub custom_headers: Rc<RefCell<Vec<CustomHeader>>>,
    pub test_btn: Button,
    pub save_btn: Button,
    pub open_library_btn: Button,
    /// Shows the result of the last connection test.
    pub status_row: adw::ActionRow,
}

pub fn build_connectivity_group(
    settings_page: &adw::PreferencesPage,
    window: &adw::ApplicationWindow,
    initial_headers: Vec<CustomHeader>,
) -> ConnectivityWidgets {
    let conn_group = adw::PreferencesGroup::builder()
        .title(gettext("Server"))
        .build();
    settings_page.add(&conn_group);

    let (internal_row, internal_switch, internal_entry) =
        add_url_row(&conn_group, &gettext("Internal URL (LAN)"), "http://...");
    let (external_row, external_switch, external_entry) =
        add_url_row(&conn_group, &gettext("External URL (WAN)"), "https://...");
    let api_key_entry = add_api_key_row(&conn_group);
    let (_custom_headers_row, custom_headers) =
        add_custom_headers_row(&conn_group, window, initial_headers);

    let status_row = adw::ActionRow::builder()
        .title(gettext("Connection"))
        .subtitle(gettext("Not tested yet"))
        .build();
    conn_group.add(&status_row);
    let test_btn = add_group_button(&conn_group, &gettext("Test Connection"), None, 12);
    let save_btn = add_group_button(
        &conn_group,
        &gettext("Save and Connect"),
        Some("suggested-action"),
        6,
    );
    // Shown once a key is saved and the library view is on; see `apply::sync_open_library`.
    let open_library_btn = add_group_button(&conn_group, &gettext("Open Library"), None, 6);
    open_library_btn.set_visible(false);

    add_connectivity_breakpoint(
        window,
        &internal_row,
        &external_row,
        &internal_entry,
        &external_entry,
        &api_key_entry,
    );

    ConnectivityWidgets {
        internal_switch,
        external_switch,
        internal_entry,
        external_entry,
        api_key_entry,
        custom_headers,
        test_btn,
        save_btn,
        open_library_btn,
        status_row,
    }
}

fn add_url_row(
    group: &adw::PreferencesGroup,
    title: &str,
    placeholder: &str,
) -> (adw::ActionRow, Switch, Entry) {
    let row = adw::ActionRow::builder()
        .title(title)
        .title_lines(1)
        .build();
    let switch = Switch::builder().valign(gtk::Align::Center).build();
    let entry = text_entry(placeholder);
    row.add_prefix(&switch);
    row.add_suffix(&entry);
    group.add(&row);
    (row, switch, entry)
}

fn add_api_key_row(group: &adw::PreferencesGroup) -> PasswordEntry {
    let row = adw::ActionRow::builder().title(gettext("API Key")).build();
    let entry = PasswordEntry::builder()
        .valign(gtk::Align::Center)
        .width_request(140)
        .max_width_chars(16)
        .hexpand(true)
        .build();
    row.add_suffix(&entry);
    group.add(&row);
    entry
}

fn add_custom_headers_row(
    group: &adw::PreferencesGroup,
    window: &adw::ApplicationWindow,
    initial_headers: Vec<CustomHeader>,
) -> (adw::ExpanderRow, Rc<RefCell<Vec<CustomHeader>>>) {
    let has_headers = !initial_headers.is_empty();
    let expander = adw::ExpanderRow::builder()
        .title(gettext("Custom Proxy Headers"))
        .expanded(has_headers)
        .build();
    group.add(&expander);

    let headers = Rc::new(RefCell::new(initial_headers));
    let rows_tracker = Rc::new(RefCell::new(Vec::new()));

    rebuild_headers_list(window, &expander, &headers, &rows_tracker);

    (expander, headers)
}

fn update_expander_subtitle(expander: &adw::ExpanderRow, count: usize) {
    if count == 0 {
        expander.set_subtitle(&gettext(
            "Optional HTTP headers for reverse proxies (e.g. Cloudflare Access)",
        ));
    } else if count == 1 {
        expander.set_subtitle(&gettext("1 header configured"));
    } else {
        expander.set_subtitle(&format!("{count} {}", gettext("headers configured")));
    }
}

fn rebuild_headers_list(
    window: &adw::ApplicationWindow,
    expander: &adw::ExpanderRow,
    headers: &Rc<RefCell<Vec<CustomHeader>>>,
    rows_tracker: &Rc<RefCell<Vec<adw::ActionRow>>>,
) {
    for old_row in rows_tracker.borrow_mut().drain(..) {
        expander.remove(&old_row);
    }

    let count = headers.borrow().len();
    update_expander_subtitle(expander, count);

    for (index, header) in headers.borrow().iter().enumerate() {
        let row = build_header_item_row(window, expander, headers, rows_tracker, header, index);
        expander.add_row(&row);
        rows_tracker.borrow_mut().push(row);
    }

    let add_row = build_add_header_row(window, expander, headers, rows_tracker);
    expander.add_row(&add_row);
    rows_tracker.borrow_mut().push(add_row);
}

fn build_header_item_row(
    window: &adw::ApplicationWindow,
    expander: &adw::ExpanderRow,
    headers: &Rc<RefCell<Vec<CustomHeader>>>,
    rows_tracker: &Rc<RefCell<Vec<adw::ActionRow>>>,
    header: &CustomHeader,
    index: usize,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&header.name)
        .subtitle("••••••••")
        .build();

    let scope_badge = gtk::Label::builder()
        .label(header.scope.short_label())
        .css_classes(["dim-label"])
        .valign(gtk::Align::Center)
        .build();
    row.add_suffix(&scope_badge);

    let delete_btn = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Remove header"))
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();

    let headers = headers.clone();
    let expander = expander.clone();
    let window = window.clone();
    let rows_tracker = rows_tracker.clone();
    delete_btn.connect_clicked(move |_| {
        if index < headers.borrow().len() {
            headers.borrow_mut().remove(index);
            rebuild_headers_list(&window, &expander, &headers, &rows_tracker);
        }
    });
    row.add_suffix(&delete_btn);

    row
}

fn build_add_header_row(
    window: &adw::ApplicationWindow,
    expander: &adw::ExpanderRow,
    headers: &Rc<RefCell<Vec<CustomHeader>>>,
    rows_tracker: &Rc<RefCell<Vec<adw::ActionRow>>>,
) -> adw::ActionRow {
    let add_row = adw::ActionRow::builder()
        .title(gettext("Add Header…"))
        .activatable(true)
        .build();
    let add_icon = gtk::Image::builder()
        .icon_name("list-add-symbolic")
        .valign(gtk::Align::Center)
        .build();
    add_row.add_prefix(&add_icon);

    let headers = headers.clone();
    let expander = expander.clone();
    let window = window.clone();
    let rows_tracker = rows_tracker.clone();
    add_row.connect_activated(move |_| {
        show_add_header_dialog(&window, &expander, &headers, &rows_tracker);
    });

    add_row
}

fn make_paste_button(on_paste: impl Fn(String) + 'static) -> gtk::Button {
    let btn = gtk::Button::builder()
        .icon_name("edit-paste-symbolic")
        .tooltip_text(gettext("Paste from clipboard"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();

    let cb = Rc::new(on_paste);
    btn.connect_clicked(move |b| {
        let clipboard = b.clipboard();
        let cb = cb.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Ok(Some(text)) = clipboard.read_text_future().await {
                cb(text.trim().to_string());
            }
        });
    });
    btn
}

fn validate_add_button(
    name_row: &adw::EntryRow,
    val_row: &adw::PasswordEntryRow,
    add_btn: &gtk::Button,
) {
    let name = name_row.text().trim().to_string();
    let val = val_row.text().trim().to_string();
    let header = CustomHeader::new(&name, &val, HeaderScope::Wan);
    let valid = !name.is_empty() && !val.is_empty() && header.is_valid_name();
    add_btn.set_sensitive(valid);
}

fn show_add_header_dialog(
    window: &adw::ApplicationWindow,
    expander: &adw::ExpanderRow,
    headers: &Rc<RefCell<Vec<CustomHeader>>>,
    rows_tracker: &Rc<RefCell<Vec<adw::ActionRow>>>,
) {
    let name_row = adw::EntryRow::builder()
        .title(gettext("Header Name"))
        .build();
    let n_clone = name_row.clone();
    name_row.add_suffix(&make_paste_button(move |text| {
        n_clone.set_text(&text);
    }));

    let val_row = adw::PasswordEntryRow::builder()
        .title(gettext("Secret / Value"))
        .build();
    let v_clone = val_row.clone();
    val_row.add_suffix(&make_paste_button(move |text| {
        v_clone.set_text(&text);
    }));

    let scopes = [
        HeaderScope::Wan.label(),
        HeaderScope::All.label(),
        HeaderScope::Lan.label(),
    ];
    let string_list = gtk::StringList::new(&[&scopes[0], &scopes[1], &scopes[2]]);
    let scope_row = adw::ComboRow::builder()
        .title(gettext("Scope"))
        .model(&string_list)
        .selected(0)
        .build();

    let cancel_btn = gtk::Button::builder().label(gettext("Cancel")).build();
    let add_btn = gtk::Button::builder()
        .label(gettext("Add"))
        .css_classes(["suggested-action"])
        .sensitive(false)
        .build();

    let (n_val, v_val, a_val) = (name_row.clone(), val_row.clone(), add_btn.clone());
    let update_validity = Rc::new(move || {
        validate_add_button(&n_val, &v_val, &a_val);
    });
    let uv1 = update_validity.clone();
    name_row.connect_changed(move |_| uv1());
    let uv2 = update_validity;
    val_row.connect_changed(move |_| uv2());

    let header_bar = adw::HeaderBar::builder().build();
    header_bar.pack_start(&cancel_btn);
    header_bar.pack_end(&add_btn);

    let group = adw::PreferencesGroup::new();
    group.add(&name_row);
    group.add(&val_row);
    group.add(&scope_row);

    let page = adw::PreferencesPage::new();
    page.add(&group);

    let toolbar = adw::ToolbarView::builder().content(&page).build();
    toolbar.add_top_bar(&header_bar);

    let dialog = adw::Dialog::builder()
        .title(gettext("Add Custom Header"))
        .content_width(420)
        .child(&toolbar)
        .build();

    let d_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        d_cancel.close();
    });

    let d_add = dialog.clone();
    let headers = headers.clone();
    let expander = expander.clone();
    let window_clone = window.clone();
    let rows_tracker = rows_tracker.clone();
    add_btn.connect_clicked(move |_| {
        let name = name_row.text().trim().to_string();
        let val = val_row.text().trim().to_string();
        let scope = match scope_row.selected() {
            1 => HeaderScope::All,
            2 => HeaderScope::Lan,
            _ => HeaderScope::Wan,
        };
        headers
            .borrow_mut()
            .push(CustomHeader::new(name, val, scope));
        expander.set_expanded(true);
        rebuild_headers_list(&window_clone, &expander, &headers, &rows_tracker);
        d_add.close();
    });

    dialog.present(Some(window));
}

fn text_entry(placeholder: &str) -> Entry {
    Entry::builder()
        .placeholder_text(placeholder)
        .valign(gtk::Align::Center)
        .width_request(140)
        .max_width_chars(16)
        .hexpand(true)
        .build()
}

fn add_group_button(
    group: &adw::PreferencesGroup,
    label: &str,
    css_class: Option<&str>,
    margin_top: i32,
) -> Button {
    let mut builder = Button::builder().label(label).margin_top(margin_top);
    if let Some(css_class) = css_class {
        builder = builder.css_classes(vec![css_class.to_string()]);
    }
    let button = builder.build();
    group.add(&button);
    button
}

fn add_connectivity_breakpoint(
    window: &adw::ApplicationWindow,
    internal_row: &adw::ActionRow,
    external_row: &adw::ActionRow,
    internal_entry: &Entry,
    external_entry: &Entry,
    api_key_entry: &PasswordEntry,
) {
    let breakpoint = adw::Breakpoint::new(
        adw::BreakpointCondition::parse("max-width: 500sp").expect("valid breakpoint condition"),
    );
    breakpoint.add_setter(internal_row, "title", Some(&gettext("LAN URL").to_value()));
    breakpoint.add_setter(external_row, "title", Some(&gettext("WAN URL").to_value()));
    breakpoint.add_setter(internal_entry, "width-request", Some(&140i32.to_value()));
    breakpoint.add_setter(external_entry, "width-request", Some(&140i32.to_value()));
    breakpoint.add_setter(api_key_entry, "width-request", Some(&140i32.to_value()));
    window.add_breakpoint(breakpoint);
}
