//! Ascent-1 on Sounding Stick. Fixture is written on first settle (PR3).

use serde::Deserialize;
use std::path::PathBuf;
use strata_ksp::ascent::fly_sounding_stick;
use strata_ksp::craft::CraftSpec;
use strata_ksp::physics::{planet, planet_by_id, set_active_planet, Vec2, Vessel, GOLDEN_PLANET};

/// The active world is one setting for the process, and cargo runs these
/// tests in parallel, so anything that sets it has to hold this first or two
/// tests will fly each other's planet. The lock is the price of not threading
/// a planet through every equation; it is paid here rather than everywhere.
static WORLD: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn world_guard() -> std::sync::MutexGuard<'static, ()> {
    WORLD.lock().unwrap_or_else(|e| e.into_inner())
}

/// The fixture is a vacuum flight. Pin the world so tuning the atmosphere on
/// the worlds people actually fly cannot silently rewrite what this asserts.
fn pin_golden_world() {
    set_active_planet(*planet_by_id(GOLDEN_PLANET).expect("golden planet"));
}

#[derive(Deserialize)]
struct Fixture {
    samples: Vec<Sample>,
}

#[derive(Deserialize)]
struct Sample {
    t: f64,
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    fuel: f64,
    e: f64,
    r_pe: f64,
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/sounding-stick-ascent1.json")
}

#[test]
fn sounding_stick_reaches_orbit_in_window() {
    let _world = world_guard();
    pin_golden_world();
    let flight = fly_sounding_stick(60.0);
    if std::env::var("GENERATE_GOLDEN").is_ok() {
        let path = fixture_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body = serde_json::json!({ "samples": flight.samples });
        std::fs::write(&path, serde_json::to_string_pretty(&body).unwrap()).unwrap();
        eprintln!("wrote {}", path.display());
    }
    let win_t = flight.win_t.expect("Ascent-1 should reach orbit");
    assert!(
        (5.0..45.0).contains(&win_t),
        "win at {win_t} s, expected 5..45 (toy kerb circularizes at first apoapsis)"
    );
    assert!(
        flight.max_r < 8.0 * planet().radius,
        "max |r| {} exceeded 8R",
        flight.max_r
    );
}

#[test]
fn golden_fixture_relative_drift() {
    let _world = world_guard();
    pin_golden_world();
    let path = fixture_path();
    assert!(
        path.exists(),
        "PR9 freeze: golden fixture missing at {} — GENERATE_GOLDEN=1 is a last resort, not a skip",
        path.display()
    );
    let raw = std::fs::read_to_string(&path).expect("read fixture");
    let fixture: Fixture = serde_json::from_str(&raw).expect("parse fixture");
    let flight = fly_sounding_stick(60.0);
    assert_eq!(
        flight.samples.len(),
        fixture.samples.len(),
        "sample count drifted"
    );
    for (got, exp) in flight.samples.iter().zip(fixture.samples.iter()) {
        for (name, a, b) in [
            ("t", got.t, exp.t),
            ("x", got.x, exp.x),
            ("y", got.y, exp.y),
            ("vx", got.vx, exp.vx),
            ("vy", got.vy, exp.vy),
            ("fuel", got.fuel, exp.fuel),
            ("e", got.e, exp.e),
            ("r_pe", got.r_pe, exp.r_pe),
        ] {
            let denom = b.abs().max(1.0);
            let rel = (a - b).abs() / denom;
            assert!(rel <= 1e-3, "{name} rel drift {rel} at t={}", got.t);
        }
    }
}

/// The golden flight is pinned to an airless world, which leaves the world
/// people actually launch from covered by nothing. This is that cover: the
/// stock stack has to still make orbit through the default atmosphere.
///
/// It is a range rather than a fixture because the point is that the default
/// world is flyable, not that its trajectory never moves.
#[test]
fn stock_stack_still_reaches_orbit_through_the_default_air() {
    let _world = world_guard();
    set_active_planet(strata_ksp::physics::PLANETS[0]);
    assert!(
        strata_ksp::physics::planet().has_air(),
        "the default world is supposed to have an atmosphere"
    );
    let flight = fly_sounding_stick(90.0);
    let win_t = flight
        .win_t
        .expect("the stock stack should still reach orbit with air in the way");
    assert!(
        (5.0..60.0).contains(&win_t),
        "win at {win_t} s through the atmosphere, expected 5..60"
    );
}

