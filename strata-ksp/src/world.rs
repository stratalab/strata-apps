//! World: Strata hangar + launch tapes. Integrator holds registers only.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
#[cfg(feature = "server")]
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
#[cfg(feature = "server")]
use std::thread::{self, JoinHandle};

use crate::clock::Instant;

use serde_json::Value;
use stratadb::Database;

use crate::ascent::{apply_pilot, mark_orbit_if_won, powered_step, AscentPhase, AutoPilot};
use crate::craft::CraftSpec;
use crate::findings::{known_at_compile_time, Finding, Kind, Log};
use crate::physics::{FlightStatus, Vessel, DT, MAX_STEPS_PER_WALL_TICK};
use crate::snapshot::{LaunchView, Snapshot, TrailSample};
use crate::store::{self, EnsureError, PersistStats, BRANCH_VAB};
use crate::telemetry::{
    self, VesselSample, EVENT_CONTROL, EVENT_EDIT, EVENT_FLAMEOUT, EVENT_FORK, EVENT_LAUNCH,
    EVENT_REWIND, EVENT_STAGE, LIVE_LAUNCH_CAP, TAPE_CAP, TICK_DT, WALL_TICK_MIN_MS,
};

pub struct OpenArgs {
    pub cache: bool,
    pub db_path: PathBuf,
}

struct PersistAck {
    seq: u64,
    result: Result<(), String>,
}

#[derive(Clone)]
enum PersistKind {
    Tick,
    Discrete {
        event_type: String,
        payload: Value,
        graph_delete: Vec<String>,
        spec: Option<CraftSpec>,
        rebuild_graph: bool,
        replace_trail: Option<Vec<TrailSample>>,
    },
}

struct PersistJob {
    launch: String,
    kind: PersistKind,
    sample: VesselSample,
    ack: Option<SyncSender<PersistAck>>,
}

#[derive(Clone, Default)]
struct TapeView {
    trail: VecDeque<TrailSample>,
    sample: Option<VesselSample>,
    seq: u64,
}

struct PersistCounters {
    total_commits: AtomicU64,
    persist_sum_us: AtomicU64,
    persist_max_us: AtomicU64,
    persist_saves: AtomicU64,
    last_persist_us: AtomicU64,
    last_commits: AtomicU64,
}

impl PersistCounters {
    fn new() -> Self {
        Self {
            total_commits: AtomicU64::new(0),
            persist_sum_us: AtomicU64::new(0),
            persist_max_us: AtomicU64::new(0),
            persist_saves: AtomicU64::new(0),
            last_persist_us: AtomicU64::new(0),
            last_commits: AtomicU64::new(0),
        }
    }

    fn record(&self, stats: &PersistStats) {
        let us = stats.total_us();
        self.last_persist_us.store(us, Ordering::Relaxed);
        self.last_commits.store(stats.commits, Ordering::Relaxed);
        self.total_commits
            .fetch_add(stats.commits, Ordering::Relaxed);
        self.persist_sum_us.fetch_add(us, Ordering::Relaxed);
        self.persist_saves.fetch_add(1, Ordering::Relaxed);
        let mut max = self.persist_max_us.load(Ordering::Relaxed);
        while us > max {
            match self.persist_max_us.compare_exchange(
                max,
                us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => max = current,
            }
        }
    }
}

pub struct World {
    db: Arc<Mutex<Database>>,
    #[cfg(feature = "server")]
    persist_tx: Mutex<Option<Sender<PersistJob>>>,
    #[cfg(feature = "server")]
    persist_join: Mutex<Option<JoinHandle<()>>>,
    vab: Mutex<CraftSpec>,
    vab_edit: Mutex<()>,
    launch_mu: Mutex<()>,
    launches: Mutex<BTreeMap<String, LiveShip>>,
    tapes: Arc<Mutex<BTreeMap<String, TapeView>>>,
    findings: Arc<Log>,
    counters: Arc<PersistCounters>,
    running: AtomicBool,
    hz: AtomicU64,
    warp: AtomicU32,
    steps_last_frame: AtomicU32,
    branch_count: AtomicU32,
    next_launch: AtomicU64,
    next_design: AtomicU64,
    focused: Mutex<String>,
    persist_wall_cap: AtomicBool,
    last_tick_wall: Mutex<Option<Instant>>,
    last_promote: Mutex<Option<Value>>,
    last_compare: Mutex<Option<Value>>,
    last_archive: Mutex<Option<Value>>,
    durable: bool,
    db_path: String,
}

struct LiveShip {
    name: String,
    parent: String,
    /// Version of `parent` this launch branched at; 0 for a pad launch.
    fork_seq: u64,
    design: String,
    vessel: Vessel,
    spec: CraftSpec,
    stage: u32,
    autopilot: AutoPilot,
    warp: u32,
    graph_ids: Vec<String>,
    last_persist_t: f64,
    pending_tick: Option<PersistJob>,
}

impl LiveShip {
    fn sample(&self) -> VesselSample {
        VesselSample::from_ship(
            &self.name,
            &self.parent,
            &self.design,
            &self.vessel,
            self.stage,
            self.autopilot.enabled,
            self.autopilot.phase,
            self.warp,
        )
    }

    fn graph_ids_for_spec(spec: &CraftSpec) -> Vec<String> {
        spec.parts
            .iter()
            .enumerate()
            .map(|(i, p)| CraftSpec::node_id(i, &p.part_id))
            .collect()
    }
}

