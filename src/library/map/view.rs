//! Map page: every geotagged asset, clustered by screen distance; a click lists the photos.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk::prelude::*;
use shumate::prelude::*;

use crate::api_client::MapMarker;
use crate::library::LibraryWindowUi;
use crate::library::navigation;
use crate::library::state::{AssetIds, LibrarySource};

use super::camera;
use super::cluster::{self, Cluster, PixelWindow};
use super::controls::{self, MapControls};
use super::coords::GeoPoint;
use super::layout::Mode;
use super::markers::{self, Members};
use super::minimap::Minimap;
use super::styles::MapStyle;
use super::tiles::Tiles;

const REFRESH_DELAY: Duration = Duration::from_millis(120);
const FOCUS_ZOOM: f64 = 15.0;
const FALLBACK_SIZE: (f64, f64) = (800.0, 600.0);
const CACHED_LEVELS: usize = 5;

/// Photos keep their marker across zoom levels; groups are rebuilt for each level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum MarkerKey {
    Photo(usize),
    Group(u32, (i64, i64)),
}

fn marker_key(level: u32, cluster: &Cluster) -> MarkerKey {
    match cluster.members.as_slice() {
        [single] => MarkerKey::Photo(*single),
        _ => MarkerKey::Group(level, cluster.key),
    }
}

pub struct MapView {
    /// The map beside or above the content stack; the map half hides outside map views.
    pub split: gtk::Paned,
    pub root: libadwaita::ToastOverlay,
    pub(super) map: shumate::SimpleMap,
    layer: shumate::MarkerLayer,
    here_layer: shumate::MarkerLayer,
    status: libadwaita::StatusPage,
    resize_watch: gtk::DrawingArea,
    pub(super) controls: MapControls,
    pub tiles: Rc<Tiles>,
    /// Markers are loaded; cleared after location edits so the next visit reloads.
    pub populated: Cell<bool>,
    pub(super) mode: Cell<Mode>,
    /// The sidebar was hidden to make room for the photos and comes back with the map alone.
    pub(super) sidebar_restore: Cell<bool>,
    pub(super) minimap: RefCell<Option<Minimap>>,
    loading: Cell<bool>,
    state: RefCell<ViewState>,
    refresh_pending: RefCell<Option<glib::SourceId>>,
}

#[derive(Default)]
struct ViewState {
    markers: Arc<Vec<MapMarker>>,
    /// Bumped whenever the markers reload, so clusters computed for older ones are dropped.
    generation: u64,
    clusters: HashMap<u32, Arc<Vec<Cluster>>>,
    computing: HashSet<(u64, u32)>,
    shown: HashMap<MarkerKey, shumate::Marker>,
    camera_placed: bool,
    focus: Option<GeoPoint>,
    /// A marker in the cluster whose photos are listed, so that cluster stays highlighted.
    selected: Option<usize>,
}

enum Status<'a> {
    Loading,
    Empty,
    Error(&'a str),
    Hidden,
}

pub fn build_map_view() -> MapView {
    let map = shumate::SimpleMap::builder()
        .show_zoom_buttons(false)
        .vexpand(true)
        .hexpand(true)
        .build();
    if let Some(compass) = map.compass() {
        compass.set_visible(false);
    }
    let viewport = map.viewport().expect("SimpleMap creates its viewport");
    let here_layer = shumate::MarkerLayer::new(&viewport);
    let layer = shumate::MarkerLayer::new(&viewport);
    map.add_overlay_layer(&here_layer);
    map.add_overlay_layer(&layer);
    let (status, resize_watch) = (status_page(), resize_watch());
    let controls = controls::build(&viewport);
    let overlay = gtk::Overlay::builder().child(&map).build();
    overlay.add_overlay(&resize_watch);
    overlay.add_overlay(&status);
    overlay.add_overlay(&controls.root);
    let root = libadwaita::ToastOverlay::new();
    root.set_child(Some(&overlay));
    root.set_visible(false);
    MapView {
        split: split_with(&root),
        root,
        map,
        layer,
        here_layer,
        status,
        resize_watch,
        controls,
        tiles: Tiles::new(),
        populated: Cell::new(false),
        mode: Cell::default(),
        sidebar_restore: Cell::new(false),
        minimap: RefCell::default(),
        loading: Cell::new(false),
        state: RefCell::default(),
        refresh_pending: RefCell::default(),
    }
}

