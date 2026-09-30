//! Variations are branches.
//!
//! This is the whole reason the app exists. A chess GUI keeps its variation
//! tree in memory and writes it out as PGN with brackets in it; here a
//! variation *is* a database branch, and the tree in the sidebar is the branch
//! list read back from the engine.
//!
//! The shape is the one strata-paint uses, because it works: one commit per
//! move, `move/<4-digit ply>` holding the move and `head` holding the count,
//! both written through a single `put_batch`. Playing a different move from a
//! position you have already been to forks at that commit, which is exactly
//! what starting a variation means.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use stratadb::kv::{KvKey, KvValue};
use stratadb::{BranchName, CacheOpenOptions, Database, EngineError, ProductSpace};

pub const SPACE: &str = "chess";
pub const MAIN: &str = "default";
pub const MOVE_PREFIX: &str = "move/";
pub const HEAD_KEY: &str = "head";
const SEQ_WIDTH: usize = 4;
/// Variations are named for the ply they left the line at, so the tree reads
/// as "this is where it went differently" rather than as a list of nonce names.
pub const LINE_STEM: &str = "line";

/// One half-move, as played.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Ply {
    pub san: String,
    pub uci: String,
    /// The position *after* the move, so a scrub needs no replay.
    pub fen: String,
    pub by: String,
}

#[derive(Clone, Copy, Debug)]
pub struct Commit {
    pub plies: u64,
    pub version: u64,
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

fn ply_key(seq: u64) -> Result<KvKey, EngineError> {
    KvKey::new(format!("{MOVE_PREFIX}{seq:0SEQ_WIDTH$}").into_bytes())
}

fn head_key() -> Result<KvKey, EngineError> {
    KvKey::new(HEAD_KEY.as_bytes().to_vec())
}

fn engine(error: EngineError) -> String {
    error.to_string()
}

/// Appends one half-move to a line.
pub fn append(db: &mut Database, on: &str, ply: &Ply) -> Result<Commit, String> {
    let payload = serde_json::to_vec(ply).map_err(|e| format!("move does not encode: {e}"))?;
    let next = head(db, on).map_err(engine)? + 1;
    let mut kv = db
        .kv(branch(on).map_err(engine)?, space().map_err(engine)?)
        .map_err(engine)?;
    let outcome = kv
        .put_batch([
            (ply_key(next).map_err(engine)?, KvValue::new(payload)),
            (
                head_key().map_err(engine)?,
                KvValue::new(next.to_string().into_bytes()),
            ),
        ])
        .map_err(engine)?;
    let commit = outcome.commit();
    Ok(Commit {
        plies: next,
        version: commit.version().as_u64(),
        timestamp: commit.timestamp().as_micros(),
    })
}

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

/// The line as played, oldest first.
pub fn line(db: &mut Database, on: &str) -> Result<Vec<Ply>, EngineError> {
    let prefix = KvKey::new(MOVE_PREFIX.as_bytes().to_vec())?;
    let mut kv = db.kv(branch(on)?, space()?)?;
    let mut keys = kv.list(Some(&prefix))?;
    keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    let mut out = Vec::with_capacity(keys.len());
    for key in keys {
        let Some(value) = kv.get(&key)? else { continue };
        if let Ok(ply) = serde_json::from_slice::<Ply>(value.as_bytes()) {
            out.push(ply);
        }
    }
    Ok(out)
}

/// Every commit on a line, oldest first: the database's own history.
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
            plies: row
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

/// Starts a variation from a commit on an existing line.
///
/// `fork_at_timestamp`, the same call strata-paint uses to let you draw in the
/// past. Here it is the move that was not played.
pub fn branch_at(
    db: &mut Database,
    source: &str,
    name: &str,
    timestamp: u64,
) -> Result<(), String> {
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

pub fn lines(db: &mut Database) -> Result<Vec<String>, EngineError> {
    let mut names: Vec<String> = db
        .branches()?
        .list()?
        .into_iter()
        .map(|s| s.name().as_str().to_owned())
        .collect();
    names.sort_by_key(|n| (n != MAIN, n.clone()));
    Ok(names)
}

/// A name for a variation that leaves the line at `ply`, unused so far.
pub fn free_name(db: &mut Database, ply: u64) -> Result<String, EngineError> {
    let taken = lines(db)?;
    let base = format!("{LINE_STEM}-{ply}");
    if !taken.iter().any(|n| *n == base) {
        return Ok(base);
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !taken.iter().any(|name| *name == candidate) {
            return Ok(candidate);
        }
    }
    Ok(format!("{base}-{}", taken.len() + 1))
}

pub fn commits_json(commits: &[Commit]) -> Value {
    Value::Array(
        commits
            .iter()
            .map(|c| serde_json::json!({ "plies": c.plies, "version": c.version, "timestamp": c.timestamp }))
            .collect(),
    )
}
