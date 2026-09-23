//! PR9: golden freeze, archive launches, compare, clippy-clean.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use serde::Deserialize;
use strata_ksp::world::{OpenArgs, World};

const INDEX: &str = include_str!("../static/index.html");
const CSS: &str = include_str!("../static/style.css");
const JS: &str = include_str!("../static/app.js");

#[derive(Parser, Debug)]
#[command(
    name = "ksp",
    about = "Mini Kerbal on Strata — PR9 golden freeze, archive, compare.",
    after_help = "While this process holds --db, `strata ./ksp-db branch list` is unavailable.engine.persistence. Delete the directory to roll back."
)]
struct Cli {
    /// Durable database directory.
    #[arg(long, default_value = "./ksp-db")]
    db: PathBuf,
    /// In-memory cache mode (non-durable).
    #[arg(long)]
    cache: bool,
    /// Bind address.
    #[arg(long, default_value = "127.0.0.1:7430")]
    bind: String,
    /// Ticker frame rate (wall Hz). Period is 1000/hz ms.
    #[arg(long, default_value_t = 30.0)]
    hz: f64,
}

struct AppState {
    world: Arc<World>,
    tx: tokio::sync::broadcast::Sender<String>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    println!("opening Strata…");
    let world = World::open(OpenArgs {
        cache: cli.cache,
        db_path: cli.db,
    })
    .unwrap_or_else(|error| {
        eprintln!("failed to open world: {error}");
        std::process::exit(1);
    });
    world.set_hz(cli.hz);
    let world = Arc::new(world);

    let (tx, _rx) = tokio::sync::broadcast::channel::<String>(16);
    let state = Arc::new(AppState {
        world: world.clone(),
        tx: tx.clone(),
    });

    let ticker_world = world.clone();
    let ticker_tx = tx;
    tokio::spawn(async move {
        loop {
            let ms = ticker_world.period_ms().max(16);
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            if ticker_world.is_running() {
                ticker_world.tick_frame();
            }
            if let Ok(text) = serde_json::to_string(&ticker_world.snapshot()) {
                let _ = ticker_tx.send(text);
            }
        }
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/style.css", get(css))
        .route("/app.js", get(js))
        .route("/ws", get(ws_handler))
        .route("/api/state", get(api_state))
        .route("/api/run", post(api_run))
        .route("/api/pause", post(api_pause))
        .route("/api/warp", post(api_warp))
        .route("/api/reset", post(api_reset))
        .route("/api/stage", post(api_stage))
        .route("/api/vab/add", post(api_vab_add))
        .route("/api/vab/remove", post(api_vab_remove))
        .route("/api/vab/tune", post(api_vab_tune))
        .route("/api/vab/reset", post(api_vab_reset))
        .route("/api/vab/save", post(api_vab_save))
        .route("/api/launch", post(api_launch))
        .route("/api/autopilot", post(api_autopilot))
        .route("/api/throttle", post(api_throttle))
        .route("/api/audit", post(api_audit))
        .route("/api/focus", post(api_focus))
        .route("/api/fork", post(api_fork))
        .route("/api/rewind", post(api_rewind))
        .route("/api/add-tank", post(api_add_tank))
        .route("/api/promote", post(api_promote))
        .route("/api/compare", post(api_compare))
        .route("/api/archive", post(api_archive))
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
    println!("strata-ksp PR9 on http://{addr}");
    println!("  Frozen golden — archive launches, compare counts, design refcount");
    axum::serve(listener, app).await.expect("server");
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

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| socket_loop(socket, state))
}

async fn socket_loop(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.tx.subscribe();
    if let Ok(text) = serde_json::to_string(&state.world.snapshot()) {
        let _ = socket.send(Message::text(text)).await;
    }
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(p))) => {
                        let _ = socket.send(Message::Pong(p)).await;
                    }
                    _ => {}
                }
            }
            outgoing = rx.recv() => {
                match outgoing {
                    Ok(text) => {
                        if socket.send(Message::text(text)).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

async fn api_state(State(state): State<Arc<AppState>>) -> Response {
    Json(state.world.snapshot()).into_response()
}

async fn api_run(State(state): State<Arc<AppState>>) -> Response {
    state.world.set_running(true);
    Json(state.world.snapshot()).into_response()
}

async fn api_pause(State(state): State<Arc<AppState>>) -> Response {
    state.world.set_running(false);
    Json(state.world.snapshot()).into_response()
}

#[derive(Deserialize)]
struct WarpBody {
    mult: u32,
}

async fn api_warp(State(state): State<Arc<AppState>>, Json(body): Json<WarpBody>) -> Response {
    state.world.set_warp(body.mult);
    Json(state.world.snapshot()).into_response()
}

async fn api_reset(State(state): State<Arc<AppState>>) -> Response {
    api_launch(State(state)).await
}

#[derive(Deserialize)]
struct VabAddBody {
    part_id: String,
    index: Option<usize>,
}

async fn api_vab_add(State(state): State<Arc<AppState>>, Json(body): Json<VabAddBody>) -> Response {
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || world.vab_add(&body.part_id, body.index)).await {
        Ok(Ok(())) => Json(state.world.snapshot()).into_response(),
        Ok(Err(message)) => vab_error("invalid_argument.ksp.part", &message),
        Err(error) => join_error(&error.to_string()),
    }
}

#[derive(Deserialize)]
struct VabTuneBody {
    index: usize,
    fuel: Option<f64>,
    thrust_limit: Option<f64>,
}

async fn api_vab_tune(
    State(state): State<Arc<AppState>>,
    Json(body): Json<VabTuneBody>,
) -> Response {
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || {
        world.vab_tune(body.index, body.fuel, body.thrust_limit)
    })
    .await
    {
        Ok(Ok(())) => Json(state.world.snapshot()).into_response(),
        Ok(Err(message)) => vab_error("invalid_argument.ksp.part", &message),
        Err(error) => join_error(&error.to_string()),
    }
}

#[derive(Deserialize)]
struct VabRemoveBody {
    index: usize,
}

async fn api_vab_remove(
    State(state): State<Arc<AppState>>,
    Json(body): Json<VabRemoveBody>,
) -> Response {
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || world.vab_remove(body.index)).await {
        Ok(Ok(())) => Json(state.world.snapshot()).into_response(),
        Ok(Err(message)) => vab_error("invalid_argument.ksp.stack", &message),
        Err(error) => join_error(&error.to_string()),
    }
}

