//! The API Keys page: the key Mimick signs in with, which can get a new secret, and your other
//! keys, which can be deleted.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::ApiKey;
use crate::library::LibraryWindowUi;
use crate::library::shared_link_dialog::format_expiry;

/// Immich added `POST /api-keys/{id}/rotate` in v3.2.0.
const ROTATE_SINCE: (u32, u32) = (3, 2);

/// The page's two lists, reloaded after a change.
#[derive(Clone)]
struct Lists {
    mine: gtk::ListBox,
    others: gtk::ListBox,
}

pub(super) fn page(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
) -> libadwaita::PreferencesPage {
    let lists = Lists {
        mine: super::boxed_list(),
        others: super::boxed_list(),
    };
    let mine = libadwaita::PreferencesGroup::builder()
        .title(gettext("This App"))
        .description(gettext("The key Mimick signs in to Immich with."))
        .build();
    mine.add(&lists.mine);
    let others = libadwaita::PreferencesGroup::builder()
        .title(gettext("Other Keys"))
        .description(gettext("Make new keys in Immich's web app."))
        .build();
    others.add(&lists.others);
    let page = libadwaita::PreferencesPage::builder()
        .title(gettext("API Keys"))
        .icon_name("dialog-password-symbolic")
        .build();
    page.add(&mine);
    page.add(&others);
    load(ui, dialog, &lists);
    page
}

fn load(ui: &Rc<LibraryWindowUi>, dialog: &libadwaita::PreferencesDialog, lists: &Lists) {
    let (ui, dialog, lists) = (ui.clone(), dialog.clone(), lists.clone());
    glib::MainContext::default().spawn_local(async move {
        let api = &ui.ctx.api_client;
        let current = api.fetch_current_api_key().await.ok();
        let rotate = match api.fetch_server_about().await {
            Ok(about) => at_least(&about.version, ROTATE_SINCE),
            Err(_) => false,
        };
        lists.mine.remove_all();
        if let Some(key) = &current {
            lists.mine.append(&own_key_row(&ui, &dialog, key, rotate));
        }
        let others = api.fetch_api_keys().await;
        lists.others.remove_all();
        let current_id = current.as_ref().map(|key| key.id.as_str());
        match others {
            Ok(keys) => fill_others(&ui, &dialog, &lists, keys, current_id),
            Err(_) => lists
                .others
                .append(&super::note_row(&super::needs_permission("apiKey.read"))),
        }
    });
}

fn fill_others(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
    lists: &Lists,
    keys: Vec<ApiKey>,
    current: Option<&str>,
) {
    let others: Vec<ApiKey> = keys
        .into_iter()
        .filter(|key| Some(key.id.as_str()) != current)
        .collect();
    if others.is_empty() {
        lists
            .others
            .append(&super::note_row(&gettext("No other keys.")));
    }
    for key in &others {
        lists.others.append(&other_key_row(ui, dialog, lists, key));
    }
}

fn key_row(key: &ApiKey) -> libadwaita::ActionRow {
    let name = if key.name.trim().is_empty() {
        gettext("Unnamed key")
    } else {
        key.name.clone()
    };
    libadwaita::ActionRow::builder()
        .title(name)
        .subtitle(key_details(key))
        .use_markup(false)
        .build()
}

/// "Made 9 Oct 2026 · All permissions"
fn key_details(key: &ApiKey) -> String {
    let made = fill(
        gettext("Made {date}"),
        &[("date", &format_expiry(&key.created_at))],
    );
    let scope = if key.permissions.iter().any(|p| p == "all") {
        gettext("All permissions")
    } else {
        fill(
            ngettext(
                "{n} permission",
                "{n} permissions",
                plural_n(key.permissions.len()),
            ),
            &[("n", &key.permissions.len().to_string())],
        )
    };
    format!("{made} · {scope}")
}

fn own_key_row(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
    key: &ApiKey,
    rotate: bool,
) -> libadwaita::ActionRow {
    let row = key_row(key);
    if !rotate {
        return row;
    }
    let button = gtk::Button::builder()
        .label(gettext("Rotate…"))
        .tooltip_text(gettext("Get a new secret for this key"))
        .valign(gtk::Align::Center)
        .build();
    row.add_suffix(&button);
    let (ui, dialog_ref, id) = (ui.clone(), dialog.clone(), key.id.clone());
    button.connect_clicked(move |_| {
        let (ui, dialog, id) = (ui.clone(), dialog_ref.clone(), id.clone());
        super::confirm(
            &dialog_ref,
            (
                &gettext("Get a new secret for this key?"),
                &gettext(
                    "Mimick saves the new secret and keeps working. Anything else using this key stops working until you give it the new secret.",
                ),
                &gettext("Rotate"),
            ),
            move || rotate_own_key(&ui, &dialog, id.clone()),
        );
    });
    row
}