impl World {
    pub fn open(args: OpenArgs) -> Result<Self, String> {
        let findings = Arc::new(Log::new());
        for finding in known_at_compile_time() {
            findings.push(finding);
        }

        let (db, durable, db_path) = if args.cache {
            findings.push(Finding::new(
                Kind::Note,
                "engine.open",
                "Cache mode: hangar lives only in this process",
                "open_cache skips WAL, manifest, snapshot, and locks. A process kill forgets the VAB.",
            ));
            (store::open_cache().map_err(eng)?, false, "cache".to_owned())
        } else {
            findings.push(Finding::new(
                Kind::Gap,
                "engine.open / ipc",
                "strata CLI cannot inspect this directory while we hold it",
                format!(
                    "`strata {} branch list` will fail with unavailable.engine.persistence until this process exits. Delete the directory to roll back.",
                    args.db_path.display()
                ),
            ));
            std::fs::create_dir_all(&args.db_path).map_err(|e| e.to_string())?;
            (
                store::open_local(&args.db_path).map_err(eng)?,
                true,
                args.db_path.display().to_string(),
            )
        };

        let mut db = db;
        let outcome = match store::ensure_world_seed(&mut db) {
            Ok(outcome) => outcome,
            Err(EnsureError::PlanetVersion) => {
                return Err(format!(
                    "failed_precondition.ksp.planet_version: compiled planet does not match {db_path}. Delete the directory and reopen."
                ));
            }
            Err(EnsureError::CatalogVersion) => {
                return Err(format!(
                    "failed_precondition.ksp.catalog_version: compiled catalog does not match {db_path}. Delete the directory and reopen."
                ));
            }
            Err(EnsureError::Craft(message)) => {
                return Err(format!("invalid_argument.ksp.craft: {message}"));
            }
            Err(EnsureError::Engine(error)) => return Err(eng(error)),
        };

        let meta = store::read_vab_meta(&mut db).map_err(eng)?;
        let all_names = store::list_product_branches(&mut db).map_err(eng)?;
        let launch_names = store::list_launch_branches(&mut db).map_err(eng)?;
        let mut launches = BTreeMap::new();
        let mut tapes = BTreeMap::new();
        let mut max_launch = meta.next_launch;
        let mut max_design = meta.next_design;
        let mut focused = String::new();
        for name in &all_names {
            if let Some(n) = parse_numbered("design-", name) {
                max_design = max_design.max(n + 1);
            }
        }

        for name in &launch_names {
            if let Some(n) = parse_numbered("launch-", name) {
                max_launch = max_launch.max(n + 1);
            }
            match store::reconstruct_launch(&mut db, name) {
                Ok(Some(resume)) => {
                    let mut vessel = Vessel::at_pad(resume.spec.wet_mass(), resume.spec.fuel());
                    resume.sample.apply_to(&mut vessel);
                    let ship = LiveShip {
                        name: name.clone(),
                        parent: resume.sample.parent.clone(),
                        fork_seq: resume.fork_seq,
                        design: resume.sample.design.clone(),
                        vessel,
                        spec: resume.spec,
                        stage: resume.sample.stage,
                        autopilot: AutoPilot {
                            enabled: resume.sample.autopilot,
                            phase: resume.sample.phase,
                        },
                        warp: resume.sample.warp.max(1),
                        graph_ids: resume.graph_ids,
                        last_persist_t: resume.sample.t,
                        pending_tick: None,
                    };
                    let mut tape = TapeView {
                        seq: resume.sample.last_event_seq,
                        sample: Some(resume.sample),
                        trail: resume.trail.into_iter().collect(),
                    };
                    while tape.trail.len() > TAPE_CAP {
                        tape.trail.pop_front();
                    }
                    tapes.insert(name.clone(), tape);
                    launches.insert(name.clone(), ship);
                    focused = name.clone();
                }
                Ok(None) => {}
                Err(error) => return Err(ensure_err(error)),
            }
        }

        if outcome.resumed {
            findings.push(Finding::new(
                Kind::Note,
                "demo",
                "Resumed hangar from the existing database",
                format!("Opened {db_path} and loaded craft JSON from branch vab."),
            ));
            if !launches.is_empty() {
                findings.push(Finding::new(
                    Kind::Note,
                    "demo",
                    "Resumed launch tapes from events",
                    format!(
                        "Rebuilt {} launch branch(es) from the event tape (KV is a cache).",
                        launches.len()
                    ),
                ));
            }
        }

        let db = Arc::new(Mutex::new(db));
        let tapes = Arc::new(Mutex::new(tapes));
        let counters = Arc::new(PersistCounters::new());
        // The browser has one thread, so there is nothing to hand work to and
        // nothing to drain a queue. Jobs run inline there instead; see
        // `send_job`.
        #[cfg(feature = "server")]
        let (tx, rx) = mpsc::channel();
        #[cfg(feature = "server")]
        let join = spawn_persist_worker(
            rx,
            db.clone(),
            tapes.clone(),
            findings.clone(),
            counters.clone(),
        )?;
        let branch_count = {
            let mut db = db.lock().expect("db lock");
            store::list_product_branches(&mut db).map_err(eng)?.len() as u32
        };

        Ok(Self {
            db,
            #[cfg(feature = "server")]
            persist_tx: Mutex::new(Some(tx)),
            #[cfg(feature = "server")]
            persist_join: Mutex::new(Some(join)),
            vab: Mutex::new(outcome.spec),
            vab_edit: Mutex::new(()),
            launch_mu: Mutex::new(()),
            launches: Mutex::new(launches),
            tapes,
            findings,
            counters,
            running: AtomicBool::new(true),
            hz: AtomicU64::new(30),
            warp: AtomicU32::new(1),
            steps_last_frame: AtomicU32::new(0),
            branch_count: AtomicU32::new(branch_count),
            next_launch: AtomicU64::new(max_launch.max(1)),
            next_design: AtomicU64::new(max_design.max(1)),
            focused: Mutex::new(focused),
            persist_wall_cap: AtomicBool::new(true),
            last_tick_wall: Mutex::new(None),
            last_promote: Mutex::new(None),
            last_compare: Mutex::new(None),
            last_archive: Mutex::new(None),
            durable,
            db_path,
        })
    }

    pub fn set_persist_wall_cap(&self, on: bool) {
        self.persist_wall_cap.store(on, Ordering::Relaxed);
    }

    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn hz(&self) -> f64 {
        self.hz.load(Ordering::Relaxed) as f64
    }

    pub fn set_hz(&self, hz: f64) {
        let hz = hz.clamp(1.0, 60.0);
        self.hz.store(hz.round() as u64, Ordering::Relaxed);
    }

    pub fn warp(&self) -> u32 {
        let focused = self.focused.lock().expect("focused lock").clone();
        let launches = self.launches.lock().expect("launches lock");
        launches
            .get(&focused)
            .map(|ship| ship.warp)
            .unwrap_or_else(|| self.warp.load(Ordering::Relaxed))
    }

    pub fn set_warp(&self, mult: u32) {
        let allowed = [1, 2, 4, 10, 50];
        let warp = if allowed.contains(&mult) { mult } else { 1 };
        self.warp.store(warp, Ordering::Relaxed);
        // Warp is the clock, not a property of a vehicle. Two timelines
        // advancing at different rates cannot be compared - one just falls
        // behind - so every live launch runs at the same rate.
        let mut launches = self.launches.lock().expect("launches lock");
        for ship in launches.values_mut() {
            ship.warp = warp;
        }
    }

