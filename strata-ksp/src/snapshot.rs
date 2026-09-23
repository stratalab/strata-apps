//! Wire contract for GET /api/state and /ws. Frozen in PR1; filled in later PRs.

use serde::Serialize;

use crate::findings::Finding;
use crate::physics::{OrbitElements, Vec2, Vessel};

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub running: bool,
    pub hz: f64,
    pub persist_ms: f64,
    pub avg_persist_ms: f64,
    pub max_persist_ms: f64,
    pub commits_this_persist: u64,
    pub total_commits: u64,
    pub branch_count: u32,
    pub durable: bool,
    pub db_path: String,
    pub physics_steps_last_frame: u32,
    pub live_launch_cap: u32,
    pub vab: VabView,
    pub launches: Vec<LaunchView>,
    pub focused: String,
    pub findings: Vec<Finding>,
    pub last_compare: Option<serde_json::Value>,
    pub last_promote: Option<serde_json::Value>,
    pub last_archive: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VabView {
    pub name: String,
    pub parts: Vec<PartView>,
    pub catalog: Vec<CatalogPartView>,
    pub wet_mass: f64,
    pub dry_mass: f64,
    pub fuel: f64,
    pub dv_budget_mps: f64,
    /// Thrust of the stage that fires first, in newtons. With wet mass this
    /// is thrust-to-weight, which is the number that decides whether a stack
    /// leaves the pad at all.
    pub thrust_n: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PartView {
    pub part_id: String,
    pub ordinal: u32,
    pub kind: String,
    pub dry_kg: f64,
    pub fuel_kg: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogPartView {
    pub part_id: String,
    pub kind: String,
    pub dry_kg: f64,
    pub fuel_cap_kg: f64,
    pub thrust_n: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct LaunchView {
    pub name: String,
    pub parent: String,
    /// The version of `parent` this branch was cut at. Zero when the launch
    /// came off the pad, so a client can tell a root from a fork.
    pub fork_seq: u64,
    pub design: String,
    pub status: crate::physics::FlightStatus,
    pub t: f64,
    pub seq: u64,
    pub warp: u32,
    pub autopilot: bool,
    pub throttle: f64,
    pub mass: f64,
    pub fuel: f64,
    pub stage: u32,
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub theta: f64,
    pub pe: f64,
    pub ap: f64,
    pub e: f64,
    pub trail: Vec<TrailSample>,
    pub predicted: Vec<TrailSample>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct TrailSample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub seq: u64,
    /// Fuel remaining at this sample. Lets a client tell whether a burn lies
    /// between two points in a flight, which is the only thing that decides
    /// whether changing the vehicle's mass can change where it goes.
    #[serde(default)]
    pub fuel: f64,
}

impl TrailSample {
    pub fn from_vec(t: f64, p: Vec2) -> Self {
        Self {
            t,
            x: p.x,
            y: p.y,
            seq: 0,
            fuel: 0.0,
        }
    }
}

impl LaunchView {
    pub fn from_vessel(
        name: &str,
        vessel: &Vessel,
        trail: &[TrailSample],
        seq: u64,
        warp: u32,
    ) -> Self {
        let el = OrbitElements::of(vessel.r, vessel.v);
        let predicted = crate::physics::predicted_conic(vessel.r, vessel.v, 96)
            .into_iter()
            .map(|p| TrailSample::from_vec(vessel.t, p))
            .collect();
        Self {
            name: name.to_owned(),
            parent: "vab".into(),
            fork_seq: 0,
            design: "design-0001".into(),
            status: vessel.status,
            t: vessel.t,
            seq,
            warp,
            autopilot: true,
            throttle: vessel.throttle,
            mass: vessel.mass,
            fuel: vessel.fuel,
            stage: 0,
            x: vessel.r.x,
            y: vessel.r.y,
            vx: vessel.v.x,
            vy: vessel.v.y,
            theta: vessel.facing.y.atan2(vessel.facing.x),
            pe: el.h_pe(),
            ap: el.h_ap(),
            e: el.e,
            trail: trail.to_vec(),
            predicted,
        }
    }
}
