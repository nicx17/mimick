//! Previous, current, and next photo side by side, swiped and sprung between like a carousel.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use libadwaita::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slot {
    Prev,
    Current,
    Next,
}

impl Slot {
    fn index(self) -> usize {
        match self {
            Self::Prev => 0,
            Self::Current => 1,
            Self::Next => 2,
        }
    }
}

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use libadwaita::prelude::AnimationExt;
    use libadwaita::subclass::prelude::*;

    #[derive(Default)]
    pub struct PhotoStrip {
        pub(super) pictures: RefCell<Vec<gtk::Picture>>,
        pub(super) assets: RefCell<[String; 3]>,
        pub(super) progress: Cell<f64>,
        pub(super) neighbours: Cell<(bool, bool)>,
        pub(super) animation: RefCell<Option<(libadwaita::SpringAnimation, f64)>>,
        pub(super) tracker: OnceCell<libadwaita::SwipeTracker>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PhotoStrip {
        const NAME: &str = "MimickPhotoStrip";
        type Type = super::PhotoStrip;
        type ParentType = gtk::Widget;
        type Interfaces = (libadwaita::Swipeable,);
    }

    impl ObjectImpl for PhotoStrip {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_overflow(gtk::Overflow::Hidden);
            let pictures: Vec<gtk::Picture> = (0..3)
                .map(|_| super::super::widgets::lightbox_picture())
                .collect();
            for picture in &pictures {
                picture.set_parent(&*obj);
            }
            *self.pictures.borrow_mut() = pictures;
            let tracker = libadwaita::SwipeTracker::new(&*obj);
            tracker.set_allow_long_swipes(false);
            tracker.set_allow_mouse_drag(false);
            let _ = self.tracker.set(tracker);
        }

        fn dispose(&self) {
            if let Some((animation, _)) = self.animation.take() {
                animation.pause();
            }
            for picture in self.pictures.take() {
                picture.unparent();
            }
        }
    }

    impl WidgetImpl for PhotoStrip {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            self.pictures.borrow()[1].measure(orientation, for_size)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let progress = self.progress.get();
            for (slot, picture) in self.pictures.borrow().iter().enumerate() {
                let x = ((slot as f64 - 1.0 - progress) * f64::from(width)).round() as i32;
                picture.size_allocate(&gtk::Allocation::new(x, 0, width, height), -1);
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let progress = self.progress.get();
            let obj = self.obj();
            for (slot, picture) in self.pictures.borrow().iter().enumerate() {
                if (slot as f64 - 1.0 - progress).abs() < 1.0 {
                    obj.snapshot_child(picture, snapshot);
                }
            }
        }
    }

    impl SwipeableImpl for PhotoStrip {
        fn distance(&self) -> f64 {
            f64::from(self.obj().width())
        }

        fn snap_points(&self) -> Vec<f64> {
            let (prev, next) = self.neighbours.get();
            [prev.then_some(-1.0), Some(0.0), next.then_some(1.0)]
                .into_iter()
                .flatten()
                .collect()
        }

        fn progress(&self) -> f64 {
            self.progress.get()
        }

        fn cancel_progress(&self) -> f64 {
            0.0
        }
    }
}

glib::wrapper! {
    pub struct PhotoStrip(ObjectSubclass<imp::PhotoStrip>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, libadwaita::Swipeable;
}

impl PhotoStrip {
    pub(super) fn new() -> Self {
        glib::Object::builder()
            .property("hexpand", true)
            .property("vexpand", true)
            .build()
    }

    pub(super) fn picture(&self, slot: Slot) -> gtk::Picture {
        self.imp().pictures.borrow()[slot.index()].clone()
    }

    pub(super) fn pictures(&self) -> Vec<gtk::Picture> {
        self.imp().pictures.borrow().clone()
    }

    pub(super) fn asset(&self, slot: Slot) -> String {
        self.imp().assets.borrow()[slot.index()].clone()
    }

    pub(super) fn set_asset(&self, slot: Slot, asset_id: &str) {
        self.imp().assets.borrow_mut()[slot.index()] = asset_id.to_string();
    }

    /// Whether `picture` is still a neighbour showing `asset_id`, after any rotations.
    pub(super) fn neighbour_shows(&self, picture: &gtk::Picture, asset_id: &str) -> bool {
        let imp = self.imp();
        let pictures = imp.pictures.borrow();
        pictures
            .iter()
            .position(|candidate| candidate == picture)
            .is_some_and(|index| {
                index != Slot::Current.index() && imp.assets.borrow()[index] == asset_id
            })
    }

    pub(super) fn set_neighbours(&self, prev: bool, next: bool) {
        self.imp().neighbours.set((prev, next));
    }

    pub(super) fn tracker(&self) -> libadwaita::SwipeTracker {
        self.imp()
            .tracker
            .get()
            .expect("tracker is created in constructed")
            .clone()
    }

    pub(super) fn set_swipe_enabled(&self, enabled: bool) {
        self.tracker().set_enabled(enabled);
    }

    pub(super) fn progress(&self) -> f64 {
        self.imp().progress.get()
    }

    pub(super) fn set_progress(&self, progress: f64) {
        self.imp().progress.set(progress);
        self.queue_allocate();
        self.queue_draw();
    }

    /// The photo `step` away becomes current; progress shifts so nothing moves on screen.
    pub(super) fn rotate(&self, step: i32) {
        let imp = self.imp();
        let mut pictures = imp.pictures.borrow_mut();
        let mut assets = imp.assets.borrow_mut();
        if step > 0 {
            pictures.rotate_left(1);
            assets.rotate_left(1);
        } else {
            pictures.rotate_right(1);
            assets.rotate_right(1);
        }
        drop((pictures, assets));
        self.set_progress(self.progress() - f64::from(step));
        self.queue_resize();
    }

    pub(super) fn animate_to(&self, to: f64, velocity: f64, done: impl Fn() + 'static) {
        self.stop();
        let weak = self.downgrade();
        let target = libadwaita::CallbackAnimationTarget::new(move |value| {
            if let Some(strip) = weak.upgrade() {
                strip.set_progress(value);
            }
        });
        let spring = libadwaita::SpringAnimation::new(
            self,
            self.progress(),
            to,
            libadwaita::SpringParams::new(1.0, 0.5, 500.0),
            target,
        );
        spring.set_initial_velocity(velocity);
        spring.set_clamp(true);
        let weak = self.downgrade();
        spring.connect_done(move |_| {
            if let Some(strip) = weak.upgrade() {
                strip.imp().animation.take();
            }
            done();
        });
        *self.imp().animation.borrow_mut() = Some((spring.clone(), to));
        spring.play();
    }

    /// Halt any spring where it is; returns its target and current velocity.
    pub(super) fn stop(&self) -> Option<(f64, f64)> {
        let (spring, to) = self.imp().animation.take()?;
        let velocity = spring.velocity();
        spring.pause();
        Some((to, velocity))
    }
}
