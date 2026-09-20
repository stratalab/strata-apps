//! Each crash is a separate writer process, terminated without running destructors.
use strata_island::{
    extract, places, scenario, store,
    world::{OpenArgs, World},
};
#[test]
fn crash_worker() {
    let Ok(path) = std::env::var("ISLAND_TEST_DB") else {
        return;
    };
    let stage = std::env::var("ISLAND_TEST_STAGE").unwrap();
    let args = OpenArgs {
        cache: false,
        db_path: path.into(),
        memory_budget_bytes: None,
    };
    if let Some(checkpoint) = stage.strip_prefix("upgrade-") {
        let mut db = store::open_local(&args.db_path, None).unwrap();
        store::write_json(&db, "default", "dataset", serde_json::json!({"schema":2})).unwrap();
        let roads = extract::parse_city(include_str!("../fixtures/manhattan-drive.json")).unwrap();
        let aliases = extract::parse_gazetteer(include_str!("../fixtures/gazetteer.json")).unwrap();
        let closure =
            extract::parse_closure(include_str!("../fixtures/closure-42nd.json")).unwrap();
        store::import_city(&mut db, &roads, &aliases).unwrap();
        let baseline = std::env::var("ISLAND_TEST_BASELINE").unwrap();
        let (fixture, revision) = if baseline == "1" {
            (places::LEGACY_FIXTURE, 1)
        } else if baseline == "2" {
            (places::PREVIOUS_FIXTURE, 2)
        } else {
            (places::OSM_FIXTURE, 3)
        };
        places::import_catalog(&db, "city", fixture, revision).unwrap();
        scenario::create(&mut db, &roads, &closure, 1).unwrap();
        // Terminate an additive child upgrade while the official catalog is still old.
        std::env::set_var("ISLAND_TEST_CRASH_AT", checkpoint);
        places::import_on(&db, "desk-0001").unwrap();
        panic!("upgrade checkpoint was not reached");
    }
    if stage.starts_with("import-") {
        std::env::set_var("ISLAND_TEST_CRASH_AT", &stage);
    }
    let world = World::open_v2(args).unwrap();
    if stage.starts_with("scenario-") {
        std::env::set_var("ISLAND_TEST_CRASH_AT", stage);
        world.close_42nd(None).unwrap();
    } else if stage == "reopen-batch" {
        let desk = world.close_42nd(None).unwrap().desk;
        let version = world.place_snapshot(&desk).unwrap().version;
        std::env::set_var("ISLAND_TEST_CRASH_AT", "scenario-batch");
        world
            .scenario_operation(&desk, "reopen", false, version.as_u64())
            .unwrap();
    }
    panic!("checkpoint was not reached");
}
#[test]
fn process_termination_recovers_complete_versions() {
    let dir = tempfile::tempdir().unwrap();
    for stage in [
        "import-documents",
        "import-graph",
        "import-subway",
        "import-ready",
        "scenario-fork",
        "scenario-batch",
        "scenario-event",
        "scenario-ready",
        "reopen-batch",
    ] {
        let path = dir.path().join(stage);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_worker", "--nocapture"])
            .env("ISLAND_TEST_DB", &path)
            .env("ISLAND_TEST_STAGE", stage)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(77), "{stage}");
        let world = World::open_v2(OpenArgs {
            cache: false,
            db_path: path.clone(),
            memory_budget_bytes: None,
        })
        .unwrap_or_else(|error| panic!("{stage}: {error:?}"));
        let city = world.place_snapshot("city").unwrap();
        assert_eq!(city.places.len(), 1852);
        if stage.starts_with("scenario-") || stage == "reopen-batch" {
            let child = world.place_snapshot("desk-0001").unwrap();
            assert_eq!(child.places.as_ref(), city.places.as_ref());
            assert_eq!(
                child.roads.edge_count(),
                city.roads.edge_count() - if stage == "reopen-batch" { 0 } else { 8 },
                "{stage}"
            );
            assert!(world.verify_desk_chain("desk-0001").unwrap());
            let history = world.scenario_history("desk-0001").unwrap();
            assert_eq!(
                history["operations"].as_array().unwrap().last().unwrap()["status"],
                "ready"
            );
        }
    }
}

#[test]
fn interrupted_catalog_upgrade_preserves_scenarios_and_history() {
    let dir = tempfile::tempdir().unwrap();
    for (baseline, stage) in [
        (1, "upgrade-import-graph"),
        (2, "upgrade-import-documents"),
        (2, "upgrade-import-graph"),
        (2, "upgrade-import-ready"),
        (3, "upgrade-import-subway"),
        (3, "upgrade-import-ready"),
    ] {
        let historical_count = match baseline {
            1 => 50,
            2 => 550,
            _ => 1701,
        };
        let path = dir.path().join(format!("v{baseline}-{stage}"));
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_worker", "--nocapture"])
            .env("ISLAND_TEST_DB", &path)
            .env("ISLAND_TEST_STAGE", stage)
            .env("ISLAND_TEST_BASELINE", baseline.to_string())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(77), "{stage}");
        let args = || OpenArgs {
            cache: false,
            db_path: path.clone(),
            memory_budget_bytes: None,
        };
        let world = World::open_v2(args()).unwrap();
        let city = world.place_snapshot("city").unwrap();
        let child = world.place_snapshot("desk-0001").unwrap();
        assert_eq!(city.places.len(), 1852);
        assert_eq!(child.places, city.places);
        assert_eq!(city.relations.node_count(), 3035);
        assert_eq!(city.relations.edge_count(), 3666);
        assert_eq!(
            world.subway("city", None).unwrap()["edges"]
                .as_array()
                .unwrap()
                .len(),
            842
        );
        assert_eq!(
            world.subway("desk-0001", None).unwrap()["stations"]
                .as_array()
                .unwrap()
                .len(),
            151
        );
        assert_eq!(child.roads.edge_count() + 8, city.roads.edge_count());
        let history = world.scenario_history("desk-0001").unwrap();
        let old = history["operations"][0]["version"].as_u64().unwrap();
        assert!(child.version.as_u64() > old);
        let before = world
            .historical_places("desk-0001", history["parent_version"].as_u64().unwrap())
            .unwrap();
        let closed = world.historical_places("desk-0001", old).unwrap();
        assert_eq!(before.places.len(), historical_count);
        assert_eq!(closed.places, before.places);
        assert_eq!(before.roads.edge_count(), city.roads.edge_count());
        assert_eq!(closed.roads.edge_count(), child.roads.edge_count());
        assert!(world
            .scenario_operation("desk-0001", "stale", false, old)
            .is_err());
        world
            .scenario_operation(
                "desk-0001",
                "reopen-upgraded",
                false,
                child.version.as_u64(),
            )
            .unwrap();
        let reopened = world.place_snapshot("desk-0001").unwrap();
        assert_eq!(reopened.places.len(), 1852);
        assert_eq!(reopened.roads.edge_count(), city.roads.edge_count());
        let version = reopened.version;
        drop(world);
        let world = World::open_v2(args()).unwrap();
        assert_eq!(world.place_snapshot("desk-0001").unwrap().version, version);
        assert_eq!(
            world
                .historical_places("desk-0001", old)
                .unwrap()
                .places
                .len(),
            historical_count
        );
        assert!(world.verify_desk_chain("desk-0001").unwrap());
    }
}
