//! Closed 2D Kepler physics: planet `kerb`, RK4, orbit elements, win/fail.
//!
//! Thrust and staging land in PR2/PR3. PR1 is ballistic (a ≡ gravity).

use serde::{Deserialize, Serialize};

/// Frozen planet document version. A mismatch with `./ksp-db` refuses boot.
pub const PLANET_VERSION: u32 = 1;
/// Planet radius (m). Frozen.
pub const R: f64 = 200.0;
/// Surface gravity (m/s²). Frozen.
pub const G0: f64 = 9.81;
/// Standard gravitational parameter µ = g₀ R² (m³/s²). Frozen.
pub const MU: f64 = G0 * R * R;
/// Vacuum win periapsis radius (m).
pub const R_PE_MIN: f64 = R + 24.0;
/// Vacuum win eccentricity cap.
pub const E_MAX: f64 = 0.15;
/// Escape cutoff |r| (m).
pub const R_ESCAPE: f64 = 20.0 * R;
/// RK4 step (s).
pub const DT: f64 = 1.0 / 60.0;
/// Pad position: +y north.
pub const PAD_X: f64 = 0.0;
pub const PAD_Y: f64 = R;

/// Global RK4 step budget per wall tick (round-robin across launches).
pub const MAX_STEPS_PER_WALL_TICK: u32 = 2000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn norm(self) -> f64 {
        self.x.hypot(self.y)
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    pub fn scaled(self, s: f64) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }

    pub fn normalized(self) -> Self {
        let n = self.norm();
        if n < 1e-12 {
            Self::new(0.0, 1.0)
        } else {
            self.scaled(1.0 / n)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightStatus {
    #[default]
    Flying,
    Orbit,
    Suborbital,
    Crashed,
    Escaped,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Vessel {
    pub t: f64,
    pub r: Vec2,
    pub v: Vec2,
    pub mass: f64,
    pub fuel: f64,
    pub facing: Vec2,
    pub throttle: f64,
    pub status: FlightStatus,
}

impl Vessel {
    pub fn at_pad(mass: f64, fuel: f64) -> Self {
        Self {
            t: 0.0,
            r: Vec2::new(PAD_X, PAD_Y + 0.05),
            v: Vec2::new(0.0, 0.0),
            mass,
            fuel,
            facing: Vec2::new(0.0, 1.0),
            throttle: 0.0,
            status: FlightStatus::Flying,
        }
    }

    /// Pad as periapsis of a lofted ellipse (r_pe = 208 m, r_ap = 280 m).
    ///
    /// Start 8 m above the lithosphere so RK4 cannot lithobrake on the first
    /// step of a surface-grazing throw.
    pub fn ballistic_throw() -> Self {
        let r_pe = 208.0;
        let r_ap = 280.0;
        let a = 0.5 * (r_pe + r_ap);
        let v_pe = (MU * (2.0 / r_pe - 1.0 / a)).sqrt();
        Self {
            t: 0.0,
            r: Vec2::new(0.0, r_pe),
            v: Vec2::new(v_pe, 0.0),
            mass: 1.0,
            fuel: 0.0,
            facing: Vec2::new(1.0, 0.0),
            throttle: 0.0,
            status: FlightStatus::Flying,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct OrbitElements {
    pub h: f64,
    pub energy: f64,
    pub e: f64,
    pub a: f64,
    pub r_pe: f64,
    pub r_ap: f64,
}

impl OrbitElements {
    pub fn of(r: Vec2, v: Vec2) -> Self {
        let rm = r.norm();
        let v2 = v.dot(v);
        let h = r.x * v.y - r.y * v.x;
        let energy = v2 / 2.0 - MU / rm;
        let e_x = (v.y * h) / MU - r.x / rm;
        let e_y = (-v.x * h) / MU - r.y / rm;
        let e = e_x.hypot(e_y);
        let (a, r_pe, r_ap) = if energy < 0.0 {
            let a = -MU / (2.0 * energy);
            (a, a * (1.0 - e), a * (1.0 + e))
        } else {
            let r_pe = if (1.0 + e).abs() > 1e-12 {
                (h * h) / (MU * (1.0 + e))
            } else {
                0.0
            };
            (f64::INFINITY, r_pe, f64::INFINITY)
        };
        Self {
            h,
            energy,
            e,
            a,
            r_pe,
            r_ap,
        }
    }

    pub fn h_pe(&self) -> f64 {
        self.r_pe - R
    }

    pub fn h_ap(&self) -> f64 {
        if self.r_ap.is_finite() {
            self.r_ap - R
        } else {
            f64::INFINITY
        }
    }

    pub fn is_win(&self) -> bool {
        self.energy < 0.0 && self.r_pe >= R_PE_MIN && self.e < E_MAX
    }
}

fn gravity(r: Vec2) -> Vec2 {
    let rm = r.norm();
    let s = -MU / (rm * rm * rm);
    r.scaled(s)
}

/// One RK4 step with gravity only (ballistic tests).
pub fn step(vessel: &mut Vessel) {
    step_with_accel(vessel, Vec2::default());
}

/// RK4 with a thrust acceleration held constant over the step (facing snapped).
pub fn step_with_accel(vessel: &mut Vessel, thrust_accel: Vec2) {
    if matches!(vessel.status, FlightStatus::Crashed | FlightStatus::Escaped) {
        return;
    }
    let dt = DT;
    let r = vessel.r;
    let v = vessel.v;
    let a = |pos: Vec2| gravity(pos).add(thrust_accel);

    let k1v = a(r);
    let k1r = v;
    let k2v = a(r.add(k1r.scaled(dt * 0.5)));
    let k2r = v.add(k1v.scaled(dt * 0.5));
    let k3v = a(r.add(k2r.scaled(dt * 0.5)));
    let k3r = v.add(k2v.scaled(dt * 0.5));
    let k4v = a(r.add(k3r.scaled(dt)));
    let k4r = v.add(k3v.scaled(dt));

    vessel.v = v.add(
        k1v.add(k2v.scaled(2.0))
            .add(k3v.scaled(2.0))
            .add(k4v)
            .scaled(dt / 6.0),
    );
    vessel.r = r.add(
        k1r.add(k2r.scaled(2.0))
            .add(k3r.scaled(2.0))
            .add(k4r)
            .scaled(dt / 6.0),
    );
    vessel.t += dt;

    apply_bounds(vessel);
}

fn apply_bounds(vessel: &mut Vessel) {
    let finite = vessel.r.x.is_finite()
        && vessel.r.y.is_finite()
        && vessel.v.x.is_finite()
        && vessel.v.y.is_finite()
        && vessel.mass.is_finite()
        && vessel.fuel.is_finite();
    if !finite || vessel.mass <= 0.0 {
        vessel.status = FlightStatus::Crashed;
        return;
    }
    let rm = vessel.r.norm();
    if rm < R {
        vessel.status = FlightStatus::Crashed;
        return;
    }
    if rm > R_ESCAPE {
        vessel.status = FlightStatus::Escaped;
        return;
    }
    if vessel.status == FlightStatus::Orbit {
        return;
    }
    if vessel.fuel <= 1e-12 && vessel.status == FlightStatus::Flying {
        let el = OrbitElements::of(vessel.r, vessel.v);
        if !el.is_win() {
            vessel.status = FlightStatus::Suborbital;
        }
    }
}

/// Sample the Kepler conic through `r,v` in inertial frame (for the dashed overlay).
pub fn predicted_conic(r: Vec2, v: Vec2, samples: usize) -> Vec<Vec2> {
    let el = OrbitElements::of(r, v);
    if !el.e.is_finite() || el.e >= 0.98 || !el.a.is_finite() || el.a <= 0.0 {
        return Vec::new();
    }
    let rm = r.norm();
    let e_x = (v.y * el.h) / MU - r.x / rm;
    let e_y = (-v.x * el.h) / MU - r.y / rm;
    let argp = e_y.atan2(e_x);
    let b = el.a * (1.0 - el.e * el.e).max(0.0).sqrt();
    let mut out = Vec::with_capacity(samples);
    for i in 0..samples {
        let e_anom = (i as f64) * std::f64::consts::TAU / (samples as f64);
        let pq_x = el.a * (e_anom.cos() - el.e);
        let pq_y = b * e_anom.sin();
        let c = argp.cos();
        let s = argp.sin();
        out.push(Vec2::new(c * pq_x - s * pq_y, s * pq_x + c * pq_y));
    }
    out
}

pub fn circular_velocity(radius: f64) -> f64 {
    (MU / radius).sqrt()
}

pub fn period(semi_major: f64) -> f64 {
    std::f64::consts::TAU * (semi_major.powi(3) / MU).sqrt()
}

/// East along the local horizon so +x is east at the pad.
pub fn east_horizon(r: Vec2) -> Vec2 {
    let rhat = r.normalized();
    Vec2::new(rhat.y, -rhat.x)
}

pub fn nlerp(a: Vec2, b: Vec2, t: f64) -> Vec2 {
    let t = t.clamp(0.0, 1.0);
    a.scaled(1.0 - t).add(b.scaled(t)).normalized()
}
