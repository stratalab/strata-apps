//! Strata persistence for one colony and for the twenty-branch world.

use crate::findings::{Finding, Kind, Log};
use crate::life::Board;
use crate::patterns::{colony_name, COLONY_COUNT, SEED_BRANCH, SPACE};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;
use stratadb::branch::{BranchStateSelector, ComparedCapability};
use stratadb::event::{EventPayload, EventRangeDirection, EventSequence, EventType};
use stratadb::graph::{
    GraphEdgeData, GraphEdgeType, GraphName, GraphNodeData, GraphNodeId, GraphProperties,
};
use stratadb::json::{JsonDocumentId, JsonPath, JsonValue};
use stratadb::kv::{KvKey, KvValue};
use stratadb::{BranchName, CacheOpenOptions, Database, DurableLocalOpenOptions, ProductSpace};

pub const BOARD_KEY: &[u8] = b"board";
pub const DOC_ID: &str = "colony";
pub const TICK_TYPE: &str = "tick";
pub const GRAPH_NAME: &str = "lineage";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistMode {
    /// One KV blob for the whole board (fast path, still three commits/tick).
    Blob,
    /// Live cells as individual KV keys plus the blob (write-amplifying stress).
    Cells,
}

#[derive(Clone, Debug, Default)]
pub struct PersistStats {
    pub commits: u64,
    pub kv_us: u64,
    pub json_us: u64,
    pub event_us: u64,
}

impl PersistStats {
    pub fn total_us(&self) -> u64 {
        self.kv_us + self.json_us + self.event_us
    }
}

pub fn kv_key(bytes: &[u8]) -> Result<KvKey, stratadb::EngineError> {
    KvKey::new(bytes)
}

pub fn branch(name: &str) -> Result<BranchName, stratadb::EngineError> {
    BranchName::new(name)
}

pub fn space() -> Result<ProductSpace, stratadb::EngineError> {
    ProductSpace::new(SPACE)
}

pub fn open_cache() -> Result<Database, stratadb::EngineError> {
    Ok(Database::open_cache(CacheOpenOptions::new())?.into_database())
}

pub fn open_local(path: &Path) -> Result<Database, stratadb::EngineError> {
    Ok(Database::open_local(path, DurableLocalOpenOptions::new())?.into_database())
}

pub fn write_board(
    db: &mut Database,
    branch_name: &str,
    board: &Board,
    prev_live: Option<&HashSet<(u32, u32)>>,
    mode: PersistMode,
    stats: &mut PersistStats,
    log: &Log,
) -> Result<(), stratadb::EngineError> {
    let started = Instant::now();
    {
        let mut kv = db.kv(branch(branch_name)?, space()?)?;
        kv.put(kv_key(BOARD_KEY)?, KvValue::new(board.packed_vec()))?;
        stats.commits += 1;

        if mode == PersistMode::Cells {
            let next: HashSet<Vec<u8>> = board
                .live_cells()
                .into_iter()
                .map(|(x, y)| format!("c:{x}:{y}").into_bytes())
                .collect();
            let prev: HashSet<Vec<u8>> = match prev_live {
                Some(cells) => cells
                    .iter()
                    .map(|(x, y)| format!("c:{x}:{y}").into_bytes())
                    .collect(),
                None => kv
                    .list(Some(&kv_key(b"c:")?))?
                    .into_iter()
                    .map(|key| key.as_bytes().to_vec())
                    .collect(),
            };
            let births: Vec<(KvKey, KvValue)> = next
                .difference(&prev)
                .map(|key| Ok((kv_key(key)?, KvValue::new(b"1".to_vec()))))
                .collect::<Result<_, stratadb::EngineError>>()?;
            let deaths: Vec<KvKey> = prev
                .difference(&next)
                .map(|key| kv_key(key))
                .collect::<Result<_, _>>()?;
            if !births.is_empty() {
                kv.put_batch(births)?;
                stats.commits += 1;
            }
            if !deaths.is_empty() {
                kv.delete_batch(deaths)?;
                stats.commits += 1;
            }
        }
    }
    stats.kv_us += u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);

    if mode == PersistMode::Cells {
        log.push(Finding::new(
            Kind::Note,
            "engine.kv",
            "Sparse-cell mode issues a second KV commit for diffs",
            "Births and deaths cannot join the board-blob put: put_batch and delete_batch are separate methods, each their own commit. A mixed put+delete batch is not on the KV service.",
        ));
    }
    Ok(())
}

