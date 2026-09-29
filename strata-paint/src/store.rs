//! Every database call the app makes.
//!
//! The shape of the thing:
//!
//! 1. A stroke is one commit. `stroke/<seq>` holds the stroke and `head` holds
//!    the count, and they go in together through `put_batch`, so the timeline
//!    has exactly one entry per stroke rather than two.
//! 2. The timeline is not remembered by the app. `get_versions("head")` asks
//!    the database what its own history is, which is why a scrub still works
//!    after a reload of the page that drew it, and why the slider cannot drift
//!    out of step with what was actually committed.
//! 3. A scrub is an as-of read. `list_at(prefix, timestamp)` returns the keys
//!    that existed at that commit; each is then read at latest, which is the
//!    same answer because a stroke is immutable once written.
//!
//! 4. Drawing in the past forks. `fork_at_timestamp` starts a new branch from
//!    the commit being viewed, and the brush moves to it. A fork inherits the
//!    history it was taken from, so the scrubber works on it immediately.
//!
//! What is deliberately not here: merging. Two timelines, either of which you
//! can paint on; promotion is KSP's demo, not this one.

use serde_json::Value;
use stratadb::kv::{KvKey, KvValue};
use stratadb::{BranchName, CacheOpenOptions, Database, EngineError, ProductSpace};

use crate::stroke::Stroke;

pub const SPACE: &str = "paint";
pub const BRANCH: &str = "default";
/// New timelines are named from this, numbered when the name is taken.
pub const FORK_STEM: &str = "what-if";
/// Zero-padded so a prefix scan comes back in the order the strokes were
/// drawn. Lexicographic order is the only order `list_at` promises.
pub const STROKE_PREFIX: &str = "stroke/";
pub const HEAD_KEY: &str = "head";
const SEQ_WIDTH: usize = 8;

/// One commit on the painting's timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Commit {
    /// How many strokes existed at this commit.
    pub strokes: u64,
    pub version: u64,
    /// What `--as-of` takes: a position on the logical commit timeline, not a
    /// calendar date.
    pub timestamp: u64,
}

pub fn open_cache() -> Result<Database, EngineError> {
    Ok(Database::open_cache(CacheOpenOptions::new())?.into_database())
}

fn branch(name: &str) -> Result<BranchName, EngineError> {
    BranchName::new(name)
}

fn space() -> Result<ProductSpace, EngineError> {
    ProductSpace::new(SPACE)
}

fn stroke_key(seq: u64) -> Result<KvKey, EngineError> {
    KvKey::new(format!("{STROKE_PREFIX}{seq:0SEQ_WIDTH$}").into_bytes())
}

fn head_key() -> Result<KvKey, EngineError> {
    KvKey::new(HEAD_KEY.as_bytes().to_vec())
}

/// Appends one stroke and returns the commit it landed on.
///
/// The count is read before the write rather than held in memory: two tabs on
/// one database would otherwise both think they were writing stroke 12.
pub fn append(db: &mut Database, on: &str, stroke: &Stroke) -> Result<Commit, String> {
    stroke.validate()?;
    let payload = serde_json::to_vec(stroke).map_err(|e| format!("stroke does not encode: {e}"))?;

    let next = head(db, on).map_err(engine)? + 1;
    let mut kv = db
        .kv(branch(on).map_err(engine)?, space().map_err(engine)?)
        .map_err(engine)?;
    let outcome = kv
        .put_batch([
            (stroke_key(next).map_err(engine)?, KvValue::new(payload)),
            (
                head_key().map_err(engine)?,
                KvValue::new(next.to_string().into_bytes()),
            ),
        ])
        .map_err(engine)?;

    let commit = outcome.commit();
    Ok(Commit {
        strokes: next,
        version: commit.version().as_u64(),
        timestamp: commit.timestamp().as_micros(),
    })
}

/// How many strokes the painting holds now.
pub fn head(db: &mut Database, on: &str) -> Result<u64, EngineError> {
    let mut kv = db.kv(branch(on)?, space()?)?;
    let Some(value) = kv.get(&head_key()?)? else {
        return Ok(0);
    };
    Ok(String::from_utf8_lossy(value.as_bytes())
        .trim()
        .parse()
        .unwrap_or(0))
}

