//! Frozen Manhattan extract. Do not retune the recipe to make this pass.

use strata_island::drive;
use strata_island::extract::{
    self, CLOSURE_EDGE_COUNT, EXTRACT_BYTES, EXTRACT_EDGE_COUNT, EXTRACT_FNV1A64,
    EXTRACT_NODE_COUNT,
};
use strata_island::route;
use strata_island::snapshot::CITY_VERSION;

const CITY: &str = include_str!("../fixtures/manhattan-drive.json");
const CLOSURE: &str = include_str!("../fixtures/closure-42nd.json");
const GAZETTEER: &str = include_str!("../fixtures/gazetteer.json");

#[test]
fn city_version_is_frozen() {
    assert_eq!(CITY_VERSION, 1);
}

#[test]
fn extract_counts_and_hash_are_frozen() {
    assert_eq!(CITY.len(), EXTRACT_BYTES);
    assert_eq!(extract::fnv1a64(CITY.as_bytes()), EXTRACT_FNV1A64);
    let city = extract::parse_city(CITY).expect("parse");
    assert_eq!(city.nodes.len(), EXTRACT_NODE_COUNT);
    assert_eq!(city.edges.len(), EXTRACT_EDGE_COUNT);
}

#[test]
fn closure_triples_exist_in_extract() {
    let city = extract::parse_city(CITY).expect("city");
    let closure = extract::parse_closure(CLOSURE).expect("closure");
    assert_eq!(closure.edges.len(), CLOSURE_EDGE_COUNT);
    for edge in &closure.edges {
        assert_eq!(edge.edge_type, "street");
        let found = city
            .edges
            .iter()
            .any(|e| e.src == edge.src && e.dst == edge.dst);
        assert!(found, "missing {} -> {}", edge.src, edge.dst);
    }
}

#[test]
fn gazetteer_pins_exist() {
    let city = extract::parse_city(CITY).expect("city");
    let pois = extract::parse_gazetteer(GAZETTEER).expect("poi");
    assert_eq!(pois.len(), 6);
    for poi in &pois {
        assert!(
            city.nodes.iter().any(|n| n.id == poi.node),
            "missing {}",
            poi.node
        );
    }
    let ids: Vec<_> = pois.iter().map(|p| p.id.as_str()).collect();
    assert!(ids.contains(&"poi:port-authority"));
    assert!(ids.contains(&"poi:grand-central"));
}

fn poi_node<'a>(pois: &'a [extract::GazetteerPoi], id: &str) -> &'a str {
    pois.iter()
        .find(|poi| poi.id == id)
        .map(|poi| poi.node.as_str())
        .unwrap_or_else(|| panic!("{id}"))
}

#[test]
fn talk_pair_parent_uses_closed_42nd() {
    let city = extract::parse_city(CITY).expect("city");
    let closure = extract::parse_closure(CLOSURE).expect("closure");
    let pois = extract::parse_gazetteer(GAZETTEER).expect("poi");
    let index = drive::from_extract("city", &city);
    let src = index
        .node_index(poi_node(&pois, "poi:port-authority"))
        .expect("from node");
    let dst = index
        .node_index(poi_node(&pois, "poi:grand-central"))
        .expect("to node");
    let path = route::route(&index, src, dst).expect("talk path");
    assert!(
        route::uses_closed(&path, &closure.edges),
        "parent talk path must use a closed 42nd triple (gate for PR5)"
    );
}
