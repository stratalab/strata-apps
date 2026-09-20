//! Navigation console: curated places, graph discovery, scenarios, and history.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use strata_island::error::IslandError;
use strata_island::extract::GazetteerPoi;
use strata_island::places;
use strata_island::snapshot;
use strata_island::world::{OpenArgs, World};
use tokio::sync::Semaphore;

const INDEX: &str = include_str!("../static/index.html");
const CSS: &str = include_str!("../static/style.css");
const JS: &str = include_str!("../static/app.js");

#[derive(Parser, Debug)]
#[command(
    name = "island",
    about = "Manhattan drive map on Strata — close 42nd, compare, audit. Not a legal drive.",
    after_help = "Default is durable ./island-db. While this process holds --db, `strata ./island-db branch list` is unavailable.engine.persistence. Do not shell out to strata. V2 uses a separate ./island-db-v2 directory. Durable writes sync before acknowledgement. http://127.0.0.1:7450"
)]
struct Cli {
    /// Dataset selection. V2 has landmarks, MTA subway stations, and graph discovery.
    #[arg(long, default_value = "v1", value_parser = ["v1", "v2"])]
    dataset: String,
    /// Durable database directory.
    #[arg(long)]
    db: Option<PathBuf>,
    /// In-memory cache mode (non-durable).
    #[arg(long)]
    cache: bool,
    /// Storage memory budget in MiB.
    #[arg(long)]
    memory_mb: Option<u64>,
    /// Bind address.
    #[arg(long, default_value = "127.0.0.1:7450")]
    bind: String,
}

struct AppState {
    world: Arc<World>,
    admission: Arc<Semaphore>,
    workers: Arc<Semaphore>,
    search_workers: Arc<Semaphore>,
    graph_jobs: AtomicU64,
    graph_micros: AtomicU64,
}

#[derive(Deserialize)]
struct CityQuery {
    branch: Option<String>,
}

#[derive(Deserialize)]
struct RouteReq {
    mode: Option<String>,
    branch: Option<String>,
    from: Option<String>,
    to: Option<String>,
    version: Option<u64>,
    from_xy: Option<[i32; 2]>,
    to_xy: Option<[i32; 2]>,
}

#[derive(Deserialize)]
struct CloseReq {
    request_id: Option<String>,
    from: Option<String>,
}

#[derive(Deserialize)]
struct CompareReq {
    a: Option<String>,
    b: Option<String>,
}

#[derive(Deserialize)]
struct ArchiveReq {
    desk: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let open = if cli.dataset == "v2" {
        World::open_v2
    } else {
        World::open
    };
    let world = open(OpenArgs {
        cache: cli.cache,
        db_path: cli.db.unwrap_or_else(|| {
            if cli.dataset == "v2" {
                "./island-db-v2".into()
            } else {
                "./island-db".into()
            }
        }),
        memory_budget_bytes: cli.memory_mb.map(|mb| mb.saturating_mul(1024 * 1024)),
    })
    .unwrap_or_else(|error| {
        eprintln!("failed to open world: {error}");
        std::process::exit(1);
    });
    let state = Arc::new(AppState {
        world: Arc::new(world),
        admission: Arc::new(Semaphore::new(10)),
        workers: Arc::new(Semaphore::new(2)),
        search_workers: Arc::new(Semaphore::new(4)),
        graph_jobs: AtomicU64::new(0),
        graph_micros: AtomicU64::new(0),
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/style.css", get(css))
        .route("/app.js", get(js))
        .route("/api/city", get(api_city))
        .route("/api/gazetteer", get(api_gazetteer))
        .route("/api/meta", get(api_meta))
        .route("/api/search", get(api_search))
        .route("/api/addresses/{id}", get(api_address))
        .route("/api/address-explore", post(api_address_explore))
        .route("/api/address-discover", post(api_address_discover))
        .route(
            "/api/addresses/{id}/nearest-stations",
            post(api_address_stations),
        )
        .route(
            "/api/scenarios/{id}/address-impact",
            post(api_address_impact),
        )
        .route("/api/places", get(api_places))
        .route("/api/places/{id}", get(api_place))
        .route("/api/discover", post(api_discover))
        .route("/api/explore", post(api_explore))
        .route("/api/subway", get(api_subway))
        .route("/api/subway/explore", post(api_subway_explore))
        .route("/api/scenarios/{id}/impact", post(api_impact))
        .route("/api/scenarios", post(api_scenario))
        .route("/api/scenarios/{id}/operations", post(api_operation))
        .route("/api/scenarios/{id}/history", get(api_history))
        .route("/api/graph-metrics", get(api_metrics))
        .route("/api/route", post(api_route))
        .route("/api/close", post(api_close))
        .route("/api/compare", post(api_compare))
        .route("/api/archive", post(api_archive))
        .route("/api/audit", post(api_audit))
        .with_state(state);

    let addr: SocketAddr = cli.bind.parse().unwrap_or_else(|_| {
        eprintln!("invalid --bind {}", cli.bind);
        std::process::exit(1);
    });
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("bind {addr}: {e}");
            std::process::exit(1);
        });
    println!("strata-island on http://{addr}");
    println!("  Navigation console — explore routes, create closure scenarios, compare, audit");
    println!(
        "  While the server holds its database, the Strata CLI cannot open the same directory."
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let interrupt = async {
                let _ = tokio::signal::ctrl_c().await;
            };
            #[cfg(unix)]
            {
                let mut terminate =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("signal handler");
                tokio::select! {_ = interrupt => {}, _ = terminate.recv() => {}}
            }
            #[cfg(not(unix))]
            interrupt.await;
        })
        .await
        .expect("server");
}

