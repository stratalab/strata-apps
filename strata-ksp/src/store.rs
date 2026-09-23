//! Strata adapter for the hangar.
//!
//! Load-bearing rules (see docs/implementation-plan.md):
//! 1. `Database` is exclusive `&mut self`. `World` holds `Mutex<Database>`.
//!    HTTP handlers that read/write the db `spawn_blocking` and take the same
//!    mutex. Never hold the db mutex across an `.await`. The ticker never
//!    calls `event()` / `kv()` / `graph()`.
//! 2. `kv()`, `json()`, `event()`, `graph()`, `branches()` each borrow
//!    `Database` mutably. They cannot be held together. One VAB save is
//!    multiple commits (json, then graph, then event).
//! 3. There is no public cross-capability `CommitPlan`. JSON spec is the
//!    design source of truth; the graph is a projection rebuilt from it.
//! 4. `EventPayload::new` requires a JSON object and rejects non-finite
//!    floats. Telemetry is an object of numbers.
//! 5. `EventService::range` is latest-only. Rewind keys off event sequence.
//! 6. `BranchService::delete` refuses `default` and the last active branch.
//! 7. `create_from_head` ≡ `fork_current`. Historical launch forks (PR6)
//!    call `fork_at_version`.
//! 8. `put_batch` pre-reads every key. One fat `vessel` key (PR5).
//! 9. Library-opened DBs do not host IPC. Do not tell the UI to shell out
//!    to `strata`.
//! 10. Graph edges require both endpoint nodes. `GraphBatchWrite` applies
//!     in order: `UpsertNode` before `UpsertEdge`. Staging and VAB rebuilds
//!     go through `batch_write`.
//! 11. `ProductSpace::new("flight")` once, everywhere. `KvKey::new` takes
//!     bytes: `KvKey::new(b"vessel".as_slice())`.
//! 12. `json.set_or_create` takes `&JsonPath`. Call sites use
//!     `json.set_or_create(id, &JsonPath::root(), value)`.
//! 13. `promote` carries JSON + KV only. After promoting craft JSON onto
//!     `vab`, rebuild the hangar graph. Never `promote(launch, vab)`.

#![allow(clippy::result_large_err)] // EngineError is ~152 bytes; boxing it is worse than matching colonies.

use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};
// 1.2.x namespaces the per-primitive types. Opening, naming and errors stay at
// the crate root; everything that belongs to a primitive now lives under it.
use stratadb::branch::{
    BranchStateSelector, ComparedCapability, PromotionOutcome, PromotionStrategy,
};
use stratadb::event::{EventPayload, EventRangeDirection, EventSequence, EventType};
use stratadb::graph::{
    GraphBatchOperation, GraphBatchWrite, GraphEdgeData, GraphEdgeType, GraphName, GraphNodeData,
    GraphNodeId, GraphProperties, GraphService,
};
use stratadb::json::{JsonDocumentId, JsonPath, JsonValue};
use stratadb::kv::{KvKey, KvValue};
use stratadb::{BranchName, CacheOpenOptions, Database, DurableLocalOpenOptions, ProductSpace};

use crate::craft::{CraftSpec, CATALOG, CATALOG_VERSION};
use crate::physics::{E_MAX, G0, MU, PLANET_VERSION, R, R_PE_MIN};
use crate::telemetry::{self, VesselSample, EVENT_SOFT_CAP, EVENT_TICK};

pub const SPACE: &str = "flight";
pub const GRAPH_CRAFT: &str = "craft";
pub const DOC_CRAFT: &str = "craft";
pub const DOC_CATALOG: &str = "catalog";
pub const DOC_PLANET: &str = "planet";
pub const DOC_META: &str = "meta";
pub const KV_VESSEL: &[u8] = b"vessel";
pub const BRANCH_DEFAULT: &str = "default";
pub const BRANCH_VAB: &str = "vab";
pub const EDGE_STACKED_ON: &str = "stacked_on";
pub const EVENT_VAB_SAVE: &str = "vab_save";
pub const EVENT_PROMOTED: &str = "promoted";

#[derive(Clone, Debug, Default)]
pub struct PersistStats {
    pub commits: u64,
    pub json_us: u64,
    pub graph_us: u64,
    pub event_us: u64,
    pub kv_us: u64,
}

impl PersistStats {
    pub fn total_us(&self) -> u64 {
        self.json_us
            .saturating_add(self.graph_us)
            .saturating_add(self.event_us)
            .saturating_add(self.kv_us)
    }
}

#[derive(Debug)]
pub enum EnsureError {
    Engine(stratadb::EngineError),
    PlanetVersion,
    CatalogVersion,
    Craft(String),
}

impl From<stratadb::EngineError> for EnsureError {
    fn from(error: stratadb::EngineError) -> Self {
        Self::Engine(error)
    }
}

#[derive(Clone, Debug)]
pub struct EnsureOutcome {
    pub spec: CraftSpec,
    pub resumed: bool,
    pub branch_count: u32,
}

pub fn space() -> Result<ProductSpace, stratadb::EngineError> {
    ProductSpace::new(SPACE)
}

pub fn branch(name: &str) -> Result<BranchName, stratadb::EngineError> {
    BranchName::new(name)
}

pub fn open_cache() -> Result<Database, stratadb::EngineError> {
    Ok(Database::open_cache(CacheOpenOptions::new())?.into_database())
}

