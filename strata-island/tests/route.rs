//! Integer Dijkstra on the 8×8 RAM grid. No Database.

use strata_island::drive::{DriveEdge, DriveIndex};
use strata_island::extract::ClosureEdge;
use strata_island::route;
use strata_island::world::World;

fn idx(index: &DriveIndex, id: &str) -> usize {
    index.node_index(id).unwrap_or_else(|| panic!("{id}"))
}

fn drop_pairs(index: &DriveIndex, pairs: &[(&str, &str)]) -> DriveIndex {
    let mut cut = index.clone();
    for (src, outgoing) in cut.outgoing.iter_mut().enumerate() {
        let src_id = &index.node_ids[src];
        outgoing.retain(|edge| {
            let dst_id = &index.node_ids[edge.dst];
            !pairs
                .iter()
                .any(|(s, d)| *s == src_id.as_str() && *d == dst_id.as_str())
        });
    }
    cut
}

fn drop_closed(index: &DriveIndex, closed: &[ClosureEdge]) -> DriveIndex {
    let pairs: Vec<(&str, &str)> = closed
        .iter()
        .map(|edge| (edge.src.as_str(), edge.dst.as_str()))
        .collect();
    drop_pairs(index, &pairs)
}

#[test]
fn manhattan_distance_on_two_way_grid() {
    let world = World::open_ram_grid().expect("grid");
    let city = world.city_index();
    let path = world
        .route(idx(city, "n:g0_0"), idx(city, "n:g3_4"))
        .expect("path");
    assert_eq!(path.length_m, 700);
    assert_eq!(path.nodes.first().map(String::as_str), Some("n:g0_0"));
    assert_eq!(path.nodes.last().map(String::as_str), Some("n:g3_4"));
}

#[test]
fn one_way_forces_a_detour() {
    let world = World::open_ram_grid().expect("grid");
    let city = world.city_index();
    let two_way = world
        .route(idx(city, "n:g1_0"), idx(city, "n:g0_0"))
        .expect("two-way");
    assert_eq!(two_way.length_m, 100);

    let one_way = drop_pairs(city, &[("n:g1_0", "n:g0_0")]);
    let detour =
        route::route(&one_way, idx(&one_way, "n:g1_0"), idx(&one_way, "n:g0_0")).expect("detour");
    assert_eq!(detour.length_m, 300);
    assert!(detour.length_m > two_way.length_m);
}

#[test]
fn unreachable_after_a_cut() {
    let world = World::open_ram_grid().expect("grid");
    let city = world.city_index();
    let isolated = "n:g7_7";
    let pairs: Vec<(String, String)> = city
        .outgoing
        .iter()
        .enumerate()
        .flat_map(|(src, edges)| {
            let src_id = city.node_ids[src].clone();
            edges.iter().filter_map(move |edge| {
                let dst_id = city.node_ids[edge.dst].clone();
                if src_id == isolated || dst_id == isolated {
                    Some((src_id.clone(), dst_id))
                } else {
                    None
                }
            })
        })
        .collect();
    let refs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(s, d)| (s.as_str(), d.as_str()))
        .collect();
    let cut = drop_pairs(city, &refs);
    let err = route::route(&cut, idx(&cut, "n:g0_0"), idx(&cut, isolated)).expect_err("cut");
    assert_eq!(err.code, "failed_precondition.island.unreachable");
    assert_eq!(err.class(), "failed_precondition");
}

#[test]
fn forty_second_analog_parent_uses_closed_child_detours() {
    let world = World::open_ram_grid().expect("grid");
    let city = world.city_index();
    let src = idx(city, "n:g3_4");
    let dst = idx(city, "n:g6_4");
    let parent = world.route(src, dst).expect("parent");
    assert_eq!(parent.length_m, 300);
    assert!(world.uses_closed(&parent));

    let child_index = drop_closed(city, &world.closure().edges);
    let child = route::route(&child_index, src, dst).expect("child");
    assert!(!route::uses_closed(&child, &world.closure().edges));
    assert!(child.length_m >= parent.length_m);
}

#[test]
fn tie_break_is_lowest_predecessor_index() {
    let index = DriveIndex {
        branch: "city".into(),
        node_ids: vec!["a".into(), "b".into(), "c".into(), "d".into()],
        xy: vec![(0, 0), (10, 0), (0, 10), (10, 10)],
        outgoing: vec![
            vec![
                DriveEdge {
                    dst: 1,
                    length_m: 10,
                    name: None,
                },
                DriveEdge {
                    dst: 2,
                    length_m: 1,
                    name: None,
                },
            ],
            vec![DriveEdge {
                dst: 3,
                length_m: 1,
                name: None,
            }],
            vec![DriveEdge {
                dst: 3,
                length_m: 10,
                name: None,
            }],
            vec![],
        ],
    };
    let path = route::route(&index, 0, 3).expect("tie");
    assert_eq!(path.length_m, 11);
    assert_eq!(path.nodes, ["a", "b", "d"]);
}

#[test]
fn tie_break_is_deterministic_on_the_grid() {
    let world = World::open_ram_grid().expect("grid");
    let city = world.city_index();
    let src = idx(city, "n:g0_0");
    let dst = idx(city, "n:g1_1");
    let a = world.route(src, dst).expect("a");
    let b = world.route(src, dst).expect("b");
    assert_eq!(a.nodes, b.nodes);
    assert_eq!(a.nodes, ["n:g0_0", "n:g0_1", "n:g1_1"]);
    assert_eq!(a.length_m, 200);
}

#[test]
fn same_node_is_zero_length() {
    let world = World::open_ram_grid().expect("grid");
    let path = world.route(0, 0).expect("zero");
    assert_eq!(path.length_m, 0);
    assert_eq!(path.nodes.len(), 1);
}

#[test]
fn grid_talk_pair_by_poi_id() {
    let world = World::open_ram_grid().expect("grid");
    let src = world
        .lookup_node("poi:port-authority")
        .expect("port authority");
    let dst = world
        .lookup_node("poi:grand-central")
        .expect("grand central");
    let path = world.route(src, dst).expect("talk");
    assert_eq!(path.length_m, 300);
    assert!(world.uses_closed(&path));
    let err = world.lookup_node("poi:nope").expect_err("missing");
    assert_eq!(err.code, "not_found.island.node");
}
