//! Replaceable multimodal adapter. Strata owns the weighted graph and computes
//! distances; application predecessors reconstruct legs until #3456 is available.
use crate::{error::IslandError, extract, store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    sync::{Arc, OnceLock},
};
use stratadb::{
    graph::*,
    json::{JsonDocumentId, JsonPath},
    CommitVersion, Database,
};
pub const GRAPH: &str = "journeys_v1";
pub const READY: &str = "ready:journeys-v1";
const FIXTURE: &str = include_str!(concat!(env!("OUT_DIR"), "/journeys-v1.json"));
#[derive(Clone, Deserialize, Serialize)]
pub struct Node {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub kind: String,
    #[serde(default)]
    pub name: String,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub seconds: u32,
    pub meters: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub points: Vec<[i32; 2]>,
    #[serde(default)]
    pub route: String,
    #[serde(default)]
    pub headsign: String,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub station: String,
    #[serde(default)]
    pub from_station: String,
    #[serde(default)]
    pub to_station: String,
}
#[derive(Deserialize)]
pub struct Data {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub routes: Vec<Value>,
    pub reference_date: String,
    pub semantics: String,
}
pub fn fixture() -> Arc<Data> {
    static DATA: OnceLock<Arc<Data>> = OnceLock::new();
    DATA.get_or_init(|| Arc::new(serde_json::from_str(FIXTURE).expect("pinned journey fixture")))
        .clone()
}
pub fn hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| format!("{:016x}", extract::fnv1a64(FIXTURE.as_bytes())))
}
pub fn ready_version(db: &Database, branch: &str) -> Result<Option<CommitVersion>, IslandError> {
    Ok(db
        .json(store::branch(branch)?, store::space()?)?
        .get_versioned(&JsonDocumentId::new(READY)?, &JsonPath::root())?
        .map(|r| r.version()))
}
pub fn import_on(db: &Database, branch: &str) -> Result<(), IslandError> {
    import_rows(db, branch, &fixture(), hash())
}
/// Deterministic publication, also exercised by small crash-recovery fixtures.
pub fn import_rows(
    db: &Database,
    branch: &str,
    data: &Data,
    source_hash: &str,
) -> Result<(), IslandError> {
    if let Some(ready) = store::read_json(db, branch, READY)? {
        if ready["hash"] != source_hash {
            return Err(IslandError::code(
                "failed_precondition.island.journey_catalog",
            ));
        }
        return Ok(());
    }
    let name = GraphName::new(GRAPH)?;
    let mut graph = db.graph(store::branch(branch)?, store::space()?)?;
    if graph.graph_info(&name)?.is_none() {
        graph.create_graph(name.clone())?;
    }
    eprintln!(
        "journeys: importing {} nodes / {} edges on {branch}",
        data.nodes.len(),
        data.edges.len()
    );
    // Separate, bounded idempotent stages. No delete_graph on interrupted import.
    for chunk in data.nodes.chunks(512) {
        let nodes = chunk
            .iter()
            .map(|n| {
                Ok((
                    GraphNodeId::new(&n.id)?,
                    GraphNodeData::new(
                        Some(GraphProperties::new(serde_json::to_value(n).unwrap())?),
                        None,
                    ),
                ))
            })
            .collect::<Result<Vec<_>, IslandError>>()?;
        graph.bulk_insert(&name, &nodes, &[], Some(512))?;
    }
    crate::scenario::checkpoint("journeys-nodes");
    for chunk in data.edges.chunks(512) {
        let edges = chunk
            .iter()
            .map(|e| {
                Ok((
                    GraphNodeId::new(&e.source)?,
                    GraphEdgeType::new(&e.kind)?,
                    GraphNodeId::new(&e.target)?,
                    GraphEdgeData::new(
                        e.seconds as f64,
                        Some(GraphProperties::new(serde_json::to_value(e).unwrap())?),
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, IslandError>>()?;
        graph.bulk_insert(&name, &[], &edges, Some(512))?;
    }
    crate::scenario::checkpoint("journeys-edges");
    let info = graph.graph_info(&name)?.unwrap();
    if info.node_count() != data.nodes.len() as u64 || info.edge_count() != data.edges.len() as u64
    {
        return Err(IslandError::code(
            "failed_precondition.island.journey_catalog",
        ));
    }
    drop(graph);
    store::write_json(
        db,
        branch,
        READY,
        json!({"hash":source_hash,"nodes":data.nodes.len(),"edges":data.edges.len(),"reference_date":data.reference_date}),
    )?;
    crate::scenario::checkpoint("journeys-ready");
    Ok(())
}
pub struct Catalog {
    pub graph: GraphAdjacencyIndex,
    pub data: Arc<Data>,
    xy: Vec<[i32; 2]>,
    grid: HashMap<(i32, i32), Vec<usize>>,
    edges: HashMap<(usize, usize, usize), usize>,
    names: HashMap<String, String>,
}
impl Catalog {
    pub fn load(
        db: &Database,
        branch: &str,
        version: CommitVersion,
    ) -> Result<Arc<Self>, IslandError> {
        let data = fixture();
        let graph = db
            .graph(store::branch(branch)?, store::space()?)?
            .adjacency_index_at_version(
                &GraphName::new(GRAPH)?,
                &GraphAnalyticsBudget::new(data.nodes.len(), data.edges.len()),
                version,
            )?;
        Self::from_graph(graph, data).map(Arc::new)
    }
    pub fn from_graph(graph: GraphAdjacencyIndex, data: Arc<Data>) -> Result<Self, IslandError> {
        if graph.node_count() != data.nodes.len() || graph.edge_count() != data.edges.len() as u64 {
            return Err(IslandError::code(
                "failed_precondition.island.journey_catalog",
            ));
        }
        let mut xy = vec![[0, 0]; graph.node_count()];
        let mut grid: HashMap<_, Vec<_>> = HashMap::new();
        let mut names = HashMap::new();
        let ids: HashMap<_, _> = graph
            .node_ids()
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect();
        for n in &data.nodes {
            let Some(&i) = ids.get(n.id.as_str()) else {
                return Err(IslandError::code(
                    "failed_precondition.island.journey_catalog",
                ));
            };
            xy[i] = [n.x, n.y];
            if n.kind == "walk" {
                grid.entry((n.x.div_euclid(150), n.y.div_euclid(150)))
                    .or_default()
                    .push(i);
            }
            if n.kind == "station" {
                names.insert(n.id.clone(), n.name.clone());
            }
        }
        let mut edges = HashMap::new();
        for (i, e) in data.edges.iter().enumerate() {
            let a = ids[e.source.as_str()];
            let b = ids[e.target.as_str()];
            let kind =
                graph
                    .edge_type_index(&GraphEdgeType::new(&e.kind)?)
                    .ok_or(IslandError::code(
                        "failed_precondition.island.journey_catalog",
                    ))?;
            if !graph.outgoing(a).iter().any(|v| {
                v.neighbor() == b && v.edge_type() == kind && v.weight() == e.seconds as f64
            }) {
                return Err(IslandError::code(
                    "failed_precondition.island.journey_catalog",
                ));
            }
            edges.insert((a, b, kind), i);
        }
        Ok(Self {
            graph,
            data,
            xy,
            grid,
            edges,
            names,
        })
    }
    fn snap(&self, id: Option<&str>, xy: [i32; 2]) -> Result<(usize, u32), IslandError> {
        if let Some(id) = id.filter(|s| s.starts_with("p:mta:station:")) {
            if let Some(i) = self.graph.node_index(&GraphNodeId::new(id)?) {
                return Ok((i, 0));
            }
        }
        let (gx, gy) = (xy[0].div_euclid(150), xy[1].div_euclid(150));
        let mut best = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(nodes) = self.grid.get(&(gx + dx, gy + dy)) {
                    for &i in nodes {
                        let [x, y] = self.xy[i];
                        let d = (x as i64 - xy[0] as i64).pow(2) + (y as i64 - xy[1] as i64).pow(2);
                        if d <= 150 * 150 && best.is_none_or(|b| (d, i) < b) {
                            best = Some((d, i));
                        }
                    }
                }
            }
        }
        best.map(|(d, i)| (i, (d as f64).sqrt().round() as u32))
            .ok_or(IslandError::code("failed_precondition.island.walk_anchor"))
    }
    fn name(&self, id: &str) -> String {
        self.names.get(id).cloned().unwrap_or_else(|| id.to_owned())
    }
    pub fn route(
        &self,
        branch: &str,
        version: u64,
        from: Option<&str>,
        to: Option<&str>,
        start: [i32; 2],
        end: [i32; 2],
    ) -> Result<Value, IslandError> {
        let (src, approach) = self.snap(from, start)?;
        let (dst, exit) = self.snap(to, end)?;
        let started = std::time::Instant::now();
        let native = self
            .graph
            .sssp(self.graph.node_id(src).unwrap(), GraphDirection::Outgoing)?;
        let total = native
            .distance(dst)
            .ok_or(IslandError::code("failed_precondition.island.unreachable"))?
            as u32;
        let algorithm_ms = started.elapsed().as_secs_f64() * 1000.;
        let mut dist = vec![u32::MAX; self.graph.node_count()];
        let mut pred = vec![None; dist.len()];
        let mut heap = BinaryHeap::new();
        dist[src] = 0;
        heap.push(Reverse((0, src)));
        while let Some(Reverse((cost, a))) = heap.pop() {
            if cost != dist[a] {
                continue;
            }
            if a == dst {
                break;
            }
            for e in self.graph.outgoing(a) {
                let b = e.neighbor();
                let next = cost.saturating_add(e.weight() as u32);
                if next < dist[b] {
                    dist[b] = next;
                    pred[b] = Some((a, self.edges[&(a, b, e.edge_type())]));
                    heap.push(Reverse((next, b)));
                }
            }
        }
        if dist[dst] != total {
            return Err(IslandError::code(
                "failed_precondition.island.journey_distance",
            ));
        }
        let mut path = vec![];
        let mut cur = dst;
        while cur != src {
            let (p, e) =
                pred[cur].ok_or(IslandError::code("failed_precondition.island.unreachable"))?;
            path.push(e);
            cur = p;
        }
        path.reverse();
        let mut legs: Vec<Value> = vec![];
        let mut points = vec![json!({"x":start[0],"y":start[1]})];
        let mut distance = approach + exit;
        let mut walking = approach + exit;
        let mut boardings: u32 = 0;
        let walk_leg =
            |legs: &mut Vec<Value>, pts: Vec<[i32; 2]>, meters: u32, seconds: u32, approx: bool| {
                if legs.last().is_none_or(|l| l["mode"] != "walk") {
                    legs.push(
                    json!({"mode":"walk","meters":0,"seconds":0,"points":[],"approximate":false}),
                );
                }
                let l = legs.last_mut().unwrap();
                l["meters"] = json!(l["meters"].as_u64().unwrap() + meters as u64);
                l["seconds"] = json!(l["seconds"].as_u64().unwrap() + seconds as u64);
                l["approximate"] = json!(l["approximate"] == true || approx);
                l["points"]
                    .as_array_mut()
                    .unwrap()
                    .extend(pts.into_iter().map(|p| json!({"x":p[0],"y":p[1]})));
            };
        if approach > 0 {
            walk_leg(
                &mut legs,
                vec![start, self.xy[src]],
                approach,
                (approach as f64 / 1.35).ceil() as u32,
                true,
            );
        }
        for i in path {
            let e = &self.data.edges[i];
            distance += e.meters;
            points.extend(e.points.iter().map(|p| json!({"x":p[0],"y":p[1]})));
            match e.kind.as_str() {
                "walk" | "access" | "transfer" => {
                    walking += e.meters;
                    walk_leg(
                        &mut legs,
                        e.points.clone(),
                        e.meters,
                        e.seconds,
                        e.kind != "walk",
                    );
                }
                "board" => {
                    boardings += 1;
                    let color = self
                        .data
                        .routes
                        .iter()
                        .find(|r| r["id"] == e.route)
                        .and_then(|r| r["color"].as_str())
                        .unwrap_or("2563eb");
                    legs.push(json!({"mode":"subway","route":e.route,"headsign":e.headsign,"color":color,"from":self.name(&e.station),"to":self.name(&e.station),"stops":0,"seconds":e.seconds,"meters":0,"points":[],"boarding_allowance_s":e.seconds}));
                }
                "ride" => {
                    let l = legs
                        .last_mut()
                        .ok_or(IslandError::code("failed_precondition.island.journey_path"))?;
                    if l["mode"] != "subway" {
                        return Err(IslandError::code("failed_precondition.island.journey_path"));
                    }
                    l["to"] = json!(self.name(&e.to_station));
                    l["stops"] = json!(l["stops"].as_u64().unwrap() + 1);
                    l["seconds"] = json!(l["seconds"].as_u64().unwrap() + e.seconds as u64);
                    l["meters"] = json!(l["meters"].as_u64().unwrap() + e.meters as u64);
                    l["points"]
                        .as_array_mut()
                        .unwrap()
                        .extend(e.points.iter().map(|p| json!({"x":p[0],"y":p[1]})));
                }
                "alight" => {
                    if let Some(l) = legs.last_mut() {
                        l["seconds"] = json!(l["seconds"].as_u64().unwrap() + e.seconds as u64);
                    }
                }
                _ => return Err(IslandError::code("failed_precondition.island.journey_path")),
            }
        }
        if exit > 0 {
            walk_leg(
                &mut legs,
                vec![self.xy[dst], end],
                exit,
                (exit as f64 / 1.35).ceil() as u32,
                true,
            );
        }
        if legs.is_empty() {
            walk_leg(&mut legs, vec![start, end], 0, 0, false);
        }
        points.push(json!({"x":end[0],"y":end[1]}));
        Ok(
            json!({"mode":"transit","branch":branch,"version":version,"from":from,"to":to,"points":points,"nodes":[],"length_m":distance,"walking_m":walking,"duration_s":total+(approach as f64/1.35).ceil() as u32+(exit as f64/1.35).ceil() as u32,"transfers":boardings.saturating_sub(1),"boardings":boardings,"legs":legs,"used_closed":false,"estimate":true,"reference_date":self.data.reference_date,"semantics":self.data.semantics,"algorithm":"Strata SSSP on pedestrian + train-pattern states; application path reconstruction","algorithm_ms":algorithm_ms,"scenario_effect":"Car closures leave pedestrian and subway service unchanged."}),
        )
    }
}
