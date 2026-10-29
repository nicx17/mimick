//! Tags section of the lightbox details pane.
//!
//! Each tag is a chip: clicking its name searches for that tag, × removes it from
//! the asset. "Add Tag" opens a picker of existing tags that can also create a new
//! one (Immich creates any missing parents for nested `Parent/Child` tags).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::Tag;

use super::LibraryWindowUi;
use super::context_menu::show_error_toast;

/// Most tags listed in the picker at once; typing narrows the list.
const PICKER_LIMIT: usize = 8;

/// Window and search handler, set once by the lightbox through `bind`.
struct TagContext {
    ui: Rc<LibraryWindowUi>,
    on_search: Rc<dyn Fn(Tag)>,
}

/// "Tags" group (matching the EXIF groups) with an add button and wrapping tag pills.
/// Cheap to clone.
#[derive(Clone)]
pub(super) struct TagSection {
    pub root: libadwaita::PreferencesGroup,
    chips: gtk::FlowBox,
    empty_label: gtk::Label,
    add_btn: gtk::MenuButton,
    asset_id: Rc<RefCell<String>>,
    tags: Rc<RefCell<Vec<Tag>>>,
    context: Rc<RefCell<Option<TagContext>>>,
}

impl TagSection {
    pub(super) fn new() -> Self {
        let add_btn = gtk::MenuButton::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add tag")
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular"])
            .build();
        let root = libadwaita::PreferencesGroup::builder()
            .title("Tags")
            .header_suffix(&add_btn)
            .visible(false)
            .build();
        let chips = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .column_spacing(6)
            .row_spacing(6)
            .max_children_per_line(8)
            .homogeneous(false)
            .build();
        let empty_label = gtk::Label::builder()
            .label("No tags yet")
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        root.add(&chips);
        root.add(&empty_label);
        let section = Self {
            root,
            chips,
            empty_label,
            add_btn,
            asset_id: Rc::default(),
            tags: Rc::default(),
            context: Rc::default(),
        };
        section.add_btn.set_popover(Some(&section.build_picker()));
        section
    }

    /// Connect the section to the library window; `on_search` runs when a tag is clicked.
    pub(super) fn bind(&self, ui: Rc<LibraryWindowUi>, on_search: Rc<dyn Fn(Tag)>) {
        *self.context.borrow_mut() = Some(TagContext { ui, on_search });
    }

    /// Hide until the next asset's tags arrive (local-only assets never show it).
    pub(super) fn clear(&self) {
        self.root.set_visible(false);
        self.tags.borrow_mut().clear();
        self.rebuild_chips();
    }

    pub(super) fn show(&self, asset_id: &str, mut tags: Vec<Tag>) {
        tags.sort_by_key(|tag| tag.value.to_lowercase());
        *self.asset_id.borrow_mut() = asset_id.to_string();
        *self.tags.borrow_mut() = tags;
        self.rebuild_chips();
        self.root.set_visible(true);
    }

    fn ui(&self) -> Option<Rc<LibraryWindowUi>> {
        self.context.borrow().as_ref().map(|c| c.ui.clone())
    }

    fn rebuild_chips(&self) {
        self.chips.remove_all();
        let tags = self.tags.borrow();
        for tag in tags.iter() {
            let slot = gtk::FlowBoxChild::builder()
                .child(&self.chip(tag))
                .halign(gtk::Align::Start)
                .focusable(false)
                .css_classes(["mimick-tag-slot"])
                .build();
            self.chips.insert(&slot, -1);
        }
        self.chips.set_visible(!tags.is_empty());
        self.empty_label.set_visible(tags.is_empty());
    }

    /// One pill: color dot and label (click to search), plus a remove button.
    fn chip(&self, tag: &Tag) -> gtk::Box {
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .build();
        content.append(&color_dot(tag.color.as_deref(), 10));
        content.append(
            &gtk::Label::builder()
                .use_markup(true)
                .label(tag_markup(&tag.value))
                .build(),
        );
        let name_btn = gtk::Button::builder()
            .child(&content)
            .tooltip_text(format!("Show photos tagged “{}”", tag.value))
            .css_classes(["flat", "mimick-tag-name"])
            .build();
        let section = self.clone();
        let clicked = tag.clone();
        name_btn.connect_clicked(move |_| section.search(clicked.clone()));
        let remove_btn = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text(format!("Remove “{}”", tag.value))
            .css_classes(["flat", "mimick-tag-remove"])
            .build();
        let section = self.clone();
        let removed = tag.clone();
        remove_btn.connect_clicked(move |_| section.remove_tag(removed.clone()));
        let chip = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            // The FlowBox hands spare row width to its children; keep the pill tight.
            .halign(gtk::Align::Start)
            .css_classes(["mimick-tag-chip"])
            .build();
        chip.append(&name_btn);
        chip.append(&remove_btn);
        chip
    }

