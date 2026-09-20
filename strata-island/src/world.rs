//! City snapshot in RAM. Gazetteer hydrates from JSON after import.
//!
//! `city_snapshot` and `route` never call store / graph / kv / json / event.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use stratadb::Database;

use crate::drive::{self, DriveIndex};
use crate::error::IslandError;
use crate::extract::{self, ClosureFixture, Extract, GazetteerPoi};
use crate::findings::{self, Finding, Kind, Log};
use crate::geo;
use crate::route;
use crate::snapshot::{
    self, AuditView, BranchView, CloseView, CompareView, DeskAudit, RouteView, CITY_VERSION,
    LIVE_CAP,
};
use crate::store::{self, BRANCH_CITY, BRANCH_DEFAULT};

pub use crate::drive::DriveEdge;

const CITY_JSON: &str = include_str!("../fixtures/manhattan-drive.json");
const GRID_JSON: &str = include_str!("../fixtures/grid-8x8.json");
const GRID_CLOSURE_JSON: &str = include_str!("../fixtures/grid-closure-42nd.json");
const GRID_GAZETTEER_JSON: &str = include_str!("../fixtures/grid-gazetteer.json");
const CLOSURE_JSON: &str = include_str!("../fixtures/closure-42nd.json");
const GAZETTEER_JSON: &str = include_str!("../fixtures/gazetteer.json");

pub struct OpenArgs {
    pub cache: bool,
    pub db_path: PathBuf,
    pub memory_budget_bytes: Option<u64>,
}

pub struct World {
    /// Held for import, mutations, direct graph reads, and historical snapshots.
    /// Current route reconstruction and the camera stay in RAM.
    db: Mutex<Database>,
    mutations: Mutex<()>,
    pub(crate) place_snapshots: Mutex<BTreeMap<String, Arc<crate::places::Snapshot>>>,
    address_catalogs: Mutex<BTreeMap<String, Arc<crate::addresses::Catalog>>>,
    expanded: bool,
    journeys: Option<Arc<crate::journeys::Catalog>>,
    city: Mutex<BTreeMap<String, Arc<DriveIndex>>>,
    gazetteer: Mutex<Vec<GazetteerPoi>>,
    closure: ClosureFixture,
    extract: Extract,
    findings: Arc<Log>,
    durable: bool,
    db_path: String,
    last_persist_ms: AtomicU64,
    last_route: Mutex<Option<RouteView>>,
    last_compare: Mutex<Option<CompareView>>,
    focused: Mutex<String>,
    next_desk: AtomicU64,
    branch_meta: Mutex<BTreeMap<String, BranchView>>,
}

impl World {
    /// RAM-only 8×8 fixture for `tests/geo.rs`. Does not open a database.
    pub fn open_ram_grid() -> Result<RamCity, IslandError> {
        RamCity::load(GRID_JSON, GRID_CLOSURE_JSON, GRID_GAZETTEER_JSON)
    }

    pub fn open(args: OpenArgs) -> Result<Self, IslandError> {
        Self::open_dataset(args, false)
    }

    pub fn open_v2(args: OpenArgs) -> Result<Self, IslandError> {
        Self::open_dataset(args, true)
    }

