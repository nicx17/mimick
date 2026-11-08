//! The calendar's grid: a rounded square per day in the accent colour, month names above
//! and weekday names beside. It fills the width it's given, up to a size, and stays small
//! enough on a phone to scroll sideways.

use chrono::{Duration, NaiveDate};
use gtk::gdk;
use gtk::glib;
use gtk::graphene::{Point, Rect, Size};
use gtk::gsk::RoundedRect;
use gtk::pango;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use super::layout::{GAP, MIN_CELL, cell_of, columns, index_at, level, month_starts, step_for};
use crate::i18n::format_day;

/// Accent strength for levels 1 to 4; level 0 is a faint square in the text colour.
const LEVEL_ALPHAS: [f32; 4] = [0.35, 0.55, 0.78, 1.0];
const EMPTY_ALPHA: f32 = 0.08;
const LABEL_ALPHA: f32 = 0.6;
const LABEL_SCALE: f64 = 0.8;
/// Space between the labels and the squares.
const LABEL_PAD: f32 = 6.0;
type OnSelect = Box<dyn Fn(Option<(NaiveDate, u32)>)>;

mod imp {
    use std::cell::{Cell, RefCell};

    use chrono::NaiveDate;
    use gtk::glib;
    use gtk::subclass::prelude::*;

    use super::OnSelect;

