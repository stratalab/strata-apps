//! Versioned, curated OSM destinations and the semantic graph.
use crate::{error::IslandError, extract, store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Instant;
use stratadb::graph::*;
use stratadb::json::{JsonDocumentId, JsonPath};
use stratadb::{CommitVersion, Database};

pub const FIXTURE: &str = include_str!("../fixtures/places-curated.json");
pub const GRAPH: &str = "places";
pub const READY: &str = "ready:catalog-v4";
pub const OSM_READY: &str = "ready:catalog-v3";
pub const OSM_FIXTURE: &str = include_str!("../fixtures/places-catalog-v3.json");
pub const PREVIOUS_READY: &str = "ready:catalog-v2";
pub const LEGACY_READY: &str = "ready:catalog-v1";
pub const LEGACY_FIXTURE: &str = include_str!("../fixtures/places-catalog-v1.json");
pub const PREVIOUS_FIXTURE: &str = include_str!("../fixtures/places-catalog-v2.json");
pub const CATALOG_COUNT: usize = 1852;
const CATALOGS: [(u32, &str, &str); 4] = [
    (1, LEGACY_READY, LEGACY_FIXTURE),
    (2, PREVIOUS_READY, PREVIOUS_FIXTURE),
    (3, OSM_READY, OSM_FIXTURE),
    (4, READY, FIXTURE),
];
pub const CATEGORIES: [&str; 3] = ["landmark", "park", "transit"];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub category: String,
    pub x: i32,
    pub y: i32,
    pub node: Option<String>,
    pub approach_m: u32,
    pub attachment: String,
    pub source_url: String,
    pub coordinate_method: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subway: Option<crate::subway::StationMetadata>,
}
#[derive(Deserialize)]
struct Fixture {
    places: Vec<Place>,
}

pub fn fixture() -> Result<Vec<Place>, IslandError> {
    parse_fixture(FIXTURE)
}

fn parse_fixture(raw: &str) -> Result<Vec<Place>, IslandError> {
    let mut places = serde_json::from_str::<Fixture>(raw)
        .map_err(|_| IslandError::code("invalid_argument.island.places"))?
        .places;
    places.sort_by(|a, b| a.id.cmp(&b.id));
    let mut ids = BTreeSet::new();
    let mut aliases = BTreeSet::new();
    for p in &places {
        if !ids.insert(&p.id)
            || !CATEGORIES.contains(&p.category.as_str())
            || p.name.is_empty()
            || !crate::geo::in_aabb(p.x, p.y)
            || p.aliases.iter().any(|a| !aliases.insert(a))
        {
            return Err(IslandError::code("invalid_argument.island.places"));
        }
    }
    Ok(places)
}

pub fn ready_version(db: &Database, branch: &str) -> Result<Option<CommitVersion>, IslandError> {
    let mut documents = db.json(store::branch(branch)?, store::space()?)?;
    for &(_, id, _) in CATALOGS.iter().rev() {
        if let Some(record) =
            documents.get_versioned(&JsonDocumentId::new(id)?, &JsonPath::root())?
        {
            return Ok(Some(record.version()));
        }
    }
    Ok(None)
}

pub fn import(db: &Database) -> Result<(), IslandError> {
    import_on(db, "city")
}

pub fn import_on(db: &Database, branch: &str) -> Result<(), IslandError> {
    import_catalog(db, branch, FIXTURE, 4)
}