    fn open_dataset(args: OpenArgs, expanded: bool) -> Result<Self, IslandError> {
        let extract = extract::parse_city(CITY_JSON)?;
        if extract.nodes.len() != extract::EXTRACT_NODE_COUNT
            || extract.edges.len() != extract::EXTRACT_EDGE_COUNT
        {
            return Err(IslandError::code("invalid_argument.island.extract"));
        }
        if extract::fnv1a64(CITY_JSON.as_bytes()) != extract::EXTRACT_FNV1A64 {
            return Err(IslandError::code("invalid_argument.island.extract"));
        }
        let closure = extract::parse_closure(CLOSURE_JSON)?;
        let seed = extract::parse_gazetteer(GAZETTEER_JSON)?;
        for poi in &seed {
            if !extract.nodes.iter().any(|n| n.id == poi.node) {
                return Err(IslandError::code("invalid_argument.island.extract"));
            }
        }
        for edge in &closure.edges {
            let ok = extract
                .edges
                .iter()
                .any(|e| e.src == edge.src && e.dst == edge.dst);
            if !ok {
                return Err(IslandError::code("invalid_argument.island.extract"));
            }
        }

        let preexisting_nonempty = !args.cache
            && args.db_path.exists()
            && std::fs::read_dir(&args.db_path)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(true);
        let mut db = if args.cache {
            store::open_cache(args.memory_budget_bytes).map_err(|e| IslandError::engine(&e))?
        } else {
            store::open_local(&args.db_path, args.memory_budget_bytes)
                .map_err(|e| IslandError::engine(&e))?
        };

        let schema = store::read_json(&db, BRANCH_DEFAULT, "dataset")?;
        let has_city = store::read_default_spec(&mut db)?.is_some();
        if schema
            .as_ref()
            .is_some_and(|s| s["schema"] != if expanded { 2 } else { 1 })
            || (expanded && (has_city || preexisting_nonempty) && schema.is_none())
        {
            return Err(IslandError::code("failed_precondition.island.dataset"));
        }
        if expanded && schema.is_none() {
            store::write_json(
                &db,
                BRANCH_DEFAULT,
                "dataset",
                serde_json::json!({"schema":2}),
            )?;
        }
        let persist_ms = match store::read_default_spec(&mut db)? {
            Some(spec) if !store::spec_matches(&spec) => {
                return Err(IslandError::code("failed_precondition.island.city"));
            }
            Some(_) if store::read_meta_status(&mut db)? == Some("imported".into()) => 0,
            _ => store::import_city(&mut db, &extract, &seed)?,
        };

        if expanded {
            crate::places::import(&db)?;
        }
        let gazetteer = store::load_gazetteer(&db, &seed)?;
        for poi in &gazetteer {
            if !extract.nodes.iter().any(|n| n.id == poi.node) {
                return Err(IslandError::code("failed_precondition.island.city"));
            }
        }

        let index = store::load_city_index(&mut db, &extract)?;
        let city_nodes = index.node_ids.len() as u64;
        let city_edges = index.edge_count() as u64;
        let (desks, next_desk) =
            store::resume_desks(&mut db, &extract, &closure, city_nodes, city_edges)?;
        let findings = Log::new();
        for finding in findings::known_at_compile_time() {
            findings.push(finding);
        }
        let mut city = BTreeMap::new();
        city.insert(BRANCH_CITY.to_owned(), Arc::new(index));
        let mut branch_meta = BTreeMap::new();
        branch_meta.insert(
            BRANCH_CITY.to_owned(),
            BranchView {
                name: BRANCH_CITY.to_owned(),
                parent: BRANCH_DEFAULT.to_owned(),
                status: "imported".to_owned(),
                closed_edges: None,
            },
        );
        for desk in desks {
            branch_meta.insert(
                desk.name.clone(),
                BranchView {
                    name: desk.name.clone(),
                    parent: BRANCH_CITY.to_owned(),
                    status: if expanded {
                        if desk.closed_edges == 0 {
                            "reopened"
                        } else {
                            "closed"
                        }
                    } else {
                        store::META_CLOSED
                    }
                    .to_owned(),
                    closed_edges: Some(desk.closed_edges),
                },
            );
            city.insert(desk.name, Arc::new(desk.index));
        }
        let mut place_snapshots = BTreeMap::new();
        let mut address_catalogs = BTreeMap::new();
        let mut journeys = None;
        if expanded {
            for name in city.keys() {
                crate::places::import_on(&db, name)?;
                crate::addresses::import_on(&db, name)?;
                crate::journeys::import_on(&db, name)?;
                let version = crate::places::current_version(&db, name)?;
                place_snapshots.insert(
                    name.clone(),
                    Arc::new(crate::places::snapshot(&db, name, version)?),
                );
                if name == "city" {
                    journeys = Some(crate::journeys::Catalog::load(&db, name, version)?);
                }
                let catalog =
                    crate::addresses::load(&db, name, version, &place_snapshots[name].places)?;
                eprintln!(
                    "addresses: hydrated {name} in {:.1}s",
                    catalog.load_ms / 1000.
                );
                address_catalogs.insert(name.clone(), catalog);
            }
        }
        crate::scenario::checkpoint("address-cache");
        Ok(Self {
            mutations: Mutex::new(()),
            place_snapshots: Mutex::new(place_snapshots),
            expanded,
            journeys,
            address_catalogs: Mutex::new(address_catalogs),
            db: Mutex::new(db),
            city: Mutex::new(city),
            gazetteer: Mutex::new(gazetteer),
            closure,
            extract,
            findings: Arc::new(findings),
            durable: !args.cache,
            db_path: args.db_path.display().to_string(),
            last_persist_ms: AtomicU64::new(persist_ms),
            last_route: Mutex::new(None),
            last_compare: Mutex::new(None),
            focused: Mutex::new(BRANCH_CITY.to_owned()),
            next_desk: AtomicU64::new(next_desk),
            branch_meta: Mutex::new(branch_meta),
        })
    }

    pub fn city_index(&self) -> Arc<DriveIndex> {
        self.city
            .lock()
            .expect("city lock")
            .get(BRANCH_CITY)
            .expect("city index")
            .clone()
    }

    pub fn record_graph_error(&self, error: &IslandError) {
        if error.code.contains(".engine.") {
            self.findings.hit(
                Kind::Friction,
                "engine.graph",
                "Graph request returned an engine error",
                error.code.clone(),
                None,
            );
        }
    }

    pub fn expanded(&self) -> bool {
        self.expanded
    }

    pub fn subway(
        &self,
        branch: &str,
        explore: Option<(&str, usize)>,
    ) -> Result<serde_json::Value, IslandError> {
        let snapshot = self.place_snapshot(branch)?;
        let db = self.db.lock().expect("db lock");
        match explore {
            Some((seed, depth)) => {
                crate::subway::explore(&db, branch, snapshot.version, seed, depth)
            }
            None => crate::subway::network(&db, branch, snapshot.version),
        }
    }

    pub fn place_snapshot(
        &self,
        branch: &str,
    ) -> Result<Arc<crate::places::Snapshot>, IslandError> {
        self.place_snapshots
            .lock()
            .expect("place snapshots")
            .get(branch)
            .cloned()
            .ok_or(IslandError::code("not_found.island.catalog"))
    }

    pub fn place_detail(&self, branch: &str, id: &str) -> Result<serde_json::Value, IslandError> {
        let snapshot = self.place_snapshot(branch)?;
        let p = snapshot
            .places
            .iter()
            .find(|p| p.id == id)
            .ok_or(IslandError::code("not_found.island.place"))?;
        let db = self.db.lock().expect("db lock");
        crate::places::detail(&db, branch, snapshot.version, p)
    }

    pub fn gazetteer(&self) -> Vec<GazetteerPoi> {
        self.gazetteer.lock().expect("gazetteer lock").clone()
    }

    pub fn closure(&self) -> &ClosureFixture {
        &self.closure
    }

    pub fn extract(&self) -> &Extract {
        &self.extract
    }

