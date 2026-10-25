//! Turns a server-provided MapLibre style into a libshumate vector renderer.
//!
//! libshumate needs TileJSON embedded in the style, loads no sprites by itself, and
//! rejects a whole style over one unknown operator, so each is handled here.

use serde_json::Value;

use crate::api_client::ImmichApiClient;

/// Operators libshumate cannot parse; Pango shapes every script, so these hold anyway.
const ALWAYS_TRUE_OPERATORS: [&str; 1] = ["is-supported-script"];
const TILEJSON_KEYS: [&str; 5] = ["tiles", "minzoom", "maxzoom", "attribution", "bounds"];

struct Sprite {
    png: Vec<u8>,
    json: String,
    scale: f64,
}

pub async fn load_renderer(
    api: &ImmichApiClient,
    url: &str,
) -> Result<shumate::VectorRenderer, String> {
    let mut style = fetch_json(api, url).await?;
    inline_tilejson(api, &mut style).await?;
    rewrite_unsupported(&mut style);
    let sprite = match sprite_base(&style) {
        Some(base) => fetch_sprite(api, &base).await,
        None => None,
    };
    let renderer = build_renderer(&cache_id(url), style)?;
    if let Some(sprite) = sprite {
        attach_sprite(&renderer, &sprite);
    }
    Ok(renderer)
}

async fn fetch_json(api: &ImmichApiClient, url: &str) -> Result<Value, String> {
    let bytes = api.fetch_public_bytes(url).await?;
    serde_json::from_slice(&bytes).map_err(|err| format!("{url}: {err}"))
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
        let json = api
            .fetch_public_bytes(&sprite_url(base, suffix, "json"))
            .await;
        let png = api
            .fetch_public_bytes(&sprite_url(base, suffix, "png"))
            .await;
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
