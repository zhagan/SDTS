use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::scenario::{safe_scenario_name, Scenario, DEFAULT_SCENARIO, SCENARIOS_DIR};
use crate::AppState;

const RECORDINGS_DIR: &str = "recordings";

#[derive(Serialize)]
struct RecordingInfo {
    name: String,
    size: u64,
}

#[derive(Serialize)]
struct ScenarioInfo {
    file: String,
    name: String,
    description: String,
}

pub async fn list_scenarios() -> impl IntoResponse {
    let mut scenarios = Vec::new();
    if let Ok(entries) = std::fs::read_dir(SCENARIOS_DIR) {
        for entry in entries.flatten() {
            let Some(file) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if safe_scenario_name(&file).is_err() {
                continue;
            }
            if let Ok(scenario) = Scenario::load(&file) {
                scenarios.push(ScenarioInfo {
                    file,
                    name: scenario.name,
                    description: scenario.description,
                });
            }
        }
    }
    scenarios.sort_by(|a, b| a.name.cmp(&b.name));
    Json(scenarios)
}

/// Lists every recorded session available for replay, newest first
/// (session ids are timestamp-prefixed, so name order is chronological).
pub async fn list_recordings() -> impl IntoResponse {
    let mut recordings = Vec::new();
    if let Ok(entries) = std::fs::read_dir(RECORDINGS_DIR) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            recordings.push(RecordingInfo {
                name: name.to_string(),
                size,
            });
        }
    }
    recordings.sort_by(|a, b| b.name.cmp(&a.name));
    Json(recordings)
}

/// Deletes one recording after constraining the request to a bare `.jsonl`
/// filename inside the recordings directory.
pub async fn delete_recording(Path(file): Path<String>) -> impl IntoResponse {
    let Ok(name) = safe_recording_name(&file) else {
        return (
            StatusCode::BAD_REQUEST,
            "invalid recording file".to_string(),
        )
            .into_response();
    };
    let path = format!("{RECORDINGS_DIR}/{name}");
    if !std::path::Path::new(&path).is_file() {
        return (StatusCode::NOT_FOUND, "recording not found".to_string()).into_response();
    }

    match std::fs::remove_file(&path) {
        Ok(()) => Json(json!({ "ok": true, "file": name })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to delete recording: {err}"),
        )
            .into_response(),
    }
}

/// Starts a brand new live recording session, tearing down whatever
/// session (live or replay) was previously running.
#[derive(Deserialize)]
pub struct RecordRequest {
    #[serde(default = "default_scenario")]
    scenario: String,
}

fn default_scenario() -> String {
    DEFAULT_SCENARIO.to_string()
}

pub async fn start_record(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RecordRequest>,
) -> impl IntoResponse {
    match crate::spawn_live(&state, &req.scenario) {
        Ok(session_id) => Json(json!({ "ok": true, "session_id": session_id })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to start recording: {err}"),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct ReplayRequest {
    file: String,
    #[serde(default = "default_speed")]
    speed: f64,
}

fn default_speed() -> f64 {
    1.0
}

/// Starts replaying a previously recorded session, tearing down whatever
/// session was previously running. `file` must be a bare filename (no path
/// separators) naming a `.jsonl` recording that actually exists in
/// `recordings/` — this is the only user-supplied path in the app, so it's
/// checked carefully to rule out escaping the recordings directory.
pub async fn start_replay(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ReplayRequest>,
) -> impl IntoResponse {
    let Ok(name) = safe_recording_name(&req.file) else {
        return (
            StatusCode::BAD_REQUEST,
            "invalid recording file".to_string(),
        )
            .into_response();
    };

    let path = format!("{RECORDINGS_DIR}/{name}");
    if !std::path::Path::new(&path).is_file() {
        return (StatusCode::NOT_FOUND, "recording not found".to_string()).into_response();
    }

    match crate::spawn_replay(&state, &path, req.speed.max(0.01)) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to start replay: {err}"),
        )
            .into_response(),
    }
}

fn safe_recording_name(name: &str) -> Result<&str, ()> {
    let path = std::path::Path::new(name);
    if name.is_empty()
        || path.file_name().and_then(|value| value.to_str()) != Some(name)
        || !name.ends_with(".jsonl")
    {
        return Err(());
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::safe_recording_name;

    #[test]
    fn recording_names_cannot_escape_the_recordings_directory() {
        assert_eq!(safe_recording_name("session.jsonl"), Ok("session.jsonl"));
        assert!(safe_recording_name("../session.jsonl").is_err());
        assert!(safe_recording_name("subdir/session.jsonl").is_err());
        assert!(safe_recording_name("session.json").is_err());
    }
}
