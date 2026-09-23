//! The browser build.
//!
//! Same simulation, same engine, same UI. What changes is the transport: the
//! native app answers HTTP and pushes snapshots over a WebSocket, and here
//! JavaScript holds the `World` directly and calls into it.
//!
//! `command` mirrors the route table in `main.rs` one for one, including its
//! argument defaults, so `static/app.js` speaks the same protocol to both
//! builds and there is one UI rather than two.
//!
//! The database is the in-memory cache. `localfs` is off (strata-core #3536),
//! there is no filesystem to be durable against, and everything the demo is
//! about - branches, events, promotion, as-of reads - works the same without
//! it. It lives for as long as the tab does.

use std::sync::Arc;

use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

use crate::world::{OpenArgs, World};

#[wasm_bindgen]
pub struct Ksp {
    world: Arc<World>,
}

/// The shape `persist_error` returns natively, so the client's error handling
/// is the same on both builds.
fn err(message: &str) -> String {
    let code = message
        .split_once(':')
        .map(|(code, _)| code)
        .unwrap_or("internal.ksp.persist");
    json!({ "error": { "code": code, "message": message } }).to_string()
}

fn focused_or(world: &World, explicit: Option<&str>) -> Option<String> {
    if let Some(name) = explicit.filter(|n| !n.is_empty()) {
        return Some(name.to_owned());
    }
    let snap = world.snapshot();
    (!snap.focused.is_empty()).then_some(snap.focused)
}

#[wasm_bindgen]
impl Ksp {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Ksp, JsValue> {
        let world = World::open(OpenArgs {
            cache: true,
            db_path: Default::default(),
        })
        .map_err(|e| JsValue::from_str(&e))?;
        // Nothing is flying yet, so nothing should be advancing. The native
        // build starts running because a server has no other way to be asked.
        world.set_running(false);
        Ok(Ksp {
            world: Arc::new(world),
        })
    }

    /// Advance one frame if running, and return the snapshot either way. This
    /// is the ticker in `main.rs`, called from requestAnimationFrame instead
    /// of from a tokio interval.
    pub fn tick(&self) -> String {
        if self.world.is_running() {
            self.world.tick_frame();
        }
        self.snapshot()
    }

    pub fn snapshot(&self) -> String {
        serde_json::to_string(&self.world.snapshot()).unwrap_or_else(|e| err(&e.to_string()))
    }

    pub fn hz(&self) -> f64 {
        self.world.hz()
    }

