//! Turns a server-provided or bundled MapLibre style into a libshumate vector renderer.
//!
//! libshumate needs TileJSON embedded in the style, loads no sprites by itself, and
//! rejects a whole style over one unknown operator, so each is handled here.

use std::collections::BTreeMap;

use gtk::gio;
use serde_json::Value;
use shumate::prelude::*;

use crate::api_client::ImmichApiClient;

/// Operators libshumate cannot parse; Pango shapes every script, so these hold anyway.
const ALWAYS_TRUE_OPERATORS: [&str; 1] = ["is-supported-script"];
const TILEJSON_KEYS: [&str; 5] = ["tiles", "minzoom", "maxzoom", "attribution", "bounds"];
const BUNDLED_STYLES: &str = "/dev/nicx/mimick/map-styles";

struct Sprite {
    png: Vec<u8>,
    json: String,
    scale: f64,
}

pub async fn load_renderer(
    api: &ImmichApiClient,
    url: &str,
) -> Result<shumate::VectorRenderer, String> {
    let style = fetch_json(api, url).await?;
    prepare_renderer(api, style, url).await
}

pub async fn load_bundled_renderer(
    api: &ImmichApiClient,
    theme: &str,
) -> Result<shumate::VectorRenderer, String> {
    let path = bundled_path(theme);
    let style = bundled_style(&path)?;
    prepare_renderer(api, style, &path).await
}

fn bundled_path(theme: &str) -> String {
    format!("{BUNDLED_STYLES}/{theme}.json")
}

fn bundled_style(path: &str) -> Result<Value, String> {
    let bytes = gio::resources_lookup_data(path, gio::ResourceLookupFlags::NONE)
        .map_err(|err| format!("{path}: {err}"))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("{path}: {err}"))
}

async fn prepare_renderer(
    api: &ImmichApiClient,
    mut style: Value,
    id: &str,
) -> Result<shumate::VectorRenderer, String> {
    keep_main_source(&mut style);
    let base = sprite_base(&style);
    let (inlined, sprite) = futures_util::join!(inline_tilejson(api, &mut style), async {
        match &base {
            Some(base) => fetch_sprite(api, base).await,
            None => None,
        }
    });
    inlined?;
    rewrite_unsupported(&mut style);
    flatten_text_fonts(&mut style);
    let credit = attribution(&style);
    let renderer = build_renderer(&cache_id(id), style)?;
    if let Some(credit) = credit {
        renderer.set_license(&credit);
    }
    if let Some(sprite) = sprite {
        attach_sprite(&renderer, &sprite);
    }
    Ok(renderer)
}

async fn fetch_json(api: &ImmichApiClient, url: &str) -> Result<Value, String> {
    let bytes = api.fetch_public_bytes(url).await?;
    serde_json::from_slice(&bytes).map_err(|err| format!("{url}: {err}"))
}

fn keep_main_source(style: &mut Value) {
    let Some(main) = main_source(style) else {
        return;
    };
    if let Some(sources) = style.get_mut("sources").and_then(Value::as_object_mut) {
        sources.retain(|id, _| *id == main);
    }
    if let Some(layers) = style.get_mut("layers").and_then(Value::as_array_mut) {
        layers.retain(|layer| {
            layer
                .get("source")
                .and_then(Value::as_str)
                .is_none_or(|source| source == main)
        });
    }
}

fn main_source(style: &Value) -> Option<String> {
    let mut counts = BTreeMap::<&str, usize>::new();
    let layers = style.get("layers")?.as_array()?;
    for source in layers
        .iter()
        .filter_map(|layer| layer.get("source")?.as_str())
    {
        *counts.entry(source).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(id, _)| id.to_owned())
}

async fn inline_tilejson(api: &ImmichApiClient, style: &mut Value) -> Result<(), String> {
    let Some(sources) = style.get_mut("sources").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    for source in sources.values_mut() {
        let Some(url) = source.get("url").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        let tilejson = fetch_json(api, &url).await?;
        merge_tilejson(source, &tilejson);
    }
    Ok(())
}

/// Replace a source's TileJSON link with its contents; keys set on the source win.
fn merge_tilejson(source: &mut Value, tilejson: &Value) {
    let Some(source) = source.as_object_mut() else {
        return;
    };
    source.remove("url");
    for key in TILEJSON_KEYS {
        if let Some(value) = tilejson.get(key) {
            source
                .entry(key.to_string())
                .or_insert_with(|| value.clone());
        }
    }
}