async fn api_vab_reset(State(state): State<Arc<AppState>>) -> Response {
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || world.vab_reset_stick()).await {
        Ok(Ok(())) => Json(state.world.snapshot()).into_response(),
        Ok(Err(message)) => vab_error("failed_precondition.ksp.vab", &message),
        Err(error) => join_error(&error.to_string()),
    }
}

async fn api_vab_save(State(state): State<Arc<AppState>>) -> Response {
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || world.vab_save()).await {
        Ok(Ok(())) => Json(state.world.snapshot()).into_response(),
        Ok(Err(message)) => persist_error(&message),
        Err(error) => join_error(&error.to_string()),
    }
}

async fn api_launch(State(state): State<Arc<AppState>>) -> Response {
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || world.launch_from_pad()).await {
        Ok(Ok(_)) => {
            state.world.set_running(true);
            Json(state.world.snapshot()).into_response()
        }
        Ok(Err(message)) => persist_error(&message),
        Err(error) => join_error(&error.to_string()),
    }
}

#[derive(Deserialize)]
struct AutopilotBody {
    on: bool,
}

async fn api_autopilot(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AutopilotBody>,
) -> Response {
    state.world.set_autopilot(body.on);
    Json(state.world.snapshot()).into_response()
}

#[derive(Deserialize)]
struct ThrottleBody {
    value: f64,
}

async fn api_throttle(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ThrottleBody>,
) -> Response {
    state.world.set_throttle(body.value);
    Json(state.world.snapshot()).into_response()
}

async fn api_stage(State(state): State<Arc<AppState>>) -> Response {
    match state.world.stage_flight() {
        Ok(_) => Json(state.world.snapshot()).into_response(),
        Err(message) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": { "code": "failed_precondition.ksp.stage", "message": message } })),
        )
            .into_response(),
    }
}

async fn api_audit(State(state): State<Arc<AppState>>) -> Response {
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || world.audit()).await {
        Ok(Ok(body)) => Json(body).into_response(),
        Ok(Err(message)) => persist_error(&message),
        Err(error) => join_error(&error.to_string()),
    }
}

#[derive(Deserialize)]
struct FocusBody {
    launch: String,
}

async fn api_focus(State(state): State<Arc<AppState>>, Json(body): Json<FocusBody>) -> Response {
    match state.world.set_focus(&body.launch) {
        Ok(()) => Json(state.world.snapshot()).into_response(),
        Err(message) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": { "code": "not_found.ksp.launch", "message": message } })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct ForkBody {
    from: String,
    at_seq: Option<u64>,
}

async fn api_fork(State(state): State<Arc<AppState>>, Json(body): Json<ForkBody>) -> Response {
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || world.fork_at(&body.from, body.at_seq)).await {
        Ok(Ok(_)) => Json(state.world.snapshot()).into_response(),
        Ok(Err(message)) => persist_error(&message),
        Err(error) => join_error(&error.to_string()),
    }
}

