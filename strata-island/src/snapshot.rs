//! Frozen wire contract for `/api/city`, `/api/meta`, `/api/route`, and `/api/audit`.

use serde::Serialize;

use crate::drive::DriveIndex;
use crate::extract::GazetteerPoi;
use crate::findings::Finding;
use crate::geo::{ORIGIN_LAT_E4, ORIGIN_LON_E4};
use crate::world::World;

#[derive(Serialize)]
pub struct CityView {
    pub branch: String,
    pub origin: OriginView,
    pub nodes: Vec<NodeView>,
    pub edges: Vec<EdgeView>,
}

#[derive(Serialize)]
pub struct OriginView {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Serialize)]
pub struct NodeView {
    pub id: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Serialize)]
pub struct EdgeView {
    pub s: usize,
    pub d: usize,
    pub m: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,
}

#[derive(Serialize)]
pub struct MetaView {
    pub durable: bool,
    pub db_path: String,
    pub city_version: u32,
    pub focused: String,
    pub nodes: usize,
    pub edges: usize,
    pub persist_ms: u64,
    pub branch_count: usize,
    pub live_cap: usize,
    pub branches: Vec<BranchView>,
    pub findings: Vec<Finding>,
    pub last_compare: Option<CompareView>,
    pub last_route: Option<RouteView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompareView {
    pub a: String,
    pub b: String,
    pub empty: bool,
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    pub graph_entities: usize,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CloseView {
    pub desk: String,
    pub closed_edges: usize,
    pub persist_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuditView {
    pub ok: bool,
    pub city_version: u32,
    pub city_nodes: u64,
    pub city_edges: u64,
    pub desks: Vec<DeskAudit>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeskAudit {
    pub desk: String,
    pub chain_ok: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct RouteView {
    pub branch: String,
    pub from: String,
    pub to: String,
    pub length_m: u32,
    pub nodes: Vec<String>,
    pub points: Vec<PointView>,
    pub used_closed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PointView {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Serialize)]
pub struct BranchView {
    pub name: String,
    pub parent: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_edges: Option<usize>,
}

/// Frozen in PR6. Do not bump without a new extract and a new talk.
pub const CITY_VERSION: u32 = 1;
pub const LIVE_CAP: usize = 4;

impl OriginView {
    #[must_use]
    pub fn battery() -> Self {
        Self {
            lat: f64::from(ORIGIN_LAT_E4) / 10_000.0,
            lon: f64::from(ORIGIN_LON_E4) / 10_000.0,
        }
    }
}

pub fn city_view(index: &DriveIndex) -> CityView {
    let mut nodes = Vec::with_capacity(index.node_ids.len());
    for (id, (x, y)) in index.node_ids.iter().zip(index.xy.iter()) {
        nodes.push(NodeView {
            id: id.clone(),
            x: *x,
            y: *y,
        });
    }
    let mut edges = Vec::new();
    for (src, outgoing) in index.outgoing.iter().enumerate() {
        for edge in outgoing {
            edges.push(EdgeView {
                s: src,
                d: edge.dst,
                m: edge.length_m,
                n: edge.name.clone(),
            });
        }
    }
    CityView {
        branch: index.branch.clone(),
        origin: OriginView::battery(),
        nodes,
        edges,
    }
}

pub fn meta_view(world: &World) -> MetaView {
    let focused = world.focused();
    // Focused is always a live RAM branch; city is the fallback if archive raced.
    let index = world
        .city_snapshot(Some(&focused))
        .unwrap_or_else(|_| world.city_index());
    let branches = world.branch_views();
    MetaView {
        durable: world.durable(),
        db_path: world.db_path().to_owned(),
        city_version: CITY_VERSION,
        focused,
        nodes: index.node_ids.len(),
        edges: index.edge_count(),
        persist_ms: world.last_persist_ms(),
        branch_count: branches.len(),
        live_cap: LIVE_CAP,
        branches,
        findings: world.findings_snapshot(),
        last_compare: world.last_compare(),
        last_route: world.last_route(),
    }
}

pub fn route_view(
    branch: &str,
    from: &str,
    to: &str,
    path: &crate::route::Path,
    used_closed: bool,
) -> RouteView {
    RouteView {
        branch: branch.to_owned(),
        from: from.to_owned(),
        to: to.to_owned(),
        length_m: path.length_m,
        nodes: path.nodes.clone(),
        points: path
            .points
            .iter()
            .map(|&(x, y)| PointView { x, y })
            .collect(),
        used_closed,
    }
}

pub fn gazetteer_view(pois: &[GazetteerPoi]) -> Vec<GazetteerPoi> {
    pois.to_vec()
}