    pub fn durable(&self) -> bool {
        self.durable
    }

    pub fn db_path(&self) -> &str {
        &self.db_path
    }

    pub fn last_persist_ms(&self) -> u64 {
        self.last_persist_ms.load(Ordering::Relaxed)
    }

    pub fn findings_snapshot(&self) -> Vec<Finding> {
        self.findings.snapshot()
    }

    /// RAM only. Never takes `db`.
    pub fn city_snapshot(&self, branch: Option<&str>) -> Result<Arc<DriveIndex>, IslandError> {
        let name = branch.unwrap_or(BRANCH_CITY);
        self.city
            .lock()
            .expect("city lock")
            .get(name)
            .cloned()
            .ok_or(IslandError::code("invalid_argument.island.branch"))
    }

    pub fn lookup_node(&self, index: &DriveIndex, token: &str) -> Result<usize, IslandError> {
        if token.starts_with("a:nyc:") {
            let catalog = self.address_catalog(&index.branch)?;
            let a = catalog
                .by_id
                .get(token)
                .and_then(|i| catalog.rows.get(*i))
                .ok_or(IslandError::code("not_found.island.address"))?;
            return a
                .place
                .node
                .as_deref()
                .and_then(|n| index.node_index(n))
                .ok_or(IslandError::code("failed_precondition.island.place_anchor"));
        }
        if self.expanded {
            let snapshots = self.place_snapshots.lock().expect("place snapshots");
            if let Some(snapshot) = snapshots.get(&index.branch) {
                if let Some(p) = snapshot
                    .places
                    .iter()
                    .find(|p| p.id == token || p.aliases.iter().any(|a| a == token))
                {
                    return p
                        .node
                        .as_deref()
                        .and_then(|n| index.node_index(n))
                        .ok_or(IslandError::code("failed_precondition.island.place_anchor"));
                }
            }
        }
        if let Some(poi) = self.gazetteer().into_iter().find(|p| p.id == token) {
            return index
                .node_ids
                .iter()
                .position(|id| id == &poi.node)
                .ok_or(IslandError::code("not_found.island.node"));
        }
        index
            .node_ids
            .iter()
            .position(|id| id == token)
            .ok_or(IslandError::code("not_found.island.node"))
    }

    pub fn snap_xy(&self, index: &DriveIndex, x: i32, y: i32) -> Result<usize, IslandError> {
        geo::snap(&index.xy, x, y)
    }

    pub fn last_route(&self) -> Option<RouteView> {
        self.last_route.lock().expect("last_route lock").clone()
    }

    pub fn last_compare(&self) -> Option<CompareView> {
        self.last_compare.lock().expect("last_compare lock").clone()
    }

    pub fn focused(&self) -> String {
        self.focused.lock().expect("focused lock").clone()
    }

    pub fn branch_views(&self) -> Vec<BranchView> {
        self.branch_meta
            .lock()
            .expect("branch_meta lock")
            .values()
            .cloned()
            .collect()
    }

    pub fn latest_desk(&self) -> Option<String> {
        self.branch_meta
            .lock()
            .expect("branch_meta lock")
            .keys()
            .filter(|name| store::desk_number(name).is_some())
            .max()
            .cloned()
    }

    /// RAM Dijkstra. Never takes `db`.
    pub fn route(
        &self,
        index: &DriveIndex,
        src: usize,
        dst: usize,
    ) -> Result<route::Path, IslandError> {
        route::route(index, src, dst)
    }

    /// Tests-only distance check. Not the click path (#3456).
    pub fn engine_sssp_distance_m(&self, from: &str, to: &str) -> Result<u32, IslandError> {
        let db = self.db.lock().expect("db lock");
        store::sssp_distance_m(&db, from, to)
    }

    /// Tests-only JSON round-trip. Picker reads RAM, not this.
    pub fn json_gazetteer_poi(&self, poi_id: &str) -> Result<GazetteerPoi, IslandError> {
        self.json_gazetteer_poi_on(BRANCH_CITY, poi_id)
    }

    pub fn json_gazetteer_poi_on(
        &self,
        branch_name: &str,
        poi_id: &str,
    ) -> Result<GazetteerPoi, IslandError> {
        let db = self.db.lock().expect("db lock");
        store::read_gazetteer_poi_on(&db, branch_name, poi_id)
    }

    /// Tests-only binding round-trip. Picker reads RAM, not this.
    pub fn graph_binding_node(&self, poi_id: &str) -> Result<String, IslandError> {
        let db = self.db.lock().expect("db lock");
        store::binding_node_for_poi(&db, poi_id)
    }

    pub fn node_binding_poi(&self, node_id: &str) -> Result<String, IslandError> {
        let db = self.db.lock().expect("db lock");
        store::node_binding_poi(&db, node_id)
    }

    pub fn route_view(
        &self,
        index: &DriveIndex,
        from: &str,
        to: &str,
        src: usize,
        dst: usize,
    ) -> Result<RouteView, IslandError> {
        let path = route::route(index, src, dst)?;
        let used_closed = route::uses_closed(&path, &self.closure.edges);
        let view = snapshot::route_view(&index.branch, from, to, &path, used_closed);
        *self.last_route.lock().expect("last_route lock") = Some(view.clone());
        Ok(view)
    }

    pub fn close_42nd(&self, from: Option<&str>) -> Result<CloseView, IslandError> {
        self.create_closure(from, &self.closure)
    }

    pub fn create_closure(
        &self,
        from: Option<&str>,
        closure: &ClosureFixture,
    ) -> Result<CloseView, IslandError> {
        self.create_closure_request(from, closure, None)
    }

