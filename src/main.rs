mod api;
mod protocol;
mod recorder;
mod scenario;
mod scoring;
mod ws;

use std::env;
use std::io;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use axum::routing::{delete, get, post};
use axum::Router;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tower_http::services::ServeDir;

use protocol::Envelope;
use recorder::Recorder;
use scenario::{Scenario, DEFAULT_SCENARIO};

/// A single point-in-time session: either a live recording session or the
/// playback of a past recording. In replay mode `recorder` and
/// `session_start` are `None` — replay is a read-only playback of a past
/// session, not a new one being recorded.
pub struct Session {
    pub session_start_envelope: Envelope,
    pub target_spawn_envelopes: Vec<Envelope>,
    pub tx: broadcast::Sender<Envelope>,
    pub recorder: Option<Mutex<Recorder>>,
    pub scenario: Option<Scenario>,
    pub session_start: Option<Instant>,
}

/// Shared state for the running SDTS Core. `session` holds whichever
/// session (live or replay) is currently active; the page can swap it out
/// at runtime via the `/api/session/*` endpoints instead of restarting the
/// process. `task` is the background driver (ticker or replay loop) for the
/// current session, kept around only so it can be aborted on switch.
pub struct AppState {
    session: RwLock<Option<Arc<Session>>>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl AppState {
    pub fn current(&self) -> Arc<Session> {
        self.session
            .read()
            .expect("session lock poisoned")
            .clone()
            .expect("session initialized before first use")
    }

    fn replace(&self, session: Arc<Session>, task: JoinHandle<()>) {
        if let Some(old_task) = self.task.lock().expect("task lock poisoned").replace(task) {
            old_task.abort();
        }
        *self.session.write().expect("session lock poisoned") = Some(session);
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    let state = Arc::new(AppState {
        session: RwLock::new(None),
        task: Mutex::new(None),
    });

    if args.len() >= 3 && args[1] == "replay" {
        let speed = args
            .get(3)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(1.0);
        spawn_replay(&state, &args[2], speed).expect("failed to load recording for replay");
    } else {
        let scenario_name = args.get(1).map(String::as_str).unwrap_or(DEFAULT_SCENARIO);
        spawn_live(&state, scenario_name).expect("failed to start live session");
    }

    let app = Router::new()
        .route("/ws", get(ws::ws_handler))
        .route("/api/recordings", get(api::list_recordings))
        .route("/api/recordings/:file", delete(api::delete_recording))
        .route("/api/scenarios", get(api::list_scenarios))
        .route("/api/session/record", post(api::start_record))
        .route("/api/session/replay", post(api::start_replay))
        .fallback_service(ServeDir::new("static"))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("failed to bind 127.0.0.1:3000");
    println!("OpenSDTS core listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await.expect("server error");
}

/// Starts a brand new live recording session, replacing whatever session
/// (live or replay) was previously active. Returns the new session id.
pub fn spawn_live(state: &Arc<AppState>, scenario_name: &str) -> io::Result<String> {
    let session_id = recorder::new_session_id();
    let scenario = Scenario::load(scenario_name)?;
    let mut rec = Recorder::new(&session_id)?;

    let session_start_envelope = protocol::session_start(
        0.0,
        &session_id,
        scenario.arena.width_mm,
        scenario.arena.height_mm,
        scenario_name,
    );
    let target_spawn_envelopes: Vec<_> = scenario
        .targets
        .iter()
        .map(|target| {
            protocol::target_spawn(
                0.0,
                &target.id,
                &target.display.shape,
                target.display.radius_mm,
                &target.display.fill,
                &target.display.stroke,
            )
        })
        .collect();
    rec.append(&session_start_envelope)?;
    for envelope in &target_spawn_envelopes {
        rec.append(envelope)?;
    }

    let (tx, _rx) = broadcast::channel(1024);
    let session = Arc::new(Session {
        session_start_envelope,
        target_spawn_envelopes,
        tx: tx.clone(),
        recorder: Some(Mutex::new(rec)),
        scenario: Some(scenario),
        session_start: Some(Instant::now()),
    });

    // The ticker captures its own `Arc<Session>` rather than reading back
    // through `AppState`, so an old ticker left running past a mode switch
    // (before its abort lands) can never write into the new session.
    let ticker_session = session.clone();
    let task = tokio::spawn(async move {
        let start = ticker_session
            .session_start
            .expect("live session always has a start time");
        let mut ticker = tokio::time::interval(Duration::from_millis(16));
        loop {
            ticker.tick().await;
            let t = Instant::now().duration_since(start).as_secs_f64();
            let states = ticker_session
                .scenario
                .as_ref()
                .expect("live session always has a scenario")
                .states_at(t);
            for target in states {
                let env = protocol::target_update(
                    t,
                    &target.id,
                    target.x_mm,
                    target.y_mm,
                    target.visible,
                );
                if let Some(recorder) = &ticker_session.recorder {
                    let mut recorder = recorder.lock().expect("recorder mutex poisoned");
                    let _ = recorder.append(&env);
                }
                let _ = ticker_session.tx.send(env);
            }
        }
    });

    println!(
        "Recording scenario '{}' as session {session_id} to recordings/{session_id}.jsonl",
        ticker_session_name(&session)
    );
    state.replace(session, task);
    Ok(session_id)
}

/// Starts replaying `path`, replacing whatever session was previously
/// active. `speed` scales playback rate: `1.0` is real-time, `0.5` is half
/// speed (twice as slow), `2.0` is double speed. Only the wall-clock gap
/// between messages is scaled — recorded `time` values are untouched, so
/// the on-screen clock still reads the original session time, just paced
/// out more slowly.
pub fn spawn_replay(state: &Arc<AppState>, path: &str, speed: f64) -> io::Result<()> {
    let contents = std::fs::read_to_string(path)?;
    let recorded: Vec<Envelope> = contents
        .lines()
        .filter_map(|line| serde_json::from_str::<Envelope>(line).ok())
        .collect();

    let session_start_envelope = recorded
        .iter()
        .find(|e| e.kind == protocol::TYPE_SESSION_START)
        .cloned()
        .unwrap_or_else(|| protocol::session_start(0.0, "replay", 1600.0, 1000.0, "replay"));
    let target_spawn_envelopes: Vec<Envelope> = recorded
        .iter()
        .filter(|e| e.kind == protocol::TYPE_TARGET_SPAWN)
        .cloned()
        .collect();

    let playback: Vec<Envelope> = recorded
        .into_iter()
        .filter(|e| e.kind == protocol::TYPE_TARGET_UPDATE || e.kind == protocol::TYPE_RESULT)
        .collect();

    let (tx, _rx) = broadcast::channel(1024);
    let session = Arc::new(Session {
        session_start_envelope,
        target_spawn_envelopes,
        tx: tx.clone(),
        recorder: None,
        scenario: None,
        session_start: None,
    });

    println!(
        "Replaying {} messages from {path} at {speed}x speed (loops every playthrough)",
        playback.len()
    );

    let task = tokio::spawn(async move {
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

    state.replace(session, task);
    Ok(())
}

fn ticker_session_name(session: &Session) -> &str {
    session
        .scenario
        .as_ref()
        .map(|scenario| scenario.name.as_str())
        .unwrap_or("replay")
}
