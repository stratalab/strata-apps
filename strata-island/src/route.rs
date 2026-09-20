//! Directed shortest path on a RAM drive index.
//!
//! Integer Dijkstra with predecessors. Engine `sssp` is distances only
//! (#3456). Keep this module integer-only. `tests/geo.rs` greps the source.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::drive::DriveIndex;
use crate::error::IslandError;
use crate::extract::ClosureEdge;

#[derive(Clone, Debug)]
pub struct Path {
    pub nodes: Vec<String>,
    pub points: Vec<(i32, i32)>,
    pub length_m: u32,
}

pub fn route(index: &DriveIndex, src: usize, dst: usize) -> Result<Path, IslandError> {
    let n = index.node_ids.len();
    if src >= n || dst >= n {
        return Err(IslandError::code("not_found.island.node"));
    }
    if src == dst {
        return Ok(Path {
            nodes: vec![index.node_ids[src].clone()],
            points: vec![index.xy[src]],
            length_m: 0,
        });
    }

    let mut dist: Vec<Option<u32>> = vec![None; n];
    let mut pred: Vec<Option<usize>> = vec![None; n];
    dist[src] = Some(0);
    let mut heap = BinaryHeap::new();
    heap.push(Reverse((0_u32, src)));

    while let Some(Reverse((cost, u))) = heap.pop() {
        if u == dst {
            break;
        }
        let Some(known) = dist[u] else {
            continue;
        };
        if cost > known {
            continue;
        }
        for edge in &index.outgoing[u] {
            let cand = cost.saturating_add(edge.length_m);
            let better = match dist[edge.dst] {
                None => true,
                Some(cur) if cand < cur => true,
                Some(cur) if cand == cur => pred[edge.dst].is_none_or(|p| u < p),
                Some(_) => false,
            };
            if better {
                dist[edge.dst] = Some(cand);
                pred[edge.dst] = Some(u);
                heap.push(Reverse((cand, edge.dst)));
            }
        }
    }

    let Some(length_m) = dist[dst] else {
        return Err(IslandError::code("failed_precondition.island.unreachable"));
    };

    let mut chain = Vec::new();
    let mut cur = dst;
    loop {
        chain.push(cur);
        if cur == src {
            break;
        }
        let Some(prev) = pred[cur] else {
            return Err(IslandError::code("failed_precondition.island.unreachable"));
        };
        cur = prev;
    }
    chain.reverse();

    let nodes = chain.iter().map(|&i| index.node_ids[i].clone()).collect();
    let points = chain.iter().map(|&i| index.xy[i]).collect();
    Ok(Path {
        nodes,
        points,
        length_m,
    })
}

#[must_use]
pub fn uses_closed(path: &Path, closed: &[ClosureEdge]) -> bool {
    path.nodes.windows(2).any(|pair| {
        closed
            .iter()
            .any(|edge| edge.src == pair[0] && edge.dst == pair[1])
    })
}
