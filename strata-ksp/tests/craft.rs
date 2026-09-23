//! Staging mass and catalog. No Database.

use strata_ksp::craft::CraftSpec;

#[test]
fn sounding_stick_pad_mass_is_5_05() {
    let spec = CraftSpec::sounding_stick();
    assert!((spec.wet_mass() - 5.05).abs() < 1e-9);
    assert!((spec.dry_mass() - (5.05 - 1.80 - 0.90)).abs() < 1e-9);
}

#[test]
fn first_stage_drops_boost_and_leaves_2_30_kg() {
    let mut spec = CraftSpec::sounding_stick();
    let drop = spec.stage();
    assert_eq!(drop.ids, ["lvt-30", "tank-m", "stack-sep"]);
    assert!((drop.dropped_fuel - 1.80).abs() < 1e-9);
    assert!((spec.wet_mass() - 2.30).abs() < 1e-9);
}

#[test]
fn second_stage_leaves_the_pod() {
    let mut spec = CraftSpec::sounding_stick();
    spec.stage();
    let drop = spec.stage();
    assert_eq!(drop.ids, ["terrier", "tank-s", "stack-sep"]);
    assert!((spec.wet_mass() - 0.80).abs() < 1e-9);
    assert_eq!(spec.parts.len(), 1);
    assert_eq!(spec.parts[0].part_id, "mk1-pod");
}

#[test]
fn third_stage_on_pod_only_drops_the_pod() {
    let mut spec = CraftSpec::sounding_stick();
    spec.stage();
    spec.stage();
    let drop = spec.stage();
    assert_eq!(drop.ids, ["mk1-pod"]);
    assert!(spec.parts.is_empty());
    assert_eq!(spec.wet_mass(), 0.0);
}

#[test]
fn stack_without_decoupler_drops_everything() {
    let mut spec = CraftSpec::from_ids("bare", &["lvt-30", "tank-m", "mk1-pod"]).unwrap();
    let drop = spec.stage();
    assert_eq!(drop.ids, ["lvt-30", "tank-m", "mk1-pod"]);
    assert!(spec.parts.is_empty());
}

#[test]
fn add_tank_s_increases_fuel_cap_and_dry() {
    let mut spec = CraftSpec::sounding_stick();
    let dry0 = spec.dry_mass();
    let cap0 = spec.fuel_cap();
    spec.add("tank-s", Some(1)).unwrap();
    assert!((spec.dry_mass() - dry0 - 0.30).abs() < 1e-9);
    assert!((spec.fuel_cap() - cap0 - 0.90).abs() < 1e-9);
    assert_eq!(spec.parts[1].part_id, "tank-s");
}
