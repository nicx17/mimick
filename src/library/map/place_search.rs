//! Place search for the Map page and Set Location: Photon first, Immich's own places when it can't answer.

use crate::i18n::fill;
use gettextrs::gettext;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::f64::consts::PI;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gtk::prelude::*;
use shumate::prelude::*;

use crate::api_client::{
    ImmichApiClient, PhotonPlace, PhotonProperties, PhotonReply, Place, SearchBias,
};

use super::coords::GeoPoint;

pub const MIN_QUERY_CHARS: usize = 3;
pub const SEARCH_DELAY_MS: u32 = 150;
const MAX_RESULTS: usize = 8;
const CACHE_LIMIT: usize = 64;
const LANGUAGES: [&str; 3] = ["en", "de", "fr"];
const FIT_SHARE: f64 = 0.8;
const WEB_TILE: f64 = 256.0;
const PHOTON_CREDIT: &str = "Search by <a href=\"https://photon.komoot.io\">Photon</a> · \
    <a href=\"https://www.openstreetmap.org/copyright\">© OpenStreetMap contributors</a>";
const IMMICH_CREDIT: &str = "Places from your Immich server";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Photon,
    Immich,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub places: Vec<FoundPlace>,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FoundPlace {
    pub name: String,
    pub detail: String,
    pub point: GeoPoint,
    zooms: Zooms,
    bounds: Option<Bounds>,
}

/// Web-map zoom levels (256 px tiles) that suit a kind of place.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Zooms {
    min: f64,
    default: f64,
    max: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Bounds {
    west: f64,
    north: f64,
    east: f64,
    south: f64,
}

impl FoundPlace {
    /// The zoom showing the whole place in a `width` × `height` map drawn with `tile_size` px tiles.
    fn zoom_for(&self, width: f64, height: f64, tile_size: f64) -> f64 {
        let fit = self
            .bounds
            .and_then(|bounds| fit_zoom(bounds, width, height));
        let web = fit.map_or(self.zooms.default, |fit| {
            fit.clamp(self.zooms.min, self.zooms.max)
        });
        web - (tile_size / WEB_TILE).log2()
    }
}

type Done = Box<dyn FnOnce(Found)>;
type Request = (String, Option<SearchBias>, Done);

/// Searches one at a time, remembers answers, and stops asking Photon while it says to wait.
pub struct PlaceSearch {
    api: Arc<ImmichApiClient>,
    cache: RefCell<HashMap<String, Found>>,
    paused_until: Cell<Option<Instant>>,
    busy: Cell<bool>,
    queued: RefCell<Option<Request>>,
}

impl PlaceSearch {
    pub fn new(api: Arc<ImmichApiClient>) -> Rc<Self> {
        Rc::new(Self {
            api,
            cache: RefCell::default(),
            paused_until: Cell::new(None),
            busy: Cell::new(false),
            queued: RefCell::default(),
        })
    }

    pub fn cached(&self, query: &str, bias: Option<SearchBias>) -> Option<Found> {
        self.cache.borrow().get(&cache_key(query, bias)).cloned()
    }

    /// A search made while another runs waits for it, replacing any search already waiting.
    pub fn search(
        self: &Rc<Self>,
        query: String,
        bias: Option<SearchBias>,
        done: impl FnOnce(Found) + 'static,
    ) {
        let request: Request = (query, bias, Box::new(done));
        if self.busy.get() {
            self.queued.replace(Some(request));
            return;
        }
        self.run(request);
    }

