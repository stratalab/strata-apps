//! Reproducible storage/search/fork workload; always disposable, never the live DB.
use serde_json::json;
use std::time::Instant;
use strata_island::{addresses, store};
use stratadb::graph::*;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count: usize = std::env::args().nth(1).unwrap_or("1000".into()).parse()?;
    let durable = std::env::args().any(|s| s == "--durable");
    let temp = tempfile::tempdir()?;
    let mut db = if durable {
        store::open_local(temp.path(), None)?
    } else {
        store::open_cache(None)?
    };
    let mut rows = addresses::fixture()?;
    rows.truncate(count);
    let start = Instant::now();
    addresses::import_rows(&db, "default", &rows, &format!("probe-{}", rows.len()))?;
    let import_ms = start.elapsed().as_secs_f64() * 1000.;
    let version = addresses::ready_version(&db, "default")?.unwrap();
    let catalog = addresses::load(&db, "default", version, &[])?;
    let mut times = Vec::new();
    for a in rows.iter().step_by((rows.len() / 40).max(1)).take(40) {
        let t = Instant::now();
        let result =
            catalog
                .search
                .query(&a.place.name, None, 8, None, "default", version.as_u64())?;
        assert!(
            result["total"].as_u64().unwrap_or(0) > 0,
            "{}",
            a.place.name
        );
        times.push(t.elapsed().as_secs_f64() * 1000.);
    }
    let mut graph = db.graph(store::branch("default")?, store::space()?)?;
    let name = GraphName::new(addresses::GRAPH)?;
    let t = Instant::now();
    let info = graph.graph_info(&name)?.unwrap();
    let info_ms = t.elapsed().as_secs_f64() * 1000.;
    let t = Instant::now();
    let page = graph.nodes_by_type_at_version(
        &name,
        &GraphTypeName::new("address")?,
        None,
        20,
        version,
    )?;
    let page_ms = t.elapsed().as_secs_f64() * 1000.;
    assert_eq!(page.nodes().len(), 20.min(rows.len()));
    let mut degree = std::collections::BTreeMap::new();
    for a in &rows {
        *degree.entry(&a.street_id).or_insert(0) += 1;
    }
    let hub = degree.iter().max_by_key(|(_, n)| **n).unwrap();
    let t = Instant::now();
    let page = graph.neighbors_at_version(
        &name,
        &GraphNodeId::new(*hub.0)?,
        GraphDirection::Incoming,
        None,
        None,
        10,
        version,
    )?;
    let neighbor_ms = t.elapsed().as_secs_f64() * 1000.;
    let returned = page.neighbors().len();
    drop(graph);
    let t = Instant::now();
    db.branches()?
        .fork_at_version(&store::branch("default")?, store::branch("child")?, version)?;
    let fork_ms = t.elapsed().as_secs_f64() * 1000.;
    let t = Instant::now();
    db.branches()?.delete(&store::branch("child")?)?;
    let archive_ms = t.elapsed().as_secs_f64() * 1000.;
    let graph_result = addresses::explore(&db, "default", version, &rows[0].place.id, 30)?;
    let mut reopen_ms = None;
    if durable {
        db.close()?;
        drop(db);
        let t = Instant::now();
        let db = store::open_local(temp.path(), None)?;
        let restored = addresses::load(&db, "default", version, &[])?;
        assert_eq!(restored.rows.len(), rows.len());
        reopen_ms = Some(t.elapsed().as_secs_f64() * 1000.);
    }
    fn bytes(path: &std::path::Path) -> u64 {
        std::fs::read_dir(path)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| {
                if e.path().is_dir() {
                    bytes(&e.path())
                } else {
                    e.metadata().unwrap().len()
                }
            })
            .sum()
    }
    times.sort_by(f64::total_cmp);
    println!(
        "{}",
        json!({"profile":"address-storage","engine":"v1.2.3 / 6fc481c","addresses":rows.len(),"durable":durable,"import_ms":import_ms,"hydrate_index_ms":catalog.load_ms,"nodes":info.node_count(),"edges":info.edge_count(),"graph_info_ms":info_ms,"typed_first_20_ms":page_ms,"hub_degree":hub.1,"neighbors_returned":returned,"neighbors_first_10_ms":neighbor_ms,"query_reps":times.len(),"query_p50_ms":times[times.len()/2],"query_p95_ms":times[times.len()*95/100],"fork_ms":fork_ms,"archive_ms":archive_ms,"relationship_explore":graph_result["algorithm_ms"],"reopen_hydrate_ms":reopen_ms,"logical_disk_bytes":bytes(temp.path()),"source_hash":addresses::hash()})
    );
    Ok(())
}
