//! PR4: VAB JSON + craft graph. Cache in-process; durable tempfile resume.

use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::json;
use strata_ksp::craft::CraftSpec;
use strata_ksp::store::{self, BRANCH_DEFAULT, DOC_CATALOG, DOC_PLANET};
use strata_ksp::world::{OpenArgs, World};

fn cache_world() -> World {
    World::open(OpenArgs {
        cache: true,
        db_path: PathBuf::from("cache"),
    })
    .expect("open cache")
}

fn durable_world(path: &std::path::Path) -> World {
    World::open(OpenArgs {
        cache: false,
        db_path: path.to_path_buf(),
    })
    .expect("open durable")
}

#[test]
fn cache_bootstrap_writes_default_and_vab() {
    let mut db = store::open_cache().expect("cache");
    let outcome = store::ensure_world_seed(&mut db).expect("seed");
    assert!(!outcome.resumed);
    let names: HashSet<String> = store::list_product_branches(&mut db)
        .expect("list")
        .into_iter()
        .collect();
    assert!(names.contains("default"));
    assert!(names.contains("vab"));
    assert_eq!(outcome.spec.name, "Sounding Stick");
    assert_eq!(outcome.spec.parts.len(), 7);

    let planet = store::read_json_doc(&mut db, BRANCH_DEFAULT, DOC_PLANET)
        .expect("planet")
        .expect("planet present");
    assert!(store::planet_matches(&planet));
    let catalog = store::read_json_doc(&mut db, BRANCH_DEFAULT, DOC_CATALOG)
        .expect("catalog")
        .expect("catalog present");
    assert!(store::catalog_matches(&catalog));
}

#[test]
fn cache_vab_save_round_trips_json_and_graph() {
    let world = cache_world();
    assert_eq!(world.vab_spec().parts.len(), 7);

    world.vab_add("tank-s", None).expect("add tank-s");
    let spec = world.vab_spec();
    assert_eq!(
        spec.parts.last().map(|p| p.part_id.as_str()),
        Some("tank-s")
    );
    assert_eq!(spec.parts.len(), 8);

    let snap = world.snapshot();
    assert!(snap.commits_this_persist >= 3);
    assert!(snap.total_commits >= 3);
    assert_eq!(snap.branch_count, 2);
    assert!(!snap.durable);
    assert_eq!(snap.vab.parts.len(), 8);
    assert_eq!(
        snap.vab.parts.last().map(|p| p.part_id.as_str()),
        Some("tank-s")
    );

    let nodes: HashSet<String> = world.craft_node_ids().expect("nodes").into_iter().collect();
    assert!(nodes.contains("p-00-lvt-30"));
    assert!(nodes.contains("p-06-mk1-pod"));
    assert!(nodes.contains("p-07-tank-s"));
    assert!(!nodes.contains("p-07-mk1-pod"));
}

#[test]
fn cache_rebuild_drops_removed_nodes() {
    let world = cache_world();
    world.vab_add("fin", None).expect("add fin");
    let with_fin: HashSet<String> = world.craft_node_ids().expect("nodes").into_iter().collect();
    assert!(with_fin.contains("p-07-fin"));

    world.vab_remove(7).expect("remove fin");
    let after: HashSet<String> = world.craft_node_ids().expect("nodes").into_iter().collect();
    assert!(!after.contains("p-07-fin"));
    assert_eq!(world.vab_spec().parts.len(), 7);
}

#[test]
fn durable_resume_keeps_vab_stack() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let world = durable_world(dir.path());
        world.vab_add("fin", None).expect("add fin");
        world.vab_save().expect("save");
        assert_eq!(
            world.vab_spec().parts.last().map(|p| p.part_id.as_str()),
            Some("fin")
        );
        assert!(world.snapshot().durable);
    }
    let world = durable_world(dir.path());
    let spec = world.vab_spec();
    assert_eq!(spec.parts.len(), 8);
    assert_eq!(spec.parts.last().map(|p| p.part_id.as_str()), Some("fin"));
    let nodes: HashSet<String> = world.craft_node_ids().expect("nodes").into_iter().collect();
    assert!(nodes.contains("p-07-fin"));
    let snap = world.snapshot();
    assert!(snap
        .findings
        .iter()
        .any(|f| f.title.contains("Resumed hangar")));
}

