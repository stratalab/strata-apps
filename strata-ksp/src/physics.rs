//! Closed 2D Kepler physics: planet `kerb`, RK4, orbit elements, win/fail.
//!
//! Thrust and staging land in PR2/PR3. PR1 is ballistic (a ≡ gravity).

use serde::{Deserialize, Serialize};

/// Frozen planet document version. A mismatch with `./ksp-db` refuses boot.
pub const PLANET_VERSION: u32 = 2;
/// Vacuum win eccentricity cap.
pub const E_MAX: f64 = 0.15;
/// RK4 step (s).
pub const DT: f64 = 1.0 / 60.0;
/// Standard gravity (m/s²). This is the constant specific impulse is defined
/// against, not the surface gravity of wherever you happen to be launching
/// from, so a stack's delta-v does not change when you pick another world.
pub const G_STANDARD: f64 = 9.80665;

/// A world to launch from.
///
/// Radius and surface gravity set the orbit you are trying to reach; the air
/// sets how hard it is to get through the first hundred metres. An airless
/// body has `rho0` of zero and every aerodynamic term below folds to nothing,
/// which is the honest behaviour rather than a special case.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Planet {
    pub id: &'static str,
    pub name: &'static str,
    /// Radius (m).
    pub radius: f64,
    /// Surface gravity (m/s²).
    pub g0: f64,
    /// Air density at the surface (kg/m³). Zero for an airless body.
    pub rho0: f64,
    /// Density falls by 1/e every this many metres.
    pub scale_height: f64,
    pub blurb: &'static str,
}

impl Planet {
    /// µ = g₀R².
    pub fn mu(&self) -> f64 {
        self.g0 * self.radius * self.radius
    }

    pub fn r_pe_min(&self) -> f64 {
        self.radius + 24.0
    }

    pub fn r_escape(&self) -> f64 {
        20.0 * self.radius
    }

    /// Air density at an altitude above the surface.
    pub fn density(&self, altitude: f64) -> f64 {
        if self.rho0 <= 0.0 {
            return 0.0;
        }
        self.rho0 * (-altitude.max(0.0) / self.scale_height).exp()
    }

    /// Where the air has thinned to roughly a thousandth of sea level. Used
    /// for display and for deciding when aerodynamics stop mattering.
    pub fn atmosphere_top(&self) -> f64 {
        if self.rho0 <= 0.0 {
            0.0
        } else {
            self.scale_height * 7.0
        }
    }

    pub fn has_air(&self) -> bool {
        self.rho0 > 0.0
    }
}

pub const PLANETS: &[Planet] = &[
    Planet {
        id: "aeris",
        name: "Aeris",
        radius: 200.0,
        g0: 9.81,
        rho0: 0.10,
        scale_height: 10.0,
        blurb: "Air near the ground, thinning fast. Fins earn their mass here.",
    },
    Planet {
        id: "kerb",
        name: "Kerb",
        radius: 200.0,
        g0: 9.81,
        rho0: 0.0,
        scale_height: 1.0,
        blurb: "The same rock without the weather. Nothing to slow you down, and nothing for a fin to bite.",
    },
    Planet {
        id: "mun",
        name: "Mun",
        radius: 120.0,
        g0: 2.6,
        rho0: 0.0,
        scale_height: 1.0,
        blurb: "Airless and light. Orbit is cheap; landing is the hard part.",
    },
    Planet {
        id: "heave",
        name: "Heave",
        radius: 320.0,
        g0: 13.2,
        rho0: 0.30,
        scale_height: 16.0,
        blurb: "Heavy, and the air is worse. Bring thrust, and keep it pointed straight.",
    },
];

/// The world the frozen golden fixture was recorded in. Airless on purpose:
/// that test exists to catch integrator drift, and it cannot do that if the
/// trajectory also moves whenever the atmosphere is tuned.
pub const GOLDEN_PLANET: &str = "kerb";

pub fn planet_by_id(id: &str) -> Option<&'static Planet> {
    PLANETS.iter().find(|p| p.id == id)
}

/// The world this process is flying from.
///
/// A launch is only comparable with another launch from the same place, so
/// this is one setting for the session rather than a parameter threaded
/// through every equation. Changing it clears the pad; see `World::set_planet`.
static ACTIVE: std::sync::RwLock<Planet> = std::sync::RwLock::new(PLANETS[0]);

pub fn planet() -> Planet {
    *ACTIVE.read().expect("planet lock")
}

pub fn set_active_planet(p: Planet) {
    *ACTIVE.write().expect("planet lock") = p;
}

/// Pad position: +y north.
pub const PAD_X: f64 = 0.0;

pub fn pad_y() -> f64 {
    planet().radius
}

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
    /// Angle between where the vehicle points and where it is going, in
    /// radians. Zero is flying straight. A stable stack drives this toward
    /// zero; an unstable one lets it run away, which is what tumbling is.
    pub aoa: f64,
}

