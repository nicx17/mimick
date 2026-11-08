//! Map tiles shared by every map in the window, for the chosen style and appearance.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use glib::prelude::*;
use gtk::prelude::*;
use shumate::prelude::*;

use crate::api_client::{ImmichApiClient, MAP_USER_AGENT};
use crate::library::LibraryWindowUi;

use super::styles::{MapAppearance, MapStyle};

const OSM_TILE_URL: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";
const OSM_CREDIT: &str =
    "<a href=\"https://www.openstreetmap.org/copyright\">© OpenStreetMap contributors</a>";
const TOPO_CREDIT: &str = "<a href=\"https://www.openstreetmap.org/copyright\">© OpenStreetMap \
contributors</a>, SRTM · <a href=\"https://opentopomap.org\">© OpenTopoMap</a> (CC-BY-SA)";
const HOT_CREDIT: &str = "<a href=\"https://www.openstreetmap.org/copyright\">© OpenStreetMap \
contributors</a> · <a href=\"https://www.hotosm.org\">Humanitarian OpenStreetMap Team</a>, \
hosted by <a href=\"https://www.openstreetmap.fr\">OpenStreetMap France</a>";

/// A tile server's map: id, URL, credit, and the deepest zoom it draws.
struct TileMap {
    id: &'static str,
    url: &'static str,
    credit: &'static str,
    max_zoom: u32,
}

const TOPO: TileMap = TileMap {
    id: "opentopomap",
    url: "https://tile.opentopomap.org/{z}/{x}/{y}.png",
    credit: TOPO_CREDIT,
    max_zoom: 17,
};

const HUMANITARIAN: TileMap = TileMap {
    id: "humanitarian",
    url: "https://a.tile.openstreetmap.fr/hot/{z}/{x}/{y}.png",
    credit: HOT_CREDIT,
    max_zoom: 19,
};

/// Recently used styles stay loaded, so switching back to one is instant.
const KEPT_STYLES: usize = 3;

#[derive(Clone)]
struct Sources {
    light: shumate::MapSource,
    dark: shumate::MapSource,
    labels: Option<shumate::MapSource>,
}

impl Sources {
    fn single(source: shumate::MapSource) -> Self {
        Self {
            light: source.clone(),
            dark: source,
            labels: None,
        }
    }

    fn raster() -> Self {
        Self::single(raster())
    }
}

struct Shown {
    base: shumate::MapSource,
    labels: Option<shumate::MapSource>,
}

struct Attached {
    map: glib::WeakRef<shumate::SimpleMap>,
    labels: Option<shumate::MapLayer>,
}

/// Loads the chosen style once and keeps attached maps on its light or dark variant.
pub struct Tiles {
    style: Cell<MapStyle>,
    appearance: Cell<MapAppearance>,
    /// Loaded styles, most recently loaded first.
    sources: RefCell<Vec<(MapStyle, Sources)>>,
    loading: Cell<Option<MapStyle>>,
    /// The server was unreachable, so the next attach asks it for its styles again.
    retry: Cell<bool>,
    maps: RefCell<Vec<Attached>>,
    /// Projection for maps still waiting for their style, so markers can be placed.
    placeholder: OnceCell<shumate::MapSource>,
}

impl Tiles {
    pub fn new() -> Rc<Self> {
        shumate::functions::set_user_agent(Some(MAP_USER_AGENT));
        Rc::new(Self {
            style: Cell::new(MapStyle::Immich),
            appearance: Cell::new(MapAppearance::System),
            sources: RefCell::default(),
            loading: Cell::new(None),
            retry: Cell::new(false),
            maps: RefCell::default(),
            placeholder: OnceCell::new(),
        })
    }

    pub fn style(&self) -> MapStyle {
        self.style.get()
    }

    pub fn appearance(&self) -> MapAppearance {
        self.appearance.get()
    }

    /// Start with the saved style and appearance without loading anything yet.
    pub fn preset(&self, style: MapStyle, appearance: MapAppearance) {
        self.style.set(style);
        self.appearance.set(appearance);
    }

    pub fn set_appearance(&self, appearance: MapAppearance) {
        if self.appearance.replace(appearance) != appearance {
            self.apply();
        }
    }

    pub fn attach(self: &Rc<Self>, ui: &Rc<LibraryWindowUi>, map: &shumate::SimpleMap) {
        let mut maps = self.maps.borrow_mut();
        maps.retain(|attached| attached.map.upgrade().is_some());
        if !maps
            .iter()
            .any(|attached| attached.map.upgrade().as_ref() == Some(map))
        {
            maps.push(Attached {
                map: map.downgrade(),
                labels: None,
            });
        }
        drop(maps);
        show_license_links(map);
        match self.current() {
            Some(_) => self.apply(),
            None => self.reference_placeholder(map),
        }
        self.ensure_loaded(ui);
    }