    pub fn create_closure_request(
        &self,
        from: Option<&str>,
        closure: &ClosureFixture,
        request_id: Option<&str>,
    ) -> Result<CloseView, IslandError> {
        let _mutation = self.mutations.lock().expect("mutation coordinator");
        let from = from.unwrap_or(BRANCH_CITY);
        if from != BRANCH_CITY {
            return Err(IslandError::code("invalid_argument.island.branch"));
        }
        if closure.edges.is_empty() || closure.edges.len() > 200 || closure.name.len() > 120 {
            return Err(IslandError::code("invalid_argument.island.closure"));
        }
        let mut unique = std::collections::BTreeSet::new();
        for e in &closure.edges {
            if e.edge_type != "street"
                || !unique.insert((&e.src, &e.dst))
                || !self
                    .extract
                    .edges
                    .iter()
                    .any(|x| x.src == e.src && x.dst == e.dst)
            {
                return Err(IslandError::code("invalid_argument.island.closure"));
            }
        }
        let mut reserved = None;
        let request_key = request_id.map(|id| format!("closure-request:{id}"));
        if let Some(id) = request_id {
            if id.is_empty()
                || id.len() > 48
                || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err(IslandError::code("invalid_argument.island.request_id"));
            }
            let db = self.db.lock().expect("db");
            if let Some(record) = store::read_json(&db, "city", request_key.as_deref().unwrap())? {
                if record["closure"] != serde_json::to_value(closure).unwrap() {
                    return Err(IslandError::code("failed_precondition.island.request_id"));
                }
                let n = record["desk_number"]
                    .as_u64()
                    .ok_or(IslandError::code("failed_precondition.island.request_id"))?;
                let name = store::desk_name(n);
                if self.city.lock().expect("city").contains_key(&name) {
                    return Ok(CloseView {
                        desk: name,
                        closed_edges: closure.edges.len(),
                        persist_ms: 0,
                    });
                }
                // Archived/completed requests must never create another scenario.
                if store::read_json(&db, "city", &format!("scenario:{name}"))?.is_some() {
                    return Err(IslandError::code(
                        "failed_precondition.island.request_completed",
                    ));
                }
                reserved = Some(n);
            }
        }
        let live = self.city.lock().expect("city lock").len();
        if live >= LIVE_CAP {
            return Err(IslandError::code("failed_precondition.island.desk_cap"));
        }
        let next = reserved.unwrap_or_else(|| self.next_desk.load(Ordering::Relaxed));
        if let Some(key) = request_key.filter(|_| reserved.is_none()) {
            use stratadb::json::{JsonDocumentId, JsonPath, JsonSetEntry, JsonValue};
            let db = self.db.lock().expect("db");
            let mut meta = store::read_json(&db, "city", store::DOC_META)?.unwrap();
            meta["next_desk"] = serde_json::json!(next + 1);
            // Same-primitive batch atomically reserves the ID and retry token.
            db.json(store::branch("city")?, store::space()?)?
                .batch_set_or_create([
                    JsonSetEntry::new(
                        JsonDocumentId::new(key)?,
                        JsonPath::root(),
                        JsonValue::new(serde_json::json!({"desk_number":next,"closure":closure}))?,
                    ),
                    JsonSetEntry::new(
                        JsonDocumentId::new(store::DOC_META)?,
                        JsonPath::root(),
                        JsonValue::new(meta)?,
                    ),
                ])?;
            self.next_desk.store(next + 1, Ordering::Relaxed);
            crate::scenario::checkpoint("closure-request-reserved");
        }
        let outcome = {
            let mut db = self.db.lock().expect("db lock");
            store::close_42nd(&mut db, &self.extract, closure, next)?
        };
        if self.expanded {
            let inherited = self.address_catalog("city")?;
            self.address_catalogs
                .lock()
                .expect("address catalogs")
                .insert(outcome.desk.clone(), inherited);
            let db = self.db.lock().expect("db lock");
            let version = crate::places::current_version(&db, &outcome.desk)?;
            let snapshot = crate::places::snapshot(&db, &outcome.desk, version)?;
            self.place_snapshots
                .lock()
                .expect("place snapshots")
                .insert(outcome.desk.clone(), Arc::new(snapshot));
        }
        self.city
            .lock()
            .expect("city lock")
            .insert(outcome.desk.clone(), Arc::new(outcome.index));
        self.branch_meta.lock().expect("branch_meta lock").insert(
            outcome.desk.clone(),
            BranchView {
                name: outcome.desk.clone(),
                parent: BRANCH_CITY.to_owned(),
                status: store::META_CLOSED.to_owned(),
                closed_edges: Some(outcome.closed_edges),
            },
        );
        *self.focused.lock().expect("focused lock") = outcome.desk.clone();
        self.next_desk
            .fetch_max(outcome.next_desk, Ordering::Relaxed);
        self.last_persist_ms
            .store(outcome.persist_ms, Ordering::Relaxed);
        Ok(CloseView {
            desk: outcome.desk,
            closed_edges: outcome.closed_edges,
            persist_ms: outcome.persist_ms,
        })
    }

    pub fn scenario_history(&self, desk: &str) -> Result<serde_json::Value, IslandError> {
        let db = self.db.lock().expect("db lock");
        let mut state = crate::scenario::state(&db, desk)?
            .ok_or(IslandError::code("not_found.island.scenario"))?;
        state["current_version"] = serde_json::json!(crate::places::current_version(&db, desk)?);
        Ok(state)
    }

    pub fn scenario_operation(
        &self,
        desk: &str,
        id: &str,
        closed: bool,
        expected: u64,
    ) -> Result<serde_json::Value, IslandError> {
        let _mutation = self.mutations.lock().expect("mutation coordinator");
        if store::desk_number(desk).is_none()
            || !self.city.lock().expect("city lock").contains_key(desk)
        {
            return Err(IslandError::code("not_found.island.scenario"));
        }
        let mut db = self.db.lock().expect("db lock");
        crate::scenario::apply(&db, desk, &self.extract, id, closed, expected)?;
        let version = crate::places::current_version(&db, desk)?;
        let snapshot = crate::places::snapshot(&db, desk, version)?;
        let index = store::load_drive_index(&mut db, desk, &self.extract)?;
        let state = crate::scenario::state(&db, desk)?.unwrap();
        let last = state["operations"].as_array().unwrap().last().unwrap();
        let count = if last["closed"] == true {
            state["closure"]["edges"].as_array().unwrap().len()
        } else {
            0
        };
        self.city
            .lock()
            .expect("city lock")
            .insert(desk.into(), Arc::new(index));
        self.place_snapshots
            .lock()
            .expect("place snapshots")
            .insert(desk.into(), Arc::new(snapshot));
        if let Some(meta) = self
            .branch_meta
            .lock()
            .expect("branch metadata")
            .get_mut(desk)
        {
            meta.closed_edges = Some(count);
            meta.status = if count == 0 { "reopened" } else { "closed" }.into();
        }
        Ok(state)
    }

    pub fn historical_places(
        &self,
        desk: &str,
        version: u64,
    ) -> Result<crate::places::Snapshot, IslandError> {
        let db = self.db.lock().expect("db lock");
        let state = crate::scenario::state(&db, desk)?
            .ok_or(IslandError::code("not_found.island.scenario"))?;
        if crate::places::current_version(&db, desk)?.as_u64() != version
            && state["parent_version"].as_u64() != Some(version)
            && !state["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o["version"].as_u64() == Some(version))
        {
            return Err(IslandError::code("invalid_argument.island.version"));
        }
        crate::places::snapshot(&db, desk, stratadb::CommitVersion::new(version))
    }

    pub fn compare(&self, a: &str, b: &str) -> Result<CompareView, IslandError> {
        if a == b {
            return Err(IslandError::code("invalid_argument.island.branch"));
        }
        let view = {
            let mut db = self.db.lock().expect("db lock");
            store::compare_branches(&mut db, a, b)?
        };
        *self.last_compare.lock().expect("last_compare lock") = Some(view.clone());
        Ok(view)
    }

    pub fn archive(&self, desk: &str) -> Result<(), IslandError> {
        let _mutation = self.mutations.lock().expect("mutation coordinator");
        if desk == BRANCH_CITY || desk == BRANCH_DEFAULT || store::desk_number(desk).is_none() {
            return Err(IslandError::code("invalid_argument.island.branch"));
        }
        {
            let mut db = self.db.lock().expect("db lock");
            store::delete_branch(&mut db, desk)?;
        }
        self.city.lock().expect("city lock").remove(desk);
        self.address_catalogs
            .lock()
            .expect("address catalogs")
            .remove(desk);
        self.place_snapshots
            .lock()
            .expect("place snapshots")
            .remove(desk);
        self.branch_meta
            .lock()
            .expect("branch_meta lock")
            .remove(desk);
        let mut focused = self.focused.lock().expect("focused lock");
        if focused.as_str() == desk {
            *focused = BRANCH_CITY.to_owned();
        }
        let mut last = self.last_compare.lock().expect("last_compare lock");
        if last
            .as_ref()
            .is_some_and(|view| view.a == desk || view.b == desk)
        {
            *last = None;
        }
        Ok(())
    }

    pub fn verify_desk_chain(&self, desk: &str) -> Result<bool, IslandError> {
        let mut db = self.db.lock().expect("db lock");
        store::verify_chain(&mut db, desk)
    }

    /// `verify_chain` on each desk and `graph_info` on city. Failures are
    /// `Kind::Bug` with `error.code()`. Never holds RAM city with `db`.
    pub fn audit(&self) -> Result<AuditView, IslandError> {
        let ram = self.city_index();
        // This city is thousands of nodes; the count fits u64.
        let ram_nodes = u64::try_from(ram.node_ids.len()).unwrap_or(u64::MAX);
        let ram_edges = u64::try_from(ram.edge_count()).unwrap_or(u64::MAX);
        let desks: Vec<String> = self
            .branch_meta
            .lock()
            .expect("branch_meta lock")
            .keys()
            .filter(|name| store::desk_number(name).is_some())
            .cloned()
            .collect();

        let mut db = self.db.lock().expect("db lock");
        let (city_nodes, city_edges) = match store::city_graph_info(&mut db) {
            Ok(counts) => counts,
            Err(error) => {
                eprintln!("audit city graph_info: {}", error.code);
                self.findings.hit(
                    Kind::Bug,
                    "engine.graph",
                    "graph_info failed on city",
                    error.code.clone(),
                    None,
                );
                return Err(error);
            }
        };
        if city_nodes != ram_nodes || city_edges != ram_edges {
            self.findings.hit(
                Kind::Bug,
                "engine.graph",
                "graph_info counts do not match the RAM city",
                format!("graph_info {city_nodes}/{city_edges} ram {ram_nodes}/{ram_edges}"),
                None,
            );
        }

        let mut desk_audits = Vec::with_capacity(desks.len());
        let mut ok = city_nodes == ram_nodes && city_edges == ram_edges;
        for desk in desks {
            let chain_ok = match store::verify_chain(&mut db, &desk) {
                Ok(true) => true,
                Ok(false) => {
                    ok = false;
                    self.findings.hit(
                        Kind::Bug,
                        "engine.event",
                        "verify_chain returned false",
                        desk.clone(),
                        None,
                    );
                    false
                }
                Err(error) => {
                    ok = false;
                    eprintln!("audit {desk}: {}", error.code);
                    self.findings.hit(
                        Kind::Bug,
                        "engine.event",
                        "verify_chain failed",
                        error.code.clone(),
                        None,
                    );
                    false
                }
            };
            desk_audits.push(DeskAudit { desk, chain_ok });
        }
        drop(db);
        Ok(AuditView {
            ok,
            city_version: CITY_VERSION,
            city_nodes,
            city_edges,
            desks: desk_audits,
        })
    }

    pub fn delete_branch(&self, name: &str) -> Result<(), IslandError> {
        let mut db = self.db.lock().expect("db lock");
        store::delete_branch(&mut db, name)
    }

    /// Tests-only: fork and delete 42nd without writing `closed-42nd` meta
    /// so resume must repair.
    pub fn fork_desk_pending(&self) -> Result<String, IslandError> {
        let next = self.next_desk.load(Ordering::Relaxed);
        let desk = {
            let mut db = self.db.lock().expect("db lock");
            store::fork_desk_pending(&mut db, &self.extract, &self.closure, next)?
        };
        self.next_desk
            .store(next.saturating_add(1), Ordering::Relaxed);
        Ok(desk)
    }
}

