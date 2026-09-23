//! PR1 physics gates. No Database.

use strata_ksp::physics::{planet, step, step_with_accel, OrbitElements, Vec2, Vessel, DT, E_MAX};

fn circular_at(radius: f64) -> Vessel {
    let v = (planet().mu() / radius).sqrt();
    Vessel {
        t: 0.0,
        r: Vec2::new(radius, 0.0),
        v: Vec2::new(0.0, v),
        mass: 1.0,
        fuel: 0.0,
        facing: Vec2::new(0.0, 1.0),
        throttle: 0.0,
        status: strata_ksp::physics::FlightStatus::Flying,
        aoa: 0.0,
    }
}

fn period(a: f64) -> f64 {
    std::f64::consts::TAU * (a.powi(3) / planet().mu()).sqrt()
}

#[test]
fn circular_orbit_holds_for_two_periods() {
    let radius = 240.0;
    let mut vessel = circular_at(radius);
    let t_end = 2.0 * period(radius);
    let steps = (t_end / DT).ceil() as usize;
    for _ in 0..steps {
        step(&mut vessel);
        assert_ne!(
            vessel.status,
            strata_ksp::physics::FlightStatus::Crashed,
            "lithobrake at t={}",
            vessel.t
        );
    }
    let r = vessel.r.norm();
    assert!(
        (r - radius).abs() / radius < 0.01,
        "|r| drifted: {r} vs {radius}"
    );
    let e = OrbitElements::of(vessel.r, vessel.v).e;
    assert!(e < 0.02, "eccentricity {e}");
}

#[test]
fn coasting_ellipse_energy_drifts_less_than_1e_4() {
    let mut vessel = Vessel::ballistic_throw();
    let e0 = OrbitElements::of(vessel.r, vessel.v);
    assert!(e0.energy < 0.0, "throw should be bound");
    let t_end = period(e0.a);
    let steps = (t_end / DT).ceil() as usize;
    for _ in 0..steps {
        step(&mut vessel);
    }
    let e1 = OrbitElements::of(vessel.r, vessel.v);
    let rel = (e1.energy - e0.energy).abs() / e0.energy.abs();
    assert!(rel < 1e-4, "relative energy drift {rel}");
}

#[test]
fn prograde_dv_at_periapsis_raises_apoapsis() {
    let radius = 240.0;
    let mut vessel = circular_at(radius);
    let before = OrbitElements::of(vessel.r, vessel.v);
    let dv = 3.0;
    let speed = vessel.v.norm();
    vessel.v = vessel.v.scaled((speed + dv) / speed);
    let after = OrbitElements::of(vessel.r, vessel.v);
    // Vis-viva: ε' = ε + v dv + dv²/2; a' = -µ/(2ε')
    let v = speed;
    let energy_p = before.energy + v * dv + dv * dv / 2.0;
    let a_p = -planet().mu() / (2.0 * energy_p);
    let r = vessel.r.norm();
    let r_ap_pred = 2.0 * a_p - r;
    let err = (after.r_ap - r_ap_pred).abs() / r_ap_pred;
    assert!(after.r_ap > before.r_ap, "apoapsis should rise");
    assert!(err < 0.05, "apoapsis prediction error {err}");
}

#[test]
fn lithobrake_on_next_step_below_radius() {
    let mut vessel = circular_at(240.0);
    vessel.r = Vec2::new(planet().radius - 0.01, 0.0);
    vessel.v = Vec2::new(0.0, 0.0);
    step(&mut vessel);
    assert_eq!(vessel.status, strata_ksp::physics::FlightStatus::Crashed);
}

#[test]
fn win_predicate_near_circular_240() {
    let v = circular_at(240.0);
    let el = OrbitElements::of(v.r, v.v);
    assert!(el.is_win(), "r=240 circular should win");
    assert!(el.r_pe >= planet().r_pe_min());
    assert!(el.e < E_MAX);
}

#[test]
fn pad_rest_is_not_win() {
    let v = Vessel::at_pad(1.0, 0.0);
    let el = OrbitElements::of(v.r, v.v);
    assert!(!el.is_win());
    assert!(
        el.r_pe < planet().radius
            || el.energy >= 0.0
            || el.e >= E_MAX
            || el.r_pe < planet().r_pe_min()
    );
}

#[test]
fn prograde_thrust_raises_energy() {
    let mut vessel = circular_at(240.0);
    let e0 = OrbitElements::of(vessel.r, vessel.v).energy;
    let prograde = vessel.v.normalized();
    for _ in 0..60 {
        step_with_accel(&mut vessel, prograde.scaled(2.0));
    }
    let e1 = OrbitElements::of(vessel.r, vessel.v).energy;
    assert!(e1 > e0, "energy {e1} vs {e0}");
}