/// Imports only pinned catalog revisions; validates all prior readiness records.
pub fn import_catalog(
    db: &Database,
    branch: &str,
    raw: &str,
    catalog: u32,
) -> Result<(), IslandError> {
    let (_, ready_id, expected_raw) = CATALOGS
        .iter()
        .find(|(v, _, _)| *v == catalog)
        .ok_or(IslandError::code("invalid_argument.island.catalog"))?;
    if raw != *expected_raw {
        return Err(IslandError::code("invalid_argument.island.catalog"));
    }
    let mut upgrading = false;
    let mut ready = false;
    for (version, id, fixture) in CATALOGS {
        if let Some(record) = store::read_json(db, branch, id)? {
            if version > catalog
                || record["hash"] != format!("{:016x}", extract::fnv1a64(fixture.as_bytes()))
            {
                return Err(IslandError::code("failed_precondition.island.dataset"));
            }
            upgrading |= version < catalog;
            ready |= version == catalog;
        }
    }
    if ready {
        if catalog >= 4 {
            crate::subway::validate_ready(db, branch)?;
        }
        return Ok(());
    }
    let hash = format!("{:016x}", extract::fnv1a64(raw.as_bytes()));
    let places = parse_fixture(raw)?;
    // Resume unpublished imports by replaying deterministic upserts.
    let mut graph = db.graph(store::branch(branch)?, store::space()?)?;
    let name = GraphName::new(GRAPH)?;
    let exists = graph.graph_info(&name)?.is_some();
    if upgrading && !exists {
        return Err(IslandError::code("failed_precondition.island.places"));
    }
    // Never delete a partially imported graph: large durable deletes exceed the
    // engine's commit-row limit. Replay preserves both staged and historical rows.
    if !exists {
        graph.create_graph(name.clone())?;
    }
    let frozen = graph
        .ontology(&name)?
        .is_some_and(|o| o.status() == GraphOntologyStatus::Frozen);
    if !frozen {
        for t in ["landmark", "park", "transit", "anchor", "category"] {
            graph
                .define_object_type(&name, GraphObjectTypeDef::new(GraphTypeName::new(t)?, [])?)?;
        }
        // The engine declares a single source type per link: use concrete, typed relationship names.
        for c in CATEGORIES {
            for (suffix, target) in [("near_road", "anchor"), ("in_category", "category")] {
                graph.define_link_type(
                    &name,
                    GraphLinkTypeDef::new(
                        GraphTypeName::new(format!("{c}_{suffix}"))?,
                        GraphTypeName::new(c)?,
                        GraphTypeName::new(target)?,
                        Some("many-to-one".into()),
                        [],
                    )?,
                )?;
            }
        }
        graph.freeze_ontology(&name)?;
    }
    drop(graph);
    let mut nodes = BTreeMap::new();
    let mut edges = Vec::new();
    for c in CATEGORIES {
        nodes.insert(
            format!("category:{c}"),
            GraphNodeData::new(Some(GraphProperties::new(json!({"name":c}))?), None)
                .with_object_type(GraphTypeName::new("category")?),
        );
    }
    for p in &places {
        store::write_json(
            db,
            branch,
            &p.id,
            serde_json::to_value(p).expect("place serialization"),
        )?;
        let binding = GraphEntityBinding::new(GraphBindingTarget::new(
            GraphBindingPrimitive::Json,
            None,
            store::space()?,
            p.id.as_str(),
        )?);
        nodes.insert(
            p.id.clone(),
            GraphNodeData::new(
                Some(GraphProperties::new(
                    json!({"name":p.name,"x":p.x,"y":p.y}),
                )?),
                Some(binding),
            )
            .with_object_type(GraphTypeName::new(&p.category)?),
        );
        edges.push((
            GraphNodeId::new(&p.id)?,
            GraphEdgeType::new(format!("{}_in_category", p.category))?,
            GraphNodeId::new(format!("category:{}", p.category))?,
            GraphEdgeData::default(),
        ));
        if let Some(road) = &p.node {
            let anchor = format!("anchor:{road}");
            nodes.insert(
                anchor.clone(),
                GraphNodeData::new(
                    Some(GraphProperties::new(json!({"road_node_id":road}))?),
                    None,
                )
                .with_object_type(GraphTypeName::new("anchor")?),
            );
            edges.push((
                GraphNodeId::new(&p.id)?,
                GraphEdgeType::new(format!("{}_near_road", p.category))?,
                GraphNodeId::new(anchor)?,
                GraphEdgeData::default(),
            ));
        }
    }
    crate::scenario::checkpoint("import-documents");
    let nodes = nodes
        .into_iter()
        .map(|(id, data)| Ok((GraphNodeId::new(id)?, data)))
        .collect::<Result<Vec<_>, IslandError>>()?;
    db.graph(store::branch(branch)?, store::space()?)?
        .bulk_insert(&name, &nodes, &edges, Some(512))?;
    crate::scenario::checkpoint("import-graph");
    let loaded = load(db, branch)?;
    if loaded != places {
        return Err(IslandError::code("failed_precondition.island.places"));
    }
    // Publish the catalog only after its separate subway graph is complete.
    if catalog >= 4 {
        crate::subway::import_on(db, branch)?;
    }
    store::write_json(
        db,
        branch,
        ready_id,
        json!({"schema":2,"catalog":catalog,"hash":hash,"places":places.len(),"nodes":nodes.len(),"edges":edges.len()}),
    )?;
    crate::scenario::checkpoint("import-ready");
    Ok(())
}