    /// libshumate can't position markers without a reference source; this one is never drawn.
    fn reference_placeholder(&self, map: &shumate::SimpleMap) {
        if let Some(viewport) = map.viewport()
            && viewport.reference_map_source().is_none()
        {
            viewport.set_reference_map_source(Some(self.placeholder.get_or_init(raster)));
        }
    }

    pub fn set_style(self: &Rc<Self>, ui: &Rc<LibraryWindowUi>, style: MapStyle) {
        if self.style.replace(style) == style {
            return;
        }
        self.apply();
        self.ensure_loaded(ui);
    }

    /// Point every live map at the current style's light or dark source and its labels.
    pub fn apply(&self) {
        let Some(shown) = self.current() else {
            return;
        };
        for attached in self.maps.borrow_mut().iter_mut() {
            if let Some(map) = attached.map.upgrade() {
                map.set_map_source(Some(&shown.base));
                attached.labels = show_labels(&map, attached.labels.take(), shown.labels.as_ref());
            }
        }
    }

    /// Request tiles that failed to download again, e.g. after reconnecting.
    pub fn retry_failed(&self) {
        for attached in self.maps.borrow().iter() {
            let Some(map) = attached.map.upgrade() else {
                continue;
            };
            for layer in map
                .base_map_layer()
                .into_iter()
                .chain(attached.labels.clone())
            {
                layer.retry_failed();
            }
        }
    }

    fn current(&self) -> Option<Shown> {
        let system_dark = libadwaita::StyleManager::default().is_dark();
        let dark = self.appearance.get().is_dark(system_dark);
        let style = self.style.get();
        self.sources
            .borrow()
            .iter()
            .find(|(loaded, _)| *loaded == style)
            .map(|(_, sources)| Shown {
                base: if dark { &sources.dark } else { &sources.light }.clone(),
                labels: sources.labels.clone(),
            })
    }

    fn needs_load(&self) -> bool {
        let style = self.style.get();
        let loaded = self.sources.borrow().iter().any(|(s, _)| *s == style);
        !loaded || (style == MapStyle::Immich && self.retry.get())
    }

    fn ensure_loaded(self: &Rc<Self>, ui: &Rc<LibraryWindowUi>) {
        let style = self.style.get();
        if !self.needs_load() || self.loading.get() == Some(style) {
            return;
        }
        self.loading.set(Some(style));
        let (tiles, api) = (self.clone(), ui.ctx.api_client.clone());
        glib::MainContext::default().spawn_local(async move {
            let (sources, reached) = load_sources(&api, style).await;
            if tiles.loading.get() == Some(style) {
                tiles.loading.set(None);
            }
            if style == MapStyle::Immich {
                tiles.retry.set(!reached);
            }
            remember(&mut tiles.sources.borrow_mut(), style, sources);
            if tiles.style.get() == style {
                tiles.apply();
            }
        });
    }
}

/// Put `style` first, replacing an older load of it, and forget the least recent beyond the limit.
fn remember<T>(loaded: &mut Vec<(MapStyle, T)>, style: MapStyle, value: T) {
    loaded.retain(|(existing, _)| *existing != style);
    loaded.insert(0, (style, value));
    loaded.truncate(KEPT_STYLES);
}

/// The style's light and dark sources, and whether the server answered when asked.
async fn load_sources(api: &Arc<ImmichApiClient>, style: MapStyle) -> (Sources, bool) {
    if style == MapStyle::Immich {
        return server_sources(api).await;
    }
    if style == MapStyle::Satellite {
        let mut sources = Sources::single(super::satellite::source(api));
        sources.labels = labels_source(api, style).await;
        return (sources, true);
    }
    let Some((light, dark)) = style.themes() else {
        return (Sources::single(tile_map(style)), true);
    };
    if light == dark {
        let light = bundled_source(api, &light).await;
        return (Sources::single(light), true);
    }
    let (light, dark) =
        futures_util::join!(bundled_source(api, &light), bundled_source(api, &dark));
    (
        Sources {
            light,
            dark,
            labels: None,
        },
        true,
    )
}

async fn labels_source(api: &ImmichApiClient, style: MapStyle) -> Option<shumate::MapSource> {
    let theme = style.labels()?;
    match super::style::load_bundled_renderer(api, theme).await {
        Ok(renderer) => Some(renderer.upcast()),
        Err(err) => {
            log::warn!("Map labels {theme} unavailable: {err}");
            None
        }
    }
}

