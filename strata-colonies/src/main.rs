//! Twenty Game of Life colonies on twenty Strata branches.

mod findings;
mod life;
mod patterns;
mod store;
mod world;

use crate::store::PersistMode;
use crate::world::{OpenArgs, World};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, watch};

const INDEX: &str = include_str!("../static/index.html");
const CSS: &str = include_str!("../static/style.css");
const JS: &str = include_str!("../static/app.js");

#[derive(Parser, Debug)]
#[command(
    name = "colonies",
    about = "Twenty Game of Life colonies on Strata branches. One seed, nineteen one-cell lies.",
    after_help = "Open the printed URL. The plate is live; the database is the twenty branches behind it."
)]
struct Cli {
    /// Durable database directory.
    #[arg(long, default_value = "./colonies-db")]
    db: PathBuf,
    /// In-memory cache mode (non-durable).
    #[arg(long)]
    cache: bool,
    /// Bind address.
    #[arg(long, default_value = "127.0.0.1:7420")]
    bind: String,
    /// Grid width.
    #[arg(long, default_value_t = 64)]
    width: u32,
    /// Grid height.
    #[arg(long, default_value_t = 48)]
    height: u32,
    /// Genesis soup seed.
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Persist each live cell as its own KV key (write-amplifying).
    #[arg(long)]
    stress_cells: bool,
    /// Starting tick rate.
    #[arg(long, default_value_t = 8.0)]
    hz: f64,
}

struct AppState {
    world: Arc<World>,
    tx: broadcast::Sender<String>,
    shutdown: watch::Sender<bool>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let persist_mode = if cli.stress_cells {
        PersistMode::Cells
    } else {
        PersistMode::Blob
    };

    println!("opening Strata…");
    let world = World::open(OpenArgs {
        width: cli.width,
        height: cli.height,
        seed: cli.seed,
        persist_mode,
        cache: cli.cache,
        db_path: cli.db,
    })
    .unwrap_or_else(|error| {
        eprintln!("failed to open world: {error}");
        std::process::exit(1);
    });
    world.set_hz(cli.hz);
    let world = Arc::new(world);

    let (tx, _rx) = broadcast::channel::<String>(16);
    let (shutdown, _) = watch::channel(false);
    let state = Arc::new(AppState {
        world: world.clone(),
        tx: tx.clone(),
        shutdown: shutdown.clone(),
    });

