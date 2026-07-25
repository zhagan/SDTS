mod protocol;
mod recorder;
mod scoring;
mod sim;
mod ws;

use std::env;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::routing::get;
use axum::Router;
use tokio::sync::broadcast;
use tower_http::services::ServeDir;

use protocol::Envelope;
use recorder::Recorder;
use sim::Simulation;

/// Shared state for the running SDTS Core: the sim, the recorder, and the
/// broadcast channel that fans messages out to every connected renderer.
/// In replay mode `recorder` and `session_start` are `None` — replay is a
/// read-only playback of a past session, not a new one being recorded.
pub struct AppState {
    pub session_start_envelope: Envelope,
    pub target_spawn_envelope: Envelope,
    pub tx: broadcast::Sender<Envelope>,
    pub recorder: Option<Mutex<Recorder>>,
    pub sim: Simulation,
    pub session_start: Option<Instant>,
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    let state = if args.len() >= 3 && args[1] == "replay" {
        let speed = args
            .get(3)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(1.0);
        build_replay_state(&args[2], speed).expect("failed to load recording for replay")
    } else {
        build_live_state().expect("failed to start live session")
    };

    let app = Router::new()
        .route("/ws", get(ws::ws_handler))
        .fallback_service(ServeDir::new("static"))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind 127.0.0.1:3000");
    println!("OpenSDTS core listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await.expect("server error");
}

fn build_live_state() -> std::io::Result<Arc<AppState>> {
    let session_id = recorder::new_session_id();
    let sim = Simulation::new();
    let mut rec = Recorder::new(&session_id)?;

    let session_start_envelope =
        protocol::session_start(0.0, &session_id, sim.arena_width_mm, sim.arena_height_mm);
    let target_spawn_envelope = protocol::target_spawn(0.0, "circle-1", sim.radius_mm);
    rec.append(&session_start_envelope)?;
    rec.append(&target_spawn_envelope)?;

    let (tx, _rx) = broadcast::channel(1024);
    let state = Arc::new(AppState {
        session_start_envelope,
        target_spawn_envelope,
        tx: tx.clone(),
        recorder: Some(Mutex::new(rec)),
        sim,
        session_start: Some(Instant::now()),
    });

    println!("Recording session {session_id} to recordings/{session_id}.jsonl");

    let tick_state = state.clone();
    tokio::spawn(async move {
        let start = tick_state
            .session_start
            .expect("live session always has a start time");
        let mut ticker = tokio::time::interval(Duration::from_millis(16));
        loop {
            ticker.tick().await;
            let t = Instant::now().duration_since(start).as_secs_f64();
            let (x, y) = tick_state.sim.position_at(t);
            let env = protocol::target_update(t, "circle-1", x, y);
            if let Some(recorder) = &tick_state.recorder {
                let mut recorder = recorder.lock().expect("recorder mutex poisoned");
                let _ = recorder.append(&env);
            }
            let _ = tick_state.tx.send(env);
        }
    });

    Ok(state)
}

/// `speed` scales playback rate: `1.0` is real-time, `0.5` is half speed
/// (twice as slow), `2.0` is double speed. Only the wall-clock gap between
/// messages is scaled — recorded `time` values are untouched, so the
/// on-screen clock still reads the original session time, just paced out
/// more slowly.
fn build_replay_state(path: &str, speed: f64) -> std::io::Result<Arc<AppState>> {
    let contents = std::fs::read_to_string(path)?;
    let recorded: Vec<Envelope> = contents
        .lines()
        .filter_map(|line| serde_json::from_str::<Envelope>(line).ok())
        .collect();

    let sim = Simulation::new();
    let session_start_envelope = recorded
        .iter()
        .find(|e| e.kind == protocol::TYPE_SESSION_START)
        .cloned()
        .unwrap_or_else(|| {
            protocol::session_start(0.0, "replay", sim.arena_width_mm, sim.arena_height_mm)
        });
    let target_spawn_envelope = recorded
        .iter()
        .find(|e| e.kind == protocol::TYPE_TARGET_SPAWN)
        .cloned()
        .unwrap_or_else(|| protocol::target_spawn(0.0, "circle-1", sim.radius_mm));

    let playback: Vec<Envelope> = recorded
        .into_iter()
        .filter(|e| e.kind == protocol::TYPE_TARGET_UPDATE || e.kind == protocol::TYPE_RESULT)
        .collect();

    let (tx, _rx) = broadcast::channel(1024);
    let state = Arc::new(AppState {
        session_start_envelope,
        target_spawn_envelope,
        tx: tx.clone(),
        recorder: None,
        sim,
        session_start: None,
    });

    println!(
        "Replaying {} messages from {path} at {speed}x speed (loops every playthrough)",
        playback.len()
    );

    tokio::spawn(async move {
        loop {
            let mut prev_time = 0.0f64;
            for env in &playback {
                let gap = (env.time - prev_time).max(0.0) / speed;
                if gap > 0.0 {
                    tokio::time::sleep(Duration::from_secs_f64(gap)).await;
                }
                prev_time = env.time;
                let _ = tx.send(env.clone());
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    Ok(state)
}
