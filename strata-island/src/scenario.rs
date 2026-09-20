//! Replayable close/reopen operations. Ready records pin graph + event completion.
use crate::{
    error::IslandError,
    extract::{ClosureFixture, Extract},
    places, store,
};
use serde_json::{json, Value};
use std::time::Instant;
use stratadb::event::{EventPayload, EventRangeDirection, EventSequence, EventType};
use stratadb::graph::{
    GraphBatchOperation, GraphBatchWrite, GraphEdgeData, GraphNodeId, GraphProperties,
};
use stratadb::json::{JsonDocumentId, JsonPath};
use stratadb::{CommitVersion, Database};

pub fn state(db: &Database, desk: &str) -> Result<Option<Value>, IslandError> {
    store::read_json(db, desk, "scenario-state")
}

pub fn create(
    db: &mut Database,
    extract: &Extract,
    closure: &ClosureFixture,
    next: u64,
) -> Result<store::CloseOutcome, IslandError> {
    let started = Instant::now();
    let desk = store::desk_name(next);
    let base = places::current_version(db, "city")?;
    // Parent-side intent survives the narrow fork-before-child-document crash window.
    let intent = json!({"name":closure.name,"closure":closure,"parent_version":base,"operations":[{"id":"initial","closed":true,"status":"pending"}]});
    store::write_json(db, "city", &format!("scenario:{desk}"), intent.clone())?;
    db.branches()?
        .fork_at_version(&store::branch("city")?, store::branch(&desk)?, base)?;
    checkpoint("scenario-fork");
    store::write_json(db, &desk, "scenario-state", intent)?;
    recover(db, &desk, extract)?;
    let index = store::load_drive_index(db, &desk, extract)?;
    let mut meta = store::read_json(db, "city", store::DOC_META)?.unwrap();
    meta["next_desk"] = json!(meta["next_desk"].as_u64().unwrap_or(1).max(next + 1));
    store::write_json(db, "city", store::DOC_META, meta)?;
    Ok(store::CloseOutcome {
        desk,
        index,
        closed_edges: closure.edges.len(),
        persist_ms: started.elapsed().as_millis() as u64,
        next_desk: next + 1,
    })
}

