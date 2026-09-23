//! Parts catalog, ordered stack, staging mass.
//!
//! A stage is the prefix through the lowest remaining decoupler, or the
//! entire remaining stack if there is no decoupler. Lowest stage fires first.

use crate::physics::G0;
use crate::snapshot::{CatalogPartView, PartView, VabView};
use serde_json::{json, Value};

/// Frozen at PR9 with the golden fixture. Do not bump; a retune re-records
/// `fixtures/sounding-stick-ascent1.json`. Mismatch with `./ksp-db` refuses boot.
pub const CATALOG_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PartKind {
    Capsule,
    Tank,
    Engine,
    Decoupler,
    Fin,
}

impl PartKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capsule => "capsule",
            Self::Tank => "tank",
            Self::Engine => "engine",
            Self::Decoupler => "decoupler",
            Self::Fin => "fin",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PartDef {
    pub id: &'static str,
    pub kind: PartKind,
    pub dry_kg: f64,
    pub fuel_cap_kg: f64,
    pub thrust_n: f64,
    pub fuel_rate_kg_s: f64,
    /// Display only in v1. Integration uses thrust_n / fuel_rate.
    pub isp: Option<f64>,
}

pub const CATALOG: &[PartDef] = &[
    PartDef {
        id: "mk1-pod",
        kind: PartKind::Capsule,
        dry_kg: 0.80,
        fuel_cap_kg: 0.0,
        thrust_n: 0.0,
        fuel_rate_kg_s: 0.0,
        isp: None,
    },
    PartDef {
        id: "tank-s",
        kind: PartKind::Tank,
        dry_kg: 0.30,
        fuel_cap_kg: 0.90,
        thrust_n: 0.0,
        fuel_rate_kg_s: 0.0,
        isp: None,
    },
    PartDef {
        id: "tank-m",
        kind: PartKind::Tank,
        dry_kg: 0.50,
        fuel_cap_kg: 1.80,
        thrust_n: 0.0,
        fuel_rate_kg_s: 0.0,
        isp: None,
    },
    PartDef {
        id: "lvt-30",
        kind: PartKind::Engine,
        dry_kg: 0.40,
        fuel_cap_kg: 0.0,
        thrust_n: 80.0,
        fuel_rate_kg_s: 0.22,
        isp: Some(14.0),
    },
    PartDef {
        id: "terrier",
        kind: PartKind::Engine,
        dry_kg: 0.25,
        fuel_cap_kg: 0.0,
        thrust_n: 28.0,
        fuel_rate_kg_s: 0.08,
        isp: Some(22.0),
    },
    PartDef {
        id: "stack-sep",
        kind: PartKind::Decoupler,
        dry_kg: 0.05,
        fuel_cap_kg: 0.0,
        thrust_n: 0.0,
        fuel_rate_kg_s: 0.0,
        isp: None,
    },
    PartDef {
        id: "fin",
        kind: PartKind::Fin,
        dry_kg: 0.03,
        fuel_cap_kg: 0.0,
        thrust_n: 0.0,
        fuel_rate_kg_s: 0.0,
        isp: None,
    },
];

pub fn lookup(id: &str) -> Option<&'static PartDef> {
    CATALOG.iter().find(|p| p.id == id)
}

#[derive(Clone, Debug, PartialEq)]
pub struct StackPart {
    pub part_id: String,
    /// Remaining fuel in this tank (0 for non-tanks). Starts at fuel_cap.
    pub fuel: f64,
}

impl StackPart {
    pub fn new(part_id: impl Into<String>) -> Option<Self> {
        let part_id = part_id.into();
        let def = lookup(&part_id)?;
        Some(Self {
            part_id,
            fuel: def.fuel_cap_kg,
        })
    }

    pub fn def(&self) -> &'static PartDef {
        lookup(&self.part_id).expect("catalog id")
    }
}

/// Ordered stack, bottom (index 0) to top.
#[derive(Clone, Debug, PartialEq)]
pub struct CraftSpec {
    pub name: String,
    pub parts: Vec<StackPart>,
}

impl CraftSpec {
    pub fn sounding_stick() -> Self {
        Self::from_ids(
            "Sounding Stick",
            &[
                "lvt-30",
                "tank-m",
                "stack-sep",
                "terrier",
                "tank-s",
                "stack-sep",
                "mk1-pod",
            ],
        )
        .expect("catalog")
    }