#[derive(Deserialize)]
struct RewindBody {
    launch: String,
    seq: u64,
}

async fn api_rewind(State(state): State<Arc<AppState>>, Json(body): Json<RewindBody>) -> Response {
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || world.rewind(&body.launch, body.seq)).await {
        Ok(Ok(())) => Json(state.world.snapshot()).into_response(),
        Ok(Err(message)) => persist_error(&message),
        Err(error) => join_error(&error.to_string()),
    }
}

#[derive(Deserialize)]
struct AddTankBody {
    launch: Option<String>,
    part_id: Option<String>,
}

async fn api_add_tank(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AddTankBody>,
) -> Response {
    let launch = body.launch.or_else(|| {
        let snap = state.world.snapshot();
        if snap.focused.is_empty() {
            None
        } else {
            Some(snap.focused)
        }
    });
    let Some(launch) = launch else {
        return persist_error("failed_precondition.ksp.edit: no focused launch");
    };
    let part = body.part_id.unwrap_or_else(|| "tank-s".into());
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || world.add_tank(&launch, &part)).await {
        Ok(Ok(())) => Json(state.world.snapshot()).into_response(),
        Ok(Err(message)) => persist_error(&message),
        Err(error) => join_error(&error.to_string()),
    }
}

#[derive(Deserialize)]
struct PromoteBody {
    launch: Option<String>,
    strategy: Option<String>,
}

async fn api_promote(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PromoteBody>,
) -> Response {
    let launch = body.launch.or_else(|| {
        let snap = state.world.snapshot();
        if snap.focused.is_empty() {
            None
        } else {
            Some(snap.focused)
        }
    });
    let Some(launch) = launch else {
        return persist_error("failed_precondition.ksp.promote: no focused launch");
    };
    let strategy = body.strategy.unwrap_or_else(|| "strict".into());
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || world.promote_this_design(&launch, &strategy)).await {
        Ok(Ok(_)) => Json(state.world.snapshot()).into_response(),
        Ok(Err(message)) => persist_error(&message),
        Err(error) => join_error(&error.to_string()),
    }
}

#[derive(Deserialize)]
struct CompareBody {
    a: Option<String>,
    b: Option<String>,
}

async fn api_compare(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CompareBody>,
) -> Response {
    let snap = state.world.snapshot();
    let a = body.a.filter(|name| !name.is_empty()).or_else(|| {
        if snap.focused.is_empty() {
            None
        } else {
            Some(snap.focused.clone())
        }
    });
    let Some(a) = a else {
        return persist_error("failed_precondition.ksp.compare: no focused launch");
    };
    let b = body
        .b
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "vab".into());
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || world.compare(&a, &b)).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(message)) => persist_error(&message),
        Err(error) => join_error(&error.to_string()),
    }
}

#[derive(Deserialize)]
struct ArchiveBody {
    launch: Option<String>,
    keep_snapshot: Option<bool>,
}

async fn api_archive(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ArchiveBody>,
) -> Response {
    let launch = body.launch.filter(|name| !name.is_empty()).or_else(|| {
        let snap = state.world.snapshot();
        if snap.focused.is_empty() {
            None
        } else {
            Some(snap.focused)
        }
    });
    let Some(launch) = launch else {
        return persist_error("failed_precondition.ksp.archive: no focused launch");
    };
    let keep = body.keep_snapshot.unwrap_or(false);
    let world = state.world.clone();
    match tokio::task::spawn_blocking(move || world.archive(&launch, keep)).await {
        Ok(Ok(_)) => Json(state.world.snapshot()).into_response(),
        Ok(Err(message)) => persist_error(&message),
        Err(error) => join_error(&error.to_string()),
    }
}

fn vab_error(code: &str, message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

fn persist_error(message: &str) -> Response {
    let code = message
        .split_once(':')
        .map(|(code, _)| code)
        .unwrap_or("internal.ksp.persist");
    let status = if code.starts_with("not_found") {
        StatusCode::NOT_FOUND
    } else if code.starts_with("conflict") {
        StatusCode::CONFLICT
    } else if code.starts_with("failed_precondition") || code.starts_with("invalid_argument") {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (
        status,
        Json(serde_json::json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

fn join_error(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": { "code": "internal.ksp.join", "message": message } })),
    )
        .into_response()
}
