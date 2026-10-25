//! The computer's position from the XDG Location portal, which asks the user for consent.

use std::time::Duration;

use ashpd::WindowIdentifier;
use ashpd::desktop::location::{Accuracy, CreateSessionOptions, LocationProxy};
use futures_util::StreamExt;
use gtk::prelude::*;

use super::coords::GeoPoint;

const LOCATE_TIMEOUT: Duration = Duration::from_secs(20);

pub async fn current_position(window: &impl IsA<gtk::Window>) -> Result<GeoPoint, String> {
    tokio::time::timeout(LOCATE_TIMEOUT, request(window))
        .await
        .map_err(|_| "The location service did not answer in time.".to_string())?
}

async fn request(window: &impl IsA<gtk::Window>) -> Result<GeoPoint, String> {
    let proxy = LocationProxy::new().await.map_err(unavailable)?;
    let session = proxy
        .create_session(CreateSessionOptions::default().set_accuracy(Accuracy::Street))
        .await
        .map_err(unavailable)?;
    let mut updates = proxy
        .receive_location_updated()
        .await
        .map_err(unavailable)?;
    let identifier = match window.as_ref().native() {
        Some(native) => WindowIdentifier::from_native(&native).await,
        None => None,
    };
    proxy
        .start(&session, identifier.as_ref(), Default::default())
        .await
        .map_err(unavailable)?
        .response()
        .map_err(|err| format!("Location access was not allowed ({err})."))?;
    let location = updates.next().await;
    let _ = session.close().await;
    let location = location.ok_or("The location service stopped before finding you.")?;
    GeoPoint::new(location.latitude(), location.longitude())
        .ok_or_else(|| "The location service returned an invalid position.".to_string())
}

fn unavailable(err: ashpd::Error) -> String {
    format!("Location services are unavailable ({err}). Check that they are turned on.")
}
