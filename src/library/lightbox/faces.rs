//! Face mode in the lightbox: face boxes on the photo itself, to rename, move, remove, or draw.

use gettextrs::gettext;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

use super::{AssetInfo, Lightbox};
use crate::api_client::{AssetFace, ImmichApiClient, NewFace, PersonUpdate};
use crate::library::asset_object::AssetObject;
use crate::library::context_menu::{Menu, show_error_toast};
use crate::library::people_view::display_name;
use crate::library::person_picker::{self, PersonChoice};
use crate::library::photo_canvas::Region;

/// Hand-drawn faces smaller than this many image pixels are taken as slips.
const MIN_FACE: f64 = 12.0;

/// The shown photo's faces, for face mode and for naming the face under the pointer.
#[derive(Default)]
pub(super) struct PhotoFaces {
    /// Server id of the photo they belong to; empty when it has none to show.
    pub(super) asset_id: RefCell<String>,
    pub(super) list: RefCell<Vec<AssetFace>>,
    /// The face under the pointer, so moving within it doesn't redraw.
    pub(super) hovered: Cell<Option<usize>>,
    /// A menu is open for the highlighted faces, which stay lit wherever the pointer goes.
    pub(super) held: Cell<bool>,
}

/// The bar over the photo while face mode is on.
pub(super) struct FaceMode {
    pub(super) bar: gtk::Revealer,
    done: gtk::Button,
    active: Cell<bool>,
}

impl FaceMode {
    pub(super) fn new() -> Self {
        // The bar says what to do: drawing needs no extra button, so nothing has to be found.
        let hint = gtk::Label::builder()
            .label(gettext("Drag around a face to tag it"))
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .margin_start(10)
            .build();
        let done = gtk::Button::builder()
            .label(gettext("Done"))
            .css_classes(["suggested-action", "pill"])
            .build();
        let bar_content = gtk::Box::builder()
            .spacing(8)
            .css_classes(["osd", "mimick-mode-bar"])
            .build();
        bar_content.append(&gtk::Image::from_icon_name("avatar-default-symbolic"));
        bar_content.append(&hint);
        bar_content.append(&done);
        let bar = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .margin_top(12)
            .child(&bar_content)
            .build();
        Self {
            bar,
            done,
            active: Cell::new(false),
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.active.get()
    }
}

impl Lightbox {
    pub(super) fn connect_faces(self: &Rc<Self>) {
        self.connect_face_input();
        self.on_click(&self.face_mode.done, Self::exit_face_mode);
        let lb = Rc::downgrade(self);
        self.picture.faces.connect_marker_tapped(move |index| {
            if let Some(lb) = lb.upgrade() {
                lb.show_face_menu(index);
            }
        });
        let lb = Rc::downgrade(self);
        self.picture.faces.connect_selection_changed(move |region| {
            if let (Some(lb), Some(region)) = (lb.upgrade(), region) {
                lb.face_drawn(region);
            }
        });
    }

    /// Drags and taps on the photo reach the face boxes only while face mode is on.
    fn connect_face_input(self: &Rc<Self>) {
        let lb = Rc::downgrade(self);
        self.picture.faces.attach_input(
            &self.picture.scrolled,
            gtk::PropagationPhase::Capture,
            move |x, y| {
                let lb = lb.upgrade()?;
                if !lb.face_mode.is_active() {
                    return None;
                }
                let point = lb.picture.scrolled.compute_point(
                    &lb.picture.faces.area,
                    &gtk::graphene::Point::new(x as f32, y as f32),
                )?;
                Some((f64::from(point.x()), f64::from(point.y())))
            },
        );
    }