pub fn load(db: &Database, branch: &str) -> Result<Vec<Place>, IslandError> {
    load_at(db, branch, None)
}

fn load_at(
    db: &Database,
    branch: &str,
    version: Option<CommitVersion>,
) -> Result<Vec<Place>, IslandError> {
    let mut graph = db.graph(store::branch(branch)?, store::space()?)?;
    let name = GraphName::new(GRAPH)?;
    let mut places = Vec::new();
    for c in CATEGORIES {
        let mut cursor = None;
        loop {
            let kind = GraphTypeName::new(c)?;
            let page = match version {
                Some(v) => graph.nodes_by_type_at_version(&name, &kind, cursor.as_ref(), 100, v)?,
                None => graph.nodes_by_type(&name, &kind, cursor.as_ref(), 100)?,
            };
            for node in page.nodes() {
                let binding = node
                    .data()
                    .binding()
                    .ok_or(IslandError::code("failed_precondition.island.binding"))?;
                if binding.target().key() != node.node_id().as_str()
                    || binding.target().branch().is_some()
                {
                    return Err(IslandError::code("failed_precondition.island.binding"));
                }
                let value = match version {
                    Some(v) => db
                        .json(store::branch(branch)?, store::space()?)?
                        .get_at_version(
                            &JsonDocumentId::new(binding.target().key())?,
                            &JsonPath::root(),
                            v,
                        )?
                        .map(|j| j.into_inner()),
                    None => store::read_json(db, branch, binding.target().key())?,
                }
                .ok_or(IslandError::code("not_found.island.place"))?;
                let p: Place = serde_json::from_value(value)
                    .map_err(|_| IslandError::code("failed_precondition.island.places"))?;
                if p.id != node.node_id().as_str() || p.category != c {
                    return Err(IslandError::code("failed_precondition.island.places"));
                }
                let relation = GraphEdgeType::new(format!("{c}_in_category"))?;
                let category = match version {
                    Some(v) => graph.neighbors_at_version(
                        &name,
                        node.node_id(),
                        GraphDirection::Outgoing,
                        Some(&relation),
                        None,
                        2,
                        v,
                    )?,
                    None => graph.neighbors(
                        &name,
                        node.node_id(),
                        GraphDirection::Outgoing,
                        Some(&relation),
                        None,
                        2,
                    )?,
                };
                if category.neighbors().len() != 1
                    || category.neighbors()[0].node().node_id().as_str() != format!("category:{c}")
                {
                    return Err(IslandError::code(
                        "failed_precondition.island.place_category",
                    ));
                }
                let relation = GraphEdgeType::new(format!("{c}_near_road"))?;
                let anchors = match version {
                    Some(v) => graph.neighbors_at_version(
                        &name,
                        node.node_id(),
                        GraphDirection::Outgoing,
                        Some(&relation),
                        None,
                        2,
                        v,
                    )?,
                    None => graph.neighbors(
                        &name,
                        node.node_id(),
                        GraphDirection::Outgoing,
                        Some(&relation),
                        None,
                        2,
                    )?,
                };
                let road = anchors
                    .neighbors()
                    .first()
                    .and_then(|n| n.node().data().properties())
                    .and_then(|p| p.as_inner()["road_node_id"].as_str())
                    .map(str::to_owned);
                if anchors.neighbors().len() > 1 || road != p.node {
                    return Err(IslandError::code("failed_precondition.island.place_anchor"));
                }
                // Category and anchor membership are validated against persisted graph edges.
                places.push(p);
            }
            if !page.has_more() {
                break;
            }
            cursor = page.cursor().cloned();
        }
    }
    places.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(places)
}

pub struct Snapshot {
    pub version: CommitVersion,
    pub roads: Arc<GraphAdjacencyIndex>,
    pub relations: Arc<GraphAdjacencyIndex>,
    pub places: Arc<Vec<Place>>,
    pub snapshot_ms: f64,
}