pub fn recover(db: &Database, desk: &str, extract: &Extract) -> Result<Option<usize>, IslandError> {
    let Some(mut state) =
        state(db, desk)?.or(store::read_json(db, "city", &format!("scenario:{desk}"))?)
    else {
        return Ok(None);
    };
    let closure: ClosureFixture = serde_json::from_value(state["closure"].clone())
        .map_err(|_| IslandError::code("failed_precondition.island.scenario"))?;
    let operations = state["operations"]
        .as_array_mut()
        .ok_or(IslandError::code("failed_precondition.island.scenario"))?;
    let last = operations
        .last_mut()
        .ok_or(IslandError::code("failed_precondition.island.scenario"))?;
    let closed = last["closed"]
        .as_bool()
        .ok_or(IslandError::code("failed_precondition.island.scenario"))?;
    let count = if closed { closure.edges.len() } else { 0 };
    if last["status"] == "ready" {
        return Ok(Some(count));
    }
    let id = last["id"]
        .as_str()
        .ok_or(IslandError::code("failed_precondition.island.scenario"))?
        .to_owned();
    let ready_doc = format!("ready:operation:{id}");
    if store::read_json(db, desk, &ready_doc)?.is_none() {
        let mut graph = db.graph(store::branch(desk)?, store::space()?)?;
        let mut batch = Vec::new();
        for e in &closure.edges {
            let src = GraphNodeId::new(&e.src)?;
            let dst = GraphNodeId::new(&e.dst)?;
            let edge_type = store::edge_street()?;
            if closed {
                batch.push(GraphBatchOperation::DeleteEdge {
                    src,
                    edge_type,
                    dst,
                });
            } else {
                let original = extract
                    .edges
                    .iter()
                    .find(|x| x.src == e.src && x.dst == e.dst)
                    .ok_or(IslandError::code(
                        "failed_precondition.island.closure_missing",
                    ))?;
                let mut props = json!({"length_m":original.length_m});
                if let Some(n) = &original.name {
                    props["name"] = json!(n);
                }
                batch.push(GraphBatchOperation::UpsertEdge {
                    src,
                    edge_type,
                    dst,
                    data: GraphEdgeData::new(
                        original.length_m as f64,
                        Some(GraphProperties::new(props)?),
                    )?,
                });
            }
        }
        graph.batch_write(&store::graph_name()?, &GraphBatchWrite::new(batch))?;
        checkpoint("scenario-batch");
        drop(graph);
        let mut events = db.event(store::branch(desk)?, store::space()?)?;
        // Each operation has a unique event type; replay cannot append duplicate completion events.
        let event_type = EventType::new(format!("operation:{id}"))?;
        let page = events.range(
            EventSequence::new(0),
            None,
            Some(1),
            EventRangeDirection::Forward,
            Some(&event_type),
        )?;
        if page.events().is_empty() {
            events.append(
                event_type,
                EventPayload::new(json!({"id":id,"closed":closed,"edges":closure.edges}))?,
            )?;
        }
        checkpoint("scenario-event");
        store::write_json(
            db,
            desk,
            store::DOC_META,
            json!({"name":desk,"parent":"city","status":if closed{"closed"}else{"reopened"},"closed_edges":count,"city_version":1}),
        )?;
        store::write_json(
            db,
            desk,
            &ready_doc,
            json!({"id":id,"closed":closed,"count":count}),
        )?;
    }
    checkpoint("scenario-ready");
    let version = db
        .json(store::branch(desk)?, store::space()?)?
        .get_versioned(&JsonDocumentId::new(&ready_doc)?, &JsonPath::root())?
        .unwrap()
        .version();
    last["version"] = json!(version);
    last["status"] = json!("ready");
    store::write_json(db, desk, "scenario-state", state)?;
    Ok(Some(count))
}

pub fn version(db: &Database, desk: &str) -> Result<Option<CommitVersion>, IslandError> {
    Ok(state(db, desk)?
        .and_then(|s| s["operations"].as_array()?.last()?["version"].as_u64())
        .map(CommitVersion::new))
}

pub fn apply(
    db: &Database,
    desk: &str,
    extract: &Extract,
    id: &str,
    closed: bool,
    expected: u64,
) -> Result<(), IslandError> {
    if id.is_empty() || id.len() > 48 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(IslandError::code("invalid_argument.island.operation"));
    }
    let mut state = state(db, desk)?.ok_or(IslandError::code("not_found.island.scenario"))?;
    let ops = state["operations"].as_array_mut().unwrap();
    if let Some(old) = ops.iter().find(|o| o["id"] == id) {
        if old["closed"] != closed {
            return Err(IslandError::code("failed_precondition.island.operation"));
        }
        recover(db, desk, extract)?;
        return Ok(());
    }
    if ops.len() >= 100 || places::current_version(db, desk)?.as_u64() != expected {
        return Err(IslandError::code("failed_precondition.island.version"));
    }
    ops.push(json!({"id":id,"closed":closed,"status":"pending"}));
    store::write_json(db, desk, "scenario-state", state)?;
    recover(db, desk, extract)?;
    Ok(())
}

/// Debug-build process-termination hook used only by the recovery integration tests.
#[doc(hidden)]
pub fn checkpoint(name: &str) {
    #[cfg(debug_assertions)]
    if std::env::var("ISLAND_TEST_CRASH_AT").ok().as_deref() == Some(name) {
        std::process::exit(77); // Deliberately skip destructors, as an abruptly killed writer would.
    }
    #[cfg(not(debug_assertions))]
    let _ = name;
}
