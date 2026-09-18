//! API gaps, friction, and bugs surfaced while driving Strata from this demo.

use serde::Serialize;
use std::sync::Mutex;
use stratadb::EngineError;

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
        }
    }
}

pub struct Log {
    inner: Mutex<Vec<Finding>>,
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
            "Every service method takes &mut self on Database. Twenty colonies on twenty branches cannot commit in parallel in one process: they queue on a single Mutex. Different branches still share one writer. IPC would serialize at the host too.",
        ),
        Finding::new(
            Kind::Friction,
            "engine.Database",
            "Capability services cannot be held together",
            "kv(), json(), event(), graph(), and branches() each borrow the database mutably. A tick that writes the board, the status document, and the generation event must be three sequential service acquisitions — three commits, not one.",
        ),
        Finding::new(
            Kind::Gap,
            "engine.commit",
            "No cross-capability atomic commit",
            "There is no public way to put a KV row, a JSON document, and an event in one CommitPlan. A crash between the KV put and the event append leaves a colony with a board that has no matching tick in the log.",
        ),
        Finding::new(
            Kind::Friction,
            "engine.kv",
            "KvKey is bytes-only; the README passes &str",
            "KvKey::new takes impl Into<Vec<u8>>. A string literal does not implement that, so KvKey::new(\"board\") does not compile. Callers write KvKey::new(b\"board\".as_slice()). stratadb's crate docs still show KvKey::new(\"greeting\").",
        ),
        Finding::new(
            Kind::Friction,
            "engine.Database",
            "kv/json/event take BranchName and ProductSpace by value",
            "Every persist clones the branch name and space. A &BranchName would match how rarely those values change during a session.",
        ),
        Finding::new(
            Kind::Note,
            "engine.kv",
            "put_batch pre-reads every key to flag create vs update",
            "The engine probes latest visibility for each key before committing so the outcome can say created=true/false. A board blob is one probe. A sparse-cell stress tick of hundreds of births is hundreds of extra reads on the hot path.",
        ),
        Finding::new(
            Kind::Gap,
            "engine.event",
            "Event logs cannot be truncated",
            "Resetting a colony cannot rewind its append-only log in place. The demo deletes the mut-* branches and re-forks from the immutable seed, because there is no compact/truncate/reset on EventService.",
        ),
        Finding::new(
            Kind::Note,
            "engine.event",
            "Event payloads must be JSON objects",
            "A packed board wants to be bytes. EventPayload::new rejects arrays and strings, so the demo base64-encodes the bitset inside an object. Fine, but it is an extra encoding for a binary generation snapshot.",
        ),
        Finding::new(
            Kind::Gap,
            "engine.branch",
            "Cherry-pick and revert are still absent",
            "A natural 'undo this lie' would be revert of the perturbation commit. V1 exposes fork, compare, preview, and promote (merge). The remaining mutating ops are intentionally refused.",
        ),
        Finding::new(
            Kind::Gap,
            "engine.open / ipc",
            "A library-opened database does not host IPC",
            "Database::open_local takes the exclusive lock and never starts the Unix-socket broker. While this demo holds the directory, `strata ./colonies-db branch list` retries on EAGAIN and ends as unavailable.engine.persistence. --ipc off still needs the lock. There is no read-only peek at a process-embedded database.",
        ),
    ]
}