    pub fn from_ids(name: impl Into<String>, ids: &[&str]) -> Option<Self> {
        let mut parts = Vec::with_capacity(ids.len());
        for id in ids {
            parts.push(StackPart::new(*id)?);
        }
        Some(Self {
            name: name.into(),
            parts,
        })
    }

    pub fn dry_mass(&self) -> f64 {
        self.parts.iter().map(|p| p.def().dry_kg).sum()
    }

    pub fn fuel(&self) -> f64 {
        self.parts.iter().map(|p| p.fuel).sum()
    }

    pub fn fuel_cap(&self) -> f64 {
        self.parts.iter().map(|p| p.def().fuel_cap_kg).sum()
    }

    pub fn wet_mass(&self) -> f64 {
        self.dry_mass() + self.fuel()
    }

    /// Index of the first decoupler, or `parts.len()` if none.
    pub fn current_stage_end(&self) -> usize {
        self.parts
            .iter()
            .position(|p| p.def().kind == PartKind::Decoupler)
            .map_or(self.parts.len(), |i| i + 1)
    }

    pub fn current_stage_has_decoupler(&self) -> bool {
        self.parts
            .iter()
            .any(|p| p.def().kind == PartKind::Decoupler)
    }

    pub fn current_stage_thrust(&self) -> f64 {
        let end = self.current_stage_end();
        self.parts[..end].iter().map(|p| p.def().thrust_n).sum()
    }

    pub fn current_stage_fuel_rate(&self) -> f64 {
        let end = self.current_stage_end();
        self.parts[..end]
            .iter()
            .map(|p| p.def().fuel_rate_kg_s)
            .sum()
    }

    pub fn current_stage_fuel(&self) -> f64 {
        let end = self.current_stage_end();
        self.parts[..end].iter().map(|p| p.fuel).sum()
    }

    /// Spent booster still attached: engine in this stage, tanks empty, a sep remains.
    pub fn spent_engine_and_decoupler(&self) -> bool {
        if !self.current_stage_has_decoupler() {
            return false;
        }
        let end = self.current_stage_end();
        let stage = &self.parts[..end];
        let has_engine = stage.iter().any(|p| p.def().kind == PartKind::Engine);
        has_engine && self.current_stage_fuel() <= 1e-9
    }

    pub fn current_stage_is_boost(&self) -> bool {
        let end = self.current_stage_end();
        self.parts[..end].iter().any(|p| p.part_id == "lvt-30")
    }

    pub fn drain_fuel(&mut self, amount: f64) -> f64 {
        let mut left = amount.max(0.0);
        for part in &mut self.parts {
            if left <= 0.0 {
                break;
            }
            let take = part.fuel.min(left);
            part.fuel -= take;
            left -= take;
        }
        amount - left
    }

    /// Drop the current stage. Returns dropped part ids and discarded fuel.
    pub fn stage(&mut self) -> StageDrop {
        let end = self.current_stage_end();
        let dropped: Vec<StackPart> = self.parts.drain(0..end).collect();
        let ids: Vec<String> = dropped.iter().map(|p| p.part_id.clone()).collect();
        let dropped_fuel: f64 = dropped.iter().map(|p| p.fuel).sum();
        StageDrop { ids, dropped_fuel }
    }

    pub fn add(&mut self, part_id: &str, index: Option<usize>) -> Result<(), String> {
        let part = StackPart::new(part_id).ok_or_else(|| format!("unknown part `{part_id}`"))?;
        let i = index.unwrap_or(self.parts.len()).min(self.parts.len());
        self.parts.insert(i, part);
        Ok(())
    }

    /// Insert a tank (or any part) immediately above the current-stage engine.
    pub fn insert_above_current_engine(&mut self, part_id: &str) -> Result<usize, String> {
        let end = self.current_stage_end();
        let idx = self.parts[..end]
            .iter()
            .position(|p| p.def().kind == PartKind::Engine)
            .map(|i| i + 1)
            .unwrap_or(0);
        self.add(part_id, Some(idx))?;
        Ok(idx)
    }