    pub fn period_ms(&self) -> u64 {
        let hz = self.hz().max(1.0);
        (1000.0 / hz).round() as u64
    }

    pub fn set_focus(&self, name: &str) -> Result<(), String> {
        let launches = self.launches.lock().expect("launches lock");
        if !launches.contains_key(name) {
            return Err(format!("not_found.ksp.launch: {name}"));
        }
        drop(launches);
        *self.focused.lock().expect("focused lock") = name.to_owned();
        Ok(())
    }

    /// Advance sim: round-robin RK4 across live launches, cap 2000 steps.
    pub fn tick_frame(&self) {
        if !self.is_running() {
            self.steps_last_frame.store(0, Ordering::Relaxed);
            return;
        }
        let hz = self.hz().max(1.0);
        let mut launches = self.launches.lock().expect("launches lock");
        if launches.is_empty() {
            self.steps_last_frame.store(0, Ordering::Relaxed);
            return;
        }
        let names: Vec<String> = launches.keys().cloned().collect();
        let mut remaining: BTreeMap<String, u32> = BTreeMap::new();
        for name in &names {
            let warp = launches.get(name).map(|s| s.warp).unwrap_or(1) as f64;
            let want = ((warp * 60.0) / hz).round() as u32;
            remaining.insert(name.clone(), want.max(1));
        }
        let mut steps = 0u32;
        'budget: for _ in 0..MAX_STEPS_PER_WALL_TICK {
            let mut progressed = false;
            for name in &names {
                if remaining.get(name).copied().unwrap_or(0) == 0 {
                    continue;
                }
                let ship = launches.get_mut(name).expect("ship");
                if matches!(
                    ship.vessel.status,
                    FlightStatus::Crashed | FlightStatus::Escaped
                ) {
                    remaining.insert(name.clone(), 0);
                    continue;
                }
                self.step_ship(ship);
                remaining.insert(name.clone(), remaining[name] - 1);
                steps += 1;
                progressed = true;
                if steps >= MAX_STEPS_PER_WALL_TICK {
                    break 'budget;
                }
            }
            if !progressed {
                break;
            }
        }

