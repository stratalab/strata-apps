use serde_json::json;
use strata_island::{
    addresses::{self, Address},
    places::Place,
    search, store,
};
use stratadb::{
    json::{JsonDocumentId, JsonPath, JsonValue},
    CommitVersion,
};
fn sample(id: &str, house: &str, street: &str) -> Address {
    Address {
        place: Place {
            id: id.into(),
            name: format!("{house} {street}"),
            category: "address".into(),
            x: 0,
            y: 0,
            node: Some("n:1".into()),
            approach_m: 20,
            attachment: "approximate".into(),
            source_url: "https://data.cityofnewyork.us/d/uf93-f8nk".into(),
            coordinate_method: "nyc_address_point".into(),
            aliases: vec![],
            subway: None,
        },
        house: house.into(),
        street: search::normalize(street),
        street_id: format!("street:{}", search::normalize(street)),
        bin: Some("1234567".into()),
        zip: Some("10001".into()),
        source_rows: vec![id.into()],
        source: json!({}),
        attachment_evidence: json!({}),
        place_ids: vec![],
    }
}
#[test]
fn search_aliases_numbers_directions_ranges_and_cursors() {
    let rows = vec![
        sample("a:1", "350", "5 Avenue"),
        sample("a:2", "35", "5 Avenue"),
        sample("a:3", "230", "West 55 Street"),
        sample("a:4", "230", "East 55 Street"),
        sample("a:5", "12-14", "Broadway"),
        sample("a:6", "94 1/2", "Greenwich Street"),
    ];
    let idx = search::Index::new(&rows, &[]);
    for q in [
        "350 Fifth Ave",
        "350 fif",
        "350 5th Avenue New York NY 10001",
    ] {
        let r = idx.query(q, None, 8, None, "city", 1).unwrap();
        assert_eq!(r["places"][0]["id"], "a:1", "{q}");
        assert_eq!(r["total"], 1);
    }
    for q in ["94 1/2 Greenwich Street", "94½ Greenwich St"] {
        assert_eq!(
            idx.query(q, None, 8, None, "city", 1).unwrap()["places"][0]["id"],
            "a:6"
        );
    }
    assert_eq!(
        idx.query("Fifth Avenue 350", None, 8, None, "city", 1)
            .unwrap()["places"][0]["id"],
        "a:1"
    );
    assert_eq!(
        idx.query("5th Avenue", None, 8, None, "city", 1).unwrap()["total"],
        2
    );
    let r = idx.query("35 5 Ave", None, 8, None, "city", 1).unwrap();
    assert_eq!(r["places"][0]["id"], "a:2");
    assert_eq!(
        idx.query("230 E 55th", None, 8, None, "city", 1).unwrap()["places"][0]["id"],
        "a:4"
    );
    assert_eq!(
        idx.query("13 Broadway", None, 8, None, "city", 1).unwrap()["total"],
        0
    );
    assert_eq!(
        idx.query("12–14 Broadway", None, 8, None, "city", 1)
            .unwrap()["total"],
        1
    );
    assert_eq!(
        idx.query("350 5 Ave apt 1", None, 8, None, "city", 1)
            .unwrap()["total"],
        0
    );
    let first = idx.query("230 55", None, 1, None, "city", 1).unwrap();
    let cursor = first["cursor"].as_str().unwrap();
    let next = idx
        .query("230 55", None, 1, Some(cursor), "city", 1)
        .unwrap();
    assert_ne!(first["places"][0]["id"], next["places"][0]["id"]);
    assert!(idx
        .query("230 55", None, 1, Some(cursor), "city", 2)
        .is_err());
}
#[test]
fn name_search_keeps_landmarks_ahead_of_numbered_addresses() {
    let rows: Vec<_> = (1..=10)
        .map(|n| sample(&format!("a:{n}"), &n.to_string(), "Bryant Park"))
        .collect();
    let mut park = rows[0].place.clone();
    park.id = "p:park".into();
    park.name = "Bryant Park".into();
    park.category = "park".into();
    let mut statue = park.clone();
    statue.id = "p:statue".into();
    statue.name = "William Cullen Bryant".into();
    statue.category = "landmark".into();
    let idx = search::Index::new(&rows, &[park, statue]);
    let result = idx.query("Bryant", None, 5, None, "city", 1).unwrap();
    assert_eq!(result["places"][0]["id"], "p:park");
    assert_eq!(result["places"][1]["id"], "p:statue");
    let result = idx.query("1 Bryant", None, 5, None, "city", 1).unwrap();
    assert_eq!(result["places"][0]["id"], "a:1");
    assert_eq!(result["total"], 1);
    let result = idx
        .query("Bryant", Some("address"), 5, None, "city", 1)
        .unwrap();
    assert!(result["places"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["category"] == "address"));
}
#[test]
fn publication_reopen_historical_binding_and_fork() {
    let dir = tempfile::tempdir().unwrap();
    let rows = vec![
        sample("a:1", "350", "5 Avenue"),
        sample("a:2", "351", "5 Avenue"),
    ];
    let old;
    {
        let mut db = store::open_local(dir.path(), None).unwrap();
        addresses::import_rows(&db, "default", &rows, "test").unwrap();
        old = addresses::ready_version(&db, "default").unwrap().unwrap();
        let catalog = addresses::load(&db, "default", old, &[]).unwrap();
        assert_eq!(catalog.rows, rows);
        db.branches()
            .unwrap()
            .fork_at_version(
                &store::branch("default").unwrap(),
                store::branch("child").unwrap(),
                old,
            )
            .unwrap();
        db.json(store::branch("child").unwrap(), store::space().unwrap())
            .unwrap()
            .set_or_create(
                JsonDocumentId::new("a:1").unwrap(),
                &JsonPath::root(),
                JsonValue::new(json!({"changed":true})).unwrap(),
            )
            .unwrap();
        let original = addresses::detail(&db, "child", old, "a:1").unwrap();
        assert_eq!(original["address"]["name"], "350 5 Avenue");
        let graph = addresses::explore(&db, "default", old, "a:1", 3).unwrap();
        assert!(graph["nodes"].as_array().unwrap().len() <= 3);
        assert_eq!(graph["truncated"], true);
        db.close().unwrap();
    }
    let db = store::open_local(dir.path(), None).unwrap();
    addresses::import_rows(&db, "default", &rows, "test").unwrap();
    assert_eq!(
        addresses::ready_version(&db, "default").unwrap().unwrap(),
        old
    );
    assert_eq!(
        addresses::load(&db, "default", old, &[]).unwrap().rows,
        rows
    );
    assert!(addresses::load(&db, "default", CommitVersion::new(1), &[]).is_err());
}
#[test]
fn impact_classification_has_no_radius_cutoff() {
    use addresses::impact_status as s;
    assert_eq!(s(false, None, None), "unconnected");
    assert_eq!(s(true, None, None), "already_unreachable");
    assert_eq!(s(true, Some(200001.), Some(200002.)), "farther");
    assert_eq!(s(true, Some(1.), None), "newly_unreachable");
    assert_eq!(s(true, None, Some(1.)), "newly_reachable");
    assert_eq!(s(true, Some(2.), Some(1.)), "closer");
    assert_eq!(s(true, Some(1.), Some(1.)), "unchanged");
}
#[test]
fn address_crash_worker() {
    let Ok(path) = std::env::var("ADDRESS_TEST_DB") else {
        return;
    };
    let stage = std::env::var("ADDRESS_TEST_STAGE").unwrap();
    let db = store::open_local(std::path::Path::new(&path), None).unwrap();
    std::env::set_var("ISLAND_TEST_CRASH_AT", stage);
    addresses::import_rows(
        &db,
        "default",
        &[
            sample("a:1", "350", "5 Avenue"),
            sample("a:2", "351", "5 Avenue"),
        ],
        "test",
    )
    .unwrap();
    panic!("checkpoint not hit")
}
#[test]
fn address_import_crash_replay() {
    if !cfg!(debug_assertions) {
        return;
    }
    for stage in [
        "address-documents",
        "address-nodes",
        "address-edges",
        "address-validated",
        "address-ready",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "address_crash_worker", "--nocapture"])
            .env("ADDRESS_TEST_DB", dir.path())
            .env("ADDRESS_TEST_STAGE", stage)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(77));
        let db = store::open_local(dir.path(), None).unwrap();
        let rows = vec![
            sample("a:1", "350", "5 Avenue"),
            sample("a:2", "351", "5 Avenue"),
        ];
        addresses::import_rows(&db, "default", &rows, "test").unwrap();
        let v = addresses::ready_version(&db, "default").unwrap().unwrap();
        assert_eq!(addresses::load(&db, "default", v, &[]).unwrap().rows, rows);
    }
}
