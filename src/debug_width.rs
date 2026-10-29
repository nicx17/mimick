//! Debug aid: with `MIMICK_DEBUG_WIDTH` set, print the widget tree's widths each time the
//! width available to the window's content changes, and the too-wide widgets the moment
//! Adwaita warns that the content "exceeds" its container. Widths are measured against the
//! space the content really gets, so the GTK Inspector's adaptive preview (a 360 px phone
//! frame inside a wide window) is checked at 360 px. `MIMICK_DEBUG_WIDTH=all` prints every
//! visible widget; any other value prints widgets at least 200 px wide and any that are too wide.

use std::cell::Cell;
use std::time::Duration;

use gtk::prelude::*;

const MIN_REPORTED: i32 = 200;

/// Call while building the window, so its child is still Mimick's own content.
pub fn install(window: &impl IsA<gtk::Window>) {
    let Some(mode) = std::env::var_os("MIMICK_DEBUG_WIDTH") else {
        return;
    };
    let Some(content) = window.as_ref().child() else {
        return;
    };
    let print_all = mode == "all";
    let content = content.downgrade();
    let last_width = Cell::new(-1);
    glib::timeout_add_local(Duration::from_millis(700), move || {
        let Some(content) = content.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let width = available_width(&content);
        if width > 0 && width != last_width.replace(width) {
            dump(&content, width, print_all);
        }
        glib::ControlFlow::Continue
    });
}

/// Width of the container the content sits in: the window, or the Inspector's phone frame.
fn available_width(content: &gtk::Widget) -> i32 {
    content
        .parent()
        .map(|parent| parent.width())
        .unwrap_or_else(|| content.width())
}

/// Print the too-wide widgets right when Adwaita reports the overflow, mid-resize.
/// Call while building the window, like [`install`].
pub fn trace_overflow_warnings(window: &impl IsA<gtk::Window>) {
    if std::env::var_os("MIMICK_DEBUG_WIDTH").is_none() {
        return;
    }
    let Some(content) = window.as_ref().child() else {
        return;
    };
    let content = glib::SendWeakRef::from(content.downgrade());
    glib::log_set_handler(
        Some("Adwaita"),
        glib::LogLevels::LEVEL_WARNING,
        false,
        false,
        move |domain, level, message| {
            glib::log_default_handler(domain, level, Some(message));
            if !message.contains("exceeds") {
                return;
            }
            if let Some(content) = content.upgrade() {
                let width = available_width(&content);
                eprintln!("[width] ===== at the warning: {width} px available =====");
                let mut culprits = Vec::new();
                walk(&content, 0, width, true, &mut culprits, false);
                print_summary(width, culprits);
            }
        },
    );
}

fn dump(root: &gtk::Widget, width: i32, print_all: bool) {
    eprintln!("[width] ===== {width} px available =====");
    eprintln!("[width] min / nat = minimum and natural width | alloc = allocated width");
    let mut culprits = Vec::new();
    walk(root, 0, width, print_all, &mut culprits, false);
    print_summary(width, culprits);
}

fn print_summary(width: i32, culprits: Vec<String>) {
    if culprits.is_empty() {
        eprintln!("[width] summary: nothing is wider than the {width} px window");
    }
    for line in culprits {
        eprintln!("[width] summary: TOO WIDE {line}");
    }
}

/// Print `widget` and its subtree; returns whether anything in it is wider than `limit`.
/// The innermost too-wide widgets go into `culprits`.
fn walk(
    widget: &gtk::Widget,
    depth: usize,
    limit: i32,
    print_all: bool,
    culprits: &mut Vec<String>,
    quiet: bool,
) -> bool {
    if !widget.is_visible() {
        return false;
    }
    let (min, nat, _, _) = widget.measure(gtk::Orientation::Horizontal, -1);
    let over = min > limit;
    if !quiet && (print_all || over || min >= MIN_REPORTED) {
        eprintln!(
            "[width] {}{} min {min} nat {nat} | alloc {}{}",
            "  ".repeat(depth),
            describe(widget),
            widget.width(),
            if over { "  <-- WIDER THAN WINDOW" } else { "" },
        );
    }
    let mut child_over = false;
    let mut child = widget.first_child();
    while let Some(current) = child {
        child_over |= walk(&current, depth + 1, limit, print_all, culprits, quiet);
        child = current.next_sibling();
    }
    if over && !child_over {
        culprits.push(format!(
            "{} min {min} nat {nat} | alloc {}",
            path(widget),
            widget.width()
        ));
    }
    over || child_over
}

fn path(widget: &gtk::Widget) -> String {
    let mut parts = Vec::new();
    let mut current = Some(widget.clone());
    while let Some(node) = current {
        parts.push(describe(&node));
        current = node.parent();
    }
    parts.reverse();
    parts.join(" > ")
}

fn describe(widget: &gtk::Widget) -> String {
    let mut text = widget.type_().name().to_string();
    let name = widget.widget_name();
    if !name.is_empty() && name != text {
        text.push_str(&format!("#{name}"));
    }
    let classes = widget.css_classes();
    if !classes.is_empty() {
        let joined: Vec<&str> = classes.iter().map(|class| class.as_str()).collect();
        text.push_str(&format!(".{}", joined.join(".")));
    }
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        text.push_str(&format!(" {:?}", label.label()));
    } else if let Some(button) = widget.downcast_ref::<gtk::Button>()
        && let Some(label) = button.label()
    {
        text.push_str(&format!(" {label:?}"));
    }
    text
}