    /// One route, one call. `body` is the same JSON the native build would
    /// have received in the request body.
    pub fn command(&self, path: &str, body: &str) -> String {
        let b: Value = serde_json::from_str(body).unwrap_or_else(|_| json!({}));
        let w = &self.world;
        let str_of = |k: &str| b.get(k).and_then(Value::as_str).map(str::to_owned);
        let u64_of = |k: &str| b.get(k).and_then(Value::as_u64);

        match path {
            "/api/state" => return self.snapshot(),
            "/api/run" => w.set_running(true),
            "/api/pause" => w.set_running(false),
            "/api/warp" => w.set_warp(u64_of("mult").unwrap_or(1) as u32),
            "/api/autopilot" => {
                w.set_autopilot(b.get("on").and_then(Value::as_bool).unwrap_or(false))
            }
            "/api/throttle" => {
                w.set_throttle(b.get("value").and_then(Value::as_f64).unwrap_or(0.0))
            }
            "/api/launch" | "/api/reset" => match w.launch_from_pad() {
                Ok(_) => w.set_running(true),
                Err(m) => return err(&m),
            },
            "/api/stage" => {
                if let Err(m) = w.stage_flight() {
                    return err(&m);
                }
            }
            "/api/vab/add" => {
                let Some(part) = str_of("part_id") else {
                    return err("invalid_argument.ksp.vab: part_id is required");
                };
                let index = b.get("index").and_then(Value::as_u64).map(|i| i as usize);
                if let Err(m) = w.vab_add(&part, index) {
                    return err(&m);
                }
            }
            "/api/vab/remove" => {
                let Some(index) = u64_of("index") else {
                    return err("invalid_argument.ksp.vab: index is required");
                };
                if let Err(m) = w.vab_remove(index as usize) {
                    return err(&m);
                }
            }
            "/api/vab/tune" => {
                let Some(index) = u64_of("index") else {
                    return err("invalid_argument.ksp.part: index is required");
                };
                let f = |k: &str| b.get(k).and_then(Value::as_f64);
                if let Err(m) = w.vab_tune(index as usize, f("fuel"), f("thrust_limit")) {
                    return err(&m);
                }
            }
            "/api/planet" => {
                let Some(id) = str_of("id") else {
                    return err("invalid_argument.ksp.planet: id is required");
                };
                if let Err(m) = w.set_planet(&id) {
                    return err(&m);
                }
            }
            "/api/vab/reset" => {
                if let Err(m) = w.vab_reset_stick() {
                    return err(&m);
                }
            }
            "/api/vab/save" => {
                if let Err(m) = w.vab_save() {
                    return err(&m);
                }
            }
            "/api/audit" => {
                return match w.audit() {
                    Ok(v) => v.to_string(),
                    Err(m) => err(&m),
                }
            }
            "/api/focus" => {
                let Some(launch) = str_of("launch") else {
                    return err("invalid_argument.ksp.focus: launch is required");
                };
                if let Err(m) = w.set_focus(&launch) {
                    return err(&m);
                }
            }
            "/api/fork" => {
                let Some(from) = focused_or(w, str_of("from").as_deref()) else {
                    return err("failed_precondition.ksp.fork: no focused launch");
                };
                if let Err(m) = w.fork_at(&from, u64_of("at_seq")) {
                    return err(&m);
                }
            }
            "/api/rewind" => {
                let Some(launch) = focused_or(w, str_of("launch").as_deref()) else {
                    return err("failed_precondition.ksp.rewind: no focused launch");
                };
                let Some(seq) = u64_of("seq") else {
                    return err("invalid_argument.ksp.rewind: seq is required");
                };
                if let Err(m) = w.rewind(&launch, seq) {
                    return err(&m);
                }
            }
            "/api/add-tank" => {
                let Some(launch) = focused_or(w, str_of("launch").as_deref()) else {
                    return err("failed_precondition.ksp.edit: no focused launch");
                };
                let part = str_of("part_id").unwrap_or_else(|| "tank-s".into());
                if let Err(m) = w.add_tank(&launch, &part) {
                    return err(&m);
                }
            }
            "/api/promote" => {
                let Some(launch) = focused_or(w, str_of("launch").as_deref()) else {
                    return err("failed_precondition.ksp.promote: no focused launch");
                };
                let strategy = str_of("strategy").unwrap_or_else(|| "strict".into());
                if let Err(m) = w.promote_this_design(&launch, &strategy) {
                    return err(&m);
                }
            }
            "/api/compare" => {
                let Some(a) = focused_or(w, str_of("a").as_deref()) else {
                    return err("failed_precondition.ksp.compare: no focused launch");
                };
                let b_name = str_of("b").unwrap_or_else(|| "vab".into());
                return match w.compare(&a, &b_name) {
                    Ok(v) => v.to_string(),
                    Err(m) => err(&m),
                };
            }
            "/api/archive" => {
                let Some(launch) = focused_or(w, str_of("launch").as_deref()) else {
                    return err("failed_precondition.ksp.archive: no focused launch");
                };
                let keep = b
                    .get("keep_snapshot")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if let Err(m) = w.archive(&launch, keep) {
                    return err(&m);
                }
            }
            other => return err(&format!("not_found.ksp.route: {other}")),
        }
        self.snapshot()
    }
}
