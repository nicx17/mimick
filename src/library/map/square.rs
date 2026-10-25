//! A single-child container that is always as tall as it is wide.

use gtk::prelude::*;

mod imp {
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Default)]
    pub struct Square;

    #[glib::object_subclass]
    impl ObjectSubclass for Square {
        const NAME: &str = "MimickSquare";
        type Type = super::Square;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Square {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Square {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.obj().first_child() else {
                return (0, 0, -1, -1);
            };
            let (min, natural, _, _) = child.measure(orientation, -1);
            if orientation == gtk::Orientation::Vertical && for_size >= 0 {
                let side = for_size.max(min);
                return (side, side, -1, -1);
            }
            (min, natural, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

glib::wrapper! {
    pub struct Square(ObjectSubclass<imp::Square>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Square {
    pub fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let square: Self = glib::Object::new();
        child.set_parent(&square);
        square
    }
}