pub fn open_local(path: &Path) -> Result<Database, stratadb::EngineError> {
    Ok(Database::open_local(path, DurableLocalOpenOptions::new())?.into_database())
}

pub fn list_product_branches(db: &mut Database) -> Result<Vec<String>, stratadb::EngineError> {
    Ok(db
        .branches()?
        .list()?
        .into_iter()
        .map(|summary| summary.name().as_str().to_owned())
        .collect())
}

pub fn read_json_doc(
    db: &mut Database,
    branch_name: &str,
    doc_id: &str,
) -> Result<Option<Value>, stratadb::EngineError> {
    let mut json = db.json(branch(branch_name)?, space()?)?;
    match json.get(&JsonDocumentId::new(doc_id)?, &JsonPath::root())? {
        Some(value) => Ok(Some(value.into_inner())),
        None => Ok(None),
    }
}

pub fn write_json_doc(
    db: &mut Database,
    branch_name: &str,
    doc_id: &str,
    value: Value,
) -> Result<(), stratadb::EngineError> {
    let mut json = db.json(branch(branch_name)?, space()?)?;
    json.set_or_create(
        JsonDocumentId::new(doc_id)?,
        &JsonPath::root(),
        JsonValue::new(value)?,
    )?;
    Ok(())
}

pub fn read_craft(db: &mut Database, branch_name: &str) -> Result<Option<CraftSpec>, EnsureError> {
    match read_json_doc(db, branch_name, DOC_CRAFT)? {
        Some(value) => CraftSpec::from_doc(&value)
            .map(Some)
            .map_err(EnsureError::Craft),
        None => Ok(None),
    }
}

/// Bootstrap `default` (planet + catalog) and `vab` (Sounding Stick), or resume.
pub fn ensure_world_seed(db: &mut Database) -> Result<EnsureOutcome, EnsureError> {
    let names = list_product_branches(db)?;
    let planet = read_json_doc(db, BRANCH_DEFAULT, DOC_PLANET)?;
    if let Some(planet) = planet {
        if !planet_matches(&planet) {
            return Err(EnsureError::PlanetVersion);
        }
        let catalog =
            read_json_doc(db, BRANCH_DEFAULT, DOC_CATALOG)?.ok_or(EnsureError::CatalogVersion)?;
        if !catalog_matches(&catalog) {
            return Err(EnsureError::CatalogVersion);
        }
        if !names.iter().any(|n| n == BRANCH_VAB) {
            fork_vab(db)?;
        }
        let spec = match read_craft(db, BRANCH_VAB)? {
            Some(spec) => spec,
            None => {
                let spec = CraftSpec::sounding_stick();
                save_vab(db, &spec)?;
                write_vab_meta(db)?;
                spec
            }
        };
        let branch_count = list_product_branches(db)?.len() as u32;
        return Ok(EnsureOutcome {
            spec,
            resumed: true,
            branch_count,
        });
    }

    write_json_doc(db, BRANCH_DEFAULT, DOC_PLANET, compiled_planet_doc())?;
    write_json_doc(db, BRANCH_DEFAULT, DOC_CATALOG, compiled_catalog_doc())?;
    if !names.iter().any(|n| n == BRANCH_VAB) {
        fork_vab(db)?;
    }
    write_vab_meta(db)?;
    let spec = CraftSpec::sounding_stick();
    save_vab(db, &spec)?;
    let branch_count = list_product_branches(db)?.len() as u32;
    Ok(EnsureOutcome {
        spec,
        resumed: false,
        branch_count,
    })
}

fn fork_vab(db: &mut Database) -> Result<(), stratadb::EngineError> {
    match db
        .branches()?
        .fork_current(&branch(BRANCH_DEFAULT)?, branch(BRANCH_VAB)?)
    {
        Ok(_) => Ok(()),
        Err(error) if error.code() == "already_exists.engine.branch" => Ok(()),
        Err(error) => Err(error),
    }
}

fn write_vab_meta(db: &mut Database) -> Result<(), stratadb::EngineError> {
    write_json_doc(
        db,
        BRANCH_VAB,
        DOC_META,
        json!({ "next_launch": 1, "next_design": 1 }),
    )
}

/// Three commits on `vab`: JSON spec, graph projection, `vab_save` event.
pub fn save_vab(
    db: &mut Database,
    spec: &CraftSpec,
) -> Result<PersistStats, stratadb::EngineError> {
    let mut stats = PersistStats::default();
    let started = Instant::now();
    {
        let mut json = db.json(branch(BRANCH_VAB)?, space()?)?;
        json.set_or_create(
            JsonDocumentId::new(DOC_CRAFT)?,
            &JsonPath::root(),
            JsonValue::new(spec.to_doc())?,
        )?;
        stats.commits += 1;
    }
    stats.json_us += micros(started.elapsed());

    let started = Instant::now();
    {
        let mut graph = db.graph(branch(BRANCH_VAB)?, space()?)?;
        let name = GraphName::new(GRAPH_CRAFT)?;
        match graph.create_graph(name.clone()) {
            Ok(_) => stats.commits += 1,
            Err(error) if error.code() == "already_exists.engine.graph" => {}
            Err(error) => return Err(error),
        }
        let batch = craft_rebuild_batch(&mut graph, &name, spec)?;
        if !batch.is_empty() {
            graph.batch_write(&name, &batch)?;
            stats.commits += 1;
        }
    }
    stats.graph_us += micros(started.elapsed());

    let started = Instant::now();
    {
        let mut events = db.event(branch(BRANCH_VAB)?, space()?)?;
        let payload = json!({
            "t": 0.0,
            "kind": EVENT_VAB_SAVE,
            "parts": spec.parts.len() as u64,
            "dv_budget": spec.dv_budget_mps(),
        });
        events.append(EventType::new(EVENT_VAB_SAVE)?, event_payload(payload)?)?;
        stats.commits += 1;
    }
    stats.event_us += micros(started.elapsed());
    Ok(stats)
}