/// Drag decelerates, measured on the integrator rather than on a flight.
///
/// The obvious version of this test - fly the stock stack with and without
/// air and compare how high it got - is wrong, and measuring said so: the air
/// flight reached 253.36 against the vacuum flight's 253.18. The ascent
/// autopilot steers on state, so adding drag changes where it points as well
/// as what it loses, and over a fixed window those can cancel or invert. The
/// claim worth making is about the force, so this makes it about the force:
/// same vessel, same step, air or not.
#[test]
fn drag_takes_speed_out_of_a_vessel() {
    let _world = world_guard();
    let air = strata_ksp::physics::PLANETS[0];
    assert!(air.has_air());
    set_active_planet(air);

    let make = || {
        let mut v = Vessel::at_pad(5.0, 2.0);
        // Low and fast, which is where the air is worth anything.
        v.r = Vec2::new(0.0, air.radius + 5.0);
        v.v = Vec2::new(40.0, 0.0);
        v
    };

    let mut coasting = make();
    let mut dragging = make();
    for _ in 0..60 {
        strata_ksp::physics::step_with_forces(&mut coasting, Vec2::default(), 0.0);
        strata_ksp::physics::step_with_forces(&mut dragging, Vec2::default(), 0.05);
    }
    assert!(
        dragging.v.norm() < coasting.v.norm(),
        "with drag {} vs without {}",
        dragging.v.norm(),
        coasting.v.norm()
    );

    // And on an airless world the same drag area is inert.
    set_active_planet(*planet_by_id("mun").expect("mun"));
    let mut a = make();
    let mut b = make();
    for _ in 0..60 {
        strata_ksp::physics::step_with_forces(&mut a, Vec2::default(), 0.0);
        strata_ksp::physics::step_with_forces(&mut b, Vec2::default(), 0.05);
    }
    assert_eq!(
        a.v.norm(),
        b.v.norm(),
        "drag area did something without air"
    );
}

/// Fins are the difference between a dart and a tumbling brick.
#[test]
fn fins_make_a_stack_stable() {
    let bare = CraftSpec::sounding_stick();
    let mut finned = CraftSpec::sounding_stick();
    finned.add("fin", Some(0)).expect("fin at the base");
    assert!(
        finned.stability_margin() > bare.stability_margin(),
        "a fin at the base should move the centre of pressure aft: {} -> {}",
        bare.stability_margin(),
        finned.stability_margin()
    );
    assert!(
        finned.stability_margin() > 0.0,
        "one fin should be enough to make the stock stack stable, got {}",
        finned.stability_margin()
    );
}

/// Where you put the fins decides whether they save the flight or end it.
///
/// Measured, not asserted from theory: on the default world the stock stack
/// reaches orbit at a margin of -0.05, three fins at the base take it to
/// +3.37 and it climbs past a kilometre, and the same three fins on the nose
/// take it to -4.03 and it tumbles to 91 degrees and hits the ground.
#[test]
fn fins_at_the_nose_are_worse_than_no_fins() {
    let base = CraftSpec::sounding_stick().stability_margin();

    let mut tail = CraftSpec::sounding_stick();
    for _ in 0..3 {
        tail.add("fin", Some(0)).expect("fin at the base");
    }

    let mut nose = CraftSpec::sounding_stick();
    for _ in 0..3 {
        let top = nose.parts.len();
        nose.add("fin", Some(top)).expect("fin at the nose");
    }

    assert!(
        tail.stability_margin() > base && base > nose.stability_margin(),
        "base {base}, tail {}, nose {}",
        tail.stability_margin(),
        nose.stability_margin()
    );
    assert!(
        tail.stability_margin() > 0.0,
        "fins at the base should stabilise"
    );
    assert!(
        nose.stability_margin() < 0.0,
        "fins at the nose should destabilise"
    );
}

/// An unstable stack in air ends up pointing somewhere other than where it is
/// going; the same stack in vacuum has nothing to push it around.
#[test]
fn only_air_can_tumble_a_rocket() {
    let mut nose_heavy = CraftSpec::sounding_stick();
    for _ in 0..3 {
        let top = nose_heavy.parts.len();
        nose_heavy.add("fin", Some(top)).expect("fin");
    }
    assert!(nose_heavy.stability_margin() < 0.0);

    let _world = world_guard();
    let fly = |id: &str| {
        set_active_planet(*planet_by_id(id).expect("planet"));
        let mut spec = nose_heavy.clone();
        let mut v = Vessel::at_pad(spec.wet_mass(), spec.fuel());
        v.throttle = 1.0;
        for _ in 0..600 {
            strata_ksp::ascent::powered_step(&mut v, &mut spec);
        }
        v.aoa.abs()
    };
    let in_air = fly("aeris");
    let in_vacuum = fly("kerb");
    assert!(in_vacuum < 1e-9, "vacuum tumbled to {in_vacuum} rad");
    assert!(in_air > in_vacuum, "air {in_air} vs vacuum {in_vacuum}");
}
