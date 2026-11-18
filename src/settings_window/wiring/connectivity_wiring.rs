use std::sync::Arc;

use adw::prelude::*;
use gettextrs::gettext;
use glib::clone;
use gtk::prelude::*;
use libadwaita as adw;

use crate::api_client::ImmichApiClient;
use crate::config::CustomHeader;
use crate::i18n::fill;
use crate::settings_window::connectivity::ConnectivityWidgets;
use crate::settings_window::show_alert;

/// One line for a connection test: which address worked, and how the other fared.
fn connection_summary(
    internal_ok: bool,
    external_ok: bool,
    internal_on: bool,
    external_on: bool,
) -> String {
    let other = |name: &str, on: bool, ok: bool| match (on, ok) {
        (false, _) => String::new(),
        (true, true) => format!(
            " · {}",
            fill(gettext("{name} also works"), &[("name", name)])
        ),
        (true, false) => format!(
            " · {}",
            fill(gettext("{name} unreachable"), &[("name", name)])
        ),
    };
    if internal_ok {
        gettext("Connected through LAN") + &other("WAN", external_on, external_ok)
    } else if external_ok {
        gettext("Connected through WAN") + &other("LAN", internal_on, internal_ok)
    } else {
        gettext("Couldn't reach Immich at either address")
    }
}

/// Ping the enabled addresses off the GTK thread and show the result in the Connection row.
pub(crate) fn connect_test_button(conn: &ConnectivityWidgets, api_client: Arc<ImmichApiClient>) {
    let status_row = conn.status_row.downgrade();
    conn.test_btn.connect_clicked(clone!(
        #[weak(rename_to = internal_switch)]
        conn.internal_switch,
        #[weak(rename_to = external_switch)]
        conn.external_switch,
        #[weak(rename_to = internal_entry)]
        conn.internal_entry,
        #[weak(rename_to = external_entry)]
        conn.external_entry,
        #[strong(rename_to = custom_headers)]
        conn.custom_headers,
        move |btn| {
            let input = ping_input(
                &internal_switch,
                &internal_entry,
                &external_switch,
                &external_entry,
                &custom_headers,
            );
            handle_test_click(btn, &status_row, api_client.clone(), input);
        }
    ));
}

fn handle_test_click(
    btn: &gtk::Button,
    status_row: &glib::WeakRef<adw::ActionRow>,
    api_client: Arc<ImmichApiClient>,
    input: PingInput,
) {
    btn.set_sensitive(false);
    let (internal_on, external_on) = (!input.internal.is_empty(), !input.external.is_empty());
    if let Some(row) = status_row.upgrade() {
        row.set_subtitle(&gettext("Testing…"));
    }
    let rx = spawn_ping(api_client, input);
    let (status_row, btn) = (status_row.clone(), btn.clone());
    glib::MainContext::default().spawn_local(async move {
        let Ok((int_ok, ext_ok)) = rx.await else {
            return;
        };
        btn.set_sensitive(true);
        if let Some(row) = status_row.upgrade() {
            row.set_subtitle(&connection_summary(
                int_ok,
                ext_ok,
                internal_on,
                external_on,
            ));
        }
    });
}

struct PingInput {
    internal: String,
    external: String,
    headers: Vec<CustomHeader>,
}

fn ping_input(
    internal_switch: &gtk::Switch,
    internal_entry: &gtk::Entry,
    external_switch: &gtk::Switch,
    external_entry: &gtk::Entry,
    custom_headers: &std::rc::Rc<std::cell::RefCell<Vec<CustomHeader>>>,
) -> PingInput {
    let enabled_url = |switch: &gtk::Switch, entry: &gtk::Entry| {
        if switch.is_active() {
            entry.text().to_string()
        } else {
            String::new()
        }
    };
    PingInput {
        internal: enabled_url(internal_switch, internal_entry),
        external: enabled_url(external_switch, external_entry),
        headers: custom_headers.borrow().clone(),
    }
}

/// Ping both addresses on Tokio; resolves to `(internal_ok, external_ok)`.
/// Reuses the app-wide API client: a new one per click opens a connection pool
/// that lingers ~30s after the test completes.
fn spawn_ping(
    api_client: Arc<ImmichApiClient>,
    input: PingInput,
) -> tokio::sync::oneshot::Receiver<(bool, bool)> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let headers = &input.headers;
        let int_ok = !input.internal.is_empty()
            && api_client
                .ping_url_custom(&input.internal, false, headers)
                .await;
        let ext_ok = !input.external.is_empty()
            && api_client
                .ping_url_custom(&input.external, true, headers)
                .await;
        let _ = tx.send((int_ok, ext_ok));
    });
    rx
}

/// At least one server URL must stay enabled; each entry is editable only while enabled.
pub(crate) fn connect_url_switches(window: &adw::ApplicationWindow, conn: &ConnectivityWidgets) {
    let pairs = [
        (
            &conn.internal_switch,
            &conn.external_switch,
            &conn.internal_entry,
        ),
        (
            &conn.external_switch,
            &conn.internal_switch,
            &conn.external_entry,
        ),
    ];
    for (switch, other, entry) in pairs {
        switch.connect_active_notify(clone!(
            #[weak]
            other,
            #[weak]
            entry,
            #[weak]
            window,
            move |switch| {
                if !switch.is_active() && !other.is_active() {
                    switch.set_active(true);
                    show_alert(
                        &window,
                        &gettext("Invalid Selection"),
                        &gettext("At least one URL (Internal or External) must be enabled."),
                    );
                }
                entry.set_sensitive(switch.is_active());
            }
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_summary_names_what_worked() {
        assert_eq!(
            connection_summary(true, false, true, true),
            "Connected through LAN · WAN unreachable"
        );
        assert_eq!(
            connection_summary(true, true, true, true),
            "Connected through LAN · WAN also works"
        );
        assert_eq!(
            connection_summary(false, true, true, true),
            "Connected through WAN · LAN unreachable"
        );
        assert_eq!(
            connection_summary(true, false, true, false),
            "Connected through LAN"
        );
        assert_eq!(
            connection_summary(false, false, true, true),
            "Couldn't reach Immich at either address"
        );
    }
}
