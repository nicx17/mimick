use std::rc::Rc;

use gtk::prelude::*;

pub(super) struct TreePage {
    pub root: gtk::Box,
    pub toolbar: gtk::Box,
    pub list: gtk::ListBox,
    crumbs: gtk::Box,
    /// Swapped for the Folders path entry while it is open.
    pub(super) crumb_scroller: gtk::ScrolledWindow,
    empty: libadwaita::StatusPage,
}

pub(super) fn build_tree_page(empty_icon: &str) -> TreePage {
    let crumbs = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(2)
        .build();
    let crumb_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .child(&crumbs)
        .build();
    let toolbar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    toolbar.append(&crumb_scroller);
    let (scroller, list, empty) = build_list_area(empty_icon);
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    root.append(&toolbar);
    root.append(&scroller);
    TreePage {
        root,
        toolbar,
        list,
        crumbs,
        crumb_scroller,
        empty,
    }
}

fn build_list_area(
    empty_icon: &str,
) -> (gtk::ScrolledWindow, gtk::ListBox, libadwaita::StatusPage) {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .valign(gtk::Align::Start)
        .build();
    let empty = libadwaita::StatusPage::builder()
        .icon_name(empty_icon)
        .vexpand(true)
        .visible(false)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&list);
    content.append(&empty);
    let clamp = libadwaita::Clamp::builder()
        .maximum_size(820)
        .child(&content)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    (scroller, list, empty)
}

pub(super) fn show_empty_state(page: &TreePage, empty: Option<(&str, &str)>) {
    if let Some((title, description)) = empty {
        page.empty.set_title(title);
        page.empty.set_description(Some(description));
    }
    page.empty.set_visible(empty.is_some());
    page.list.set_visible(empty.is_none());
}

pub(super) fn render_breadcrumbs(
    page: &TreePage,
    root_label: &str,
    path: Option<&str>,
    on_click: Rc<dyn Fn(Option<String>)>,
) {
    let crumbs = &page.crumbs;
    while let Some(child) = crumbs.first_child() {
        crumbs.remove(&child);
    }
    let segments = segments(root_label, path);
    let last = segments.len() - 1;
    for (index, (label, target)) in segments.into_iter().enumerate() {
        if index > 0 {
            let separator = gtk::Label::builder()
                .label("/")
                .css_classes(["dim-label"])
                .build();
            crumbs.append(&separator);
        }
        let button = gtk::Button::builder()
            .label(&label)
            .css_classes(["flat"])
            .build();
        if index == last {
            button.add_css_class("heading");
        }
        let on_click = on_click.clone();
        button.connect_clicked(move |_| on_click(target.clone()));
        crumbs.append(&button);
    }
    let adjustment = page.crumb_scroller.hadjustment();
    glib::idle_add_local_once(move || adjustment.set_value(adjustment.upper()));
}

pub(super) fn parent_path(path: &str) -> Option<&str> {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
}

pub(super) fn leaf_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn segments(root_label: &str, path: Option<&str>) -> Vec<(String, Option<String>)> {
    let mut crumbs = vec![(root_label.to_string(), None)];
    let Some(path) = path else {
        return crumbs;
    };
    let ends = path
        .match_indices('/')
        .map(|(index, _)| index)
        .chain(std::iter::once(path.len()));
    for end in ends {
        let prefix = &path[..end];
        let name = leaf_name(prefix);
        if !name.is_empty() {
            crumbs.push((name.to_string(), Some(prefix.to_string())));
        }
    }
    crumbs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crumb(label: &str, target: Option<&str>) -> (String, Option<String>) {
        (label.to_string(), target.map(str::to_string))
    }

    #[test]
    fn segments_build_each_ancestor() {
        assert_eq!(segments("All Tags", None), [crumb("All Tags", None)]);
        assert_eq!(
            segments("All Tags", Some("Trips/Japan")),
            [
                crumb("All Tags", None),
                crumb("Trips", Some("Trips")),
                crumb("Japan", Some("Trips/Japan")),
            ]
        );
    }

    #[test]
    fn segments_keep_the_leading_slash_of_absolute_paths() {
        assert_eq!(
            segments("Folders", Some("/photos/2024")),
            [
                crumb("Folders", None),
                crumb("photos", Some("/photos")),
                crumb("2024", Some("/photos/2024")),
            ]
        );
    }

    #[test]
    fn parent_path_stops_at_the_root() {
        assert_eq!(parent_path("Trips/Japan"), Some("Trips"));
        assert_eq!(parent_path("Trips"), None);
        assert_eq!(parent_path("/photos/2024"), Some("/photos"));
        assert_eq!(parent_path("/photos"), None);
        assert_eq!(leaf_name("/photos/2024"), "2024");
    }
}
