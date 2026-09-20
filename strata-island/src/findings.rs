//! Engine friction this app designs around, plus anything we hit while building.
//!
//! Protocol:
//! 1. `Log::push` a finding (deduped by title). Runtime hits use `hit`.
//! 2. Add a row to `docs/friction.md`.
//! 3. File or update an issue on `stratalab/strata-core`. Do not patch the engine
//!    from this app.

use serde::Serialize;
use std::sync::Mutex;
use stratadb::EngineError;

const ISSUES: &str = "https://github.com/stratalab/strata-core/issues";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Gap,
    Friction,
    Bug,
    Note,
}

#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub kind: Kind,
    pub surface: String,
    pub title: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
}

impl Finding {
    pub fn new(
        kind: Kind,
        surface: impl Into<String>,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            surface: surface.into(),
            title: title.into(),
            detail: detail.into(),
            issue: None,
        }
    }

    pub fn issue(mut self, number: u32) -> Self {
        self.issue = Some(format!("{ISSUES}/{number}"));
        self
    }
}

pub struct Log {
    inner: Mutex<Vec<Finding>>,
}

impl Default for Log {
    fn default() -> Self {
        Self::new()
    }
}

impl Log {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
        }
    }

    pub fn push(&self, finding: Finding) {
        if let Ok(mut guard) = self.inner.lock() {
            if guard.iter().any(|existing| existing.title == finding.title) {
                return;
            }
            guard.push(finding);
        }
    }

    pub fn hit(
        &self,
        kind: Kind,
        surface: &str,
        title: &str,
        detail: impl Into<String>,
        issue: Option<u32>,
    ) {
        let mut finding = Finding::new(kind, surface, title, detail);
        if let Some(number) = issue {
            finding = finding.issue(number);
        }
        self.push(finding);
    }

    pub fn snapshot(&self) -> Vec<Finding> {
        self.inner
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| Vec::new())
    }

    pub fn from_engine(&self, kind: Kind, surface: &str, title: &str, error: &EngineError) {
        self.push(Finding::new(
            kind,
            surface,
            title,
            format!(
                "{code} ({class:?}): {message}",
                code = error.code(),
                class = error.class(),
                message = error.message()
            ),
        ));
    }
}

