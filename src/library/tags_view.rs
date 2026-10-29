use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::Tag;

use super::LibraryWindowUi;
use super::asset_tags::color_dot;
use super::context_menu::{show_alert_dialog, show_error_toast};
use super::dialogs::{TextPrompt, confirm, popover_menu, prompt_text};
use super::state::{LibraryPage, LibrarySource, Location};
use super::tree_page::{
    TreePage, build_tree_page, parent_path, render_breadcrumbs, show_empty_state,
};

const PRESET_COLORS: [&str; 10] = [
    "#e11d48", "#f97316", "#eab308", "#22c55e", "#14b8a6", "#0ea5e9", "#6366f1", "#a855f7",
    "#ec4899", "#64748b",
];

pub(super) struct TagsView {
    pub page: TreePage,
    new_tag: gtk::Button,
    tags: RefCell<Vec<Tag>>,
    path: RefCell<Option<String>>,
}

pub(super) fn build_tags_view() -> TagsView {
    let page = build_tree_page("mimick-tag-symbolic");
    let new_tag = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("New tag")
        .build();
    page.toolbar.append(&new_tag);
    TagsView {
        page,
        new_tag,
        tags: RefCell::default(),
        path: RefCell::default(),
    }
}

pub(super) fn connect_tags_view(ui: Rc<LibraryWindowUi>) {
    let ui_new = ui.clone();
    ui.tags
        .new_tag
        .connect_clicked(move |_| create_tag(&ui_new));
}

pub(super) fn show(ui: &Rc<LibraryWindowUi>, path: Option<String>) {
    *ui.tags.path.borrow_mut() = path;
    ui.content_stack.set_visible_child_name("tags");
    render(ui);
    reload(ui.clone());
}

fn reload(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.fetch_tags().await {
            Ok(tags) => {
                *ui.tags.tags.borrow_mut() = tags;
                render(&ui);
            }
            Err(err) => show_alert_dialog(
                &ui,
                "Could Not Load Tags",
                &format!("{err}\n\nYour API key needs the 'tag.read' permission."),
            ),
        }
    });
}

fn open_path(ui: &Rc<LibraryWindowUi>, path: Option<String>) {
    super::navigation::navigate(ui, Location::Page(LibraryPage::Tags { path }));
}

fn render(ui: &Rc<LibraryWindowUi>) {
    let view = &ui.tags;
    let tags = view.tags.borrow().clone();
    let path = view
        .path
        .borrow()
        .clone()
        .filter(|path| tags.iter().any(|tag| &tag.value == path));
    *view.path.borrow_mut() = path.clone();
    let ui_crumb = ui.clone();
    render_breadcrumbs(
        &view.page,
        "All Tags",
        path.as_deref(),
        Rc::new(move |target| open_path(&ui_crumb, target)),
    );

    view.page.list.remove_all();
    let children = children_of(&tags, path.as_deref());
    for tag in &children {
        view.page
            .list
            .append(&tag_row(ui, tag, child_count(&tags, &tag.value)));
    }
    let empty = match &path {
        None => (
            "No Tags",
            "Create a tag with the + button, or tag photos from the lightbox details pane.",
        ),
        Some(_) => ("No Tags Inside", "Use the + button to add one here."),
    };
    show_empty_state(&view.page, children.is_empty().then_some(empty));
}

fn tag_row(ui: &Rc<LibraryWindowUi>, tag: &Tag, children: usize) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .title(&tag.name)
        .use_markup(false)
        .title_lines(1)
        .activatable(true)
        .tooltip_text(format!("Show photos tagged {}", tag.value))
        .build();
    if children > 0 {
        row.set_subtitle(&match children {
            1 => "1 tag inside".to_string(),
            n => format!("{n} tags inside"),
        });
    }
    row.add_prefix(&color_dot(tag.color.as_deref(), 12));
    let menu = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("More")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    popover_menu(&menu, tag_menu_items(ui, tag));
    row.add_suffix(&menu);
    if children > 0 {
        let browse = gtk::Button::builder()
            .icon_name("go-next-symbolic")
            .tooltip_text(format!("Open {}", tag.name))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let (ui_browse, value) = (ui.clone(), tag.value.clone());
        browse.connect_clicked(move |_| open_path(&ui_browse, Some(value.clone())));
        row.add_suffix(&browse);
    }
    let (ui_open, id, value) = (ui.clone(), tag.id.clone(), tag.value.clone());
    row.connect_activated(move |_| {
        super::navigation::navigate_source(
            &ui_open,
            LibrarySource::Tag {
                id: id.clone(),
                name: value.clone(),
            },
        );
    });
    row
}