#[test]
fn catalog_version_mismatch_refuses_boot() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let _world = durable_world(dir.path());
    }
    {
        let mut db = store::open_local(dir.path()).expect("reopen");
        store::write_json_doc(
            &mut db,
            BRANCH_DEFAULT,
            DOC_CATALOG,
            json!({
                "version": 99,
                "hash": "nope",
                "parts": []
            }),
        )
        .expect("tamper catalog");
    }
    let err = match World::open(OpenArgs {
        cache: false,
        db_path: dir.path().to_path_buf(),
    }) {
        Ok(_) => panic!("expected catalog version mismatch"),
        Err(err) => err,
    };
    assert!(
        err.contains("failed_precondition.ksp.catalog_version"),
        "got {err}"
    );
}

#[test]
fn planet_version_mismatch_refuses_boot() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let _world = durable_world(dir.path());
    }
    {
        let mut db = store::open_local(dir.path()).expect("reopen");
        store::write_json_doc(
            &mut db,
            BRANCH_DEFAULT,
            DOC_PLANET,
            json!({
                "name": "kerb",
                "version": 2,
                "hash": "nope",
                "R": 200.0
            }),
        )
        .expect("tamper planet");
    }
    let err = match World::open(OpenArgs {
        cache: false,
        db_path: dir.path().to_path_buf(),
    }) {
        Ok(_) => panic!("expected planet version mismatch"),
        Err(err) => err,
    };
    assert!(
        err.contains("failed_precondition.ksp.planet_version"),
        "got {err}"
    );
}

#[test]
fn messy_physics_floats_roundtrip_on_the_event_tape() {
    use strata_ksp::ascent::{powered_step, AscentPhase};
    use strata_ksp::physics::Vessel;
    use strata_ksp::store::{self, BRANCH_VAB};
    use strata_ksp::telemetry::VesselSample;

    let mut db = store::open_cache().expect("cache");
    store::ensure_world_seed(&mut db).expect("seed");
    store::fork_current(&mut db, BRANCH_VAB, "launch-0001").expect("fork");
    let mut spec = CraftSpec::sounding_stick();
    let mut vessel = Vessel::at_pad(spec.wet_mass(), spec.fuel());
    for _ in 0..4 {
        powered_step(&mut vessel, &mut spec);
    }
    let sample = VesselSample::from_ship(
        "launch-0001",
        "vab",
        "design-0001",
        &vessel,
        0,
        true,
        AscentPhase::Boost,
        1,
    );
    store::persist_tick(&mut db, "launch-0001", &sample).expect("tick");
    let events = store::range_events(&mut db, "launch-0001").expect("range");
    assert!(events.iter().any(|e| e.event_type().as_str() == "tick"));
}

#[test]
fn cache_launch_persists_ticks_and_verify_chain() {
    let world = cache_world();
    world.set_persist_wall_cap(false);
    let name = world.launch_from_pad().expect("launch");
    assert_eq!(name, "launch-0001");
    for _ in 0..500 {
        world.tick_frame();
    }
    world.flush_durable(&name).expect("flush");
    let kinds = world.event_kinds(&name).expect("kinds");
    let ticks = kinds.iter().filter(|k| *k == "tick").count();
    assert!(
        ticks >= 30,
        "expected >= 30 tick events, got {ticks} (kinds {})",
        kinds.len()
    );
    assert!(kinds.iter().any(|k| k == "launch"));
    assert!(world.verify_chain(&name).expect("verify launch"));
    assert!(world.verify_chain("vab").expect("verify vab"));
    let snap = world.snapshot();
    assert_eq!(snap.launches.len(), 1);
    assert_eq!(snap.focused, name);
    assert!(snap.launches[0].seq >= 30);
    assert!(snap.launches[0].trail.len() >= 30);
    assert!(snap.total_commits >= 30);
}

#[test]
fn durable_resume_mid_coast_keeps_t_and_trail() {
    let dir = tempfile::tempdir().expect("tempdir");
    let t1;
    let seq1;
    {
        let world = durable_world(dir.path());
        world.set_persist_wall_cap(false);
        let name = world.launch_from_pad().expect("launch");
        for _ in 0..240 {
            world.tick_frame();
        }
        world.flush_durable(&name).expect("flush");
        let snap = world.snapshot();
        t1 = snap.launches[0].t;
        seq1 = snap.launches[0].seq;
        assert!(t1 > 1.0, "sim t {t1}");
        assert!(seq1 >= 5, "seq {seq1}");
        assert!(!snap.launches[0].trail.is_empty());
    }
    let world = durable_world(dir.path());
    let snap = world.snapshot();
    assert_eq!(snap.launches.len(), 1);
    assert_eq!(snap.focused, "launch-0001");
    assert!(
        (snap.launches[0].t - t1).abs() < 0.5,
        "resume t {} vs {t1}",
        snap.launches[0].t
    );
    assert!(snap.launches[0].trail.len() >= 5);
    assert!(world.verify_chain("launch-0001").expect("verify"));
}

