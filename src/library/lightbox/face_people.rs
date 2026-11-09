//! The photo's faces and the details pane's People point at each other: the face under the
//! pointer is outlined, named, and its person ringed; pointing at or focusing a person there
//! outlines their faces. Each person's menu there changes or removes their faces.

use gettextrs::gettext;
use std::rc::Rc;

use gtk::prelude::*;

use super::Lightbox;
use super::faces::{face_name, person_id};
use super::people::PeopleHooks;
use crate::api_client::{ImmichApiClient, Person};
use crate::library::actions::TrashTarget;
use crate::library::context_menu::Menu;
use crate::library::people_view::display_name;
use crate::library::person_picker;
use crate::library::photo_canvas::{BoxLayer, Region};

/// Space between a face box and its name, in screen pixels.
const LABEL_GAP: f64 = 4.0;

/// The name shown with a highlighted face, placed by `content`, the overlay holding `layer`.
pub(super) fn face_label(content: &gtk::Overlay, layer: &BoxLayer) -> gtk::Label {
    let label = gtk::Label::builder()
        .css_classes(["mimick-face-label"])
        .can_target(false)
        .visible(false)
        .build();
    content.add_overlay(&label);
    let (layer, placed) = (layer.clone(), label.downgrade());
    content.connect_get_child_position(move |overlay, child| {
        let label = placed
            .upgrade()
            .filter(|label| label.upcast_ref::<gtk::Widget>() == child)?;
        let face = layer.highlight_rect()?;
        let (_, natural) = label.preferred_size();
        let size = (f64::from(natural.width()), f64::from(natural.height()));
        let area = (f64::from(overlay.width()), f64::from(overlay.height()));
        let (x, y) = label_spot(face, size, area);
        Some(gtk::gdk::Rectangle::new(
            x as i32,
            y as i32,
            natural.width(),
            natural.height(),
        ))
    });
    label
}

/// Top-left corner for a `label`-sized name centred under `face`, or over it when it would
/// run off the bottom, kept inside `area`.
fn label_spot(face: Region, label: (f64, f64), area: (f64, f64)) -> (f64, f64) {
    let x = (face.x + (face.w - label.0) / 2.0).clamp(0.0, (area.0 - label.0).max(0.0));
    let below = face.y + face.h + LABEL_GAP;
    let y = if below + label.1 <= area.1 {
        below
    } else {
        (face.y - LABEL_GAP - label.1).max(0.0)
    };
    (x, y)
}

impl Lightbox {
    /// Pointing at faces on the photo, and at people in the details pane.
    pub(super) fn connect_face_highlights(self: &Rc<Self>) {
        let motion = gtk::EventControllerMotion::new();
        let lb = Rc::downgrade(self);
        motion.connect_motion(move |_, x, y| {
            if let Some(lb) = lb.upgrade() {
                lb.point_at_face(lb.face_under(x, y));
            }
        });
        let lb = Rc::downgrade(self);
        motion.connect_leave(move |_| {
            if let Some(lb) = lb.upgrade() {
                lb.point_at_face(None);
            }
        });
        self.picture.scrolled.add_controller(motion);
        let (on_point, on_menu) = (Rc::downgrade(self), Rc::downgrade(self));
        self.details.people.bind(PeopleHooks {
            point: Box::new(move |person| {
                if let Some(lb) = on_point.upgrade() {
                    lb.point_at_person(person);
                }
            }),
            menu: Box::new(move |person, tile, x, y| {
                if let Some(lb) = on_menu.upgrade() {
                    lb.show_person_menu(person, tile, x, y);
                }
            }),
        });
    }

    /// The face under a point on the picture scroller that pointing names: any face in face
    /// mode, otherwise only people the details pane shows. None while text or edits are shown.
    fn face_under(&self, x: f64, y: f64) -> Option<usize> {
        if self.edit_mode.is_active()
            || self.slideshow.is_running()
            || self.picture.ocr.area.is_visible()
        {
            return None;
        }
        let point = self.picture.scrolled.compute_point(
            &self.picture.faces.area,
            &gtk::graphene::Point::new(x as f32, y as f32),
        )?;
        let index = self
            .picture
            .faces
            .marker_at((f64::from(point.x()), f64::from(point.y())))?;
        let shown = self
            .faces
            .list
            .borrow()
            .get(index)
            .is_some_and(|face| face.person.as_ref().is_some_and(|person| !person.is_hidden));
        (shown || self.face_mode.is_active()).then_some(index)
    }

    fn point_at_face(&self, face: Option<usize>) {
        if self.faces.held.get() || self.faces.hovered.replace(face) == face {
            return;
        }
        self.show_face_highlight(face.into_iter().collect());
    }

    fn point_at_person(&self, person: Option<&str>) {
        if self.faces.held.get() {
            return;
        }
        self.faces.hovered.set(None);
        let faces = person.map(|id| self.faces_of(id)).unwrap_or_default();
        self.show_face_highlight(faces);
    }