/// The old secret stops working the moment Immich answers, so the new one goes into use at
/// once; if it can't be saved, it's shown so it isn't lost.
fn rotate_own_key(ui: &Rc<LibraryWindowUi>, dialog: &libadwaita::PreferencesDialog, id: String) {
    let (ui, dialog) = (ui.clone(), dialog.clone());
    glib::MainContext::default().spawn_local(async move {
        let secret = match ui.ctx.api_client.rotate_api_key(&id).await {
            Ok(secret) => secret,
            Err(err) => {
                return dialog.add_toast(libadwaita::Toast::new(&fill(
                    gettext("Couldn't rotate the key: {err}"),
                    &[("err", &err)],
                )));
            }
        };
        ui.ctx.api_client.replace_api_key(secret.clone());
        let saved = ui.ctx.config.read().set_api_key(&secret);
        match saved {
            Ok(_) => dialog.add_toast(libadwaita::Toast::new(&gettext("New secret saved"))),
            Err(err) => show_unsaved_secret(&dialog, &secret, &err),
        }
    });
}

fn show_unsaved_secret(dialog: &libadwaita::PreferencesDialog, secret: &str, err: &str) {
    let alert = libadwaita::AlertDialog::builder()
        .heading(gettext("Copy Your New Secret"))
        .body(fill(
            gettext("Mimick is using it now but couldn't save it ({err}). Paste it in Settings so it's kept:\n\n{secret}"),
            &[("err", err), ("secret", secret)],
        ))
        .body_use_markup(false)
        .build();
    alert.add_response("ok", &gettext("OK"));
    alert.present(Some(dialog));
}

fn other_key_row(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
    lists: &Lists,
    key: &ApiKey,
) -> libadwaita::ActionRow {
    let row = key_row(key);
    let delete = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Delete key"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    row.add_suffix(&delete);
    let (ui, dialog_ref, lists, id) = (ui.clone(), dialog.clone(), lists.clone(), key.id.clone());
    delete.connect_clicked(move |_| {
        let (ui, dialog, lists, id) = (ui.clone(), dialog_ref.clone(), lists.clone(), id.clone());
        super::confirm(
            &dialog_ref,
            (
                &gettext("Delete this key?"),
                &gettext("Anything signing in with it stops working. This can't be undone."),
                &gettext("Delete"),
            ),
            move || delete_key(&ui, &dialog, &lists, id.clone()),
        );
    });
    row
}

fn delete_key(
    ui: &Rc<LibraryWindowUi>,
    dialog: &libadwaita::PreferencesDialog,
    lists: &Lists,
    id: String,
) {
    let (ui, dialog, lists) = (ui.clone(), dialog.clone(), lists.clone());
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.delete_api_key(&id).await {
            Ok(()) => load(&ui, &dialog, &lists),
            Err(err) => dialog.add_toast(libadwaita::Toast::new(&fill(
                gettext("Couldn't delete the key: {err}"),
                &[("err", &err)],
            ))),
        }
    });
}

/// Whether a version such as "v3.3.6" is `(major, minor)` or newer.
fn at_least(version: &str, (major, minor): (u32, u32)) -> bool {
    let mut parts = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0));
    let found = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    found >= (major, minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_needs_immich_3_2() {
        assert!(at_least("v3.3.6", ROTATE_SINCE));
        assert!(at_least("3.2.0", ROTATE_SINCE));
        assert!(!at_least("v3.1.9", ROTATE_SINCE));
        assert!(!at_least("garbage", ROTATE_SINCE));
    }

    #[test]
    fn keys_say_how_much_they_can_do() {
        let key = |permissions: &[&str]| ApiKey {
            id: "k".into(),
            name: "Phone".into(),
            created_at: String::new(),
            permissions: permissions.iter().map(ToString::to_string).collect(),
        };
        assert!(key_details(&key(&["all"])).ends_with("All permissions"));
        assert!(key_details(&key(&["asset.read", "album.read"])).ends_with("2 permissions"));
    }
}