/// Eight live launches, and a ninth evicts rather than refuses.
///
/// This used to assert that the ninth call failed. It does not any more:
/// Launch is the only verb the app is built around and walling it off at
/// attempt nine is a dead end for the person using it. The cap still holds -
/// what changed is which launch gives way. The archived one keeps its events;
/// it is only no longer live.
#[test]
fn ninth_launch_archives_the_oldest_rather_than_refusing() {
    let world = cache_world();
    world.set_persist_wall_cap(false);
    for i in 0..8 {
        let name = world.launch_from_pad().expect("launch");
        assert_eq!(name, format!("launch-{:04}", i + 1));
    }
    assert_eq!(world.snapshot().launches.len(), 8);

    let ninth = world.launch_from_pad().expect("ninth launch");
    assert_eq!(ninth, "launch-0009");

    let snap = world.snapshot();
    assert_eq!(snap.launches.len(), 8, "the cap still holds");
    assert_eq!(snap.focused, "launch-0009");
    let names: Vec<&str> = snap.launches.iter().map(|l| l.name.as_str()).collect();
    assert!(
        !names.contains(&"launch-0001"),
        "oldest gave way: {names:?}"
    );
    assert!(names.contains(&"launch-0009"));
}

#[test]
fn sounding_stick_doc_round_trip() {
    let spec = CraftSpec::sounding_stick();
    let doc = spec.to_doc();
    let back = CraftSpec::from_doc(&doc).expect("from_doc");
    assert_eq!(spec, back);
    assert_eq!(doc["name"].as_str(), Some("Sounding Stick"));
    assert_eq!(doc["parts"].as_array().map(|a| a.len()), Some(7));
}

fn first_seq(world: &World, launch: &str, kind: &str) -> Option<u64> {
    world
        .event_log(launch)
        .ok()?
        .into_iter()
        .find(|(_, k)| k == kind)
        .map(|(seq, _)| seq)
}

fn tick_until_kind(world: &World, launch: &str, kind: &str, frames: u32) -> u64 {
    for _ in 0..frames {
        world.tick_frame();
        world.flush_durable(launch).expect("flush");
        if let Some(seq) = first_seq(world, launch, kind) {
            return seq;
        }
    }
    panic!("never saw {kind} on {launch}");
}

#[test]
fn fork_at_pre_stage_child_is_prefix_parent_keeps_future() {
    let world = cache_world();
    world.set_persist_wall_cap(false);
    let parent = world.launch_from_pad().expect("launch");
    let stage_seq = tick_until_kind(&world, &parent, "stage", 800);
    assert!(stage_seq > 2, "stage seq {stage_seq}");
    let pre = stage_seq - 1;
    let parent_len = world.event_len(&parent).expect("len");
    let design = world.snapshot().launches[0].design.clone();

    let child = world.fork_at(&parent, Some(pre)).expect("fork");
    assert_eq!(child, "launch-0002");
    assert_eq!(world.snapshot().launches.len(), 2);

    let child_log = world.event_log(&child).expect("child log");
    assert!(
        child_log
            .iter()
            .all(|(seq, kind)| *seq <= pre || kind == "fork"),
        "child must be prefix + fork, got {child_log:?}"
    );
    assert!(
        !child_log
            .iter()
            .any(|(seq, kind)| kind == "stage" && *seq <= pre),
        "pre-stage fork must not include a stage event"
    );
    assert!(child_log.iter().any(|(_, k)| k == "fork"));

    let parent_log = world.event_log(&parent).expect("parent log");
    assert!(parent_log.iter().any(|(_, k)| k == "stage"));
    assert!(world.event_len(&parent).expect("parent len") >= parent_len);
    assert!(world.verify_chain(&parent).expect("parent chain"));
    assert!(world.verify_chain(&child).expect("child chain"));

    let child_nodes: HashSet<String> = world
        .graph_nodes(&child)
        .expect("child graph")
        .into_iter()
        .collect();
    assert!(
        child_nodes.contains("p-00-lvt-30"),
        "child at pre-stage still has boost nodes: {child_nodes:?}"
    );
    let snap = world.snapshot();
    assert!(snap.launches.iter().all(|l| l.design == design));
    assert_eq!(
        snap.launches
            .iter()
            .find(|l| l.name == child)
            .map(|l| l.parent.as_str()),
        Some(parent.as_str())
    );
}