async fn index() -> Html<&'static str> {
    Html(INDEX)
}

async fn css() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/css; charset=utf-8"),
        )],
        CSS,
    )
        .into_response()
}

async fn js() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/javascript; charset=utf-8"),
        )],
        JS,
    )
        .into_response()
}

async fn api_city(State(state): State<Arc<AppState>>, Query(query): Query<CityQuery>) -> Response {
    match state.world.city_snapshot(query.branch.as_deref()) {
        Ok(index) => Json(snapshot::city_view(&index)).into_response(),
        Err(err) => island_err(err),
    }
}

async fn api_gazetteer(State(state): State<Arc<AppState>>) -> Json<Vec<GazetteerPoi>> {
    Json(snapshot::gazetteer_view(&state.world.gazetteer()))
}

async fn api_meta(State(state): State<Arc<AppState>>) -> Response {
    let mut meta = serde_json::to_value(snapshot::meta_view(&state.world)).expect("meta");
    meta["dataset"] = json!(if state.world.expanded() { "v2" } else { "v1" });
    meta["place_count"] = json!(state
        .world
        .place_snapshot("city")
        .map(|s| s.places.len())
        .unwrap_or(6));
    if let Ok(c) = state.world.address_catalog("city") {
        meta["address_count"] = json!(c.rows.len());
        meta["address_catalog"] = json!(c.hash);
        meta["address_graph"] = json!({"nodes":c.nodes,"edges":c.edges,"connected":c.rows.iter().filter(|a|a.place.node.is_some()).count()});
    }
    if state.world.expanded() {
        let data = strata_island::journeys::fixture();
        meta["journey_graph"] = json!({"nodes":data.nodes.len(),"edges":data.edges.len(),"reference_date":data.reference_date});
        meta["travel_modes"] = json!(["car", "transit"]);
    }
    meta["versions"] = json!(state
        .world
        .branch_views()
        .iter()
        .filter_map(|b| state
            .world
            .place_snapshot(&b.name)
            .ok()
            .map(|p| (b.name.clone(), p.version.as_u64())))
        .collect::<std::collections::BTreeMap<_, _>>());
    Json(meta).into_response()
}

async fn api_route(State(state): State<Arc<AppState>>, Json(req): Json<RouteReq>) -> Response {
    if req
        .mode
        .as_deref()
        .is_some_and(|m| !["car", "transit"].contains(&m))
    {
        return island_err(IslandError::code("invalid_argument.island.route_mode"));
    }
    graph_job(state, move |world| {
        let branch = req.branch.as_deref().unwrap_or("city");
        if req.mode.as_deref() == Some("transit") {
            world.transit_request(
                branch,
                req.from.as_deref(),
                req.to.as_deref(),
                req.from_xy,
                req.to_xy,
                req.version,
            )
        } else {
            let mut route = world.route_request(
                branch,
                req.from.as_deref(),
                req.to.as_deref(),
                req.from_xy,
                req.to_xy,
                req.version,
            )?;
            route["mode"] = json!("car");
            Ok(route)
        }
    })
    .await
}