    pub fn remove(&mut self, index: usize) -> Result<StackPart, String> {
        if index >= self.parts.len() {
            return Err("index out of range".into());
        }
        Ok(self.parts.remove(index))
    }

    /// Display Δv: rocket equation per stage using that stage's engine Isp.
    pub fn dv_budget_mps(&self) -> f64 {
        let mut remaining = self.parts.as_slice();
        let mut total = 0.0;
        while !remaining.is_empty() {
            let end = remaining
                .iter()
                .position(|p| p.def().kind == PartKind::Decoupler)
                .map_or(remaining.len(), |i| i + 1);
            let stage = &remaining[..end];
            let m0: f64 = remaining.iter().map(|p| p.def().dry_kg + p.fuel).sum();
            let stage_fuel: f64 = stage.iter().map(|p| p.fuel).sum();
            let mf = (m0 - stage_fuel).max(1e-9);
            let isp = stage
                .iter()
                .filter_map(|p| p.def().isp)
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(0.0);
            if isp > 0.0 && m0 > mf {
                total += isp * G0 * (m0 / mf).ln();
            }
            remaining = &remaining[end..];
        }
        total
    }

    pub fn node_id(ordinal: usize, part_id: &str) -> String {
        format!("p-{ordinal:02}-{part_id}")
    }

    /// Authored hangar document. Fuel is not stored; tanks refill from catalog cap on load.
    pub fn to_doc(&self) -> Value {
        json!({
            "name": self.name,
            "parts": self
                .parts
                .iter()
                .enumerate()
                .map(|(ordinal, part)| json!({
                    "part_id": part.part_id,
                    "ordinal": ordinal
                }))
                .collect::<Vec<_>>(),
            "dv_budget_mps": self.dv_budget_mps(),
        })
    }

    pub fn from_doc(value: &Value) -> Result<Self, String> {
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("craft")
            .to_owned();
        let mut entries: Vec<(u32, String)> = Vec::new();
        let parts = value
            .get("parts")
            .and_then(Value::as_array)
            .ok_or_else(|| "craft document missing parts array".to_owned())?;
        for (i, part) in parts.iter().enumerate() {
            let part_id = part
                .get("part_id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("part {i} missing part_id"))?
                .to_owned();
            if lookup(&part_id).is_none() {
                return Err(format!("unknown part `{part_id}`"));
            }
            let ordinal = part
                .get("ordinal")
                .and_then(Value::as_u64)
                .map(|n| n as u32)
                .unwrap_or(i as u32);
            entries.push((ordinal, part_id));
        }
        entries.sort_by_key(|(ordinal, _)| *ordinal);
        let mut parts = Vec::with_capacity(entries.len());
        for (_, part_id) in entries {
            parts.push(StackPart::new(part_id).expect("catalog id checked"));
        }
        Ok(Self { name, parts })
    }

    /// Stage index of the part at `ordinal` (0 = first prefix through a decoupler).
    pub fn stage_of(&self, ordinal: usize) -> u32 {
        let mut stage = 0u32;
        for (i, part) in self.parts.iter().enumerate() {
            if i == ordinal {
                return stage;
            }
            if part.def().kind == PartKind::Decoupler {
                stage += 1;
            }
        }
        stage
    }

    pub fn to_vab_view(&self) -> VabView {
        VabView {
            name: self.name.clone(),
            thrust_n: self.current_stage_thrust(),
            parts: self
                .parts
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let d = p.def();
                    PartView {
                        part_id: p.part_id.clone(),
                        ordinal: i as u32,
                        kind: d.kind.as_str().to_owned(),
                        dry_kg: d.dry_kg,
                        fuel_kg: p.fuel,
                    }
                })
                .collect(),
            catalog: CATALOG
                .iter()
                .map(|d| CatalogPartView {
                    part_id: d.id.to_owned(),
                    kind: d.kind.as_str().to_owned(),
                    dry_kg: d.dry_kg,
                    fuel_cap_kg: d.fuel_cap_kg,
                    thrust_n: d.thrust_n,
                })
                .collect(),
            wet_mass: self.wet_mass(),
            dry_mass: self.dry_mass(),
            fuel: self.fuel(),
            dv_budget_mps: self.dv_budget_mps(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StageDrop {
    pub ids: Vec<String>,
    pub dropped_fuel: f64,
}
