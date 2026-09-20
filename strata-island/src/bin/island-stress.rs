//! Isolated graph workloads. Never opens an existing database directory.
use clap::Parser;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Instant,
};
use strata_island::{places, store};
use stratadb::graph::*;
use stratadb::{BranchName, ProductSpace};
#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "synthetic-1000")]
    profile: String,
    #[arg(long,default_value="cache",value_parser=["cache","durable"])]
    mode: String,
    #[arg(long)]
    db: Option<PathBuf>,
    #[arg(long)]
    report: PathBuf,
    #[arg(long, default_value_t = 200)]
    repetitions: usize,
    #[arg(long, default_value_t = 20)]
    warmup: usize,
    #[arg(long, default_value_t = 4)]
    branches: usize,
    #[arg(long, default_value_t = 4)]
    concurrency: usize,
    #[arg(long, default_value_t = 512)]
    chunk: usize,
    #[arg(long, default_value_t = 2048)]
    memory_mb: u64,
    #[arg(long, default_value_t = 42)]
    seed: u64,
}
fn bytes(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| {
                    if e.path().is_dir() {
                        bytes(&e.path())
                    } else {
                        e.metadata().map(|m| m.len()).unwrap_or(0)
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}
fn measure<F>(
    samples: &mut BTreeMap<String, Vec<f64>>,
    name: &str,
    n: usize,
    warmup: usize,
    mut f: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut() -> Result<(), Box<dyn std::error::Error>>,
{
    for i in 0..(n + warmup) {
        let t = Instant::now();
        f()?;
        if i >= warmup {
            samples
                .entry(name.into())
                .or_default()
                .push(t.elapsed().as_secs_f64() * 1000.);
        }
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = Args::parse();
    if !(1..=2000).contains(&a.repetitions)
        || a.warmup > 200
        || !(1..=32).contains(&a.branches)
        || ![128, 512, 800].contains(&a.chunk)
        || ![1, 4, 8, 16].contains(&a.concurrency)
    {
        return Err("invalid workload limits".into());
    }
    if let Some(path) = &a.db {
        if path.exists() {
            return Err("refusing existing database path; choose a fresh run directory".into());
        }
    }
    let mut db = if a.mode == "durable" {
        store::open_local(
            a.db.as_deref().ok_or("durable mode requires --db")?,
            Some(a.memory_mb * 1024 * 1024),
        )?
    } else {
        store::open_cache(Some(a.memory_mb * 1024 * 1024))?
    };
    let curated = match a.profile.as_str() {
        "curated-50" => Some((places::LEGACY_FIXTURE, 1)),
        "curated-550" => Some((places::PREVIOUS_FIXTURE, 2)),
        "curated-all" => Some((places::OSM_FIXTURE, 3)),
        "curated-subway" => Some((places::FIXTURE, 4)),
        _ => None,
    };
    let started = Instant::now();
    let (branch, name, origin) = if let Some((fixture, revision)) = curated {
        let extract = strata_island::extract::parse_city(include_str!(
            "../../fixtures/manhattan-drive.json"
        ))?;
        let legacy =
            strata_island::extract::parse_gazetteer(include_str!("../../fixtures/gazetteer.json"))?;
        store::import_city(&mut db, &extract, &legacy)?;
        places::import_catalog(&db, "city", fixture, revision)?;
        (
            BranchName::new("city")?,
            GraphName::new("manhattan")?,
            GraphNodeId::new("n:42435663")?,
        )
    } else {
        let count: usize = a
            .profile
            .strip_prefix("synthetic-")
            .ok_or("unknown profile")?
            .parse()?;
        if !(10..=100_000).contains(&count) {
            return Err("synthetic size must be 10..100000".into());
        }
        let mut graph = db.graph(BranchName::new("default")?, ProductSpace::new("city")?)?;
        let name = GraphName::new("stress")?;
        graph.create_graph(name.clone())?;
        graph.define_object_type(
            &name,
            GraphObjectTypeDef::new(GraphTypeName::new("Place")?, [])?,
        )?;
        let mut nodes = Vec::with_capacity(count);
        let mut edges = Vec::with_capacity(count * 4);
        for i in 0..count {
            nodes.push((
                GraphNodeId::new(format!("n:{i:06}"))?,
                GraphNodeData::new(None, None).with_object_type(GraphTypeName::new("Place")?),
            ));
        }
        // Bidirectional ring, forward stride, and skewed hub: 4 distinct directed edges/node.
        for i in 0..count {
            for (kind, target) in [
                ("forward", (i + 1) % count),
                ("reverse", (i + count - 1) % count),
                ("stride", (i + 17) % count),
                ("hub", 0),
            ] {
                edges.push((
                    nodes[i].0.clone(),
                    GraphEdgeType::new(kind)?,
                    nodes[target].0.clone(),
                    GraphEdgeData::new(((i as u64 + a.seed) % 100 + 1) as f64, None)?,
                ));
            }
        }
        graph.bulk_insert(&name, &nodes, &edges, Some(a.chunk))?;
        (BranchName::new("default")?, name, nodes[0].0.clone())
    };
    let import_ms = started.elapsed().as_secs_f64() * 1000.;
    let budget = GraphAnalyticsBudget::new(110_000, 500_000);
    let mut samples = BTreeMap::new();
    let space = ProductSpace::new("city")?;
    let mut graph = db.graph(branch.clone(), space.clone())?;
    let info = graph.graph_info(&name)?.unwrap();
    let t = Instant::now();
    let index = graph.adjacency_index(&name, &budget)?;
    let snapshot_ms = t.elapsed().as_secs_f64() * 1000.;
    measure(
        &mut samples,
        "snapshot_storage",
        a.repetitions.min(20),
        a.warmup.min(2),
        || {
            graph.adjacency_index(&name, &budget)?;
            Ok(())
        },
    )?;
    measure(
        &mut samples,
        "graph_info_storage",
        a.repetitions,
        a.warmup,
        || {
            graph.graph_info(&name)?;
            Ok(())
        },
    )?;
    if curated.is_none() {
        for (label, cursor) in [
            ("typed_first", None),
            ("typed_middle", index.node_id(index.node_count() / 2)),
            ("typed_last", index.node_id(index.node_count() - 2)),
        ] {
            measure(&mut samples, label, a.repetitions, a.warmup, || {
                graph.nodes_by_type(&name, &GraphTypeName::new("Place")?, cursor, 20)?;
                Ok(())
            })?;
        }
    }
    measure(&mut samples, "sssp_cached", a.repetitions, a.warmup, || {
        let result = index.sssp(&origin, GraphDirection::Outgoing)?;
        assert_eq!(
            result.distance(index.node_index(&origin).unwrap()),
            Some(0.)
        );
        Ok(())
    })?;
    measure(&mut samples, "bfs_cached", a.repetitions, a.warmup, || {
        index.bfs(
            &origin,
            &GraphBfsOptions::new(3, Some(100), None, GraphDirection::Both),
        )?;
        Ok(())
    })?;
    measure(&mut samples, "wcc_cached", a.repetitions, a.warmup, || {
        let _ = index.wcc();
        Ok(())
    })?;
    measure(
        &mut samples,
        "sssp_parallel_round",
        a.repetitions,
        a.warmup,
        || {
            std::thread::scope(|scope| {
                let jobs: Vec<_> = (0..a.concurrency)
                    .map(|_| {
                        scope.spawn(|| {
                            index
                                .sssp(&origin, GraphDirection::Outgoing)
                                .map_err(strata_island::error::IslandError::from)
                        })
                    })
                    .collect();
                for job in jobs {
                    job.join().expect("analysis thread")?;
                }
                Ok::<(), strata_island::error::IslandError>(())
            })?;
            Ok(())
        },
    )?;
    let mut semantic_counts = None;
    if curated.is_some() {
        let semantic_name = GraphName::new(places::GRAPH)?;
        let semantic = graph.adjacency_index(&semantic_name, &budget)?;
        semantic_counts =
            Some(json!({"nodes":semantic.node_count(),"edges":semantic.edge_count()}));
        let kind = GraphTypeName::new("landmark")?;
        let mut typed = Vec::new();
        let mut cursor = None;
        loop {
            let page = graph.nodes_by_type(&semantic_name, &kind, cursor.as_ref(), 1000)?;
            typed.extend(page.nodes().iter().map(|n| n.node_id().clone()));
            if !page.has_more() {
                break;
            }
            cursor = page.cursor().cloned();
        }
        for (label, cursor) in [
            ("places_typed_first_storage", None),
            ("places_typed_middle_storage", Some(&typed[typed.len() / 2])),
            ("places_typed_last_storage", Some(&typed[typed.len() - 2])),
        ] {
            measure(&mut samples, label, a.repetitions, a.warmup, || {
                graph.nodes_by_type(&semantic_name, &kind, cursor, 20)?;
                Ok(())
            })?;
        }
        measure(
            &mut samples,
            "places_snapshot_storage",
            a.repetitions.min(20),
            a.warmup,
            || {
                graph.adjacency_index(&semantic_name, &budget)?;
                Ok(())
            },
        )?;
        let seed = &typed[0];
        measure(
            &mut samples,
            "places_bfs_subgraph_cached",
            a.repetitions,
            a.warmup,
            || {
                let bfs = semantic.bfs(
                    seed,
                    &GraphBfsOptions::new(2, Some(100), None, GraphDirection::Both),
                )?;
                let ids: Vec<_> = bfs
                    .visited()
                    .iter()
                    .map(|i| semantic.node_id(*i).unwrap().clone())
                    .collect();
                let _ = semantic.subgraph(&ids);
                Ok(())
            },
        )?;
    }
    let first = index
        .outgoing(index.node_index(&origin).unwrap())
        .first()
        .ok_or("origin has no edges")?;
    let dst = index.node_id(first.neighbor()).unwrap();
    let typ = index.edge_type_name(first.edge_type()).unwrap();
    let original = graph
        .get_edge(&name, &origin, typ, dst)?
        .unwrap()
        .data()
        .clone();
    let batch = GraphBatchWrite::new(vec![GraphBatchOperation::UpsertEdge {
        src: origin.clone(),
        edge_type: typ.clone(),
        dst: dst.clone(),
        data: original,
    }]);
    measure(
        &mut samples,
        "one_edge_batch_storage",
        a.repetitions,
        a.warmup,
        || {
            graph.batch_write(&name, &batch)?;
            Ok(())
        },
    )?;
    drop(graph);
    let mut fork_ms = Vec::new();
    let mut compare_ms = Vec::new();
    let mut delete_ms = Vec::new();
    for i in 0..a.branches {
        let child = BranchName::new(format!("stress-{i}"))?;
        let t = Instant::now();
        db.branches()?.fork_current(&branch, child.clone())?;
        fork_ms.push(t.elapsed().as_secs_f64() * 1000.);
        let t = Instant::now();
        let comparison = db.branches()?.compare(
            &branch,
            &child,
            stratadb::branch::BranchStateSelector::Current,
        )?;
        assert!(comparison.is_empty());
        compare_ms.push(t.elapsed().as_secs_f64() * 1000.);
    }
    for i in 0..a.branches {
        let t = Instant::now();
        db.branches()?
            .delete(&BranchName::new(format!("stress-{i}"))?)?;
        delete_ms.push(t.elapsed().as_secs_f64() * 1000.);
    }
    samples.insert("fork_storage".into(), fork_ms);
    samples.insert("compare_storage".into(), compare_ms);
    samples.insert("archive_storage".into(), delete_ms);
    let stats:BTreeMap<String,Value>=samples.into_iter().map(|(name,mut values)|{values.sort_by(f64::total_cmp);let n=values.len();let percentile=|p:f64|values[((n-1) as f64*p).ceil() as usize];(name,json!({"samples":n,"p50_ms":percentile(0.5),"p95_ms":if n>=20{Some(percentile(0.95))}else{None},"p99_ms":if n>=100{Some(percentile(0.99))}else{None},"raw_ms":values}))}).collect();
    let proc_status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let rss = proc_status
        .lines()
        .find(|s| s.starts_with("VmHWM:"))
        .unwrap_or("unavailable");
    let git = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .lines()
        .find(|s| s.starts_with("model name"))
        .unwrap_or("unavailable")
        .to_owned();
    let source_hash = format!(
        "{:016x}",
        strata_island::extract::fnv1a64(
            concat!(
                include_str!("island-stress.rs"),
                include_str!("../places.rs"),
                include_str!("../store.rs"),
                include_str!("../scenario.rs")
            )
            .as_bytes()
        )
    );
    let report = json!({"profile":a.profile,"mode":a.mode,"durability":if a.mode=="durable"{"always"}else{"cache"},"seed":a.seed,"chunk":a.chunk,"memory_mb":a.memory_mb,"warmup":a.warmup,"branches":a.branches,"concurrency":a.concurrency,"nodes":info.node_count(),"edges":info.edge_count(),"semantic_graph":semantic_counts,"engine":"v1.2.3","engine_commit":"6fc481c33473efd7d1724284107b67be08625dcd","app_commit":git,"app_source_fnv1a64":source_hash,"catalog_fnv1a64":format!("{:016x}",strata_island::extract::fnv1a64(curated.map(|(fixture, _)| fixture).unwrap_or(places::FIXTURE).as_bytes())),"build":if cfg!(debug_assertions){"debug"}else{"release"},"os":std::env::consts::OS,"cpu":cpu,"peak_rss":rss,"db_bytes":a.db.as_ref().map(|p|bytes(p)),"import_ms":import_ms,"first_snapshot_ms":snapshot_ms,"workloads":stats,"limitations":"Storage calls are single-client; sssp_parallel_round runs the configured concurrent jobs and includes thread scheduling. Snapshot samples capped at 20. Cold import is one sample per process. No cancellation or scanned-row counters. Run independent processes for repeat and cold-open studies."});
    if let Some(parent) = a.report.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&a.report, serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{}: {} nodes / {} edges → {}",
        a.profile,
        info.node_count(),
        info.edge_count(),
        a.report.display()
    );
    Ok(())
}