fn craft_rebuild_batch(
    graph: &mut GraphService<'_>,
    name: &GraphName,
    spec: &CraftSpec,
) -> Result<GraphBatchWrite, stratadb::EngineError> {
    let page = graph.list_nodes(name, None, None, 1024)?;
    let wanted: HashSet<String> = spec
        .parts
        .iter()
        .enumerate()
        .map(|(i, p)| CraftSpec::node_id(i, &p.part_id))
        .collect();

    let mut ops = Vec::new();
    for node in page.nodes() {
        if !wanted.contains(node.node_id().as_str()) {
            ops.push(GraphBatchOperation::DeleteNode {
                node_id: node.node_id().clone(),
            });
        }
    }

    for (ordinal, part) in spec.parts.iter().enumerate() {
        let def = part.def();
        let props = GraphProperties::new(json!({
            "kind": def.kind.as_str(),
            "part_id": part.part_id,
            "mass_dry": def.dry_kg,
            "fuel": part.fuel,
            "thrust": def.thrust_n,
            "stage": spec.stage_of(ordinal),
        }))?;
        ops.push(GraphBatchOperation::UpsertNode {
            node_id: GraphNodeId::new(CraftSpec::node_id(ordinal, &part.part_id))?,
            data: GraphNodeData::new(Some(props), None),
        });
    }

    let stacked_on = GraphEdgeType::new(EDGE_STACKED_ON)?;
    for ordinal in 1..spec.parts.len() {
        let above = &spec.parts[ordinal];
        let below = &spec.parts[ordinal - 1];
        ops.push(GraphBatchOperation::UpsertEdge {
            src: GraphNodeId::new(CraftSpec::node_id(ordinal, &above.part_id))?,
            edge_type: stacked_on.clone(),
            dst: GraphNodeId::new(CraftSpec::node_id(ordinal - 1, &below.part_id))?,
            data: GraphEdgeData::default_weight(None),
        });
    }

    Ok(GraphBatchWrite::new(ops))
}

pub fn list_craft_node_ids(
    db: &mut Database,
    branch_name: &str,
) -> Result<Vec<String>, stratadb::EngineError> {
    let mut graph = db.graph(branch(branch_name)?, space()?)?;
    let name = GraphName::new(GRAPH_CRAFT)?;
    let page = graph.list_nodes(&name, None, None, 1024)?;
    Ok(page
        .nodes()
        .iter()
        .map(|node| node.node_id().as_str().to_owned())
        .collect())
}

pub fn compiled_planet_doc() -> Value {
    let body = planet_body();
    json!({
        "name": "kerb",
        "version": PLANET_VERSION,
        "hash": content_hash(&body),
        "R": R,
        "g0": G0,
        "mu": MU,
        "r_pe_min": R_PE_MIN,
        "e_max": E_MAX,
    })
}

pub fn compiled_catalog_doc() -> Value {
    let parts = catalog_parts_body();
    json!({
        "version": CATALOG_VERSION,
        "hash": content_hash(&parts),
        "parts": parts,
    })
}

pub fn planet_matches(doc: &Value) -> bool {
    let version = doc.get("version").and_then(Value::as_u64);
    let hash = doc.get("hash").and_then(Value::as_str);
    version == Some(PLANET_VERSION as u64) && hash == Some(content_hash(&planet_body()).as_str())
}

pub fn catalog_matches(doc: &Value) -> bool {
    let version = doc.get("version").and_then(Value::as_u64);
    let hash = doc.get("hash").and_then(Value::as_str);
    version == Some(CATALOG_VERSION as u64)
        && hash == Some(content_hash(&catalog_parts_body()).as_str())
}

fn planet_body() -> Value {
    json!({
        "name": "kerb",
        "R": R,
        "g0": G0,
        "mu": MU,
        "r_pe_min": R_PE_MIN,
        "e_max": E_MAX,
    })
}

fn catalog_parts_body() -> Value {
    Value::Array(
        CATALOG
            .iter()
            .map(|part| {
                json!({
                    "id": part.id,
                    "kind": part.kind.as_str(),
                    "dry_kg": part.dry_kg,
                    "fuel_cap_kg": part.fuel_cap_kg,
                    "thrust_n": part.thrust_n,
                    "fuel_rate_kg_s": part.fuel_rate_kg_s,
                    "isp": part.isp,
                })
            })
            .collect(),
    )
}

fn content_hash(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).expect("json value encodes");
    format!("{:016x}", fnv1a64(&bytes))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn micros(elapsed: std::time::Duration) -> u64 {
    u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
}