impl Vessel {
    pub fn at_pad(mass: f64, fuel: f64) -> Self {
        Self {
            t: 0.0,
            aoa: 0.0,
            r: Vec2::new(PAD_X, pad_y() + 0.05),
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
        let v_pe = (planet().mu() * (2.0 / r_pe - 1.0 / a)).sqrt();
        Self {
            t: 0.0,
            r: Vec2::new(0.0, r_pe),
            v: Vec2::new(v_pe, 0.0),
            mass: 1.0,
            fuel: 0.0,
            facing: Vec2::new(1.0, 0.0),
            throttle: 0.0,
            status: FlightStatus::Flying,
            aoa: 0.0,
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
        let mu = planet().mu();
        let energy = v2 / 2.0 - mu / rm;
        let e_x = (v.y * h) / mu - r.x / rm;
        let e_y = (-v.x * h) / mu - r.y / rm;
        let e = e_x.hypot(e_y);
        let (a, r_pe, r_ap) = if energy < 0.0 {
            let a = -mu / (2.0 * energy);
            (a, a * (1.0 - e), a * (1.0 + e))
        } else {
            let r_pe = if (1.0 + e).abs() > 1e-12 {
                (h * h) / (mu * (1.0 + e))
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
        self.r_pe - planet().radius
    }

    pub fn h_ap(&self) -> f64 {
        if self.r_ap.is_finite() {
            self.r_ap - planet().radius
        } else {
            f64::INFINITY
        }
    }

    pub fn is_win(&self) -> bool {
        self.energy < 0.0 && self.r_pe >= planet().r_pe_min() && self.e < E_MAX
    }
}

fn gravity(r: Vec2) -> Vec2 {
    let rm = r.norm();
    let s = -planet().mu() / (rm * rm * rm);
    r.scaled(s)
}

/// One RK4 step with gravity only (ballistic tests).
pub fn step(vessel: &mut Vessel) {
    step_with_accel(vessel, Vec2::default());
}

/// Dynamic pressure, ½ρv². The number that decides how much the air matters.
pub fn dynamic_pressure(r: Vec2, v: Vec2) -> f64 {
    let alt = r.norm() - planet().radius;
    0.5 * planet().density(alt) * v.dot(v)
}

/// RK4 with a thrust acceleration held constant over the step (facing snapped).
pub fn step_with_accel(vessel: &mut Vessel, thrust_accel: Vec2) {
    step_with_forces(vessel, thrust_accel, 0.0)
}

/// As above, with air.
///
/// Drag is inside the integrator rather than applied once per step, because
/// it depends on velocity and velocity is what the step is solving for. On an
/// airless world `drag_area` is irrelevant: density is zero and every term
/// below vanishes.
pub fn step_with_forces(vessel: &mut Vessel, thrust_accel: Vec2, drag_area: f64) {
    if matches!(vessel.status, FlightStatus::Crashed | FlightStatus::Escaped) {
        return;
    }
    let dt = DT;
    let r = vessel.r;
    let v = vessel.v;
    let world = planet();
    let mass = vessel.mass.max(1e-9);
    // A vehicle flying at an angle to its path presents more of itself to the
    // air. This is the whole reason a tumbling rocket stops climbing.
    let broadside = 1.0 + 4.0 * vessel.aoa.sin().powi(2);
    let cda = drag_area * broadside;

    let a = |pos: Vec2, vel: Vec2| {
        let mut acc = gravity(pos).add(thrust_accel);
        if cda > 0.0 && world.has_air() {
            let speed = vel.norm();
            if speed > 1e-9 {
                let alt = pos.norm() - world.radius;
                let f = 0.5 * world.density(alt) * speed * speed * cda;
                acc = acc.add(vel.scaled(-f / (mass * speed)));
            }
        }
        acc
    };

    let k1v = a(r, v);
    let k1r = v;
    let k2v = a(r.add(k1r.scaled(dt * 0.5)), v.add(k1v.scaled(dt * 0.5)));
    let k2r = v.add(k1v.scaled(dt * 0.5));
    let k3v = a(r.add(k2r.scaled(dt * 0.5)), v.add(k2v.scaled(dt * 0.5)));
    let k3r = v.add(k2v.scaled(dt * 0.5));
    let k4v = a(r.add(k3r.scaled(dt)), v.add(k3v.scaled(dt)));
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
    let world = planet();
    let rm = vessel.r.norm();
    if rm < world.radius {
        vessel.status = FlightStatus::Crashed;
        return;
    }
    if rm > world.r_escape() {
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
    let mu = planet().mu();
    let e_x = (v.y * el.h) / mu - r.x / rm;
    let e_y = (-v.x * el.h) / mu - r.y / rm;
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
    (planet().mu() / radius).sqrt()
}

pub fn period(semi_major: f64) -> f64 {
    std::f64::consts::TAU * (semi_major.powi(3) / planet().mu()).sqrt()
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