    /// Where `person`'s faces are in the shown photo's list.
    fn faces_of(&self, person: &str) -> Vec<usize> {
        self.faces
            .list
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, face)| face.person.as_ref().is_some_and(|p| p.id == person))
            .map(|(index, _)| index)
            .collect()
    }

    /// Keeps `faces` lit, wherever the pointer goes, until `release_face_highlight`.
    pub(super) fn hold_face_highlight(&self, faces: Vec<usize>) {
        self.show_face_highlight(faces);
        self.faces.held.set(true);
    }

    pub(super) fn release_face_highlight(&self) {
        self.faces.held.set(false);
        self.faces.hovered.set(None);
        self.show_face_highlight(Vec::new());
    }

    /// Outlines `faces`, names the first, and rings its person in the details pane.
    fn show_face_highlight(&self, faces: Vec<usize>) {
        let (name, person) = {
            let list = self.faces.list.borrow();
            let first = faces.first().and_then(|&index| list.get(index));
            let person = first.and_then(|face| face.person.as_ref());
            (first.map(face_name), person.map(|p| p.id.clone()))
        };
        self.details.people.mark(person.as_deref());
        self.picture.faces.highlight(faces);
        let label = &self.picture.face_label;
        label.set_label(name.as_deref().unwrap_or_default());
        label.set_visible(name.is_some());
        label.queue_resize();
    }

    /// A person's menu in the details pane; their faces stay outlined while it's open.
    fn show_person_menu(self: &Rc<Self>, person: &Person, tile: &gtk::Button, x: f64, y: f64) {
        let menu = Menu::new(tile, x, y);
        menu.heading(&display_name(person));
        let (lb, id, anchor) = (self.clone(), person.id.clone(), tile.downgrade());
        menu.add_with_icon(
            "avatar-default-symbolic",
            &gettext("Change Person…"),
            false,
            move || {
                if let Some(tile) = anchor.upgrade() {
                    lb.change_person_on_photo(&id, &tile);
                }
            },
        );
        let (lb, id) = (self.clone(), person.id.clone());
        menu.add_with_icon(
            "list-remove-symbolic",
            &gettext("Remove from Photo"),
            true,
            move || {
                lb.remove_person_from_photo(&id);
            },
        );
        self.hold_face_highlight(self.faces_of(&person.id));
        let lb = Rc::downgrade(self);
        menu.on_closed(move || {
            if let Some(lb) = lb.upgrade() {
                lb.release_face_highlight();
            }
        });
        menu.popup();
    }

    /// Moves `person`'s faces on the photo to whoever is picked, from a picker by their tile.
    fn change_person_on_photo(self: &Rc<Self>, person: &str, tile: &gtk::Button) {
        let Some(asset_id) = self.shown_remote_id() else {
            return;
        };
        let at = gtk::gdk::Rectangle::new(0, 0, tile.width(), tile.height());
        let (lb, person) = (self.clone(), person.to_string());
        person_picker::present(&self.ui, tile, at, move |choice| {
            let (asset_id, person) = (asset_id.clone(), person.clone());
            lb.change_faces(gettext("Face moved"), async move |api| {
                let faces = face_ids_of(&api, &asset_id, &person).await?;
                if faces.is_empty() {
                    return Ok(());
                }
                let to = person_id(&api, choice).await?;
                for face in faces {
                    api.reassign_face(&face, &to).await?;
                }
                Ok(())
            });
        });
    }

    fn remove_person_from_photo(self: &Rc<Self>, person: &str) {
        let Some(asset_id) = self.shown_remote_id() else {
            return;
        };
        let person = person.to_string();
        self.change_faces(gettext("Face removed"), async move |api| {
            for face in face_ids_of(&api, &asset_id, &person).await? {
                api.delete_face(&face).await?;
            }
            Ok(())
        });
    }

    fn shown_remote_id(&self) -> Option<String> {
        let item = self.current_item()?;
        TrashTarget::from_item(&item).map(|target| target.remote_id().to_string())
    }
}

/// `person`'s faces on the photo, fetched afresh in case the shown ones are still loading.
async fn face_ids_of(
    api: &ImmichApiClient,
    asset_id: &str,
    person: &str,
) -> Result<Vec<String>, String> {
    let faces = api.fetch_faces(asset_id).await?;
    Ok(faces
        .into_iter()
        .filter(|face| face.person.as_ref().is_some_and(|p| p.id == person))
        .map(|face| face.id)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(x: f64, y: f64) -> Region {
        Region {
            x,
            y,
            w: 40.0,
            h: 40.0,
        }
    }

    #[test]
    fn names_sit_centred_under_the_face() {
        assert_eq!(
            label_spot(face(100.0, 100.0), (60.0, 20.0), (500.0, 500.0)),
            (90.0, 144.0)
        );
    }

    #[test]
    fn names_go_over_faces_at_the_bottom_and_stay_inside() {
        assert_eq!(
            label_spot(face(-10.0, 470.0), (60.0, 20.0), (500.0, 500.0)),
            (0.0, 446.0)
        );
        assert_eq!(
            label_spot(face(480.0, 0.0), (60.0, 20.0), (500.0, 500.0)),
            (440.0, 44.0)
        );
    }
}