pub(super) fn tag_menu_items(ui: &Rc<LibraryWindowUi>, tag: &Tag) -> Vec<(String, Box<dyn Fn()>)> {
    let (ui_rename, t_rename) = (ui.clone(), tag.clone());
    let (ui_color, t_color) = (ui.clone(), tag.clone());
    let (ui_delete, t_delete) = (ui.clone(), tag.clone());
    vec![
        (
            "Rename…".to_string(),
            Box::new(move || rename_tag(&ui_rename, &t_rename)),
        ),
        (
            "Change Color…".to_string(),
            Box::new(move || pick_color(&ui_color, &t_color)),
        ),
        (
            "Delete…".to_string(),
            Box::new(move || delete_tag(&ui_delete, &t_delete)),
        ),
    ]
}

fn run(
    ui: &Rc<LibraryWindowUi>,
    heading: &'static str,
    task: impl Future<Output = Result<(), String>> + 'static,
) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match task.await {
            Ok(()) => {
                super::source_header::refresh(&ui);
                reload(ui);
            }
            Err(err) => show_alert_dialog(&ui, heading, &err),
        }
    });
}

fn create_tag(ui: &Rc<LibraryWindowUi>) {
    let parent = ui.tags.path.borrow().clone();
    let body = match &parent {
        Some(parent) => format!("The tag is created inside {parent}."),
        None => "Use “Parent/Child” to create a tag inside another.".to_string(),
    };
    let ui_create = ui.clone();
    prompt_text(
        ui,
        TextPrompt {
            heading: "New Tag",
            body: &body,
            initial: "",
            placeholder: "Tag name",
            accept: "Create",
        },
        move |name| {
            let value = match &parent {
                Some(parent) => format!("{parent}/{name}"),
                None => name,
            };
            let api = ui_create.ctx.api_client.clone();
            run(&ui_create, "Could Not Create Tag", async move {
                api.create_tag(&value).await.map(|_| ())
            });
        },
    );
}

fn rename_tag(ui: &Rc<LibraryWindowUi>, tag: &Tag) {
    let (ui_rename, tag_id) = (ui.clone(), tag.id.clone());
    prompt_text(
        ui,
        TextPrompt {
            heading: "Rename Tag",
            body: "Photos keep the tag under its new name.",
            initial: &tag.name,
            placeholder: "Tag name",
            accept: "Rename",
        },
        move |name| {
            if name.contains('/') {
                show_error_toast(
                    &ui_rename,
                    "Couldn't rename the tag",
                    "Tag names can't contain “/”.",
                );
                return;
            }
            let (api, tag_id) = (ui_rename.ctx.api_client.clone(), tag_id.clone());
            run(&ui_rename, "Could Not Rename Tag", async move {
                api.rename_tag(&tag_id, &name).await
            });
        },
    );
}

fn delete_tag(ui: &Rc<LibraryWindowUi>, tag: &Tag) {
    let (ui_delete, tag_id) = (ui.clone(), tag.id.clone());
    confirm(
        ui,
        &format!("Delete “{}”?", tag.name),
        "The tag is removed from every photo. This can't be undone.",
        "Delete",
        move || {
            let (api, tag_id) = (ui_delete.ctx.api_client.clone(), tag_id.clone());
            let ui_after = ui_delete.clone();
            run(&ui_delete, "Could Not Delete Tag", async move {
                api.delete_tag(&tag_id).await?;
                let viewing = matches!(
                    super::navigation::current(&ui_after),
                    Location::Source(LibrarySource::Tag { id, .. }) if id == tag_id
                );
                if viewing {
                    super::navigation::go_back(&ui_after);
                }
                Ok(())
            });
        },
    );
}

type ChosenColor = Rc<RefCell<String>>;