    #[derive(Default)]
    pub struct HeatmapGrid {
        pub days: RefCell<Vec<(NaiveDate, u32)>>,
        pub selected: Cell<Option<usize>>,
        pub on_select: RefCell<Option<OnSelect>>,
        pub on_activate: RefCell<Option<Box<dyn Fn()>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HeatmapGrid {
        const NAME: &str = "MimickHeatmapGrid";
        type Type = super::HeatmapGrid;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for HeatmapGrid {}

    impl WidgetImpl for HeatmapGrid {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let (min, natural) = self.obj().measure_grid(orientation, for_size);
            (min, natural, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            self.obj().fit_height(width);
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct HeatmapGrid(ObjectSubclass<imp::HeatmapGrid>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// Where the squares start and how far apart they are, for one width.
struct Geometry {
    left: f32,
    top: f32,
    step: f32,
}

impl Geometry {
    fn cell(&self, (col, row): (usize, usize)) -> Rect {
        let size = self.step - GAP;
        Rect::new(
            self.left + col as f32 * self.step,
            self.top + row as f32 * self.step,
            size,
            size,
        )
    }
}

impl HeatmapGrid {
    pub fn new() -> Self {
        let grid: Self = glib::Object::builder()
            .property("focusable", true)
            .property("has-tooltip", true)
            .property("hexpand", true)
            .build();
        grid.update_property(&[gtk::accessible::Property::Label(&gettextrs::gettext(
            "Photos per day; use the arrow keys to pick a day",
        ))]);
        grid.connect_input();
        let style = libadwaita::StyleManager::default();
        let weak = grid.downgrade();
        style.connect_accent_color_rgba_notify(move |_| {
            if let Some(grid) = weak.upgrade() {
                grid.queue_draw();
            }
        });
        grid
    }

    pub fn set_days(&self, days: Vec<(NaiveDate, u32)>) {
        self.imp().days.replace(days);
        self.imp().selected.set(None);
        self.queue_resize();
    }

    pub fn selected_day(&self) -> Option<(NaiveDate, u32)> {
        let index = self.imp().selected.get()?;
        self.imp().days.borrow().get(index).copied()
    }

    pub fn connect_selected(&self, f: impl Fn(Option<(NaiveDate, u32)>) + 'static) {
        self.imp().on_select.replace(Some(Box::new(f)));
    }

    pub fn connect_activated(&self, f: impl Fn() + 'static) {
        self.imp().on_activate.replace(Some(Box::new(f)));
    }

    fn select(&self, index: Option<usize>) {
        self.imp().selected.set(index);
        self.queue_draw();
        let day = self.selected_day();
        if let Some(on_select) = self.imp().on_select.borrow().as_ref() {
            on_select(day);
        }
    }

    fn activate_selected(&self) {
        if let Some(on_activate) = self.imp().on_activate.borrow().as_ref() {
            on_activate();
        }
    }

    fn first_and_count(&self) -> Option<(NaiveDate, usize)> {
        let days = self.imp().days.borrow();
        Some((days.first()?.0, days.len()))
    }

    fn column_count(&self) -> usize {
        self.first_and_count()
            .map_or(0, |(first, count)| columns(first, count))
    }

    /// A small, dimmed label in the widget's font.
    fn label(&self, text: &str) -> pango::Layout {
        let layout = self.create_pango_layout(Some(text));
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrFloat::new_scale(LABEL_SCALE));
        layout.set_attributes(Some(&attrs));
        layout
    }

    /// The width of the weekday column and the height of the month row.
    fn label_space(&self) -> (f32, f32) {
        let widest = weekday_names()
            .iter()
            .map(|name| self.label(name).pixel_size().0)
            .max()
            .unwrap_or(0);
        let height = self.label("0").pixel_size().1;
        (widest as f32 + LABEL_PAD, height as f32 + LABEL_PAD / 2.0)
    }

    fn geometry(&self, width: f32) -> Geometry {
        let (gutter, header) = self.label_space();
        let cols = self.column_count().max(1);
        let step = step_for(width - gutter, cols);
        let used = gutter + step * cols as f32;
        Geometry {
            left: ((width - used) / 2.0).max(0.0) + gutter,
            top: header,
            step,
        }
    }

    /// The smallest squares set the minimum width; the height follows the width given.
    fn measure_grid(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32) {
        let (gutter, header) = self.label_space();
        let min_width = gutter + self.column_count().max(1) as f32 * (MIN_CELL + GAP);
        match orientation {
            gtk::Orientation::Horizontal => (min_width.ceil() as i32, min_width.ceil() as i32),
            _ => {
                let width = if for_size > 0 {
                    for_size as f32
                } else {
                    min_width
                };
                let step = self.geometry(width).step;
                let height = (header + 7.0 * step - GAP).ceil() as i32;
                (height, height)
            }
        }
    }

    /// A sideways scroller measures the grid's height as if it had its minimum width, so
    /// wide windows cut off the lower rows. Ask for the height of the width it really got.
    fn fit_height(&self, width: i32) {
        let needed = self.measure_grid(gtk::Orientation::Vertical, width).0;
        if needed == self.height_request() {
            return;
        }
        // Resizing from inside an allocation would be ignored until the next one.
        let grid = self.downgrade();
        glib::idle_add_local_once(move || {
            if let Some(grid) = grid.upgrade() {
                grid.set_height_request(needed);
            }
        });
    }

    fn index_at_point(&self, x: f64, y: f64) -> Option<usize> {
        let (first, count) = self.first_and_count()?;
        let geo = self.geometry(self.width() as f32);
        let col = (x as f32 - geo.left) / geo.step;
        let row = (y as f32 - geo.top) / geo.step;
        if col < 0.0 || row < 0.0 {
            return None;
        }
        index_at(first, count, (col as usize, row as usize))
    }

    fn connect_input(&self) {
        let click = gtk::GestureClick::new();
        let weak = self.downgrade();
        click.connect_released(move |_, _, x, y| {
            if let Some(grid) = weak.upgrade() {
                grid.grab_focus();
                if let Some(index) = grid.index_at_point(x, y) {
                    grid.select(Some(index));
                }
            }
        });
        self.add_controller(click);
        let keys = gtk::EventControllerKey::new();
        let weak = self.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            let handled = weak.upgrade().is_some_and(|grid| grid.handle_key(key));
            if handled {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.add_controller(keys);
        self.connect_query_tooltip(|grid, x, y, _, tooltip| {
            let Some(index) = grid.index_at_point(f64::from(x), f64::from(y)) else {
                return false;
            };
            let day = grid.imp().days.borrow().get(index).copied();
            let Some((day, count)) = day else {
                return false;
            };
            tooltip.set_text(Some(&super::day_label(day, count)));
            true
        });
    }

    /// Left and Right move a week, Up and Down a day; Enter and Space open the day.
    fn handle_key(&self, key: gdk::Key) -> bool {
        let step: i64 = match key {
            gdk::Key::Left => -7,
            gdk::Key::Right => 7,
            gdk::Key::Up => -1,
            gdk::Key::Down => 1,
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space => {
                self.activate_selected();
                return true;
            }
            _ => return false,
        };
        let count = self.imp().days.borrow().len() as i64;
        if count == 0 {
            return false;
        }
        let start = self.imp().selected.get().map_or(count - 1, |i| i as i64);
        let moved = (start + step).clamp(0, count - 1);
        self.select(usize::try_from(moved).ok());
        true
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let Some((first, _)) = self.first_and_count() else {
            return;
        };
        let geo = self.geometry(self.width() as f32);
        let text = self.color();
        self.draw_squares(snapshot, &geo, first, &text);
        self.draw_months(snapshot, &geo, first, &text);
        self.draw_weekdays(snapshot, &geo, &text);
        if let Some(index) = self.imp().selected.get() {
            let ring = if self.has_focus() {
                libadwaita::StyleManager::default().accent_color_rgba()
            } else {
                text
            };
            outline(snapshot, &geo.cell(cell_of(first, index)), &ring);
        }
    }

    fn draw_squares(
        &self,
        snapshot: &gtk::Snapshot,
        geo: &Geometry,
        first: NaiveDate,
        text: &gdk::RGBA,
    ) {
        let accent = libadwaita::StyleManager::default().accent_color_rgba();
        let days = self.imp().days.borrow();
        let max = days.iter().map(|(_, count)| *count).max().unwrap_or(0);
        let radius = ((geo.step - GAP) * 0.25).max(2.0);
        for (index, (_, count)) in days.iter().enumerate() {
            let color = match level(*count, max) {
                0 => with_alpha(text, EMPTY_ALPHA),
                n => with_alpha(&accent, LEVEL_ALPHAS[n - 1]),
            };
            rounded(snapshot, &geo.cell(cell_of(first, index)), radius, &color);
        }
    }

    /// Month names above the week where each month begins, skipping ones that would collide.
    fn draw_months(
        &self,
        snapshot: &gtk::Snapshot,
        geo: &Geometry,
        first: NaiveDate,
        text: &gdk::RGBA,
    ) {
        let count = self.imp().days.borrow().len();
        let widest = self.label(&format_day(first, "%b")).pixel_size().0 as f32 + LABEL_PAD;
        let min_gap = (widest / geo.step).ceil() as usize;
        let color = with_alpha(text, LABEL_ALPHA);
        for (col, day) in month_starts(first, count, min_gap.max(1)) {
            let layout = self.label(&format_day(day, "%b"));
            let at = Point::new(geo.left + col as f32 * geo.step, 0.0);
            draw_layout(snapshot, &layout, at, &color);
        }
    }

    /// A name beside every row; every other row when large text makes them taller than a row.
    fn draw_weekdays(&self, snapshot: &gtk::Snapshot, geo: &Geometry, text: &gdk::RGBA) {
        let color = with_alpha(text, LABEL_ALPHA);
        let label_height = self.label("0").pixel_size().1 as f32;
        let every = if label_height > geo.step { 2 } else { 1 };
        for (row, name) in weekday_names().iter().enumerate().step_by(every) {
            let layout = self.label(name);
            let (width, height) = layout.pixel_size();
            let cell = geo.cell((0, row));
            let at = Point::new(
                geo.left - LABEL_PAD - width as f32,
                cell.y() + (cell.height() - height as f32) / 2.0,
            );
            draw_layout(snapshot, &layout, at, &color);
        }
    }
}

impl Default for HeatmapGrid {
    fn default() -> Self {
        Self::new()
    }
}

/// Monday to Sunday in the user's language, short, one per row.
fn weekday_names() -> [String; 7] {
    // 1 January 2024 was a Monday.
    let monday = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap_or_default();
    std::array::from_fn(|row| format_day(monday + Duration::days(row as i64), "%a"))
}

fn with_alpha(color: &gdk::RGBA, alpha: f32) -> gdk::RGBA {
    gdk::RGBA::new(color.red(), color.green(), color.blue(), alpha)
}

fn rounded(snapshot: &gtk::Snapshot, rect: &Rect, radius: f32, color: &gdk::RGBA) {
    let corner = Size::new(radius, radius);
    snapshot.push_rounded_clip(&RoundedRect::new(*rect, corner, corner, corner, corner));
    snapshot.append_color(color, rect);
    snapshot.pop();
}

/// A 2 px ring just outside a square, marking the picked day.
fn outline(snapshot: &gtk::Snapshot, rect: &Rect, color: &gdk::RGBA) {
    let ring = rect.inset_r(-2.0, -2.0);
    let corner = Size::new(4.0, 4.0);
    let shape = RoundedRect::new(ring, corner, corner, corner, corner);
    snapshot.append_border(&shape, &[2.0; 4], &[*color; 4]);
}

fn draw_layout(snapshot: &gtk::Snapshot, layout: &pango::Layout, at: Point, color: &gdk::RGBA) {
    snapshot.save();
    snapshot.translate(&at);
    snapshot.append_layout(layout, color);
    snapshot.restore();
}
