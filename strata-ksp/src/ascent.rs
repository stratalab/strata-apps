//! Ascent-1: snap heading, no PID. Pure function of (t, vessel, spec, phase).

use serde::{Deserialize, Serialize};

use crate::craft::{CraftSpec, StageDrop};
use crate::physics::{
    east_horizon, nlerp, step_with_accel, FlightStatus, OrbitElements, Vec2, Vessel, DT,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AscentPhase {
    #[default]
    Boost,
    Coast,
    Circ,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AutoPilot {
    pub enabled: bool,
    pub phase: AscentPhase,
}

impl AutoPilot {
    pub fn on() -> Self {
        Self {
            enabled: true,
            phase: AscentPhase::Boost,
        }
    }

    pub fn off() -> Self {
        Self {
            enabled: false,
            phase: AscentPhase::Boost,
        }
    }
}

pub struct PilotCommand {
    pub throttle: f64,
    pub facing: Vec2,
    pub stage: bool,
}

pub fn ascent1(
    phase: AscentPhase,
    t: f64,
    vessel: &Vessel,
    spec: &CraftSpec,
) -> (PilotCommand, AscentPhase) {
    let el = OrbitElements::of(vessel.r, vessel.v);
    let rhat = vessel.r.normalized();
    let vhat = if vessel.v.norm() < 1.0 {
        rhat
    } else {
        vessel.v.normalized()
    };
    let east = east_horizon(vessel.r);

    match phase {
        AscentPhase::Boost => {
            // Stay mostly radial so periapsis stays on the pad; a shallow
            // east bias gives the ellipse angular momentum without
            // circularizing during the boost (which would "win" at t≈8 s).
            let facing = if t < 2.0 {
                rhat
            } else if t < 6.0 {
                nlerp(rhat, east, 0.45 * (t - 2.0) / 4.0)
            } else {
                vhat
            };
            let cut = el.h_ap() >= 48.0 || spec.current_stage_fuel() <= 1e-9;
            if cut {
                (
                    PilotCommand {
                        throttle: 0.0,
                        facing: vhat,
                        stage: false,
                    },
                    AscentPhase::Coast,
                )
            } else {
                (
                    PilotCommand {
                        throttle: 1.0,
                        facing,
                        stage: false,
                    },
                    AscentPhase::Boost,
                )
            }
        }
        AscentPhase::Coast => {
            // Jettison the booster after cutoff even if a dribble of fuel remains.
            let stage = spec.current_stage_has_decoupler()
                && (spec.spent_engine_and_decoupler() || spec.current_stage_is_boost());
            let radial = vessel.r.dot(vessel.v) / vessel.r.norm().max(1e-12);
            // Surface period is ~28 s; circularize at first apoapsis (~9 s)
            // or the ellipse hits the ground on the way back.
            let next = if radial.abs() < 0.5 && el.h_ap() >= 24.0 {
                AscentPhase::Circ
            } else {
                AscentPhase::Coast
            };
            (
                PilotCommand {
                    throttle: 0.0,
                    facing: vhat,
                    stage,
                },
                next,
            )
        }
        AscentPhase::Circ => {
            let stage = spec.current_stage_is_boost() && spec.current_stage_has_decoupler();
            let throttle = if el.is_win() || el.energy >= 0.0 {
                0.0
            } else {
                1.0
            };
            (
                PilotCommand {
                    throttle,
                    facing: vhat,
                    stage,
                },
                AscentPhase::Circ,
            )
        }
    }
}

/// Gravity + current-stage thrust; fuel Euler-drained after the RK4 step.
pub fn powered_step(vessel: &mut Vessel, spec: &mut CraftSpec) {
    let throttle = vessel.throttle.clamp(0.0, 1.0);
    let live_fuel = vessel.fuel > 1e-12;
    let thrust_n = if live_fuel {
        throttle * spec.current_stage_thrust()
    } else {
        0.0
    };
    let facing = vessel.facing.normalized();
    let accel = if vessel.mass > 1e-9 {
        facing.scaled(thrust_n / vessel.mass)
    } else {
        Vec2::default()
    };
    step_with_accel(vessel, accel);
    let burned = if live_fuel {
        throttle * spec.current_stage_fuel_rate() * DT
    } else {
        0.0
    };
    spec.drain_fuel(burned);
    vessel.fuel = spec.fuel();
    vessel.mass = spec.wet_mass().max(1e-12);
}

pub fn mark_orbit_if_won(vessel: &mut Vessel, auto: &AutoPilot) {
    if auto.phase == AscentPhase::Circ
        && vessel.throttle <= 1e-9
        && vessel.status == FlightStatus::Flying
        && OrbitElements::of(vessel.r, vessel.v).is_win()
    {
        vessel.status = FlightStatus::Orbit;
    }
}

pub fn apply_pilot(
    vessel: &mut Vessel,
    spec: &mut CraftSpec,
    auto: &mut AutoPilot,
) -> Option<StageDrop> {
    if !auto.enabled {
        return None;
    }
    let (cmd, next) = ascent1(auto.phase, vessel.t, vessel, spec);
    auto.phase = next;
    vessel.throttle = cmd.throttle;
    vessel.facing = cmd.facing.normalized();
    if cmd.stage && spec.current_stage_has_decoupler() && !spec.parts.is_empty() {
        let drop = spec.stage();
        vessel.mass = spec.wet_mass().max(1e-12);
        vessel.fuel = spec.fuel();
        return Some(drop);
    }
    None
}

#[derive(Clone, Debug, Serialize)]
pub struct GoldenSample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub fuel: f64,
    pub e: f64,
    pub r_pe: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct GoldenFlight {
    pub win_t: Option<f64>,
    pub max_r: f64,
    pub status: FlightStatus,
    pub samples: Vec<GoldenSample>,
}

/// Deterministic Ascent-1 on Sounding Stick, warp 1, no UI.
pub fn fly_sounding_stick(max_t: f64) -> GoldenFlight {
    let mut spec = CraftSpec::sounding_stick();
    let mut vessel = Vessel::at_pad(spec.wet_mass(), spec.fuel());
    let mut auto = AutoPilot::on();
    let mut samples = Vec::new();
    let mut max_r = vessel.r.norm();
    let mut next_sample = 0.0;
    let mut win_t = None;

    while vessel.t < max_t
        && !matches!(vessel.status, FlightStatus::Crashed | FlightStatus::Escaped)
    {
        apply_pilot(&mut vessel, &mut spec, &mut auto);
        powered_step(&mut vessel, &mut spec);
        mark_orbit_if_won(&mut vessel, &auto);
        max_r = max_r.max(vessel.r.norm());
        if vessel.t + 1e-12 >= next_sample {
            let el = OrbitElements::of(vessel.r, vessel.v);
            samples.push(GoldenSample {
                t: vessel.t,
                x: vessel.r.x,
                y: vessel.r.y,
                vx: vessel.v.x,
                vy: vessel.v.y,
                fuel: vessel.fuel,
                e: el.e,
                r_pe: el.r_pe,
            });
            next_sample += 0.25;
        }
        if vessel.status == FlightStatus::Orbit && win_t.is_none() {
            win_t = Some(vessel.t);
            if vessel.throttle <= 1e-9 {
                break;
            }
        }
    }
    GoldenFlight {
        win_t,
        max_r,
        status: vessel.status,
        samples,
    }
}