#[test]
fn fork_missing_seq_refuses_silent_fork_current() {
    let world = cache_world();
    world.set_persist_wall_cap(false);
    let parent = world.launch_from_pad().expect("launch");
    world.flush_durable(&parent).expect("flush");
    let err = world
        .fork_at(&parent, Some(99_999))
        .expect_err("missing seq");
    assert!(err.contains("not_found.ksp.seq"), "got {err}");
    assert_eq!(world.snapshot().launches.len(), 1);
}

#[test]
fn rewind_past_stage_restores_boost_nodes() {
    let world = cache_world();
    world.set_persist_wall_cap(false);
    let launch = world.launch_from_pad().expect("launch");
    let stage_seq = tick_until_kind(&world, &launch, "stage", 800);
    let pre = stage_seq - 1;
    let t_after = world.snapshot().launches[0].t;
    let nodes_after: HashSet<String> = world
        .graph_nodes(&launch)
        .expect("graph")
        .into_iter()
        .collect();
    assert!(
        !nodes_after.contains("p-00-lvt-30"),
        "after stage boost should be gone: {nodes_after:?}"
    );

    world.rewind(&launch, pre).expect("rewind");
    let nodes: HashSet<String> = world
        .graph_nodes(&launch)
        .expect("graph")
        .into_iter()
        .collect();
    assert!(
        nodes.contains("p-00-lvt-30"),
        "rewind restores boost nodes: {nodes:?}"
    );
    let spec = world.launch_spec(&launch).expect("spec");
    assert_eq!(spec.parts[0].part_id, "lvt-30");
    let t_rewound = world.snapshot().launches[0].t;
    assert!(t_rewound < t_after, "t {t_rewound} vs {t_after}");

    for _ in 0..30 {
        world.tick_frame();
    }
    world.flush_durable(&launch).expect("flush");
    let t_later = world.snapshot().launches[0].t;
    assert!(t_later > t_rewound, "tape continues after rewind");
    let kinds = world.event_kinds(&launch).expect("kinds");
    assert!(kinds.iter().any(|k| k == "rewind"));
    assert!(world.verify_chain(&launch).expect("chain"));
}

#[test]
fn add_tank_then_rewind_restores_original_stick() {
    let world = cache_world();
    world.set_persist_wall_cap(false);
    let parent = world.launch_from_pad().expect("launch");
    for _ in 0..20 {
        world.tick_frame();
    }
    world.flush_durable(&parent).expect("flush");
    let at = world.snapshot().launches[0].seq;
    let child = world.fork_at(&parent, Some(at)).expect("fork");
    let original = world.launch_spec(&child).expect("spec").parts.len();
    world.add_tank(&child, "tank-s").expect("tank");
    let edited = world.launch_spec(&child).expect("spec");
    assert!(
        edited
            .parts
            .iter()
            .filter(|p| p.part_id == "tank-s")
            .count()
            > CraftSpec::sounding_stick()
                .parts
                .iter()
                .filter(|p| p.part_id == "tank-s")
                .count()
    );
    assert!(edited.parts.len() > original);
    let nodes: HashSet<String> = world
        .graph_nodes(&child)
        .expect("graph")
        .into_iter()
        .collect();
    assert!(nodes.iter().any(|id| id.contains("tank-s")));

    let pre_edit = world
        .event_log(&child)
        .expect("log")
        .into_iter()
        .rev()
        .find(|(_, k)| k != "edit")
        .map(|(seq, _)| seq)
        .expect("pre-edit seq");
    world.rewind(&child, pre_edit).expect("rewind");
    let restored = world.launch_spec(&child).expect("spec");
    assert_eq!(restored.parts.len(), original);
    assert_eq!(
        restored
            .parts
            .iter()
            .map(|p| p.part_id.as_str())
            .collect::<Vec<_>>(),
        CraftSpec::sounding_stick()
            .parts
            .iter()
            .map(|p| p.part_id.as_str())
            .collect::<Vec<_>>()
    );
    let parent_spec = world.launch_spec(&parent).expect("parent spec");
    assert_eq!(
        parent_spec.parts.len(),
        CraftSpec::sounding_stick().parts.len()
    );
}

