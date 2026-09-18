//! Twenty colonies, one seed, one exclusive database handle.

use crate::findings::{known_at_compile_time, Finding, Kind, Log};
use crate::life::Board;
use crate::patterns::{colony_name, genesis, perturbation, COLONY_COUNT, SEED_BRANCH};
use crate::store::{
    self, board_from_tick, colony_status, expected_colony_names, tick_payload, CompareView,
    PersistMode, PersistStats,
};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use stratadb::Database;

const HISTORY: usize = 240;

pub struct Colony {
    pub index: usize,
    pub name: String,
    pub board: Board,
    pub perturbed: Option<(u32, u32)>,
    pub divergence: u32,
    pub history: VecDeque<u32>,
    pub prev_live: HashSet<(u32, u32)>,
}

impl Colony {
    fn view(&self) -> ColonyView {
        ColonyView {
            id: self.index,
            name: self.name.clone(),
            live: self.board.live_count(),
            divergence: self.divergence,
            perturbed: self.perturbed.map(|(x, y)| [x, y]),
            fingerprint: format!("{:016x}", self.board.fingerprint()),
            board: B64.encode(self.board.packed()),
            history: self.history.iter().copied().collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ColonyView {
    pub id: usize,
    pub name: String,
    pub live: u32,
    pub divergence: u32,
    pub perturbed: Option<[u32; 2]>,
    pub fingerprint: String,
    pub board: String,
    pub history: Vec<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub generation: u64,
    pub running: bool,
    pub persist_ms: f64,
    pub commits_this_tick: u64,
    pub total_commits: u64,
    pub tick_hz: f64,
    pub width: u32,
    pub height: u32,
    pub persist_mode: String,
    pub durable: bool,
    pub db_path: String,
    pub colonies: Vec<ColonyView>,
    pub findings: Vec<Finding>,
    pub last_compare: Option<CompareView>,
    pub avg_persist_ms: f64,
    pub max_persist_ms: f64,
}

pub struct World {
    mutation: Mutex<()>,
    db: Mutex<Database>,
    colonies: Mutex<Vec<Colony>>,
    findings: Log,
    running: AtomicBool,
    generation: AtomicU64,
    period_ms: AtomicU64,
    total_commits: AtomicU64,
    persist_sum_us: AtomicU64,
    persist_max_us: AtomicU64,
    persist_ticks: AtomicU64,
    last_persist_us: AtomicU64,
    last_commits: AtomicU64,
    last_compare: Mutex<Option<CompareView>>,
    width: u32,
    height: u32,
    seed: u64,
    persist_mode: PersistMode,
    durable: bool,
    db_path: String,
}

pub struct OpenArgs {
    pub width: u32,
    pub height: u32,
    pub seed: u64,
    pub persist_mode: PersistMode,
    pub cache: bool,
    pub db_path: PathBuf,
}

impl World {
    pub fn open(args: OpenArgs) -> Result<Self, String> {
        let findings = Log::new();
        for finding in known_at_compile_time() {
            findings.push(finding);
        }

        let (db, durable, db_path) = if args.cache {
            findings.push(Finding::new(
                Kind::Note,
                "engine.open",
                "Cache mode: twenty branches, zero durability",
                "open_cache skips WAL, manifest, snapshot, and locks. Fine for a live demo; a process kill forgets every colony.",
            ));
            (store::open_cache().map_err(eng)?, false, "cache".to_owned())
        } else {
            std::fs::create_dir_all(&args.db_path).map_err(|e| e.to_string())?;
            (
                store::open_local(&args.db_path).map_err(eng)?,
                true,
                args.db_path.display().to_string(),
            )
        };

        let world = Self {
            mutation: Mutex::new(()),
            db: Mutex::new(db),
            colonies: Mutex::new(Vec::new()),
            findings,
            running: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            period_ms: AtomicU64::new(125),
            total_commits: AtomicU64::new(0),
            persist_sum_us: AtomicU64::new(0),
            persist_max_us: AtomicU64::new(0),
            persist_ticks: AtomicU64::new(0),
            last_persist_us: AtomicU64::new(0),
            last_commits: AtomicU64::new(0),
            last_compare: Mutex::new(None),
            width: args.width,
            height: args.height,
            seed: args.seed,
            persist_mode: args.persist_mode,
            durable,
            db_path,
        };
        world.bootstrap()?;
        Ok(world)
    }

    fn bootstrap(&self) -> Result<(), String> {
        let mut db = self.db.lock().map_err(|e| e.to_string())?;
        let names = store::list_product_branches(&mut db).map_err(eng)?;
        let expected = expected_colony_names();
        let have_all = expected.iter().all(|name| names.iter().any(|n| n == name));

        if have_all {
            let mut colonies = Vec::with_capacity(COLONY_COUNT);
            let mut generation = 0u64;
            for (index, name) in expected.iter().enumerate() {
                let board = store::read_board(&mut db, name)
                    .map_err(eng)?
                    .ok_or_else(|| format!("branch {name} has no board blob"))?;
                let status = store::read_status(&mut db, name).map_err(eng)?;
                generation = status
                    .get("generation")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(generation);
                let perturbed = status.get("perturbed").and_then(|value| {
                    let arr = value.as_array()?;
                    Some((arr.first()?.as_u64()? as u32, arr.get(1)?.as_u64()? as u32))
                });
                colonies.push(Colony {
                    index,
                    name: name.clone(),
                    prev_live: board.live_cells(),
                    board,
                    perturbed,
                    divergence: 0,
                    history: VecDeque::new(),
                });
            }
            self.recompute_divergence(&mut colonies);
            self.generation.store(generation, Ordering::Relaxed);
            *self.colonies.lock().map_err(|e| e.to_string())? = colonies;
            self.findings.push(Finding::new(
                Kind::Note,
                "demo",
                "Resumed twenty colonies from the existing database",
                format!(
                    "Opened {path} and loaded boards from KV blobs on each branch.",
                    path = self.db_path
                ),
            ));
            return Ok(());
        }

        drop(names);
        let board = genesis(self.width, self.height, self.seed);
        let mut stats = PersistStats::default();
        self.persist_colony(
            &mut db,
            SEED_BRANCH,
            0,
            None,
            &board,
            0,
            "genesis",
            None,
            &mut stats,
        )?;
        if let Err(error) = store::write_lineage_graph(&mut db, &self.findings) {
            self.findings.from_engine(
                Kind::Friction,
                "engine.graph",
                "Lineage graph was not written",
                &error,
            );
        }

        for index in 0..COLONY_COUNT {
            let name = colony_name(index);
            match store::fork_from_seed(&mut db, &name) {
                Ok(()) => {}
                Err(error) if error.code() == "already_exists.engine.branch" => {}
                Err(error) => return Err(eng(error)),
            }
        }

        let mut colonies = Vec::with_capacity(COLONY_COUNT);
        let mut used_lies = HashSet::new();
        for index in 0..COLONY_COUNT {
            let name = colony_name(index);
            let mut colony_board = board.clone();
            let perturbed = if index == 0 {
                None
            } else {
                let cell = perturbation(&colony_board, index, &used_lies);
                used_lies.insert(cell);
                colony_board.flip(cell.0, cell.1);
                Some(cell)
            };
            if perturbed.is_some() {
                self.persist_colony(
                    &mut db,
                    &name,
                    index,
                    perturbed,
                    &colony_board,
                    0,
                    "perturb",
                    None,
                    &mut stats,
                )?;
            }
            colonies.push(Colony {
                index,
                name,
                prev_live: colony_board.live_cells(),
                board: colony_board,
                perturbed,
                divergence: 0,
                history: VecDeque::new(),
            });
        }
        self.recompute_divergence(&mut colonies);
        self.record_stats(&stats);
        *self.colonies.lock().map_err(|e| e.to_string())? = colonies;
        Ok(())
    }

    fn persist_colony(
        &self,
        db: &mut Database,
        name: &str,
        index: usize,
        perturbed: Option<(u32, u32)>,
        board: &Board,
        generation: u64,
        kind: &str,
        prev_live: Option<&HashSet<(u32, u32)>>,
        stats: &mut PersistStats,
    ) -> Result<(), String> {
        let divergence = 0;
        store::write_board(
            db,
            name,
            board,
            prev_live,
            self.persist_mode,
            stats,
            &self.findings,
        )
        .map_err(eng)?;
        store::write_status(
            db,
            name,
            colony_status(name, index, board, generation, perturbed, divergence),
            stats,
        )
        .map_err(eng)?;
        store::append_tick(db, name, tick_payload(generation, board, kind), stats).map_err(eng)?;
        Ok(())
    }

    fn recompute_divergence(&self, colonies: &mut [Colony]) {
        let control = colonies
            .first()
            .map(|c| c.board.clone())
            .unwrap_or_else(|| Board::new(self.width, self.height));
        for colony in colonies.iter_mut() {
            colony.divergence = colony.board.divergence(&control);
            colony.history.push_back(colony.divergence);
            if colony.history.len() > HISTORY {
                colony.history.pop_front();
            }
        }
    }

    fn record_stats(&self, stats: &PersistStats) {
        self.total_commits
            .fetch_add(stats.commits, Ordering::Relaxed);
        self.last_commits.store(stats.commits, Ordering::Relaxed);
        self.last_persist_us
            .store(stats.total_us(), Ordering::Relaxed);
        self.persist_sum_us
            .fetch_add(stats.total_us(), Ordering::Relaxed);
        self.persist_ticks.fetch_add(1, Ordering::Relaxed);
        let mut max = self.persist_max_us.load(Ordering::Relaxed);
        while stats.total_us() > max {
            match self.persist_max_us.compare_exchange(
                max,
                stats.total_us(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(now) => max = now,
            }
        }
    }

    pub fn tick(&self) -> Result<Snapshot, String> {
        let _mutation = self.mutation.lock().map_err(|e| e.to_string())?;
        let started = Instant::now();
        let generation = self.generation.load(Ordering::Relaxed) + 1;
        let next_boards;
        let prev_live;
        let perturbed;
        {
            let colonies = self.colonies.lock().map_err(|e| e.to_string())?;
            next_boards = colonies.iter().map(|c| c.board.step()).collect::<Vec<_>>();
            prev_live = colonies
                .iter()
                .map(|c| c.prev_live.clone())
                .collect::<Vec<_>>();
            perturbed = colonies.iter().map(|c| c.perturbed).collect::<Vec<_>>();
        }

        let mut stats = PersistStats::default();
        {
            let mut db = self.db.lock().map_err(|e| e.to_string())?;
            for (index, board) in next_boards.iter().enumerate() {
                self.persist_colony(
                    &mut db,
                    &colony_name(index),
                    index,
                    perturbed[index],
                    board,
                    generation,
                    "tick",
                    Some(&prev_live[index]),
                    &mut stats,
                )?;
            }
        }

        {
            let mut colonies = self.colonies.lock().map_err(|e| e.to_string())?;
            for (index, colony) in colonies.iter_mut().enumerate() {
                colony.board = next_boards[index].clone();
                colony.prev_live = colony.board.live_cells();
            }
            self.recompute_divergence(&mut colonies);
            self.generation.store(generation, Ordering::Relaxed);
        }

        self.record_stats(&stats);
        let elapsed = started.elapsed();
        if elapsed.as_millis() > 250 {
            self.findings.push(Finding::new(
                Kind::Note,
                "demo",
                "A tick took more than 250ms",
                format!(
                    "generation {generation} spent {:.1}ms on twenty serialized branch commits ({commits} commits).",
                    elapsed.as_secs_f64() * 1000.0,
                    commits = stats.commits
                ),
            ));
        }
        self.snapshot()
    }

    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let colonies = self.colonies.lock().map_err(|e| e.to_string())?;
        let ticks = self.persist_ticks.load(Ordering::Relaxed).max(1);
        let avg = self.persist_sum_us.load(Ordering::Relaxed) as f64 / ticks as f64 / 1000.0;
        Ok(Snapshot {
            generation: self.generation.load(Ordering::Relaxed),
            running: self.running.load(Ordering::Relaxed),
            persist_ms: self.last_persist_us.load(Ordering::Relaxed) as f64 / 1000.0,
            commits_this_tick: self.last_commits.load(Ordering::Relaxed),
            total_commits: self.total_commits.load(Ordering::Relaxed),
            tick_hz: 1000.0 / self.period_ms.load(Ordering::Relaxed).max(1) as f64,
            width: self.width,
            height: self.height,
            persist_mode: match self.persist_mode {
                PersistMode::Blob => "blob".into(),
                PersistMode::Cells => "cells".into(),
            },
            durable: self.durable,
            db_path: self.db_path.clone(),
            colonies: colonies.iter().map(Colony::view).collect(),
            findings: self.findings.snapshot(),
            last_compare: self.last_compare.lock().map_err(|e| e.to_string())?.clone(),
            avg_persist_ms: avg,
            max_persist_ms: self.persist_max_us.load(Ordering::Relaxed) as f64 / 1000.0,
        })
    }

    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Relaxed);
    }

    pub fn close(&self) -> Result<(), String> {
        let _mutation = self.mutation.lock().map_err(|e| e.to_string())?;
        self.db
            .lock()
            .map_err(|e| e.to_string())?
            .close()
            .map_err(eng)?;
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn period_ms(&self) -> u64 {
        self.period_ms.load(Ordering::Relaxed).max(16)
    }

    pub fn set_hz(&self, hz: f64) {
        let hz = hz.clamp(0.5, 30.0);
        self.period_ms
            .store((1000.0 / hz) as u64, Ordering::Relaxed);
    }

    pub fn perturb(&self, name: &str, x: u32, y: u32) -> Result<Snapshot, String> {
        let _mutation = self.mutation.lock().map_err(|e| e.to_string())?;
        if x >= self.width || y >= self.height {
            return Err(format!("cell ({x}, {y}) is outside the board"));
        }
        let generation = self.generation.load(Ordering::Relaxed);
        let mut stats = PersistStats::default();
        {
            let colonies = self.colonies.lock().map_err(|e| e.to_string())?;
            let colony = colonies
                .iter()
                .find(|c| c.name == name)
                .ok_or_else(|| format!("no colony named {name}"))?;
            let mut board = colony.board.clone();
            board.flip(x, y);
            let prev = colony.prev_live.clone();
            let index = colony.index;
            drop(colonies);

            let mut db = self.db.lock().map_err(|e| e.to_string())?;
            self.persist_colony(
                &mut db,
                name,
                index,
                Some((x, y)),
                &board,
                generation,
                "perturb",
                Some(&prev),
                &mut stats,
            )?;
            drop(db);
            let mut colonies = self.colonies.lock().map_err(|e| e.to_string())?;
            colonies[index].prev_live = board.live_cells();
            colonies[index].board = board;
            colonies[index].perturbed = Some((x, y));
        }
        {
            let mut colonies = self.colonies.lock().map_err(|e| e.to_string())?;
            self.recompute_divergence(&mut colonies);
        }
        self.record_stats(&stats);
        self.snapshot()
    }

    pub fn rewind(&self, generation: u64) -> Result<Snapshot, String> {
        let _mutation = self.mutation.lock().map_err(|e| e.to_string())?;
        let mut stats = PersistStats::default();
        let mut restored = Vec::new();
        {
            let mut db = self.db.lock().map_err(|e| e.to_string())?;
            for index in 0..COLONY_COUNT {
                let name = colony_name(index);
                let payload = store::tick_at_generation(&mut db, &name, generation)
                    .map_err(eng)?
                    .ok_or_else(|| format!("{name} has no tick at generation {generation}"))?;
                let board = board_from_tick(&payload)
                    .ok_or_else(|| format!("{name} tick payload did not decode"))?;
                restored.push(board);
            }
            let colonies = self.colonies.lock().map_err(|e| e.to_string())?;
            let perturbed: Vec<_> = colonies.iter().map(|c| c.perturbed).collect();
            drop(colonies);
            for (index, board) in restored.iter().enumerate() {
                self.persist_colony(
                    &mut db,
                    &colony_name(index),
                    index,
                    perturbed[index],
                    board,
                    generation,
                    "rewind",
                    None,
                    &mut stats,
                )?;
            }
        }
        {
            let mut colonies = self.colonies.lock().map_err(|e| e.to_string())?;
            for (index, colony) in colonies.iter_mut().enumerate() {
                colony.prev_live = restored[index].live_cells();
                colony.board = restored[index].clone();
            }
            self.recompute_divergence(&mut colonies);
            self.generation.store(generation, Ordering::Relaxed);
        }
        self.record_stats(&stats);
        self.findings.push(Finding::new(
            Kind::Note,
            "engine.event",
            "Rewind walked the event log, not KV history",
            "KvService::get_at exists, but a packed board at generation N is easiest to recover from the tick event we appended. Event.range has no as_of twin; we scan payloads for generation.",
        ));
        self.snapshot()
    }

    pub fn compare(&self, name: &str) -> Result<Snapshot, String> {
        let started = Instant::now();
        let view = {
            let mut db = self.db.lock().map_err(|e| e.to_string())?;
            store::compare_to_control(&mut db, name).map_err(eng)?
        };
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        self.findings.push(Finding::new(
            Kind::Note,
            "engine.branch",
            "branch.compare scans every authored entity",
            format!(
                "Comparing control vs {name} took {ms:.1}ms and reported {} added, {} removed, {} modified across {:?}.",
                view.added, view.removed, view.modified, view.capabilities
            ),
        ));
        *self.last_compare.lock().map_err(|e| e.to_string())? = Some(view);
        self.snapshot()
    }

    pub fn audit_chains(&self) -> Result<Snapshot, String> {
        let mut db = self.db.lock().map_err(|e| e.to_string())?;
        let mut bad = Vec::new();
        for name in expected_colony_names() {
            match store::verify_chain(&mut db, &name) {
                Ok(true) => {}
                Ok(false) => bad.push(name),
                Err(error) => {
                    self.findings.from_engine(
                        Kind::Bug,
                        "engine.event",
                        &format!("verify_chain failed on {name}"),
                        &error,
                    );
                    bad.push(name);
                }
            }
        }
        if bad.is_empty() {
            self.findings.push(Finding::new(
                Kind::Note,
                "engine.event",
                "All twenty event chains verified",
                "EventService::verify_chain reports density + hash linkage on every colony branch.",
            ));
        } else {
            self.findings.push(Finding::new(
                Kind::Bug,
                "engine.event",
                "Some event chains failed verification",
                format!("Broken branches: {}", bad.join(", ")),
            ));
        }
        drop(db);
        self.snapshot()
    }

    pub fn reset(&self) -> Result<Snapshot, String> {
        let _mutation = self.mutation.lock().map_err(|e| e.to_string())?;
        self.set_running(false);
        {
            let mut db = self.db.lock().map_err(|e| e.to_string())?;
            for name in expected_colony_names() {
                match store::delete_branch(&mut db, &name) {
                    Ok(()) => {}
                    Err(error)
                        if error.code() == "not_found.engine.branch"
                            || error.code() == "invalid_argument.engine.branch_delete" => {}
                    Err(error) => {
                        self.findings.from_engine(
                            Kind::Friction,
                            "engine.branch",
                            &format!("Could not delete {name} on reset"),
                            &error,
                        );
                    }
                }
            }
        }
        self.generation.store(0, Ordering::Relaxed);
        self.bootstrap()?;
        self.findings.push(Finding::new(
            Kind::Note,
            "engine.branch",
            "Reset deletes mut-* / control and re-forks from default",
            "The seed branch cannot be deleted (it is default, and the last-branch rule). Event logs die with the deleted branches; that is the only reset the public API offers.",
        ));
        self.snapshot()
    }
}

fn eng(error: stratadb::EngineError) -> String {
    format!("{}: {}", error.code(), error.message())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::PersistMode;

    fn cache_world(mode: PersistMode) -> World {
        World::open(OpenArgs {
            width: 16,
            height: 12,
            seed: 7,
            persist_mode: mode,
            cache: true,
            db_path: PathBuf::new(),
        })
        .expect("open cache world")
    }

    #[test]
    fn durable_world_resumes_after_close() {
        let directory = tempfile::tempdir().unwrap();
        let open = || {
            World::open(OpenArgs {
                width: 16,
                height: 12,
                seed: 7,
                persist_mode: PersistMode::Blob,
                cache: false,
                db_path: directory.path().to_path_buf(),
            })
            .unwrap()
        };
        let world = open();
        world.tick().unwrap();
        world.reset().unwrap();
        let saved = world.tick().unwrap();
        world.close().unwrap();
        drop(world);
        let world = open();
        let resumed = world.snapshot().unwrap();
        assert_eq!(saved.generation, resumed.generation);
        for (before, after) in saved.colonies.iter().zip(&resumed.colonies) {
            assert_eq!(before.board, after.board);
        }
        world.close().unwrap();
    }

    #[test]
    fn rewind_restores_mutations_and_latest_edit_at_generation() {
        let world = cache_world(PersistMode::Blob);
        let original = world.snapshot().unwrap();
        world.tick().unwrap();
        let restored = world.rewind(0).unwrap();
        for (before, after) in original.colonies.iter().zip(&restored.colonies) {
            assert_eq!(before.board, after.board, "{}", before.name);
        }
        let edited = world.perturb("mut-01", 1, 1).unwrap();
        world.tick().unwrap();
        let restored = world.rewind(0).unwrap();
        assert_eq!(edited.colonies[1].board, restored.colonies[1].board);
    }

    fn assert_sparse_keys_match(world: &World) {
        let colonies = world.colonies.lock().unwrap();
        let db = world.db.lock().unwrap();
        for colony in colonies.iter() {
            let mut kv = db
                .kv(
                    store::branch(&colony.name).unwrap(),
                    store::space().unwrap(),
                )
                .unwrap();
            let actual: HashSet<_> = kv
                .list(Some(&store::kv_key(b"c:").unwrap()))
                .unwrap()
                .into_iter()
                .map(|key| key.as_bytes().to_vec())
                .collect();
            let expected: HashSet<_> = colony
                .board
                .live_cells()
                .into_iter()
                .map(|(x, y)| format!("c:{x}:{y}").into_bytes())
                .collect();
            assert_eq!(actual, expected, "{}", colony.name);
        }
    }

    #[test]
    fn sparse_keys_follow_ticks_edits_and_rewinds() {
        let world = cache_world(PersistMode::Cells);
        assert_sparse_keys_match(&world);
        for _ in 0..3 {
            world.tick().unwrap();
            assert_sparse_keys_match(&world);
        }
        world.perturb("mut-01", 1, 1).unwrap();
        assert_sparse_keys_match(&world);
        world.rewind(0).unwrap();
        assert_sparse_keys_match(&world);
    }

    #[test]
    fn concurrent_ticks_match_sequential_evolution() {
        let world = cache_world(PersistMode::Blob);
        let expected: Vec<_> = world
            .colonies
            .lock()
            .unwrap()
            .iter()
            .map(|colony| colony.board.step().step().step().step())
            .collect();
        let barrier = std::sync::Barrier::new(4);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    barrier.wait();
                    world.tick().unwrap();
                });
            }
        });
        assert_eq!(world.snapshot().unwrap().generation, 4);
        for (colony, expected) in world.colonies.lock().unwrap().iter().zip(expected) {
            assert_eq!(colony.board, expected, "{}", colony.name);
        }
    }

    #[test]
    fn cache_world_forks_twenty_and_ticks() {
        let world = World::open(OpenArgs {
            width: 16,
            height: 12,
            seed: 7,
            persist_mode: PersistMode::Blob,
            cache: true,
            db_path: PathBuf::from("/tmp/unused-colonies"),
        })
        .expect("open cache world");
        let before = world.snapshot().expect("snapshot");
        assert_eq!(before.colonies.len(), 20);
        assert_eq!(before.colonies[0].name, "control");
        assert!(before.colonies[1].perturbed.is_some());
        assert!(before.colonies[1].divergence >= 1);
        let lies: HashSet<_> = before.colonies.iter().filter_map(|c| c.perturbed).collect();
        assert_eq!(lies.len(), 19);
        let after = world.tick().expect("tick");
        assert_eq!(after.generation, 1);
        assert!(after.commits_this_tick >= 60);
    }
}