/// JSON number round-trip can change f64 bits (ryu shortest vs parser).
/// The engine hashes `to_vec(payload)` then stores the payload nested in a
/// record document; decode parses that document and hashes again. One
/// serialize/parse cycle up front makes those two hashes match.
fn event_payload(value: Value) -> Result<EventPayload, stratadb::EngineError> {
    let bytes = serde_json::to_vec(&value).expect("event payload encodes");
    let canonical: Value = serde_json::from_slice(&bytes).expect("event payload JSON roundtrips");
    EventPayload::new(canonical)
}

pub fn kv_key(bytes: &[u8]) -> Result<KvKey, stratadb::EngineError> {
    KvKey::new(bytes)
}

#[derive(Clone, Debug)]
pub struct VabMeta {
    pub next_launch: u64,
    pub next_design: u64,
}

impl Default for VabMeta {
    fn default() -> Self {
        Self {
            next_launch: 1,
            next_design: 1,
        }
    }
}

pub fn read_vab_meta(db: &mut Database) -> Result<VabMeta, stratadb::EngineError> {
    match read_json_doc(db, BRANCH_VAB, DOC_META)? {
        Some(value) => Ok(VabMeta {
            next_launch: value
                .get("next_launch")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .max(1),
            next_design: value
                .get("next_design")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .max(1),
        }),
        None => Ok(VabMeta::default()),
    }
}

pub fn write_vab_meta_doc(db: &mut Database, meta: &VabMeta) -> Result<(), stratadb::EngineError> {
    write_json_doc(
        db,
        BRANCH_VAB,
        DOC_META,
        json!({
            "next_launch": meta.next_launch,
            "next_design": meta.next_design,
        }),
    )
}

pub fn write_launch_meta(
    db: &mut Database,
    launch: &str,
    parent: &str,
    design: &str,
    fork_seq: u64,
    sample: &VesselSample,
) -> Result<(), stratadb::EngineError> {
    write_json_doc(
        db,
        launch,
        DOC_META,
        json!({
            "name": launch,
            "parent": parent,
            "design": design,
            // The version the branch was cut at. Zero for a launch that came
            // off the pad rather than off another launch.
            "fork_seq": fork_seq,
            "status": sample.status,
            "warp": sample.warp,
            "autopilot": sample.autopilot,
            "phase": sample.phase,
        }),
    )
}

pub fn read_launch_meta(db: &mut Database, launch: &str) -> Result<Value, stratadb::EngineError> {
    Ok(read_json_doc(db, launch, DOC_META)?.unwrap_or_else(|| json!({})))
}

