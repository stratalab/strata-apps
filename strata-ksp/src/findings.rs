//! Known engine constraints this demo designs around. Quieter than colonies.

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

    pub fn snapshot(&self) -> Vec<Finding> {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
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

/// Friction we know before the first open, from reading the public surface.
pub fn known_at_compile_time() -> Vec<Finding> {
    vec![
        Finding::new(
            Kind::Gap,
            "engine.Database",
            "One exclusive &mut handle for the whole database",
            "Every service method takes &mut self on Database. Hangar saves and launch persists queue on one Mutex. Different branches still share one writer.",
        )
        .issue(3126),
        Finding::new(
            Kind::Friction,
            "engine.Database",
            "Capability services cannot be held together",
            "kv(), json(), event(), graph(), and branches() each borrow the database mutably. A VAB save is three sequential acquisitions — JSON, graph, event — three commits, not one.",
        )
        .issue(3126),
        Finding::new(
            Kind::Gap,
            "engine.commit",
            "No cross-capability atomic commit",
            "There is no public CommitPlan that writes JSON, graph, and an event together. A crash between the spec write and the graph rebuild is a real invariant; resume re-reads JSON and rebuilds the projection.",
        )
        .issue(3127),
        Finding::new(
            Kind::Friction,
            "engine.kv",
            "KvKey is bytes-only; the README passes &str",
            "KvKey::new takes impl Into<Vec<u8>>. A string literal does not implement that, so KvKey::new(\"vessel\") does not compile. Callers write KvKey::new(b\"vessel\".as_slice()).",
        )
        .issue(3189),
        Finding::new(
            Kind::Gap,
            "engine.open / ipc",
            "A library-opened database does not host IPC",
            "Database::open_local takes the exclusive lock and never starts the Unix-socket broker. While this demo holds the directory, `strata ./ksp-db branch list` ends as unavailable.engine.persistence.",
        )
        .issue(3128),
        Finding::new(
            Kind::Note,
            "engine.branch",
            "promote carries JSON + KV only",
            "Graph and Event adapters set supports_promotion() == false. Promote this design copies the craft JSON onto vab and rebuilds the hangar graph. The tape and spent graph do not move. compare is not a dry-run of promote.",
        )
        .issue(3177),
        Finding::new(
            Kind::Gap,
            "engine.branch",
            "promote requires a direct fork or merge parent",
            "A grandchild of design-* is invalid_argument.engine.branch_point. The demo mutates design-* itself; fork-at children share that design so a second promote uses the merge edge.",
        )
        .issue(3178),
        Finding::new(
            Kind::Friction,
            "engine.graph",
            "Edges require both endpoints; batch ops apply in order",
            "GraphBatchWrite is all-or-nothing. UpsertNode ops must precede UpsertEdge ops in the same batch. Staging deletes go through DeleteNode, which drops incident edges.",
        )
        .issue(3192),
        Finding::new(
            Kind::Note,
            "engine.event",
            "range is latest-only; rewind is reconstruct",
            "EventService::range has no commit-timeline as-of. In-place rewind appends a rewind marker and walks an active segment. fork_at_version uses EventVersionedRecord::version().",
        )
        .issue(3145),
        Finding::new(
            Kind::Note,
            "engine.event",
            "No tick batch_append; seq and CommitVersion stay 1:1",
            "batch_append shares one CommitVersion across the batch, so fork_at_version cannot slice mid-batch. One tick = one append = one commit. fork_at_timestamp is unused (commit time ≠ sim t).",
        )
        .issue(3179),
        Finding::new(
            Kind::Note,
            "engine.commit",
            "Persist oneshot ack stands in for a session flush",
            "Fork-at, promote, and archive call flush_durable: enqueue with an ack, block, then take db. V1 has no CommitOutcome waiter from a second caller.",
        )
        .issue(3180),
        Finding::new(
            Kind::Note,
            "engine.event",
            "Event logs cannot be truncated",
            "Archive = branches.delete of the launch. design-* dies only at refcount 0. Promote never deletes a design.",
        )
        .issue(3129),
        Finding::new(
            Kind::Friction,
            "engine.event",
            "Payload hash is not stable across JSON number round-trip",
            "The engine hashes to_vec of the in-memory Value, stores it nested, then hashes again on decode. Physics f64s need a serialize/parse canonicalize before EventPayload::new.",
        )
        .issue(3188),
        Finding::new(
            Kind::Note,
            "engine.branch",
            "design-* is reference counted",
            "refcount is live launch-* branches whose meta.design names it. Fork-at shares the parent's design. Archive decrements; keep_snapshot skips the delete.",
        ),
        Finding::new(
            Kind::Gap,
            "engine.branch",
            "Durable delete refuses a fork source while children live",
            "Cache lets you archive launch-0001 while launch-0002 (fork_at_version child) still flies. Durable returns failed_precondition.engine.persistence (storage_api.state) to protect recovery. Archive leaves first, or the original.",
        )
        .issue(3196),
    ]
}
