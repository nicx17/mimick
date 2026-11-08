//! Map page, details-pane mini map, and the Set Location dialog, drawn with libshumate.

mod album;
mod camera;
mod cluster;
mod controls;
mod coords;
mod layout;
mod locate;
mod location_dialog;
mod markers;
mod minimap;
mod place_search;
mod satellite;
mod search;
mod settings;
mod square;
mod status;
mod style;
mod styles;
mod tiles;
mod view;

pub use album::{album_shown, show_album};
pub use coords::GeoPoint;
pub use layout::{photos_full, set_photos_full, sync as sync_layout};
pub use location_dialog::{LocationTarget, set_location, set_location_for_asset};
pub use minimap::attach as attach_minimap;
pub use view::{MapView, build_map_view, connect_map_view};
