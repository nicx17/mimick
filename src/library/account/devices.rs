//! The Devices page: browsers and apps signed in to your account, each one signed out alone.

use crate::i18n::fill;
use gettextrs::gettext;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::Session;
use crate::library::LibraryWindowUi;
use crate::library::shared_link_dialog::format_expiry;

pub(super) fn page(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
) -> libadwaita::PreferencesPage {
    // There's no "sign out everywhere": with an API key, Immich would sign out every device.
    let group = libadwaita::PreferencesGroup::builder()
        .title(gettext("Signed-in Devices"))
        .description(gettext(
            "Browsers and apps signed in to your account. Mimick uses an API key, so it isn't listed.",
        ))
        .build();
    let list = super::boxed_list();
    group.add(&list);
    let page = libadwaita::PreferencesPage::builder()
        .title(gettext("Devices"))
        .icon_name("computer-symbolic")
        .build();
    page.add(&group);
    load(ui, dialog, &list);
    page
}

fn load(ui: &Rc<LibraryWindowUi>, dialog: &libadwaita::PreferencesDialog, list: &gtk::ListBox) {
    let (ui, dialog, list) = (ui.clone(), dialog.clone(), list.clone());
    glib::MainContext::default().spawn_local(async move {
        let rows = match ui.ctx.api_client.fetch_sessions().await {
            Ok(sessions) if sessions.is_empty() => {
                vec![super::note_row(&gettext(
                    "No browsers or apps are signed in.",
                ))]
            }
            Ok(sessions) => sessions
                .iter()
                .map(|session| session_row(&ui, &dialog, &list, session))
                .collect(),
            Err(_) => vec![super::note_row(&super::needs_permission("session.read"))],
        };
        list.remove_all();
        for row in rows {
            list.append(&row);
        }
    });
}

/// "Firefox · Linux", when it was last used, and a sign-out button.
fn session_row(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
    list: &gtk::ListBox,
    session: &Session,
) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .title(device_title(session))
        .subtitle(device_subtitle(session))
        .use_markup(false)
        .build();
    let sign_out = gtk::Button::builder()
        .icon_name("system-log-out-symbolic")
        .tooltip_text(gettext("Sign out"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    row.add_suffix(&sign_out);
    let (ui, dialog_ref, list, id) = (ui.clone(), dialog.clone(), list.clone(), session.id.clone());
    let title = device_title(session);
    sign_out.connect_clicked(move |_| {
        let (ui, dialog, list, id) = (ui.clone(), dialog_ref.clone(), list.clone(), id.clone());
        super::confirm(
            &dialog_ref,
            (
                &fill(gettext("Sign out {device}?"), &[("device", &title)]),
                &gettext("It will have to sign in to Immich again."),
                &gettext("Sign Out"),
            ),
            move || sign_out_device(&ui, &dialog, &list, id.clone()),
        );
    });
    row
}

fn sign_out_device(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
    list: &gtk::ListBox,
    id: String,
) {
    let (ui, dialog, list) = (ui.clone(), dialog.clone(), list.clone());
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.sign_out_session(&id).await {
            Ok(()) => load(&ui, &dialog, &list),
            Err(err) => dialog.add_toast(libadwaita::Toast::new(&fill(
                gettext("Couldn't sign it out: {err}"),
                &[("err", &err)],
            ))),
        }
    });
}

fn device_title(session: &Session) -> String {
    let parts: Vec<&str> = [session.device_type.as_str(), session.device_os.as_str()]
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect();
    if parts.is_empty() {
        gettext("Unknown device")
    } else {
        parts.join(" · ")
    }
}

fn device_subtitle(session: &Session) -> String {
    let used = fill(
        gettext("Last used {date}"),
        &[("date", &format_expiry(&session.updated_at))],
    );
    match session.app_version.as_deref().filter(|v| !v.is_empty()) {
        Some(version) => format!("{used} · {version}"),
        None => used,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(device_type: &str, device_os: &str) -> Session {
        Session {
            id: "s".into(),
            device_os: device_os.into(),
            device_type: device_type.into(),
            app_version: None,
            updated_at: String::new(),
        }
    }

    #[test]
    fn devices_are_named_by_what_immich_knows() {
        assert_eq!(
            device_title(&session("Firefox", "Linux")),
            "Firefox · Linux"
        );
        assert_eq!(device_title(&session("", "iOS")), "iOS");
        assert_eq!(device_title(&session("", "")), "Unknown device");
    }
}