#[test]
fn promote_launch_onto_vab_is_refused() {
    let world = cache_world();
    world.set_persist_wall_cap(false);
    let launch = world.launch_from_pad().expect("launch");
    let err = world
        .promote_named(&launch, "vab", "strict")
        .expect_err("guard");
    assert!(err.contains("failed_precondition.ksp.promote"), "got {err}");
}

#[test]
fn promote_grandchild_is_branch_point() {
    let world = cache_world();
    world.set_persist_wall_cap(false);
    let _launch = world.launch_from_pad().expect("launch");
    let design = world.snapshot().launches[0].design.clone();
    world.fork_named(&design, "promo-0001").expect("grandchild");
    let err = world
        .promote_named("promo-0001", "vab", "strict")
        .expect_err("grandchild");
    assert!(
        err.contains("invalid_argument.engine.branch_point"),
        "got {err}"
    );
}

#[test]
fn promote_strict_conflicts_when_hangar_changed_and_sourcewins_overwrites() {
    let world = cache_world();
    world.set_persist_wall_cap(false);
    let launch = world.launch_from_pad().expect("launch");
    let design = world.snapshot().launches[0].design.clone();
    world.add_tank(&launch, "tank-s").expect("tank");
    world.vab_add("fin", None).expect("fin");
    assert!(world.vab_spec().parts.iter().any(|p| p.part_id == "fin"));

    let err = world
        .promote_this_design(&launch, "strict")
        .expect_err("strict");
    assert!(err.contains("conflict.engine.promotion"), "got {err}");
    assert!(
        world.vab_spec().parts.iter().any(|p| p.part_id == "fin"),
        "strict must not mutate vab"
    );

    let view = world
        .promote_this_design(&launch, "source_wins")
        .expect("source_wins");
    assert_eq!(view["ok"], true);
    assert_eq!(view["design"], design);
    let unsupported = view["unsupported"]
        .as_array()
        .expect("unsupported")
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>();
    assert!(unsupported.contains(&"event"), "{unsupported:?}");
    assert!(unsupported.contains(&"graph"), "{unsupported:?}");

    let vab = world.vab_spec();
    assert!(vab.parts.iter().any(|p| p.part_id == "tank-s"));
    assert!(
        vab.parts.iter().all(|p| p.part_id != "fin"),
        "source_wins drops the hangar fin"
    );
    let nodes: HashSet<String> = world
        .graph_nodes("vab")
        .expect("vab graph")
        .into_iter()
        .collect();
    assert!(nodes.iter().any(|id| id.contains("tank-s")));
    assert!(nodes.iter().all(|id| !id.contains("fin")));

    let names = world.list_branches().expect("branches");
    assert!(
        names.contains(&design),
        "design must survive promote: {names:?}"
    );

    let child = world.fork_at(&launch, None).expect("fork-at");
    world.add_tank(&child, "tank-s").expect("second tank");
    let again = world
        .promote_this_design(&child, "strict")
        .expect("second promote via merge-edge");
    assert_eq!(again["ok"], true);
    assert_eq!(again["design"], design);
}

#[test]
fn compare_focused_launch_vs_vab_is_counts_dto() {
    let world = cache_world();
    world.set_persist_wall_cap(false);
    let launch = world.launch_from_pad().expect("launch");
    for _ in 0..8 {
        world.tick_frame();
    }
    world.flush_durable(&launch).expect("flush");
    let view = world.compare(&launch, "vab").expect("compare");
    assert!(view["event_entities"].as_u64().unwrap_or(0) > 0);
    assert!(view.get("json_entities").is_some());
    assert!(view.get("kv_entities").is_some());
    assert!(view.get("graph_entities").is_some());
    assert!(view.get("added").is_some());
    let snap = world.snapshot();
    assert!(snap.last_compare.is_some());
}