fn pick_color(ui: &Rc<LibraryWindowUi>, tag: &Tag) {
    let chosen: ChosenColor = Rc::new(RefCell::new(tag.color.clone().unwrap_or_default()));
    let (swatches, buttons) = color_swatches(&chosen);
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    body.append(&swatches);
    body.append(&custom_color_row(tag, &chosen, buttons));
    let dialog = libadwaita::AlertDialog::builder()
        .heading(format!("Color for “{}”", tag.name))
        .extra_child(&body)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("save", "Save");
    dialog.set_response_appearance("save", libadwaita::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");
    let (ui_save, tag_id) = (ui.clone(), tag.id.clone());
    dialog.connect_response(None, move |_, response| {
        let color = chosen.borrow().clone();
        if response != "save" || color.is_empty() {
            return;
        }
        let (api, tag_id) = (ui_save.ctx.api_client.clone(), tag_id.clone());
        run(&ui_save, "Could Not Change Color", async move {
            api.set_tag_color(&tag_id, &color).await
        });
    });
    dialog.present(Some(&ui.window));
}

fn color_swatches(chosen: &ChosenColor) -> (gtk::FlowBox, Rc<Vec<gtk::ToggleButton>>) {
    let swatches = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(5)
        .max_children_per_line(5)
        .halign(gtk::Align::Center)
        .build();
    let buttons: Rc<Vec<gtk::ToggleButton>> = Rc::new(
        PRESET_COLORS
            .iter()
            .map(|color| {
                gtk::ToggleButton::builder()
                    .child(&color_dot(Some(color), 22))
                    .tooltip_text(*color)
                    .css_classes(["flat", "circular"])
                    .active(chosen.borrow().eq_ignore_ascii_case(color))
                    .build()
            })
            .collect(),
    );
    for (swatch, color) in buttons.iter().zip(PRESET_COLORS) {
        let (chosen, buttons_for_toggle) = (chosen.clone(), Rc::downgrade(&buttons));
        swatch.connect_toggled(move |swatch| {
            if !swatch.is_active() {
                return;
            }
            *chosen.borrow_mut() = color.to_string();
            if let Some(buttons) = buttons_for_toggle.upgrade() {
                for other in buttons.iter().filter(|other| *other != swatch) {
                    other.set_active(false);
                }
            }
        });
        swatches.append(swatch);
    }
    (swatches, buttons)
}

fn custom_color_row(
    tag: &Tag,
    chosen: &ChosenColor,
    swatches: Rc<Vec<gtk::ToggleButton>>,
) -> gtk::Box {
    let custom = gtk::ColorDialogButton::builder()
        .dialog(&gtk::ColorDialog::builder().with_alpha(false).build())
        .halign(gtk::Align::Center)
        .tooltip_text("Custom color")
        .build();
    if let Some(rgba) = tag
        .color
        .as_deref()
        .and_then(|c| gtk::gdk::RGBA::parse(c).ok())
    {
        custom.set_rgba(&rgba);
    }
    let chosen = chosen.clone();
    custom.connect_rgba_notify(move |button| {
        for swatch in swatches.iter() {
            swatch.set_active(false);
        }
        *chosen.borrow_mut() = rgba_hex(&button.rgba());
    });
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Center)
        .build();
    row.append(&gtk::Label::new(Some("Custom")));
    row.append(&custom);
    row
}

fn rgba_hex(rgba: &gtk::gdk::RGBA) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(rgba.red()),
        channel(rgba.green()),
        channel(rgba.blue())
    )
}

fn children_of<'a>(tags: &'a [Tag], parent: Option<&str>) -> Vec<&'a Tag> {
    tags.iter()
        .filter(|tag| parent_path(&tag.value) == parent)
        .collect()
}

fn child_count(tags: &[Tag], value: &str) -> usize {
    tags.iter()
        .filter(|tag| parent_path(&tag.value) == Some(value))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(value: &str) -> Tag {
        Tag {
            id: value.into(),
            name: value.rsplit('/').next().unwrap().into(),
            value: value.into(),
            color: None,
        }
    }

    #[test]
    fn children_of_lists_one_level() {
        let tags = vec![
            tag("Trips"),
            tag("Trips/Japan"),
            tag("Trips/Japan/Kyoto"),
            tag("Food"),
        ];
        let values = |found: Vec<&Tag>| found.iter().map(|t| t.value.clone()).collect::<Vec<_>>();
        assert_eq!(values(children_of(&tags, None)), ["Trips", "Food"]);
        assert_eq!(values(children_of(&tags, Some("Trips"))), ["Trips/Japan"]);
        assert_eq!(child_count(&tags, "Trips"), 1);
        assert_eq!(child_count(&tags, "Food"), 0);
    }

    #[test]
    fn rgba_hex_formats_lowercase_hex() {
        let rgba = gtk::gdk::RGBA::new(1.0, 0.0, 0.5, 1.0);
        assert_eq!(rgba_hex(&rgba), "#ff0080");
    }
}