    let ticker_world = world.clone();
    let ticker_tx = tx;
    let ticker = tokio::spawn(async move {
        loop {
            let ms = ticker_world.period_ms();
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            if !ticker_world.is_running() {
                continue;
            }
            let world = ticker_world.clone();
            match tokio::task::spawn_blocking(move || world.tick()).await {
                Ok(Ok(snapshot)) => {
                    if let Ok(text) = serde_json::to_string(&snapshot) {
                        let _ = ticker_tx.send(text);
                    }
                }
                Ok(Err(error)) => eprintln!("tick failed: {error}"),
                Err(error) => eprintln!("tick join failed: {error}"),
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
        .route("/api/step", post(api_step))
        .route("/api/reset", post(api_reset))
        .route("/api/speed", post(api_speed))
        .route("/api/perturb", post(api_perturb))
        .route("/api/rewind", post(api_rewind))
        .route("/api/compare", post(api_compare))
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
    println!("colonies on http://{addr}");
    println!(
        "  {}×{}  20 branches  {}",
        cli.width,
        cli.height,
        if cli.cache { "cache" } else { "durable" }
    );
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown.send_replace(true);
        })
        .await;
    world.set_running(false);
    ticker.abort();
    let _ = ticker.await;
    tokio::task::spawn_blocking(move || world.close())
        .await
        .expect("database close task")
        .expect("close database");
    result.expect("server");
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install termination handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .expect("install interrupt handler");
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
            HeaderValue::from_static("application/javascript; charset=utf-8"),
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
    let mut shutdown = state.shutdown.subscribe();
    if *shutdown.borrow() {
        return;
    }
    if let Ok(snapshot) = state.world.snapshot() {
        if let Ok(text) = serde_json::to_string(&snapshot) {
            let _ = socket.send(Message::text(text)).await;
        }
    }
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
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
    json_snapshot(&state.world)
}

async fn api_run(State(state): State<Arc<AppState>>) -> Response {
    state.world.set_running(true);
    publish_state(&state)
}

async fn api_pause(State(state): State<Arc<AppState>>) -> Response {
    state.world.set_running(false);
    publish_state(&state)
}

async fn api_step(State(state): State<Arc<AppState>>) -> Response {
    let world = state.world.clone();
    let tx = state.tx.clone();
    match tokio::task::spawn_blocking(move || world.tick()).await {
        Ok(Ok(snapshot)) => publish(&tx, &snapshot),
        Ok(Err(error)) => err(error),
        Err(error) => err(error.to_string()),
    }
}

async fn api_reset(State(state): State<Arc<AppState>>) -> Response {
    let world = state.world.clone();
    let tx = state.tx.clone();
    match tokio::task::spawn_blocking(move || world.reset()).await {
        Ok(Ok(snapshot)) => publish(&tx, &snapshot),
        Ok(Err(error)) => err(error),
        Err(error) => err(error.to_string()),
    }
}

#[derive(Deserialize)]
struct SpeedBody {
    hz: f64,
}

async fn api_speed(State(state): State<Arc<AppState>>, Json(body): Json<SpeedBody>) -> Response {
    state.world.set_hz(body.hz);
    publish_state(&state)
}

#[derive(Deserialize)]
struct PerturbBody {
    colony: String,
    x: u32,
    y: u32,
}

async fn api_perturb(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PerturbBody>,
) -> Response {
    let world = state.world.clone();
    let tx = state.tx.clone();
    match tokio::task::spawn_blocking(move || world.perturb(&body.colony, body.x, body.y)).await {
        Ok(Ok(snapshot)) => publish(&tx, &snapshot),
        Ok(Err(error)) => err(error),
        Err(error) => err(error.to_string()),
    }
}

#[derive(Deserialize)]
struct RewindBody {
    generation: u64,
}

async fn api_rewind(State(state): State<Arc<AppState>>, Json(body): Json<RewindBody>) -> Response {
    let world = state.world.clone();
    let tx = state.tx.clone();
    match tokio::task::spawn_blocking(move || world.rewind(body.generation)).await {
        Ok(Ok(snapshot)) => publish(&tx, &snapshot),
        Ok(Err(error)) => err(error),
        Err(error) => err(error.to_string()),
    }
}

#[derive(Deserialize)]
struct CompareBody {
    colony: String,
}

async fn api_compare(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CompareBody>,
) -> Response {
    let world = state.world.clone();
    let tx = state.tx.clone();
    match tokio::task::spawn_blocking(move || world.compare(&body.colony)).await {
        Ok(Ok(snapshot)) => publish(&tx, &snapshot),
        Ok(Err(error)) => err(error),
        Err(error) => err(error.to_string()),
    }
}

async fn api_audit(State(state): State<Arc<AppState>>) -> Response {
    let world = state.world.clone();
    let tx = state.tx.clone();
    match tokio::task::spawn_blocking(move || world.audit_chains()).await {
        Ok(Ok(snapshot)) => publish(&tx, &snapshot),
        Ok(Err(error)) => err(error),
        Err(error) => err(error.to_string()),
    }
}

fn json_snapshot(world: &World) -> Response {
    match world.snapshot() {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(error) => err(error),
    }
}

fn publish_state(state: &AppState) -> Response {
    match state.world.snapshot() {
        Ok(snapshot) => publish(&state.tx, &snapshot),
        Err(error) => err(error),
    }
}

fn publish(tx: &broadcast::Sender<String>, snapshot: &world::Snapshot) -> Response {
    if let Ok(text) = serde_json::to_string(snapshot) {
        let _ = tx.send(text);
    }
    Json(snapshot).into_response()
}

fn err(message: String) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": message })),
    )
        .into_response()
}