pub fn snapshot(
    db: &Database,
    branch: &str,
    version: CommitVersion,
) -> Result<Snapshot, IslandError> {
    let started = Instant::now();
    let mut graph = db.graph(store::branch(branch)?, store::space()?)?;
    let roads = graph.adjacency_index_at_version(
        &store::graph_name()?,
        &store::analytics_budget(),
        version,
    )?;
    let relations = graph.adjacency_index_at_version(
        &GraphName::new(GRAPH)?,
        &GraphAnalyticsBudget::new(CATALOG_COUNT * 2 + CATEGORIES.len(), CATALOG_COUNT * 2),
        version,
    )?;
    let places = load_at(db, branch, Some(version))?;
    for p in &places {
        if let Some(node) = &p.node {
            if roads.node_index(&GraphNodeId::new(node)?).is_none() {
                return Err(IslandError::code("failed_precondition.island.place_anchor"));
            }
        }
    }
    Ok(Snapshot {
        version,
        roads: Arc::new(roads),
        relations: Arc::new(relations),
        places: Arc::new(places),
        snapshot_ms: started.elapsed().as_secs_f64() * 1000.,
    })
}

pub fn discover(
    snapshot: &Snapshot,
    branch: &str,
    origin: &str,
    category: Option<&str>,
    max_m: u32,
) -> Result<Value, IslandError> {
    if max_m > 100_000 || category.is_some_and(|c| !CATEGORIES.contains(&c)) {
        return Err(IslandError::code("invalid_argument.island.discovery"));
    }
    let origin = snapshot
        .places
        .iter()
        .find(|p| p.id == origin || p.aliases.iter().any(|a| a == origin))
        .map_or(Some(origin), |p| p.node.as_deref())
        .ok_or(IslandError::code("failed_precondition.island.place_anchor"))?;
    let started = Instant::now();
    let distances = snapshot
        .roads
        .sssp(&GraphNodeId::new(origin)?, GraphDirection::Outgoing)?;
    let algorithm_ms = started.elapsed().as_secs_f64() * 1000.;
    let mut results = Vec::new();
    let mut unreachable = 0;
    for p in snapshot
        .places
        .iter()
        .filter(|p| category.is_none_or(|c| p.category == c))
    {
        let distance = p
            .node
            .as_ref()
            .and_then(|n| GraphNodeId::new(n).ok())
            .and_then(|n| snapshot.roads.node_index(&n))
            .and_then(|i| distances.distance(i));
        match distance {
            Some(d) if d <= f64::from(max_m) => results.push(json!({"place":p,"distance_m":d})),
            None => unreachable += 1,
            _ => {}
        }
    }
    results.sort_by(|a, b| {
        a["distance_m"]
            .as_f64()
            .unwrap()
            .total_cmp(&b["distance_m"].as_f64().unwrap())
            .then_with(|| a["place"]["id"].as_str().cmp(&b["place"]["id"].as_str()))
    });
    Ok(
        json!({"branch":branch,"version":snapshot.version,"dataset":format!("curated-{}",snapshot.places.len()),"results":results,"unreachable":unreachable,"algorithm":"Strata SSSP","algorithm_ms":algorithm_ms,"snapshot_ms":snapshot.snapshot_ms,"snapshot_cached":true,"reachable_road_nodes":distances.reachable_count()}),
    )
}

pub fn explore(
    snapshot: &Snapshot,
    branch: &str,
    seed: &str,
    depth: usize,
    limit: usize,
) -> Result<Value, IslandError> {
    if depth > 3 || !(1..=100).contains(&limit) {
        return Err(IslandError::code("invalid_argument.island.explore"));
    }
    let started = Instant::now();
    let bfs = snapshot.relations.bfs(
        &GraphNodeId::new(seed)?,
        &GraphBfsOptions::new(depth, Some(limit), None, GraphDirection::Both),
    )?;
    let ids: Vec<_> = bfs
        .visited()
        .iter()
        .map(|i| snapshot.relations.node_id(*i).unwrap().clone())
        .collect();
    let sub = snapshot.relations.subgraph(&ids);
    let nodes:Vec<_>=ids.iter().map(|id|json!({"id":id.as_str(),"name":snapshot.places.iter().find(|p|p.id==id.as_str()).map(|p|p.name.as_str()).unwrap_or_else(||if id.as_str().starts_with("anchor:"){"Road intersection"}else{id.as_str().strip_prefix("category:").unwrap_or(id.as_str())})})).collect();
    let edges:Vec<_>=sub.edges().iter().map(|e|json!({"source":snapshot.relations.node_id(e.source()).unwrap().as_str(),"target":snapshot.relations.node_id(e.target()).unwrap().as_str(),"relation":snapshot.relations.edge_type_name(e.edge_type()).unwrap().as_str()})).collect();
    Ok(
        json!({"branch":branch,"version":snapshot.version,"nodes":nodes,"edges":edges,"truncated":bfs.truncated(),"algorithm":"Strata BFS + induced subgraph","algorithm_ms":started.elapsed().as_secs_f64()*1000.}),
    )
}

