//! Debug aid: with `MIMICK_DEBUG_WIDTH` set, print the widget tree's widths each time the
//! window width changes, and the too-wide widgets the moment Adwaita warns that the window
//! content "exceeds AdwApplicationWindow width". `MIMICK_DEBUG_WIDTH=all` prints every
//! visible widget; any other value prints widgets at least 200 px wide and any that are too wide.

use std::cell::Cell;
use std::time::Duration;

use gtk::prelude::*;

const MIN_REPORTED: i32 = 200;

pub fn install(window: &impl IsA<gtk::Window>) {
    let Some(mode) = std::env::var_os("MIMICK_DEBUG_WIDTH") else {
        return;
    };
    let print_all = mode == "all";
    let window = window.as_ref().downgrade();
    let last_width = Cell::new(-1);
    glib::timeout_add_local(Duration::from_millis(700), move || {
        let Some(window) = window.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let width = window.width();
        if width > 0 && width != last_width.replace(width) {
            dump(&window, width, print_all);
        }
        glib::ControlFlow::Continue
    });
}

/// Print the too-wide widgets right when Adwaita reports the overflow, mid-resize.
pub fn trace_overflow_warnings(window: &impl IsA<gtk::Window>) {
    if std::env::var_os("MIMICK_DEBUG_WIDTH").is_none() {
        return;
    }
    let window = glib::SendWeakRef::from(window.as_ref().downgrade());
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
            if let Some(window) = window.upgrade() {
                let width = window.width();
                eprintln!("[width] ===== at the warning: window {width} px wide =====");
                let mut culprits = Vec::new();
                if let Some(root) = window.child() {
                    walk(&root, 0, width, false, &mut culprits, true);
                }
                print_summary(width, culprits);
            }
        },
    );
}

fn dump(window: &gtk::Window, width: i32, print_all: bool) {
    let Some(root) = window.child() else {
        return;
    };
    let height = window.height();
    eprintln!("[width] ===== window {width}x{height} =====");
    eprintln!("[width] min / nat = minimum and natural width | alloc = allocated width");
    let mut culprits = Vec::new();
    walk(&root, 0, width, print_all, &mut culprits, false);
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
