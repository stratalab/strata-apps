//! Parse a vendored city JSON (8×8 grid in PR1; Manhattan in PR2).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::IslandError;
use crate::geo;

pub const NODE_CAP: usize = 20_000;

/// Frozen Manhattan extract (PR2). Recipe recorded, not retuned.
pub const EXTRACT_NODE_COUNT: usize = 12_862;
pub const EXTRACT_EDGE_COUNT: usize = 28_802;
pub const EXTRACT_BYTES: usize = 2_779_926;
pub const EXTRACT_FNV1A64: u64 = 0x036b_cf8e_7f1c_86d0;
pub const CLOSURE_EDGE_COUNT: usize = 8;

#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[derive(Clone, Debug)]
pub struct Extract {
    pub attribution: String,
    pub nodes: Vec<ExtractNode>,
    pub edges: Vec<ExtractEdge>,
}

#[derive(Clone, Debug)]
pub struct ExtractNode {
    pub id: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug)]
pub struct ExtractEdge {
    pub src: String,
    pub dst: String,
    pub length_m: u32,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClosureFixture {
    pub name: String,
    pub between: Vec<String>,
    pub edges: Vec<ClosureEdge>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClosureEdge {
    pub src: String,
    pub edge_type: String,
    pub dst: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GazetteerPoi {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub node: String,
}

pub fn parse_city(bytes: &str) -> Result<Extract, IslandError> {
    let value: Value = serde_json::from_str(bytes)
        .map_err(|_| IslandError::code("invalid_argument.island.extract"))?;
    let object = value
        .as_object()
        .ok_or(IslandError::code("invalid_argument.island.extract"))?;
    let attribution = object
        .get("attribution")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let nodes_v = object
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or(IslandError::code("invalid_argument.island.extract"))?;
    let edges_v = object
        .get("edges")
        .and_then(Value::as_array)
        .ok_or(IslandError::code("invalid_argument.island.extract"))?;

    if nodes_v.is_empty() {
        return Err(IslandError::code("invalid_argument.island.extract"));
    }
    if nodes_v.len() > NODE_CAP {
        return Err(IslandError::code("failed_precondition.island.import_cap"));
    }

    let mut nodes = Vec::with_capacity(nodes_v.len());
    let mut seen = BTreeSet::new();
    for node in nodes_v {
        let id = node
            .get("id")
            .and_then(Value::as_str)
            .ok_or(IslandError::code("invalid_argument.island.extract"))?
            .to_owned();
        if !seen.insert(id.clone()) {
            return Err(IslandError::code("invalid_argument.island.extract"));
        }
        let x = json_i32(node.get("x"))?;
        let y = json_i32(node.get("y"))?;
        if !geo::in_aabb(x, y) {
            return Err(IslandError::code("invalid_argument.island.extract"));
        }
        nodes.push(ExtractNode { id, x, y });
    }

    let mut by_id: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        by_id.insert(node.id.as_str(), index);
    }

    let mut edges = Vec::with_capacity(edges_v.len());
    let mut directed = BTreeSet::new();
    for edge in edges_v {
        let src = edge
            .get("src")
            .and_then(Value::as_str)
            .ok_or(IslandError::code("invalid_argument.island.extract"))?
            .to_owned();
        let dst = edge
            .get("dst")
            .and_then(Value::as_str)
            .ok_or(IslandError::code("invalid_argument.island.extract"))?
            .to_owned();
        if src == dst {
            return Err(IslandError::code("invalid_argument.island.extract"));
        }
        if !by_id.contains_key(src.as_str()) || !by_id.contains_key(dst.as_str()) {
            return Err(IslandError::code("invalid_argument.island.extract"));
        }
        if !directed.insert((src.clone(), dst.clone())) {
            return Err(IslandError::code("invalid_argument.island.extract"));
        }
        let length_m = json_u32(edge.get("length_m"))?;
        if length_m == 0 {
            return Err(IslandError::code("invalid_argument.island.edge_length"));
        }
        let name = edge
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|s| !s.is_empty());
        edges.push(ExtractEdge {
            src,
            dst,
            length_m,
            name,
        });
    }

    Ok(Extract {
        attribution,
        nodes,
        edges,
    })
}

fn json_i32(value: Option<&Value>) -> Result<i32, IslandError> {
    let value = value.ok_or(IslandError::code("invalid_argument.island.extract"))?;
    value
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .ok_or(IslandError::code("invalid_argument.island.extract"))
}

fn json_u32(value: Option<&Value>) -> Result<u32, IslandError> {
    let value = value.ok_or(IslandError::code("invalid_argument.island.extract"))?;
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(IslandError::code("invalid_argument.island.edge_length"))
}

pub fn parse_closure(bytes: &str) -> Result<ClosureFixture, IslandError> {
    serde_json::from_str(bytes).map_err(|_| IslandError::code("invalid_argument.island.extract"))
}

pub fn parse_gazetteer(bytes: &str) -> Result<Vec<GazetteerPoi>, IslandError> {
    serde_json::from_str(bytes).map_err(|_| IslandError::code("invalid_argument.island.extract"))
}

pub fn poi_from_value(value: &Value) -> Result<GazetteerPoi, IslandError> {
    let str_field = |key: &str| -> Result<String, IslandError> {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(IslandError::code("failed_precondition.island.city"))
    };
    Ok(GazetteerPoi {
        id: str_field("id")?,
        name: str_field("name")?,
        kind: str_field("kind")?,
        node: str_field("node")?,
    })
}

#[must_use]
pub fn name_join(extract: &Extract) -> HashMap<(String, String), String> {
    let mut names = HashMap::new();
    for edge in &extract.edges {
        if let Some(name) = &edge.name {
            names.insert((edge.src.clone(), edge.dst.clone()), name.clone());
        }
    }
    names
}