async fn api_close(State(state): State<Arc<AppState>>, Json(req): Json<CloseReq>) -> Response {
    let world = Arc::clone(&state.world);
    match tokio::task::spawn_blocking(move || {
        world.create_closure_request(
            req.from.as_deref(),
            world.closure(),
            req.request_id.as_deref(),
        )
    })
    .await
    {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(err)) => island_err(err),
        Err(_) => island_err(IslandError::code("failed_precondition.island.city")),
    }
}

async fn api_compare(State(state): State<Arc<AppState>>, Json(req): Json<CompareReq>) -> Response {
    let a = req.a.unwrap_or_else(|| "city".to_owned());
    let b = match req.b.or_else(|| state.world.latest_desk()) {
        Some(desk) => desk,
        None => return island_err(IslandError::code("invalid_argument.island.branch")),
    };
    let world = Arc::clone(&state.world);
    match tokio::task::spawn_blocking(move || world.compare(&a, &b)).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(err)) => island_err(err),
        Err(_) => island_err(IslandError::code("failed_precondition.island.city")),
    }
}

async fn api_audit(State(state): State<Arc<AppState>>) -> Response {
    let world = Arc::clone(&state.world);
    match tokio::task::spawn_blocking(move || world.audit()).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(err)) => island_err(err),
        Err(_) => island_err(IslandError::code("failed_precondition.island.city")),
    }
}

async fn api_archive(State(state): State<Arc<AppState>>, Json(req): Json<ArchiveReq>) -> Response {
    let world = Arc::clone(&state.world);
    match tokio::task::spawn_blocking(move || world.archive(&req.desk)).await {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})).into_response(),
        Ok(Err(err)) => island_err(err),
        Err(_) => island_err(IslandError::code("failed_precondition.island.city")),
    }
}

fn status_for_code(code: &str) -> StatusCode {
    if code.starts_with("not_found.") {
        StatusCode::NOT_FOUND
    } else if code.starts_with("failed_precondition.") {
        StatusCode::PRECONDITION_FAILED
    } else {
        StatusCode::BAD_REQUEST
    }
}

fn island_err(err: IslandError) -> Response {
    (
        status_for_code(&err.code),
        Json(serde_json::json!({
            "code": err.code,
            "class": err.class(),
        })),
    )
        .into_response()
}

#[derive(Deserialize)]
struct PlacesQuery {
    branch: Option<String>,
    version: Option<u64>,
    q: Option<String>,
    category: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
}

async fn api_places(State(state): State<Arc<AppState>>, Query(q): Query<PlacesQuery>) -> Response {
    let branch = q.branch.as_deref().unwrap_or("city");
    let snap = match state.world.place_snapshot(branch) {
        Ok(s) => s,
        Err(e) => return island_err(e),
    };
    if q.version.is_some_and(|v| v != snap.version.as_u64()) {
        return island_err(IslandError::code("failed_precondition.island.version"));
    }
    let category = q.category.as_deref().filter(|s| !s.is_empty());
    let query = q.q.unwrap_or_default().to_lowercase();
    let limit = q.limit.unwrap_or(20);
    if query.len() > 200
        || !(1..=100).contains(&limit)
        || category.is_some_and(|c| !places::CATEGORIES.contains(&c))
    {
        return island_err(IslandError::code("invalid_argument.island.places"));
    }
    let signature = format!(
        "{:016x}",
        strata_island::extract::fnv1a64(
            format!("{branch}:{}:{category:?}:{query}", snap.version).as_bytes()
        )
    );
    let offset = match q.cursor {
        None => 0,
        Some(c) => match c.split_once(':') {
            Some((sig, n)) if sig == signature => match n.parse::<usize>() {
                Ok(n) => n,
                Err(_) => return island_err(IslandError::code("invalid_argument.island.cursor")),
            },
            _ => return island_err(IslandError::code("invalid_argument.island.cursor")),
        },
    };
    let matches: Vec<_> = snap
        .places
        .iter()
        .filter(|p| {
            category.is_none_or(|c| p.category == c)
                && query
                    .split_whitespace()
                    .all(|term| p.name.to_lowercase().contains(term))
        })
        .collect();
    let results: Vec<_> = matches.iter().skip(offset).take(limit).collect();
    let next = offset.saturating_add(results.len());
    Json(json!({"branch":branch,"version":snap.version,"dataset":format!("curated-{}",snap.places.len()),"total":matches.len(),"places":results,"cursor":if next<matches.len(){Some(format!("{signature}:{next}"))}else{None},"search_index":"application RAM, hydrated from Strata typed nodes and JSON"})).into_response()
}