    fn search(&self, tag: Tag) {
        let on_search = self.context.borrow().as_ref().map(|c| c.on_search.clone());
        if let Some(on_search) = on_search {
            on_search(tag);
        }
    }

    fn remove_tag(&self, tag: Tag) {
        let Some(ui) = self.ui() else {
            return;
        };
        let section = self.clone();
        let asset_id = self.asset_id.borrow().clone();
        glib::MainContext::default().spawn_local(async move {
            match ui.ctx.api_client.untag_asset(&tag.id, &asset_id).await {
                Ok(()) if *section.asset_id.borrow() == asset_id => {
                    section.tags.borrow_mut().retain(|t| t.id != tag.id);
                    section.rebuild_chips();
                }
                Ok(()) => {}
                Err(err) => show_error_toast(&ui, "Couldn't remove the tag", &err),
            }
        });
    }

    /// Tag the current asset, creating the tag first when `existing` is `None`.
    fn add_tag(&self, value: String, existing: Option<Tag>) {
        let Some(ui) = self.ui() else {
            return;
        };
        let section = self.clone();
        let asset_id = self.asset_id.borrow().clone();
        glib::MainContext::default().spawn_local(async move {
            let api = &ui.ctx.api_client;
            let result = match existing {
                Some(tag) => Ok(tag),
                None => api.create_tag(&value).await,
            };
            let result = match result {
                Ok(tag) => api.tag_asset(&tag.id, &asset_id).await.map(|()| tag),
                Err(err) => Err(err),
            };
            match result {
                Ok(tag) if *section.asset_id.borrow() == asset_id => {
                    let mut tags = section.tags.borrow().clone();
                    tags.retain(|t| t.id != tag.id);
                    tags.push(tag);
                    section.show(&asset_id, tags);
                }
                Ok(_) => {}
                Err(err) => show_error_toast(&ui, "Couldn't add the tag", &err),
            }
        });
    }

    /// Popover with a search entry over the server's tags, refreshed each time it opens.
    fn build_picker(&self) -> gtk::Popover {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Find or create a tag")
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .width_request(240)
            .build();
        content.append(&entry);
        content.append(&list);
        let popover = gtk::Popover::builder().child(&content).build();
        let all_tags: Rc<RefCell<Vec<Tag>>> = Rc::default();

        let picker = Picker {
            section: self.clone(),
            popover: popover.clone(),
            entry: entry.clone(),
            list,
            all_tags,
        };
        let on_show = picker.clone();
        popover.connect_show(move |_| on_show.open());
        let on_change = picker.clone();
        entry.connect_search_changed(move |_| on_change.refill());
        entry.connect_activate(move |_| picker.choose_typed());
        popover
    }
}

/// State behind the "Add Tag" popover.
#[derive(Clone)]
struct Picker {
    section: TagSection,
    popover: gtk::Popover,
    entry: gtk::SearchEntry,
    list: gtk::ListBox,
    all_tags: Rc<RefCell<Vec<Tag>>>,
}

impl Picker {
    fn open(&self) {
        self.entry.set_text("");
        self.entry.grab_focus();
        self.refill();
        let Some(ui) = self.section.ui() else {
            return;
        };
        let picker = self.clone();
        glib::MainContext::default().spawn_local(async move {
            match ui.ctx.api_client.fetch_tags().await {
                Ok(tags) => {
                    *picker.all_tags.borrow_mut() = tags;
                    picker.refill();
                }
                Err(err) => log::warn!("Could not load tags: {}", err),
            }
        });
    }

    fn refill(&self) {
        self.list.remove_all();
        let query = self.entry.text().trim().to_string();
        let applied = self.section.tags.borrow().clone();
        let all_tags = self.all_tags.borrow().clone();
        for tag in picker_matches(&all_tags, &applied, &query) {
            let label = tag.value.clone();
            self.append_row(&label, move |picker| {
                picker.choose(tag.value.clone(), Some(tag.clone()))
            });
        }
        if !query.is_empty()
            && !all_tags
                .iter()
                .any(|t| t.value.eq_ignore_ascii_case(&query))
        {
            self.append_row(&format!("Create “{query}”"), move |picker| {
                picker.choose(query.clone(), None)
            });
        }
    }

    fn append_row(&self, title: &str, on_activate: impl Fn(&Picker) + 'static) {
        let row = libadwaita::ActionRow::builder()
            .title(title)
            .activatable(true)
            .build();
        let picker = self.clone();
        row.connect_activated(move |_| on_activate(&picker));
        self.list.append(&row);
    }