fn split_with(map: &libadwaita::ToastOverlay) -> gtk::Paned {
    gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(map)
        .resize_start_child(true)
        .resize_end_child(false)
        .shrink_start_child(false)
        .shrink_end_child(false)
        .wide_handle(true)
        .vexpand(true)
        .build()
}

fn status_page() -> libadwaita::StatusPage {
    libadwaita::StatusPage::builder()
        .icon_name("mark-location-symbolic")
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .can_target(false)
        .css_classes(["card", "compact", "mimick-map-status"])
        .visible(false)
        .build()
}

/// An empty overlay whose `resize` signal reports the map's size changes.
fn resize_watch() -> gtk::DrawingArea {
    gtk::DrawingArea::builder().can_target(false).build()
}

/// Recluster as the map moves or resizes, and follow the theme and the saved style.
pub fn connect_map_view(ui: &Rc<LibraryWindowUi>) {
    let saved = MapStyle::from_id(&ui.ctx.config.read().data.library_map_style);
    ui.map.tiles.preset(saved);
    camera::hold_in_world(&ui.map.map);
    if let Some(viewport) = ui.map.map.viewport() {
        let weak = Rc::downgrade(ui);
        viewport.connect_changed(move |_| {
            if let Some(ui) = weak.upgrade() {
                controls::sync_north(&ui);
                schedule_refresh(&ui);
            }
        });
    }
    let weak = Rc::downgrade(ui);
    ui.map.resize_watch.connect_resize(move |_, _, height| {
        if let Some(ui) = weak.upgrade() {
            controls::fit_height(&ui, height);
            camera::keep_in_world(&ui.map.map);
            schedule_refresh(&ui);
        }
    });
    let weak = Rc::downgrade(ui);
    libadwaita::StyleManager::default().connect_dark_notify(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.map.tiles.apply();
        }
    });
    controls::connect(ui);
    super::layout::connect(ui);
}

pub(super) fn show(ui: &Rc<LibraryWindowUi>) {
    ui.map.tiles.attach(ui, &ui.map.map);
    if !ui.map.populated.get() {
        load(ui.clone());
    }
}

/// Centre the map on `point`, now or once the markers have loaded.
pub(super) fn focus(ui: &Rc<LibraryWindowUi>, point: GeoPoint) {
    let view = &ui.map;
    if !view.populated.get() {
        view.state.borrow_mut().focus = Some(point);
        return;
    }
    if let Some(map) = view.map.map() {
        map.go_to_full(point.lat, point.lon, FOCUS_ZOOM);
    }
}

/// Open every photo on screen beside the map; `false` when there are none.
pub(super) fn open_visible(ui: &Rc<LibraryWindowUi>) -> bool {
    let Some(current) = camera::view_of(&ui.map.map) else {
        return false;
    };
    let markers = ui.map.state.borrow().markers.clone();
    let members = cluster::markers_in_view(&markers, &current);
    if members.is_empty() {
        return false;
    }
    open_members(ui, &markers, &members, Some("This Area".to_string()));
    true
}

/// Mark `point` as the user's position and fly there.
pub(super) fn show_here(ui: &Rc<LibraryWindowUi>, point: GeoPoint) {
    ui.map.here_layer.remove_all();
    ui.map.here_layer.add_marker(&markers::here(point));
    if let Some(map) = ui.map.map.map() {
        map.go_to_full(point.lat, point.lon, FOCUS_ZOOM);
    }
}

pub(super) fn clear_selection(ui: &Rc<LibraryWindowUi>) {
    if ui.map.state.borrow().selected.is_some() {
        select(ui, None);
    }
}

/// Highlight the cluster holding `anchor`; markers are rebuilt once the click that chose it is over.
fn select(ui: &Rc<LibraryWindowUi>, anchor: Option<usize>) {
    ui.map.state.borrow_mut().selected = anchor;
    let weak = Rc::downgrade(ui);
    glib::idle_add_local_once(move || {
        if let Some(ui) = weak.upgrade() {
            clear_shown(&ui.map);
            refresh_markers(&ui);
        }
    });
}

fn load(ui: Rc<LibraryWindowUi>) {
    if ui.map.loading.replace(true) {
        return;
    }
    show_status(&ui.map, Status::Loading);
    glib::MainContext::default().spawn_local(async move {
        let result = ui.ctx.api_client.fetch_map_markers().await;
        ui.map.loading.set(false);
        match result {
            Ok(markers) => apply_markers(&ui, markers),
            Err(err) => show_status(&ui.map, Status::Error(&err)),
        }
    });
}