    fn run(self: &Rc<Self>, (query, bias, done): Request) {
        self.busy.set(true);
        let search = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let found = search.find(&query, bias).await;
            search.busy.set(false);
            done(found);
            if let Some(next) = search.queued.take() {
                search.run(next);
            }
        });
    }

    async fn find(&self, query: &str, bias: Option<SearchBias>) -> Found {
        let key = cache_key(query, bias);
        if let Some(found) = self.cache.borrow().get(&key) {
            return found.clone();
        }
        match self.photon(query, bias).await {
            Ok(places) => {
                let found = Found {
                    places,
                    source: Source::Photon,
                };
                self.remember(key, found.clone());
                found
            }
            Err(err) => {
                log::warn!("Photon place search failed, using Immich's: {err}");
                self.immich(query).await
            }
        }
    }

    async fn photon(
        &self,
        query: &str,
        bias: Option<SearchBias>,
    ) -> Result<Vec<FoundPlace>, String> {
        let paused = self.paused_until.get();
        if paused.is_some_and(|until| Instant::now() < until) {
            return Err("waiting for Photon's Retry-After".to_string());
        }
        match self.api.search_photon(query, language(), bias).await? {
            PhotonReply::Places(places) => Ok(places.into_iter().filter_map(from_photon).collect()),
            PhotonReply::RetryAfter(wait) => {
                self.paused_until.set(Some(Instant::now() + wait));
                Err(fill(
                    gettext("Photon asked to wait {seconds} s"),
                    &[("seconds", &wait.as_secs().to_string())],
                ))
            }
        }
    }

    async fn immich(&self, query: &str) -> Found {
        let places = self.api.search_places(query).await.unwrap_or_else(|err| {
            log::warn!("Place search failed: {err}");
            Vec::new()
        });
        Found {
            places: places
                .into_iter()
                .filter_map(from_immich)
                .take(MAX_RESULTS)
                .collect(),
            source: Source::Immich,
        }
    }

    fn remember(&self, key: String, found: Found) {
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, found);
    }
}

/// Where `map` is looking, so nearby places rank first.
pub fn bias_of(map: &shumate::SimpleMap) -> Option<SearchBias> {
    let viewport = map.viewport()?;
    let web = viewport.zoom_level() + (tile_size(map) / WEB_TILE).log2();
    Some(SearchBias {
        lat: viewport.latitude(),
        lon: viewport.longitude(),
        zoom: web.round().clamp(0.0, 18.0) as u32,
    })
}

/// Fly `map` to `place`, the way "Show my location" does.
pub fn fly_to(map: &shumate::SimpleMap, place: &FoundPlace) {
    let (width, height) = (f64::from(map.width()), f64::from(map.height()));
    let zoom = place.zoom_for(width, height, tile_size(map));
    if let Some(inner) = map.map() {
        inner.go_to_full(place.point.lat, place.point.lon, zoom);
    }
}

fn tile_size(map: &shumate::SimpleMap) -> f64 {
    map.viewport()
        .and_then(|viewport| viewport.reference_map_source())
        .map_or(WEB_TILE, |source| f64::from(source.tile_size()))
}

pub fn place_row(place: &FoundPlace) -> libadwaita::ActionRow {
    libadwaita::ActionRow::builder()
        .title(&place.name)
        .subtitle(&place.detail)
        .use_markup(false)
        .activatable(true)
        .build()
}

/// The line under the results: a spinner while a search is out, then who answered.
pub struct SearchFooter {
    pub root: gtk::Box,
    spinner: gtk::Spinner,
    label: gtk::Label,
}

pub fn search_footer() -> SearchFooter {
    let spinner = gtk::Spinner::builder().visible(false).build();
    let label = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["caption", "dim-label"])
        .build();
    let root = gtk::Box::builder().spacing(6).build();
    root.append(&spinner);
    root.append(&label);
    SearchFooter {
        root,
        spinner,
        label,
    }
}

impl SearchFooter {
    pub fn searching(&self) {
        self.spinner.set_visible(true);
        self.spinner.start();
        self.label.set_text(&gettext("Searching…"));
    }

    pub fn answered(&self, source: Source) {
        self.spinner.stop();
        self.spinner.set_visible(false);
        match source {
            Source::Photon => self.label.set_markup(PHOTON_CREDIT),
            Source::Immich => self.label.set_text(IMMICH_CREDIT),
        }
    }
}

fn from_photon(place: PhotonPlace) -> Option<FoundPlace> {
    let [lon, lat] = place.geometry.coordinates;
    let point = GeoPoint::new(lat, lon)?;
    let properties = place.properties;
    let name = photon_name(&properties)?;
    let kind = properties.kind.as_deref().unwrap_or("other");
    Some(FoundPlace {
        detail: photon_detail(&properties, &name),
        name,
        point,
        zooms: zooms_for(kind),
        bounds: properties.extent.map(|[west, north, east, south]| Bounds {
            west,
            north,
            east,
            south,
        }),
    })
}

