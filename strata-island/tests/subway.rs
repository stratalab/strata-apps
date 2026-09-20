use std::collections::BTreeSet;
use strata_island::{
    extract, places, store, subway,
    world::{OpenArgs, World},
};

#[test]
fn ready_catalog_still_rejects_mismatched_subway_data() {
    let mut db = store::open_cache(None).unwrap();
    let roads = extract::parse_city(include_str!("../fixtures/manhattan-drive.json")).unwrap();
    let aliases = extract::parse_gazetteer(include_str!("../fixtures/gazetteer.json")).unwrap();
    store::import_city(&mut db, &roads, &aliases).unwrap();
    places::import_on(&db, "city").unwrap();
    places::import_on(&db, "city").unwrap();
    store::write_json(
        &db,
        "city",
        "ready:subway-v1",
        serde_json::json!({"hash":"mismatched"}),
    )
    .unwrap();
    assert!(places::import_on(&db, "city").is_err());
}

#[test]
fn persisted_subway_matches_pin_and_bfs_is_independently_verified() {
    let world = World::open_v2(OpenArgs {
        cache: true,
        db_path: "unused".into(),
        memory_budget_bytes: None,
    })
    .unwrap();
    let pin: serde_json::Value = serde_json::from_str(subway::FIXTURE).unwrap();
    let actual = world.subway("city", None).unwrap();
    let edges = |v: &serde_json::Value| {
        v["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["source"].as_str().unwrap().to_owned(),
                    e["target"].as_str().unwrap().to_owned(),
                    e["kind"].as_str().unwrap().to_owned(),
                )
            })
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(edges(&actual), edges(&pin));
    assert_eq!(actual["stations"], pin["stations"]);
    let places = world.place_snapshot("city").unwrap();
    for station in pin["stations"].as_array().unwrap() {
        let p = places
            .places
            .iter()
            .find(|p| p.id == station["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(p.category, "transit");
        let metadata = p.subway.as_ref().unwrap();
        assert_eq!(metadata.station_id, station["station_id"]);
        if metadata.station_id == "151" || metadata.station_id == "167" {
            assert_eq!(metadata.gtfs_stop_ids.len(), 2);
        }
        let detail = world.place_detail("city", &p.id).unwrap();
        assert_eq!(detail["binding"]["target"]["key"], p.id);
    }
    for seed in [
        "p:mta:station:151",
        "p:mta:station:222",
        "p:mta:station:330",
    ] {
        let mut expected = BTreeSet::from([seed.to_owned()]);
        for _ in 0..2 {
            let next = edges(&pin)
                .into_iter()
                .filter(|(s, _, _)| expected.contains(s))
                .map(|(_, d, _)| d)
                .collect::<Vec<_>>();
            expected.extend(next);
        }
        let result = world.subway("city", Some((seed, 2))).unwrap();
        let reached = result["stations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(reached, expected);
    }
    assert!(world
        .subway("city", Some(("p:mta:station:151", 7)))
        .is_err());
    assert!(world.subway("city", Some(("not-a-station", 2))).is_err());
    let desk = world.close_42nd(None).unwrap().desk;
    assert_eq!(edges(&world.subway(&desk, None).unwrap()), edges(&actual));
    assert_eq!(
        world.place_snapshot(&desk).unwrap().roads.edge_count() + 8,
        places.roads.edge_count()
    );
}