pub fn current_version(db: &Database, branch: &str) -> Result<CommitVersion, IslandError> {
    let operation_version = crate::scenario::version(db, branch)?;
    let ready = ready_version(db, branch)?
        .ok_or(IslandError::code("failed_precondition.island.dataset"))?;
    let ready = crate::addresses::ready_version(db, branch)?.map_or(ready, |a| ready.max(a));
    let ready = crate::journeys::ready_version(db, branch)?.map_or(ready, |a| ready.max(a));
    if let Some(version) = operation_version {
        return Ok(version.max(ready));
    }
    if branch == "city" {
        return Ok(ready);
    }
    let meta = db
        .json(store::branch(branch)?, store::space()?)?
        .get_versioned(&JsonDocumentId::new(store::DOC_META)?, &JsonPath::root())?
        .map(|r| r.version())
        .unwrap_or(ready);
    Ok(ready.max(meta))
}

pub fn detail(
    db: &Database,
    branch: &str,
    version: CommitVersion,
    p: &Place,
) -> Result<Value, IslandError> {
    let mut graph = db.graph(store::branch(branch)?, store::space()?)?;
    let name = GraphName::new(GRAPH)?;
    let node = graph
        .get_node_at_version(&name, &GraphNodeId::new(&p.id)?, version)?
        .ok_or(IslandError::code("not_found.island.place"))?;
    let page = graph.neighbors_at_version(
        &name,
        &GraphNodeId::new(&p.id)?,
        GraphDirection::Outgoing,
        None,
        None,
        10,
        version,
    )?;
    let connections:Vec<_>=page.neighbors().iter().map(|n|json!({"id":n.node().node_id().as_str(),"relation":n.edge().edge_type().as_str(),"properties":n.node().data().properties()})).collect();
    let binding = node
        .data()
        .binding()
        .ok_or(IslandError::code("failed_precondition.island.binding"))?;
    let resolved = graph.resolve_binding_target_at_version(binding.target(), version)?;
    Ok(
        json!({"branch":branch,"version":version,"place":p,"connections":connections,"binding":binding,"binding_status":format!("{resolved:?}")}),
    )
}

pub fn impact(
    parent: &Snapshot,
    child: &Snapshot,
    branch: &str,
    origin: &str,
) -> Result<Value, IslandError> {
    let before = discover(parent, "city", origin, None, 100_000)?;
    let after = discover(child, branch, origin, None, 100_000)?;
    let distances = |v: &Value| -> BTreeMap<String, f64> {
        v["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["place"]["id"].as_str().unwrap().to_owned(),
                    r["distance_m"].as_f64().unwrap(),
                )
            })
            .collect()
    };
    let a = distances(&before);
    let b = distances(&after);
    let rows: Vec<_> = parent
        .places
        .iter()
        .map(|p| {
            let status = match (a.get(&p.id), b.get(&p.id)) {
                (None, None) => "already_unreachable",
                (Some(_), None) => "newly_unreachable",
                (None, Some(_)) => "newly_reachable",
                (Some(x), Some(y)) if y > x => "farther",
                (Some(x), Some(y)) if y < x => "closer",
                _ => "unchanged",
            };
            json!({"place":p,"status":status,"before_m":a.get(&p.id),"after_m":b.get(&p.id)})
        })
        .collect();
    let started = Instant::now();
    let components = child.roads.wcc();
    let weak_components = components
        .components()
        .iter()
        .collect::<BTreeSet<_>>()
        .len();
    Ok(
        json!({"branch":branch,"parent_version":parent.version,"version":child.version,"results":rows,"weak_components":weak_components,"sssp_ms":before["algorithm_ms"].as_f64().unwrap()+after["algorithm_ms"].as_f64().unwrap(),"wcc_ms":started.elapsed().as_secs_f64()*1000.}),
    )
}
