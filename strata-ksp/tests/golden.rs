//! Ascent-1 on Sounding Stick. Fixture is written on first settle (PR3).

use serde::Deserialize;
use std::path::PathBuf;
use strata_ksp::ascent::fly_sounding_stick;
use strata_ksp::physics::R;

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
        flight.max_r < 8.0 * R,
        "max |r| {} exceeded 8R",
        flight.max_r
    );
}

#[test]
fn golden_fixture_relative_drift() {
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