async fn graph_job<F>(state: Arc<AppState>, job: F) -> Response
where
    F: FnOnce(&World) -> Result<Value, IslandError> + Send + 'static,
{
    let admission = match state.admission.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"code":"resource_exhausted.island.analysis"})),
            )
                .into_response()
        }
    };
    let worker = state
        .workers
        .clone()
        .acquire_owned()
        .await
        .expect("workers");
    let result = tokio::task::spawn_blocking(move || {
        let (_admission, _worker) = (admission, worker);
        let started = Instant::now();
        let result = job(&state.world);
        if let Err(error) = &result {
            state.world.record_graph_error(error);
        }
        state.graph_jobs.fetch_add(1, Ordering::Relaxed);
        state
            .graph_micros
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        result
    })
    .await;
    match result {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(e)) => island_err(e),
        Err(_) => island_err(IslandError::code("failed_precondition.island.analysis")),
    }
}
async fn api_place(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<CityQuery>,
) -> Response {
    graph_job(state, move |world| {
        world.place_detail(q.branch.as_deref().unwrap_or("city"), &id)
    })
    .await
}
#[derive(Deserialize)]
struct DiscoverReq {
    branch: Option<String>,
    origin: String,
    category: Option<String>,
    max_m: Option<u32>,
    version: Option<u64>,
}
async fn api_discover(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DiscoverReq>,
) -> Response {
    graph_job(state, move |world| {
        let branch = req.branch.as_deref().unwrap_or("city");
        let s = world.place_snapshot(branch)?;
        let historical = if let Some(v) = req.version.filter(|v| *v != s.version.as_u64()) {
            Some(world.historical_places(branch, v)?)
        } else {
            None
        };
        let resolved_origin = if req.origin.starts_with("a:nyc:") {
            let index = world.city_snapshot(Some(branch))?;
            index.node_ids[world.lookup_node(&index, &req.origin)?].clone()
        } else {
            req.origin.clone()
        };
        let mut result = places::discover(
            historical.as_ref().unwrap_or(&s),
            branch,
            &resolved_origin,
            req.category.as_deref().filter(|c| !c.is_empty()),
            req.max_m.unwrap_or(2000),
        )?;
        result["snapshot_cached"] = json!(historical.is_none());
        Ok(result)
    })
    .await
}
#[derive(Deserialize)]
struct ExploreReq {
    branch: Option<String>,
    seed: String,
    depth: Option<usize>,
    limit: Option<usize>,
}
async fn api_explore(State(state): State<Arc<AppState>>, Json(req): Json<ExploreReq>) -> Response {
    graph_job(state, move |world| {
        let branch = req.branch.as_deref().unwrap_or("city");
        places::explore(
            world.place_snapshot(branch)?.as_ref(),
            branch,
            &req.seed,
            req.depth.unwrap_or(2),
            req.limit.unwrap_or(30),
        )
    })
    .await
}
async fn api_subway(State(state): State<Arc<AppState>>, Query(q): Query<CityQuery>) -> Response {
    graph_job(state, move |world| {
        world.subway(q.branch.as_deref().unwrap_or("city"), None)
    })
    .await
}
async fn api_subway_explore(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExploreReq>,
) -> Response {
    graph_job(state, move |world| {
        world.subway(
            req.branch.as_deref().unwrap_or("city"),
            Some((&req.seed, req.depth.unwrap_or(2))),
        )
    })
    .await
}
async fn api_impact(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<DiscoverReq>,
) -> Response {
    graph_job(state, move |world| {
        places::impact(
            world.place_snapshot("city")?.as_ref(),
            world.place_snapshot(&id)?.as_ref(),
            &id,
            &req.origin,
        )
    })
    .await
}
async fn api_metrics(State(state): State<Arc<AppState>>) -> Response {
    Json(json!({"completed_jobs":state.graph_jobs.load(Ordering::Relaxed),"total_service_ms":state.graph_micros.load(Ordering::Relaxed) as f64/1000.,"active_jobs":2-state.workers.available_permits(),"admitted_jobs":10-state.admission.available_permits(),"note":"Storage snapshot construction is reported separately from cached Strata algorithms; pan/zoom runs locally."})).into_response()
}

