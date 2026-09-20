//! Pinned Manhattan subway service topology, isolated from the street graph.
use crate::{error::IslandError, extract, store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use stratadb::graph::*;
use stratadb::{CommitVersion, Database};

pub const FIXTURE: &str = include_str!("../fixtures/subway.json");
pub const GRAPH: &str = "subway";
const READY: &str = "ready:subway-v1";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct StationMetadata {
    pub station_id: String,
    pub complex_id: String,
    pub gtfs_stop_ids: Vec<String>,
    pub routes: Vec<String>,
    pub lines: Vec<String>,
}

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("pinned subway fixture")
}

pub fn validate_ready(db: &Database, branch: &str) -> Result<(), IslandError> {
    let hash = format!("{:016x}", extract::fnv1a64(FIXTURE.as_bytes()));
    if store::read_json(db, branch, READY)?.is_none_or(|ready| ready["hash"] != hash) {
        return Err(IslandError::code("failed_precondition.island.subway"));
    }
    Ok(())
}

pub fn import_on(db: &Database, branch: &str) -> Result<(), IslandError> {
    let hash = format!("{:016x}", extract::fnv1a64(FIXTURE.as_bytes()));
    if let Some(ready) = store::read_json(db, branch, READY)? {
        if ready["hash"] != hash {
            return Err(IslandError::code("failed_precondition.island.subway"));
        }
        return Ok(());
    }
    let data = fixture();
    let name = GraphName::new(GRAPH)?;
    let mut graph = db.graph(store::branch(branch)?, store::space()?)?;
    if graph.graph_info(&name)?.is_none() {
        graph.create_graph(name.clone())?;
    }
    // Deterministic upserts also resume interrupted imports; never delete a graph.
    let nodes = data["stations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            let id = s["id"].as_str().unwrap();
            Ok((
                GraphNodeId::new(id)?,
                GraphNodeData::new(
                    Some(GraphProperties::new(s.clone())?),
                    Some(GraphEntityBinding::new(GraphBindingTarget::new(
                        GraphBindingPrimitive::Json,
                        None,
                        store::space()?,
                        id,
                    )?)),
                ),
            ))
        })
        .collect::<Result<Vec<_>, IslandError>>()?;
    let edges = data["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            Ok((
                GraphNodeId::new(e["source"].as_str().unwrap())?,
                GraphEdgeType::new(e["kind"].as_str().unwrap())?,
                GraphNodeId::new(e["target"].as_str().unwrap())?,
                GraphEdgeData::default(),
            ))
        })
        .collect::<Result<Vec<_>, IslandError>>()?;
    graph.bulk_insert(&name, &nodes, &edges, Some(512))?;
    drop(graph);
    crate::scenario::checkpoint("import-subway");
    store::write_json(
        db,
        branch,
        READY,
        json!({"hash":hash,"nodes":nodes.len(),"edges":edges.len()}),
    )?;
    Ok(())
}

pub fn index(
    db: &Database,
    branch: &str,
    version: CommitVersion,
) -> Result<GraphAdjacencyIndex, IslandError> {
    Ok(db
        .graph(store::branch(branch)?, store::space()?)?
        .adjacency_index_at_version(
            &GraphName::new(GRAPH)?,
            &GraphAnalyticsBudget::new(151, 842),
            version,
        )?)
}

pub fn network(db: &Database, branch: &str, version: CommitVersion) -> Result<Value, IslandError> {
    let graph = index(db, branch, version)?;
    let mut data = fixture();
    if graph.node_count() != data["stations"].as_array().unwrap().len()
        || graph.edge_count() != data["edges"].as_array().unwrap().len() as u64
    {
        return Err(IslandError::code("failed_precondition.island.subway"));
    }
    // Geometry and route labels come from the pin; connectivity is read from Strata.
    let subgraph = graph.subgraph(graph.node_ids());
    data["edges"] = json!(subgraph
        .edges()
        .iter()
        .map(|e| json!({
            "source":graph.node_id(e.source()).unwrap().as_str(),
            "target":graph.node_id(e.target()).unwrap().as_str(),
            "kind":graph.edge_type_name(e.edge_type()).unwrap().as_str()
        }))
        .collect::<Vec<_>>());
    data["branch"] = json!(branch);
    data["version"] = json!(version);
    Ok(data)
}

pub fn explore(
    db: &Database,
    branch: &str,
    version: CommitVersion,
    seed: &str,
    depth: usize,
) -> Result<Value, IslandError> {
    if !(1..=6).contains(&depth) {
        return Err(IslandError::code("invalid_argument.island.subway"));
    }
    let snapshot_started = std::time::Instant::now();
    let graph = index(db, branch, version)?;
    let snapshot_ms = snapshot_started.elapsed().as_secs_f64() * 1000.;
    let id = GraphNodeId::new(seed)?;
    if graph.node_index(&id).is_none() {
        return Err(IslandError::code("not_found.island.subway_station"));
    }
    let started = std::time::Instant::now();
    let bfs = graph.bfs(
        &id,
        &GraphBfsOptions::new(depth, Some(151), None, GraphDirection::Outgoing),
    )?;
    let ids: Vec<_> = bfs
        .visited()
        .iter()
        .map(|i| graph.node_id(*i).unwrap().clone())
        .collect();
    let subgraph = graph.subgraph(&ids);
    let algorithm_ms = started.elapsed().as_secs_f64() * 1000.;
    let data = fixture();
    let stations: Vec<_> = data["stations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| {
            ids.iter()
                .any(|id| id.as_str() == s["id"].as_str().unwrap())
        })
        .collect();
    Ok(
        json!({"branch":branch,"version":version,"seed":seed,"depth":depth,
        "stations":stations,"connections":subgraph.edge_count(),"algorithm":"Strata BFS + subgraph",
        "algorithm_ms":algorithm_ms,"snapshot_ms":snapshot_ms,"snapshot_cached":false,
        "semantics":"A hop is one scheduled service connection or transfer; express services may skip stops. This is not a timed journey."}),
    )
}
