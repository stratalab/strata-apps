//! Integer geo: origin, snap, AABB. No Database.

use strata_island::geo::{
    self, AABB_X_MAX, AABB_X_MIN, AABB_Y_MAX, AABB_Y_MIN, ORIGIN_LAT_E4, ORIGIN_LON_E4, SNAP_R2,
};
use strata_island::world::World;

#[test]
fn origin_constants_match_battery() {
    assert_eq!(ORIGIN_LAT_E4, 407_003);
    assert_eq!(ORIGIN_LON_E4, -740_170);
}

#[test]
fn integer_euclidean_3_4_is_25() {
    assert_eq!(geo::dist2(0, 0, 3, 4), 25);
}

#[test]
fn snap_inside_80m_hits() {
    let xy = [(0, 0)];
    assert_eq!(geo::snap(&xy, 0, 0).expect("hit"), 0);
    assert_eq!(geo::snap(&xy, 80, 0).expect("edge"), 0);
}

#[test]
fn snap_81m_is_invalid_argument_island_snap() {
    let xy = [(0, 0)];
    let err = geo::snap(&xy, 81, 0).expect_err("too far");
    assert_eq!(err.code, "invalid_argument.island.snap");
    assert_eq!(err.class(), "invalid_argument");
    let _ = SNAP_R2;
}

#[test]
fn aabb_cull() {
    assert!(geo::in_aabb(0, 0));
    assert!(geo::in_aabb(AABB_X_MIN, AABB_Y_MIN));
    assert!(geo::in_aabb(AABB_X_MAX, AABB_Y_MAX));
    assert!(!geo::in_aabb(AABB_X_MIN - 1, 0));
    assert!(!geo::in_aabb(0, AABB_Y_MAX + 1));
}

#[test]
fn geo_and_route_are_integer_only() {
    let geo_src = include_str!("../src/geo.rs");
    let route_src = include_str!("../src/route.rs");
    for (name, src) in [("geo.rs", geo_src), ("route.rs", route_src)] {
        assert!(!src.contains("f64"), "{name} must not contain f64");
        assert!(!src.contains("NaN"), "{name} must not contain NaN");
        assert!(!src.contains("inf"), "{name} must not contain inf");
    }
}

#[test]
fn world_loads_8x8_without_database() {
    let world = World::open_ram_grid().expect("grid");
    let city = world.city_index();
    assert_eq!(city.node_ids.len(), 64);
    let edges: usize = city.outgoing.iter().map(Vec::len).sum();
    assert_eq!(edges, 224);
    assert_eq!(world.gazetteer().len(), 6);
    assert_eq!(world.closure().edges.len(), 6);
    let path = world.route(0, 1).expect("adjacent");
    assert_eq!(path.length_m, 100);
}

#[test]
fn snap_on_grid_corner() {
    let world = World::open_ram_grid().expect("grid");
    assert_eq!(world.snap_xy(0, 0).expect("origin"), 0);
    let err = world.snap_xy(10_000, 10_000).expect_err("far");
    assert_eq!(err.code, "invalid_argument.island.snap");
}