    /// Shows the photo's face boxes over it; dragging on the photo draws a new one.
    pub(in crate::library) fn enter_face_mode(self: &Rc<Self>) {
        let Some(item) = self.current_item() else {
            return;
        };
        let Some(id) = super::super::actions::TrashTarget::from_item(&item)
            .map(|target| target.remote_id().to_string())
        else {
            return;
        };
        self.stop_slideshow();
        self.stop_live();
        self.leave_panorama();
        self.exit_edit_mode();
        self.actions.ocr_toggle.set_active(false);
        // On a phone the details pane covers the photo, which has to show to tag faces on it.
        if self.ui.split.is_collapsed() {
            self.header.details_btn.set_active(false);
        }
        if self.faces.asset_id.replace(id.clone()) != id {
            self.faces.list.borrow_mut().clear();
        }
        self.face_mode.active.set(true);
        self.place_faces();
        self.picture.faces.set_outline_all(true);
        self.picture.faces.set_can_draw(true);
        self.picture
            .scrolled
            .set_cursor_from_name(Some("crosshair"));
        self.face_mode.bar.set_reveal_child(true);
        self.picture.strip.set_swipe_enabled(false);
        self.reload_faces();
    }

    /// Size of the photo on screen, for placing boxes; any texture of it has the right shape.
    fn shown_size(&self, item: &AssetObject) -> (f64, f64) {
        self.active_picture()
            .paintable()
            .map(|p| {
                (
                    f64::from(p.intrinsic_width()),
                    f64::from(p.intrinsic_height()),
                )
            })
            .filter(|(w, h)| *w > 0.0 && *h > 0.0)
            .unwrap_or((f64::from(item.width()), f64::from(item.height())))
    }

    /// Leaves face mode; the boxes stay, unseen until one is pointed at.
    pub(super) fn exit_face_mode(self: &Rc<Self>) {
        let mode = &self.face_mode;
        if !mode.active.replace(false) {
            return;
        }
        mode.bar.set_reveal_child(false);
        self.picture.faces.set_can_draw(false);
        self.picture.faces.set_outline_all(false);
        self.picture.scrolled.set_cursor(None::<&gtk::gdk::Cursor>);
        self.picture.faces.set_selection(None);
        self.picture
            .strip
            .set_swipe_enabled((self.zoom_level.get() - 1.0).abs() < 0.001);
    }

    /// Escape leaves face mode before it would close the lightbox.
    pub(super) fn handle_face_key(self: &Rc<Self>, key: gtk::gdk::Key) -> bool {
        let leaving = self.face_mode.is_active() && key == gtk::gdk::Key::Escape;
        if leaving {
            self.exit_face_mode();
        }
        leaving
    }

    /// Loads the faces of your own photos, the ones the People section is shown for.
    pub(super) fn load_faces(self: &Rc<Self>, info: &AssetInfo, editable: bool) {
        let wanted = editable && !info.is_video() && self.ui.features.get().people;
        self.faces.list.borrow_mut().clear();
        self.place_faces();
        let id = if wanted {
            info.asset_id.clone()
        } else {
            String::new()
        };
        self.faces.asset_id.replace(id);
        self.reload_faces();
    }

