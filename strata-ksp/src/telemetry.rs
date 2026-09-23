//! Event payload shapes. All objects, all finite.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ascent::AscentPhase;
use crate::craft::CraftSpec;
use crate::physics::{FlightStatus, OrbitElements, Vec2, Vessel};
use crate::snapshot::TrailSample;

pub const EVENT_TICK: &str = "tick";
pub const EVENT_LAUNCH: &str = "launch";
pub const EVENT_STAGE: &str = "stage";
pub const EVENT_ORBIT: &str = "orbit";
pub const EVENT_CRASH: &str = "crash";
pub const EVENT_FLAMEOUT: &str = "flameout";
pub const EVENT_ESCAPED: &str = "escaped";
pub const EVENT_CONTROL: &str = "control";
pub const EVENT_VAB_SAVE: &str = "vab_save";
pub const EVENT_REWIND: &str = "rewind";
pub const EVENT_EDIT: &str = "edit";
pub const EVENT_FORK: &str = "fork";
pub const EVENT_PROMOTED: &str = "promoted";

pub const TAPE_CAP: usize = 2400;
pub const TICK_DT: f64 = 0.25;
pub const LIVE_LAUNCH_CAP: usize = 8;
pub const EVENT_SOFT_CAP: u64 = 20_000;
pub const WALL_TICK_MIN_MS: u128 = 125;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VesselSample {
    pub name: String,
    pub parent: String,
    pub design: String,
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub fuel: f64,
    pub mass: f64,
    pub stage: u32,
    pub theta: f64,
    pub throttle: f64,
    pub pe: f64,
    pub ap: f64,
    pub e: f64,
    pub status: FlightStatus,
    pub autopilot: bool,
    pub phase: AscentPhase,
    pub warp: u32,
    #[serde(default)]
    pub last_event_seq: u64,
}

impl VesselSample {
    #[allow(clippy::too_many_arguments)]
    pub fn from_ship(
        name: &str,
        parent: &str,
        design: &str,
        vessel: &Vessel,
        stage: u32,
        autopilot: bool,
        phase: AscentPhase,
        warp: u32,
    ) -> Self {
        let el = OrbitElements::of(vessel.r, vessel.v);
        Self {
            name: name.to_owned(),
            parent: parent.to_owned(),
            design: design.to_owned(),
            t: vessel.t,
            x: vessel.r.x,
            y: vessel.r.y,
            vx: vessel.v.x,
            vy: vessel.v.y,
            fuel: vessel.fuel,
            mass: vessel.mass,
            stage,
            theta: vessel.facing.y.atan2(vessel.facing.x),
            throttle: vessel.throttle,
            pe: el.h_pe(),
            ap: el.h_ap(),
            e: el.e,
            status: vessel.status,
            autopilot,
            phase,
            warp,
            last_event_seq: 0,
        }
    }

    pub fn trail_sample(&self) -> TrailSample {
        TrailSample {
            t: self.t,
            x: self.x,
            y: self.y,
            seq: self.last_event_seq,
            fuel: self.fuel,
        }
    }

    pub fn apply_to(&self, vessel: &mut Vessel) {
        vessel.t = self.t;
        vessel.r = Vec2::new(self.x, self.y);
        vessel.v = Vec2::new(self.vx, self.vy);
        vessel.fuel = self.fuel;
        vessel.mass = self.mass.max(1e-12);
        vessel.throttle = self.throttle;
        vessel.status = self.status;
        let (sin, cos) = self.theta.sin_cos();
        // theta = atan2(y, x) so facing = (cos θ, sin θ)
        vessel.facing = Vec2::new(cos, sin).normalized();
    }
}

pub fn finite_or_null(value: f64) -> Value {
    if value.is_finite() {
        json!(value)
    } else {
        Value::Null
    }
}

pub fn event_object(kind: &str, sample: &VesselSample) -> Value {
    json!({
        "t": sample.t,
        "kind": kind,
        "x": sample.x,
        "y": sample.y,
        "vx": sample.vx,
        "vy": sample.vy,
        "fuel": sample.fuel,
        "mass": sample.mass,
        "stage": sample.stage,
        "theta": sample.theta,
        "throttle": sample.throttle,
        "pe": finite_or_null(sample.pe),
        "ap": finite_or_null(sample.ap),
        "e": finite_or_null(sample.e),
        "status": sample.status,
    })
}

pub fn launch_object(sample: &VesselSample, craft_hash: &str) -> Value {
    let mut obj = event_object(EVENT_LAUNCH, sample);
    if let Some(map) = obj.as_object_mut() {
        map.insert("craft_hash".into(), json!(craft_hash));
        map.insert("parent".into(), json!(sample.parent));
        map.insert("design".into(), json!(sample.design));
    }
    obj
}