/// The painting's whole history, oldest first.
///
/// `get_versions` returns newest-first, which is the right default for "what
/// happened to this key lately" and the wrong one for a scrubber, where the
/// left end of the slider is the first stroke.
pub fn timeline(db: &mut Database, on: &str) -> Result<Vec<Commit>, EngineError> {
    let mut kv = db.kv(branch(on)?, space()?)?;
    let Some(history) = kv.get_versions(&head_key()?)? else {
        return Ok(Vec::new());
    };
    let mut out: Vec<Commit> = history
        .rows()
        .iter()
        .filter(|row| !row.is_tombstone())
        .map(|row| Commit {
            strokes: row
                .value()
                .and_then(|v| String::from_utf8_lossy(v.as_bytes()).trim().parse().ok())
                .unwrap_or(0),
            version: row.version().as_u64(),
            timestamp: row.timestamp().as_micros(),
        })
        .collect();
    out.reverse();
    Ok(out)
}

/// The strokes that existed at a commit timestamp.
///
/// This is the time-travel read the whole app is built to show. It asks the
/// database for the keys visible at that point rather than replaying a prefix
/// of a log the app happens to be holding - the app could be wrong; the
/// database's answer is the painting as it actually was.
pub fn canvas_at(
    db: &mut Database,
    on: &str,
    timestamp: u64,
) -> Result<Vec<Stroke>, EngineError> {
    let prefix = KvKey::new(STROKE_PREFIX.as_bytes().to_vec())?;
    let mut kv = db.kv(branch(on)?, space()?)?;
    let keys = kv.list_at(Some(&prefix), stratadb::Timestamp::from_micros(timestamp))?;
    read_keys(&mut kv, keys)
}

/// The painting as it is now.
pub fn canvas_now(db: &mut Database, on: &str) -> Result<Vec<Stroke>, EngineError> {
    let prefix = KvKey::new(STROKE_PREFIX.as_bytes().to_vec())?;
    let mut kv = db.kv(branch(on)?, space()?)?;
    let keys = kv.list(Some(&prefix))?;
    read_keys(&mut kv, keys)
}

/// A stroke never changes after it is written, so reading at latest returns
/// what the as-of scan already decided was visible.
fn read_keys(
    kv: &mut stratadb::kv::KvService<'_>,
    mut keys: Vec<KvKey>,
) -> Result<Vec<Stroke>, EngineError> {
    // `list_at` promises lexicographic order, but nothing in the type says
    // so and KvKey is not Ord. Sorting on the bytes keeps the strokes in the
    // order they were drawn, which for overlapping paint is the difference
    // between the picture and a different picture.
    keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    let mut out = Vec::with_capacity(keys.len());
    for key in keys {
        let Some(value) = kv.get(&key)? else { continue };
        // A stroke that will not decode is skipped rather than failing the
        // whole read: one bad row should cost one stroke, not the painting.
        if let Ok(stroke) = serde_json::from_slice::<Stroke>(value.as_bytes()) {
            out.push(stroke);
        }
    }
    Ok(out)
}

/// Strokes and commits as JSON, for the bridge.
pub fn commits_json(commits: &[Commit]) -> Value {
    Value::Array(
        commits
            .iter()
            .map(|c| {
                serde_json::json!({
                    "strokes": c.strokes,
                    "version": c.version,
                    "timestamp": c.timestamp,
                })
            })
            .collect(),
    )
}

/// Starts a new timeline from a point on an existing one.
///
/// This is `fork_at_timestamp`, which is the whole reason the app can let you
/// paint in the past: a commit on the source branch becomes the first state of
/// a new branch, and the two go their own ways from there. Forking from the
/// head would only ever give you a copy of now.
///
/// The source must still be inside the retained window - the engine will not
/// invent a state it no longer holds.
pub fn fork_at(db: &mut Database, source: &str, name: &str, timestamp: u64) -> Result<(), String> {
    db.branches()
        .map_err(engine)?
        .fork_at_timestamp(
            &branch(source).map_err(engine)?,
            branch(name).map_err(engine)?,
            stratadb::Timestamp::from_micros(timestamp),
        )
        .map_err(engine)?;
    Ok(())
}

/// Every timeline in this database, `default` first.
pub fn branch_names(db: &mut Database) -> Result<Vec<String>, EngineError> {
    let mut names: Vec<String> = db
        .branches()?
        .list()?
        .into_iter()
        .map(|summary| summary.name().as_str().to_owned())
        .collect();
    names.sort_by_key(|n| (n != BRANCH, n.clone()));
    Ok(names)
}

/// A name no existing branch has taken.
pub fn free_name(db: &mut Database) -> Result<String, EngineError> {
    let taken = branch_names(db)?;
    if !taken.iter().any(|n| n == FORK_STEM) {
        return Ok(FORK_STEM.to_owned());
    }
    for n in 2..1000 {
        let candidate = format!("{FORK_STEM}-{n}");
        if !taken.iter().any(|name| *name == candidate) {
            return Ok(candidate);
        }
    }
    Ok(format!("{FORK_STEM}-{}", taken.len() + 1))
}

fn engine(error: EngineError) -> String {
    error.to_string()
}
