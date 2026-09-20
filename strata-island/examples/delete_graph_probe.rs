//! Engine-only reproducer: graph deletion exceeds durable commit row limits.
#![allow(clippy::result_large_err)]
use stratadb::graph::*;
use stratadb::{BranchName, Database, DurabilityMode, DurableLocalOpenOptions, ProductSpace};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let count: usize = std::env::args().nth(1).unwrap_or("3000".into()).parse()?;
    let db = Database::open_local(
        dir.path(),
        DurableLocalOpenOptions::new().with_durability(DurabilityMode::Always),
    )?
    .into_database();
    let mut graph = db.graph(BranchName::new("default")?, ProductSpace::new("probe")?)?;
    let name = GraphName::new("places")?;
    graph.create_graph(name.clone())?;
    let nodes: Vec<_> = (0..count)
        .map(|i| {
            Ok((
                GraphNodeId::new(format!("n:{i}"))?,
                GraphNodeData::default(),
            ))
        })
        .collect::<Result<_, stratadb::EngineError>>()?;
    let edges: Vec<_> = (0..count)
        .map(|i| {
            Ok((
                nodes[i].0.clone(),
                GraphEdgeType::new("next")?,
                nodes[(i + 1) % count].0.clone(),
                GraphEdgeData::default(),
            ))
        })
        .collect::<Result<_, stratadb::EngineError>>()?;
    graph.bulk_insert(&name, &nodes, &edges, Some(512))?;
    let result = graph.delete_graph(&name);
    println!("nodes={count} edges={count} delete={result:?}");
    if let Err(e) = &result {
        let mut source = std::error::Error::source(e);
        while let Some(s) = source {
            println!("caused by: {s:?}");
            source = s.source();
        }
    }
    result?;
    Ok(())
}