pub fn write_status(
    db: &mut Database,
    branch_name: &str,
    status: Value,
    stats: &mut PersistStats,
) -> Result<(), stratadb::EngineError> {
    let started = Instant::now();
    {
        let mut json = db.json(branch(branch_name)?, space()?)?;
        json.set_or_create(
            JsonDocumentId::new(DOC_ID)?,
            &JsonPath::root(),
            JsonValue::new(status)?,
        )?;
        stats.commits += 1;
    }
    stats.json_us += u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    Ok(())
}

pub fn append_tick(
    db: &mut Database,
    branch_name: &str,
    payload: Value,
    stats: &mut PersistStats,
) -> Result<(), stratadb::EngineError> {
    let started = Instant::now();
    {
        let mut events = db.event(branch(branch_name)?, space()?)?;
        events.append(EventType::new(TICK_TYPE)?, EventPayload::new(payload)?)?;
        stats.commits += 1;
    }
    stats.event_us += u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    Ok(())
}

pub fn read_board(
    db: &mut Database,
    branch_name: &str,
) -> Result<Option<Board>, stratadb::EngineError> {
    let packed = {
        let mut kv = db.kv(branch(branch_name)?, space()?)?;
        kv.get(&kv_key(BOARD_KEY)?)?
    };
    let Some(value) = packed else {
        return Ok(None);
    };
    let status = read_status(db, branch_name)?;
    let width = status.get("width").and_then(Value::as_u64).unwrap_or(64) as u32;
    let height = status.get("height").and_then(Value::as_u64).unwrap_or(48) as u32;
    Ok(Board::from_packed(width, height, value.into_bytes()))
}

pub fn read_status(db: &mut Database, branch_name: &str) -> Result<Value, stratadb::EngineError> {
    let mut json = db.json(branch(branch_name)?, space()?)?;
    match json.get(&JsonDocumentId::new(DOC_ID)?, &JsonPath::root())? {
        Some(value) => Ok(value.into_inner()),
        None => Ok(json!({})),
    }
}

pub fn list_product_branches(db: &mut Database) -> Result<Vec<String>, stratadb::EngineError> {
    Ok(db
        .branches()?
        .list()?
        .into_iter()
        .map(|summary| summary.name().as_str().to_owned())
        .collect())
}

pub fn fork_from_seed(db: &mut Database, name: &str) -> Result<(), stratadb::EngineError> {
    db.branches()?
        .fork_current(&branch(SEED_BRANCH)?, branch(name)?)?;
    Ok(())
}

pub fn delete_branch(db: &mut Database, name: &str) -> Result<(), stratadb::EngineError> {
    db.branches()?.delete(&branch(name)?)?;
    Ok(())
}