/// 8×8 RAM city for unit tests (no Database).
pub struct RamCity {
    city: DriveIndex,
    gazetteer: Vec<GazetteerPoi>,
    closure: ClosureFixture,
}

impl RamCity {
    pub fn load(
        city_json: &str,
        closure_json: &str,
        gazetteer_json: &str,
    ) -> Result<Self, IslandError> {
        let extract = extract::parse_city(city_json)?;
        let closure = extract::parse_closure(closure_json)?;
        let gazetteer = extract::parse_gazetteer(gazetteer_json)?;
        Ok(Self {
            city: drive::from_extract("city", &extract),
            gazetteer,
            closure,
        })
    }

    pub fn city_index(&self) -> &DriveIndex {
        &self.city
    }

    pub fn gazetteer(&self) -> &[GazetteerPoi] {
        &self.gazetteer
    }

    pub fn closure(&self) -> &ClosureFixture {
        &self.closure
    }

    pub fn snap_xy(&self, x: i32, y: i32) -> Result<usize, IslandError> {
        geo::snap(&self.city.xy, x, y)
    }

    pub fn lookup_node(&self, token: &str) -> Result<usize, IslandError> {
        if let Some(poi) = self.gazetteer.iter().find(|poi| poi.id == token) {
            return self
                .city
                .node_index(&poi.node)
                .ok_or(IslandError::code("not_found.island.node"));
        }
        self.city
            .node_index(token)
            .ok_or(IslandError::code("not_found.island.node"))
    }

