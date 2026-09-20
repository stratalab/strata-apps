use serde_json::{json, Value};
use std::sync::Arc;
use strata_island::{
    journeys::{Catalog, Data},
    store,
};
use stratadb::graph::*;
fn synthetic() -> Catalog {
    let mut nodes = vec![];
    let mut edges = vec![];
    for i in 0..3 {
        let station = format!("p:mta:station:{i}");
        nodes.push(
            json!({"id":station,"x":i*3000,"y":0,"kind":"station","name":format!("Station {i}")}),
        );
        nodes.push(json!({"id":format!("w:{i}"),"x":i*3000,"y":0,"kind":"walk"}));
        for (a, b) in [
            (format!("w:{i}"), station.clone()),
            (station, format!("w:{i}")),
        ] {
            edges.push(
                json!({"source":a,"target":b,"kind":"access","seconds":1,"meters":0,"points":[]}),
            );
        }
    }
    for i in 0..2 {
        let a = format!("p:mta:station:{i}");
        let b = format!("p:mta:station:{}", i + 1);
        let src = format!("t:{i}:a");
        let dst = format!("t:{i}:b");
        nodes.push(json!({"id":src,"x":i*3000,"y":0,"kind":"train"}));
        nodes.push(json!({"id":dst,"x":(i+1)*3000,"y":0,"kind":"train"}));
        edges.push(json!({"source":a,"target":src,"kind":"board","seconds":360,"meters":0,"route":format!("{i}"),"headsign":"Uptown","station":a}));
        edges.push(json!({"source":src,"target":dst,"kind":"ride","seconds":120,"meters":3000,"route":format!("{i}"),"from_station":a,"to_station":b,"points":[[i*3000,0],[(i+1)*3000,0]]}));
        edges.push(
            json!({"source":dst,"target":b,"kind":"alight","seconds":60,"meters":0,"station":b}),
        );
        for (s, d) in [(i, i + 1), (i + 1, i)] {
            edges.push(json!({"source":format!("w:{s}"),"target":format!("w:{d}"),"kind":"walk","seconds":2500,"meters":3000,"points":[[s*3000,0],[d*3000,0]]}));
        }
    }
    let data:Data=serde_json::from_value(json!({"nodes":nodes,"edges":edges,"routes":[],"reference_date":"2026-09-21","semantics":"test"})).unwrap();
    let db = store::open_cache(None).unwrap();
    let mut graph = db
        .graph(store::branch("default").unwrap(), store::space().unwrap())
        .unwrap();
    let name = GraphName::new("test-journeys").unwrap();
    graph.create_graph(name.clone()).unwrap();
    let nodes = data
        .nodes
        .iter()
        .map(|n| (GraphNodeId::new(&n.id).unwrap(), GraphNodeData::default()))
        .collect::<Vec<_>>();
    let edges = data
        .edges
        .iter()
        .map(|e| {
            (
                GraphNodeId::new(&e.source).unwrap(),
                GraphEdgeType::new(&e.kind).unwrap(),
                GraphNodeId::new(&e.target).unwrap(),
                GraphEdgeData::new(e.seconds as f64, None).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    graph.bulk_insert(&name, &nodes, &edges, Some(512)).unwrap();
    let snapshot = graph
        .adjacency_index(&name, &GraphAnalyticsBudget::default())
        .unwrap();
    Catalog::from_graph(snapshot, Arc::new(data)).unwrap()
}
#[test]
fn transfers_have_boarding_cost_and_direction_is_respected() {
    let c = synthetic();
    let r = c
        .route(
            "city",
            1,
            Some("p:mta:station:0"),
            Some("p:mta:station:2"),
            [0, 0],
            [6000, 0],
        )
        .unwrap();
    assert_eq!(r["boardings"], 2);
    assert_eq!(r["transfers"], 1);
    assert_eq!(r["duration_s"], 1080);
    let rides: Vec<&Value> = r["legs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["mode"] == "subway")
        .collect();
    assert_eq!(rides.len(), 2);
    assert_eq!(rides[0]["to"], "Station 1");
    assert_eq!(rides[1]["from"], "Station 1");
    let reverse = c
        .route(
            "city",
            1,
            Some("p:mta:station:2"),
            Some("p:mta:station:0"),
            [6000, 0],
            [0, 0],
        )
        .unwrap();
    assert_eq!(reverse["boardings"], 0);
    assert_eq!(reverse["duration_s"], 5002);
}
#[test]
fn walking_approaches_same_point_and_coverage() {
    let c = synthetic();
    let r = c.route("city", 1, None, None, [0, 0], [0, 0]).unwrap();
    assert_eq!(r["duration_s"], 0);
    assert_eq!(r["boardings"], 0);
    let r = c.route("city", 1, None, None, [10, 0], [5990, 0]).unwrap();
    assert_eq!(r["walking_m"], 20);
    assert_eq!(r["duration_s"], 1098);
    assert_eq!(
        c.route("city", 1, None, None, [0, 3000], [0, 0])
            .unwrap_err()
            .code,
        "failed_precondition.island.walk_anchor"
    );
}
#[test]
fn full_fixture_patterns_and_costs_are_consistent() {
    let data = strata_island::journeys::fixture();
    let ids: std::collections::BTreeSet<_> = data.nodes.iter().map(|n| &n.id).collect();
    assert_eq!(ids.len(), data.nodes.len());
    assert!(data.nodes.len() > 60_000);
    assert!(data.edges.len() > 190_000);
    for e in &data.edges {
        assert!(ids.contains(&e.source) && ids.contains(&e.target));
        assert!(e.seconds > 0);
        if e.kind == "ride" {
            assert!(!e.pattern.is_empty());
            assert!(!e.headsign.is_empty());
        }
        if e.kind == "walk" {
            assert!(e.seconds as f64 >= e.meters as f64 / 1.35);
            assert!(e.points.len() >= 2);
        }
    }
}
fn tiny() -> Data {
    serde_json::from_value(json!({"nodes":[{"id":"w:1","kind":"walk","x":0,"y":0},{"id":"w:2","kind":"walk","x":100,"y":0}],"edges":[{"source":"w:1","target":"w:2","kind":"walk","seconds":75,"meters":100,"points":[[0,0],[100,0]]}],"routes":[],"reference_date":"2026-09-21","semantics":"test"})).unwrap()
}
#[test]
fn journey_crash_worker() {
    let Ok(path) = std::env::var("JOURNEY_TEST_DB") else {
        return;
    };
    let db = store::open_local(std::path::Path::new(&path), None).unwrap();
    std::env::set_var(
        "ISLAND_TEST_CRASH_AT",
        std::env::var("JOURNEY_TEST_STAGE").unwrap(),
    );
    strata_island::journeys::import_rows(&db, "default", &tiny(), "test").unwrap();
    panic!("checkpoint not reached")
}
#[test]
fn durable_import_replays_each_publication_stage() {
    if !cfg!(debug_assertions) {
        return;
    }
    for stage in ["journeys-nodes", "journeys-edges", "journeys-ready"] {
        let dir = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "journey_crash_worker", "--nocapture"])
            .env("JOURNEY_TEST_DB", dir.path())
            .env("JOURNEY_TEST_STAGE", stage)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(77));
        let db = store::open_local(dir.path(), None).unwrap();
        let data = tiny();
        strata_island::journeys::import_rows(&db, "default", &data, "test").unwrap();
        let v = strata_island::journeys::ready_version(&db, "default")
            .unwrap()
            .unwrap();
        let graph = db
            .graph(store::branch("default").unwrap(), store::space().unwrap())
            .unwrap()
            .adjacency_index_at_version(
                &GraphName::new(strata_island::journeys::GRAPH).unwrap(),
                &GraphAnalyticsBudget::default(),
                v,
            )
            .unwrap();
        let c = Catalog::from_graph(graph, Arc::new(data)).unwrap();
        assert_eq!(
            c.route("default", v.as_u64(), None, None, [0, 0], [100, 0])
                .unwrap()["duration_s"],
            75
        );
        strata_island::journeys::import_rows(&db, "default", &tiny(), "test").unwrap();
        assert_eq!(
            strata_island::journeys::ready_version(&db, "default").unwrap(),
            Some(v)
        );
    }
}