#[derive(Deserialize)]
struct ScenarioReq {
    #[serde(flatten)]
    closure: strata_island::extract::ClosureFixture,
    request_id: Option<String>,
}
async fn api_scenario(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ScenarioReq>,
) -> Response {
    graph_job(state, move |world| {
        if !world.expanded() {
            return Err(IslandError::code("failed_precondition.island.dataset"));
        }
        Ok(serde_json::to_value(world.create_closure_request(
            None,
            &req.closure,
            req.request_id.as_deref(),
        )?)
        .expect("scenario"))
    })
    .await
}
#[derive(Deserialize)]
struct OperationReq {
    id: String,
    closed: bool,
    expected_version: u64,
}
async fn api_operation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<OperationReq>,
) -> Response {
    graph_job(state, move |world| {
        world.scenario_operation(&id, &req.id, req.closed, req.expected_version)
    })
    .await
}
async fn api_history(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    graph_job(state, move |world| world.scenario_history(&id)).await
}

#[derive(Deserialize, Default)]
struct AddressQuery {
    branch: Option<String>,
    version: Option<u64>,
    q: Option<String>,
    category: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}
async fn api_search(State(state): State<Arc<AppState>>, Query(q): Query<AddressQuery>) -> Response {
    let permit = match state.search_workers.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"code":"resource_exhausted.island.search"})),
            )
                .into_response()
        }
    };
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        state.world.search(
            q.branch.as_deref().unwrap_or("city"),
            q.version,
            q.q.as_deref().unwrap_or(""),
            q.category.as_deref().filter(|c| !c.is_empty()),
            q.limit.unwrap_or(8),
            q.cursor.as_deref(),
        )
    })
    .await
    {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => island_err(e),
        Err(_) => island_err(IslandError::code("failed_precondition.island.search")),
    }
}
async fn api_address(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<AddressQuery>,
) -> Response {
    graph_job(state, move |w| {
        w.address_detail(q.branch.as_deref().unwrap_or("city"), q.version, &id)
    })
    .await
}
#[derive(Deserialize, Default)]
struct AddressReq {
    branch: Option<String>,
    version: Option<u64>,
    seed: Option<String>,
    origin: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
    status: Option<String>,
}
async fn api_address_explore(
    State(state): State<Arc<AppState>>,
    Json(r): Json<AddressReq>,
) -> Response {
    graph_job(state, move |w| {
        w.address_explore(
            r.branch.as_deref().unwrap_or("city"),
            r.version,
            r.seed.as_deref().unwrap_or(""),
            r.limit.unwrap_or(24),
        )
    })
    .await
}
async fn api_address_stations(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(r): Json<AddressReq>,
) -> Response {
    graph_job(state, move |w| {
        w.address_stations(r.branch.as_deref().unwrap_or("city"), &id, r.version)
    })
    .await
}
async fn api_address_impact(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(r): Json<AddressReq>,
) -> Response {
    graph_job(state, move |w| {
        if r.version.is_some_and(|v| {
            w.place_snapshot(&id)
                .map_or(true, |p| p.version.as_u64() != v)
        }) {
            return Err(IslandError::code("failed_precondition.island.version"));
        }
        w.address_impact(
            &id,
            r.origin.as_deref().unwrap_or(""),
            r.status.as_deref(),
            r.cursor.as_deref(),
            r.limit.unwrap_or(20),
        )
    })
    .await
}

async fn api_address_discover(
    State(state): State<Arc<AppState>>,
    Json(r): Json<DiscoverReq>,
) -> Response {
    graph_job(state, move |w| {
        w.address_discover(
            r.branch.as_deref().unwrap_or("city"),
            r.version,
            &r.origin,
            r.max_m.unwrap_or(1000),
        )
    })
    .await
}