    /// Fetches the faces again; a result for a photo the user has left is dropped.
    fn reload_faces(self: &Rc<Self>) {
        let id = self.faces.asset_id.borrow().clone();
        if id.is_empty() {
            return;
        }
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = lb.ui.ctx.api_client.fetch_faces(&id).await;
            if *lb.faces.asset_id.borrow() != id {
                return;
            }
            match result {
                Ok(faces) => lb.show_faces(faces),
                Err(err) if lb.face_mode.is_active() => {
                    show_error_toast(&lb.ui, &gettext("Couldn't load the faces"), &err)
                }
                Err(err) => log::debug!("Faces fetch failed for {id}: {err}"),
            }
        });
    }

    fn show_faces(&self, faces: Vec<AssetFace>) {
        if faces.is_empty() && self.face_mode.is_active() {
            self.ui.toasts.add_toast(libadwaita::Toast::new(&gettext(
                "No faces yet. Drag around one to tag it.",
            )));
        }
        *self.faces.list.borrow_mut() = faces;
        self.place_faces();
    }

    /// Puts the face boxes on the photo, in the pixels of the picture shown, and clears
    /// any highlight, since indices into the old list no longer hold.
    fn place_faces(&self) {
        self.release_face_highlight();
        let Some(item) = self.current_item() else {
            return;
        };
        let size = self.shown_size(&item);
        self.picture.faces.set_image_size(size);
        let faces = self.faces.list.borrow();
        let markers = faces.iter().map(|face| face_region(face, size)).collect();
        self.picture.faces.set_markers(markers);
    }

    /// A box in the face layer's coordinates, as a rectangle on the picture scroller.
    fn on_scroller(&self, r: Region) -> Option<gtk::gdk::Rectangle> {
        let layer = &self.picture.faces.area;
        let point = layer.compute_point(
            &self.picture.scrolled,
            &gtk::graphene::Point::new(r.x as f32, r.y as f32),
        )?;
        Some(gtk::gdk::Rectangle::new(
            point.x() as i32,
            point.y() as i32,
            r.w.max(1.0) as i32,
            r.h.max(1.0) as i32,
        ))
    }

    fn show_face_menu(self: &Rc<Self>, index: usize) {
        let Some(face) = self.faces.list.borrow().get(index).cloned() else {
            return;
        };
        let Some(at) = self
            .picture
            .faces
            .marker_rect(index)
            .and_then(|r| self.on_scroller(r))
        else {
            return;
        };
        self.hold_face_highlight(vec![index]);
        // The menu points where the name would show, and names the face itself.
        self.picture.face_label.set_visible(false);
        let menu = Menu::new(
            &self.picture.scrolled,
            f64::from(at.x() + at.width() / 2),
            f64::from(at.y() + at.height()),
        );
        menu.heading(&face_name(&face));
        self.add_face_actions(&menu, &face, at);
        let lb = Rc::downgrade(self);
        menu.on_closed(move || {
            if let Some(lb) = lb.upgrade() {
                lb.release_face_highlight();
            }
        });
        menu.popup();
    }

    fn add_face_actions(self: &Rc<Self>, menu: &Menu, face: &AssetFace, at: gtk::gdk::Rectangle) {
        let label = if face.person.is_some() {
            gettext("Change Person…")
        } else {
            gettext("Choose Person…")
        };
        let (lb, face_id) = (self.clone(), face.id.clone());
        menu.add_with_icon("avatar-default-symbolic", &label, false, move || {
            let (lb2, face_id) = (lb.clone(), face_id.clone());
            person_picker::present(&lb.ui, &lb.picture.scrolled, at, move |choice| {
                let face_id = face_id.clone();
                lb2.change_faces(gettext("Face moved"), async move |api| {
                    let person = person_id(&api, choice).await?;
                    api.reassign_face(&face_id, &person).await
                });
            });
        });
        if let Some(person) = &face.person {
            let (lb, person_id) = (self.clone(), person.id.clone());
            menu.add_with_icon(
                "image-x-generic-symbolic",
                &gettext("Use as Cover Photo"),
                false,
                move || {
                    let update = PersonUpdate {
                        feature_face_asset_id: Some(lb.faces.asset_id.borrow().clone()),
                        ..PersonUpdate::default()
                    };
                    let person_id = person_id.clone();
                    lb.change_faces(gettext("Cover photo changed"), async move |api| {
                        api.update_person(&person_id, &update).await
                    });
                },
            );
        }
        let (lb, face_id) = (self.clone(), face.id.clone());
        menu.add_with_icon(
            "list-remove-symbolic",
            &gettext("Remove Face"),
            true,
            move || {
                let face_id = face_id.clone();
                lb.change_faces(gettext("Face removed"), async move |api| {
                    api.delete_face(&face_id).await
                });
            },
        );
    }

    /// A box drawn on the photo: ask who it is, right at the box. Tiny ones are slips.
    fn face_drawn(self: &Rc<Self>, region: Region) {
        if !self.face_mode.is_active() || region.w < MIN_FACE || region.h < MIN_FACE {
            self.picture.faces.set_selection(None);
            return;
        }
        let Some(at) = self
            .picture
            .faces
            .selection_rect()
            .and_then(|r| self.on_scroller(r))
        else {
            return;
        };
        let lb = self.clone();
        person_picker::present(&self.ui, &self.picture.scrolled, at, move |choice| {
            lb.tag_face(region, choice);
        });
    }

    fn tag_face(self: &Rc<Self>, region: Region, choice: PersonChoice) {
        let (w, h) = self.picture.faces.image_size();
        let face = NewFace {
            image_width: w.round() as u32,
            image_height: h.round() as u32,
            x: region.x.round() as u32,
            y: region.y.round() as u32,
            width: region.w.round() as u32,
            height: region.h.round() as u32,
        };
        let asset_id = self.faces.asset_id.borrow().clone();
        self.picture.faces.set_selection(None);
        self.change_faces(gettext("Face tagged"), async move |api| {
            let person = person_id(&api, choice).await?;
            api.create_face(&asset_id, &person, face).await
        });
    }

    /// Runs one change, then reloads the boxes and the People section and says `done`,
    /// or shows why it failed.
    pub(super) fn change_faces<F, Fut>(self: &Rc<Self>, done: String, change: F)
    where
        F: FnOnce(std::sync::Arc<ImmichApiClient>) -> Fut + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + 'static,
    {
        let lb = self.clone();
        let api = self.ui.ctx.api_client.clone();
        glib::MainContext::default().spawn_local(async move {
            match change(api).await {
                Ok(()) => {
                    lb.ui.toasts.add_toast(libadwaita::Toast::new(&done));
                    lb.reload_faces();
                    lb.refresh_people();
                }
                Err(err) => {
                    show_error_toast(&lb.ui, &gettext("Immich didn't accept the change"), &err)
                }
            }
        });
    }

    /// Fetches the photo's people again and redraws the details pane's People section.
    fn refresh_people(self: &Rc<Self>) {
        let (Some(item), pos) = (self.current_item(), self.pos.get()) else {
            return;
        };
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let id = item.id();
            let details = lb.ui.ctx.api_client.fetch_asset_details(&id).await;
            if lb.pos.get() != pos {
                return;
            }
            if let Ok(details) = details {
                let taken = details.local_date_time.as_deref();
                lb.details.people.show(&lb.ui, &details.people, taken, true);
            }
        });
    }
}