        let wall_ok = self.tick_wall_allows();
        let mut jobs = Vec::new();
        for ship in launches.values_mut() {
            if let Some(job) = ship.pending_tick.take() {
                if wall_ok {
                    ship.last_persist_t = job.sample.t;
                    jobs.push(job);
                } else {
                    ship.pending_tick = Some(job);
                }
            }
        }
        drop(launches);
        if wall_ok && !jobs.is_empty() {
            *self.last_tick_wall.lock().expect("tick wall") = Some(Instant::now());
        }
        for job in jobs {
            self.send_job(job);
        }
        self.steps_last_frame.store(steps, Ordering::Relaxed);
    }

    fn step_ship(&self, ship: &mut LiveShip) {
        let prev_status = ship.vessel.status;
        let prev_fuel = ship.vessel.fuel;
        let staged = apply_pilot(&mut ship.vessel, &mut ship.spec, &mut ship.autopilot);
        if let Some(drop) = staged {
            let n = drop.ids.len().min(ship.graph_ids.len());
            let deleted: Vec<String> = ship.graph_ids.drain(0..n).collect();
            ship.stage += 1;
            self.queue_discrete(
                ship,
                EVENT_STAGE,
                telemetry::stage_object(&ship.sample(), &deleted, drop.dropped_fuel),
                deleted,
            );
        }
        powered_step(&mut ship.vessel, &mut ship.spec);
        mark_orbit_if_won(&mut ship.vessel, &ship.autopilot);
        if ship.vessel.status != prev_status {
            if let Some(kind) = telemetry::status_event_type(ship.vessel.status) {
                self.queue_discrete(
                    ship,
                    kind,
                    telemetry::event_object(kind, &ship.sample()),
                    vec![],
                );
            }
        } else if prev_fuel > 1e-12 && ship.vessel.fuel <= 1e-12 {
            self.queue_discrete(
                ship,
                EVENT_FLAMEOUT,
                telemetry::event_object(EVENT_FLAMEOUT, &ship.sample()),
                vec![],
            );
        }
        if ship.vessel.t - ship.last_persist_t >= TICK_DT - DT * 0.5 {
            ship.pending_tick = Some(PersistJob {
                launch: ship.name.clone(),
                kind: PersistKind::Tick,
                sample: ship.sample(),
                ack: None,
            });
        }
    }

    fn tick_wall_allows(&self) -> bool {
        if !self.persist_wall_cap.load(Ordering::Relaxed) {
            return true;
        }
        match *self.last_tick_wall.lock().expect("tick wall") {
            None => true,
            Some(prev) => prev.elapsed().as_millis() >= WALL_TICK_MIN_MS,
        }
    }

    fn queue_discrete(
        &self,
        ship: &mut LiveShip,
        event_type: &str,
        payload: Value,
        graph_delete: Vec<String>,
    ) {
        ship.pending_tick = None;
        ship.last_persist_t = ship.vessel.t;
        let job = PersistJob {
            launch: ship.name.clone(),
            kind: PersistKind::Discrete {
                event_type: event_type.to_owned(),
                payload,
                graph_delete,
                spec: None,
                rebuild_graph: false,
                replace_trail: None,
            },
            sample: ship.sample(),
            ack: None,
        };
        self.send_job(job);
    }

    #[cfg(feature = "server")]
    fn send_job(&self, job: PersistJob) {
        if let Some(tx) = self.persist_tx.lock().expect("persist tx").as_ref() {
            let _ = tx.send(job);
        }
    }

    /* No worker to send to, so the job runs here. Callers that wait on an ack
     * get it on the same stack, before `send_job` returns. */
    #[cfg(not(feature = "server"))]
    fn send_job(&self, job: PersistJob) {
        run_persist_job(job, &self.db, &self.tapes, &self.findings, &self.counters);
    }

    pub fn launch_from_pad(&self) -> Result<String, String> {
        // Launch is the verb this app is for, and it should never refuse.
        // Past the cap the oldest attempt is archived to make room: its events
        // stay in the database, it simply stops being live.
        //
        // Before the gate, not after. `archive` takes `launch_mu` as well and
        // a std Mutex is not reentrant, so doing this while holding it
        // deadlocks - which is exactly what it did.
        loop {
            let oldest = {
                let launches = self.launches.lock().expect("launches lock");
                if launches.len() < LIVE_LAUNCH_CAP {
                    break;
                }
                let focused = self.focused.lock().expect("focused lock").clone();
                launches.keys().find(|n| **n != focused).cloned()
            };
            let Some(name) = oldest else {
                return Err(format!(
                    "failed_precondition.ksp.launch_cap: live launches capped at {LIVE_LAUNCH_CAP}"
                ));
            };
            self.archive(&name, false)?;
        }

        let _gate = self.launch_mu.lock().expect("launch lock");

        self.vab_save()?;

        let n_launch = self.next_launch.load(Ordering::Relaxed);
        let n_design = self.next_design.load(Ordering::Relaxed);
        let launch = format!("launch-{n_launch:04}");
        let design = format!("design-{n_design:04}");
        let spec = self.vab.lock().expect("vab lock").clone();
        let warp = self.warp.load(Ordering::Relaxed).max(1);
        let vessel = Vessel::at_pad(spec.wet_mass(), spec.fuel());
        let sample = VesselSample::from_ship(
            &launch,
            BRANCH_VAB,
            &design,
            &vessel,
            0,
            true,
            AscentPhase::Boost,
            warp,
        );

        {
            let mut db = self.db.lock().expect("db lock");
            store::fork_current(&mut db, BRANCH_VAB, &design).map_err(eng)?;
            store::write_vab_meta_doc(
                &mut db,
                &store::VabMeta {
                    next_launch: n_launch + 1,
                    next_design: n_design + 1,
                },
            )
            .map_err(eng)?;
            store::fork_current(&mut db, BRANCH_VAB, &launch).map_err(eng)?;
            store::write_launch_meta(&mut db, &launch, BRANCH_VAB, &design, 0, &sample)
                .map_err(eng)?;
            let count = store::list_product_branches(&mut db).map_err(eng)?.len() as u32;
            self.branch_count.store(count, Ordering::Relaxed);
        }
        self.next_launch.store(n_launch + 1, Ordering::Relaxed);
        self.next_design.store(n_design + 1, Ordering::Relaxed);

        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        let hash = telemetry::craft_hash(&spec);
        let payload = telemetry::launch_object(&sample, &hash);
        {
            let mut launches = self.launches.lock().expect("launches lock");
            let graph_ids = LiveShip::graph_ids_for_spec(&spec);
            let ship = LiveShip {
                name: launch.clone(),
                parent: BRANCH_VAB.to_owned(),
                fork_seq: 0,
                design: design.clone(),
                vessel,
                spec,
                stage: 0,
                autopilot: AutoPilot::on(),
                warp,
                graph_ids,
                last_persist_t: 0.0,
                pending_tick: None,
            };
            let job = PersistJob {
                launch: launch.clone(),
                kind: PersistKind::Discrete {
                    event_type: EVENT_LAUNCH.to_owned(),
                    payload,
                    graph_delete: vec![],
                    spec: None,
                    rebuild_graph: false,
                    replace_trail: None,
                },
                sample: ship.sample(),
                ack: Some(ack_tx),
            };
            self.send_job(job);
            launches.insert(launch.clone(), ship);
        }
        *self.focused.lock().expect("focused lock") = launch.clone();
        wait_ack(ack_rx)?;
        Ok(launch)
    }

    pub fn reset_throw(&self) -> Result<String, String> {
        self.launch_from_pad()
    }

    pub fn set_autopilot(&self, on: bool) {
        let focused = self.focused.lock().expect("focused lock").clone();
        let mut launches = self.launches.lock().expect("launches lock");
        let Some(ship) = launches.get_mut(&focused) else {
            return;
        };
        ship.autopilot.enabled = on;
        if on {
            ship.autopilot.phase = AscentPhase::Boost;
        }
        self.queue_discrete(
            ship,
            EVENT_CONTROL,
            telemetry::control_object(&ship.sample()),
            vec![],
        );
    }

    pub fn set_throttle(&self, value: f64) {
        let focused = self.focused.lock().expect("focused lock").clone();
        let mut launches = self.launches.lock().expect("launches lock");
        let Some(ship) = launches.get_mut(&focused) else {
            return;
        };
        ship.autopilot.enabled = false;
        ship.vessel.throttle = value.clamp(0.0, 1.0);
        self.queue_discrete(
            ship,
            EVENT_CONTROL,
            telemetry::control_object(&ship.sample()),
            vec![],
        );
    }

    pub fn vab_spec(&self) -> CraftSpec {
        self.vab.lock().expect("vab lock").clone()
    }

    pub fn vab_add(&self, part_id: &str, index: Option<usize>) -> Result<(), String> {
        self.edit_vab(|spec| spec.add(part_id, index))
    }

    pub fn vab_tune(
        &self,
        index: usize,
        fuel: Option<f64>,
        thrust_limit: Option<f64>,
    ) -> Result<(), String> {
        self.edit_vab(|spec| spec.tune(index, fuel, thrust_limit))
    }

    pub fn vab_remove(&self, index: usize) -> Result<(), String> {
        self.edit_vab(|spec| spec.remove(index).map(|_| ()))
    }

    pub fn vab_reset_stick(&self) -> Result<(), String> {
        self.edit_vab(|spec| {
            *spec = CraftSpec::sounding_stick();
            Ok(())
        })
    }

    pub fn vab_save(&self) -> Result<(), String> {
        let _edit = self.vab_edit.lock().expect("vab edit lock");
        let spec = self.vab.lock().expect("vab lock").clone();
        self.persist_vab(spec)
    }

    fn edit_vab(&self, f: impl FnOnce(&mut CraftSpec) -> Result<(), String>) -> Result<(), String> {
        let _edit = self.vab_edit.lock().expect("vab edit lock");
        let mut spec = self.vab.lock().expect("vab lock").clone();
        f(&mut spec)?;
        self.persist_vab(spec)
    }

    fn persist_vab(&self, spec: CraftSpec) -> Result<(), String> {
        let stats = {
            let mut db = self.db.lock().expect("db lock");
            store::save_vab(&mut db, &spec).map_err(eng)?
        };
        self.counters.record(&stats);
        let read = {
            let mut db = self.db.lock().expect("db lock");
            store::read_craft(&mut db, BRANCH_VAB).map_err(ensure_err)?
        };
        let read = read.ok_or_else(|| {
            "failed_precondition.ksp.craft: vab craft missing after save".to_owned()
        })?;
        *self.vab.lock().expect("vab lock") = read;
        Ok(())
    }

    pub fn craft_node_ids(&self) -> Result<Vec<String>, String> {
        let mut db = self.db.lock().expect("db lock");
        store::list_craft_node_ids(&mut db, BRANCH_VAB).map_err(eng)
    }

    pub fn stage_flight(&self) -> Result<Vec<String>, String> {
        let focused = self.focused.lock().expect("focused lock").clone();
        let mut launches = self.launches.lock().expect("launches lock");
        let ship = launches
            .get_mut(&focused)
            .ok_or_else(|| "failed_precondition.ksp.stage: no focused launch".to_owned())?;
        if ship.spec.parts.is_empty() {
            return Err("nothing left to stage".into());
        }
        let drop = ship.spec.stage();
        let n = drop.ids.len().min(ship.graph_ids.len());
        let deleted: Vec<String> = ship.graph_ids.drain(0..n).collect();
        ship.stage += 1;
        ship.vessel.mass = ship.spec.wet_mass();
        ship.vessel.fuel = ship.spec.fuel();
        if ship.vessel.mass <= 0.0 {
            ship.vessel.status = FlightStatus::Crashed;
        }
        let ids = drop.ids.clone();
        self.queue_discrete(
            ship,
            EVENT_STAGE,
            telemetry::stage_object(&ship.sample(), &deleted, drop.dropped_fuel),
            deleted,
        );
        Ok(ids)
    }

    pub fn flush_durable(&self, launch: &str) -> Result<u64, String> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        {
            let mut launches = self.launches.lock().expect("launches lock");
            let ship = launches
                .get_mut(launch)
                .ok_or_else(|| format!("not_found.ksp.launch: {launch}"))?;
            let mut job = ship.pending_tick.take().unwrap_or_else(|| PersistJob {
                launch: launch.to_owned(),
                kind: PersistKind::Tick,
                sample: ship.sample(),
                ack: None,
            });
            ship.last_persist_t = job.sample.t;
            job.ack = Some(ack_tx);
            self.send_job(job);
        }
        wait_ack(ack_rx)
    }

    pub fn flush_all(&self) -> Result<(), String> {
        let names: Vec<String> = self
            .launches
            .lock()
            .expect("launches lock")
            .keys()
            .cloned()
            .collect();
        for name in names {
            self.flush_durable(&name)?;
        }
        Ok(())
    }

    pub fn event_len(&self, branch_name: &str) -> Result<u64, String> {
        let mut db = self.db.lock().expect("db lock");
        store::event_len(&mut db, branch_name).map_err(eng)
    }

    pub fn verify_chain(&self, branch_name: &str) -> Result<bool, String> {
        let mut db = self.db.lock().expect("db lock");
        store::verify_chain(&mut db, branch_name).map_err(eng)
    }

    pub fn event_kinds(&self, branch_name: &str) -> Result<Vec<String>, String> {
        let mut db = self.db.lock().expect("db lock");
        Ok(store::range_events(&mut db, branch_name)
            .map_err(eng)?
            .iter()
            .map(|event| event.event_type().as_str().to_owned())
            .collect())
    }

    pub fn audit(&self) -> Result<Value, String> {
        let mut names = {
            let mut db = self.db.lock().expect("db lock");
            store::list_launch_branches(&mut db).map_err(eng)?
        };
        names.insert(0, BRANCH_VAB.to_owned());
        let mut branches = Vec::new();
        let mut ok = true;
        for name in names {
            let valid = self.verify_chain(&name)?;
            ok = ok && valid;
            branches.push(serde_json::json!({ "name": name, "ok": valid }));
        }
        Ok(serde_json::json!({ "ok": ok, "branches": branches }))
    }

    pub fn event_log(&self, branch_name: &str) -> Result<Vec<(u64, String)>, String> {
        let mut db = self.db.lock().expect("db lock");
        Ok(store::range_events(&mut db, branch_name)
            .map_err(eng)?
            .iter()
            .map(|event| {
                (
                    event.sequence().as_u64(),
                    event.event_type().as_str().to_owned(),
                )
            })
            .collect())
    }

    pub fn graph_nodes(&self, branch_name: &str) -> Result<Vec<String>, String> {
        let mut db = self.db.lock().expect("db lock");
        store::list_craft_node_ids(&mut db, branch_name).map_err(eng)
    }

    pub fn launch_spec(&self, name: &str) -> Option<CraftSpec> {
        self.launches
            .lock()
            .expect("launches lock")
            .get(name)
            .map(|ship| ship.spec.clone())
    }

    /// Fork `from` at `at_seq` (or the flushed head). Child shares `meta.design`.
    pub fn fork_at(&self, from: &str, at_seq: Option<u64>) -> Result<String, String> {
        let _gate = self.launch_mu.lock().expect("launch lock");
        let inherited = {
            let launches = self.launches.lock().expect("launches lock");
            if launches.len() >= LIVE_LAUNCH_CAP {
                return Err(format!(
                    "failed_precondition.ksp.launch_cap: live launches capped at {LIVE_LAUNCH_CAP}; archive one first"
                ));
            }
            let Some(source) = launches.get(from) else {
                return Err(format!("not_found.ksp.launch: {from}"));
            };
            (
                source.warp,
                source.autopilot.enabled,
                source.vessel.throttle,
            )
        };

        let flushed = self.flush_durable(from)?;
        let at_seq = at_seq.unwrap_or(flushed);

        let n_launch = self.next_launch.load(Ordering::Relaxed);
        let child = format!("launch-{n_launch:04}");
        let n_design = self.next_design.load(Ordering::Relaxed);

        let resume = {
            let mut db = self.db.lock().expect("db lock");
            let record = store::get_event(&mut db, from, at_seq)
                .map_err(eng)?
                .ok_or_else(|| {
                    format!("not_found.ksp.seq: {from} has no event {at_seq}; refusing silent fork_current")
                })?;
            store::fork_at_version(&mut db, from, &child, &record).map_err(eng)?;
            store::write_vab_meta_doc(
                &mut db,
                &store::VabMeta {
                    next_launch: n_launch + 1,
                    next_design: n_design,
                },
            )
            .map_err(eng)?;
            let resume = store::reconstruct_launch(&mut db, &child)
                .map_err(ensure_err)?
                .ok_or_else(|| {
                    format!("failed_precondition.ksp.fork: {child} has no flight tape")
                })?;
            let mut sample = resume.sample.clone();
            sample.name = child.clone();
            sample.parent = from.to_owned();
            store::write_launch_meta(
                &mut db,
                &child,
                from,
                &resume.sample.design,
                at_seq,
                &sample,
            )
            .map_err(eng)?;
            let count = store::list_product_branches(&mut db).map_err(eng)?.len() as u32;
            self.branch_count.store(count, Ordering::Relaxed);
            resume
        };
        self.next_launch.store(n_launch + 1, Ordering::Relaxed);

        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        let design = resume.sample.design.clone();
        let trail = resume.trail.clone();
        let mut ship = ship_from_resume(child.clone(), from.to_owned(), at_seq, design, resume);
        // A branch that flies differently for reasons you did not choose is not
        // a comparison. It inherits its parent's controls - including the time
        // warp, without which the two clocks run at different rates and the
        // fork simply falls behind - so the only thing that differs is
        // whatever you change next.
        ship.warp = inherited.0.max(1);
        ship.autopilot.enabled = inherited.1;
        ship.vessel.throttle = inherited.2;
        let payload = telemetry::fork_object(&ship.sample(), from, at_seq);
        {
            let mut launches = self.launches.lock().expect("launches lock");
            let job = PersistJob {
                launch: child.clone(),
                kind: PersistKind::Discrete {
                    event_type: EVENT_FORK.to_owned(),
                    payload,
                    graph_delete: vec![],
                    spec: Some(ship.spec.clone()),
                    rebuild_graph: true,
                    replace_trail: Some(trail),
                },
                sample: ship.sample(),
                ack: Some(ack_tx),
            };
            self.send_job(job);
            launches.insert(child.clone(), ship);
        }
        *self.focused.lock().expect("focused lock") = child.clone();
        wait_ack(ack_rx)?;
        Ok(child)
    }

    pub fn rewind(&self, launch: &str, to_seq: u64) -> Result<(), String> {
        self.flush_durable(launch)?;
        let resume = {
            let mut db = self.db.lock().expect("db lock");
            store::get_event(&mut db, launch, to_seq)
                .map_err(eng)?
                .ok_or_else(|| format!("not_found.ksp.seq: {launch} has no event {to_seq}"))?;
            store::reconstruct_launch_at(&mut db, launch, Some(to_seq))
                .map_err(ensure_err)?
                .ok_or_else(|| {
                    format!("failed_precondition.ksp.rewind: {launch} has no flight at {to_seq}")
                })?
        };

        let trail = resume.trail.clone();
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        {
            let mut launches = self.launches.lock().expect("launches lock");
            let ship = launches
                .get_mut(launch)
                .ok_or_else(|| format!("not_found.ksp.launch: {launch}"))?;
            apply_resume(ship, resume);
            ship.pending_tick = None;
            let payload = telemetry::rewind_object(&ship.sample(), to_seq);
            let job = PersistJob {
                launch: launch.to_owned(),
                kind: PersistKind::Discrete {
                    event_type: EVENT_REWIND.to_owned(),
                    payload,
                    graph_delete: vec![],
                    spec: Some(ship.spec.clone()),
                    rebuild_graph: true,
                    replace_trail: Some(trail),
                },
                sample: ship.sample(),
                ack: Some(ack_tx),
            };
            self.send_job(job);
        }
        wait_ack(ack_rx)?;
        Ok(())
    }

    pub fn add_tank(&self, launch: &str, part_id: &str) -> Result<(), String> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        {
            let mut launches = self.launches.lock().expect("launches lock");
            let ship = launches
                .get_mut(launch)
                .ok_or_else(|| format!("not_found.ksp.launch: {launch}"))?;
            let index = ship.spec.insert_above_current_engine(part_id)?;
            ship.graph_ids = LiveShip::graph_ids_for_spec(&ship.spec);
            ship.vessel.mass = ship.spec.wet_mass();
            ship.vessel.fuel = ship.spec.fuel();
            ship.pending_tick = None;
            let payload = telemetry::edit_object(&ship.sample(), "add_tank", part_id, index);
            let job = PersistJob {
                launch: launch.to_owned(),
                kind: PersistKind::Discrete {
                    event_type: EVENT_EDIT.to_owned(),
                    payload,
                    graph_delete: vec![],
                    spec: Some(ship.spec.clone()),
                    rebuild_graph: true,
                    replace_trail: None,
                },
                sample: ship.sample(),
                ack: Some(ack_tx),
            };
            self.send_job(job);
        }
        wait_ack(ack_rx)?;
        Ok(())
    }

    pub fn list_branches(&self) -> Result<Vec<String>, String> {
        let mut db = self.db.lock().expect("db lock");
        store::list_product_branches(&mut db).map_err(eng)
    }

    /// Delete a launch. Delete its `design-*` only at refcount 0 (and not
    /// `keep_snapshot`). Never called from promote.
    pub fn archive(&self, launch: &str, keep_snapshot: bool) -> Result<Value, String> {
        if !launch.starts_with("launch-") {
            return Err(format!(
                "failed_precondition.ksp.archive: {launch} is not a launch"
            ));
        }
        let _gate = self.launch_mu.lock().expect("launch lock");
        self.flush_all()?;
        let view = {
            let mut db = self.db.lock().expect("db lock");
            store::archive_launch(&mut db, launch, keep_snapshot).map_err(eng)?
        };
        {
            let mut launches = self.launches.lock().expect("launches lock");
            launches.remove(launch);
            self.tapes.lock().expect("tapes lock").remove(launch);
            let mut focused = self.focused.lock().expect("focused lock");
            if focused.as_str() == launch {
                *focused = launches.keys().next().cloned().unwrap_or_default();
            }
        }
        let count = {
            let mut db = self.db.lock().expect("db lock");
            store::list_product_branches(&mut db).map_err(eng)?.len() as u32
        };
        self.branch_count.store(count, Ordering::Relaxed);
        if view.design_deleted {
            self.findings.push(Finding::new(
                Kind::Note,
                "engine.branch",
                "Archived launch dropped an unreferenced design",
                format!(
                    "Deleted {launch} then {} (refcount 0).",
                    view.design.as_deref().unwrap_or("design")
                ),
            ));
        }
        let value = serde_json::to_value(&view).expect("archive json");
        *self.last_archive.lock().expect("archive lock") = Some(value.clone());
        Ok(value)
    }

    pub fn fork_named(&self, source: &str, child: &str) -> Result<(), String> {
        let mut db = self.db.lock().expect("db lock");
        store::fork_current(&mut db, source, child).map_err(eng)
    }

    pub fn compare(&self, a: &str, b: &str) -> Result<Value, String> {
        let view = {
            let mut db = self.db.lock().expect("db lock");
            store::compare_branches(&mut db, a, b).map_err(eng)?
        };
        let value = serde_json::to_value(&view).expect("compare json");
        *self.last_compare.lock().expect("compare lock") = Some(value.clone());
        Ok(value)
    }

    /// Promote the launch's `design-*` onto `vab`. Never promotes the flight branch.
    pub fn promote_this_design(&self, launch: &str, strategy_name: &str) -> Result<Value, String> {
        let strategy = store::parse_strategy(strategy_name)?;
        self.flush_durable(launch)?;
        let design = self
            .launches
            .lock()
            .expect("launches lock")
            .get(launch)
            .map(|ship| ship.design.clone())
            .ok_or_else(|| format!("not_found.ksp.launch: {launch}"))?;

        let (view, spec) = {
            let mut db = self.db.lock().expect("db lock");
            let spec = store::read_craft(&mut db, launch)
                .map_err(ensure_err)?
                .ok_or_else(|| format!("failed_precondition.ksp.craft: {launch} has no craft"))?;
            store::write_design_craft(&mut db, &design, &spec).map_err(eng)?;
            match store::promote_design(&mut db, &design, &spec, launch, strategy) {
                Ok(view) => (view, spec),
                Err(error) => {
                    let message = eng(error);
                    drop(db);
                    *self.last_promote.lock().expect("promote lock") = Some(serde_json::json!({
                        "ok": false,
                        "strategy": strategy_name,
                        "design": design,
                        "source_launch": launch,
                        "code": message.split_once(':').map_or(message.as_str(), |(c, _)| c),
                        "note": "hangar unchanged",
                    }));
                    return Err(message);
                }
            }
        };
        *self.vab.lock().expect("vab lock") = spec;
        let value = serde_json::to_value(&view).expect("promote json");
        *self.last_promote.lock().expect("promote lock") = Some(value.clone());
        Ok(value)
    }

    pub fn promote_named(
        &self,
        source: &str,
        target: &str,
        strategy_name: &str,
    ) -> Result<Value, String> {
        store::refuse_launch_onto_vab(source, target)?;
        let strategy = store::parse_strategy(strategy_name)?;
        let mut db = self.db.lock().expect("db lock");
        match store::promote_named(&mut db, source, target, strategy) {
            Ok(outcome) => Ok(serde_json::json!({
                "ok": true,
                "source": source,
                "target": target,
                "strategy": store::strategy_name(strategy),
                "unsupported": outcome
                    .capabilities_unsupported()
                    .iter()
                    .map(|c| format!("{c:?}"))
                    .collect::<Vec<_>>(),
            })),
            Err(error) => Err(eng(error)),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let vab = self.vab.lock().expect("vab lock").to_vab_view();
        let focused = self.focused.lock().expect("focused lock").clone();
        let launches = self.launches.lock().expect("launches lock");
        let tapes = self.tapes.lock().expect("tapes lock");
        let mut views = Vec::new();
        for (name, ship) in launches.iter() {
            let tape = tapes.get(name);
            let trail: Vec<TrailSample> = tape
                .map(|t| t.trail.iter().copied().collect())
                .unwrap_or_default();
            let seq = tape.map(|t| t.seq).unwrap_or(0);
            let use_live = *name == focused;
            let mut launch = if use_live {
                LaunchView::from_vessel(name, &ship.vessel, &trail, seq, ship.warp)
            } else if let Some(sample) = tape.and_then(|t| t.sample.as_ref()) {
                let mut vessel = ship.vessel;
                sample.apply_to(&mut vessel);
                LaunchView::from_vessel(name, &vessel, &trail, seq, sample.warp)
            } else {
                LaunchView::from_vessel(name, &ship.vessel, &trail, seq, ship.warp)
            };
            launch.parent = ship.parent.clone();
            launch.fork_seq = ship.fork_seq;
            launch.design = ship.design.clone();
            launch.stage = if use_live {
                ship.stage
            } else {
                tape.and_then(|t| t.sample.as_ref())
                    .map(|s| s.stage)
                    .unwrap_or(ship.stage)
            };
            launch.autopilot = ship.autopilot.enabled;
            launch.throttle = if use_live {
                ship.vessel.throttle
            } else {
                tape.and_then(|t| t.sample.as_ref())
                    .map(|s| s.throttle)
                    .unwrap_or(ship.vessel.throttle)
            };
            // As flown, not as designed. Reporting the spec's wet mass for the
            // launch you are watching froze the MASS gauge at the number the
            // craft weighed on the pad, and made a fresh fork look like it had
            // arrived with full tanks - the one thing a copy must not do.
            if !use_live {
                if let Some(sample) = tape.and_then(|t| t.sample.as_ref()) {
                    launch.mass = sample.mass;
                    launch.fuel = sample.fuel;
                }
            }
            views.push(launch);
        }
        drop(launches);
        drop(tapes);
        let saves = self.counters.persist_saves.load(Ordering::Relaxed);
        let persist_ms = self.counters.last_persist_us.load(Ordering::Relaxed) as f64 / 1000.0;
        let avg_persist_ms = if saves == 0 {
            0.0
        } else {
            (self.counters.persist_sum_us.load(Ordering::Relaxed) as f64 / 1000.0) / saves as f64
        };
        Snapshot {
            running: self.is_running(),
            hz: self.hz(),
            persist_ms,
            avg_persist_ms,
            max_persist_ms: self.counters.persist_max_us.load(Ordering::Relaxed) as f64 / 1000.0,
            commits_this_persist: self.counters.last_commits.load(Ordering::Relaxed),
            total_commits: self.counters.total_commits.load(Ordering::Relaxed),
            branch_count: self.branch_count.load(Ordering::Relaxed),
            durable: self.durable,
            db_path: self.db_path.clone(),
            physics_steps_last_frame: self.steps_last_frame.load(Ordering::Relaxed),
            live_launch_cap: LIVE_LAUNCH_CAP as u32,
            vab,
            focused,
            launches: views,
            findings: self.findings.snapshot(),
            last_compare: self.last_compare.lock().expect("compare lock").clone(),
            last_promote: self.last_promote.lock().expect("promote lock").clone(),
            last_archive: self.last_archive.lock().expect("archive lock").clone(),
        }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        // Nothing to wind down without a worker: jobs already ran inline.
        #[cfg(feature = "server")]
        {
            *self.persist_tx.lock().expect("persist tx") = None;
            if let Some(handle) = self.persist_join.lock().expect("persist join").take() {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(feature = "server")]
fn spawn_persist_worker(
    rx: Receiver<PersistJob>,
    db: Arc<Mutex<Database>>,
    tapes: Arc<Mutex<BTreeMap<String, TapeView>>>,
    findings: Arc<Log>,
    counters: Arc<PersistCounters>,
) -> Result<JoinHandle<()>, String> {
    thread::Builder::new()
        .name("ksp-persist".into())
        .spawn(move || persist_loop(rx, db, tapes, findings, counters))
        .map_err(|e| e.to_string())
}

/* One persist job, run to completion.
 *
 * The native build hands these to a worker thread so a slow commit cannot
 * stall the simulation. The browser build has one thread and calls this
 * directly - which is the same thing the callers already wait for, since
 * every discrete job blocks on its ack anyway. */
fn run_persist_job(
    job: PersistJob,
    db: &Mutex<Database>,
    tapes: &Mutex<BTreeMap<String, TapeView>>,
    findings: &Log,
    counters: &PersistCounters,
) {
    let outcome = {
        let mut db = db.lock().expect("db lock");
        match &job.kind {
            PersistKind::Tick => store::persist_tick(&mut db, &job.launch, &job.sample),
            PersistKind::Discrete {
                event_type,
                payload,
                graph_delete,
                spec,
                rebuild_graph,
                ..
            } => store::persist_discrete(
                &mut db,
                &job.launch,
                event_type,
                payload.clone(),
                &job.sample,
                graph_delete,
                spec.as_ref(),
                *rebuild_graph,
            ),
        }
    };
    match outcome {
        Ok((seq, stats)) => {
            if stats.commits > 0 {
                counters.record(&stats);
            }
            {
                let mut tapes = tapes.lock().expect("tapes lock");
                let tape = tapes.entry(job.launch.clone()).or_default();
                tape.seq = seq;
                let mut sample = job.sample.clone();
                sample.last_event_seq = seq;
                if let PersistKind::Discrete {
                    replace_trail: Some(trail),
                    ..
                } = &job.kind
                {
                    tape.trail = trail.iter().copied().collect();
                } else {
                    tape.trail.push_back(sample.trail_sample());
                }
                while tape.trail.len() > TAPE_CAP {
                    tape.trail.pop_front();
                }
                tape.sample = Some(sample);
            }
            if stats.commits == 0 && matches!(job.kind, PersistKind::Tick) {
                findings.push(Finding::new(
                    Kind::Note,
                    "engine.event",
                    "Tick appends stopped at the 20k soft cap",
                    "Discrete events still append. Archive the launch to reclaim the tape.",
                ));
            }
            if let Some(ack) = job.ack {
                let _ = ack.send(PersistAck {
                    seq,
                    result: Ok(()),
                });
            }
        }
        Err(error) => {
            findings.from_engine(Kind::Friction, "engine.event", "Persist job failed", &error);
            if let Some(ack) = job.ack {
                let _ = ack.send(PersistAck {
                    seq: 0,
                    result: Err(eng(error)),
                });
            }
        }
    }
}

#[cfg(feature = "server")]
fn persist_loop(
    rx: Receiver<PersistJob>,
    db: Arc<Mutex<Database>>,
    tapes: Arc<Mutex<BTreeMap<String, TapeView>>>,
    findings: Arc<Log>,
    counters: Arc<PersistCounters>,
) {
    while let Ok(job) = rx.recv() {
        run_persist_job(job, &db, &tapes, &findings, &counters);
    }
}

fn ship_from_resume(
    name: String,
    parent: String,
    fork_seq: u64,
    design: String,
    resume: store::ResumeLaunch,
) -> LiveShip {
    let mut vessel = Vessel::at_pad(resume.spec.wet_mass(), resume.spec.fuel());
    resume.sample.apply_to(&mut vessel);
    LiveShip {
        name,
        parent,
        fork_seq,
        design,
        vessel,
        spec: resume.spec,
        stage: resume.sample.stage,
        autopilot: AutoPilot {
            enabled: resume.sample.autopilot,
            phase: resume.sample.phase,
        },
        warp: resume.sample.warp.max(1),
        graph_ids: resume.graph_ids,
        last_persist_t: resume.sample.t,
        pending_tick: None,
    }
}

fn apply_resume(ship: &mut LiveShip, resume: store::ResumeLaunch) {
    resume.sample.apply_to(&mut ship.vessel);
    ship.spec = resume.spec;
    ship.stage = resume.sample.stage;
    ship.autopilot.phase = resume.sample.phase;
    ship.graph_ids = resume.graph_ids;
    ship.last_persist_t = resume.sample.t;
    ship.pending_tick = None;
}

fn wait_ack(rx: mpsc::Receiver<PersistAck>) -> Result<u64, String> {
    match rx.recv() {
        Ok(ack) => ack.result.map(|()| ack.seq),
        Err(_) => Err("internal.ksp.persist: persist worker dropped the ack".into()),
    }
}

fn parse_numbered(prefix: &str, name: &str) -> Option<u64> {
    name.strip_prefix(prefix)?.parse().ok()
}

fn eng(error: stratadb::EngineError) -> String {
    format!("{}: {}", error.code(), error.message())
}

fn ensure_err(error: EnsureError) -> String {
    match error {
        EnsureError::Engine(error) => eng(error),
        EnsureError::PlanetVersion => {
            "failed_precondition.ksp.planet_version: compiled planet does not match".into()
        }
        EnsureError::CatalogVersion => {
            "failed_precondition.ksp.catalog_version: compiled catalog does not match".into()
        }
        EnsureError::Craft(message) => format!("invalid_argument.ksp.craft: {message}"),
    }
}
