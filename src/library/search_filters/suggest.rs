//! A filter row offering the values Immich has for a field, such as camera makes or cities.

use gettextrs::gettext;
use std::cell::RefCell;

use gtk::prelude::*;
use libadwaita::prelude::*;

/// "Any" and then the values, with typing to search on long lists.
pub struct SuggestRow {
    pub row: libadwaita::ComboRow,
    values: RefCell<Vec<String>>,
}

impl SuggestRow {
    pub(super) fn new(title: &str) -> Self {
        // The value shows as the subtitle, which wraps: a long lens or city name at the end
        // of the row would make the filters panel wider than a phone.
        let row = libadwaita::ComboRow::builder()
            .title(title)
            .use_subtitle(true)
            .model(&gtk::StringList::new(&[&gettext("Any")]))
            .enable_search(true)
            .search_match_mode(gtk::StringFilterMatchMode::Substring)
            .expression(gtk::PropertyExpression::new(
                gtk::StringObject::static_type(),
                None::<gtk::Expression>,
                "string",
            ))
            .build();
        Self {
            row,
            values: RefCell::default(),
        }
    }

    pub fn value(&self) -> Option<String> {
        let index = self.row.selected().checked_sub(1)?;
        self.values.borrow().get(index as usize).cloned()
    }

    /// List `values`, keeping the chosen one when it's still there.
    pub fn set_values(&self, mut values: Vec<String>) {
        let current = self.value();
        values.sort_by_key(|value| value.to_lowercase());
        values.dedup();
        let any = gettext("Any");
        let model = gtk::StringList::new(&[any.as_str()]);
        for value in &values {
            model.append(value);
        }
        let selected = current
            .and_then(|current| values.iter().position(|value| *value == current))
            .map_or(0, |index| index as u32 + 1);
        self.values.replace(values);
        self.row.set_model(Some(&model));
        self.row.set_selected(selected);
    }

    pub fn clear(&self) {
        self.row.set_selected(0);
    }
}
