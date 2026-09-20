//! Minimal engine-only reproducer for graph-heavy branch fork cost.
#![allow(clippy::result_large_err)] // Preserve the engine-only reproduction.
use std::time::Instant;
use stratadb::graph::*;
use stratadb::{BranchName, CacheOpenOptions, Database, ProductSpace};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count: usize = std::env::args().nth(1).unwrap_or("10000".into()).parse()?;
    assert!((1000..=100000).contains(&count));
    let mut db = Database::open_cache(CacheOpenOptions::new())?.into_database();
    let parent = BranchName::new("default")?;
    let mut graph = db.graph(parent.clone(), ProductSpace::new("probe")?)?;
    let name = GraphName::new("roads")?;
    graph.create_graph(name.clone())?;
    let nodes: Vec<_> = (0..count)
        .map(|i| {
            Ok((
                GraphNodeId::new(format!("n:{i:06}"))?,
                GraphNodeData::default(),
            ))
        })
        .collect::<Result<_, stratadb::EngineError>>()?;
    let mut edges = Vec::new();
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
                GraphEdgeData::default(),
            ));
        }
    }
    graph.bulk_insert(&name, &nodes, &edges, Some(512))?;
    drop(graph);
    for i in 0..3 {
        let t = Instant::now();
        db.branches()?
            .fork_current(&parent, BranchName::new(format!("child-{i}"))?)?;
        println!(
            "nodes={count} edges={} fork_ms={:.3}",
            edges.len(),
            t.elapsed().as_secs_f64() * 1000.
        );
    }
    Ok(())
}
