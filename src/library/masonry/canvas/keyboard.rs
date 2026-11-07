use gtk::gdk::{Key, ModifierType};
use gtk::glib;
use gtk::graphene::Rect;
use gtk::gsk::RoundedRect;
use gtk::prelude::*;
use gtk::subclass::prelude::{ObjectSubclassExt, ObjectSubclassIsExt};

use super::interaction::{enable_select_mode_if_needed, toggle_selection};
use super::{CORNER_RADIUS, LaidRow, MasonryCanvas, accent_bg_color, first_row_at_or_after, imp};
use crate::library::context_menu::menu_key;
use crate::library::masonry::layout::{Step, locate, scroll_delta, step_from};

const FOCUS_RING_WIDTH: f32 = 3.0;

impl MasonryCanvas {
    pub(super) fn install_keyboard(&self) {
        self.set_focusable(true);
        let keys = gtk::EventControllerKey::new();
        let weak = self.downgrade();
        keys.connect_key_pressed(move |_, key, _, mods| {
            if weak.upgrade().is_some_and(|c| c.handle_key(key, mods)) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.add_controller(keys);

        let weak = self.downgrade();
        self.add_controller(menu_key(move || {
            if let Some(canvas) = weak.upgrade() {
                canvas.open_focused_menu();
            }
        }));

        let focus = gtk::EventControllerFocus::new();
        let weak = self.downgrade();
        focus.connect_enter(move |_| {
            if let Some(canvas) = weak.upgrade() {
                canvas.imp().focused.set(canvas.current_tile());
            }
        });
        self.add_controller(focus);
        self.connect_state_flags_changed(|canvas, _| canvas.queue_draw());
    }

    /// Scroll tile `pos` to the middle of the view and ring it, once a layout includes it.
    pub fn reveal_tile(&self, pos: u32) {
        let imp = self.imp();
        imp.focused.set(Some(pos));
        imp.pending_reveal.set(Some(pos));
        self.grab_focus();
        if let Some(window) = self.root().and_downcast::<gtk::Window>() {
            window.set_focus_visible(true);
        }
        self.try_reveal();
        self.queue_draw();
    }

    /// Finishes a pending `reveal_tile`; false while its row isn't laid out yet.
    pub(super) fn try_reveal(&self) -> bool {
        let imp = self.imp();
        let Some(pos) = imp.pending_reveal.get() else {
            return true;
        };
        let middle = {
            let rows = imp.rows.borrow();
            locate(&rows, pos).map(|(r, _)| rows[r].y + rows[r].h / 2.0)
        };
        let (Some(middle), Some(adj)) = (middle, imp.find_vadjustment()) else {
            return false;
        };
        imp.pending_reveal.set(None);
        let (top, height) = imp.viewport();
        adj.set_value(adj.value() + f64::from(middle - height / 2.0 - top));
        self.queue_draw();
        true
    }

    /// Remember a clicked tile so arrow keys continue from it.
    pub(super) fn focus_clicked(&self, pos: u32) {
        self.imp().focused.set(Some(pos));
        self.grab_focus();
    }

    fn handle_key(&self, key: Key, mods: ModifierType) -> bool {
        let held = ModifierType::CONTROL_MASK
            | ModifierType::ALT_MASK
            | ModifierType::SUPER_MASK
            | ModifierType::SHIFT_MASK;
        if mods.intersects(held) {
            return false;
        }
        match key {
            Key::Return | Key::KP_Enter | Key::ISO_Enter => self.activate_focused(),
            Key::space | Key::KP_Space => self.toggle_focused(),
            _ => arrow_step(key).is_some_and(|step| self.move_focus(step)),
        }
    }

    /// The focused tile while it's on screen, else the first tile at the top of the view.
    fn current_tile(&self) -> Option<u32> {
        let imp = self.imp();
        let rows = imp.rows.borrow();
        let (top, height) = imp.viewport();
        let on_screen = |pos| {
            locate(&rows, pos)
                .is_some_and(|(r, _)| rows[r].y < top + height && rows[r].y + rows[r].h > top)
        };
        imp.focused.get().filter(|&pos| on_screen(pos)).or_else(|| {
            let row = rows.get(first_row_at_or_after(&rows, top))?;
            row.items.first().map(|it| it.asset_index)
        })
    }

    fn move_focus(&self, step: Step) -> bool {
        let Some(current) = self.current_tile() else {
            return false;
        };
        let imp = self.imp();
        let continuing =
            imp.focused.get() == Some(current) || matches!(step, Step::First | Step::Last);
        let target = continuing
            .then(|| step_from(&imp.rows.borrow(), current, step))
            .flatten()
            .unwrap_or(current);
        self.focus_tile(target);
        true
    }

    fn focus_tile(&self, pos: u32) {
        let imp = self.imp();
        imp.focused.set(Some(pos));
        if let Some(window) = self.root().and_downcast::<gtk::Window>() {
            window.set_focus_visible(true);
        }
        let delta = {
            let rows = imp.rows.borrow();
            let (top, height) = imp.viewport();
            locate(&rows, pos).map_or(0.0, |(r, _)| {
                scroll_delta(top, height, rows[r].y, rows[r].h)
            })
        };
        if delta != 0.0
            && let Some(adj) = imp.find_vadjustment()
        {
            adj.set_value(adj.value() + f64::from(delta));
        }
        self.queue_draw();
    }

    fn activate_focused(&self) -> bool {
        let Some(pos) = self.current_tile() else {
            return false;
        };
        let imp = self.imp();
        self.focus_tile(pos);
        if imp.select_mode.get() {
            self.toggle_tile(pos);
        } else if let Some(handler) = imp.activate_handler.borrow().clone() {
            handler(pos);
        }
        true
    }

    fn toggle_focused(&self) -> bool {
        let Some(pos) = self.current_tile() else {
            return false;
        };
        self.focus_tile(pos);
        self.toggle_tile(pos);
        true
    }

    fn toggle_tile(&self, pos: u32) {
        let imp = self.imp();
        if let Some(sel) = imp.selection.get() {
            toggle_selection(sel, pos);
            enable_select_mode_if_needed(imp, pos);
            imp.last_selected.set(Some(pos));
        }
    }

    fn open_focused_menu(&self) {
        let Some(pos) = self.current_tile() else {
            return;
        };
        self.focus_tile(pos);
        let center = {
            let rows = self.imp().rows.borrow();
            locate(&rows, pos).map(|(r, i)| {
                let (row, it) = (&rows[r], &rows[r].items[i]);
                (f64::from(it.x + it.w / 2.0), f64::from(row.y + row.h / 2.0))
            })
        };
        let handler = self.imp().context_menu_handler.borrow().clone();
        if let (Some((x, y)), Some(handler)) = (center, handler)
            && let Some(open) = handler.borrow().as_ref()
        {
            open(pos, x, y);
        }
    }
}

impl imp::MasonryCanvas {
    pub(super) fn paint_focus_ring(&self, snapshot: &gtk::Snapshot, rows: &[LaidRow]) {
        if !self
            .obj()
            .state_flags()
            .contains(gtk::StateFlags::FOCUS_VISIBLE)
        {
            return;
        }
        let Some((r, i)) = self.focused.get().and_then(|pos| locate(rows, pos)) else {
            return;
        };
        let (row, it) = (&rows[r], &rows[r].items[i]);
        let rect = RoundedRect::from_rect(Rect::new(it.x, row.y, it.w, row.h), CORNER_RADIUS);
        snapshot.append_border(&rect, &[FOCUS_RING_WIDTH; 4], &[accent_bg_color(); 4]);
    }
}

fn arrow_step(key: Key) -> Option<Step> {
    match key {
        Key::Left | Key::KP_Left => Some(Step::Left),
        Key::Right | Key::KP_Right => Some(Step::Right),
        Key::Up | Key::KP_Up => Some(Step::Up),
        Key::Down | Key::KP_Down => Some(Step::Down),
        Key::Home | Key::KP_Home => Some(Step::First),
        Key::End | Key::KP_End => Some(Step::Last),
        _ => None,
    }
}

/// Where the focused index lands after `removed` items at `position` become `added`.
pub(super) fn shift_focus(focused: u32, position: u32, removed: u32, added: u32) -> Option<u32> {
    if focused < position {
        Some(focused)
    } else if focused < position + removed {
        None
    } else {
        Some(focused - removed + added)
    }
}

#[cfg(test)]
mod tests {
    use super::shift_focus;

    #[test]
    fn focus_follows_items_inserted_or_removed_before_it() {
        assert_eq!(shift_focus(5, 10, 2, 0), Some(5));
        assert_eq!(shift_focus(5, 0, 0, 3), Some(8));
        assert_eq!(shift_focus(5, 2, 2, 0), Some(3));
        assert_eq!(shift_focus(5, 4, 3, 0), None);
    }
}
