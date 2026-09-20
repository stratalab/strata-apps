//! Address persistence adapter. Replace hydration/publication fallbacks with #3485/#3486.
use crate::{error::IslandError, extract, places, scenario, store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, OnceLock},
    time::Instant,
};
use stratadb::{
    graph::*,
    json::{JsonDocumentId, JsonPath, JsonSetEntry, JsonValue},
    CommitVersion, Database,
};
pub const FIXTURE: &str = include_str!(concat!(env!("OUT_DIR"), "/addresses-v1.json"));
pub const GRAPH: &str = "addresses_v1";
pub const READY: &str = "ready:addresses-v1";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Address {
    #[serde(flatten)]
    pub place: places::Place,
    pub house: String,
    pub street: String,
    pub street_id: String,
    pub bin: Option<String>,
    pub zip: Option<String>,
    pub source_rows: Vec<String>,
    pub source: Value,
    pub attachment_evidence: Value,
    pub place_ids: Vec<String>,
}
#[derive(Deserialize)]
pub struct Fixture {
    pub addresses: Vec<Address>,
}
pub fn fixture() -> Result<Vec<Address>, IslandError> {
    serde_json::from_str::<Fixture>(FIXTURE)
        .map(|f| f.addresses)
        .map_err(|_| IslandError::code("invalid_argument.island.addresses"))
}
pub fn hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| format!("{:016x}", extract::fnv1a64(FIXTURE.as_bytes())))
        .clone()
}
pub fn ready_version(db: &Database, branch: &str) -> Result<Option<CommitVersion>, IslandError> {
    Ok(db
        .json(store::branch(branch)?, store::space()?)?
        .get_versioned(&JsonDocumentId::new(READY)?, &JsonPath::root())?
        .map(|r| r.version()))
}
fn node(
    id: &str,
    kind: &str,
    name: &str,
    binding: bool,
) -> Result<(GraphNodeId, GraphNodeData), IslandError> {
    Ok((
        GraphNodeId::new(id)?,
        GraphNodeData::new(
            Some(GraphProperties::new(json!({"name":name}))?),
            if binding {
                Some(GraphEntityBinding::new(GraphBindingTarget::new(
                    GraphBindingPrimitive::Json,
                    None,
                    store::space()?,
                    id,
                )?))
            } else {
                None
            },
        )
        .with_object_type(GraphTypeName::new(kind)?),
    ))
}
type Edge = (GraphNodeId, GraphEdgeType, GraphNodeId, GraphEdgeData);
pub fn graph_rows(
    rows: &[Address],
) -> Result<(Vec<(GraphNodeId, GraphNodeData)>, Vec<Edge>), IslandError> {
    let mut nodes = BTreeMap::new();
    let mut edges = Vec::new();
    for a in rows {
        let (id, data) = node(&a.place.id, "address", &a.place.name, true)?;
        nodes.insert(id, data);
        let mut targets = vec![(a.street_id.clone(), "street", a.street.clone(), "on_street")];
        if let Some(bin) = &a.bin {
            targets.push((
                format!("building:{bin}"),
                "building",
                format!("Building {bin}"),
                "in_building",
            ));
        }
        if let Some(road) = &a.place.node {
            targets.push((
                format!("anchor:{road}"),
                "anchor",
                road.clone(),
                "near_road",
            ));
        }
        for (target, kind, name, relation) in targets {
            let (id, data) = node(&target, kind, &name, false)?;
            nodes.insert(id, data);
            edges.push((
                GraphNodeId::new(&a.place.id)?,
                GraphEdgeType::new(relation)?,
                GraphNodeId::new(target)?,
                GraphEdgeData::default(),
            ));
        }
        for pid in &a.place_ids {
            let ref_id = format!("ref:{pid}");
            let (id, data) = node(&ref_id, "place_ref", pid, false)?;
            nodes.insert(id, data);
            edges.push((
                GraphNodeId::new(ref_id)?,
                GraphEdgeType::new("at_address")?,
                GraphNodeId::new(&a.place.id)?,
                GraphEdgeData::default(),
            ));
        }
    }
    Ok((nodes.into_iter().collect(), edges))
}
pub fn import_on(db: &Database, branch: &str) -> Result<(), IslandError> {
    let digest = hash();
    if let Some(ready) = store::read_json(db, branch, READY)? {
        if ready["hash"] != digest {
            return Err(IslandError::code(
                "failed_precondition.island.address_catalog",
            ));
        }
        return Ok(());
    }
    import_rows(db, branch, &fixture()?, &digest)
}
/// Small fixtures exercise the identical import/replay path without requiring a city-scale crash test per checkpoint.
pub fn import_rows(
    db: &Database,
    branch: &str,
    rows: &[Address],
    digest: &str,
) -> Result<(), IslandError> {
    if let Some(r) = store::read_json(db, branch, READY)? {
        if r["hash"] != digest {
            return Err(IslandError::code(
                "failed_precondition.island.address_catalog",
            ));
        }
        return Ok(());
    }
    let started = Instant::now();
    eprintln!("addresses: importing {} records on {branch}", rows.len());
    let name = GraphName::new(GRAPH)?;
    let mut graph = db.graph(store::branch(branch)?, store::space()?)?;
    if graph.graph_info(&name)?.is_none() {
        graph.create_graph(name.clone())?;
    }
    if !graph
        .ontology(&name)?
        .is_some_and(|o| o.status() == GraphOntologyStatus::Frozen)
    {
        for ty in ["address", "street", "building", "anchor", "place_ref"] {
            graph
                .define_object_type(&name, GraphObjectTypeDef::new(GraphTypeName::new(ty)?, [])?)?;
        }
        for (rel, src, dst) in [
            ("on_street", "address", "street"),
            ("in_building", "address", "building"),
            ("near_road", "address", "anchor"),
            ("at_address", "place_ref", "address"),
        ] {
            graph.define_link_type(
                &name,
                GraphLinkTypeDef::new(
                    GraphTypeName::new(rel)?,
                    GraphTypeName::new(src)?,
                    GraphTypeName::new(dst)?,
                    None,
                    [],
                )?,
            )?;
        }
        graph.freeze_ontology(&name)?;
    }
    drop(graph);
    for (i, chunk) in rows.chunks(256).enumerate() {
        let entries = chunk
            .iter()
            .map(|a| {
                Ok(JsonSetEntry::new(
                    JsonDocumentId::new(&a.place.id)?,
                    JsonPath::root(),
                    JsonValue::new(serde_json::to_value(a).unwrap())?,
                ))
            })
            .collect::<Result<Vec<_>, IslandError>>()?;
        db.json(store::branch(branch)?, store::space()?)?
            .batch_set_or_create(entries)?;
        if i == 0 {
            scenario::checkpoint("address-documents");
        }
    }
    let (nodes, edges) = graph_rows(rows)?;
    let mut graph = db.graph(store::branch(branch)?, store::space()?)?;
    // Two stages permit deterministic recovery checks between nodes and edges.
    graph.bulk_insert(&name, &nodes, &[], Some(512))?;
    scenario::checkpoint("address-nodes");
    graph.bulk_insert(&name, &[], &edges, Some(512))?;
    scenario::checkpoint("address-edges");
    let info = graph.graph_info(&name)?.ok_or(IslandError::code(
        "failed_precondition.island.address_catalog",
    ))?;
    if info.node_count() != nodes.len() as u64 || info.edge_count() != edges.len() as u64 {
        return Err(IslandError::code(
            "failed_precondition.island.address_catalog",
        ));
    }
    drop(graph);
    // Verify every bound document and graph relationship before publication.
    let staged = db
        .json(store::branch(branch)?, store::space()?)?
        .get_versioned(
            &JsonDocumentId::new(
                &rows
                    .last()
                    .ok_or(IslandError::code("invalid_argument.island.addresses"))?
                    .place
                    .id,
            )?,
            &JsonPath::root(),
        )?
        .unwrap()
        .version();
    // Documents are verified at their completed batch version; the staging graph is
    // checked at Latest while the import owns the only writer.
    let mut g = db.graph(store::branch(branch)?, store::space()?)?;
    let index = g.adjacency_index(&name, &GraphAnalyticsBudget::new(nodes.len(), edges.len()))?;
    validate_memberships(rows, &index)?;
    for a in rows {
        let v = db
            .json(store::branch(branch)?, store::space()?)?
            .get_at_version(
                &JsonDocumentId::new(&a.place.id)?,
                &JsonPath::root(),
                staged,
            )?
            .ok_or(IslandError::code(
                "failed_precondition.island.address_catalog",
            ))?;
        if v.as_inner() != &serde_json::to_value(a).unwrap() {
            return Err(IslandError::code(
                "failed_precondition.island.address_catalog",
            ));
        }
    }
    scenario::checkpoint("address-validated");
    store::write_json(
        db,
        branch,
        READY,
        json!({"hash":digest,"catalog":"addresses-v1","addresses":rows.len(),"nodes":nodes.len(),"edges":edges.len(),"import_ms":started.elapsed().as_millis() as u64}),
    )?;
    scenario::checkpoint("address-ready");
    eprintln!(
        "addresses: published {branch} in {:.1}s ({} nodes, {} edges)",
        started.elapsed().as_secs_f64(),
        nodes.len(),
        edges.len()
    );
    Ok(())
}
fn validate_memberships(rows: &[Address], graph: &GraphAdjacencyIndex) -> Result<(), IslandError> {
    for a in rows {
        let id = GraphNodeId::new(&a.place.id)?;
        let i = graph.node_index(&id).ok_or(IslandError::code(
            "failed_precondition.island.address_catalog",
        ))?;
        let actual: BTreeSet<_> = graph
            .outgoing(i)
            .iter()
            .map(|e| {
                (
                    graph
                        .edge_type_name(e.edge_type())
                        .unwrap()
                        .as_str()
                        .to_owned(),
                    graph.node_id(e.neighbor()).unwrap().as_str().to_owned(),
                )
            })
            .collect();
        let mut expected = BTreeSet::from([("on_street".to_owned(), a.street_id.clone())]);
        if let Some(b) = &a.bin {
            expected.insert(("in_building".into(), format!("building:{b}")));
        }
        if let Some(n) = &a.place.node {
            expected.insert(("near_road".into(), format!("anchor:{n}")));
        }
        let linked: BTreeSet<_> = graph
            .incoming(i)
            .iter()
            .filter(|e| graph.edge_type_name(e.edge_type()).unwrap().as_str() == "at_address")
            .map(|e| graph.node_id(e.neighbor()).unwrap().as_str().to_owned())
            .collect();
        let expected_links: BTreeSet<_> = a.place_ids.iter().map(|p| format!("ref:{p}")).collect();
        if linked != expected_links {
            return Err(IslandError::code(
                "failed_precondition.island.address_membership",
            ));
        }
        if actual != expected {
            return Err(IslandError::code(
                "failed_precondition.island.address_membership",
            ));
        }
    }
    Ok(())
}
pub struct Catalog {
    pub rows: Vec<Address>,
    pub by_id: BTreeMap<String, usize>,
    pub search: crate::search::Index,
    pub hash: String,
    pub ready: CommitVersion,
    pub load_ms: f64,
    pub nodes: u64,
    pub edges: u64,
}
pub fn load(
    db: &Database,
    branch: &str,
    version: CommitVersion,
    places: &[places::Place],
) -> Result<Arc<Catalog>, IslandError> {
    let started = Instant::now();
    let mut docs = db.json(store::branch(branch)?, store::space()?)?;
    let ready = docs
        .get_at_version(&JsonDocumentId::new(READY)?, &JsonPath::root(), version)?
        .ok_or(IslandError::code(
            "failed_precondition.island.address_version",
        ))?;
    let manifest = ready.as_inner();
    let count = manifest["addresses"].as_u64().ok_or(IslandError::code(
        "failed_precondition.island.address_catalog",
    ))? as usize;
    let mut graph = db.graph(store::branch(branch)?, store::space()?)?;
    let name = GraphName::new(GRAPH)?;
    // Single bounded type read avoids repeatedly scanning the complete typed index (#3473).
    let page = graph.nodes_by_type_at_version(
        &name,
        &GraphTypeName::new("address")?,
        None,
        count + 1,
        version,
    )?;
    let mut rows = Vec::with_capacity(count);
    for n in page.nodes() {
        let binding = n
            .data()
            .binding()
            .ok_or(IslandError::code("failed_precondition.island.binding"))?;
        if binding.target().key() != n.node_id().as_str() || binding.target().branch().is_some() {
            return Err(IslandError::code("failed_precondition.island.binding"));
        }
        let val = docs
            .get_at_version(
                &JsonDocumentId::new(binding.target().key())?,
                &JsonPath::root(),
                version,
            )?
            .ok_or(IslandError::code("failed_precondition.island.binding"))?;
        let a: Address = serde_json::from_value(val.as_inner().clone())
            .map_err(|_| IslandError::code("failed_precondition.island.address_catalog"))?;
        if a.place.id != n.node_id().as_str() {
            return Err(IslandError::code("failed_precondition.island.binding"));
        }
        rows.push(a);
    }
    if rows.len() != count {
        return Err(IslandError::code(
            "failed_precondition.island.address_catalog",
        ));
    }
    let index = graph.adjacency_index_at_version(
        &name,
        &GraphAnalyticsBudget::new(
            manifest["nodes"].as_u64().unwrap() as usize,
            manifest["edges"].as_u64().unwrap() as usize,
        ),
        version,
    )?;
    validate_memberships(&rows, &index)?;
    drop(index);
    drop(page);
    let by_id = rows
        .iter()
        .enumerate()
        .map(|(i, a)| (a.place.id.clone(), i))
        .collect();
    let search = crate::search::Index::new(&rows, places);
    Ok(Arc::new(Catalog {
        rows,
        by_id,
        search,
        hash: manifest["hash"].as_str().unwrap().into(),
        ready: ready_version(db, branch)?.unwrap(),
        load_ms: started.elapsed().as_secs_f64() * 1000.,
        nodes: manifest["nodes"].as_u64().unwrap(),
        edges: manifest["edges"].as_u64().unwrap(),
    }))
}
pub fn detail(
    db: &Database,
    branch: &str,
    version: CommitVersion,
    id: &str,
) -> Result<Value, IslandError> {
    let mut graph = db.graph(store::branch(branch)?, store::space()?)?;
    let name = GraphName::new(GRAPH)?;
    let node = GraphNodeId::new(id)?;
    let n = graph
        .get_node_at_version(&name, &node, version)?
        .ok_or(IslandError::code("not_found.island.address"))?;
    let binding = n
        .data()
        .binding()
        .ok_or(IslandError::code("failed_precondition.island.binding"))?;
    let status = graph.resolve_binding_target_at_version(binding.target(), version)?;
    let value = db
        .json(store::branch(branch)?, store::space()?)?
        .get_at_version(&JsonDocumentId::new(id)?, &JsonPath::root(), version)?
        .ok_or(IslandError::code("not_found.island.address"))?;
    let neighbors = graph.neighbors_at_version(
        &name,
        &node,
        GraphDirection::Outgoing,
        None,
        None,
        10,
        version,
    )?;
    Ok(
        json!({"branch":branch,"version":version,"address":value.as_inner(),"binding_status":format!("{status:?}"),"connections":neighbors.neighbors().iter().map(|n|json!({"id":n.node().node_id().as_str(),"relation":n.edge().edge_type().as_str(),"properties":n.node().data().properties()})).collect::<Vec<_>>()}),
    )
}
/// Storage-backed bounded relationship traversal. No full address adjacency snapshot needed.
pub fn explore(
    db: &Database,
    branch: &str,
    version: CommitVersion,
    seed: &str,
    limit: usize,
) -> Result<Value, IslandError> {
    if !(1..=100).contains(&limit) {
        return Err(IslandError::code("invalid_argument.island.limit"));
    }
    let started = Instant::now();
    let mut graph = db.graph(store::branch(branch)?, store::space()?)?;
    let name = GraphName::new(GRAPH)?;
    let node = graph
        .get_node_at_version(&name, &GraphNodeId::new(seed)?, version)?
        .ok_or(IslandError::code("not_found.island.address"))?;
    let mut nodes = BTreeMap::from([(
        seed.to_owned(),
        json!({"id":seed,"name":node.data().properties().and_then(|p|p.as_inner().get("name")).cloned().unwrap_or(json!(seed))}),
    )]);
    let mut edges = BTreeMap::new();
    let mut frontier = vec![seed.to_owned()];
    let mut truncated = false;
    for _ in 0..2 {
        let mut next = Vec::new();
        for id in frontier {
            let page = graph.neighbors_at_version(
                &name,
                &GraphNodeId::new(&id)?,
                GraphDirection::Both,
                None,
                None,
                limit + 1,
                version,
            )?;
            truncated |= page.has_more() || page.neighbors().len() > limit;
            for n in page.neighbors() {
                let key = n.node().node_id().as_str();
                if !nodes.contains_key(key) {
                    if nodes.len() >= limit {
                        truncated = true;
                        continue;
                    }
                    nodes.insert(key.into(),json!({"id":key,"name":n.node().data().properties().and_then(|p|p.as_inner().get("name")).cloned().unwrap_or(json!(key))}));
                    next.push(key.to_owned());
                }
                let e = n.edge();
                edges.insert((e.src().as_str().to_owned(),e.edge_type().as_str().to_owned(),e.dst().as_str().to_owned()),
                    json!({"source":e.src().as_str(),"target":e.dst().as_str(),"relation":e.edge_type().as_str()}));
            }
        }
        frontier = next;
    }
    Ok(
        json!({"branch":branch,"version":version,"nodes":nodes.values().collect::<Vec<_>>(),"edges":edges.values().collect::<Vec<_>>(),"depth":2,"truncated":truncated,"algorithm":"Strata typed neighbors · bounded two-hop traversal","algorithm_ms":started.elapsed().as_secs_f64()*1000.}),
    )
}
pub fn impact_status(connected: bool, before: Option<f64>, after: Option<f64>) -> &'static str {
    if !connected {
        return "unconnected";
    }
    match (before, after) {
        (None, None) => "already_unreachable",
        (Some(_), None) => "newly_unreachable",
        (None, Some(_)) => "newly_reachable",
        (Some(a), Some(b)) if b > a => "farther",
        (Some(a), Some(b)) if b < a => "closer",
        _ => "unchanged",
    }
}
