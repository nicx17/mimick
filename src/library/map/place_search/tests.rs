use super::*;

fn photon(kind: &str, mut properties: serde_json::Value) -> PhotonPlace {
    properties["type"] = kind.into();
    serde_json::from_value(serde_json::json!({
        "geometry": { "coordinates": [-9.137, 38.71] },
        "properties": properties,
    }))
    .unwrap()
}

#[test]
fn photon_results_are_named_and_described() {
    let house = from_photon(photon(
        "house",
        serde_json::json!({ "housenumber": "100", "street": "Rua Augusta",
            "district": "Baixa", "city": "Lisbon", "country": "Portugal" }),
    ))
    .unwrap();
    assert_eq!(house.name, "Rua Augusta 100");
    assert_eq!(house.detail, "Address · Baixa, Lisbon, Portugal");
    assert_eq!(house.point, GeoPoint::new(38.71, -9.137).unwrap());

    let city = from_photon(photon(
        "city",
        serde_json::json!({ "name": "Lisbon", "osm_value": "city",
            "state": "Lisbon", "country": "Portugal" }),
    ))
    .unwrap();
    assert_eq!(city.detail, "City · Portugal");

    let museum = photon(
        "house",
        serde_json::json!({ "name": "MUDE", "osm_value": "museum" }),
    );
    assert_eq!(from_photon(museum).unwrap().detail, "Museum");
    assert_eq!(from_photon(photon("other", serde_json::json!({}))), None);
}

#[test]
fn kinds_prefer_the_specific_osm_value() {
    assert_eq!(
        kind_label("other", Some("train_station"), true),
        "Train station"
    );
    assert_eq!(kind_label("street", Some("residential"), true), "Street");
    assert_eq!(kind_label("city", Some("administrative"), true), "City");
    assert_eq!(kind_label("country", Some("country"), true), "Country");
    assert_eq!(kind_label("house", Some("house"), false), "Address");
}

#[test]
fn immich_places_keep_their_region() {
    let place = |name: &str, admin2: Option<&str>, admin1: Option<&str>| Place {
        name: name.into(),
        latitude: 0.0,
        longitude: 0.0,
        admin1name: admin1.map(Into::into),
        admin2name: admin2.map(Into::into),
    };
    let detail = |p: Place| from_immich(p).unwrap().detail;
    assert_eq!(
        detail(place("Sintra", Some("Lisboa"), Some("Lisbon"))),
        "Lisboa, Lisbon"
    );
    assert_eq!(detail(place("Porto", Some("Porto"), Some("Porto"))), "");
    assert_eq!(
        detail(place("Faro", None, Some("Faro District"))),
        "Faro District"
    );
    assert_eq!(detail(place("X", Some(" "), None)), "");
}

#[test]
fn zoom_fits_the_place_within_its_kind() {
    let mut lisbon = from_photon(photon("city", serde_json::json!({ "name": "Lisbon" }))).unwrap();
    assert_eq!(lisbon.zoom_for(1000.0, 800.0, 256.0), 12.0);
    assert_eq!(lisbon.zoom_for(1000.0, 800.0, 512.0), 11.0);
    lisbon.bounds = Some(Bounds {
        west: -9.23,
        north: 38.80,
        east: -9.09,
        south: 38.69,
    });
    let fitted = lisbon.zoom_for(1000.0, 800.0, 256.0);
    assert!((12.0..=13.0).contains(&fitted), "{fitted}");
    let mut france =
        from_photon(photon("country", serde_json::json!({ "name": "France" }))).unwrap();
    france.bounds = Some(Bounds {
        west: -178.38,
        north: 51.30,
        east: 172.30,
        south: -50.21,
    });
    assert_eq!(france.zoom_for(1000.0, 800.0, 256.0), 4.0);
}

#[test]
fn crossing_the_antimeridian_keeps_a_small_span() {
    let fiji = Bounds {
        west: 177.0,
        north: -16.0,
        east: -179.0,
        south: -19.0,
    };
    assert!(fit_zoom(fiji, 1000.0, 800.0).unwrap() > 6.0);
    let point = Bounds {
        west: 1.0,
        north: 2.0,
        east: 1.0,
        south: 2.0,
    };
    assert_eq!(fit_zoom(point, 1000.0, 800.0), None);
}

#[test]
fn language_follows_the_system_where_photon_has_names() {
    assert_eq!(pick_language(["de_AT.UTF-8", "de_AT", "de", "C"]), "de");
    assert_eq!(pick_language(["fr@euro", "C"]), "fr");
    assert_eq!(pick_language(["pt_PT", "pt", "C"]), "en");
}

#[test]
fn cache_keys_ignore_case_and_small_moves() {
    let bias = |lat| SearchBias {
        lat,
        lon: -9.14,
        zoom: 11,
    };
    assert_eq!(
        cache_key(" Lisbon ", Some(bias(38.72))),
        cache_key("lisbon", Some(bias(38.74)))
    );
    assert_ne!(
        cache_key("lisbon", Some(bias(38.72))),
        cache_key("lisbon", Some(bias(40.0)))
    );
    assert_eq!(cache_key("Lisbon", None), "lisbon");
}
