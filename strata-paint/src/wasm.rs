//! The browser build.
//!
//! JavaScript holds the `Paint` handle and calls into it; there is no server
//! and no transport. The database is the in-memory cache, so the painting
//! lives as long as the tab - which is also true of the history, and is the
//! honest cost of running a database inside a page.
//!
//! Every method returns a JSON string rather than a bound struct: the shapes
//! here are small, they change as the UI does, and one `JSON.parse` on the
//! other side is cheaper to keep in step than a wasm-bindgen type per shape.

use serde_json::json;
use stratadb::Database;
use wasm_bindgen::prelude::*;

use crate::store::{self, Commit};
use crate::stroke::Stroke;

#[wasm_bindgen]
pub struct Paint {
    db: Database,
    /// The timeline being painted on. Every call below acts on this one.
    on: String,
}

fn fail(message: &str) -> JsValue {
    JsValue::from_str(message)
}

#[wasm_bindgen]
impl Paint {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Paint, JsValue> {
        let db = store::open_cache().map_err(|e| fail(&e.to_string()))?;
        Ok(Paint {
            db,
            on: store::BRANCH.to_owned(),
        })
    }

    /// The timeline currently being painted on.
    pub fn branch(&self) -> String {
        self.on.clone()
    }

    /// Every timeline in this database, `default` first.
    pub fn branches(&mut self) -> Result<String, JsValue> {
        let names = store::branch_names(&mut self.db).map_err(|e| fail(&e.to_string()))?;
        Ok(json!(names).to_string())
    }

    /// Switches which timeline the brush and the scrubber are looking at.
    pub fn use_branch(&mut self, name: &str) -> Result<(), JsValue> {
        let names = store::branch_names(&mut self.db).map_err(|e| fail(&e.to_string()))?;
        if !names.iter().any(|n| n == name) {
            return Err(fail(&format!("no branch named {name}")));
        }
        self.on = name.to_owned();
        Ok(())
    }

    /// Starts a new timeline from a commit on this one, and switches to it.
    ///
    /// The answer carries the new name because the app does not get to choose
    /// it: the store picks one nothing else has taken.
    pub fn fork_at(&mut self, timestamp: f64) -> Result<String, JsValue> {
        let name = store::free_name(&mut self.db).map_err(|e| fail(&e.to_string()))?;
        let source = self.on.clone();
        store::fork_at(&mut self.db, &source, &name, timestamp as u64).map_err(|e| fail(&e))?;
        self.on = name.clone();
        Ok(json!({ "branch": name, "from": source, "at": timestamp }).to_string())
    }

    /// Commits one stroke. Takes the stroke as JSON and answers with the
    /// commit it landed on, which is what the scrubber pins its new tick to.
    pub fn stroke(&mut self, stroke_json: &str) -> Result<String, JsValue> {
        let stroke: Stroke =
            serde_json::from_str(stroke_json).map_err(|e| fail(&format!("bad stroke: {e}")))?;
        let commit = store::append(&mut self.db, &self.on, &stroke).map_err(|e| fail(&e))?;
        Ok(json!({
            "strokes": commit.strokes,
            "version": commit.version,
            "timestamp": commit.timestamp,
        })
        .to_string())
    }

    /// The database's own commit history for this painting, oldest first.
    pub fn timeline(&mut self) -> Result<String, JsValue> {
        let commits = store::timeline(&mut self.db, &self.on).map_err(|e| fail(&e.to_string()))?;
        Ok(store::commits_json(&commits).to_string())
    }

    /// The painting as it was at a commit timestamp. The scrub.
    pub fn canvas_at(&mut self, timestamp: f64) -> Result<String, JsValue> {
        let strokes = store::canvas_at(&mut self.db, &self.on, timestamp as u64)
            .map_err(|e| fail(&e.to_string()))?;
        Ok(serde_json::to_string(&strokes).map_err(|e| fail(&e.to_string()))?)
    }

    /// The painting as it is now.
    pub fn canvas(&mut self) -> Result<String, JsValue> {
        let strokes = store::canvas_now(&mut self.db, &self.on).map_err(|e| fail(&e.to_string()))?;
        Ok(serde_json::to_string(&strokes).map_err(|e| fail(&e.to_string()))?)
    }

    /// What to print next to the timeline: how much history there is.
    pub fn stats(&mut self) -> Result<String, JsValue> {
        let commits: Vec<Commit> =
            store::timeline(&mut self.db, &self.on).map_err(|e| fail(&e.to_string()))?;
        let head = store::head(&mut self.db, &self.on).map_err(|e| fail(&e.to_string()))?;
        Ok(json!({
            "branch": self.on,
            "strokes": head,
            "commits": commits.len(),
            "first": commits.first().map(|c| c.timestamp),
            "last": commits.last().map(|c| c.timestamp),
        })
        .to_string())
    }
}
