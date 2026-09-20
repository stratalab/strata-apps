use std::sync::Arc;
use strata_island::{
    extract::{ClosureEdge, ClosureFixture},
    places,
    world::{OpenArgs, World},
};
fn cache() -> World {
    World::open_v2(OpenArgs {
        cache: true,
        db_path: "unused".into(),
        memory_budget_bytes: None,
    })
    .unwrap()
}
#[test]
fn curated_catalog_graph_queries_and_scenarios() {
    let world = cache();
    let city = world.place_snapshot("city").unwrap();
    assert_eq!(city.places.len(), 1852);
    assert_eq!(city.places.iter().flat_map(|p| p.aliases.iter()).count(), 6);
    assert_eq!(
        city.places.iter().filter(|p| p.node.is_some()).count(),
        1814
    );
    let road = world.city_index();
    let origin = world.lookup_node(&road, "poi:port-authority").unwrap();
    let discovery = places::discover(&city, "city", "poi:port-authority", None, 100_000).unwrap();
    for result in discovery["results"].as_array().unwrap() {
        let dest = world
            .lookup_node(&road, result["place"]["id"].as_str().unwrap())
            .unwrap();
        let path = world.route(&road, origin, dest).unwrap();
        assert_eq!(result["distance_m"].as_f64(), Some(path.length_m as f64));
    }
    let selected = &city.places[0];
    let detail = world.place_detail("city", &selected.id).unwrap();
    assert_eq!(detail["binding"]["target"]["key"], selected.id);
    let explored = places::explore(&city, "city", &selected.id, 2, 5).unwrap();
    assert!(explored["nodes"].as_array().unwrap().len() <= 5);
    assert!(explored["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["relation"].as_str().unwrap().ends_with("in_category")));
    assert!(places::explore(&city, "city", &selected.id, 4, 100).is_err());
    let closed = world.close_42nd(None).unwrap();
    let child = world.place_snapshot(&closed.desk).unwrap();
    assert_eq!(child.places.as_ref(), city.places.as_ref());
    assert_eq!(child.roads.edge_count() + 8, city.roads.edge_count());
    let impact = places::impact(&city, &child, &closed.desk, "poi:port-authority").unwrap();
    assert_eq!(impact["results"].as_array().unwrap().len(), 1852);
    assert!(impact["results"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["status"] == "farther"));
    let history = world.scenario_history(&closed.desk).unwrap();
    let before = world
        .historical_places(&closed.desk, history["parent_version"].as_u64().unwrap())
        .unwrap();
    assert_eq!(before.roads.edge_count(), city.roads.edge_count());
    let state = world
        .scenario_operation(&closed.desk, "reopen-1", false, child.version.as_u64())
        .unwrap();
    let reopened = world.place_snapshot(&closed.desk).unwrap();
    assert_eq!(reopened.roads.edge_count(), city.roads.edge_count());
    assert_eq!(
        world
            .scenario_operation(&closed.desk, "reopen-1", false, child.version.as_u64())
            .unwrap(),
        state
    );
    assert!(world
        .scenario_operation(&closed.desk, "other", true, child.version.as_u64())
        .is_err());
    let past = world
        .historical_places(&closed.desk, child.version.as_u64())
        .unwrap();
    assert_eq!(past.roads.edge_count(), child.roads.edge_count());
    assert!(Arc::ptr_eq(&world.city_index(), &road));
    assert_eq!(world.place_snapshot("city").unwrap().version, city.version);
    world.archive(&closed.desk).unwrap();
    assert!(world.place_snapshot(&closed.desk).is_err());
}
#[test]
fn custom_closure_and_concurrent_admission() {
    let world = Arc::new(cache());
    let edge = &world.extract().edges[100];
    let closure = ClosureFixture {
        name: "Single directed segment".into(),
        between: vec![],
        edges: vec![ClosureEdge {
            src: edge.src.clone(),
            dst: edge.dst.clone(),
            edge_type: "street".into(),
        }],
    };
    let custom = world.create_closure(None, &closure).unwrap();
    assert_eq!(
        world
            .place_snapshot(&custom.desk)
            .unwrap()
            .roads
            .edge_count()
            + 1,
        world.place_snapshot("city").unwrap().roads.edge_count()
    );
    let results = std::thread::scope(|scope| {
        let tasks: Vec<_> = (0..4)
            .map(|_| {
                let world = Arc::clone(&world);
                scope.spawn(move || world.close_42nd(None))
            })
            .collect();
        tasks
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 2);
    assert_eq!(world.branch_views().len(), 4);
}
#[test]
fn v2_durable_restart_and_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v2");
    let args = || OpenArgs {
        cache: false,
        db_path: path.clone(),
        memory_budget_bytes: None,
    };
    let world = World::open_v2(args()).unwrap();
    let desk = world.close_42nd(None).unwrap().desk;
    let version = world.place_snapshot(&desk).unwrap().version;
    world
        .scenario_operation(&desk, "reopen", false, version.as_u64())
        .unwrap();
    drop(world);
    assert!(World::open(args()).is_err());
    let reopened = World::open_v2(args()).unwrap();
    assert_eq!(
        reopened.place_snapshot(&desk).unwrap().roads.edge_count(),
        reopened.place_snapshot("city").unwrap().roads.edge_count()
    );
    let old = reopened.historical_places(&desk, version.as_u64()).unwrap();
    assert_eq!(
        old.roads.edge_count() + 8,
        reopened.place_snapshot("city").unwrap().roads.edge_count()
    );
    assert!(reopened.verify_desk_chain(&desk).unwrap());
}

#[test]
fn expanded_fixture_preserves_all_original_places() {
    let old: serde_json::Value = serde_json::from_str(places::OSM_FIXTURE).unwrap();
    let new = places::fixture().unwrap();
    for record in old["places"].as_array().unwrap() {
        let original: places::Place = serde_json::from_value(record.clone()).unwrap();
        assert!(new.contains(&original));
    }
    assert_eq!(
        new.iter().filter(|p| p.category == "landmark").count(),
        1674
    );
    assert_eq!(new.iter().filter(|p| p.category == "park").count(), 14);
    assert_eq!(new.iter().filter(|p| p.category == "transit").count(), 164);
}