pub fn fork_current(
    db: &mut Database,
    source: &str,
    child: &str,
) -> Result<(), stratadb::EngineError> {
    match db
        .branches()?
        .fork_current(&branch(source)?, branch(child)?)
    {
        Ok(_) => Ok(()),
        Err(error) if error.code() == "already_exists.engine.branch" => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn fork_at_version(
    db: &mut Database,
    source: &str,
    child: &str,
    record: &stratadb::event::EventVersionedRecord,
) -> Result<(), stratadb::EngineError> {
    db.branches()?
        .fork_at_version(&branch(source)?, branch(child)?, record.version())?;
    Ok(())
}

pub fn get_event(
    db: &mut Database,
    branch_name: &str,
    seq: u64,
) -> Result<Option<stratadb::event::EventVersionedRecord>, stratadb::EngineError> {
    db.event(branch(branch_name)?, space()?)?
        .get(EventSequence::new(seq))
}

pub fn list_launch_branches(db: &mut Database) -> Result<Vec<String>, stratadb::EngineError> {
    let mut names: Vec<String> = list_product_branches(db)?
        .into_iter()
        .filter(|name| name.starts_with("launch-"))
        .collect();
    names.sort();
    Ok(names)
}

pub fn delete_branch(db: &mut Database, name: &str) -> Result<(), stratadb::EngineError> {
    db.branches()?.delete(&branch(name)?)?;
    Ok(())
}

pub fn launch_design(
    db: &mut Database,
    launch: &str,
) -> Result<Option<String>, stratadb::EngineError> {
    Ok(read_json_doc(db, launch, DOC_META)?.and_then(|meta| {
        meta.get("design")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }))
}

/// Live `launch-*` branches whose `meta.design` names this design.
pub fn design_refcount(db: &mut Database, design: &str) -> Result<usize, stratadb::EngineError> {
    let mut count = 0usize;
    for launch in list_launch_branches(db)? {
        if launch_design(db, &launch)?.as_deref() == Some(design) {
            count += 1;
        }
    }
    Ok(count)
}

/// Delete a launch. Delete its `design-*` only when no remaining launch lists it
/// and `keep_snapshot` is false. Promote never calls this.
pub fn archive_launch(
    db: &mut Database,
    launch: &str,
    keep_snapshot: bool,
) -> Result<ArchiveView, stratadb::EngineError> {
    let design = launch_design(db, launch)?;
    delete_branch(db, launch)?;
    let mut design_deleted = false;
    let remaining = if let Some(ref design) = design {
        let remaining = design_refcount(db, design)?;
        if !keep_snapshot && remaining == 0 {
            delete_branch(db, design)?;
            design_deleted = true;
        }
        remaining
    } else {
        0
    };
    Ok(ArchiveView {
        ok: true,
        launch: launch.to_owned(),
        design,
        design_deleted,
        keep_snapshot,
        remaining_refcount: remaining,
    })
}

pub fn event_len(db: &mut Database, branch_name: &str) -> Result<u64, stratadb::EngineError> {
    Ok(db.event(branch(branch_name)?, space()?)?.len()?.count())
}

pub fn verify_chain(db: &mut Database, branch_name: &str) -> Result<bool, stratadb::EngineError> {
    Ok(db
        .event(branch(branch_name)?, space()?)?
        .verify_chain()?
        .is_valid())
}

pub fn range_events(
    db: &mut Database,
    branch_name: &str,
) -> Result<Vec<stratadb::event::EventVersionedRecord>, stratadb::EngineError> {
    let page = db.event(branch(branch_name)?, space()?)?.range(
        EventSequence::new(0),
        None,
        None,
        EventRangeDirection::Forward,
        None,
    )?;
    Ok(page.events().to_vec())
}

/// Event then KV. Returns the assigned sequence. Skips tick appends past the soft cap.
pub fn persist_tick(
    db: &mut Database,
    launch: &str,
    sample: &VesselSample,
) -> Result<(u64, PersistStats), stratadb::EngineError> {
    persist_event_then_kv(
        db,
        launch,
        EVENT_TICK,
        telemetry::event_object(EVENT_TICK, sample),
        sample,
        &[],
        None,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn persist_discrete(
    db: &mut Database,
    launch: &str,
    event_type: &str,
    payload: Value,
    sample: &VesselSample,
    graph_delete: &[String],
    spec: Option<&CraftSpec>,
    rebuild_graph: bool,
) -> Result<(u64, PersistStats), stratadb::EngineError> {
    persist_event_then_kv(
        db,
        launch,
        event_type,
        payload,
        sample,
        graph_delete,
        spec,
        rebuild_graph,
    )
}

#[allow(clippy::too_many_arguments)]
fn persist_event_then_kv(
    db: &mut Database,
    launch: &str,
    event_type: &str,
    payload: Value,
    sample: &VesselSample,
    graph_delete: &[String],
    spec: Option<&CraftSpec>,
    rebuild_graph: bool,
) -> Result<(u64, PersistStats), stratadb::EngineError> {
    let mut stats = PersistStats::default();
    if event_type == EVENT_TICK {
        let len = event_len(db, launch)?;
        if len >= EVENT_SOFT_CAP {
            return Ok((sample.last_event_seq, stats));
        }
    }

    let is_edit = event_type == telemetry::EVENT_EDIT;
    if is_edit {
        if let Some(spec) = spec {
            write_spec_and_graph(db, launch, spec, &mut stats)?;
        }
    }

    let started = Instant::now();
    let seq = {
        let mut events = db.event(branch(launch)?, space()?)?;
        let outcome = events.append(EventType::new(event_type)?, event_payload(payload)?)?;
        stats.commits += 1;
        outcome.sequence().as_u64()
    };
    stats.event_us += micros(started.elapsed());

    if !is_edit {
        if let Some(spec) = spec {
            write_spec_and_graph(db, launch, spec, &mut stats)?;
        } else if rebuild_graph {
            if let Some(live) = read_craft(db, launch).ok().flatten() {
                let started = Instant::now();
                rebuild_craft_graph(db, launch, &live, &mut stats)?;
                stats.graph_us += micros(started.elapsed());
            }
        } else if !graph_delete.is_empty() {
            let started = Instant::now();
            delete_craft_nodes(db, launch, graph_delete, &mut stats)?;
            stats.graph_us += micros(started.elapsed());
        }
    }

    let started = Instant::now();
    {
        let mut snap = sample.clone();
        snap.last_event_seq = seq;
        let bytes = serde_json::to_vec(&snap).expect("vessel snapshot encodes");
        let mut kv = db.kv(branch(launch)?, space()?)?;
        kv.put(kv_key(KV_VESSEL)?, KvValue::new(bytes))?;
        stats.commits += 1;
    }
    stats.kv_us += micros(started.elapsed());
    Ok((seq, stats))
}

fn write_spec_and_graph(
    db: &mut Database,
    launch: &str,
    spec: &CraftSpec,
    stats: &mut PersistStats,
) -> Result<(), stratadb::EngineError> {
    let started = Instant::now();
    write_json_doc(db, launch, DOC_CRAFT, spec.to_doc())?;
    stats.commits += 1;
    stats.json_us += micros(started.elapsed());
    let started = Instant::now();
    rebuild_craft_graph(db, launch, spec, stats)?;
    stats.graph_us += micros(started.elapsed());
    Ok(())
}

pub fn rebuild_craft_graph(
    db: &mut Database,
    branch_name: &str,
    spec: &CraftSpec,
    stats: &mut PersistStats,
) -> Result<(), stratadb::EngineError> {
    let mut graph = db.graph(branch(branch_name)?, space()?)?;
    let name = GraphName::new(GRAPH_CRAFT)?;
    match graph.create_graph(name.clone()) {
        Ok(_) => stats.commits += 1,
        Err(error) if error.code() == "already_exists.engine.graph" => {}
        Err(error) => return Err(error),
    }
    let batch = craft_rebuild_batch(&mut graph, &name, spec)?;
    if !batch.is_empty() {
        graph.batch_write(&name, &batch)?;
        stats.commits += 1;
    }
    Ok(())
}

fn delete_craft_nodes(
    db: &mut Database,
    branch_name: &str,
    ids: &[String],
    stats: &mut PersistStats,
) -> Result<(), stratadb::EngineError> {
    let mut graph = db.graph(branch(branch_name)?, space()?)?;
    let name = GraphName::new(GRAPH_CRAFT)?;
    match graph.create_graph(name.clone()) {
        Ok(_) => stats.commits += 1,
        Err(error) if error.code() == "already_exists.engine.graph" => {}
        Err(error) => return Err(error),
    }
    let ops: Result<Vec<_>, _> = ids
        .iter()
        .map(|id| {
            Ok(GraphBatchOperation::DeleteNode {
                node_id: GraphNodeId::new(id.as_str())?,
            })
        })
        .collect();
    let batch = GraphBatchWrite::new(ops?);
    if !batch.is_empty() {
        graph.batch_write(&name, &batch)?;
        stats.commits += 1;
    }
    Ok(())
}

pub fn get_vessel(
    db: &mut Database,
    launch: &str,
) -> Result<Option<VesselSample>, stratadb::EngineError> {
    let packed = {
        let mut kv = db.kv(branch(launch)?, space()?)?;
        kv.get(&kv_key(KV_VESSEL)?)?
    };
    let Some(value) = packed else {
        return Ok(None);
    };
    match serde_json::from_slice(value.as_bytes()) {
        Ok(sample) => Ok(Some(sample)),
        Err(_) => Ok(None), // KV is a cache; a bad blob is ignored and events replay.
    }
}

pub struct ResumeLaunch {
    pub sample: VesselSample,
    pub spec: CraftSpec,
    pub graph_ids: Vec<String>,
    pub trail: Vec<crate::snapshot::TrailSample>,
    pub fork_seq: u64,
}

/// Replay the tape (events are truth). KV is a hint when `last_event_seq`
/// matches the active head and that head is not a rewind.
pub fn reconstruct_launch(
    db: &mut Database,
    launch: &str,
) -> Result<Option<ResumeLaunch>, EnsureError> {
    reconstruct_launch_at(db, launch, None)
}

pub fn reconstruct_launch_at(
    db: &mut Database,
    launch: &str,
    end_seq: Option<u64>,
) -> Result<Option<ResumeLaunch>, EnsureError> {
    let events = range_events(db, launch)?;
    let active = active_events(&events, end_seq);
    if active
        .iter()
        .all(|event| event.event_type().as_str() == EVENT_VAB_SAVE)
    {
        return Ok(None);
    }

    let launch_ev = active
        .iter()
        .find(|event| event.event_type().as_str() == telemetry::EVENT_LAUNCH);
    let spec0 = if let Some(launch_ev) = launch_ev {
        read_craft_at_version(db, launch, launch_ev)?.ok_or_else(|| {
            EnsureError::Craft(format!("craft missing at launch commit on {launch}"))
        })?
    } else {
        read_craft(db, launch)?
            .ok_or_else(|| EnsureError::Craft(format!("craft missing on {launch}")))?
    };
    let mut spec = spec0.clone();
    let mut graph_ids: Vec<String> = spec
        .parts
        .iter()
        .enumerate()
        .map(|(i, p)| CraftSpec::node_id(i, &p.part_id))
        .collect();

    let pad = crate::physics::Vessel::at_pad(spec.wet_mass(), spec.fuel());
    let mut sample = VesselSample::from_ship(
        launch,
        "vab",
        "design-0001",
        &pad,
        0,
        true,
        crate::ascent::AscentPhase::Boost,
        1,
    );
    let mut trail = Vec::new();

    let meta = read_launch_meta(db, launch)?;
    let fork_seq = meta.get("fork_seq").and_then(Value::as_u64).unwrap_or(0);
    if let Some(parent) = meta.get("parent").and_then(Value::as_str) {
        sample.parent = parent.to_owned();
    }
    if let Some(design) = meta.get("design").and_then(Value::as_str) {
        sample.design = design.to_owned();
    }
    if let Some(autopilot) = meta.get("autopilot").and_then(Value::as_bool) {
        sample.autopilot = autopilot;
    }
    if let Some(warp) = meta.get("warp").and_then(Value::as_u64) {
        sample.warp = warp as u32;
    }
    if let Some(phase) = meta.get("phase").and_then(Value::as_str) {
        sample.phase = match phase {
            "coast" => crate::ascent::AscentPhase::Coast,
            "circ" => crate::ascent::AscentPhase::Circ,
            _ => crate::ascent::AscentPhase::Boost,
        };
    }

    let mut head_is_rewind = false;
    for event in &active {
        let kind = event.event_type().as_str();
        if kind == EVENT_VAB_SAVE {
            continue;
        }
        let payload = event.payload().as_inner();
        sample.last_event_seq = event.sequence().as_u64();
        head_is_rewind = kind == telemetry::EVENT_REWIND;
        match kind {
            telemetry::EVENT_LAUNCH
            | telemetry::EVENT_TICK
            | telemetry::EVENT_CONTROL
            | telemetry::EVENT_FORK => {
                sample = telemetry::sample_from_payload(payload, &sample);
                sample.last_event_seq = event.sequence().as_u64();
                trail.push(sample.trail_sample());
            }
            telemetry::EVENT_STAGE => {
                sample = telemetry::sample_from_payload(payload, &sample);
                sample.last_event_seq = event.sequence().as_u64();
                if !spec.parts.is_empty() {
                    let drop = spec.stage();
                    let n = drop.ids.len().min(graph_ids.len());
                    graph_ids.drain(0..n);
                }
                let extra = (spec.fuel() - sample.fuel).max(0.0);
                if extra > 0.0 {
                    spec.drain_fuel(extra);
                }
                trail.push(sample.trail_sample());
            }
            telemetry::EVENT_EDIT => {
                sample = telemetry::sample_from_payload(payload, &sample);
                sample.last_event_seq = event.sequence().as_u64();
                apply_edit(&mut spec, payload);
                graph_ids = spec
                    .parts
                    .iter()
                    .enumerate()
                    .map(|(i, p)| CraftSpec::node_id(i, &p.part_id))
                    .collect();
                trail.push(sample.trail_sample());
            }
            telemetry::EVENT_REWIND => {
                sample = telemetry::sample_from_payload(payload, &sample);
                sample.last_event_seq = event.sequence().as_u64();
            }
            telemetry::EVENT_ORBIT
            | telemetry::EVENT_CRASH
            | telemetry::EVENT_FLAMEOUT
            | telemetry::EVENT_ESCAPED => {
                sample = telemetry::sample_from_payload(payload, &sample);
                sample.last_event_seq = event.sequence().as_u64();
                if kind == telemetry::EVENT_ORBIT {
                    sample.status = crate::physics::FlightStatus::Orbit;
                } else if kind == telemetry::EVENT_CRASH {
                    sample.status = crate::physics::FlightStatus::Crashed;
                } else if kind == telemetry::EVENT_ESCAPED {
                    sample.status = crate::physics::FlightStatus::Escaped;
                }
                trail.push(sample.trail_sample());
            }
            _ => {}
        }
    }

    let active_seqs: HashSet<u64> = active
        .iter()
        .map(|event| event.sequence().as_u64())
        .collect();
    if !head_is_rewind {
        if let Some(snap) = get_vessel(db, launch)? {
            if active_seqs.contains(&snap.last_event_seq)
                && snap.last_event_seq == sample.last_event_seq
            {
                sample = snap;
            }
        }
    }

    if trail.len() > telemetry::TAPE_CAP {
        trail = trail.split_off(trail.len() - telemetry::TAPE_CAP);
    }

    Ok(Some(ResumeLaunch {
        sample,
        spec,
        graph_ids,
        trail,
        fork_seq,
    }))
}

fn active_events(
    events: &[stratadb::event::EventVersionedRecord],
    end_seq: Option<u64>,
) -> Vec<&stratadb::event::EventVersionedRecord> {
    let mut active: Vec<&stratadb::event::EventVersionedRecord> = Vec::new();
    for event in events {
        let seq = event.sequence().as_u64();
        if end_seq.is_some_and(|end| seq > end) {
            break;
        }
        if event.event_type().as_str() == telemetry::EVENT_REWIND {
            let to_seq = event
                .payload()
                .as_inner()
                .get("to_seq")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            active.retain(|kept| kept.sequence().as_u64() <= to_seq);
            continue;
        }
        active.push(event);
    }
    active
}

fn apply_edit(spec: &mut CraftSpec, payload: &Value) {
    let op = payload.get("op").and_then(Value::as_str).unwrap_or("");
    if op != "add_tank" && op != "add" {
        return;
    }
    let Some(part) = payload.get("part").and_then(Value::as_str) else {
        return;
    };
    let index = payload
        .get("index")
        .and_then(Value::as_u64)
        .map(|n| n as usize);
    let _ = spec.add(part, index);
}

fn read_craft_at_version(
    db: &mut Database,
    branch_name: &str,
    launch_ev: &stratadb::event::EventVersionedRecord,
) -> Result<Option<CraftSpec>, EnsureError> {
    let mut json = db.json(branch(branch_name)?, space()?)?;
    match json.get_at_version(
        &JsonDocumentId::new(DOC_CRAFT)?,
        &JsonPath::root(),
        launch_ev.version(),
    )? {
        Some(value) => CraftSpec::from_doc(value.as_inner())
            .map(Some)
            .map_err(EnsureError::Craft),
        None => Ok(None),
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ArchiveView {
    pub ok: bool,
    pub launch: String,
    pub design: Option<String>,
    pub design_deleted: bool,
    pub keep_snapshot: bool,
    pub remaining_refcount: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct CompareView {
    pub empty: bool,
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    pub capabilities: Vec<String>,
    pub json_entities: usize,
    pub kv_entities: usize,
    pub event_entities: usize,
    pub graph_entities: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct PromoteView {
    pub ok: bool,
    pub strategy: String,
    pub design: String,
    pub source_launch: String,
    pub applied: Vec<String>,
    pub unsupported: Vec<String>,
    pub note: String,
}

pub fn parse_strategy(name: &str) -> Result<PromotionStrategy, String> {
    match name {
        "strict" => Ok(PromotionStrategy::Strict),
        "source_wins" | "sourcewins" => Ok(PromotionStrategy::SourceWins),
        other => Err(format!(
            "invalid_argument.ksp.strategy: unknown `{other}` (strict|source_wins)"
        )),
    }
}

pub fn strategy_name(strategy: PromotionStrategy) -> &'static str {
    match strategy {
        PromotionStrategy::Strict => "strict",
        PromotionStrategy::SourceWins => "source_wins",
    }
}

pub fn refuse_launch_onto_vab(source: &str, target: &str) -> Result<(), String> {
    if source.starts_with("launch-") && target == BRANCH_VAB {
        return Err(
            "failed_precondition.ksp.promote: never promote a launch onto vab (would carry flight meta and vessel KV; graph/event would not move)"
                .into(),
        );
    }
    Ok(())
}

pub fn write_design_craft(
    db: &mut Database,
    design: &str,
    spec: &CraftSpec,
) -> Result<(), stratadb::EngineError> {
    write_json_doc(db, design, DOC_CRAFT, spec.to_doc())
}

pub fn promote_design(
    db: &mut Database,
    design: &str,
    spec: &CraftSpec,
    source_launch: &str,
    strategy: PromotionStrategy,
) -> Result<PromoteView, stratadb::EngineError> {
    let outcome = db
        .branches()?
        .promote(&branch(design)?, &branch(BRANCH_VAB)?, strategy)?;
    let unsupported = unique_cap_labels(outcome.capabilities_unsupported());
    let applied: Vec<String> = outcome.applied().iter().map(format_promoted).collect();
    let view = PromoteView {
        ok: true,
        strategy: strategy_name(strategy).to_owned(),
        design: design.to_owned(),
        source_launch: source_launch.to_owned(),
        applied,
        unsupported: unsupported.clone(),
        note: "Strata promoted the JSON spec; the hangar graph was rebuilt from it.".into(),
    };
    let mut stats = PersistStats::default();
    rebuild_craft_graph(db, BRANCH_VAB, spec, &mut stats)?;
    let payload = json!({
        "t": 0.0,
        "kind": EVENT_PROMOTED,
        "source_launch": source_launch,
        "design": design,
        "strategy": strategy_name(strategy),
        "ok": true,
        "unsupported": unsupported,
    });
    {
        let mut events = db.event(branch(BRANCH_VAB)?, space()?)?;
        events.append(EventType::new(EVENT_PROMOTED)?, event_payload(payload)?)?;
    }
    Ok(view)
}

pub fn promote_named(
    db: &mut Database,
    source: &str,
    target: &str,
    strategy: PromotionStrategy,
) -> Result<PromotionOutcome, stratadb::EngineError> {
    db.branches()?
        .promote(&branch(source)?, &branch(target)?, strategy)
}

pub fn compare_branches(
    db: &mut Database,
    a: &str,
    b: &str,
) -> Result<CompareView, stratadb::EngineError> {
    let comparison =
        db.branches()?
            .compare(&branch(a)?, &branch(b)?, BranchStateSelector::Current)?;
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut modified = 0usize;
    let mut capabilities = Vec::new();
    let mut json_entities = 0usize;
    let mut kv_entities = 0usize;
    let mut event_entities = 0usize;
    let mut graph_entities = 0usize;
    for space_cmp in comparison.comparisons() {
        let n = space_cmp.added().len() + space_cmp.removed().len() + space_cmp.modified().len();
        added += space_cmp.added().len();
        removed += space_cmp.removed().len();
        modified += space_cmp.modified().len();
        capabilities.push(format!("{:?}", space_cmp.capability()));
        match space_cmp.capability() {
            ComparedCapability::Json => json_entities += n,
            ComparedCapability::Kv => kv_entities += n,
            ComparedCapability::Event => event_entities += n,
            ComparedCapability::GraphNode
            | ComparedCapability::GraphEdge
            | ComparedCapability::GraphMetadata
            | ComparedCapability::GraphOntology => graph_entities += n,
            ComparedCapability::Vector | ComparedCapability::VectorCollection => {}
        }
    }
    Ok(CompareView {
        empty: comparison.is_empty(),
        added,
        removed,
        modified,
        capabilities,
        json_entities,
        kv_entities,
        event_entities,
        graph_entities,
    })
}

fn cap_label(cap: ComparedCapability) -> &'static str {
    match cap {
        ComparedCapability::Kv => "kv",
        ComparedCapability::Json => "json",
        ComparedCapability::Event => "event",
        ComparedCapability::GraphNode
        | ComparedCapability::GraphEdge
        | ComparedCapability::GraphMetadata
        | ComparedCapability::GraphOntology => "graph",
        ComparedCapability::Vector | ComparedCapability::VectorCollection => "vector",
    }
}

fn unique_cap_labels(caps: &[ComparedCapability]) -> Vec<String> {
    let mut out = Vec::new();
    for cap in caps {
        let label = cap_label(*cap).to_owned();
        if !out.contains(&label) {
            out.push(label);
        }
    }
    out
}

fn format_promoted(entity: &stratadb::branch::PromotedEntity) -> String {
    let cap = cap_label(entity.capability());
    let id = String::from_utf8_lossy(entity.identity());
    let printable: String = id.chars().filter(|c| c.is_ascii_graphic()).collect();
    if printable.contains("craft") {
        format!("{cap}:craft")
    } else if printable.contains("meta") {
        format!("{cap}:meta")
    } else if !printable.is_empty() {
        format!("{cap}:{printable}")
    } else {
        cap.to_owned()
    }
}