pub fn compare_to_control(
    db: &mut Database,
    name: &str,
) -> Result<CompareView, stratadb::EngineError> {
    let comparison = db.branches()?.compare(
        &branch("control")?,
        &branch(name)?,
        BranchStateSelector::Current,
    )?;
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut modified = 0usize;
    let mut capabilities = Vec::new();
    for space_cmp in comparison.comparisons() {
        added += space_cmp.added().len();
        removed += space_cmp.removed().len();
        modified += space_cmp.modified().len();
        capabilities.push(format!("{:?}", space_cmp.capability()));
    }
    Ok(CompareView {
        empty: comparison.is_empty(),
        added,
        removed,
        modified,
        capabilities,
        kv_entities: comparison
            .comparisons()
            .iter()
            .filter(|c| c.capability() == ComparedCapability::Kv)
            .map(|c| c.added().len() + c.removed().len() + c.modified().len())
            .sum(),
    })
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct CompareView {
    pub empty: bool,
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    pub capabilities: Vec<String>,
    pub kv_entities: usize,
}

pub fn verify_chain(db: &mut Database, name: &str) -> Result<bool, stratadb::EngineError> {
    let outcome = db.event(branch(name)?, space()?)?.verify_chain()?;
    Ok(outcome.is_valid())
}

pub fn tick_at_generation(
    db: &mut Database,
    branch_name: &str,
    generation: u64,
) -> Result<Option<Value>, stratadb::EngineError> {
    let mut events = db.event(branch(branch_name)?, space()?)?;
    let mut end = None;
    loop {
        // Prefer the latest edit at this generation, including branch-local
        // mutations after the inherited genesis event.
        let page = events.range(
            EventSequence::new(0),
            end,
            Some(4096),
            EventRangeDirection::Reverse,
            Some(&EventType::new(TICK_TYPE)?),
        )?;
        for record in page.events() {
            let payload = record.payload().as_inner();
            if payload.get("generation").and_then(Value::as_u64) == Some(generation) {
                return Ok(Some(payload.clone()));
            }
        }
        if !page.has_more() {
            return Ok(None);
        }
        end = page.cursor();
    }
}

pub fn write_lineage_graph(db: &mut Database, log: &Log) -> Result<(), stratadb::EngineError> {
    let mut graph = db.graph(branch(SEED_BRANCH)?, space()?)?;
    let name = GraphName::new(GRAPH_NAME)?;
    match graph.create_graph(name.clone()) {
        Ok(_) => {}
        Err(error) if error.code() == "already_exists.engine.graph" => return Ok(()),
        Err(error) => {
            log.from_engine(
                Kind::Friction,
                "engine.graph",
                "Lineage graph create failed",
                &error,
            );
            return Err(error);
        }
    }

    let seed_props = GraphProperties::new(json!({"kind": "seed"}))?;
    graph.upsert_node(
        &name,
        GraphNodeId::new(SEED_BRANCH)?,
        GraphNodeData::new(Some(seed_props), None),
    )?;

    for index in 0..COLONY_COUNT {
        let colony = colony_name(index);
        let props = GraphProperties::new(json!({"kind": "colony", "index": index}))?;
        graph.upsert_node(
            &name,
            GraphNodeId::new(colony.as_str())?,
            GraphNodeData::new(Some(props), None),
        )?;
        graph.upsert_edge(
            &name,
            GraphNodeId::new(SEED_BRANCH)?,
            GraphEdgeType::new("forked")?,
            GraphNodeId::new(colony.as_str())?,
            GraphEdgeData::default_weight(None),
        )?;
        if index > 0 {
            graph.upsert_edge(
                &name,
                GraphNodeId::new("control")?,
                GraphEdgeType::new("lied")?,
                GraphNodeId::new(colony.as_str())?,
                GraphEdgeData::default_weight(None),
            )?;
        }
    }
    Ok(())
}

pub fn colony_status(
    name: &str,
    index: usize,
    board: &Board,
    generation: u64,
    perturbed: Option<(u32, u32)>,
    divergence: u32,
) -> Value {
    json!({
        "name": name,
        "index": index,
        "generation": generation,
        "live": board.live_count(),
        "width": board.width(),
        "height": board.height(),
        "perturbed": perturbed.map(|(x, y)| json!([x, y])),
        "fingerprint": format!("{:016x}", board.fingerprint()),
        "divergence": divergence,
    })
}

pub fn tick_payload(generation: u64, board: &Board, kind: &str) -> Value {
    json!({
        "generation": generation,
        "kind": kind,
        "live": board.live_count(),
        "fingerprint": format!("{:016x}", board.fingerprint()),
        "board": B64.encode(board.packed()),
        "width": board.width(),
        "height": board.height(),
    })
}

pub fn board_from_tick(payload: &Value) -> Option<Board> {
    let width = payload.get("width")?.as_u64()? as u32;
    let height = payload.get("height")?.as_u64()? as u32;
    let encoded = payload.get("board")?.as_str()?;
    let packed = B64.decode(encoded).ok()?;
    Board::from_packed(width, height, packed)
}

pub fn expected_colony_names() -> Vec<String> {
    (0..COLONY_COUNT).map(colony_name).collect()
}
