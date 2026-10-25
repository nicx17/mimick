//! Keeps a map on one copy of the world vertically, and turns it with a short animation.

use std::f64::consts::{PI, TAU};

use gtk::prelude::*;
use libadwaita::prelude::*;
use shumate::prelude::*;

use super::cluster::{self, View};

/// One press of a rotate button.
pub const ROTATE_STEP: f64 = PI / 4.0;
const ROTATE_MS: u32 = 250;

/// What `map` shows right now; `None` before it has a size.
pub fn view_of(map: &shumate::SimpleMap) -> Option<View> {
    let viewport = map.viewport()?;
    let (width, height) = (map.width(), map.height());
    (width > 0 && height > 0).then(|| View {
        lat: viewport.latitude(),
        lon: viewport.longitude(),
        zoom: viewport.zoom_level(),
        width: f64::from(width),
        height: f64::from(height),
        rotation: viewport.rotation(),
    })
}

/// Zoom in and shift the centre just enough that the world fills the map's height,
/// since libshumate repeats it vertically once it is shorter than the map.
pub fn keep_in_world(map: &shumate::SimpleMap) {
    let (Some(view), Some(viewport), Some(inner)) = (view_of(map), map.viewport(), map.map())
    else {
        return;
    };
    let (_, tall) = view.extent();
    let min_zoom = cluster::min_zoom_for(tall);
    viewport.set_min_zoom_level(min_zoom.floor() as u32);
    if view.zoom < min_zoom {
        viewport.set_zoom_level(min_zoom);
    }
    let lat = cluster::clamp_centre_lat(view.lat, viewport.zoom_level(), tall);
    if (lat - view.lat).abs() > 1e-9 {
        inner.center_on(lat, view.lon);
    }
}

/// Apply `keep_in_world` whenever the map moves, zooms, or turns.
pub fn hold_in_world(map: &shumate::SimpleMap) {
    let Some(viewport) = map.viewport() else {
        return;
    };
    let weak = map.downgrade();
    viewport.connect_changed(move |_| {
        if let Some(map) = weak.upgrade() {
            keep_in_world(&map);
        }
    });
}

pub fn rotate_by(map: &shumate::SimpleMap, delta: f64) {
    if let Some(viewport) = map.viewport() {
        animate_rotation(map, viewport.rotation() + delta);
    }
}

pub fn reset_north(map: &shumate::SimpleMap) {
    if let Some(viewport) = map.viewport() {
        animate_rotation(map, north_target(viewport.rotation()));
    }
}

/// Turn back to north the short way round; libshumate keeps rotation in `0..TAU`.
fn north_target(rotation: f64) -> f64 {
    if rotation.rem_euclid(TAU) > PI {
        TAU
    } else {
        0.0
    }
}

fn animate_rotation(map: &shumate::SimpleMap, target: f64) {
    let Some(viewport) = map.viewport() else {
        return;
    };
    let from = viewport.rotation();
    let setter = viewport.clone();
    let animation = libadwaita::TimedAnimation::new(
        map,
        from,
        target,
        ROTATE_MS,
        libadwaita::CallbackAnimationTarget::new(move |value| setter.set_rotation(value)),
    );
    animation.set_easing(libadwaita::Easing::EaseOutCubic);
    animation.play();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn north_is_reached_the_short_way() {
        assert_eq!(north_target(0.5), 0.0);
        assert_eq!(north_target(5.5), TAU);
        assert_eq!(north_target(-0.5), TAU);
    }
}
