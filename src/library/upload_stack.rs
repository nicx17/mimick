//! "Upload and Create Stack…" in a photo's menu: upload files and stack them under the photo,
//! as Immich's viewer menu does.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::gio;

use super::LibraryWindowUi;
use super::asset_object::AssetObject;
use super::context_menu::{Menu, show_error_toast};
use crate::api_client::ImmichApiClient;

/// What the picked files are stacked with, and how the menu that offered it hears back.
#[derive(Clone)]
struct Target {
    item: AssetObject,
    asset_id: String,
    on_stacked: Rc<dyn Fn()>,
}

/// On a stacked photo it adds to that stack; `on_stacked` regroups the grid afterwards.
pub(super) fn append_button(
    menu: &Menu,
    ui: &Rc<LibraryWindowUi>,
    item: &AssetObject,
    asset_id: String,
    on_stacked: Rc<dyn Fn()>,
) {
    let label = if item.stack_id().is_empty() {
        gettext("Upload and Create Stack…")
    } else {
        gettext("Upload and Add to Stack…")
    };
    let target = Target {
        item: item.clone(),
        asset_id,
        on_stacked,
    };
    let ui = ui.clone();
    menu.add_with_icon("mimick-upload-symbolic", &label, false, move || {
        pick(&ui, target.clone())
    });
}

fn pick(ui: &Rc<LibraryWindowUi>, target: Target) {
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Upload and Stack"))
        .modal(true)
        .build();
    let (ui, window) = (ui.clone(), ui.window.clone());
    dialog.open_multiple(Some(&window), gio::Cancellable::NONE, move |result| {
        let files = match result {
            Ok(files) => files,
            Err(err) => {
                if !err.matches(gtk::DialogError::Dismissed) {
                    log::warn!("Upload and stack picker failed: {err}");
                }
                return;
            }
        };
        let paths = super::upload_picker::picked_paths(&files);
        if !paths.is_empty() {
            glib::MainContext::default().spawn_local(upload_and_stack(ui, target, paths));
        }
    });
}

async fn upload_and_stack(ui: Rc<LibraryWindowUi>, target: Target, paths: Vec<PathBuf>) {
    ui.toasts.add_toast(libadwaita::Toast::new(&fill(
        ngettext(
            "Uploading {n} file…",
            "Uploading {n} files…",
            plural_n(paths.len()),
        ),
        &[("n", &paths.len().to_string())],
    )));
    let api = &ui.ctx.api_client;
    let mut ids = vec![cover_id(api, &target).await];
    let failed = upload_all(api, &paths, &mut ids).await;
    if failed > 0 {
        report_failed(&ui, failed).await;
    }
    if ids.len() > 1 {
        stack_uploads(&ui, &target, &ids).await;
    }
}

/// Uploads each file, adding its id to `ids`; returns how many failed.
async fn upload_all(api: &ImmichApiClient, paths: &[PathBuf], ids: &mut Vec<String>) -> usize {
    let mut failed = 0;
    for path in paths {
        match upload_one(api, path).await {
            Some(id) if !ids.contains(&id) => ids.push(id),
            // The file is this photo, or picked twice: nothing to add.
            Some(_) => {}
            None => failed += 1,
        }
    }
    failed
}

async fn stack_uploads(ui: &LibraryWindowUi, target: &Target, ids: &[String]) {
    let message = if target.item.stack_id().is_empty() {
        gettext("Stack created")
    } else {
        gettext("Added to the stack")
    };
    match ui.ctx.api_client.create_stack(ids).await {
        Ok(stack) => {
            // Immich's reply lists every photo, including an old stack's it folded in.
            let count = stack.assets.len().max(ids.len());
            target.item.set_stack_id(stack.id.as_str());
            target
                .item
                .set_stack_count(u32::try_from(count).unwrap_or(u32::MAX));
            ui.toasts.add_toast(libadwaita::Toast::new(&message));
            (target.on_stacked)();
        }
        Err(err) => show_error_toast(ui, &gettext("Couldn't stack the photos"), &err),
    }
}

/// The client records why an upload failed; show that, as the sync queue's errors do.
async fn report_failed(ui: &LibraryWindowUi, failed: usize) {
    let title = fill(
        ngettext(
            "Couldn't upload {n} file",
            "Couldn't upload {n} files",
            plural_n(failed),
        ),
        &[("n", &failed.to_string())],
    );
    let details = match ui.ctx.api_client.latest_issue().await {
        Some(issue) => format!("{}\n\n{}", issue.summary, issue.guidance),
        None => gettext("Immich didn't accept the upload."),
    };
    show_error_toast(ui, &title, &details);
}

/// Immich makes the first photo the cover, and folds in an old stack only when its cover is
/// listed, so a stack member stands in for its cover.
async fn cover_id(api: &ImmichApiClient, target: &Target) -> String {
    let stack_id = target.item.stack_id();
    if stack_id.is_empty() {
        return target.asset_id.clone();
    }
    match api.fetch_stack(&stack_id).await {
        Ok(stack) => stack.primary_asset_id,
        Err(err) => {
            log::debug!("Could not load stack {stack_id}: {err}");
            target.asset_id.clone()
        }
    }
}

/// The new photo's id, or the existing one's when Immich already has the file.
async fn upload_one(api: &ImmichApiClient, path: &Path) -> Option<String> {
    let path = path.to_str()?.to_string();
    let hashed = path.clone();
    let checksum =
        tokio::task::spawn_blocking(move || crate::monitor::compute_sha1_chunked(&hashed))
            .await
            .ok()?
            .ok()?;
    if let Some(existing) = api.find_existing_asset_id(&checksum).await {
        return Some(existing);
    }
    let sidecar = crate::sidecar::find_sidecar(Path::new(&path));
    let sidecar = sidecar.as_deref().and_then(Path::to_str);
    api.upload_asset(&path, &checksum, sidecar, None).await
}
