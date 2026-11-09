//! Copy and paste. Copying several photos puts references on the clipboard, as file managers
//! do, so any number costs next to nothing; pasting goes where the Upload button would.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio};

use super::album_picker::add_and_report;
use super::asset_object::AssetObject;
use super::context_menu::show_error_toast;
use super::{LOCAL_ID_PREFIX, LibraryWindowUi};
use crate::api_client::CopyUpload;

/// What Mimick put on the clipboard, told apart from anything copied since by its provider.
pub(super) struct Copied {
    provider: gdk::ContentProvider,
    photos: Vec<CopiedPhoto>,
}

#[derive(Clone)]
pub(super) struct CopiedPhoto {
    /// Empty for a photo that's only on this computer.
    remote_id: String,
    /// The file on this computer, if there is one.
    path: Option<PathBuf>,
}

impl CopiedPhoto {
    pub(super) fn new(remote_id: &str, path: Option<PathBuf>) -> Self {
        Self {
            remote_id: remote_id.to_string(),
            path,
        }
    }

    fn from_item(item: &AssetObject) -> Option<Self> {
        let remote_id = item.property::<String>("remote-id");
        let remote_id = if remote_id.is_empty() {
            item.id()
        } else {
            remote_id
        };
        let remote_id = if remote_id.starts_with(LOCAL_ID_PREFIX) {
            String::new()
        } else {
            remote_id
        };
        let path = Some(item.property::<String>("local-path"))
            .filter(|path| !path.is_empty())
            .map(PathBuf::from);
        (!remote_id.is_empty() || path.is_some()).then_some(Self { remote_id, path })
    }
}

enum Pasted {
    Copied(Vec<CopiedPhoto>),
    Files,
    Image,
    Nothing,
}

/// `win.paste` for the Upload button's menu; Ctrl+V, and Ctrl+C on a selection.
pub(super) fn connect_clipboard(ui: &Rc<LibraryWindowUi>) {
    let action = gio::SimpleAction::new("paste", None);
    let ui_action = ui.clone();
    action.connect_activate(move |_, _| paste(&ui_action));
    ui.window.add_action(&action);
    // Text fields take these keys before they bubble up to the window, so they keep theirs.
    super::controls::add_shortcut(ui, "<Control>v", |ui| {
        let shown = library_shown(ui);
        if shown {
            paste(ui);
        }
        shown
    });
    super::controls::add_shortcut(ui, "<Control>c", copy_selection);
}

/// Not while the lightbox, which has its own Ctrl+C, or a dialog is open.
fn library_shown(ui: &LibraryWindowUi) -> bool {
    ui.nav.navigation_stack().n_items() == 1 && super::controls::no_dialog_open(ui)
}

/// Puts `provider` on the clipboard, remembering which photos it holds.
pub(super) fn set_copied(
    ui: &LibraryWindowUi,
    provider: gdk::ContentProvider,
    photos: Vec<CopiedPhoto>,
) {
    if let Err(err) = ui.window.clipboard().set_content(Some(&provider)) {
        log::warn!("Could not set the clipboard: {err}");
        return;
    }
    ui.interaction
        .copied
        .replace(Some(Copied { provider, photos }));
}

fn copy_selection(ui: &Rc<LibraryWindowUi>) -> bool {
    if !library_shown(ui) || !ui.select_toggle.is_active() {
        return false;
    }
    let items = super::actions::selected_items(ui);
    if items.is_empty() {
        return false;
    }
    copy_photos(ui, &items);
    true
}

/// One photo copies as its picture, for pasting into other apps; several, or a video, as
/// references, since only one picture fits on the clipboard.
pub(super) fn copy_photos(ui: &Rc<LibraryWindowUi>, items: &[AssetObject]) {
    match items {
        [item] if is_image(item) => super::context_menu::copy_picture(ui, item),
        _ => copy_references(ui, items),
    }
}

fn is_image(item: &AssetObject) -> bool {
    item.property::<String>("asset-type")
        .eq_ignore_ascii_case("IMAGE")
}

/// Photo ids for pasting into albums here, and the files on this computer for other apps.
fn copy_references(ui: &LibraryWindowUi, items: &[AssetObject]) {
    let photos: Vec<CopiedPhoto> = items.iter().filter_map(CopiedPhoto::from_item).collect();
    let files: Vec<gio::File> = photos
        .iter()
        .filter_map(|photo| photo.path.as_ref())
        .map(gio::File::for_path)
        .collect();
    let (count, local) = (photos.len(), files.len());
    let provider = gdk::ContentProvider::for_value(&gdk::FileList::from_array(&files).to_value());
    set_copied(ui, provider, photos);
    let title = fill(
        ngettext("Copied {n} photo", "Copied {n} photos", plural_n(count)),
        &[("n", &count.to_string())],
    );
    if local == count {
        toast(ui, &title);
        return;
    }
    // Downloading them all just in case would be the memory and disk cost copying avoids.
    let details = fill(
        gettext(
            "Pasting into an album here adds all {count}. Only {local} are on this computer, \
             so other apps such as Files paste those; use Download for the rest.",
        ),
        &[("count", &count.to_string()), ("local", &local.to_string())],
    );
    show_error_toast(ui, &title, &details);
}