pub fn stage_object(sample: &VesselSample, dropped: &[String], dropped_fuel: f64) -> Value {
    let mut obj = event_object(EVENT_STAGE, sample);
    if let Some(map) = obj.as_object_mut() {
        map.insert("dropped".into(), json!(dropped));
        map.insert("dropped_fuel".into(), json!(dropped_fuel));
    }
    obj
}

pub fn control_object(sample: &VesselSample) -> Value {
    let mut obj = event_object(EVENT_CONTROL, sample);
    if let Some(map) = obj.as_object_mut() {
        map.insert("autopilot".into(), json!(sample.autopilot));
        map.insert(
            "facing".into(),
            json!(if sample.autopilot { "auto" } else { "hold" }),
        );
        map.insert("angle".into(), json!(sample.theta));
    }
    obj
}

pub fn merge_extra(mut payload: Value, extra: &Value) -> Value {
    if let (Some(dst), Some(src)) = (payload.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    payload
}

pub fn craft_hash(spec: &CraftSpec) -> String {
    let bytes = serde_json::to_vec(&spec.to_doc()).expect("craft json encodes");
    let mut hash = 0xcbf29ce484222325u64;
    for byte in &bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn sample_from_payload(payload: &Value, fallback: &VesselSample) -> VesselSample {
    let mut sample = fallback.clone();
    if let Some(t) = payload.get("t").and_then(Value::as_f64) {
        sample.t = t;
    }
    if let Some(x) = payload.get("x").and_then(Value::as_f64) {
        sample.x = x;
    }
    if let Some(y) = payload.get("y").and_then(Value::as_f64) {
        sample.y = y;
    }
    if let Some(vx) = payload.get("vx").and_then(Value::as_f64) {
        sample.vx = vx;
    }
    if let Some(vy) = payload.get("vy").and_then(Value::as_f64) {
        sample.vy = vy;
    }
    if let Some(fuel) = payload.get("fuel").and_then(Value::as_f64) {
        sample.fuel = fuel;
    }
    if let Some(mass) = payload.get("mass").and_then(Value::as_f64) {
        sample.mass = mass;
    }
    if let Some(stage) = payload.get("stage").and_then(Value::as_u64) {
        sample.stage = stage as u32;
    }
    if let Some(theta) = payload.get("theta").and_then(Value::as_f64) {
        sample.theta = theta;
    }
    if let Some(throttle) = payload.get("throttle").and_then(Value::as_f64) {
        sample.throttle = throttle;
    }
    if let Some(pe) = payload.get("pe").and_then(Value::as_f64) {
        sample.pe = pe;
    }
    if let Some(ap) = payload.get("ap").and_then(Value::as_f64) {
        sample.ap = ap;
    }
    if let Some(e) = payload.get("e").and_then(Value::as_f64) {
        sample.e = e;
    }
    if let Some(status) = payload
        .get("status")
        .and_then(Value::as_str)
        .and_then(parse_status)
    {
        sample.status = status;
    }
    if let Some(parent) = payload.get("parent").and_then(Value::as_str) {
        sample.parent = parent.to_owned();
    }
    if let Some(design) = payload.get("design").and_then(Value::as_str) {
        sample.design = design.to_owned();
    }
    sample
}

pub fn rewind_object(sample: &VesselSample, to_seq: u64) -> Value {
    let mut obj = event_object(EVENT_REWIND, sample);
    if let Some(map) = obj.as_object_mut() {
        map.insert("to_seq".into(), json!(to_seq));
    }
    obj
}

pub fn edit_object(sample: &VesselSample, op: &str, part: &str, index: usize) -> Value {
    let mut obj = event_object(EVENT_EDIT, sample);
    if let Some(map) = obj.as_object_mut() {
        map.insert("op".into(), json!(op));
        map.insert("part".into(), json!(part));
        map.insert("index".into(), json!(index));
    }
    obj
}

pub fn fork_object(sample: &VesselSample, parent: &str, at_seq: u64) -> Value {
    let mut obj = event_object(EVENT_FORK, sample);
    if let Some(map) = obj.as_object_mut() {
        map.insert("parent".into(), json!(parent));
        map.insert("at_seq".into(), json!(at_seq));
        map.insert("design".into(), json!(sample.design));
    }
    obj
}

pub fn parse_status(text: &str) -> Option<FlightStatus> {
    match text {
        "flying" => Some(FlightStatus::Flying),
        "orbit" => Some(FlightStatus::Orbit),
        "suborbital" => Some(FlightStatus::Suborbital),
        "crashed" => Some(FlightStatus::Crashed),
        "escaped" => Some(FlightStatus::Escaped),
        _ => None,
    }
}

pub fn status_event_type(status: FlightStatus) -> Option<&'static str> {
    match status {
        FlightStatus::Orbit => Some(EVENT_ORBIT),
        FlightStatus::Crashed => Some(EVENT_CRASH),
        FlightStatus::Escaped => Some(EVENT_ESCAPED),
        FlightStatus::Flying | FlightStatus::Suborbital => None,
    }
}