    /// Enter applies an exact (case-insensitive) match, otherwise creates the typed tag.
    fn choose_typed(&self) {
        let query = self.entry.text().trim().to_string();
        if query.is_empty() {
            return;
        }
        let existing = self
            .all_tags
            .borrow()
            .iter()
            .find(|t| t.value.eq_ignore_ascii_case(&query))
            .cloned();
        self.choose(query, existing);
    }

    fn choose(&self, value: String, existing: Option<Tag>) {
        self.popover.popdown();
        self.section.add_tag(value, existing);
    }
}

/// Tag label with the parent path dimmed and the leaf bold: "Trips/<b>2024</b>".
fn tag_markup(value: &str) -> String {
    let (parent, leaf) = split_tag_path(value);
    let leaf = format!("<b>{}</b>", glib::markup_escape_text(leaf));
    match parent {
        Some(parent) => format!(
            "<span alpha=\"70%\">{}/</span>{}",
            glib::markup_escape_text(parent),
            leaf
        ),
        None => leaf,
    }
}

/// `("Trips", "2024")` for `Trips/2024`; no parent for a top-level tag.
fn split_tag_path(value: &str) -> (Option<&str>, &str) {
    match value.rsplit_once('/') {
        Some((parent, leaf)) if !parent.is_empty() => (Some(parent), leaf),
        _ => (None, value),
    }
}

/// Parse an Immich tag color (`#rrggbb`); tags without one use the theme accent.
fn tag_color(color: Option<&str>) -> Option<gdk4::RGBA> {
    color.and_then(|c| gdk4::RGBA::parse(c).ok())
}

/// Small filled circle in the tag's color.
pub(super) fn color_dot(color: Option<&str>, size: i32) -> gtk::DrawingArea {
    let rgba = tag_color(color)
        .unwrap_or_else(|| libadwaita::StyleManager::default().accent_color().to_rgba());
    let dot = gtk::DrawingArea::builder()
        .content_width(size)
        .content_height(size)
        .valign(gtk::Align::Center)
        .build();
    dot.set_draw_func(move |_, cr, width, height| {
        let radius = f64::from(width.min(height)) / 2.0;
        cr.arc(radius, radius, radius, 0.0, std::f64::consts::TAU);
        cr.set_source_rgba(
            rgba.red().into(),
            rgba.green().into(),
            rgba.blue().into(),
            rgba.alpha().into(),
        );
        let _ = cr.fill();
    });
    dot
}

/// Server tags matching `query` (case-insensitive substring) that aren't applied yet.
fn picker_matches(all_tags: &[Tag], applied: &[Tag], query: &str) -> Vec<Tag> {
    let query = query.to_lowercase();
    all_tags
        .iter()
        .filter(|tag| !applied.iter().any(|a| a.id == tag.id))
        .filter(|tag| tag.value.to_lowercase().contains(&query))
        .take(PICKER_LIMIT)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(id: &str, value: &str) -> Tag {
        Tag {
            id: id.into(),
            name: value.rsplit('/').next().unwrap_or(value).into(),
            value: value.into(),
            color: None,
        }
    }

    #[test]
    fn picker_hides_applied_tags_and_filters_case_insensitively() {
        let all = [
            tag("1", "Trips/2024"),
            tag("2", "Family"),
            tag("3", "trips"),
        ];
        let applied = [tag("3", "trips")];
        let matches = picker_matches(&all, &applied, "TRIP");
        assert_eq!(matches, vec![tag("1", "Trips/2024")]);
    }

    #[test]
    fn split_tag_path_separates_parent_and_leaf() {
        assert_eq!(split_tag_path("Trips/2024"), (Some("Trips"), "2024"));
        assert_eq!(split_tag_path("A/B/C"), (Some("A/B"), "C"));
        assert_eq!(split_tag_path("Family"), (None, "Family"));
        assert_eq!(split_tag_path("/odd"), (None, "/odd"));
    }

    #[test]
    fn tag_markup_escapes_and_bolds_the_leaf() {
        assert_eq!(tag_markup("Family"), "<b>Family</b>");
        assert_eq!(
            tag_markup("R&D/Q1"),
            "<span alpha=\"70%\">R&amp;D/</span><b>Q1</b>"
        );
    }

    #[test]
    fn tag_color_parses_hex_and_ignores_invalid() {
        let red = tag_color(Some("#ff0000")).unwrap();
        assert_eq!(red.red(), 1.0);
        assert!(tag_color(Some("not a color")).is_none());
        assert!(tag_color(None).is_none());
    }

    #[test]
    fn picker_lists_at_most_the_limit() {
        let all: Vec<Tag> = (0..20)
            .map(|i| tag(&i.to_string(), &format!("t{i}")))
            .collect();
        assert_eq!(picker_matches(&all, &[], "").len(), PICKER_LIMIT);
    }
}
