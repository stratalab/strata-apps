//! Strata adapter for the island.
//!
//! Load-bearing rules (see docs/implementation-plan.md):
//! 1. `World` holds `Mutex<Database>`. Import / resume take it. HTTP those
//!    paths `spawn_blocking`. Never hold db across `.await`. Camera and
//!    Dijkstra never call capability APIs.
//! 2. `json` / `event` / `graph` are `&self` on `Database` and may be held
//!    together. `branches()` is `&mut self`. `GraphService` mutations still
//!    `&mut self`.
//! 3. No public cross-capability `CommitPlan`. Crash between `bulk_insert`
//!    chunks is real. Resume rebuilds from graph rows. JSON `meta.status`
//!    is the watermark (#3464).
//! 4. Weights: finite `f64` of integer meters. Import refuses `fract() != 0`
//!    or `< 1`. Store the same integer as property `length_m`.
//! 5. Gazetteer JSON is written before `bulk_insert`. Bindings omit `branch`.
//! 6. Never delete `city` in the V1 UI. `promote` is not called.
//! 7. Space `city`, graph `manhattan`, edge `street`.
//! 8. No IPC (#3463). Do not tell the UI to shell out to `strata`.
//! 9. Memory budget like pit when `--memory-mb` is set.

#![allow(clippy::result_large_err)] // EngineError is large; boxing it is worse than matching ksp.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};
use stratadb::branch::{BranchStateSelector, ComparedCapability};
use stratadb::event::{EventPayload, EventRangeDirection, EventSequence, EventType};
use stratadb::graph::{
    GraphAdjacencyIndex, GraphAnalyticsBudget, GraphBindingPrimitive, GraphBindingTarget,
    GraphDirection, GraphEdgeData, GraphEdgeType, GraphEntityBinding, GraphName, GraphNodeData,
    GraphNodeId, GraphProperties,
};
use stratadb::json::{JsonDocumentId, JsonPath, JsonValue};
use stratadb::{BranchName, CacheOpenOptions, Database, DurableLocalOpenOptions, ProductSpace};

use crate::drive::{DriveEdge, DriveIndex};
use crate::error::IslandError;
use crate::extract::{
    self, ClosureEdge, ClosureFixture, Extract, GazetteerPoi, EXTRACT_EDGE_COUNT, EXTRACT_FNV1A64,
    EXTRACT_NODE_COUNT,
};
use crate::geo;
use crate::snapshot::{CompareView, CITY_VERSION, LIVE_CAP};

pub const SPACE: &str = "city";
pub const GRAPH_MANHATTAN: &str = "manhattan";
pub const EDGE_STREET: &str = "street";
pub const DOC_META: &str = "meta";
pub const DOC_CITY: &str = "city";
pub const BRANCH_DEFAULT: &str = "default";
pub const BRANCH_CITY: &str = "city";
pub const ANALYTICS_NODES: usize = 20_000;
pub const ANALYTICS_EDGES: usize = 80_000;
pub const EVENT_CLOSED: &str = "closed";
pub const META_CLOSED: &str = "closed-42nd";
pub const META_FORKING: &str = "forking";

pub fn space() -> Result<ProductSpace, stratadb::EngineError> {
    ProductSpace::new(SPACE)
}

pub fn branch(name: &str) -> Result<BranchName, stratadb::EngineError> {
    BranchName::new(name)
}

pub fn graph_name() -> Result<GraphName, stratadb::EngineError> {
    GraphName::new(GRAPH_MANHATTAN)
}

pub fn edge_street() -> Result<GraphEdgeType, stratadb::EngineError> {
    GraphEdgeType::new(EDGE_STREET)
}

#[must_use]
pub fn analytics_budget() -> GraphAnalyticsBudget {
    GraphAnalyticsBudget::new(ANALYTICS_NODES, ANALYTICS_EDGES)
}

pub fn extract_hash_hex() -> String {
    format!("{EXTRACT_FNV1A64:016x}")
}

pub fn open_cache(memory_budget_bytes: Option<u64>) -> Result<Database, stratadb::EngineError> {
    let mut options = CacheOpenOptions::new();
    if let Some(bytes) = memory_budget_bytes {
        options = options.with_memory_budget(bytes);
    }
    Ok(Database::open_cache(options)?.into_database())
}

