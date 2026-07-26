use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};

use sdts_engine::protocol::{self, Envelope, ImpactData, TYPE_IMPACT};

use crate::{AppState, Session};

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Snapshot whichever session (live or replay) is active right now. A
    // mode switch that happens later doesn't affect an already-open socket;
    // the page closes and reopens its connection whenever it switches modes,
    // which is what picks up the new session.
    let session = state.current();
    ws.on_upgrade(move |socket| handle_socket(socket, session))
}

async fn handle_socket(socket: WebSocket, session: Arc<Session>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = session.tx.subscribe();

    // The broadcast channel only carries messages sent after subscribing,
    // so bootstrap the newly connected renderer directly with the
    // session/target info it needs.
    if sender
        .send(Message::Text(session.session_start_envelope.to_line()))
        .await
        .is_err()
    {
        return;
    }
    for envelope in &session.target_spawn_envelopes {
        if sender
            .send(Message::Text(envelope.to_line()))
            .await
            .is_err()
        {
            return;
        }
    }

    let mut send_task = tokio::spawn(async move {
        while let Ok(env) = rx.recv().await {
            if sender.send(Message::Text(env.to_line())).await.is_err() {
                break;
            }
        }
    });

    let recv_session = session.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                if let Ok(env) = serde_json::from_str::<Envelope>(&text) {
                    if env.kind == TYPE_IMPACT {
                        handle_impact(&recv_session, env).await;
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}

/// Scores an incoming impact against the Core's authoritative target
/// position at receipt time (never the client's own idea of where the
/// target was), then records and broadcasts the result. All scoring logic
/// lives in `sdts_engine::Engine::impact` so it's shared byte-for-byte with
/// the WASM browser build.
async fn handle_impact(session: &Arc<Session>, impact_env: Envelope) {
    let data: ImpactData = match serde_json::from_value(impact_env.data.clone()) {
        Ok(d) => d,
        Err(_) => return,
    };

    if let Some(recorder) = &session.file_recorder {
        let mut recorder = recorder.lock().expect("recorder mutex poisoned");
        let _ = recorder.append(&impact_env);
    }

    // Replay mode has no live sim to score against — it's a read-only
    // playback of a past session, so clicks during replay are a no-op.
    let Some(session_start) = session.session_start else {
        return;
    };
    let Some(engine) = &session.engine else {
        return;
    };

    let t_now = Instant::now().duration_since(session_start).as_secs_f64();
    let outcome = engine
        .lock()
        .expect("engine mutex poisoned")
        .impact(t_now, &data.impact_id, data.x_mm, data.y_mm);

    let result_env = protocol::result(
        outcome.time,
        &outcome.target_id,
        &outcome.impact_id,
        outcome.hit,
        outcome.distance_mm,
        outcome.x_mm,
        outcome.y_mm,
        outcome.hits_remaining,
    );

    if let Some(recorder) = &session.file_recorder {
        let mut recorder = recorder.lock().expect("recorder mutex poisoned");
        let _ = recorder.append(&result_env);
    }

    let _ = session.tx.send(result_env);
}