fn apply_markers(ui: &Rc<LibraryWindowUi>, markers: Vec<MapMarker>) {
    let view = &ui.map;
    clear_shown(view);
    let empty = markers.is_empty();
    {
        let mut state = view.state.borrow_mut();
        state.markers = Arc::new(markers);
        state.generation += 1;
        state.clusters.clear();
        state.computing.clear();
        state.selected = None;
    }
    view.populated.set(true);
    show_status(view, if empty { Status::Empty } else { Status::Hidden });
    place_camera(view);
    refresh_markers(ui);
}

/// A pending focus wins; otherwise fit all markers the first time they load.
fn place_camera(view: &MapView) {
    let (Some(map), Some(viewport)) = (view.map.map(), view.map.viewport()) else {
        return;
    };
    let mut state = view.state.borrow_mut();
    let target = match state.focus.take() {
        Some(point) => Some((point.lat, point.lon, FOCUS_ZOOM)),
        None if !state.camera_placed => {
            let size = camera::view_of(&view.map).map(|v| (v.width, v.height));
            let (width, height) = size.unwrap_or(FALLBACK_SIZE);
            cluster::fit(&state.markers, width, height)
        }
        None => None,
    };
    if let Some((lat, lon, zoom)) = target {
        viewport.set_zoom_level(zoom);
        map.center_on(lat, lon);
        state.camera_placed = true;
    }
}

fn show_status(view: &MapView, status: Status<'_>) {
    let (title, description) = match status {
        Status::Loading => ("Loading Map…", String::new()),
        Status::Empty => (
            "No Photos With a Location",
            "Photos with GPS data, or a location you set, appear here".to_string(),
        ),
        Status::Error(err) => ("Could Not Load the Map", err.to_string()),
        Status::Hidden => {
            view.status.set_visible(false);
            return;
        }
    };
    view.status.set_title(title);
    view.status
        .set_description((!description.is_empty()).then_some(description.as_str()));
    view.status.set_visible(true);
}

/// At most one refresh per delay, so markers keep up while the map is dragged.
fn schedule_refresh(ui: &Rc<LibraryWindowUi>) {
    if ui.map.refresh_pending.borrow().is_some() {
        return;
    }
    let weak = Rc::downgrade(ui);
    let id = glib::timeout_add_local_once(REFRESH_DELAY, move || {
        if let Some(ui) = weak.upgrade() {
            ui.map.refresh_pending.take();
            refresh_markers(&ui);
        }
    });
    ui.map.refresh_pending.replace(Some(id));
}

/// Show the clusters inside the viewport, adding and removing only what changed.
/// The previous markers stay up until the clusters for a new zoom level are ready.
fn refresh_markers(ui: &Rc<LibraryWindowUi>) {
    let view = &ui.map;
    let Some(current) = camera::view_of(&view.map) else {
        return;
    };
    if view.state.borrow().markers.is_empty() {
        return;
    }
    let (level, window) = PixelWindow::around(&current);
    let cached = view.state.borrow().clusters.get(&level).cloned();
    if cached.is_none() {
        request_clusters(ui, level);
    }
    request_clusters(ui, level.saturating_sub(1));
    request_clusters(ui, cluster::cluster_level(f64::from(level) + 1.0));
    let Some(clusters) = cached else {
        return;
    };
    let visible: HashMap<MarkerKey, usize> = clusters
        .iter()
        .enumerate()
        .filter(|(_, cluster)| window.contains(cluster.x, cluster.y))
        .map(|(index, cluster)| (marker_key(level, cluster), index))
        .collect();
    remove_hidden(view, &visible);
    add_missing(ui, &clusters, &visible);
}

/// Cluster `level` on a worker thread unless it is cached or already being computed.
fn request_clusters(ui: &Rc<LibraryWindowUi>, level: u32) {
    let (markers, generation) = {
        let mut state = ui.map.state.borrow_mut();
        let generation = state.generation;
        if state.clusters.contains_key(&level) || !state.computing.insert((generation, level)) {
            return;
        }
        (state.markers.clone(), generation)
    };
    let weak = Rc::downgrade(ui);
    glib::MainContext::default().spawn_local(async move {
        let computed = tokio::task::spawn_blocking(move || cluster::cluster(&markers, level))
            .await
            .ok();
        if let Some(ui) = weak.upgrade() {
            store_clusters(&ui, generation, level, computed);
        }
    });
}

