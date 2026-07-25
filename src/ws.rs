use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};

use crate::protocol::{self, Envelope, ImpactData, TYPE_IMPACT};
use crate::scoring;
use crate::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    // The broadcast channel only carries messages sent after subscribing,
    // so bootstrap every newly connected renderer directly with the
    // session/target info it needs (matters for a second live viewer, and
    // for anyone connecting mid-replay).
    if sender
        .send(Message::Text(state.session_start_envelope.to_line()))
        .await
        .is_err()
    {
        return;
    }
    if sender
        .send(Message::Text(state.target_spawn_envelope.to_line()))
        .await
        .is_err()
    {
        return;
    }

    let mut send_task = tokio::spawn(async move {
        while let Ok(env) = rx.recv().await {
            if sender.send(Message::Text(env.to_line())).await.is_err() {
                break;
            }
        }
    });

    let recv_state = state.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                if let Ok(env) = serde_json::from_str::<Envelope>(&text) {
                    if env.kind == TYPE_IMPACT {
                        handle_impact(&recv_state, env).await;
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
/// target was), then records and broadcasts the result.
async fn handle_impact(state: &Arc<AppState>, impact_env: Envelope) {
    let data: ImpactData = match serde_json::from_value(impact_env.data.clone()) {
        Ok(d) => d,
        Err(_) => return,
    };

    if let Some(recorder) = &state.recorder {
        let mut recorder = recorder.lock().expect("recorder mutex poisoned");
        let _ = recorder.append(&impact_env);
    }

    // Replay mode has no live sim to score against — it's a read-only
    // playback of a past session, so clicks during replay are a no-op.
    let Some(session_start) = state.session_start else {
        return;
    };

    let t_now = Instant::now().duration_since(session_start).as_secs_f64();
    let target_pos = state.sim.position_at(t_now);
    let outcome = scoring::evaluate((data.x_mm, data.y_mm), target_pos, state.sim.radius_mm);

    let result_env = protocol::result(
        t_now,
        "circle-1",
        &data.impact_id,
        outcome.hit,
        outcome.distance_mm,
        data.x_mm,
        data.y_mm,
    );

    if let Some(recorder) = &state.recorder {
        let mut recorder = recorder.lock().expect("recorder mutex poisoned");
        let _ = recorder.append(&result_env);
    }

    let _ = state.tx.send(result_env);
}
