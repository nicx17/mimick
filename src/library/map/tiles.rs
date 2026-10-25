//! Map tiles shared by every map in the window, for the chosen style and the system theme.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use glib::prelude::*;

use crate::api_client::{ImmichApiClient, MAP_USER_AGENT};
use crate::library::LibraryWindowUi;

use super::styles::MapStyle;

const OSM_TILE_URL: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";

#[derive(Clone)]
struct Sources {
    light: shumate::MapSource,
    dark: shumate::MapSource,
}

impl Sources {
    fn raster() -> Self {
        let raster = raster();
        Self {
            light: raster.clone(),
            dark: raster,
        }
    }
}

/// Loads the chosen style once and keeps attached maps on its light or dark variant.
pub struct Tiles {
    style: Cell<MapStyle>,
    sources: RefCell<Option<(MapStyle, Sources)>>,
    loading: Cell<Option<MapStyle>>,
    /// The server was unreachable, so the next attach asks it for its styles again.
    retry: Cell<bool>,
    maps: RefCell<Vec<glib::WeakRef<shumate::SimpleMap>>>,
}

impl Tiles {
    pub fn new() -> Rc<Self> {
        shumate::functions::set_user_agent(Some(MAP_USER_AGENT));
        Rc::new(Self {
            style: Cell::new(MapStyle::Immich),
            sources: RefCell::default(),
            loading: Cell::new(None),
            retry: Cell::new(false),
            maps: RefCell::default(),
        })
    }

    pub fn style(&self) -> MapStyle {
        self.style.get()
    }

    /// Start with `style` (the saved choice) without loading anything yet.
    pub fn preset(&self, style: MapStyle) {
        self.style.set(style);
    }

    pub fn attach(self: &Rc<Self>, ui: &Rc<LibraryWindowUi>, map: &shumate::SimpleMap) {
        let mut maps = self.maps.borrow_mut();
        maps.retain(|weak| weak.upgrade().is_some());
        if !maps.iter().any(|weak| weak.upgrade().as_ref() == Some(map)) {
            maps.push(map.downgrade());
        }
        drop(maps);
        if let Some(source) = self.current() {
            map.set_map_source(Some(&source));
        }
        self.ensure_loaded(ui);
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
        let dark = libadwaita::StyleManager::default().is_dark();
        match &*self.sources.borrow() {
            Some((style, sources)) if *style == self.style.get() => {
                Some(if dark { &sources.dark } else { &sources.light }.clone())
            }
            _ => None,
        }
    }

    fn needs_load(&self) -> bool {
        match &*self.sources.borrow() {
            Some((style, _)) => {
                *style != self.style.get() || (*style == MapStyle::Immich && self.retry.get())
            }
            None => true,
        }
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
            if tiles.style.get() == style {
                tiles.sources.replace(Some((style, sources)));
                tiles.retry.set(!reached);
                tiles.apply();
            }
        });
    }
}

/// The style's light and dark sources, and whether the server answered when asked.
async fn load_sources(api: &ImmichApiClient, style: MapStyle) -> (Sources, bool) {
    if style == MapStyle::Immich {
        return server_sources(api).await;
    }
    match style.palette_urls() {
        Some((light, dark)) => {
            let light = style_source(api, Some(&light)).await;
            let dark = style_source(api, Some(&dark)).await;
            (Sources { light, dark }, true)
        }
        None => (Sources::raster(), true),
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
    let light = style_source(api, urls.light.as_deref()).await;
    let dark = if urls.dark == urls.light {
        light.clone()
    } else {
        style_source(api, urls.dark.as_deref()).await
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
    shumate::MapSourceRegistry::with_defaults()
        .by_id(shumate::MAP_SOURCE_OSM_MAPNIK)
        .unwrap_or_else(|| shumate::RasterRenderer::from_url(OSM_TILE_URL).upcast())
}
