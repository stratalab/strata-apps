//! RAM drive graph. Camera and Dijkstra read this. Never holds a Database.

use std::collections::HashMap;

use crate::extract::Extract;

#[derive(Clone, Debug)]
pub struct DriveEdge {
    pub dst: usize,
    pub length_m: u32,
    pub name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DriveIndex {
    pub branch: String,
    pub node_ids: Vec<String>,
    pub xy: Vec<(i32, i32)>,
    pub outgoing: Vec<Vec<DriveEdge>>,
}

impl DriveIndex {
    #[must_use]
    pub fn node_index(&self, id: &str) -> Option<usize> {
        self.node_ids.iter().position(|node| node == id)
    }

    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.outgoing.iter().map(Vec::len).sum()
    }

    #[must_use]
    pub fn has_directed(&self, src: &str, dst: &str) -> bool {
        let Some(src_i) = self.node_index(src) else {
            return false;
        };
        let Some(dst_i) = self.node_index(dst) else {
            return false;
        };
        self.outgoing[src_i].iter().any(|edge| edge.dst == dst_i)
    }
}

#[must_use]
pub fn from_extract(branch_name: &str, extract: &Extract) -> DriveIndex {
    let mut id_to_index = HashMap::new();
    let mut node_ids = Vec::with_capacity(extract.nodes.len());
    let mut xy = Vec::with_capacity(extract.nodes.len());
    for (i, node) in extract.nodes.iter().enumerate() {
        id_to_index.insert(node.id.clone(), i);
        node_ids.push(node.id.clone());
        xy.push((node.x, node.y));
    }
    let mut outgoing = vec![Vec::new(); node_ids.len()];
    for edge in &extract.edges {
        let src = id_to_index[&edge.src];
        let dst = id_to_index[&edge.dst];
        outgoing[src].push(DriveEdge {
            dst,
            length_m: edge.length_m,
            name: edge.name.clone(),
        });
    }
    DriveIndex {
        branch: branch_name.to_owned(),
        node_ids,
        xy,
        outgoing,
    }
}