fn paste(ui: &Rc<LibraryWindowUi>) {
    // The Upload button shows exactly where new photos can go.
    if !ui.upload_button.get_visible() {
        toast(ui, &gettext("Paste in Photos or albums"));
        return;
    }
    let album = super::controls::current_album(ui);
    let clipboard = ui.window.clipboard();
    match what_is_pasted(ui, &clipboard) {
        Pasted::Copied(photos) => paste_copied(ui, album, photos),
        Pasted::Files => {
            let ui = ui.clone();
            glib::MainContext::default().spawn_local(async move {
                match clipboard
                    .read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT)
                    .await
                {
                    Ok(value) => upload_files(&ui, album, file_paths(&value)),
                    Err(err) => show_error_toast(&ui, &gettext("Couldn't paste"), &err.to_string()),
                }
            });
        }
        Pasted::Image => {
            let ui = ui.clone();
            glib::MainContext::default().spawn_local(async move {
                match clipboard.read_texture_future().await {
                    Ok(Some(texture)) => upload_image(ui, album, texture).await,
                    Ok(None) => toast(&ui, &gettext("No photos to paste")),
                    Err(err) => show_error_toast(&ui, &gettext("Couldn't paste"), &err.to_string()),
                }
            });
        }
        Pasted::Nothing => toast(ui, &gettext("No photos to paste")),
    }
}

/// Mimick's own copy first, then files, as a file manager's copy may offer an image too.
fn what_is_pasted(ui: &LibraryWindowUi, clipboard: &gdk::Clipboard) -> Pasted {
    if let Some(copied) = ui.interaction.copied.borrow().as_ref()
        && clipboard.content().as_ref() == Some(&copied.provider)
    {
        return Pasted::Copied(copied.photos.clone());
    }
    let formats = clipboard.formats().union_deserialize_types();
    if formats.contains_type(gdk::FileList::static_type()) {
        Pasted::Files
    } else if formats.contains_type(gdk::Texture::static_type()) {
        Pasted::Image
    } else {
        Pasted::Nothing
    }
}

fn file_paths(value: &glib::Value) -> Vec<PathBuf> {
    value
        .get::<gdk::FileList>()
        .map(|list| list.files().iter().filter_map(FileExt::path).collect())
        .unwrap_or_default()
}

/// Through the upload queue, like the Upload button, so progress and retries work the same.
fn upload_files(ui: &LibraryWindowUi, album: Option<(String, String)>, paths: Vec<PathBuf>) {
    let paths: Vec<PathBuf> = paths
        .into_iter()
        .filter(|path| crate::media_kinds::is_supported_path(path))
        .collect();
    if paths.is_empty() {
        toast(ui, &gettext("No photos to paste"));
        return;
    }
    toast(
        ui,
        &fill(
            ngettext(
                "Uploading {n} file…",
                "Uploading {n} files…",
                plural_n(paths.len()),
            ),
            &[("n", &paths.len().to_string())],
        ),
    );
    super::upload_picker::spawn_enqueue(ui.ctx.clone(), album, paths);
}

/// Photos already in Immich join the album as they are; ones only on this computer upload.
fn paste_copied(
    ui: &Rc<LibraryWindowUi>,
    album: Option<(String, String)>,
    photos: Vec<CopiedPhoto>,
) {
    let (on_server, local_only): (Vec<CopiedPhoto>, Vec<CopiedPhoto>) = photos
        .into_iter()
        .partition(|photo| !photo.remote_id.is_empty());
    let local_paths: Vec<PathBuf> = local_only
        .into_iter()
        .filter_map(|photo| photo.path)
        .collect();
    if !local_paths.is_empty() {
        upload_files(ui, album.clone(), local_paths);
    }
    let ids: Vec<String> = on_server.into_iter().map(|photo| photo.remote_id).collect();
    match album {
        Some((id, name)) if !ids.is_empty() => add_and_report(ui, id, name, ids, Rc::new(|| {})),
        None if !ids.is_empty() => toast(ui, &gettext("Already in your library")),
        _ => {}
    }
}

/// An image with no file behind it, such as a screenshot, uploaded as a PNG.
async fn upload_image(
    ui: Rc<LibraryWindowUi>,
    album: Option<(String, String)>,
    texture: gdk::Texture,
) {
    toast(&ui, &gettext("Uploading pasted image…"));
    let now = chrono::Local::now();
    let upload = CopyUpload {
        bytes: texture.save_to_png_bytes().to_vec(),
        filename: format!(
            "{} {}.png",
            gettext("Pasted image"),
            now.format("%Y-%m-%d %H-%M-%S")
        ),
        mime: "image/png",
        created_at: now.to_rfc3339(),
        favorite: false,
    };
    match ui.ctx.api_client.upload_copy(upload).await {
        Ok(id) => match album {
            Some((album_id, name)) => add_and_report(&ui, album_id, name, vec![id], Rc::new(|| {})),
            None => {
                toast(&ui, &gettext("Pasted image uploaded"));
                super::refresh_library_after_mutation(ui.clone(), true);
            }
        },
        Err(err) => show_error_toast(&ui, &gettext("Couldn't paste the image"), &err),
    }
}

fn toast(ui: &LibraryWindowUi, text: &str) {
    ui.toasts.add_toast(libadwaita::Toast::new(text));
}
