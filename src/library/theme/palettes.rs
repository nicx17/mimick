//! The palettes behind each style: libadwaita's own, and tinted ones that recolour its
//! backgrounds through its CSS variables, so every widget follows.

use crate::i18n::gettext_noop;

/// One palette: the colours it gives libadwaita, also used to paint its swatch.
pub(crate) struct Palette {
    pub id: &'static str,
    pub label: &'static str,
    pub dark: bool,
    window: &'static str,
    view: &'static str,
    header: &'static str,
    sidebar: &'static str,
    fg: Option<&'static str>,
    /// Further declarations, after the generated ones so they win.
    extra: &'static str,
}

/// libadwaita's own palette, which needs no stylesheet.
pub(crate) const DEFAULT: &str = "default";

/// Black shadows vanish on black, so its borders and shades turn faintly light instead.
const BLACK_EXTRA: &str = "--dialog-bg-color: #0d0d0d; --popover-bg-color: #141414; \
     --thumbnail-bg-color: #0d0d0d; --card-bg-color: rgb(255 255 255 / 6%); \
     --headerbar-shade-color: rgb(255 255 255 / 8%); \
     --sidebar-border-color: rgb(255 255 255 / 8%); \
     --sidebar-shade-color: rgb(255 255 255 / 6%); \
     --secondary-sidebar-border-color: rgb(255 255 255 / 8%);";

const fn palette(
    id: &'static str,
    label: &'static str,
    dark: bool,
    [window, view, header, sidebar]: [&'static str; 4],
    fg: Option<&'static str>,
) -> Palette {
    Palette {
        id,
        label,
        dark,
        window,
        view,
        header,
        sidebar,
        fg,
        extra: "",
    }
}

pub(crate) const PALETTES: [Palette; 8] = [
    palette(
        DEFAULT,
        gettext_noop("Default"),
        false,
        ["#fafafb", "#ffffff", "#ffffff", "#ebebed"],
        None,
    ),
    palette(
        "warm",
        gettext_noop("Warm White"),
        false,
        ["#f7f3ea", "#fdfbf6", "#efe8da", "#efe8da"],
        Some("#3a3126"),
    ),
    palette(
        "lavender",
        gettext_noop("Lavender"),
        false,
        ["#f4f1fa", "#fcfbff", "#ebe5f5", "#ebe5f5"],
        Some("#2f2a3d"),
    ),
    palette(
        "mint",
        gettext_noop("Mint"),
        false,
        ["#eff6f2", "#fafdfb", "#e3efe8", "#e3efe8"],
        Some("#263330"),
    ),
    palette(
        DEFAULT,
        gettext_noop("Default"),
        true,
        ["#222226", "#1d1d20", "#2e2e32", "#2e2e32"],
        None,
    ),
    Palette {
        extra: BLACK_EXTRA,
        ..palette(
            "black",
            gettext_noop("Black"),
            true,
            ["#000000", "#000000", "#000000", "#000000"],
            None,
        )
    },
    palette(
        "midnight",
        gettext_noop("Midnight"),
        true,
        ["#161b26", "#11151d", "#1d2331", "#1a2030"],
        Some("#e6e9f0"),
    ),
    palette(
        "mocha",
        gettext_noop("Mocha"),
        true,
        ["#231f1c", "#1b1816", "#2c2622", "#29231f"],
        Some("#f0e9e2"),
    ),
];

/// The light or dark palette `id`; libadwaita's own for anything unknown.
pub(crate) fn find(id: &str, dark: bool) -> &'static Palette {
    let of_kind = || PALETTES.iter().filter(move |p| p.dark == dark);
    of_kind()
        .find(|p| p.id == id)
        .or_else(|| of_kind().next())
        .expect("each style has a default palette")
}

/// The palettes for the light or the dark style, in menu order.
pub(crate) fn of_style(dark: bool) -> impl Iterator<Item = &'static Palette> {
    PALETTES.iter().filter(move |p| p.dark == dark)
}

/// The stylesheet for `palette`; empty for libadwaita's own.
pub(crate) fn css(palette: &Palette) -> String {
    if palette.id == DEFAULT {
        return String::new();
    }
    let Palette {
        window,
        view,
        header,
        sidebar,
        ..
    } = palette;
    let mut css = format!(
        ":root {{ --window-bg-color: {window}; --view-bg-color: {view}; \
         --headerbar-bg-color: {header}; --headerbar-backdrop-color: {header}; \
         --sidebar-bg-color: {sidebar}; --sidebar-backdrop-color: {sidebar}; \
         --secondary-sidebar-bg-color: {sidebar}; --secondary-sidebar-backdrop-color: {sidebar}; \
         --overview-bg-color: {window}; --dialog-bg-color: {view}; --popover-bg-color: {view}; "
    );
    if !palette.dark {
        css.push_str(&format!(
            "--card-bg-color: {view}; --thumbnail-bg-color: {view}; "
        ));
    }
    if let Some(fg) = palette.fg {
        css.push_str(&foregrounds(fg));
    }
    css.push_str(palette.extra);
    css.push_str(" }");
    css
}

fn foregrounds(fg: &str) -> String {
    [
        "window",
        "view",
        "headerbar",
        "sidebar",
        "card",
        "dialog",
        "popover",
    ]
    .iter()
    .map(|part| format!("--{part}-fg-color: {fg}; "))
    .collect()
}

/// Each swatch: the palette's header colour over its content colour, split corner to corner.
pub(crate) fn swatch_css() -> String {
    PALETTES
        .iter()
        .map(|p| {
            format!(
                ".mimick-style-switcher checkbutton.{} {{ background-image: \
                 linear-gradient(to bottom right, {} 49.99%, {} 50.01%); }}\n",
                swatch_class(p),
                p.header,
                p.view
            )
        })
        .collect()
}

/// The CSS class that paints `palette`'s swatch.
pub(crate) fn swatch_class(palette: &Palette) -> String {
    let style = if palette.dark { "dark" } else { "light" };
    format!("palette-{style}-{}", palette.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_style_starts_with_libadwaitas_own_palette() {
        for dark in [false, true] {
            assert_eq!(of_style(dark).next().map(|p| p.id), Some(DEFAULT));
            assert_eq!(of_style(dark).count(), 4);
        }
    }

    #[test]
    fn unknown_palettes_fall_back_to_the_default() {
        assert_eq!(find("lavender", false).id, "lavender");
        assert_eq!(find("lavender", true).id, DEFAULT);
        assert_eq!(find("black", true).id, "black");
    }

    #[test]
    fn only_tinted_palettes_load_a_stylesheet() {
        assert!(css(find(DEFAULT, true)).is_empty());
        let black = css(find("black", true));
        assert!(black.contains("--window-bg-color: #000000"));
        assert!(black.contains("--popover-bg-color: #141414"));
        let warm = css(find("warm", false));
        assert!(warm.contains("--card-bg-color: #fdfbf6"));
        assert!(warm.contains("--view-fg-color: #3a3126"));
    }
}