pub fn open_local(
    path: &Path,
    memory_budget_bytes: Option<u64>,
) -> Result<Database, stratadb::EngineError> {
    let mut options =
        DurableLocalOpenOptions::new().with_durability(stratadb::DurabilityMode::Always);
    if let Some(bytes) = memory_budget_bytes {
        options = options.with_memory_budget(bytes);
    }
    Ok(Database::open_local(path, options)?.into_database())
}

pub fn import_city(
    db: &mut Database,
    extract: &Extract,
    gazetteer: &[GazetteerPoi],
) -> Result<u64, IslandError> {
    let started = Instant::now();
    write_json(
        db,
        BRANCH_DEFAULT,
        DOC_CITY,
        json!({
            "version": CITY_VERSION,
            "city_version": CITY_VERSION,
            "extract_hash": extract_hash_hex(),
            "nodes": EXTRACT_NODE_COUNT,
            "edges": EXTRACT_EDGE_COUNT,
        }),
    )?;

    ensure_city_branch(db)?;

    write_json(
        db,
        BRANCH_CITY,
        DOC_META,
        json!({
            "name": BRANCH_CITY,
            "parent": BRANCH_DEFAULT,
            "status": "importing",
            "next_desk": 1,
            "city_version": CITY_VERSION,
        }),
    )?;

    for poi in gazetteer {
        write_json(
            db,
            BRANCH_CITY,
            &poi.id,
            json!({
                "id": poi.id,
                "name": poi.name,
                "kind": poi.kind,
                "node": poi.node,
            }),
        )?;
    }

    let mut graph = db
        .graph(
            branch(BRANCH_CITY).map_err(|e| IslandError::engine(&e))?,
            space().map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?;
    let gname = graph_name().map_err(|e| IslandError::engine(&e))?;
    match graph.create_graph(gname.clone()) {
        Ok(_) => {}
        Err(error) if error.code() == "already_exists.engine.graph" => {}
        Err(error) => return Err(IslandError::engine(&error)),
    }

    let poi_by_node: HashMap<&str, &GazetteerPoi> =
        gazetteer.iter().map(|p| (p.node.as_str(), p)).collect();
    let space_v = space().map_err(|e| IslandError::engine(&e))?;
    let mut nodes = Vec::with_capacity(extract.nodes.len());
    for node in &extract.nodes {
        let mut binding = None;
        if let Some(poi) = poi_by_node.get(node.id.as_str()) {
            let target = GraphBindingTarget::new(
                GraphBindingPrimitive::Json,
                None,
                space_v.clone(),
                poi.id.as_str(),
            )
            .map_err(|e| IslandError::engine(&e))?;
            binding = Some(GraphEntityBinding::new(target));
        }
        let props = GraphProperties::new(json!({"x": node.x, "y": node.y}))
            .map_err(|e| IslandError::engine(&e))?;
        let id = GraphNodeId::new(node.id.as_str()).map_err(|e| IslandError::engine(&e))?;
        nodes.push((id, GraphNodeData::new(Some(props), binding)));
    }

    let street = edge_street().map_err(|e| IslandError::engine(&e))?;
    let mut edges = Vec::with_capacity(extract.edges.len());
    for edge in &extract.edges {
        if (edge.length_m as f64).fract() != 0.0 {
            return Err(IslandError::code("invalid_argument.island.edge_length"));
        }
        let mut obj = serde_json::Map::new();
        obj.insert("length_m".into(), json!(edge.length_m));
        if let Some(name) = &edge.name {
            obj.insert("name".into(), json!(name));
        }
        let props =
            GraphProperties::new(Value::Object(obj)).map_err(|e| IslandError::engine(&e))?;
        let data = GraphEdgeData::new(f64::from(edge.length_m), Some(props))
            .map_err(|e| IslandError::engine(&e))?;
        edges.push((
            GraphNodeId::new(edge.src.as_str()).map_err(|e| IslandError::engine(&e))?,
            street.clone(),
            GraphNodeId::new(edge.dst.as_str()).map_err(|e| IslandError::engine(&e))?,
            data,
        ));
    }

    graph
        .bulk_insert(&gname, &nodes, &edges, None)
        .map_err(|e| IslandError::engine(&e))?;
    drop(graph);

    write_json(
        db,
        BRANCH_CITY,
        DOC_META,
        json!({
            "name": BRANCH_CITY,
            "parent": BRANCH_DEFAULT,
            "status": "imported",
            "next_desk": 1,
            "city_version": CITY_VERSION,
            "nodes": extract.nodes.len() as u64,
            "edges": extract.edges.len() as u64,
            "extract_hash": extract_hash_hex(),
        }),
    )?;

    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(ms)
}

pub fn load_city_index(db: &mut Database, extract: &Extract) -> Result<DriveIndex, IslandError> {
    load_drive_index(db, BRANCH_CITY, extract)
}

pub fn load_drive_index(
    db: &mut Database,
    branch_name: &str,
    extract: &Extract,
) -> Result<DriveIndex, IslandError> {
    let mut graph = db
        .graph(
            branch(branch_name).map_err(|e| IslandError::engine(&e))?,
            space().map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?;
    let gname = graph_name().map_err(|e| IslandError::engine(&e))?;
    let adj = graph
        .adjacency_index(&gname, &analytics_budget())
        .map_err(|e| IslandError::engine(&e))?;
    snapshot_from_graph(&mut graph, &gname, &adj, extract, branch_name)
}

pub struct CloseOutcome {
    pub desk: String,
    pub index: DriveIndex,
    pub closed_edges: usize,
    pub persist_ms: u64,
    pub next_desk: u64,
}

pub struct DeskResume {
    pub name: String,
    pub index: DriveIndex,
    pub closed_edges: usize,
}

pub fn live_product_count(names: &[String]) -> usize {
    names
        .iter()
        .filter(|name| *name == BRANCH_CITY || desk_number(name).is_some())
        .count()
}

#[must_use]
pub fn desk_name(n: u64) -> String {
    format!("desk-{n:04}")
}

#[must_use]
pub fn desk_number(name: &str) -> Option<u64> {
    name.strip_prefix("desk-")?.parse().ok()
}

pub fn list_branch_names(db: &mut Database) -> Result<Vec<String>, IslandError> {
    let names = db
        .branches()
        .map_err(|e| IslandError::engine(&e))?
        .list()
        .map_err(|e| IslandError::engine(&e))?;
    Ok(names
        .iter()
        .map(|summary| summary.name().as_str().to_owned())
        .collect())
}

pub fn close_42nd(
    db: &mut Database,
    extract: &Extract,
    closure: &ClosureFixture,
    next_desk: u64,
) -> Result<CloseOutcome, IslandError> {
    let started = Instant::now();
    let names = list_branch_names(db)?;
    if live_product_count(&names) >= LIVE_CAP {
        return Err(IslandError::code("failed_precondition.island.desk_cap"));
    }
    if crate::places::ready_version(db, BRANCH_CITY)?.is_some() {
        return crate::scenario::create(db, extract, closure, next_desk);
    }
    let desk = desk_name(next_desk);
    let cv = if let Some(version) = crate::places::ready_version(db, BRANCH_CITY)? {
        version
    } else {
        let mut graph = db
            .graph(
                branch(BRANCH_CITY).map_err(|e| IslandError::engine(&e))?,
                space().map_err(|e| IslandError::engine(&e))?,
            )
            .map_err(|e| IslandError::engine(&e))?;
        let gname = graph_name().map_err(|e| IslandError::engine(&e))?;
        let info = graph
            .graph_info(&gname)
            .map_err(|e| IslandError::engine(&e))?
            .ok_or(IslandError::code("failed_precondition.island.city"))?;
        info.updated_version()
    };
    db.branches()
        .map_err(|e| IslandError::engine(&e))?
        .fork_at_version(
            &branch(BRANCH_CITY).map_err(|e| IslandError::engine(&e))?,
            branch(&desk).map_err(|e| IslandError::engine(&e))?,
            cv,
        )
        .map_err(|e| IslandError::engine(&e))?;
    delete_closure_edges(db, &desk, &closure.edges, true)?;
    append_closed_event(db, &desk, closure)?;
    write_desk_meta(db, &desk, META_CLOSED, closure.edges.len())?;
    bump_city_next_desk(db, next_desk.saturating_add(1))?;
    let index = load_drive_index(db, &desk, extract)?;
    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(CloseOutcome {
        desk,
        index,
        closed_edges: closure.edges.len(),
        persist_ms: ms,
        next_desk: next_desk.saturating_add(1),
    })
}

/// Fork, delete, and record the event, but leave meta as `forking` so resume repairs.
pub fn fork_desk_pending(
    db: &mut Database,
    extract: &Extract,
    closure: &ClosureFixture,
    next_desk: u64,
) -> Result<String, IslandError> {
    let desk = desk_name(next_desk);
    let cv = if let Some(version) = crate::places::ready_version(db, BRANCH_CITY)? {
        version
    } else {
        let mut graph = db
            .graph(
                branch(BRANCH_CITY).map_err(|e| IslandError::engine(&e))?,
                space().map_err(|e| IslandError::engine(&e))?,
            )
            .map_err(|e| IslandError::engine(&e))?;
        let gname = graph_name().map_err(|e| IslandError::engine(&e))?;
        let info = graph
            .graph_info(&gname)
            .map_err(|e| IslandError::engine(&e))?
            .ok_or(IslandError::code("failed_precondition.island.city"))?;
        info.updated_version()
    };
    db.branches()
        .map_err(|e| IslandError::engine(&e))?
        .fork_at_version(
            &branch(BRANCH_CITY).map_err(|e| IslandError::engine(&e))?,
            branch(&desk).map_err(|e| IslandError::engine(&e))?,
            cv,
        )
        .map_err(|e| IslandError::engine(&e))?;
    delete_closure_edges(db, &desk, &closure.edges, true)?;
    append_closed_event(db, &desk, closure)?;
    load_drive_index(db, &desk, extract)?;
    write_desk_meta(db, &desk, META_FORKING, 0)?;
    bump_city_next_desk(db, next_desk.saturating_add(1))?;
    Ok(desk)
}

pub fn resume_desks(
    db: &mut Database,
    extract: &Extract,
    closure: &ClosureFixture,
    city_nodes: u64,
    city_edges: u64,
) -> Result<(Vec<DeskResume>, u64), IslandError> {
    let names = list_branch_names(db)?;
    let mut desks = Vec::new();
    let mut max_n = 0_u64;
    let mut to_delete = Vec::new();
    for name in &names {
        let Some(n) = desk_number(name) else {
            continue;
        };
        max_n = max_n.max(n);
        match repair_or_load_desk(db, name, extract, closure, city_nodes, city_edges)? {
            Some(desk) => desks.push(desk),
            None => to_delete.push(name.clone()),
        }
    }
    for name in to_delete {
        delete_branch(db, &name)?;
    }
    let json_next = read_json(db, BRANCH_CITY, DOC_META)?
        .and_then(|value| value.get("next_desk").and_then(Value::as_u64))
        .unwrap_or(1); // first desk is 1 when meta omits the counter
    let next_desk = max_n.saturating_add(1).max(json_next).max(1);
    Ok((desks, next_desk))
}

pub fn compare_branches(db: &mut Database, a: &str, b: &str) -> Result<CompareView, IslandError> {
    let comparison = db
        .branches()
        .map_err(|e| IslandError::engine(&e))?
        .compare(
            &branch(a).map_err(|e| IslandError::engine(&e))?,
            &branch(b).map_err(|e| IslandError::engine(&e))?,
            BranchStateSelector::Current,
        )
        .map_err(|e| IslandError::engine(&e))?;
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut modified = 0usize;
    let mut graph_entities = 0usize;
    let mut capabilities = Vec::new();
    for space_cmp in comparison.comparisons() {
        let n = space_cmp.added().len() + space_cmp.removed().len() + space_cmp.modified().len();
        added += space_cmp.added().len();
        removed += space_cmp.removed().len();
        modified += space_cmp.modified().len();
        capabilities.push(format!("{:?}", space_cmp.capability()));
        if matches!(
            space_cmp.capability(),
            ComparedCapability::GraphMetadata
                | ComparedCapability::GraphNode
                | ComparedCapability::GraphEdge
                | ComparedCapability::GraphOntology
        ) {
            graph_entities += n;
        }
    }
    Ok(CompareView {
        a: a.to_owned(),
        b: b.to_owned(),
        empty: comparison.is_empty(),
        added,
        removed,
        modified,
        graph_entities,
        capabilities,
    })
}

pub fn delete_branch(db: &mut Database, name: &str) -> Result<(), IslandError> {
    db.branches()
        .map_err(|e| IslandError::engine(&e))?
        .delete(&branch(name).map_err(|e| IslandError::engine(&e))?)
        .map_err(|e| IslandError::engine(&e))?;
    Ok(())
}

pub fn city_graph_info(db: &mut Database) -> Result<(u64, u64), IslandError> {
    graph_counts(db, BRANCH_CITY)?.ok_or(IslandError::code("failed_precondition.island.city"))
}

pub fn verify_chain(db: &mut Database, branch_name: &str) -> Result<bool, IslandError> {
    let outcome = db
        .event(
            branch(branch_name).map_err(|e| IslandError::engine(&e))?,
            space().map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?
        .verify_chain()
        .map_err(|e| IslandError::engine(&e))?;
    Ok(outcome.is_valid())
}

pub fn read_gazetteer_poi_on(
    db: &Database,
    branch_name: &str,
    poi_id: &str,
) -> Result<GazetteerPoi, IslandError> {
    let Some(value) = read_json(db, branch_name, poi_id)? else {
        return Err(IslandError::code("not_found.island.node"));
    };
    extract::poi_from_value(&value)
}

fn repair_or_load_desk(
    db: &mut Database,
    name: &str,
    extract: &Extract,
    closure: &ClosureFixture,
    city_nodes: u64,
    city_edges: u64,
) -> Result<Option<DeskResume>, IslandError> {
    if let Some(count) = crate::scenario::recover(db, name, extract)? {
        return Ok(Some(DeskResume {
            name: name.to_owned(),
            index: load_drive_index(db, name, extract)?,
            closed_edges: count,
        }));
    }
    let n = closure.edges.len() as u64;
    let (node_count, edge_count) = match graph_counts(db, name)? {
        Some(counts) => counts,
        None => return Ok(None),
    };
    let status = read_json(db, name, DOC_META)?.and_then(|value| {
        value
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let closed =
        status.as_deref() == Some(META_CLOSED) && edge_count == city_edges.saturating_sub(n);
    if !closed {
        if node_count != city_nodes {
            return Ok(None);
        }
        delete_closure_edges(db, name, &closure.edges, false)?;
        if !has_closed_event(db, name)? {
            append_closed_event(db, name, closure)?;
        }
        write_desk_meta(db, name, META_CLOSED, closure.edges.len())?;
    }
    let index = load_drive_index(db, name, extract)?;
    Ok(Some(DeskResume {
        name: name.to_owned(),
        index,
        closed_edges: closure.edges.len(),
    }))
}

fn graph_counts(db: &mut Database, branch_name: &str) -> Result<Option<(u64, u64)>, IslandError> {
    let mut graph = db
        .graph(
            branch(branch_name).map_err(|e| IslandError::engine(&e))?,
            space().map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?;
    let gname = graph_name().map_err(|e| IslandError::engine(&e))?;
    Ok(graph
        .graph_info(&gname)
        .map_err(|e| IslandError::engine(&e))?
        .map(|info| (info.node_count(), info.edge_count())))
}

fn delete_closure_edges(
    db: &mut Database,
    branch_name: &str,
    edges: &[ClosureEdge],
    require_present: bool,
) -> Result<usize, IslandError> {
    let mut graph = db
        .graph(
            branch(branch_name).map_err(|e| IslandError::engine(&e))?,
            space().map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?;
    let gname = graph_name().map_err(|e| IslandError::engine(&e))?;
    let street = edge_street().map_err(|e| IslandError::engine(&e))?;
    let mut deleted = 0usize;
    for edge in edges {
        let src = GraphNodeId::new(edge.src.as_str()).map_err(|e| IslandError::engine(&e))?;
        let dst = GraphNodeId::new(edge.dst.as_str()).map_err(|e| IslandError::engine(&e))?;
        let outcome = graph
            .delete_edge(&gname, &src, &street, &dst)
            .map_err(|e| IslandError::engine(&e))?;
        if outcome.deleted() {
            deleted += 1;
        } else if require_present {
            return Err(IslandError::code(
                "failed_precondition.island.closure_missing",
            ));
        }
    }
    Ok(deleted)
}

fn append_closed_event(
    db: &Database,
    desk: &str,
    closure: &ClosureFixture,
) -> Result<(), IslandError> {
    let mut event = db
        .event(
            branch(desk).map_err(|e| IslandError::engine(&e))?,
            space().map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?;
    let edges: Vec<Value> = closure
        .edges
        .iter()
        .map(|edge| json!({"src": edge.src, "dst": edge.dst}))
        .collect();
    let payload = event_payload(json!({
        "street": closure.name,
        "desk": desk,
        "edges": edges,
    }))?;
    event
        .append(
            EventType::new(EVENT_CLOSED).map_err(|e| IslandError::engine(&e))?,
            payload,
        )
        .map_err(|e| IslandError::engine(&e))?;
    Ok(())
}

fn has_closed_event(db: &Database, desk: &str) -> Result<bool, IslandError> {
    let mut event = db
        .event(
            branch(desk).map_err(|e| IslandError::engine(&e))?,
            space().map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?;
    let closed = EventType::new(EVENT_CLOSED).map_err(|e| IslandError::engine(&e))?;
    let page = event
        .range(
            EventSequence::new(0),
            None,
            Some(16),
            EventRangeDirection::Forward,
            Some(&closed),
        )
        .map_err(|e| IslandError::engine(&e))?;
    Ok(!page.events().is_empty())
}

fn write_desk_meta(
    db: &Database,
    desk: &str,
    status: &str,
    closed_edges: usize,
) -> Result<(), IslandError> {
    write_json(
        db,
        desk,
        DOC_META,
        json!({
            "name": desk,
            "parent": BRANCH_CITY,
            "status": status,
            "closed_edges": closed_edges as u64,
            "city_version": CITY_VERSION,
        }),
    )
}

fn bump_city_next_desk(db: &Database, next_desk: u64) -> Result<(), IslandError> {
    let mut meta = match read_json(db, BRANCH_CITY, DOC_META)? {
        Some(value) => value,
        None => json!({
            "name": BRANCH_CITY,
            "parent": BRANCH_DEFAULT,
            "status": "imported",
        }),
    };
    if let Some(object) = meta.as_object_mut() {
        object.insert("next_desk".into(), json!(next_desk));
    }
    write_json(db, BRANCH_CITY, DOC_META, meta)
}

fn event_payload(value: Value) -> Result<EventPayload, IslandError> {
    let bytes = serde_json::to_vec(&value).expect("closed event encodes as JSON");
    let canonical: Value = serde_json::from_slice(&bytes).expect("closed event JSON roundtrips");
    EventPayload::new(canonical).map_err(|e| IslandError::engine(&e))
}

/// Resume hydrates the picker from JSON docs, not from the extract fixture.
pub fn load_gazetteer(
    db: &Database,
    seed: &[GazetteerPoi],
) -> Result<Vec<GazetteerPoi>, IslandError> {
    let mut pois = Vec::with_capacity(seed.len());
    for seed_poi in seed {
        let Some(value) = read_json(db, BRANCH_CITY, &seed_poi.id)? else {
            return Err(IslandError::code("failed_precondition.island.city"));
        };
        let poi = extract::poi_from_value(&value)?;
        if poi.id != seed_poi.id {
            return Err(IslandError::code("failed_precondition.island.city"));
        }
        pois.push(poi);
    }
    Ok(pois)
}

pub fn read_gazetteer_poi(db: &Database, poi_id: &str) -> Result<GazetteerPoi, IslandError> {
    let Some(value) = read_json(db, BRANCH_CITY, poi_id)? else {
        return Err(IslandError::code("not_found.island.node"));
    };
    extract::poi_from_value(&value)
}

/// Graph → JSON binding. Bindings omit `branch` (#3466).
pub fn binding_node_for_poi(db: &Database, poi_id: &str) -> Result<String, IslandError> {
    let mut graph = db
        .graph(
            branch(BRANCH_CITY).map_err(|e| IslandError::engine(&e))?,
            space().map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?;
    let target = GraphBindingTarget::new(
        GraphBindingPrimitive::Json,
        None,
        space().map_err(|e| IslandError::engine(&e))?,
        poi_id,
    )
    .map_err(|e| IslandError::engine(&e))?;
    let page = graph
        .bindings_for_entity(&target, None, 8)
        .map_err(|e| IslandError::engine(&e))?;
    let [hit] = page.bindings() else {
        return Err(IslandError::code("failed_precondition.island.city"));
    };
    if hit.graph().as_str() != GRAPH_MANHATTAN {
        return Err(IslandError::code("failed_precondition.island.city"));
    }
    if hit.binding().target().branch().is_some() {
        return Err(IslandError::code("failed_precondition.island.city"));
    }
    Ok(hit.node_id().as_str().to_owned())
}

pub fn node_binding_poi(db: &Database, node_id: &str) -> Result<String, IslandError> {
    let mut graph = db
        .graph(
            branch(BRANCH_CITY).map_err(|e| IslandError::engine(&e))?,
            space().map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?;
    let gname = graph_name().map_err(|e| IslandError::engine(&e))?;
    let id = GraphNodeId::new(node_id).map_err(|e| IslandError::engine(&e))?;
    let node = graph
        .get_node(&gname, &id)
        .map_err(|e| IslandError::engine(&e))?
        .ok_or(IslandError::code("not_found.island.node"))?;
    let binding = node
        .data()
        .binding()
        .ok_or(IslandError::code("failed_precondition.island.city"))?;
    let target = binding.target();
    if target.primitive() != GraphBindingPrimitive::Json || target.branch().is_some() {
        return Err(IslandError::code("failed_precondition.island.city"));
    }
    Ok(target.key().to_owned())
}

/// Engine `sssp` is distances only (#3456). Tests round that distance;
/// the click path unpacks predecessors in `route.rs`.
pub fn sssp_distance_m(db: &Database, from: &str, to: &str) -> Result<u32, IslandError> {
    let mut graph = db
        .graph(
            branch(BRANCH_CITY).map_err(|e| IslandError::engine(&e))?,
            space().map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?;
    let gname = graph_name().map_err(|e| IslandError::engine(&e))?;
    let adj = graph
        .adjacency_index(&gname, &analytics_budget())
        .map_err(|e| IslandError::engine(&e))?;
    let source = GraphNodeId::new(from).map_err(|e| IslandError::engine(&e))?;
    let dest = GraphNodeId::new(to).map_err(|e| IslandError::engine(&e))?;
    let result = adj
        .sssp(&source, GraphDirection::Outgoing)
        .map_err(|e| IslandError::engine(&e))?;
    let Some(index) = adj.node_index(&dest) else {
        return Err(IslandError::code("not_found.island.node"));
    };
    let Some(distance) = result.distance(index) else {
        return Err(IslandError::code("failed_precondition.island.unreachable"));
    };
    if !distance.is_finite() {
        return Err(IslandError::code("failed_precondition.island.unreachable"));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(distance.round() as u32)
}

pub fn read_meta_status(db: &mut Database) -> Result<Option<String>, IslandError> {
    match read_json(db, BRANCH_CITY, DOC_META)? {
        Some(value) => Ok(value
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_owned)),
        None => Ok(None),
    }
}

pub fn read_default_spec(db: &mut Database) -> Result<Option<Value>, IslandError> {
    read_json(db, BRANCH_DEFAULT, DOC_CITY)
}

pub fn spec_matches(spec: &Value) -> bool {
    spec.get("extract_hash").and_then(Value::as_str) == Some(extract_hash_hex().as_str())
        && spec.get("city_version").and_then(Value::as_u64) == Some(u64::from(CITY_VERSION))
}

fn ensure_city_branch(db: &mut Database) -> Result<(), IslandError> {
    let names = db
        .branches()
        .map_err(|e| IslandError::engine(&e))?
        .list()
        .map_err(|e| IslandError::engine(&e))?;
    let has_city = names.iter().any(|s| s.name().as_str() == BRANCH_CITY);
    if has_city {
        return Ok(());
    }
    db.branches()
        .map_err(|e| IslandError::engine(&e))?
        .fork_current(
            &branch(BRANCH_DEFAULT).map_err(|e| IslandError::engine(&e))?,
            branch(BRANCH_CITY).map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?;
    Ok(())
}

pub fn write_json(
    db: &Database,
    branch_name: &str,
    doc_id: &str,
    value: Value,
) -> Result<(), IslandError> {
    let mut json = db
        .json(
            branch(branch_name).map_err(|e| IslandError::engine(&e))?,
            space().map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?;
    json.set_or_create(
        JsonDocumentId::new(doc_id).map_err(|e| IslandError::engine(&e))?,
        &JsonPath::root(),
        JsonValue::new(value).map_err(|e| IslandError::engine(&e))?,
    )
    .map_err(|e| IslandError::engine(&e))?;
    Ok(())
}

pub fn read_json(
    db: &Database,
    branch_name: &str,
    doc_id: &str,
) -> Result<Option<Value>, IslandError> {
    let mut json = db
        .json(
            branch(branch_name).map_err(|e| IslandError::engine(&e))?,
            space().map_err(|e| IslandError::engine(&e))?,
        )
        .map_err(|e| IslandError::engine(&e))?;
    match json
        .get(
            &JsonDocumentId::new(doc_id).map_err(|e| IslandError::engine(&e))?,
            &JsonPath::root(),
        )
        .map_err(|e| IslandError::engine(&e))?
    {
        Some(value) => Ok(Some(value.into_inner())),
        None => Ok(None),
    }
}

fn snapshot_from_graph(
    graph: &mut stratadb::graph::GraphService<'_>,
    gname: &GraphName,
    adj: &GraphAdjacencyIndex,
    extract: &Extract,
    branch_name: &str,
) -> Result<DriveIndex, IslandError> {
    let names = extract::name_join(extract);
    let mut xy = vec![(0_i32, 0_i32); adj.node_count()];
    let mut seen = vec![false; adj.node_count()];
    let mut cursor: Option<GraphNodeId> = None;
    loop {
        let page = graph
            .list_nodes(gname, None, cursor.as_ref(), 512)
            .map_err(|e| IslandError::engine(&e))?;
        for node in page.nodes() {
            let Some(index) = adj.node_index(node.node_id()) else {
                continue;
            };
            let props = node
                .data()
                .properties()
                .ok_or(IslandError::code("failed_precondition.island.city"))?;
            let obj = props.as_inner();
            let x = obj
                .get("x")
                .and_then(Value::as_i64)
                .and_then(|n| i32::try_from(n).ok())
                .ok_or(IslandError::code("failed_precondition.island.city"))?;
            let y = obj
                .get("y")
                .and_then(Value::as_i64)
                .and_then(|n| i32::try_from(n).ok())
                .ok_or(IslandError::code("failed_precondition.island.city"))?;
            if !geo::in_aabb(x, y) {
                return Err(IslandError::code("invalid_argument.island.extract"));
            }
            xy[index] = (x, y);
            seen[index] = true;
        }
        if !page.has_more() {
            break;
        }
        cursor = page.cursor().cloned();
    }
    if seen.iter().any(|ok| !*ok) {
        return Err(IslandError::code("failed_precondition.island.city"));
    }

    let mut outgoing = Vec::with_capacity(adj.node_count());
    for i in 0..adj.node_count() {
        let mut edges = Vec::new();
        for edge in adj.outgoing(i) {
            let weight = edge.weight();
            if !weight.is_finite() || weight.fract() != 0.0 || weight < 1.0 {
                return Err(IslandError::code("invalid_argument.island.edge_length"));
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let length_m = weight as u32;
            let src_id = adj.node_id(i).expect("index").as_str();
            let dst_id = adj.node_id(edge.neighbor()).expect("nbr").as_str();
            let name = names.get(&(src_id.to_owned(), dst_id.to_owned())).cloned();
            edges.push(DriveEdge {
                dst: edge.neighbor(),
                length_m,
                name,
            });
        }
        outgoing.push(edges);
    }

    Ok(DriveIndex {
        branch: branch_name.to_owned(),
        node_ids: adj
            .node_ids()
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect(),
        xy,
        outgoing,
    })
}