async fn bundled_source(api: &ImmichApiClient, theme: &str) -> shumate::MapSource {
    match super::style::load_bundled_renderer(api, theme).await {
        Ok(renderer) => renderer.upcast(),
        Err(err) => {
            log::warn!("Map style {theme} unavailable, using OpenStreetMap: {err}");
            raster()
        }
    }
}

async fn server_sources(api: &ImmichApiClient) -> (Sources, bool) {
    let urls = match api.fetch_map_style_urls().await {
        Ok(urls) => urls,
        Err(err) => {
            log::warn!("Map styles unavailable, using OpenStreetMap: {err}");
            return (Sources::raster(), false);
        }
    };
    let (light, dark) = if urls.dark == urls.light {
        let light = style_source(api, urls.light.as_deref()).await;
        (light.clone(), light)
    } else {
        futures_util::join!(
            style_source(api, urls.light.as_deref()),
            style_source(api, urls.dark.as_deref())
        )
    };
    (
        Sources {
            light,
            dark,
            labels: None,
        },
        true,
    )
}

async fn style_source(api: &ImmichApiClient, url: Option<&str>) -> shumate::MapSource {
    let Some(url) = url.map(str::trim).filter(|url| !url.is_empty()) else {
        return raster();
    };
    match super::style::load_renderer(api, url).await {
        Ok(renderer) => renderer.upcast(),
        Err(err) => {
            log::warn!("Map style {url} unavailable, using OpenStreetMap: {err}");
            raster()
        }
    }
}

/// OpenTopoMap, Humanitarian, or else OpenStreetMap's own tiles.
fn tile_map(style: MapStyle) -> shumate::MapSource {
    let map = match style {
        MapStyle::OpenTopoMap => &TOPO,
        MapStyle::Humanitarian => &HUMANITARIAN,
        _ => return raster(),
    };
    let renderer = shumate::RasterRenderer::new_full_from_url(
        map.id,
        &style.label(),
        map.credit,
        "https://www.openstreetmap.org/copyright",
        0,
        map.max_zoom,
        256,
        shumate::MapProjection::Mercator,
        map.url,
    );
    renderer.upcast()
}

fn raster() -> shumate::MapSource {
    let source = shumate::MapSourceRegistry::with_defaults()
        .by_id(shumate::MAP_SOURCE_OSM_MAPNIK)
        .unwrap_or_else(|| shumate::RasterRenderer::from_url(OSM_TILE_URL).upcast());
    source.set_license(OSM_CREDIT);
    source
}

fn show_labels(
    map: &shumate::SimpleMap,
    layer: Option<shumate::MapLayer>,
    source: Option<&shumate::MapSource>,
) -> Option<shumate::MapLayer> {
    if layer
        .as_ref()
        .and_then(shumate::MapLayer::map_source)
        .as_ref()
        == source
    {
        return layer;
    }
    if let Some(layer) = layer {
        remove_labels(map, &layer);
    }
    add_labels(map, source?)
}

fn remove_labels(map: &shumate::SimpleMap, layer: &shumate::MapLayer) {
    if let Some(inner) = map.map() {
        inner.remove_layer(layer);
    }
    if let (Some(license), Some(source)) = (map.license(), layer.map_source()) {
        license.remove_map_source(&source);
    }
}

fn add_labels(map: &shumate::SimpleMap, source: &shumate::MapSource) -> Option<shumate::MapLayer> {
    let layer = shumate::MapLayer::new(source, &map.viewport()?);
    map.map()?
        .insert_layer_above(&layer, map.base_map_layer().as_ref());
    if let Some(license) = map.license() {
        license.append_map_source(source);
    }
    Some(layer)
}

fn show_license_links(map: &shumate::SimpleMap) {
    let label = map
        .license()
        .and_then(|license| license.first_child())
        .and_downcast::<gtk::Label>();
    if let Some(label) = label {
        label.set_use_markup(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_styles_are_kept_most_recent_first() {
        let mut loaded = Vec::new();
        for style in [
            MapStyle::Immich,
            MapStyle::Gray,
            MapStyle::Toner,
            MapStyle::Gray,
        ] {
            remember(&mut loaded, style, style.id());
        }
        let order: Vec<MapStyle> = loaded.iter().map(|(style, _)| *style).collect();
        assert_eq!(order, [MapStyle::Gray, MapStyle::Toner, MapStyle::Immich]);
        remember(&mut loaded, MapStyle::Muted, "muted");
        assert_eq!(loaded.len(), KEPT_STYLES);
        assert!(!loaded.iter().any(|(style, _)| *style == MapStyle::Immich));
    }
}