fn store_clusters(
    ui: &Rc<LibraryWindowUi>,
    generation: u64,
    level: u32,
    computed: Option<Vec<Cluster>>,
) {
    {
        let mut state = ui.map.state.borrow_mut();
        state.computing.remove(&(generation, level));
        let Some(clusters) = computed.filter(|_| state.generation == generation) else {
            return;
        };
        evict_far_levels(&mut state.clusters, level);
        state.clusters.insert(level, Arc::new(clusters));
    }
    refresh_markers(ui);
}

/// Keep the cache to a few zoom levels, dropping those farthest from `level` first.
fn evict_far_levels(cache: &mut HashMap<u32, Arc<Vec<Cluster>>>, level: u32) {
    while cache.len() >= CACHED_LEVELS {
        let Some(farthest) = cache.keys().copied().max_by_key(|l| l.abs_diff(level)) else {
            break;
        };
        cache.remove(&farthest);
    }
}

fn clear_shown(view: &MapView) {
    view.layer.remove_all();
    view.state.borrow_mut().shown.clear();
}

fn remove_hidden(view: &MapView, visible: &HashMap<MarkerKey, usize>) {
    view.state.borrow_mut().shown.retain(|key, marker| {
        let keep = visible.contains_key(key);
        if !keep {
            view.layer.remove_marker(marker);
        }
        keep
    });
}

fn add_missing(
    ui: &Rc<LibraryWindowUi>,
    clusters: &Arc<Vec<Cluster>>,
    visible: &HashMap<MarkerKey, usize>,
) {
    let view = &ui.map;
    let (markers, selected, missing) = {
        let state = view.state.borrow();
        let missing: Vec<(MarkerKey, usize)> = visible
            .iter()
            .filter(|(key, _)| !state.shown.contains_key(*key))
            .map(|(key, index)| (*key, *index))
            .collect();
        (state.markers.clone(), state.selected, missing)
    };
    for (key, index) in missing {
        let marker = cluster_marker(ui, &markers, clusters, index, selected);
        view.layer.add_marker(&marker);
        view.state.borrow_mut().shown.insert(key, marker);
    }
}

fn cluster_marker(
    ui: &Rc<LibraryWindowUi>,
    markers: &Arc<Vec<MapMarker>>,
    clusters: &Arc<Vec<Cluster>>,
    index: usize,
    selected: Option<usize>,
) -> shumate::Marker {
    let cluster = &clusters[index];
    let (button, members) = match cluster.members.as_slice() {
        [single] => (
            markers::photo(ui, &markers[*single].id),
            Members::Photo([*single]),
        ),
        group => (
            markers::bubble(group.len()),
            Members::Cluster(clusters.clone(), index),
        ),
    };
    if selected.is_some_and(|anchor| cluster.members.contains(&anchor)) {
        button.add_css_class("selected");
    }
    markers::place_tooltip(&button, markers.clone(), members.clone());
    let (weak, markers) = (Rc::downgrade(ui), markers.clone());
    button.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            open_members(&ui, &markers, members.as_slice(), None);
        }
    });
    let marker = shumate::Marker::new();
    marker.set_child(Some(&button));
    marker.set_location(cluster.lat, cluster.lon);
    marker
}

/// List `members` beside the map, titled `label` or after their most common place.
fn open_members(
    ui: &Rc<LibraryWindowUi>,
    markers: &[MapMarker],
    members: &[usize],
    label: Option<String>,
) {
    let ids = members.iter().map(|&i| markers[i].id.clone()).collect();
    let label = label
        .or_else(|| cluster::area_label(markers, members))
        .unwrap_or_else(|| "Map Area".to_string());
    select(ui, members.first().copied());
    navigation::navigate_source(
        ui,
        LibrarySource::MapArea {
            ids: AssetIds(ids),
            label,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cluster_cache_drops_the_farthest_levels() {
        let mut cache: HashMap<u32, Arc<Vec<Cluster>>> = [2, 4, 5, 6, 7]
            .into_iter()
            .map(|l| (l, Arc::default()))
            .collect();
        evict_far_levels(&mut cache, 8);
        let mut kept: Vec<u32> = cache.keys().copied().collect();
        kept.sort_unstable();
        assert_eq!(kept, [4, 5, 6, 7]);
    }

    #[test]
    fn photos_keep_their_marker_across_zoom_levels() {
        let photo = Cluster {
            key: (1, 1),
            x: 0.0,
            y: 0.0,
            lat: 0.0,
            lon: 0.0,
            members: vec![7],
        };
        let group = Cluster {
            members: vec![7, 8],
            ..photo.clone()
        };
        assert_eq!(marker_key(3, &photo), marker_key(9, &photo));
        assert_ne!(marker_key(3, &group), marker_key(4, &group));
    }
}