    pub fn route(&self, src: usize, dst: usize) -> Result<route::Path, IslandError> {
        route::route(&self.city, src, dst)
    }

    #[must_use]
    pub fn uses_closed(&self, path: &route::Path) -> bool {
        route::uses_closed(path, &self.closure.edges)
    }
}

impl World {
    pub fn address_catalog(
        &self,
        branch: &str,
    ) -> Result<Arc<crate::addresses::Catalog>, IslandError> {
        self.address_catalogs
            .lock()
            .expect("address catalogs")
            .get(branch)
            .cloned()
            .ok_or(IslandError::code("not_found.island.address_catalog"))
    }
    pub fn address_version(
        &self,
        branch: &str,
        requested: Option<u64>,
    ) -> Result<stratadb::CommitVersion, IslandError> {
        let current = self.place_snapshot(branch)?.version;
        let catalog = self.address_catalog(branch)?;
        let version = requested
            .map(stratadb::CommitVersion::new)
            .unwrap_or(current);
        if version < catalog.ready || version > current {
            return Err(IslandError::code(
                "failed_precondition.island.address_version",
            ));
        }
        Ok(version)
    }
    pub fn search(
        &self,
        branch: &str,
        version: Option<u64>,
        q: &str,
        category: Option<&str>,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<serde_json::Value, IslandError> {
        let v = self.address_version(branch, version)?;
        self.address_catalog(branch)?
            .search
            .query(q, category, limit, cursor, branch, v.as_u64())
    }
    pub fn address_detail(
        &self,
        branch: &str,
        version: Option<u64>,
        id: &str,
    ) -> Result<serde_json::Value, IslandError> {
        let v = self.address_version(branch, version)?;
        crate::addresses::detail(&self.db.lock().expect("db"), branch, v, id)
    }
    pub fn address_explore(
        &self,
        branch: &str,
        version: Option<u64>,
        id: &str,
        limit: usize,
    ) -> Result<serde_json::Value, IslandError> {
        let v = self.address_version(branch, version)?;
        crate::addresses::explore(&self.db.lock().expect("db"), branch, v, id, limit)
    }
    pub fn address_stations(
        &self,
        branch: &str,
        id: &str,
        version: Option<u64>,
    ) -> Result<serde_json::Value, IslandError> {
        use serde_json::json;
        use stratadb::graph::*;
        let v = self.address_version(branch, version)?;
        let current = self.place_snapshot(branch)?;
        let snapshot = if v == current.version {
            current
        } else {
            Arc::new(crate::places::snapshot(
                &self.db.lock().expect("db"),
                branch,
                v,
            )?)
        };
        let catalog = self.address_catalog(branch)?;
        let a = catalog
            .by_id
            .get(id)
            .map(|i| &catalog.rows[*i])
            .ok_or(IslandError::code("not_found.island.address"))?;
        let node = a
            .place
            .node
            .as_ref()
            .ok_or(IslandError::code("failed_precondition.island.place_anchor"))?;
        let start = std::time::Instant::now();
        let distances = snapshot
            .roads
            .sssp(&GraphNodeId::new(node)?, GraphDirection::Outgoing)?;
        let mut results: Vec<_> = snapshot
            .places
            .iter()
            .filter(|p| p.subway.is_some())
            .filter_map(|p| {
                p.node
                    .as_ref()
                    .and_then(|n| GraphNodeId::new(n).ok())
                    .and_then(|n| snapshot.roads.node_index(&n))
                    .and_then(|i| distances.distance(i))
                    .map(|d| (d, p))
            })
            .collect();
        results.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
        Ok(
            json!({"branch":branch,"version":v,"results":results.iter().take(5).map(|(d,p)|json!({"place":p,"distance_m":d,"network_distance_m":d,"origin_approach_m":a.place.approach_m,"station_approach_m":p.approach_m})).collect::<Vec<_>>(),"algorithm":"Strata outgoing SSSP","algorithm_ms":start.elapsed().as_secs_f64()*1000.,"semantics":"Directed street distance between approximate anchors; no subway riding or walking-time estimate."}),
        )
    }
    pub fn address_impact(
        &self,
        desk: &str,
        origin: &str,
        status: Option<&str>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<serde_json::Value, IslandError> {
        use serde_json::json;
        use stratadb::graph::*;
        if !(1..=100).contains(&limit)
            || desk == "city"
            || status.is_some_and(|s| {
                ![
                    "all",
                    "affected",
                    "unconnected",
                    "unchanged",
                    "farther",
                    "closer",
                    "newly_unreachable",
                    "newly_reachable",
                    "already_unreachable",
                ]
                .contains(&s)
            })
        {
            return Err(IslandError::code("invalid_argument.island.impact"));
        }
        let child = self.place_snapshot(desk)?;
        let catalog = self.address_catalog(desk)?;
        let (before, parent_version) = {
            let db = self.db.lock().expect("db");
            let state = crate::scenario::state(&db, desk)?
                .ok_or(IslandError::code("not_found.island.scenario"))?;
            let pv = state["parent_version"]
                .as_u64()
                .ok_or(IslandError::code("failed_precondition.island.version"))?;
            let g = db
                .graph(store::branch("city")?, store::space()?)?
                .adjacency_index_at_version(
                    &store::graph_name()?,
                    &store::analytics_budget(),
                    stratadb::CommitVersion::new(pv),
                )?;
            (g, pv)
        };
        let current = self.city_snapshot(Some(desk))?;
        let i = self.lookup_node(&current, origin)?;
        let node = GraphNodeId::new(&current.node_ids[i])?;
        let start = std::time::Instant::now();
        let a = before.sssp(&node, GraphDirection::Outgoing)?;
        let b = child.roads.sssp(&node, GraphDirection::Outgoing)?;
        let algorithm_ms = start.elapsed().as_secs_f64() * 1000.;
        let signature = format!(
            "{:016x}",
            extract::fnv1a64(
                format!(
                    "{desk}:{}:{parent_version}:{}:{origin}:{status:?}",
                    child.version, catalog.hash
                )
                .as_bytes()
            )
        );
        let offset = match cursor {
            None => 0,
            Some(c) => c
                .split_once(':')
                .filter(|(s, _)| *s == signature)
                .and_then(|(_, i)| i.parse::<usize>().ok())
                .ok_or(IslandError::code("invalid_argument.island.cursor"))?,
        };
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        let mut results = Vec::new();
        let mut matched = 0;
        for row in &catalog.rows {
            let node = row
                .place
                .node
                .as_ref()
                .and_then(|n| GraphNodeId::new(n).ok());
            let x = node
                .as_ref()
                .and_then(|n| before.node_index(n))
                .and_then(|i| a.distance(i));
            let y = node
                .as_ref()
                .and_then(|n| child.roads.node_index(n))
                .and_then(|i| b.distance(i));
            let state = crate::addresses::impact_status(node.is_some(), x, y);
            *counts.entry(state).or_default() += 1;
            if status.is_none_or(|s| {
                s == "all"
                    || s == state
                    || (s == "affected"
                        && ["farther", "closer", "newly_unreachable", "newly_reachable"]
                            .contains(&state))
            }) {
                if matched >= offset && results.len() < limit {
                    results
                        .push(json!({"place":row.place,"status":state,"before_m":x,"after_m":y}));
                }
                matched += 1;
            }
        }
        Ok(
            json!({"branch":desk,"version":child.version,"parent_version":parent_version,"catalog_version":catalog.ready,"counts":counts,"results":results,"total":matched,"cursor":if offset+limit<matched{Some(format!("{signature}:{}",offset+limit))}else{None},"algorithm":"Strata SSSP before/after + persisted address memberships","algorithm_ms":algorithm_ms,"baseline":"Original scenario parent street graph; current immutable address catalog applied to both road snapshots."}),
        )
    }
}
impl World {
    /// One published route view while scenario mutation/publication is excluded.
    pub fn route_request(
        &self,
        branch: &str,
        from: Option<&str>,
        to: Option<&str>,
        from_xy: Option<[i32; 2]>,
        to_xy: Option<[i32; 2]>,
        expected: Option<u64>,
    ) -> Result<serde_json::Value, IslandError> {
        use serde_json::json;
        let _guard = self.mutations.lock().expect("mutation coordinator");
        let version = if self.expanded {
            Some(self.place_snapshot(branch)?.version)
        } else {
            None
        };
        if expected.is_some_and(|v| version.is_none_or(|current| current.as_u64() != v)) {
            return Err(IslandError::code("failed_precondition.island.version"));
        }
        let index = self.city_snapshot(Some(branch))?;
        let resolve = |token: Option<&str>, xy: Option<[i32; 2]>| -> Result<usize, IslandError> {
            match (token, xy) {
                (Some(t), _) => self.lookup_node(&index, t),
                (_, Some([x, y])) => self.snap_xy(&index, x, y),
                _ => Err(IslandError::code("invalid_argument.island.snap")),
            }
        };
        let src = resolve(from, from_xy)?;
        let dst = resolve(to, to_xy)?;
        let from = from.unwrap_or(&index.node_ids[src]);
        let to = to.unwrap_or(&index.node_ids[dst]);
        let mut v = serde_json::to_value(self.route_view(&index, from, to, src, dst)?).unwrap();
        if self.expanded {
            v["version"] = json!(version);
            v["network_distance_m"] = v["length_m"].clone();
            let c = self.address_catalog(branch)?;
            for (key, id) in [("origin", from), ("destination", to)] {
                if let Some(&i) = c.by_id.get(id) {
                    let p = &c.rows[i].place;
                    v[key] = json!({"id":id,"name":p.name,"x":p.x,"y":p.y,"approach_distance_m":p.approach_m,"attachment":p.attachment});
                }
            }
        }
        Ok(v)
    }
}
impl World {
    pub fn address_discover(
        &self,
        branch: &str,
        version: Option<u64>,
        origin: &str,
        max_m: u32,
    ) -> Result<serde_json::Value, IslandError> {
        use serde_json::json;
        use stratadb::graph::*;
        if max_m > 10_000 {
            return Err(IslandError::code("invalid_argument.island.radius"));
        }
        let v = self.address_version(branch, version)?;
        let current = self.place_snapshot(branch)?;
        let roads = if current.version == v {
            current.roads.clone()
        } else {
            Arc::new(
                self.db
                    .lock()
                    .expect("db")
                    .graph(store::branch(branch)?, store::space()?)?
                    .adjacency_index_at_version(
                        &store::graph_name()?,
                        &store::analytics_budget(),
                        v,
                    )?,
            )
        };
        let index = self.city_snapshot(Some(branch))?;
        let n = self.lookup_node(&index, origin)?;
        let started = std::time::Instant::now();
        let distances = roads.sssp(
            &GraphNodeId::new(&index.node_ids[n])?,
            GraphDirection::Outgoing,
        )?;
        let algorithm_ms = started.elapsed().as_secs_f64() * 1000.;
        let catalog = self.address_catalog(branch)?;
        let mut results = Vec::new();
        let mut unconnected = 0;
        let mut unreachable = 0;
        let mut outside = 0;
        for a in &catalog.rows {
            if let Some(n) = &a.place.node {
                match roads
                    .node_index(&GraphNodeId::new(n)?)
                    .and_then(|i| distances.distance(i))
                {
                    Some(d) if d <= max_m as f64 => results.push((d, &a.place)),
                    Some(_) => outside += 1,
                    None => unreachable += 1,
                }
            } else {
                unconnected += 1
            }
        }
        results.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
        let total = results.len();
        Ok(
            json!({"branch":branch,"version":v,"total":total,"results":results.into_iter().take(100).map(|(d,p)|json!({"place":p,"network_distance_m":d})).collect::<Vec<_>>(),"truncated":total>100,"unconnected":unconnected,"unreachable":unreachable,"outside_radius":outside,"algorithm":"Strata outgoing SSSP + address memberships","algorithm_ms":algorithm_ms}),
        )
    }
}

impl World {
    pub fn transit_request(
        &self,
        branch: &str,
        from: Option<&str>,
        to: Option<&str>,
        from_xy: Option<[i32; 2]>,
        to_xy: Option<[i32; 2]>,
        expected: Option<u64>,
    ) -> Result<serde_json::Value, IslandError> {
        let _guard = self.mutations.lock().expect("mutation coordinator");
        let catalog = self
            .journeys
            .as_ref()
            .ok_or(IslandError::code("failed_precondition.island.dataset"))?;
        let snapshot = self.place_snapshot(branch)?;
        if expected.is_some_and(|v| v != snapshot.version.as_u64()) {
            return Err(IslandError::code("failed_precondition.island.version"));
        }
        let addresses = self.address_catalog(branch)?;
        let roads = self.city_snapshot(Some(branch))?;
        let resolve = |id: Option<&str>, xy: Option<[i32; 2]>| -> Result<[i32; 2], IslandError> {
            if let Some(id) = id {
                if let Some(&i) = addresses.by_id.get(id) {
                    let p = &addresses.rows[i].place;
                    return Ok([p.x, p.y]);
                }
                if let Some(p) = snapshot
                    .places
                    .iter()
                    .find(|p| p.id == id || p.aliases.iter().any(|a| a == id))
                {
                    return Ok([p.x, p.y]);
                }
                let i = self.lookup_node(&roads, id)?;
                return Ok([roads.xy[i].0, roads.xy[i].1]);
            }
            let xy = xy.ok_or(IslandError::code("invalid_argument.island.snap"))?;
            if !geo::in_aabb(xy[0], xy[1]) {
                return Err(IslandError::code("invalid_argument.island.snap"));
            }
            Ok(xy)
        };
        catalog.route(
            branch,
            snapshot.version.as_u64(),
            from,
            to,
            resolve(from, from_xy)?,
            resolve(to, to_xy)?,
        )
    }
}