#[test]
fn archive_shared_design_survives_until_last_launch() {
    let world = cache_world();
    world.set_persist_wall_cap(false);
    let parent = world.launch_from_pad().expect("launch");
    for _ in 0..12 {
        world.tick_frame();
    }
    world.flush_durable(&parent).expect("flush");
    let design = world.snapshot().launches[0].design.clone();
    let at = world.snapshot().launches[0].seq;
    let child = world.fork_at(&parent, Some(at)).expect("fork");
    assert_eq!(world.snapshot().launches[1].design, design);

    let first = world.archive(&parent, false).expect("archive parent");
    assert_eq!(first["ok"], true);
    assert_eq!(first["design_deleted"], false);
    assert!(first["remaining_refcount"].as_u64().unwrap() >= 1);
    let names = world.list_branches().expect("branches");
    assert!(!names.contains(&parent), "{names:?}");
    assert!(names.contains(&child), "{names:?}");
    assert!(
        names.contains(&design),
        "shared design must survive: {names:?}"
    );
    assert_eq!(world.snapshot().launches.len(), 1);

    let second = world.archive(&child, false).expect("archive child");
    assert_eq!(second["design_deleted"], true);
    let names = world.list_branches().expect("branches");
    assert!(
        !names.contains(&design),
        "refcount 0 deletes design: {names:?}"
    );
    assert!(!names.contains(&child));
    assert!(names.contains(&"vab".to_string()));
    assert!(names.contains(&"default".to_string()));
}

#[test]
fn archive_keep_snapshot_leaves_design() {
    let world = cache_world();
    world.set_persist_wall_cap(false);
    let launch = world.launch_from_pad().expect("launch");
    let design = world.snapshot().launches[0].design.clone();
    let view = world.archive(&launch, true).expect("keep");
    assert_eq!(view["keep_snapshot"], true);
    assert_eq!(view["design_deleted"], false);
    let names = world.list_branches().expect("branches");
    assert!(names.contains(&design), "{names:?}");
    assert!(!names.contains(&launch));
}

#[test]
fn archive_vab_is_refused() {
    let world = cache_world();
    let err = world.archive("vab", false).expect_err("vab");
    assert!(err.contains("failed_precondition.ksp.archive"), "got {err}");
}

#[test]
fn durable_sourcewins_then_fork_at_promote_and_archive_refcount() {
    let dir = tempfile::tempdir().expect("tempdir");
    let design;
    let child;
    let parent;
    {
        let world = durable_world(dir.path());
        world.set_persist_wall_cap(false);
        parent = world.launch_from_pad().expect("launch");
        world.add_tank(&parent, "tank-s").expect("tank");
        world.vab_add("fin", None).expect("fin");
        world
            .promote_this_design(&parent, "source_wins")
            .expect("source_wins");
        assert!(world.vab_spec().parts.iter().any(|p| p.part_id == "tank-s"));
        assert!(world.vab_spec().parts.iter().all(|p| p.part_id != "fin"));
        world.flush_durable(&parent).expect("flush");
        let at = world.snapshot().launches[0].seq;
        child = world.fork_at(&parent, Some(at)).expect("fork");
        world.add_tank(&child, "tank-s").expect("child tank");
        let again = world
            .promote_this_design(&child, "source_wins")
            .expect("second promote");
        assert_eq!(again["ok"], true);
        design = world.snapshot().launches[0].design.clone();
        let parent_first = world.archive(&parent, false);
        assert!(
            parent_first
                .as_ref()
                .err()
                .is_some_and(|e| e.contains("failed_precondition")),
            "durable must refuse deleting a fork source while the child lives: {parent_first:?}"
        );
        let leaf = world.archive(&child, false).expect("archive leaf child");
        assert_eq!(leaf["design_deleted"], false);
        let names = world.list_branches().expect("branches");
        assert!(
            names.contains(&design),
            "parent still refs design: {names:?}"
        );
        assert!(names.contains(&parent), "{names:?}");
        assert!(!names.contains(&child), "{names:?}");
    }
    {
        let world = durable_world(dir.path());
        let names = world.list_branches().expect("resume branches");
        assert!(names.contains(&parent), "{names:?}");
        assert!(names.contains(&design), "{names:?}");
        assert!(!names.contains(&child), "{names:?}");
        assert!(world.vab_spec().parts.iter().any(|p| p.part_id == "tank-s"));
        world.archive(&parent, false).expect("archive last parent");
        let names = world.list_branches().expect("after last");
        assert!(
            !names.contains(&design),
            "last archive drops design: {names:?}"
        );
    }
}
