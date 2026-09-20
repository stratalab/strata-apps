//! Import / resume. Cache is in-process; crash-restart uses tempfile + open_local.

use std::sync::{Mutex, MutexGuard};

use serde_json::json;
use strata_island::extract::{self, EXTRACT_EDGE_COUNT, EXTRACT_NODE_COUNT, NODE_CAP};
use strata_island::route;
use strata_island::store::{self, analytics_budget, ANALYTICS_EDGES, ANALYTICS_NODES};
use strata_island::world::{OpenArgs, World};
use stratadb::graph::GraphAnalyticsBudget;

fn open_cache() -> World {
    World::open(OpenArgs {
        cache: true,
        db_path: "./island-db".into(),
        memory_budget_bytes: None,
    })
    .expect("open cache")
}

/// Durable `open_local` writer locks can linger across drop in a shared
/// process. Serialize crash-restart tests so reopen is not `writer_lock`.
static DURABLE: Mutex<()> = Mutex::new(());

fn durable_gate() -> MutexGuard<'static, ()> {
    DURABLE.lock().expect("durable gate")
}

#[test]
fn analytics_budget_is_explicit() {
    assert_eq!(
        analytics_budget(),
        GraphAnalyticsBudget::new(ANALYTICS_NODES, ANALYTICS_EDGES)
    );
}

#[test]
fn parse_refuses_over_cap() {
    let mut nodes = Vec::new();
    for i in 0..=NODE_CAP {
        nodes.push(json!({"id": format!("n:{i}"), "x": 0, "y": 0}));
    }
    let doc = json!({"attribution": "t", "nodes": nodes, "edges": []});
    let err = extract::parse_city(&doc.to_string()).expect_err("cap");
    assert_eq!(err.code, "failed_precondition.island.import_cap");
}

#[test]
fn cache_import_matches_extract() {
    let world = World::open(OpenArgs {
        cache: true,
        db_path: "./island-db".into(),
        memory_budget_bytes: None,
    })
    .expect("open cache");
    let city = world.city_snapshot(None).expect("city");
    assert_eq!(city.node_ids.len(), EXTRACT_NODE_COUNT);
    let edges: usize = city.outgoing.iter().map(Vec::len).sum();
    assert_eq!(edges, EXTRACT_EDGE_COUNT);
    assert_eq!(world.gazetteer().len(), 6);
    assert!(!world.durable());
    let from = world
        .lookup_node(&city, "poi:port-authority")
        .expect("port authority");
    let to = world
        .lookup_node(&city, "poi:grand-central")
        .expect("grand central");
    let path = world.route(&city, from, to).expect("demo path");
    assert!(path.length_m > 0);
    assert!(strata_island::route::uses_closed(
        &path,
        &world.closure().edges
    ));
    let engine = world
        .engine_sssp_distance_m(&city.node_ids[from], &city.node_ids[to])
        .expect("engine sssp");
    assert_eq!(path.length_m, engine);

    let json_poi = world
        .json_gazetteer_poi("poi:grand-central")
        .expect("json poi");
    assert_eq!(json_poi.id, "poi:grand-central");
    assert_eq!(json_poi.node, city.node_ids[to]);
    let ram = world
        .gazetteer()
        .into_iter()
        .find(|poi| poi.id == "poi:grand-central")
        .expect("ram poi");
    assert_eq!(ram.node, json_poi.node);
    let bound = world
        .graph_binding_node("poi:grand-central")
        .expect("binding");
    assert_eq!(bound, json_poi.node);
    assert_eq!(
        world.node_binding_poi(&bound).expect("reverse"),
        "poi:grand-central"
    );
    let missing = world
        .lookup_node(&city, "poi:nope")
        .expect_err("unknown poi");
    assert_eq!(missing.code, "not_found.island.node");
    let audit = world.audit().expect("audit empty desks");
    assert!(audit.ok);
    assert!(audit.desks.is_empty());
    assert_eq!(audit.city_version, 1);
}

#[test]
fn durable_tempfile_resume_matches() {
    let _gate = durable_gate();
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("island-db");
    let first = World::open(OpenArgs {
        cache: false,
        db_path: path.clone(),
        memory_budget_bytes: None,
    })
    .expect("first open");
    let n = first.city_index().node_ids.len();
    let e: usize = first.city_index().outgoing.iter().map(Vec::len).sum();
    assert_eq!(n, EXTRACT_NODE_COUNT);
    assert_eq!(e, EXTRACT_EDGE_COUNT);
    drop(first);

    let second = World::open(OpenArgs {
        cache: false,
        db_path: path,
        memory_budget_bytes: None,
    })
    .expect("resume");
    assert_eq!(second.city_index().node_ids.len(), n);
    let e2: usize = second.city_index().outgoing.iter().map(Vec::len).sum();
    assert_eq!(e2, e);
    assert_eq!(second.last_persist_ms(), 0);
    assert_eq!(second.gazetteer().len(), 6);
    let city = second.city_snapshot(None).expect("resume city");
    let src = second
        .lookup_node(&city, "poi:port-authority")
        .expect("resume poi");
    assert_eq!(city.node_ids[src], "n:42435663");
    let json_poi = second
        .json_gazetteer_poi("poi:port-authority")
        .expect("resume json");
    assert_eq!(json_poi.node, "n:42435663");
}

