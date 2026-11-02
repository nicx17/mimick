//! Map tiles shared by every map in the window, for the chosen style and appearance.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use glib::prelude::*;
use gtk::prelude::*;
use shumate::prelude::*;

use crate::api_client::{ImmichApiClient, MAP_USER_AGENT};
use crate::library::LibraryWindowUi;

use super::styles::{MapAppearance, MapStyle};

const OSM_TILE_URL: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";
const OSM_CREDIT: &str =
    "<a href=\"https://www.openstreetmap.org/copyright\">© OpenStreetMap contributors</a>";
/// Recently used styles stay loaded, so switching back to one is instant.
const KEPT_STYLES: usize = 3;

#[derive(Clone)]
struct Sources {
    light: shumate::MapSource,
    dark: shumate::MapSource,
}

impl Sources {
    fn single(source: shumate::MapSource) -> Self {
        Self {
            light: source.clone(),
            dark: source,
        }
    }

    fn raster() -> Self {
        Self::single(raster())
    }
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
    maps: RefCell<Vec<glib::WeakRef<shumate::SimpleMap>>>,
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
        maps.retain(|weak| weak.upgrade().is_some());
        if !maps.iter().any(|weak| weak.upgrade().as_ref() == Some(map)) {
            maps.push(map.downgrade());
        }
        drop(maps);
        show_license_links(map);
        match self.current() {
            Some(source) => map.set_map_source(Some(&source)),
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

    /// Point every live map at the current style's light or dark source.
    pub fn apply(&self) {
        let Some(source) = self.current() else {
            return;
        };
        for map in self.live_maps() {
            map.set_map_source(Some(&source));
        }
    }

    /// Request tiles that failed to download again, e.g. after reconnecting.
    pub fn retry_failed(&self) {
        for layer in self
            .live_maps()
            .iter()
            .filter_map(|map| map.base_map_layer())
        {
            layer.retry_failed();
        }
    }

    fn live_maps(&self) -> Vec<shumate::SimpleMap> {
        self.maps
            .borrow()
            .iter()
            .filter_map(|weak| weak.upgrade())
            .collect()
    }

    fn current(&self) -> Option<shumate::MapSource> {
        let system_dark = libadwaita::StyleManager::default().is_dark();
        let dark = self.appearance.get().is_dark(system_dark);
        let style = self.style.get();
        self.sources
            .borrow()
            .iter()
            .find(|(loaded, _)| *loaded == style)
            .map(|(_, sources)| if dark { &sources.dark } else { &sources.light }.clone())
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
async fn load_sources(api: &ImmichApiClient, style: MapStyle) -> (Sources, bool) {
    if style == MapStyle::Immich {
        return server_sources(api).await;
    }
    let Some((light, dark)) = style.themes() else {
        return (Sources::raster(), true);
    };
    if light == dark {
        let light = bundled_source(api, &light).await;
        return (Sources::single(light), true);
    }
    let (light, dark) =
        futures_util::join!(bundled_source(api, &light), bundled_source(api, &dark));
    (Sources { light, dark }, true)
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
    (Sources { light, dark }, true)
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

fn raster() -> shumate::MapSource {
    let source = shumate::MapSourceRegistry::with_defaults()
        .by_id(shumate::MAP_SOURCE_OSM_MAPNIK)
        .unwrap_or_else(|| shumate::RasterRenderer::from_url(OSM_TILE_URL).upcast());
    source.set_license(OSM_CREDIT);
    source
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