/// The style's source attributions as Pango markup, keeping their links.
fn attribution(style: &Value) -> Option<String> {
    let parts: Vec<String> = style
        .get("sources")?
        .as_object()?
        .values()
        .filter_map(|source| source.get("attribution")?.as_str())
        .map(markup)
        .filter(|text| !text.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// HTML as Pango markup: links kept, other tags dropped, and whitespace collapsed.
fn markup(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_link = false;
    let mut rest = html;
    while let Some((text, after)) = rest.split_once('<') {
        out.push_str(&escape(text));
        let (tag, tail) = after.split_once('>').unwrap_or((after, ""));
        out.push_str(&link_tag(tag.trim(), &mut in_link));
        rest = tail;
    }
    out.push_str(&escape(rest));
    if in_link {
        out.push_str("</a>");
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn link_tag(tag: &str, in_link: &mut bool) -> String {
    if tag.eq_ignore_ascii_case("/a") && *in_link {
        *in_link = false;
        return "</a>".to_string();
    }
    match href(tag) {
        Some(url) if !*in_link => {
            *in_link = true;
            format!("<a href=\"{}\">", escape(url))
        }
        _ => String::new(),
    }
}

fn href(tag: &str) -> Option<&str> {
    let (name, attributes) = tag.split_once(char::is_whitespace)?;
    if !name.eq_ignore_ascii_case("a") {
        return None;
    }
    let (_, value) = attributes.split_once("href=\"")?;
    value.split_once('"').map(|(url, _)| url)
}

fn escape(html: &str) -> glib::GString {
    glib::markup_escape_text(&decode_entities(html))
}

fn decode_entities(html: &str) -> String {
    let entities = [
        ("&copy;", "©"),
        ("&nbsp;", " "),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&amp;", "&"),
    ];
    entities
        .iter()
        .fold(html.to_string(), |text, (entity, plain)| {
            text.replace(entity, plain)
        })
}

fn rewrite_unsupported(value: &mut Value) {
    match value {
        Value::Array(items) if is_always_true(items) => *value = Value::Bool(true),
        Value::Array(items) => items.iter_mut().for_each(rewrite_unsupported),
        Value::Object(map) => map.values_mut().for_each(rewrite_unsupported),
        _ => {}
    }
}

fn is_always_true(items: &[Value]) -> bool {
    items
        .first()
        .and_then(Value::as_str)
        .is_some_and(|op| ALWAYS_TRUE_OPERATORS.contains(&op))
}

/// libshumate reads `text-font` only as a list of names, so an expression there
/// becomes the font list of its last (fallback) branch.
fn flatten_text_fonts(style: &mut Value) {
    let Some(layers) = style.get_mut("layers").and_then(Value::as_array_mut) else {
        return;
    };
    let layouts = layers
        .iter_mut()
        .filter_map(|layer| layer.get_mut("layout"))
        .filter_map(Value::as_object_mut);
    for layout in layouts {
        let Some(fonts) = layout.get("text-font").filter(|fonts| !is_font_list(fonts)) else {
            continue;
        };
        match fallback_fonts(fonts) {
            Some(list) => layout.insert("text-font".to_string(), list),
            None => layout.remove("text-font"),
        };
    }
}

fn is_font_list(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().all(Value::is_string))
}

fn fallback_fonts(value: &Value) -> Option<Value> {
    let items = value.as_array()?;
    if items.first().and_then(Value::as_str) == Some("literal") {
        return items.get(1).filter(|list| is_font_list(list)).cloned();
    }
    items.iter().rev().find_map(fallback_fonts)
}

/// Build the renderer, dropping any layer libshumate still rejects instead of the whole map.
fn build_renderer(id: &str, mut style: Value) -> Result<shumate::VectorRenderer, String> {
    loop {
        let err = match shumate::VectorRenderer::new(id, &style.to_string()) {
            Ok(renderer) => return Ok(renderer),
            Err(err) => err.to_string(),
        };
        let removed = rejected_layer(&err).is_some_and(|layer| remove_layer(&mut style, layer));
        if !removed {
            return Err(err);
        }
        log::warn!("Map style: skipped a layer libshumate cannot draw: {err}");
    }
}

fn rejected_layer(error: &str) -> Option<&str> {
    error
        .strip_prefix("layer '")?
        .split_once("':")
        .map(|(id, _)| id)
}

fn remove_layer(style: &mut Value, id: &str) -> bool {
    let Some(layers) = style.get_mut("layers").and_then(Value::as_array_mut) else {
        return false;
    };
    let before = layers.len();
    layers.retain(|layer| layer.get("id").and_then(Value::as_str) != Some(id));
    layers.len() < before
}

/// Tile cache directory name; distinct per style so servers never share stale tiles.
fn cache_id(url: &str) -> String {
    format!("mimick-{}", &blake3::hash(url.as_bytes()).to_hex()[..16])
}

fn sprite_base(style: &Value) -> Option<String> {
    match style.get("sprite")? {
        Value::String(url) => Some(url.clone()),
        Value::Array(sheets) => sheets.first()?.get("url")?.as_str().map(str::to_owned),
        _ => None,
    }
}

/// `base` plus the `@2x`-style suffix and extension, keeping any query string last.
fn sprite_url(base: &str, suffix: &str, extension: &str) -> String {
    match base.split_once('?') {
        Some((path, query)) => format!("{path}{suffix}.{extension}?{query}"),
        None => format!("{base}{suffix}.{extension}"),
    }
}

async fn fetch_sprite(api: &ImmichApiClient, base: &str) -> Option<Sprite> {
    for (suffix, scale) in [("@2x", 2.0), ("", 1.0)] {
        let (json_url, png_url) = (
            sprite_url(base, suffix, "json"),
            sprite_url(base, suffix, "png"),
        );
        let (json, png) = futures_util::join!(
            api.fetch_public_bytes(&json_url),
            api.fetch_public_bytes(&png_url)
        );
        if let (Ok(json), Ok(png)) = (json, png) {
            return Some(Sprite {
                json: String::from_utf8(json).ok()?,
                png,
                scale,
            });
        }
    }
    log::warn!("Map style: no sprite sheet at {base}; icons are hidden");
    None
}

fn attach_sprite(renderer: &shumate::VectorRenderer, sprite: &Sprite) {
    let sheet = shumate::VectorSpriteSheet::new();
    let added = gtk::gdk::Texture::from_bytes(&glib::Bytes::from(&sprite.png))
        .map_err(|err| err.to_string())
        .and_then(|texture| {
            sheet
                .add_page(&texture, &sprite.json, sprite.scale)
                .map_err(|err| err.to_string())
        });
    match added {
        Ok(()) => renderer.set_sprite_sheet(&sheet),
        Err(err) => log::warn!("Map style: icons unavailable: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::styles::MapStyle;
    use super::*;
    use serde_json::json;

    #[test]
    fn tilejson_fills_the_source_without_overriding_it() {
        let mut source = json!({ "type": "vector", "url": "https://t/v1.json", "maxzoom": 12 });
        let tilejson =
            json!({ "tiles": ["https://t/{z}/{x}/{y}.mvt"], "maxzoom": 15, "name": "x" });
        merge_tilejson(&mut source, &tilejson);
        assert_eq!(
            source,
            json!({ "type": "vector", "tiles": ["https://t/{z}/{x}/{y}.mvt"], "maxzoom": 12 })
        );
    }

    #[test]
    fn every_bundled_theme_builds_a_renderer() {
        gio::resources_register_include!("mimick.gresource").unwrap();
        let themes = MapStyle::ALL
            .into_iter()
            .filter_map(MapStyle::themes)
            .flat_map(|(light, dark)| [light, dark]);
        for theme in themes {
            let path = bundled_path(&theme);
            let mut style = bundled_style(&path).unwrap();
            keep_main_source(&mut style);
            rewrite_unsupported(&mut style);
            flatten_text_fonts(&mut style);
            assert_eq!(parse_without_gtk(&style), Ok(()), "{theme}");
        }
    }

    fn parse_without_gtk(style: &Value) -> Result<(), String> {
        use glib::translate::{FromGlibPtrFull, ToGlibPtr};
        let json = style.to_string();
        let mut error = std::ptr::null_mut();
        let renderer = unsafe {
            shumate::ffi::shumate_vector_renderer_new(
                c"test".as_ptr(),
                json.as_str().to_glib_none().0,
                &mut error,
            )
        };
        if renderer.is_null() {
            let error: glib::Error = unsafe { glib::Error::from_glib_full(error) };
            return Err(error.to_string());
        }
        unsafe { glib::gobject_ffi::g_object_unref(renderer.cast()) };
        Ok(())
    }

    #[test]
    fn only_the_most_used_source_is_kept() {
        let mut style = json!({
            "sources": { "osm": {}, "landcover": {} },
            "layers": [
                { "id": "background", "type": "background" },
                { "id": "forest", "source": "landcover" },
                { "id": "roads", "source": "osm" },
                { "id": "labels", "source": "osm" }
            ]
        });
        keep_main_source(&mut style);
        assert_eq!(
            style,
            json!({
                "sources": { "osm": {} },
                "layers": [
                    { "id": "background", "type": "background" },
                    { "id": "roads", "source": "osm" },
                    { "id": "labels", "source": "osm" }
                ]
            })
        );
    }

    #[test]
    fn unsupported_operators_become_true() {
        let mut layer = json!({
            "layout": { "text-field": ["case", ["is-supported-script", ["get", "name"]], ["get", "name"], ""] }
        });
        rewrite_unsupported(&mut layer);
        assert_eq!(
            layer,
            json!({ "layout": { "text-field": ["case", true, ["get", "name"], ""] } })
        );
    }

    #[test]
    fn attributions_become_markup_with_links() {
        let style = json!({ "sources": {
            "immich": { "attribution": "<a href=\"https://www.openstreetmap.org/copyright\" target=\"_blank\">&copy; OpenStreetMap</a>" },
            "plain": { "tiles": [] }
        } });
        assert_eq!(
            attribution(&style).as_deref(),
            Some("<a href=\"https://www.openstreetmap.org/copyright\">© OpenStreetMap</a>")
        );
        let html = "<a href=\"x?a=1&b=2\">&copy; OSM</a> · <a>CC BY 4.0</a> <b>Tom</b> &amp; <i>Jerry\n <a href=\"y\">open";
        let text = markup(html);
        assert_eq!(
            text,
            "<a href=\"x?a=1&amp;b=2\">© OSM</a> · CC BY 4.0 Tom &amp; Jerry <a href=\"y\">open</a>"
        );
        assert_eq!(attribution(&json!({ "sources": {} })), None);
    }

    #[test]
    fn font_expressions_become_their_fallback_list() {
        let mut style = json!({ "layers": [
            { "id": "city", "layout": { "text-font": ["case", ["<=", ["get", "min_zoom"], 5],
                ["literal", ["Noto Sans Medium"]], ["literal", ["Noto Sans Regular"]]] } },
            { "id": "road", "layout": { "text-font": ["Noto Sans Bold"] } },
            { "id": "odd", "layout": { "text-font": ["case", ["has", "font"], ["get", "font"], ["get", "other"]] } },
            { "id": "fill", "paint": {} }
        ] });
        flatten_text_fonts(&mut style);
        assert_eq!(
            style["layers"][0]["layout"]["text-font"],
            json!(["Noto Sans Regular"])
        );
        assert_eq!(
            style["layers"][1]["layout"]["text-font"],
            json!(["Noto Sans Bold"])
        );
        assert_eq!(style["layers"][2]["layout"], json!({}));
    }

    #[test]
    fn rejected_layers_are_found_and_removed() {
        let error = "layer 'water_label': Unrecognized operator foo";
        assert_eq!(rejected_layer(error), Some("water_label"));
        assert_eq!(rejected_layer("Expected 'tiles' array"), None);
        let mut style = json!({ "layers": [{ "id": "water_label" }, { "id": "roads" }] });
        assert!(remove_layer(&mut style, "water_label"));
        assert!(!remove_layer(&mut style, "water_label"));
        assert_eq!(style, json!({ "layers": [{ "id": "roads" }] }));
    }

    #[test]
    fn sprite_urls_accept_both_style_forms() {
        let single = json!({ "sprite": "https://s/sprites/light" });
        let multi = json!({ "sprite": [{ "id": "base", "url": "https://s/base" }] });
        assert_eq!(
            sprite_base(&single).as_deref(),
            Some("https://s/sprites/light")
        );
        assert_eq!(sprite_base(&multi).as_deref(), Some("https://s/base"));
        assert_eq!(sprite_base(&json!({})), None);
        assert_eq!(
            sprite_url("https://s/a", "@2x", "png"),
            "https://s/a@2x.png"
        );
        assert_eq!(
            sprite_url("https://s/a?key=1", "", "json"),
            "https://s/a.json?key=1"
        );
    }

    #[test]
    fn cache_ids_differ_per_style() {
        assert_ne!(
            cache_id("https://a/light.json"),
            cache_id("https://a/dark.json")
        );
        assert_eq!(cache_id("x").len(), "mimick-".len() + 16);
    }
}