#[test]
fn city_snapshot_does_not_need_db() {
    let world = World::open(OpenArgs {
        cache: true,
        db_path: "./island-db".into(),
        memory_budget_bytes: None,
    })
    .expect("open");
    let a = world.city_snapshot(None).expect("a");
    let b = world.city_snapshot(Some("city")).expect("b");
    assert_eq!(a.node_ids.len(), b.node_ids.len());
    let err = world.city_snapshot(Some("nope")).expect_err("missing");
    assert_eq!(err.code, "invalid_argument.island.branch");
}

#[test]
fn dijkstra_source_does_not_touch_store() {
    let src = include_str!("../src/route.rs");
    assert!(!src.contains("store::"));
    assert!(!src.contains("Database"));
    assert!(!src.contains("graph("));
}

#[test]
fn spec_mismatch_refuses() {
    assert!(!store::spec_matches(
        &json!({"extract_hash": "nope", "city_version": 1})
    ));
    assert!(store::spec_matches(&json!({
        "extract_hash": store::extract_hash_hex(),
        "city_version": 1,
    })));
}

#[test]
fn close_never_calls_promote() {
    let store_src = include_str!("../src/store.rs");
    let world_src = include_str!("../src/world.rs");
    assert!(!store_src.contains("promote("));
    assert!(!world_src.contains("promote("));
}

#[test]
fn close_compare_and_cap_on_cache() {
    let world = open_cache();
    let city = world.city_snapshot(None).expect("city");
    let city_edges = city.edge_count();
    let from = world
        .lookup_node(&city, "poi:port-authority")
        .expect("port authority");
    let to = world
        .lookup_node(&city, "poi:grand-central")
        .expect("grand central");
    let parent = world.route(&city, from, to).expect("parent");
    assert!(route::uses_closed(&parent, &world.closure().edges));

    let err = world
        .close_42nd(Some("desk-0001"))
        .expect_err("from must be city");
    assert_eq!(err.code, "invalid_argument.island.branch");

    let closed = world.close_42nd(Some("city")).expect("close");
    assert_eq!(closed.desk, "desk-0001");
    assert_eq!(closed.closed_edges, world.closure().edges.len());

    let city_after = world.city_snapshot(Some("city")).expect("parent ram");
    assert_eq!(city_after.edge_count(), city_edges);
    for edge in &world.closure().edges {
        assert!(city_after.has_directed(&edge.src, &edge.dst));
    }

    let desk = world.city_snapshot(Some("desk-0001")).expect("desk ram");
    assert_eq!(desk.edge_count(), city_edges - world.closure().edges.len());
    for edge in &world.closure().edges {
        assert!(!desk.has_directed(&edge.src, &edge.dst));
    }

    let desk_from = world
        .lookup_node(&desk, "poi:port-authority")
        .expect("desk from");
    let desk_to = world
        .lookup_node(&desk, "poi:grand-central")
        .expect("desk to");
    let child = world.route(&desk, desk_from, desk_to).expect("child");
    assert!(!route::uses_closed(&child, &world.closure().edges));
    assert!(child.length_m >= parent.length_m);

    let poi = world
        .json_gazetteer_poi_on("desk-0001", "poi:grand-central")
        .expect("child json");
    assert_eq!(poi.id, "poi:grand-central");
    assert!(world.verify_desk_chain("desk-0001").expect("chain"));

    let cmp = world.compare("city", "desk-0001").expect("compare");
    assert!(cmp.graph_entities > 0);
    assert!(!cmp.empty);

    let audit = world.audit().expect("audit");
    assert!(audit.ok);
    assert_eq!(audit.city_version, 1);
    assert_eq!(
        audit.city_nodes,
        u64::try_from(city_after.node_ids.len()).expect("nodes")
    );
    assert_eq!(
        audit.city_edges,
        u64::try_from(city_after.edge_count()).expect("edges")
    );
    assert_eq!(audit.desks.len(), 1);
    assert_eq!(audit.desks[0].desk, "desk-0001");
    assert!(audit.desks[0].chain_ok);

    world.close_42nd(None).expect("desk-0002");
    world.close_42nd(None).expect("desk-0003");
    let cap = world.close_42nd(None).expect_err("fourth desk");
    assert_eq!(cap.code, "failed_precondition.island.desk_cap");

    world.archive("desk-0003").expect("archive");
    world.close_42nd(None).expect("reuse slot");
    let refuse_city = world.archive("city").expect_err("city");
    assert_eq!(refuse_city.code, "invalid_argument.island.branch");
}