pub(super) fn face_name(face: &AssetFace) -> String {
    face.person.as_ref().map_or_else(
        || gettext("Unknown Person"),
        |person| display_name(person).into_owned(),
    )
}

/// The picked person's id, creating the person first when a new name was typed.
pub(super) async fn person_id(
    api: &ImmichApiClient,
    choice: PersonChoice,
) -> Result<String, String> {
    match choice {
        PersonChoice::Existing(person) => Ok(person.id),
        PersonChoice::New(name) => Ok(api.create_person(&name).await?.id),
    }
}

/// A face box in the shown photo's pixels; faces carry the size of the image they were found on.
pub(super) fn face_region(face: &AssetFace, (w, h): (f64, f64)) -> Region {
    let sx = w / f64::from(face.image_width.max(1));
    let sy = h / f64::from(face.image_height.max(1));
    Region {
        x: face.x1 as f64 * sx,
        y: face.y1 as f64 * sy,
        w: (face.x2 - face.x1).max(0) as f64 * sx,
        h: (face.y2 - face.y1).max(0) as f64 * sy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_boxes_scale_to_the_shown_photo() {
        let face: AssetFace = serde_json::from_value(serde_json::json!({
            "id": "f",
            "imageWidth": 1000,
            "imageHeight": 500,
            "boundingBoxX1": 100, "boundingBoxY1": 50,
            "boundingBoxX2": 300, "boundingBoxY2": 150,
            "person": null
        }))
        .unwrap();
        let region = face_region(&face, (500.0, 250.0));
        assert_eq!(
            region,
            Region {
                x: 50.0,
                y: 25.0,
                w: 100.0,
                h: 50.0
            }
        );
    }
}