fn photon_name(properties: &PhotonProperties) -> Option<String> {
    let named = properties.name.as_deref().map(str::trim);
    if let Some(name) = named.filter(|name| !name.is_empty()) {
        return Some(name.to_string());
    }
    let street = properties.street.as_deref()?;
    Some(match properties.housenumber.as_deref() {
        Some(number) => format!("{street} {number}"),
        None => street.to_string(),
    })
}

/// "Kind · District, City, State, Country", skipping missing parts and repeats of the name.
fn photon_detail(properties: &PhotonProperties, name: &str) -> String {
    let kind = kind_label(
        properties.kind.as_deref().unwrap_or("other"),
        properties.osm_value.as_deref(),
        properties.name.is_some(),
    );
    let parts = [
        &properties.district,
        &properties.city,
        &properties.state,
        &properties.country,
    ];
    let region = region(name, parts);
    if region.is_empty() {
        kind
    } else {
        format!("{kind} · {region}")
    }
}

fn kind_label(kind: &str, osm_value: Option<&str>, named: bool) -> String {
    if kind == "house" && !named {
        return gettext("Address");
    }
    let broad = matches!(kind, "street" | "county" | "state" | "country");
    let specific = osm_value.filter(|value| !broad && *value != "administrative");
    humanize(specific.unwrap_or(kind))
}

fn humanize(value: &str) -> String {
    let text = value.replace('_', " ");
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn from_immich(place: Place) -> Option<FoundPlace> {
    Some(FoundPlace {
        point: GeoPoint::new(place.latitude, place.longitude)?,
        detail: region(&place.name, [&place.admin2name, &place.admin1name]),
        name: place.name,
        zooms: zooms_for("city"),
        bounds: None,
    })
}

/// The distinct, non-empty parts other than `name`, joined with commas.
fn region<'a>(name: &str, parts: impl IntoIterator<Item = &'a Option<String>>) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for part in parts.into_iter().flatten() {
        let part = part.trim();
        if !part.is_empty() && part != name && !kept.contains(&part) {
            kept.push(part);
        }
    }
    kept.join(", ")
}

fn zooms_for(kind: &str) -> Zooms {
    let (min, default, max) = match kind {
        "country" => (4.0, 5.0, 7.0),
        "state" => (5.0, 7.0, 10.0),
        "county" => (7.0, 9.0, 12.0),
        "city" => (9.0, 12.0, 14.0),
        "district" | "locality" => (12.0, 14.0, 16.0),
        "street" => (14.0, 16.0, 17.0),
        "house" => (16.0, 17.0, 18.0),
        _ => (10.0, 15.0, 17.0),
    };
    Zooms { min, default, max }
}

/// The web zoom at which `bounds` fill most of the map; `None` for a single point.
fn fit_zoom(bounds: Bounds, width: f64, height: f64) -> Option<f64> {
    let degrees = bounds.east - bounds.west;
    let span_x = if degrees < 0.0 {
        degrees + 360.0
    } else {
        degrees
    } / 360.0;
    let span_y = mercator_y(bounds.south) - mercator_y(bounds.north);
    let zoom_x = (FIT_SHARE * width / (span_x * WEB_TILE)).log2();
    let zoom_y = (FIT_SHARE * height / (span_y * WEB_TILE)).log2();
    let zoom = zoom_x.min(zoom_y);
    zoom.is_finite().then_some(zoom)
}

/// Web Mercator y in `0..1`, top to bottom.
fn mercator_y(lat: f64) -> f64 {
    let phi = lat.clamp(-85.0511, 85.0511).to_radians();
    (1.0 - phi.tan().asinh() / PI) / 2.0
}

fn language() -> &'static str {
    let names = glib::language_names();
    pick_language(names.iter().map(glib::GString::as_str))
}

/// The first system language Photon has names in, else English.
fn pick_language<'a>(names: impl IntoIterator<Item = &'a str>) -> &'static str {
    names
        .into_iter()
        .find_map(|name| {
            let code = name.split(['_', '.', '@']).next()?;
            LANGUAGES.into_iter().find(|lang| *lang == code)
        })
        .unwrap_or("en")
}

fn cache_key(query: &str, bias: Option<SearchBias>) -> String {
    let query = query.trim().to_lowercase();
    match bias {
        Some(bias) => format!("{query}|{:.1}|{:.1}|{}", bias.lat, bias.lon, bias.zoom),
        None => query,
    }
}

#[cfg(test)]
mod tests;