#[test]
fn durable_close_survives_reopen() {
    let _gate = durable_gate();
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("island-db");
    let parent_edges;
    let parent_len;
    let child_len;
    {
        let world = World::open(OpenArgs {
            cache: false,
            db_path: path.clone(),
            memory_budget_bytes: None,
        })
        .expect("first");
        let city = world.city_snapshot(None).expect("city");
        parent_edges = city.edge_count();
        let from = world
            .lookup_node(&city, "poi:port-authority")
            .expect("from");
        let to = world.lookup_node(&city, "poi:grand-central").expect("to");
        parent_len = world.route(&city, from, to).expect("parent").length_m;
        world.close_42nd(Some("city")).expect("close");
        let desk = world.city_snapshot(Some("desk-0001")).expect("desk");
        let desk_from = world
            .lookup_node(&desk, "poi:port-authority")
            .expect("desk from");
        let desk_to = world
            .lookup_node(&desk, "poi:grand-central")
            .expect("desk to");
        child_len = world
            .route(&desk, desk_from, desk_to)
            .expect("child")
            .length_m;
        assert!(child_len >= parent_len);
        drop(world);
    }

    let world = World::open(OpenArgs {
        cache: false,
        db_path: path,
        memory_budget_bytes: None,
    })
    .expect("resume");
    let city = world.city_snapshot(Some("city")).expect("city");
    assert_eq!(city.edge_count(), parent_edges);
    let desk = world.city_snapshot(Some("desk-0001")).expect("desk");
    for edge in &world.closure().edges {
        assert!(city.has_directed(&edge.src, &edge.dst));
        assert!(!desk.has_directed(&edge.src, &edge.dst));
    }
    let from = world
        .lookup_node(&city, "poi:port-authority")
        .expect("from");
    let to = world.lookup_node(&city, "poi:grand-central").expect("to");
    let parent = world.route(&city, from, to).expect("parent");
    let desk_from = world
        .lookup_node(&desk, "poi:port-authority")
        .expect("desk from");
    let desk_to = world
        .lookup_node(&desk, "poi:grand-central")
        .expect("desk to");
    let child = world.route(&desk, desk_from, desk_to).expect("child");
    assert!(route::uses_closed(&parent, &world.closure().edges));
    assert!(!route::uses_closed(&child, &world.closure().edges));
    assert_eq!(parent.length_m, parent_len);
    assert_eq!(child.length_m, child_len);
}

#[test]
fn half_closed_desk_is_repaired() {
    let _gate = durable_gate();
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("island-db");
    {
        let world = World::open(OpenArgs {
            cache: false,
            db_path: path.clone(),
            memory_budget_bytes: None,
        })
        .expect("first");
        let desk = world.fork_desk_pending().expect("pending");
        assert_eq!(desk, "desk-0001");
        drop(world);
    }
    let world = World::open(OpenArgs {
        cache: false,
        db_path: path,
        memory_budget_bytes: None,
    })
    .expect("repair");
    let city = world.city_snapshot(Some("city")).expect("city");
    let desk = world.city_snapshot(Some("desk-0001")).expect("repaired");
    for edge in &world.closure().edges {
        assert!(city.has_directed(&edge.src, &edge.dst));
        assert!(!desk.has_directed(&edge.src, &edge.dst));
    }
    let from = world
        .lookup_node(&desk, "poi:port-authority")
        .expect("from");
    let to = world.lookup_node(&desk, "poi:grand-central").expect("to");
    let child = world.route(&desk, from, to).expect("child");
    assert!(!route::uses_closed(&child, &world.closure().edges));
}

#[test]
fn durable_parent_delete_refused_while_desk_lives() {
    let _gate = durable_gate();
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("island-db");
    let world = World::open(OpenArgs {
        cache: false,
        db_path: path,
        memory_budget_bytes: None,
    })
    .expect("open");
    world.close_42nd(Some("city")).expect("close");
    let err = world.delete_branch("city").expect_err("parent");
    assert_eq!(err.code, "failed_precondition.engine.branch_has_children");
    let default_err = world.delete_branch("default").expect_err("default");
    assert_eq!(default_err.code, "invalid_argument.engine.branch_delete");
    world.archive("desk-0001").expect("archive");
    world.city_snapshot(Some("city")).expect("city lives");
}