/// Designed-around set. Camera never calls `graph()`. New issues only
/// (island-filed; the engine team owns duplicates).
pub fn known_at_compile_time() -> Vec<Finding> {
    vec![
        Finding::new(Kind::Gap,"engine.search","Address search uses an application index","Exact and prefix lookup are derived from published Strata documents; replace the search adapter when native field/prefix queries are available (#3482, #3483).").issue(3483),
        Finding::new(Kind::Gap,"engine.json","Indexed address field queries are unavailable","The public JSON surface creates indexes but cannot query their fields. Address exact lookup is application-owned.").issue(3482),
        Finding::new(Kind::Gap,"engine.graph","Weighted graph work has no cutoff or multi-source seeds","Nearest-station and radius features use full engine SSSP and application filtering.").issue(3484),
        Finding::new(Kind::Friction,"engine.json","Historical address hydration uses point reads","The historical document and graph binding adapter can move to batch hydration when supported.").issue(3485),
        Finding::new(Kind::Gap,"engine.graph","Address publication spans separate commits","Deterministic graph/document upserts publish through a final readiness manifest; no mixed-capability transaction is available.").issue(3486),
        Finding::new(Kind::Friction,"engine.graph","Neighbor pages load complete adjacency","Address relationship results are bounded, but a high-degree street still incurs full adjacency hydration before the page limit.").issue(3489),
        Finding::new(Kind::Friction,"engine.graph","Large graph deletion exceeds commit limits","Durable delete_graph collects all graph and index rows into one commit. A 3,000-node/3,000-edge ring imports successfully but cannot be deleted. Island resumes imports through deterministic upserts instead.").issue(3477),
        Finding::new(Kind::Friction,"engine.branch","Large graph forks take seconds","Engine-only cache probe: 100k nodes / 400k edges take 26–28 seconds per fork on the reference development machine. The interactive city remains bounded.").issue(3475),
        Finding::new(Kind::Gap,"engine.graph","Weighted queries cannot filter relationship types","V2 separates street and place graphs to prevent semantic shortcuts in SSSP.").issue(3471),
        Finding::new(Kind::Friction,"engine.graph","Small graph batches load the whole graph","The stress runner measures one-edge batches at increasing graph sizes.").issue(3472),
        Finding::new(Kind::Friction,"engine.graph","Typed pages scan the full type index","The stress runner measures first, middle and last pages independently of the app cache.").issue(3473),
        Finding::new(Kind::Friction,"engine.graph","Graph metadata scans all graph records","Readiness versions come from completed JSON writes; graph_info remains an explicit audit workload.").issue(3474),
        Finding::new(
            Kind::Gap,
            "engine.graph",
            "sssp returns distances only",
            "GraphSsspResult has no predecessor pointers. PR3 Dijkstra-with-predecessors on the RAM index. Engine SSSP powers V2 destination discovery; application Dijkstra reconstructs route paths.",
        )
        .issue(3456),
        Finding::new(
            Kind::Gap,
            "engine.graph",
            "No list_edges; adjacency carries no properties",
            "GraphAdjacencyIndex is node_ids + weighted neighbors. Street names are joined from the committed extract, not from a graph scan at 30 Hz.",
        )
        .issue(3457),
        Finding::new(
            Kind::Gap,
            "engine.graph",
            "list_nodes is id-prefix, not bbox",
            "There is no spatial index. list_nodes loads every row then paginates in RAM. Pan/zoom culls a RAM snapshot. GET /api/city sends the whole island once.",
        )
        .issue(3458),
        Finding::new(
            Kind::Friction,
            "engine.graph",
            "GraphService reads still take &mut self",
            "Database::graph is &self; adjacency_index / list_nodes / neighbors / get_edge are still &mut self on the service. Camera must not take the db mutex.",
        )
        .issue(3459),
        Finding::new(
            Kind::Friction,
            "engine.graph",
            "sssp re-scans every edge for negatives",
            "Every sssp walks all outgoing edges before Dijkstra. Import already refuses non-integer meters < 1. Pure overhead on this city.",
        )
        .issue(3460),
        Finding::new(
            Kind::Note,
            "engine.graph",
            "Graph is node/edge, not turn-expanded",
            "No-left / U-turn cannot be a row. V1 is directed shortest path, not a legal Manhattan drive.",
        )
        .issue(3461),
        Finding::new(
            Kind::Gap,
            "engine.graph",
            "Graph does not promote",
            "supports_promotion is false on every graph adapter. Compare is the landing. No promote button. A legal branch promote would not carry deleted 42nd edges.",
        )
        .issue(3462),
        Finding::new(
            Kind::Friction,
            "engine.branch",
            "Durable parent delete is refused while desks live",
            "delete(city) with a live desk is failed_precondition.engine.branch_has_children. Archive desks first, oldest leaf outwards. Cache mode does not refuse — durable tests cover it.",
        ),
        Finding::new(
            Kind::Friction,
            "engine.event",
            "EventPayload must be canonicalized before new",
            "serialize→parse before EventPayload::new so object order cannot drift. One closed event per desk. verify_chain after close.",
        ),
        Finding::new(
            Kind::Gap,
            "engine.open / ipc",
            "A library-opened database does not host IPC",
            "When this process holds ./island-db, strata ./island-db branch list is unavailable.engine.persistence. Do not shell out to strata from the plat.",
        )
        .issue(3463),
        Finding::new(
            Kind::Friction,
            "engine.graph",
            "bulk_insert is many commits",
            "Chunks of 512 (max 800). Crash between chunks is a half-imported city. Resume upserts. JSON meta.status is the watermark.",
        )
        .issue(3464),
        Finding::new(
            Kind::Friction,
            "engine.graph",
            "Edge weight is f64 only",
            "Island meters are u32. Engine weight is length_m as f64 plus a JSON integer property. geo.rs / route.rs stay integer.",
        )
        .issue(3465),
        Finding::new(
            Kind::Note,
            "engine.graph",
            "Bindings cannot name another branch",
            "Gazetteer bindings omit branch (None) so a forked desk does not trip unsupported.engine.graph_binding_cross_branch.",
        )
        .issue(3466),
    ]
}
